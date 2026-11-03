//! Halo CE missions on this runtime: Halo CE's scripts (`blam_h1::script`
//! reads them into the same [`ScriptData`]) begun part-way, the way Halo 3's
//! `ins_*` insertion scripts begin a mission, and The Maw's Warthog run
//! ([`THE_MAW`]).

use std::sync::Arc;

use blam_geometry::Scene;
use blam_script::ScriptData;

use crate::objects::Objects;
use crate::vm::{Config, Runtime};

/// A Halo CE mission of ours: which part of the map, where the player
/// starts, and how the map's scripts begin, as Halo 3's `ins_*` insertion
/// scripts begin a mission part-way.
pub struct Insertion {
    /// The structure BSPs decoded (scenario indices); the first holds the
    /// start.
    pub bsps: &'static [u32],
    /// The named placement the player starts at, facing its way (on the
    /// dirt bike; `--foot` on foot).
    pub start_at: &'static str,
    /// Startup scripts that do not run (the mission's own start).
    pub stop: &'static [&'static str],
    /// Static scripts run to their end at the start, before anything else
    /// (the mission's own start runs its settings first).
    pub console: &'static [&'static str],
    /// Dormant scripts woken at the start.
    pub wake: &'static [&'static str],
    /// Trigger volumes the player counts as having passed through.
    pub passed: &'static [&'static str],
    /// Calls of these scripts win the mission / lose it
    /// (`halo3_script::Config::won_at`, `lost_at`).
    pub won_at: &'static [&'static str],
    pub lost_at: &'static [&'static str],
}

/// Halo CE's last mission, The Maw (`d40`), from where the Warthogs wait at
/// the top of the lift (the walk there is left out). The map's own scripts
/// run from there: the countdown `timer_begin` starts (6:00; 5:00 on
/// Legendary by `diff_control`), the run's triggers fire, the countdown
/// stops for the Pelican scene at `enc6_6_1` as in the game, and
/// `test_for_endgame` wins it at `grand_finale` by the Longsword (its
/// `cinematic_finale`), or the countdown running out loses it
/// (`cinematic_time_up`). Only the run's BSPs, `d40f`, `d40g` and `d40h`
/// (5, 6, 7), switched by the map's own BSP switch volumes.
pub const THE_MAW: Insertion = Insertion {
    bsps: &[5, 6, 7],
    // The first Warthog, in its bay: it leaves forwards up the bay's ramp,
    // as the bike does; the run turns right at the wall ahead. (Facing the
    // run's first trigger instead runs the bike into the bay's low wheel
    // clamps.)
    start_at: "trench_jeep1",
    // The mission's own start (intro, the first sections) stays out of the
    // way; its difficulty settings run as it would have run them. The
    // earlier sections' continuous spawners, which those sections' own
    // scripts put to sleep (`sleep -1 enc1_4_manager`), stop too: with
    // the AI behind them they would refill encounters behind the start.
    stop: &[
        "mission",
        "enc1_4_manager",
        "enc3_5_manager",
        "enc3_6_manager",
        "enc4_2_manager",
        "enc5_1_manager",
        "enc5_1_spawner",
        "enc5_1_spawner_sents",
        "enc5_1_explosions",
    ],
    console: &["diff_control"],
    // `section6` waits for the top of the lift, which the run starts just
    // past, and `enc6_0_dialog` for the walk to the Warthogs (the run's
    // music and objective); `timer_begin` is woken on the lift ride.
    wake: &["section6", "section7", "timer_begin"],
    passed: &["section6", "enc6_0_dialog"],
    won_at: &["cinematic_finale"],
    lost_at: &["cinematic_time_up", "cutscene_lose"],
};

/// The map's scripts on Halo 3's runtime, begun as `ins` says: its own
/// start stopped, the difficulty scripts run, the mission's scripts woken,
/// the volumes behind the start passed. `switch_bsp` reaches the zone set
/// of each decoded BSP. Objects are not given to it yet: object calls do
/// nothing.
pub fn runtime(data: ScriptData, scene: &Scene, ins: &Insertion, difficulty: i16) -> Runtime {
    let mut bsp_zone_sets = vec![None; 32];
    for (z, set) in scene.zone_sets.iter().enumerate() {
        if set.bsps.count_ones() == 1 {
            bsp_zone_sets[set.bsps.trailing_zeros() as usize] = Some(z);
        }
    }
    let names = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let config = Config {
        difficulty,
        bsp_zone_sets,
        won_at: names(ins.won_at),
        lost_at: names(ins.lost_at),
        ..Default::default()
    };
    let mut rt = Runtime::new(
        Arc::new(data),
        Objects::default(),
        scene.trigger_volumes.clone(),
        Vec::new(),
        config,
    );
    for s in ins.stop {
        rt.stop(s);
    }
    for s in ins.console {
        rt.run_now(s);
    }
    for s in ins.wake {
        rt.wake(s);
    }
    rt.pass_volumes(&names(ins.passed));
    rt
}
