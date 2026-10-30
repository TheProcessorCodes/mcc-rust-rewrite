//! The Warthog run's islands move by their own animations: 120_halo's
//! scripts run headless (`ins_trench_run`), the player is walked into the
//! island's triggers, and `100_island01` must tilt (`tilt_up`) while it
//! stays a floor, then fall (`other_fall`) and lose its collision only once
//! it has really dropped. Skips quietly when the map is not installed.

use std::path::PathBuf;
use std::sync::Arc;

use glam::Vec3;
use halo3_anim::MachinePoser;
use halo3_script::vm::{Config, WorldApi};
use halo3_script::{DevicePoser, NoAi, setup};

struct World {
    player: Vec3,
    zone_set: usize,
}

impl WorldApi for World {
    fn player_position(&self) -> Option<Vec3> {
        Some(self.player)
    }
    fn teleport_player(&mut self, position: Vec3, _yaw: f32) {
        self.player = position;
    }
    fn current_zone_set(&self) -> Option<usize> {
        Some(self.zone_set)
    }
    fn switch_zone_set(&mut self, zone_set: usize) {
        self.zone_set = zone_set;
    }
    fn effect(&mut self, _tag: &str, _at: Vec3) {}
}

fn map() -> Option<PathBuf> {
    let dir = std::env::var_os("HALO3_MAPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps")
    });
    let p = dir.join("120_halo.map");
    p.exists().then_some(p)
}

#[test]
fn islands_tilt_then_fall() {
    let Some(path) = map() else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    let options = blam_geometry::Options {
        bsps: Some(vec![7, 8, 9]),
        collision: false,
        ..Default::default()
    };
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(&cache, &options, &mut scene).unwrap();
    let data = Arc::new(blam_script::ScriptData::read(&cache).unwrap());
    setup::mark_dynamic(&mut scene, &data);
    let mut rt = setup::runtime(&cache, &scene, Arc::clone(&data), Config { editor: true, ..Default::default() });
    let poser = MachinePoser::load(&cache, rt.objects.info.iter().filter(|o| o.scene.is_some()).filter_map(|o| o.tag));
    assert!(!poser.is_empty());

    // The animation itself: identity at the start, a long way down at the end.
    let island = rt.objects.by_name[data.object_index("100_island01").unwrap()].unwrap() as usize;
    let tag = rt.objects.info[island].tag.unwrap();
    let model = poser.model(tag).expect("island_test animations");
    let start = halo3_anim::devices::machine_pose(model, Some("other_fall"), 0.0, Some("tilt_up"), 0.0).unwrap();
    assert!(start.abs_diff_eq(glam::Mat4::IDENTITY, 1e-5), "{start:?}");
    let anchor = poser.anchor(tag).unwrap();
    let end = halo3_anim::devices::machine_pose(model, Some("other_fall"), 1.0, Some("tilt_up"), 1.0).unwrap();
    let fallen = anchor.z - end.transform_point3(anchor).z;
    assert!(fallen > 20.0, "other_fall drops the island {fallen} wu");

    assert!(rt.console("ins_trench_run"));
    let mut world = World {
        player: Vec3::new(-263.64, -433.77, -5.05),
        zone_set: 0,
    };
    let mut ai = NoAi;
    let mut step = |rt: &mut halo3_script::Runtime, world: &mut World, ticks: usize| {
        for _ in 0..ticks {
            rt.step(world, &mut ai, Some(&poser as &dyn DevicePoser));
        }
    };
    step(&mut rt, &mut world, 90);
    let centre = |name: &str| {
        let v = scene.trigger_volumes.iter().find(|v| v.name == name).unwrap();
        let left = v.up.cross(v.forward);
        v.position + v.forward * v.extents.x * 0.5 + left * v.extents.y * 0.5 + v.up * v.extents.z * 0.5
    };
    let base = rt.objects.info[island].base;
    eprintln!("100_island01 at {:?}, trig01 at {:?}", base.w_axis.truncate(), centre("100_island_trig01"));
    world.player = centre("100_island_trig01");
    // Two seconds into the four-second tilt.
    step(&mut rt, &mut world, 60);
    let s = &rt.objects.state[island];
    assert_eq!(s.device.track.as_deref(), Some("other_fall"));
    assert!(s.device.overlay > 0.2 && s.device.overlay < 0.9, "overlay {}", s.device.overlay);
    assert!(s.transform != base, "the island does not tilt");
    assert!(s.solid, "a tilting island must stay solid");
    // On to the trigger that drops it; a second into the ten-second fall it
    // is still a floor, by the end it has gone.
    world.player = centre("trench_run_100_trig07");
    step(&mut rt, &mut world, 30);
    let s = &rt.objects.state[island];
    assert!(s.device.position > 0.0 && s.device.position < 0.1, "position {}", s.device.position);
    assert!(s.solid, "solid just as it starts to fall");
    step(&mut rt, &mut world, 30 * 10);
    let s = &rt.objects.state[island];
    assert!(s.device.position > 0.99, "position {}", s.device.position);
    assert!(!s.solid, "a fallen island must not be solid");
    let dropped = base.transform_point3(anchor).z - s.transform.transform_point3(anchor).z;
    assert!(dropped > 20.0, "dropped {dropped} wu");
}
