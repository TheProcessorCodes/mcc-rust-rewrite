//! Halo CE's objects on The Maw's Warthog run (`h1:warthog_run`): the
//! dirt bike stops at an oxygen tank in the Longsword's tunnel (it rode
//! through before), and the map's own scripts reach the objects by name:
//! at `grand_finale`, `test_for_endgame`'s `object_destroy_all` and
//! `object_create_containing "bsp_8_1"` .. act on them and the run is still
//! won. Skips quietly when the map is not installed.
//!
//! One test only: the removed objects are process-wide
//! (`halo3_core::objects`).

use std::path::PathBuf;

use blam_geometry::{CollisionSource, Scene, TriggerVolume};
use glam::Vec3;
use halo3_bike::physics::{Bike, Controls};
use halo3_core::coords::{to_halo, to_render, to_render_dir};
use halo3_script::NoAi;
use halo3_script::h1::{THE_MAW, runtime_with_devices};
use halo3_script::h1_objects::H1Device;
use halo3_script::vm::{Runtime, WorldApi};

struct Player {
    at: Vec3,
    zone_set: usize,
}

impl WorldApi for Player {
    fn player_position(&self) -> Option<Vec3> {
        Some(self.at)
    }
    fn teleport_player(&mut self, position: Vec3, _yaw: f32) {
        self.at = position;
    }
    fn current_zone_set(&self) -> Option<usize> {
        Some(self.zone_set)
    }
    fn switch_zone_set(&mut self, zone_set: usize) {
        self.zone_set = zone_set;
    }
    fn effect(&mut self, _tag: &str, _at: Vec3) {}
}

fn d40() -> Option<PathBuf> {
    let p = game_paths::halo1_maps()?.join("d40.map");
    p.exists().then_some(p)
}

fn middle(v: &TriggerVolume) -> Vec3 {
    let left = v.up.cross(v.forward);
    v.position + (v.forward * v.extents.x + left * v.extents.y + v.up * v.extents.z) * 0.5
}

/// As `halo3/src/h1_objects.rs` hands them over.
fn devices(map: &blam_h1::Map, scene: &Scene) -> Vec<Option<H1Device>> {
    blam_h1::objects::devices(map, scene)
        .into_iter()
        .map(|d| {
            d.map(|d| H1Device {
                door: d.machine == Some(blam_h1::objects::MachineType::Door),
                automatic: d.automatic_door(),
                position_seconds: d.position_seconds,
                door_open_seconds: d.door_open_seconds,
                initially_open: d.initially_open(),
                initially_off: d.initially_off(),
                radius: d.radius,
                centre: d.centre,
            })
        })
        .collect()
}

/// Full throttle along +X (Halo) from `from` for four seconds: the
/// furthest x the bike's middle reached.
fn ride(world: &halo3_bike::physics::World, from: Vec3) -> f32 {
    let d = to_render_dir(Vec3::X);
    let mut bike = Bike::dropped(world, to_render(from + Vec3::Z), d.x.atan2(d.z));
    let mut furthest = f32::MIN;
    let controls = Controls {
        throttle: 1.0,
        ..Default::default()
    };
    for _ in 0..240 {
        bike.step(world, controls, 1.0 / 60.0);
        furthest = furthest.max(to_halo(bike.position).x);
    }
    furthest
}

#[test]
fn the_runs_objects_are_solid_and_scripted() {
    let Some(path) = d40() else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let options = blam_h1::Options {
        textures: false,
        bsps: Some(THE_MAW.bsps.to_vec()),
        ..Default::default()
    };
    let mut scene = blam_h1::decode(&map, &options).unwrap().scene;

    // ---- The bike and an oxygen tank in the tunnel to the hangar. ----
    // `bsp_8_75` stands on the run's line (y = -4) at x 1020.9, in BSP 7.
    let tank = scene.objects.iter().position(|o| o.name.as_deref() == Some("bsp_8_75")).unwrap();
    let at = scene.objects[tank].transform.w_axis.truncate();
    let start = Vec3::new(at.x - 12.0, at.y, at.z);
    let with = halo3_bike::build_world(&scene.collision, 1 << 7);
    let mut without = scene.collision.clone();
    without.triangles.retain(|t| matches!(t.source, CollisionSource::Bsp(_)));
    let without = halo3_bike::build_world(&without, 1 << 7);
    let (stopped, passed) = (ride(&with, start), ride(&without, start));
    eprintln!("tank bsp_8_75 at x {:.2}: the bike reaches x {stopped:.2} (without object collision {passed:.2})", at.x);
    assert!(passed > at.x + 2.0, "the floor past the tank must be ridable: {passed}");
    assert!(stopped < at.x, "the bike rode through the tank: {stopped}");

    // ---- The scripts find the objects. ----
    let data = blam_h1::script::read(&map).unwrap();
    let devices = devices(&map, &scene);
    let dynamic = halo3_script::h1_objects::mark_dynamic(&mut scene, &data, &devices);
    let mut rt: Runtime = runtime_with_devices(data, &scene, &THE_MAW, 1, &devices);
    let n = scene.objects.len();
    let (mut solid, mut posed) = (vec![true; n], vec![false; n]);
    let entry = |rt: &Runtime, name: &str| rt.objects.by_name[rt.data.object_index(name).unwrap()].unwrap() as usize;
    let exists = |rt: &Runtime, name: &str| rt.objects.state[entry(rt, name)].exists;
    eprintln!("{n} objects, {dynamic} drawn on their own");
    assert_eq!(entry(&rt, "bsp_8_75"), tank);
    assert!(exists(&rt, "bsp_8_22") && exists(&rt, "bsp_8_75"));
    // Placed only by the scripts (`+0x4` bit 0).
    assert!(!exists(&rt, "bsp_8_77") && !exists(&rt, "hangar_tank_1"));
    let mut player = Player {
        at: blam_h1::object_named(&map, THE_MAW.start_at).unwrap().position,
        zone_set: 0,
    };
    let mut step = |rt: &mut Runtime, player: &mut Player, ticks: usize| {
        for _ in 0..ticks {
            rt.step(player, &mut NoAi, None);
        }
    };
    step(&mut rt, &mut player, 30);
    halo3_script::publish_collision(&rt.objects, &mut solid, &mut posed);
    assert!(!halo3_core::objects::collision_removed(tank as u32));
    let absent = entry(&rt, "bsp_8_77");
    assert!(halo3_core::objects::collision_removed(absent as u32), "an object not created yet does not collide");

    let mut route: Vec<&TriggerVolume> = scene
        .trigger_volumes
        .iter()
        .filter(|v| (v.name.starts_with("enc6_") || v.name.starts_with("enc7_") || v.name == "section7") && middle(v).z > 20.0)
        .filter(|v| !v.name.contains("cov") && !v.name.contains("retreat"))
        .collect();
    route.sort_by(|a, b| middle(a).x.total_cmp(&middle(b).x));
    for v in route {
        player.at = middle(v);
        step(&mut rt, &mut player, 20);
        if v.name == "enc6_6_1" {
            step(&mut rt, &mut player, 640 + 180 + 30);
        }
    }
    assert!(exists(&rt, "bsp_8_75"), "the tanks stay until the hangar");
    player.at = middle(scene.trigger_volumes.iter().find(|v| v.name == "grand_finale").unwrap());
    step(&mut rt, &mut player, 60);
    assert_eq!(rt.mission_over.as_deref(), Some("cinematic_finale"));
    // `object_destroy_all`, then `object_create_containing` "bsp_8_1",
    // "bsp_8_5", "bsp_8_7", "bsp_8_8".
    assert!(!exists(&rt, "bsp_8_22") && !exists(&rt, "bsp_8_75") && !exists(&rt, "fighter_hangar"));
    for name in ["bsp_8_1", "bsp_8_13", "bsp_8_5", "bsp_8_57", "bsp_8_77", "bsp_8_8", "bsp_8_86"] {
        assert!(exists(&rt, name), "{name}");
    }
    halo3_script::publish_collision(&rt.objects, &mut solid, &mut posed);
    assert!(halo3_core::objects::collision_removed(tank as u32));
    assert!(!halo3_core::objects::collision_removed(absent as u32));
}
