//! The Warthog run's music, as 120_halo's own scripts drive it: insert at
//! the run (`ins_trench_run`, `editor` on), drive the player through the
//! trigger volumes the music scripts wait for, and check the sound calls
//! come out in order: `120_music_15` starts on the run's first encounter
//! volume, goes to its alternate loop on leaving the second island, and
//! stops at the last stretch. Skips quietly when the map is not installed.

use std::path::PathBuf;
use std::sync::Arc;

use glam::Vec3;
use mcc_core::script_sound::SoundRequest;
use mcc_script::vm::{Config, WorldApi};
use mcc_script::{NoAi, setup};

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
    let dir = game_paths::halo3_maps()?;
    let p = dir.join("120_halo.map");
    p.exists().then_some(p)
}

/// The volumes, in driving order, that the run's music and the scripts
/// leading to it wait for (`blam scripts 120_halo --grep 120_music_15`,
/// `--grep navpoint_deactive`, `--grep 120_trench_run_timer_04`).
const ROUTE: [&str; 11] = [
    "120_08_trench_encounter_100",
    "100_game_save_trig",
    "100_island_trig02",
    "110_game_save_trig",
    "trench_run_110_island_trig01",
    "trench_run_110_trig09x",
    "110_game_save02_trig",
    "110_island02_trig_exit",
    "120_08_trench_encounter_120",
    "trench_run_120_trig12",
    "trench_run_120_trig12x",
];

#[test]
fn warthog_run_music_cues() {
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
    let config = Config {
        editor: true,
        ..Default::default()
    };
    let mut rt = setup::runtime(&cache, &scene, Arc::clone(&data), config);
    assert!(rt.console("ins_trench_run"));
    let mut world = World {
        player: Vec3::new(-263.64, -433.77, -5.05),
        zone_set: 0,
    };
    let mut ai = NoAi;
    // (tick, volume the player was in, request)
    let mut heard: Vec<(u64, &str, SoundRequest)> = Vec::new();
    let mut run = |rt: &mut mcc_script::Runtime, world: &mut World, ticks: u32, at: &'static str, heard: &mut Vec<_>| {
        for _ in 0..ticks {
            rt.step(world, &mut ai, None);
            for r in rt.sounds.drain(..) {
                heard.push((rt.tick, at, r));
            }
        }
    };
    run(&mut rt, &mut world, 90, "start", &mut heard);
    let centre = |name: &str| {
        let v = scene.trigger_volumes.iter().find(|v| v.name == name).unwrap_or_else(|| panic!("no volume {name}"));
        let left = v.up.cross(v.forward);
        v.position + v.forward * v.extents.x * 0.5 + left * v.extents.y * 0.5 + v.up * v.extents.z * 0.5
    };
    for volume in ROUTE {
        world.player = centre(volume);
        run(&mut rt, &mut world, 45, volume, &mut heard);
    }
    // Let the music thread notice the global and stop.
    run(&mut rt, &mut world, 60, "end", &mut heard);

    let run_tag = cache
        .tags
        .of_class("lsnd")
        .find(|t| t.name().ends_with("120_music_15"))
        .unwrap()
        .index;
    let music: Vec<(&str, String)> = heard
        .iter()
        .filter_map(|(_, at, r)| match r {
            SoundRequest::LoopStart { tag, at: place, scale, .. } if *tag == run_tag => {
                assert_eq!((*place, *scale), (None, 1.0));
                Some((*at, "start".to_string()))
            }
            SoundRequest::LoopAlternate { tag, on } if *tag == run_tag => Some((*at, format!("alternate {on}"))),
            SoundRequest::LoopStop { tag, immediately } if *tag == run_tag => {
                Some((*at, if *immediately { "stop now" } else { "stop" }.to_string()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        music,
        vec![
            ("120_08_trench_encounter_100", "start".to_string()),
            ("110_island02_trig_exit", "alternate true".to_string()),
            ("trench_run_120_trig12x", "stop".to_string()),
        ],
        "all sound calls: {heard:#?}"
    );
    // No other music piece is started on the run.
    let others = heard
        .iter()
        .filter(|(_, _, r)| matches!(r, SoundRequest::LoopStart { tag, .. } if *tag != run_tag))
        .count();
    assert_eq!(others, 0, "{heard:#?}");
}
