//! The sound The Maw's scripts ask for on the Warthog run (`h1:warthog_run`,
//! begun as `mcc_script::h1::THE_MAW` begins it): `d40_07` from the start
//! (the insertion passes `enc6_0_dialog`, whose script starts it), its
//! alternate at the Pelican scene, `d40_08` after the scene, `d40_09` near
//! the end, `d40_08` stopped at `grand_finale`; Cortana's and the pilot's
//! lines one after another, each sleeping as long as the line plays
//! (`sound_impulse_time`); none telling the rider to get back in the
//! Warthog. Skips quietly when the map is not installed.

use std::path::PathBuf;

use blam_geometry::{Scene, TriggerVolume};
use glam::Vec3;
use mcc_core::script_sound::SoundRequest;
use mcc_script::NoAi;
use mcc_script::h1::{THE_MAW, runtime};
use mcc_script::vm::{Runtime, WorldApi};

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
    // On the dirt bike, which stands in for the Warthog.
    fn player_in_vehicle(&self) -> bool {
        true
    }
}

fn d40() -> Option<PathBuf> {
    let p = game_paths::halo1_maps()?.join("d40.map");
    p.exists().then_some(p)
}

fn middle(v: &TriggerVolume) -> Vec3 {
    let left = v.up.cross(v.forward);
    v.position + (v.forward * v.extents.x + left * v.extents.y + v.up * v.extents.z) * 0.5
}

/// What was asked, by tick: "start d40_07", "alt d40_07 true", "line
/// d40_370_cortana", ...
fn take(rt: &mut Runtime, tick: u64, heard: &mut Vec<(u64, String)>) {
    let last = |p: &str| p.rsplit('\\').next().unwrap_or(p).to_string();
    for r in std::mem::take(&mut rt.sounds) {
        let what = match r {
            SoundRequest::LoopStart { name, .. } => format!("start {}", last(&name)),
            SoundRequest::LoopStop { tag, .. } => format!("stop #{tag}"),
            SoundRequest::LoopAlternate { tag, on } => format!("alt #{tag} {on}"),
            // Lines are heard everywhere; sound effects play at the object
            // they are started on (the Pelican's `burn_pel_out`), now the
            // scripts find Halo CE's objects.
            SoundRequest::Impulse { name, at, .. } if name.contains(r"\dialog\") => {
                assert!(at.is_none(), "{name} is placed");
                format!("line {}", last(&name))
            }
            SoundRequest::Impulse { name, .. } => format!("sfx {}", last(&name)),
            SoundRequest::ImpulseStop { tag } => format!("cut #{tag}"),
            SoundRequest::ClassGain { class, gain, .. } => format!("class {class} {gain}"),
        };
        heard.push((tick, what));
    }
}

#[test]
fn the_run_music_and_lines() {
    let Some(path) = d40() else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let options = blam_h1::Options {
        textures: false,
        objects: false,
        bsps: Some(THE_MAW.bsps.to_vec()),
        ..Default::default()
    };
    let scene: Scene = blam_h1::decode(&map, &options).unwrap().scene;
    let data = blam_h1::script::read(&map).unwrap();
    let mut rt = runtime(data, &scene, &THE_MAW, 1);
    rt.dialogue.seconds = blam_h1::sound::script_seconds(&map);
    let tag = |name: &str| map.tags.iter().position(|t| t.name == name).unwrap() as u32;
    let (d40_07, d40_08) = (tag(r"levels\d40\music\d40_07"), tag(r"levels\d40\music\d40_08"));
    let start = blam_h1::object_named(&map, THE_MAW.start_at).unwrap().position;
    let mut world = World { player: start, zone_set: 0 };
    let mut heard = Vec::new();
    let mut tick = 0u64;
    let mut run = |rt: &mut Runtime, world: &mut World, n: u64, heard: &mut Vec<(u64, String)>| {
        for _ in 0..n {
            rt.step(world, &mut NoAi, None);
            tick += 1;
            take(rt, tick, heard);
        }
    };
    run(&mut rt, &mut world, 30, &mut heard);
    // The run's music from the start.
    assert!(heard.iter().any(|(_, w)| w == "start d40_07"), "{heard:?}");

    let mut route: Vec<&TriggerVolume> = scene
        .trigger_volumes
        .iter()
        .filter(|v| (v.name.starts_with("enc6_") || v.name.starts_with("enc7_") || v.name == "section7") && middle(v).z > 20.0)
        .filter(|v| !v.name.contains("cov") && !v.name.contains("retreat"))
        .collect();
    route.sort_by(|a, b| middle(a).x.total_cmp(&middle(b).x));
    for v in route {
        world.player = middle(v);
        run(&mut rt, &mut world, 20, &mut heard);
        if v.name == "enc6_6_1" {
            // The Pelican scene plays out.
            run(&mut rt, &mut world, 640 + 180 + 30, &mut heard);
        }
    }
    world.player = middle(scene.trigger_volumes.iter().find(|v| v.name == "grand_finale").unwrap());
    run(&mut rt, &mut world, 60, &mut heard);
    for (t, w) in &heard {
        println!("{t:6} {w}");
    }
    let at = |what: &str| heard.iter().find(|(_, w)| w == what).map(|(t, _)| *t).unwrap_or_else(|| panic!("no {what}"));
    let alt = at(&format!("alt #{d40_07} true"));
    let stop_07 = at(&format!("stop #{d40_07}"));
    let start_08 = at("start d40_08");
    let start_09 = at("start d40_09");
    let stop_08 = at(&format!("stop #{d40_08}"));
    assert!(at("start d40_07") < alt && alt < stop_07 && stop_07 <= start_08 && start_08 < start_09 && start_09 < stop_08);
    // Cortana at the Pelican, the pilot, Cortana again after the scene.
    let lines: Vec<&str> = heard.iter().filter_map(|(_, w)| w.strip_prefix("line ")).collect();
    for l in ["d40_370_cortana", "d40_400_cortana", "d40_440_cortana", "d40_450_cortana"] {
        assert!(lines.contains(&l), "{l} not in {lines:?}");
    }
    // The bike counts as a Warthog: Cortana never tells him to get back in
    // one (`trench_jeep_test`'s d40_360).
    assert!(!lines.contains(&"d40_360_cortana"), "{lines:?}");
    // A line's script sleeps as long as it plays, less what the script
    // overlaps (`(- (sound_impulse_time snd) 30)`): `timer_begin` says
    // d40_310, waits, then says d40_330.
    let secs = rt.dialogue.seconds[&tag(r"sound\dialog\d40\d40_310_cortana")];
    let wait = (secs * 30.0).round() as i64 - 30;
    let gap = at("line d40_330_cortana") as i64 - at("line d40_310_cortana") as i64;
    println!("d40_310 {secs:.2} s: d40_330 {gap} ticks later (sleep {wait})");
    assert!(wait > 60 && (gap - wait).abs() <= 3, "gap {gap}, sleep {wait}");
}
