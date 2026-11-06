//! Runs Halo CE's The Maw (`d40`) scripts with no window, begun as the
//! `h1:warthog_run` mission begins them (`mcc_script::h1::THE_MAW`): the
//! countdown starts at the Warthogs, stops for the Pelican scene, the run
//! is won at `grand_finale` and lost when the countdown runs out. The
//! player is moved through the run's trigger volumes. Skips quietly when
//! the map is not installed.

use std::path::PathBuf;

use blam_geometry::{Scene, TriggerVolume};
use glam::Vec3;
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
}

fn d40() -> Option<PathBuf> {
    let p = game_paths::halo1_maps()?.join("d40.map");
    p.exists().then_some(p)
}

/// The run's BSPs and the scripts, begun as the mission begins them.
fn begin(difficulty: i16) -> Option<(Scene, Runtime, World)> {
    let map = blam_h1::Map::open(&d40()?).unwrap();
    let options = blam_h1::Options {
        textures: false,
        objects: false,
        bsps: Some(THE_MAW.bsps.to_vec()),
        ..Default::default()
    };
    let scene = blam_h1::decode(&map, &options).unwrap().scene;
    let data = blam_h1::script::read(&map).unwrap();
    let rt = runtime(data, &scene, &THE_MAW, difficulty);
    let start = blam_h1::object_named(&map, THE_MAW.start_at).unwrap().position;
    Some((scene, rt, World { player: start, zone_set: 0 }))
}

fn middle(v: &TriggerVolume) -> Vec3 {
    let left = v.up.cross(v.forward);
    v.position + (v.forward * v.extents.x + left * v.extents.y + v.up * v.extents.z) * 0.5
}

fn volume<'a>(scene: &'a Scene, name: &str) -> &'a TriggerVolume {
    scene.trigger_volumes.iter().find(|v| v.name == name).unwrap()
}

fn steps(rt: &mut Runtime, world: &mut World, n: usize) {
    for _ in 0..n {
        rt.step(world, &mut NoAi, None);
    }
}

#[test]
fn countdown_starts_at_the_warthogs() {
    let Some((_, mut rt, mut world)) = begin(1) else { return };
    steps(&mut rt, &mut world, 30);
    let t = rt.hud_timer;
    assert!(t.shown && !t.paused, "{t:?}");
    // 6:00 less the second just run (and what `timer_begin` took to start).
    let left = t.ticks.unwrap();
    assert!((6 * 60 * 30 - 30..6 * 60 * 30).contains(&left), "{left}");
    assert_eq!(t.warning, 60 * 30);
    // Legendary takes a minute off (`diff_control`).
    let Some((_, mut rt, mut world)) = begin(3) else { return };
    steps(&mut rt, &mut world, 30);
    let left = rt.hud_timer.ticks.unwrap();
    assert!((5 * 60 * 30 - 30..5 * 60 * 30).contains(&left), "{left}");
}

#[test]
fn the_run_is_won_at_the_longsword() {
    let Some((scene, mut rt, mut world)) = begin(1) else { return };
    steps(&mut rt, &mut world, 30);
    // Through the run's volumes in the order the player meets them: its
    // encounters and sections, east along the ship, then the Longsword.
    let mut route: Vec<&TriggerVolume> = scene
        .trigger_volumes
        .iter()
        .filter(|v| (v.name.starts_with("enc6_") || v.name.starts_with("enc7_") || v.name == "section7") && middle(v).z > 20.0)
        .filter(|v| !v.name.contains("cov") && !v.name.contains("retreat"))
        .collect();
    route.sort_by(|a, b| middle(a).x.total_cmp(&middle(b).x));
    let mut paused_at = None;
    for v in route {
        world.player = middle(v);
        steps(&mut rt, &mut world, 20);
        if v.name == "enc6_6_1" {
            // The Pelican scene: the countdown stops and hides.
            assert!(rt.hud_timer.paused && !rt.hud_timer.shown, "{:?}", rt.hud_timer);
            paused_at = rt.hud_timer.ticks;
            // `trench_scene_continue` once the scene has played (or the
            // player drives on to `enc6_7_1`), then three seconds more.
            steps(&mut rt, &mut world, 640 + 180 + 30);
            assert!(!rt.hud_timer.paused && rt.hud_timer.shown, "{:?}", rt.hud_timer);
        }
        assert!(rt.mission_over.is_none() && rt.mission_lost.is_none(), "ended at {}", v.name);
    }
    let paused_at = paused_at.expect("the route passes enc6_6_1");
    let left = rt.hud_timer.ticks.unwrap();
    assert!(left < paused_at && left > 0, "{left} after {paused_at}");
    world.player = middle(volume(&scene, "grand_finale"));
    steps(&mut rt, &mut world, 60);
    assert_eq!(rt.mission_over.as_deref(), Some("cinematic_finale"));
    assert_eq!(rt.mission_lost, None);
}

#[test]
fn the_run_is_lost_when_the_countdown_runs_out() {
    let Some((_, mut rt, mut world)) = begin(1) else { return };
    steps(&mut rt, &mut world, 6 * 60 * 30 + 60);
    assert_eq!(rt.hud_timer.ticks, Some(0));
    assert_eq!(rt.mission_lost.as_deref(), Some("cinematic_time_up"));
    assert_eq!(rt.mission_over, None);
}
