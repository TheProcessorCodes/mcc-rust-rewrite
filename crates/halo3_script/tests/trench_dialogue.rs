//! The Warthog run's dialogue, as 120_halo's own scripts play it: insert at
//! the run (`ins_trench_run`, with the hallway volume it starts past taken
//! as passed), drive the player through the run's volumes, and check that
//! Cortana's lines (`md_09_cortana_hints`, `md_cortana_countdown`) come out
//! as one-shots of their `snd!` tags at the right moments, and that the
//! scripts' `(sleep (ai_play_line_on_object none LINE))` waits the line's
//! real length in the language bank. Skips quietly when the map is not
//! installed.

use std::sync::Arc;

use glam::Vec3;
use halo3_core::script_sound::SoundRequest;
use halo3_script::vm::{Config, WorldApi};
use halo3_script::{NoAi, setup};

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

#[test]
fn warthog_run_dialogue() {
    let Some(path) = game_paths::halo3_map("120_halo") else { return };
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
    let ins = setup::insertion(&data, &scene, "trench_run").unwrap();
    assert_eq!(ins.passed, vec!["120_07_c_hall_end".to_string()]);
    let config = Config {
        editor: true,
        ..Default::default()
    };
    let mut rt = setup::runtime(&cache, &scene, Arc::clone(&data), config);
    rt.pass_volumes(&ins.passed);
    assert!(rt.console(&ins.script));

    // The lines and their lengths. 120mg_030 (Cortana, on reaching the run)
    // is 102,400 frames at 48 kHz in english.fsb: 2.13 s.
    let dialogue = rt.dialogue.clone();
    let line = |n: &str| dialogue.line(n).unwrap_or_else(|| panic!("no line {n}")).clone();
    let l030 = line("120mg_030");
    assert_eq!(l030.speaker, "cor");
    assert_eq!(l030.sound_name, r"sound\dialog\120_halo\mission\120mg_030_cor");
    let english = rt.dialogue.language == Some("english");
    if english {
        assert!((rt.dialogue.seconds_of(l030.sound) - 102_400.0 / 48_000.0).abs() < 1e-3);
    }
    assert!(rt.dialogue.lines.len() >= 200, "{} lines", rt.dialogue.lines.len());
    let ticks_of = |tag: u32| (dialogue.seconds_of(tag) as f64 * 30.0).round() as u64;

    let mut world = World {
        player: ins.start.unwrap().0,
        zone_set: 0,
    };
    let mut ai = NoAi;
    // (tick, line sound tag)
    let mut heard: Vec<(u64, u32, &'static str)> = Vec::new();
    let mut run = |rt: &mut halo3_script::Runtime, world: &mut World, ticks: u32, at: &'static str, heard: &mut Vec<_>| {
        for _ in 0..ticks {
            rt.step(world, &mut ai, None);
            for r in rt.sounds.drain(..) {
                if let SoundRequest::Impulse { tag, name, at: place, .. } = r
                    && name.contains(r"\dialog\")
                {
                    assert_eq!(place, None, "{name}: Cortana is in the player's ear");
                    heard.push((rt.tick, tag, at));
                }
            }
        }
    };
    let centre = |name: &str| {
        let v = scene.trigger_volumes.iter().find(|v| v.name == name).unwrap_or_else(|| panic!("no volume {name}"));
        let left = v.up.cross(v.forward);
        v.position + v.forward * v.extents.x * 0.5 + left * v.extents.y * 0.5 + v.up * v.extents.z * 0.5
    };

    // At the start: "There! Johnson's Warthog!" (the hallway is behind us).
    run(&mut rt, &mut world, 120, "start", &mut heard);
    // Onto the run: 120mg_030, then 30 s later "The Dawn is close! We can
    // make it!" and "As long as the ground doesn't fall out beneath us!".
    world.player = centre("120_08_trench_encounter_100");
    let len030 = ticks_of(l030.sound);
    run(&mut rt, &mut world, 60 + len030 as u32 + 900 + 15 + 300, "encounter_100", &mut heard);
    // The countdown, which ends the hints: "Charging sequence at 30
    // percent!", then "50 percent" on the second island.
    world.player = centre("trench_run_110_trig09");
    run(&mut rt, &mut world, 120, "110_trig09", &mut heard);
    world.player = centre("trench_run_110_island_trig02");
    run(&mut rt, &mut world, 120, "110_island_trig02", &mut heard);

    let names: Vec<(&str, String)> = heard
        .iter()
        .map(|(_, tag, at)| (*at, cache.tags.get(*tag).unwrap().name().rsplit('\\').next().unwrap().to_string()))
        .collect();
    let expect: Vec<(&str, String)> = [
        ("start", "120mg_010_cor"),
        ("encounter_100", "120mg_030_cor"),
        ("encounter_100", "120mg_040_cor"),
        ("encounter_100", "120mg_050_cor"),
        ("110_trig09", "120mg_100_cor"),
        ("110_island_trig02", "120mg_110_cor"),
    ]
    .iter()
    .map(|(a, n)| (*a, n.to_string()))
    .collect();
    assert_eq!(names, expect, "{heard:?}");

    // The waits: 120mg_040 follows 120mg_030 by its length, the script's
    // (sleep 900) and (sleep 15); 120mg_050 follows 120mg_040 by its
    // length. A tick or two of slack for the order threads run in.
    let tick = |n: &str| heard.iter().find(|(_, t, _)| *t == line(n).sound).unwrap().0;
    let gap = tick("120mg_040") - tick("120mg_030");
    assert!(gap.abs_diff(len030 + 915) <= 2, "040 came {gap} ticks after 030 ({len030} + 915 expected)");
    let len040 = ticks_of(line("120mg_040").sound);
    let gap = tick("120mg_050") - tick("120mg_040");
    assert!(gap.abs_diff(len040) <= 2, "050 came {gap} ticks after 040 ({len040} expected)");
}
