//! Runs 120_halo's own scripts with no window: insert at the Warthog run
//! the way the designers did (`ins_trench_run` at the console with
//! `editor` on), drive the player through the run's trigger volumes, and
//! check the floor plates drop. Skips quietly when the map is not
//! installed.

use std::path::PathBuf;
use std::sync::Arc;

use glam::Vec3;
use halo3_script::objects::Motion;
use halo3_script::vm::{Config, ThreadState, WorldApi};
use halo3_script::{NoAi, setup};

struct World {
    player: Vec3,
    zone_set: usize,
    teleports: Vec<Vec3>,
    effects: Vec<String>,
}

impl WorldApi for World {
    fn player_position(&self) -> Option<Vec3> {
        Some(self.player)
    }
    fn teleport_player(&mut self, position: Vec3, _yaw: f32) {
        self.player = position;
        self.teleports.push(position);
    }
    fn current_zone_set(&self) -> Option<usize> {
        Some(self.zone_set)
    }
    fn switch_zone_set(&mut self, zone_set: usize) {
        self.zone_set = zone_set;
    }
    fn effect(&mut self, tag: &str, _at: Vec3) {
        self.effects.push(tag.to_string());
    }
}

fn map() -> Option<PathBuf> {
    let dir = std::env::var_os("HALO3_MAPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps")
    });
    let p = dir.join("120_halo.map");
    p.exists().then_some(p)
}

#[test]
fn warthog_run_plates_fall() {
    let Some(path) = map() else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    // The Warthog run's BSPs only (100, 110, 120): objects are placed
    // whatever BSPs are decoded.
    let options = blam_geometry::Options {
        bsps: Some(vec![7, 8, 9]),
        collision: false,
        ..Default::default()
    };
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(&cache, &options, &mut scene).unwrap();
    let data = Arc::new(blam_script::ScriptData::read(&cache).unwrap());
    let dynamic = setup::mark_dynamic(&mut scene, &data);
    assert!(dynamic > 1000, "{dynamic} scripted objects");
    let config = Config {
        editor: true,
        ..Default::default()
    };
    let mut rt = setup::runtime(&cache, &scene, Arc::clone(&data), config);
    assert!(rt.console("ins_trench_run"));

    let mut world = World {
        player: Vec3::new(-263.64, -433.77, -5.05),
        zone_set: 0,
        teleports: Vec::new(),
        effects: Vec::new(),
    };
    let mut ai = NoAi;
    for _ in 0..90 {
        rt.step(&mut world, &mut ai, None);
    }
    // The insertion switched to the Warthog run's zone set and put the
    // player at its start flag.
    assert_eq!(world.zone_set, 4, "zone set");
    let start = data.cutscene_flags.iter().find(|f| f.name == "trench_run_player0").unwrap();
    assert_eq!(world.teleports.first().copied(), Some(Vec3::from(start.position)));
    assert_ne!(rt.script_state("120_08_trench_run"), Some(ThreadState::Dormant));
    assert_ne!(rt.script_state("100_trench_run_timer_01"), Some(ThreadState::Dormant));

    let centre = |name: &str| {
        let v = scene.trigger_volumes.iter().find(|v| v.name == name).unwrap();
        let left = v.up.cross(v.forward);
        let p = v.position + v.forward * v.extents.x * 0.5 + left * v.extents.y * 0.5 + v.up * v.extents.z * 0.5;
        assert!(v.contains(p));
        p
    };
    let plate = data.object_index("100_001f").unwrap();
    let e = rt.objects.by_name[plate].unwrap() as usize;
    // `object_management` drops the run's plate folders once the zone set
    // is in (to save the console's memory); the runtime keeps placed
    // objects, so the floor is there before the plate trigger too.
    assert!(rt.objects.state[e].exists && rt.objects.state[e].solid, "100_001f before the plate trigger");
    world.player = centre("trench_run_plate_100_trig");
    for _ in 0..30 {
        rt.step(&mut world, &mut ai, None);
    }
    assert!(rt.objects.state[e].exists && rt.objects.state[e].solid, "100_001f after the plate trigger");
    // Into the first timer's trigger.
    world.player = centre("trench_run_100_trig01");
    // `(sleep 90)` then the first row drops.
    for _ in 0..(30 * 6) {
        rt.step(&mut world, &mut ai, None);
    }
    let s = &rt.objects.state[e];
    assert!(!s.solid, "100_001f still solid");
    assert!(
        matches!(s.motion, Motion::Falling { .. }) || !s.exists,
        "100_001f neither falling nor gone"
    );
    assert!(rt.calls.get("object_damage_damage_section").copied().unwrap_or(0) >= 6);
    assert!(world.effects.iter().any(|e| e.ends_with("_fall")), "{:?}", world.effects);
    // A plate further on is still there.
    let later = rt.objects.by_name[data.object_index("120_022f").unwrap()].unwrap() as usize;
    assert!(rt.objects.state[later].solid);
}
