//! Halo CE's objects on The Maw's Warthog run (`h1:warthog_run`): the
//! dirt bike stops at an oxygen tank in the Longsword's tunnel (it rode
//! through before), and the map's own scripts reach the objects by name:
//! at `grand_finale`, `test_for_endgame`'s `object_destroy_all` and
//! `object_create_containing "bsp_8_1"` .. act on them and the run is still
//! won. Skips quietly when the map is not installed.
//!
//! One test only: the removed objects are process-wide
//! (`mcc_core::objects`).

use std::path::PathBuf;

use blam_geometry::{CollisionSource, Scene, TriggerVolume};
use glam::Vec3;
use mcc_bike::physics::{Bike, Controls};
use mcc_core::coords::{to_halo, to_render, to_render_dir};
use mcc_script::NoAi;
use mcc_script::h1::{THE_MAW, runtime_with_devices};
use mcc_script::h1_objects::H1Device;
use mcc_script::vm::{Runtime, WorldApi};

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

/// As `mcc-rust-rewrite/src/h1_objects.rs` hands them over.
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
fn ride(world: &mcc_bike::physics::World, from: Vec3) -> f32 {
    let d = to_render_dir(Vec3::X);
    let mut bike = Bike::dropped(world, to_render(from + Vec3::Z), d.x.atan2(d.z));
    // Settled first, as the bike's own tests do.
    for _ in 0..60 {
        bike.step(world, Controls::default(), 1.0 / 60.0);
    }
    let mut furthest = f32::MIN;
    let controls = Controls {
        throttle: 1.0,
        ..Default::default()
    };
    for k in 0..240 {
        bike.step(world, controls, 1.0 / 60.0);
        let p = to_halo(bike.position);
        furthest = furthest.max(p.x);
        if k % 60 == 59 {
            eprintln!("  {:.1} s: halo [{:.2}, {:.2}, {:.2}]", (k + 1) as f32 / 60.0, p.x, p.y, p.z);
        }
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
    let with = mcc_bike::build_world(&scene.collision, 1 << 7);
    // A run-up on the tank's own floor (the road drops about 4 wu before
    // it): the furthest point back, up to 12 wu, whose floor is the tank's.
    let floor_at = |x: f32| {
        let hit = with.raycast_far(to_render(Vec3::new(x, at.y, at.z + 3.0)), -Vec3::Y, 10.0 * 3.048)?;
        Some(to_halo(hit.point).z)
    };
    let back = (4..=24)
        .rev()
        .map(|k| k as f32 * 0.5)
        .find(|d| floor_at(at.x - d).is_some_and(|z| (z - at.z).abs() < 0.3))
        .expect("a floor before the tank");
    let start = Vec3::new(at.x - back, at.y, at.z);
    let mut without = scene.collision.clone();
    without.triangles.retain(|t| matches!(t.source, CollisionSource::Bsp(_)));
    let without = mcc_bike::build_world(&without, 1 << 7);
    let (stopped, passed) = (ride(&with, start), ride(&without, start));
    eprintln!("tank bsp_8_75 at x {:.2}: the bike reaches x {stopped:.2} (without object collision {passed:.2})", at.x);
    assert!(passed > at.x + 2.0, "the floor past the tank must be ridable: {passed}");
    assert!(stopped < at.x, "the bike rode through the tank: {stopped}");

    // ---- The scripts find the objects. ----
    let data = blam_h1::script::read(&map).unwrap();
    let devices = devices(&map, &scene);
    let dynamic = mcc_script::h1_objects::mark_dynamic(&mut scene, &data, &devices);
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
    let step = |rt: &mut Runtime, player: &mut Player, ticks: usize| {
        for _ in 0..ticks {
            rt.step(player, &mut NoAi, None);
        }
    };
    step(&mut rt, &mut player, 30);
    mcc_script::publish_collision(&rt.objects, &mut solid, &mut posed);
    assert!(!mcc_core::objects::collision_removed(tank as u32));
    let absent = entry(&rt, "bsp_8_77");
    assert!(mcc_core::objects::collision_removed(absent as u32), "an object not created yet does not collide");

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
    // Until the win, where the game stops the scripts (`mcc_script`'s
    // tick; the outro would bring the hangar's scene objects back).
    for _ in 0..60 {
        if rt.mission_over.is_some() {
            break;
        }
        step(&mut rt, &mut player, 1);
    }
    assert_eq!(rt.mission_over.as_deref(), Some("cinematic_finale"));
    // `object_destroy_all`, then `object_create_containing` "bsp_8_1",
    // "bsp_8_5", "bsp_8_7", "bsp_8_8". "Containing" is a substring match,
    // so "bsp_8_7" brings the tank (bsp_8_75) back too.
    assert!(!exists(&rt, "bsp_8_22") && !exists(&rt, "fighter_hangar"));
    for name in ["bsp_8_1", "bsp_8_13", "bsp_8_5", "bsp_8_57", "bsp_8_75", "bsp_8_77", "bsp_8_8", "bsp_8_86"] {
        assert!(exists(&rt, name), "{name}");
    }
    mcc_script::publish_collision(&rt.objects, &mut solid, &mut posed);
    let gone = entry(&rt, "bsp_8_22");
    assert!(mcc_core::objects::collision_removed(gone as u32), "a destroyed object does not collide");
    assert!(!mcc_core::objects::collision_removed(tank as u32));
    assert!(!mcc_core::objects::collision_removed(absent as u32));
}
