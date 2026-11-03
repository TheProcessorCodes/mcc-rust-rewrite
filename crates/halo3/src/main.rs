//! `halo3`: open a Halo 3 map and fly around it.
//!
//! ```text
//! halo3                  the map menu: pick a map, Start Game (`menu.rs`)
//! halo3 <map>            riverworld, or a path to a .map
//! halo3 warthog_run      a mission (`MISSIONS`): Halo 3's last mission
//!                        from the Warthog run on, on the dirt bike, timed
//! halo3 h1:warthog_run   Halo CE's Warthog run: The Maw from the Warthogs
//!                        at the top of the lift, against the countdown
//! halo3 --test-scene     a made-up room, for checking the camera
//!
//! halo3 <map> --shot <dir> [--view x,y,z,yaw,pitch]... [--size WxH]
//!                        no window: save PNGs and exit (see `shot.rs`)
//! halo3 <map> --shot <dir> --skate [--view ...]
//!                        the same, after two seconds of skating
//! halo3 <map> --shot <dir> --bike [--view ...]
//!                        the same, after two seconds on the dirt bike
//! halo3 <map> --shot <dir> --bench N [--view ...]
//!                        before each picture, time N frames and count what
//!                        is drawn (log lines start with `bench:`)
//!
//! halo3 <map> --zone N|NAME | --bsps 0,3,5 | --all-bsps
//!                        which structure BSPs to load first (default: the
//!                        zone set the player starts in; `world/zones.rs`)
//!
//! halo3 <map> --music NAME|N
//!                        start a music track (`audio/`; M plays the map's
//!                        showpiece, e.g. the Warthog run on 120_halo)
//!
//! `--foot` starts on foot at the start spawn (`foot/mod.rs`; F toggles).
//!
//! halo3 <map> --insertion NAME|N [--difficulty easy|normal|heroic|legendary]
//!                        run the map's own scripts (`halo3_script`): NAME
//!                        runs the insertion script `ins_NAME` as the
//!                        designers did at the console (120_halo:
//!                        `trench_run`, the Warthog run); N starts the
//!                        mission at insertion point N. `--scripts` runs
//!                        them from the mission's own start. 120_halo
//!                        also has `trench_end` (ours): the run's last
//!                        straight, before the jump into the frigate. A
//!                        named insertion starts you on the dirt bike
//!                        (`--foot`: on foot); when the scripts win the
//!                        mission a completion screen shows the run's
//!                        time (`finish.rs`).
//! halo3 <map> --anim-test flood=move_front,sentinel=aim,... [--anim-at x,y,z,yaw]
//!                        a row of the map's bipeds playing Halo 3's own
//!                        animations (`anim_test.rs`); with `--shot` for
//!                        pictures
//! `--ai nearby|zone|off|none`: how the campaign's squads are placed while
//! scripts do not place them (`halo3_ai`; default `nearby`: a squad appears
//! when you come within 70 m of it).
//!
//! In a map, J switches between flying and skating (see `skate.rs`), B
//! between flying and the dirt bike (`bike/mod.rs`), PageDown / PageUp step
//! through the zone sets, Home loads every BSP. Escape or Start pauses and
//! lists the controls (`pause.rs`).
//! ```
//!
//! The first launch asks for the player's own MCC and Skate 3 (`.iso` or
//! `default.xex`), both required, and MX vs ATV Reflex if they have it, and
//! saves the answers (`halo3_setup`); `--setup` asks again. `<map>` is looked up in that MCC's
//! `halo3\maps` (`%HALO3_MAPS%` overrides), Halo 2 maps (`coagulation`,
//! `lockout`, or `h2:NAME`) in its `halo2\h2_maps_win64_dx11`
//! (`%HALO2_MAPS%`; `halo2.rs`). Halo 3's Blackout, whose file is
//! `lockout.map`, is `blackout`.

mod anim_test;
mod countdown;
mod finish;
mod halo1;
mod halo1_effects;
mod halo1_ai;
mod halo1_anim;
mod halo2;
mod menu;
mod reach;
mod pause;
mod shot;
mod speedo;

use halo3_ai as ai;
use halo3_audio as audio;
use halo3_bike as bike;
use halo3_core::{falls, flag, fly_camera};
use halo3_foot as foot;
use halo3_rider as rider;
use halo3_skate as skate;
use halo3_swim as swim;
use halo3_textures as textures;
use halo3_weapons as weapons;
use halo3_world as world;

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, WindowMode};

/// Which game a map belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Game {
    Halo1,
    Halo2,
    Halo3,
    Reach,
}

/// A game's maps folder in the player's MCC, from the saved setup or the
/// dev overrides (`game_paths`); empty before setup.
pub(crate) fn maps_dir(game: Game) -> PathBuf {
    match game {
        Game::Halo3 => game_paths::halo3_maps(),
        Game::Halo2 => game_paths::halo2_maps(),
        Game::Halo1 => game_paths::halo1_maps(),
        Game::Reach => game_paths::reach_maps(),
    }
    .unwrap_or_default()
}

/// A path, or a bare name: a Halo 3 map in the player's MCC (`game_paths`),
/// else a Halo 2 (classic) map there. `h1:NAME` (Halo CE), `h2:NAME`,
/// `h3:NAME` and `hr:NAME` (Reach) pick the game; CE and Reach maps are only
/// found with their prefix, as their file names overlap the others
/// (`sidewinder`, `prisoner`). Halo 3's Blackout is `lockout.map`, so it
/// goes by `blackout` and a bare `lockout` is Halo 2's Lockout ([`codename`]).
fn resolve_map(arg: &str) -> PathBuf {
    let p = PathBuf::from(arg);
    if p.exists() {
        return p;
    }
    let (game, file) = codename(arg);
    let name = format!("{file}.map");
    match game {
        Some(game) => maps_dir(game).join(&name),
        None => {
            let h3 = maps_dir(Game::Halo3).join(&name);
            let h2 = maps_dir(Game::Halo2).join(&name);
            if !h3.exists() && h2.exists() { h2 } else { h3 }
        }
    }
}

/// A map argument to (game, file name without `.map`): `h1:` / `h2:` /
/// `h3:` / `hr:` force the game; `blackout` is Halo 3's `lockout`, and a
/// bare `lockout` is Halo 2's, so the two maps sharing a file name each have
/// a plain name.
fn codename(arg: &str) -> (Option<Game>, &str) {
    let prefixes = [("h1:", Game::Halo1), ("h2:", Game::Halo2), ("h3:", Game::Halo3), ("hr:", Game::Reach)];
    let (game, bare) = prefixes
        .iter()
        .find_map(|(p, g)| arg.strip_prefix(p).map(|rest| (Some(*g), rest)))
        .unwrap_or((None, arg));
    let bare = bare.strip_suffix(".map").unwrap_or(bare);
    match (game, bare) {
        (None | Some(Game::Halo3), "blackout") => (Some(Game::Halo3), "lockout"),
        (None, "lockout") => (Some(Game::Halo2), "lockout"),
        _ => (game, bare),
    }
}

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let help = matches!(args.first().map(String::as_str), Some("--help" | "-h"));
    // First-run setup: MCC and Skate 3 (required) and Reflex (optional),
    // asked once and saved (`halo3_setup`); `--setup` asks again. Until they
    // are set up the window opens on the menu's setup screen
    // (`menu/setup.rs`), and a map named on the command line waits for the
    // next launch. Headless runs (`--shot`, `--menu-shot`) never ask: they
    // need the setup done (or the dev overrides), except `--menu-setup`
    // shots of the setup screen itself.
    let redo = args.iter().any(|a| a == "--setup");
    args.retain(|a| a != "--setup");
    if !help && args.first().map(String::as_str) != Some("--test-scene") {
        let interactive = !args.iter().any(|a| a == "--shot" || a == "--menu-shot");
        if interactive {
            if redo {
                halo3_setup::forget().map_err(anyhow::Error::msg)?;
            }
            if halo3_setup::needs_setup()
                && let Some(map) = args.first().filter(|a| !a.starts_with("--")).cloned()
            {
                eprintln!("setup: the games are not set up yet; {map} can be started once they are");
                args.remove(0);
            }
        } else if !args.iter().any(|a| a == "--menu-setup") {
            halo3_setup::check().map_err(anyhow::Error::msg)?;
        }
    }
    match args.first().map(String::as_str) {
        Some("--help") | Some("-h") => {
            eprintln!("usage:
  halo3 [--foot]                                             the map menu
  halo3 --menu-shot FILE [--menu-tab N] [--menu-pick N] [--menu-loading | --menu-missing | --menu-settings | --menu-setup [--menu-step N]] [--size WxH]
  halo3 <map> [--zone N|NAME | --bsps 0,3,5 | --all-bsps]   e.g. riverworld, 120_halo
  halo3 warthog_run [--foot] [--difficulty ...]             the Warthog run of 120_halo, timed
  halo3 h1:warthog_run [--foot] [--difficulty ...]          Halo CE's Warthog run (The Maw), against its countdown
  halo3 <map> [--foot] [--ai nearby|zone|off|none]
  halo3 <map> --insertion NAME|N [--difficulty easy|normal|heroic|legendary] [--foot]   e.g. 120_halo --insertion trench_run (or trench_end)
  halo3 <map> --shot DIR [--view x,y,z,yaw,pitch]... [--size WxH] [--skate | --bike | --foot] [--bench N]
  halo3 --setup [<map>]                                      choose MCC, Skate 3 and Reflex again (on the setup screen)
  halo3 --test-scene");
            std::process::exit(2);
        }
        // No map (only options, or nothing): the map menu, which loads the
        // chosen map into the same window (`menu.rs`).
        None => run(&args, None),
        Some(a) if a.starts_with("--") && a != "--test-scene" => run(&args, None),
        Some(map) => {
            let map = load(map, &args)?;
            run(&args, Some(map))
        }
    }
}

/// A map decoded and ready to go into the app. Made up front for
/// `halo3 <map>`, or on the menu's loading thread.
pub(crate) struct LoadedMap {
    pub(crate) scene: blam_geometry::Scene,
    textures: textures::MaterialTextures,
    chief: Option<rider::ChiefModel>,
    sky: world::sky::SkyData,
    water: world::water::WaterData,
    /// Kept open for the whole run: zone sets decode their textures from it
    /// when they load (`world/stream.rs`).
    map_file: Option<&'static blam_cache::CacheFile>,
    arsenal: Option<weapons::assets::Arsenal>,
    foot_tuning: Option<foot::physics::Tuning>,
    plain_fog: Option<world::sky::PlainFogSetting>,
    /// Halo 2: the map's ambience, decoded on the sound plugin's thread.
    plain_sound: Option<audio::halo2::PlainLoader>,
    /// Halo 3 only: scripts, AI, the mission's zone sets.
    extras: MapExtras,
}

/// What a Halo 3 map brings beyond the world.
#[derive(Default)]
pub(crate) struct MapExtras {
    /// The name it was loaded by (a map or a [`Mission`]), for restarting.
    arg: String,
    /// The mission's own arguments (`--insertion trench_run`, ...), added
    /// to the command line's.
    mission_args: Vec<String>,
    /// The map's scripts, when asked for (`--insertion`, `--scripts`).
    scripts: Option<ScriptStart>,
    /// The campaign's squads and the weapons they carry.
    ai: Option<ai::AiAssets>,
    /// A mission's zone sets: the world never switches to any other.
    zone_sets: Option<Vec<usize>>,
    /// The effects the scripts and the objects they damage play, read
    /// with their bitmaps (`halo3_effects`).
    effects: Option<halo3_effects::EffectAssets>,
    /// Halo CE actors' animations (`halo1_anim.rs`).
    h1_moves: Option<halo1_anim::H1Moves>,
}

impl LoadedMap {
    pub(crate) fn scene_name(&self) -> &str {
        &self.scene.name
    }
}

/// A mission: one of the maps, started a set way. Offered on the map menu
/// (`menu.rs`) and by its name on the command line.
pub(crate) struct Mission {
    /// `halo3 <name>`.
    pub(crate) name: &'static str,
    /// The map it plays on.
    pub(crate) map: &'static str,
    /// What it starts with, as if given on the command line.
    pub(crate) args: &'static [&'static str],
    /// Only these zone sets: only their BSPs are decoded, and the world
    /// never switches to another (so nothing before the mission streams
    /// back in).
    pub(crate) zone_sets: &'static [&'static str],
    /// A Halo CE mission: its BSPs, where it starts and how its scripts
    /// begin (`halo1.rs`).
    pub(crate) h1: Option<halo1::Insertion>,
    /// The line under MISSION COMPLETE, and under MISSION FAILED
    /// (`finish.rs`).
    pub(crate) won_line: &'static str,
    pub(crate) lost_line: &'static str,
}

/// The missions.
pub(crate) const MISSIONS: &[Mission] = &[Mission {
    // Halo 3's last mission from the Warthog run on: the run's insertion
    // (`ins_trench_run`) on the dirt bike, the run's zone sets (BSPs 040,
    // 100, 110, 120), the clock and the completion screen (`finish.rs`).
    name: "warthog_run",
    map: "h3:120_halo",
    args: &["--insertion", "trench_run", "--zone", "120_04_100_110"],
    zone_sets: &["120_04_100_110", "120_100_110", "120_110_120"],
    h1: None,
    won_line: "You made it into the Forward Unto Dawn.",
    lost_line: "The mission is lost.",
}, Mission {
    // Halo CE's last mission, The Maw, from the Warthogs on: the dirt bike
    // stands in for the Warthog (`halo3_script::h1::THE_MAW`).
    name: "h1:warthog_run",
    map: "h1:d40",
    args: &[],
    zone_sets: &[],
    h1: Some(halo3_script::h1::THE_MAW),
    won_line: "You made it to the Longsword.",
    lost_line: "Time ran out: the Pillar of Autumn's reactor went up.",
}];

/// The mission called `name`, if it is one.
pub(crate) fn mission(name: &str) -> Option<&'static Mission> {
    MISSIONS.iter().find(|m| m.name == name)
}

/// Decodes a map or a [`Mission`] (by name), with the command line's
/// arguments (`args`, which a mission adds its own to).
pub(crate) fn load(arg: &str, args: &[String]) -> anyhow::Result<LoadedMap> {
    let found = mission(arg);
    let mission_args: Vec<String> = found.map_or_else(Vec::new, |m| m.args.iter().map(|a| a.to_string()).collect());
    let all: Vec<String> = args.iter().cloned().chain(mission_args.iter().cloned()).collect();
    if let Some(m) = found {
        eprintln!("mission {}: {} {}", m.name, m.map, m.args.join(" "));
    }
    let mut map = load_map(found.map_or(arg, |m| m.map), &all, found)?;
    map.extras.arg = arg.to_string();
    map.extras.mission_args = mission_args;
    Ok(map)
}

/// Decodes a map (a name or a path, see [`resolve_map`]), or the test scene.
fn load_map(map: &str, args: &[String], mission: Option<&Mission>) -> anyhow::Result<LoadedMap> {
    if map == "--test-scene" {
        return Ok(LoadedMap {
            scene: world::test_scene(),
            textures: textures::MaterialTextures::default(),
            chief: None,
            sky: world::sky::SkyData::default(),
            water: world::water::WaterData::default(),
            map_file: None,
            arsenal: None,
            foot_tuning: None,
            plain_fog: None,
            plain_sound: None,
            extras: MapExtras::default(),
        });
    }
    let path = resolve_map(map);
    // Told by the file's own header, not its name: Halo CE's `sidewinder`
    // and `prisoner` share their file names with other games' maps.
    if blam_h1::is_halo1(&path) {
        // Halo CE: decoded up front like Halo 2 (`halo1.rs`); a mission
        // decodes only its BSPs and starts its scripts.
        let insertion = mission.and_then(|m| m.h1.as_ref());
        let mut h1 = halo1::load(&path, insertion)?;
        let mut extras = MapExtras::default();
        // A mission's encounters, placed by its scripts (`halo1_ai.rs`),
        // and the weapons they carry.
        let mut arsenal = None;
        if insertion.is_some() && value_of(args, "--ai").as_deref() != Some("none") {
            if let Some(h1_ai) = halo1_ai::load(&h1.map, difficulty(args)) {
                extras.ai = Some(h1_ai.assets);
                extras.h1_moves = Some(h1_ai.moves);
                arsenal = h1_ai.arsenal;
            }
        }
        if let (Some(ins), Some(data)) = (insertion, h1.scripts.take()) {
            let start = h1.start;
            let runtime = halo3_script::h1::runtime(data, &h1.scene, ins, difficulty(args));
            extras.scripts = Some(ScriptStart {
                pending: halo3_script::PendingScripts(Some(runtime), None, None),
                devices: halo3_anim::MachinePoser::default(),
                start,
                effect_roots: Vec::new(),
            });
        }
        // The effects the scripts name (`halo1_effects.rs`).
        extras.effects = h1.effects.take();
        if let Some(mean) = world::lighting::scene_mean_irradiance(&h1.scene, &h1.textures.lightmaps) {
            eprintln!("halo ce lightmap: mean irradiance {mean:.3}");
            world::lighting::set_scene_mean(mean);
        }
        // The rider is Halo 3's Spartan, borrowed from Valhalla as for
        // Halo 2.
        let chief = blam_cache::CacheFile::open(resolve_map("riverworld"))
            .ok()
            .and_then(|c| rider::load_chief(&c));
        return Ok(LoadedMap {
            scene: h1.scene,
            textures: h1.textures,
            chief,
            sky: h1.sky,
            water: world::water::WaterData::default(),
            map_file: None,
            arsenal,
            foot_tuning: None,
            plain_fog: h1.fog,
            plain_sound: None,
            extras,
        });
    }
    if blam_h2::is_halo2(&path) {
        // Halo 2: everything decoded up front, no Halo 3 cache file
        // behind it (`halo2.rs`).
        let (scene, textures, fog, arsenal) = halo2::load(&path)?;
        // The mean lit surface, for the auto exposure (no camera fx in
        // Halo 2: the exposure stays at `lighting::EXPOSURE`).
        if let Some(mean) = world::lighting::scene_mean_irradiance(&scene, &textures.lightmaps) {
            eprintln!("halo 2 lightmap: mean irradiance {mean:.3}");
            world::lighting::set_scene_mean(mean);
        }
        // The rider is Halo 3's Spartan, borrowed from Valhalla when
        // Halo 3 is installed (Halo 2's Master Chief is not decoded).
        let chief = blam_cache::CacheFile::open(resolve_map("riverworld"))
            .ok()
            .and_then(|c| rider::load_chief(&c));
        return Ok(LoadedMap {
            scene,
            textures,
            chief,
            sky: world::sky::SkyData::default(),
            water: world::water::WaterData::default(),
            map_file: None,
            arsenal,
            foot_tuning: None,
            plain_fog: fog,
            plain_sound: Some(halo2::ambience_loader(&path)),
            extras: MapExtras::default(),
        });
    }
    let started = std::time::Instant::now();
    let cache: &'static blam_cache::CacheFile = Box::leak(Box::new(blam_cache::CacheFile::open(&path)?));
    let mut scene = blam_geometry::Scene {
        name: cache.header.map_name.clone(),
        ..Default::default()
    };
    // Reach: the same cache file, resources, shaders and bitmaps as Halo 3,
    // its own tag layouts (`blam_reach`, `reach.rs`).
    let reach = cache.engine() == blam_cache::Engine::Reach;
    let mut extras = MapExtras::default();
    if reach {
        reach::decode(cache, &mut scene)?;
    } else {
        // A mission decodes only its zone sets' BSPs.
        let mut options = blam_geometry::Options::default();
        if let Some(m) = mission.filter(|m| !m.zone_sets.is_empty()) {
            let (sets, bsps) = mission_zone_sets(cache, m)?;
            eprintln!("mission {}: zone sets {:?}, BSPs {:?} only", m.name, m.zone_sets, bsps);
            options.bsps = Some(bsps);
            extras.zone_sets = Some(sets);
        }
        blam_geometry::decode_into(cache, &options, &mut scene)?;
        // Machines stand where their own animation puts them at rest
        // (120_halo's frigate faces the run with its hangar open).
        let posed = halo3_anim::devices::place_machines(cache, &mut scene);
        if posed > 0 {
            eprintln!("machines: {posed} posed by their animations");
        }
        // Objects collide (`HALO3_OBJECT_COLLISION=0`: not): crates,
        // rocks, and the Warthog run's floor plates, which are its
        // only floor.
        if !std::env::var("HALO3_OBJECT_COLLISION").is_ok_and(|v| v == "0") {
            let n = blam_geometry::objects::add_collision(cache, &mut scene);
            eprintln!("object collision: {n} triangles");
        }
        extras.scripts = scripts_for(args, cache, &mut scene);
        if let Some(s) = &extras.scripts {
            extras.effects = Some(halo3_effects::load::load(cache, &s.effect_roots));
        }
    }
    eprintln!(
        "{}: {} meshes, {} placements, {} triangles, decoded in {:.1} s",
        scene.name,
        scene.meshes.len(),
        scene.placements.len(),
        scene.triangle_count(),
        started.elapsed().as_secs_f32()
    );
    let started = std::time::Instant::now();
    let textures = textures::load_with(cache, &scene, false);
    let bytes: usize = textures.lightmaps.iter().flatten().map(|t| t.coefficients.byte_len() + t.intensity.byte_len()).sum();
    eprintln!(
        "textures: {} materials resolved (their bitmaps load with their zone set), lightmaps {:.1} MiB, in {:.1} s",
        scene.materials.len(),
        bytes as f64 / (1 << 20) as f64,
        started.elapsed().as_secs_f32()
    );
    let mean = world::lighting::scene_mean_irradiance(&scene, &textures.lightmaps);
    if let Some(mean) = mean {
        world::lighting::set_scene_mean(mean);
    }
    match blam_geometry::camera_fx::read(cache) {
        Ok(Some(fx)) => {
            world::lighting::set_map_exposure(fx.exposure);
            world::lighting::set_auto_range(fx.auto_range);
            eprintln!(
                "camera fx {}: exposure {:.2} stops (auto range {:?}), mean irradiance {:.3} -> x{:.2}",
                fx.tag,
                fx.exposure,
                fx.auto_range,
                mean.unwrap_or(0.0),
                world::lighting::exposure()
            );
        }
        Ok(None) => eprintln!("no camera fx: exposure x{}", world::lighting::exposure()),
        Err(e) => eprintln!("camera fx: {e}; exposure x{}", world::lighting::exposure()),
    }
    if reach {
        return Ok(reach::finish(cache, scene, textures));
    }
    let chief = rider::load_chief(cache);
    // The scenario's squads, and the weapons they carry.
    if value_of(args, "--ai").as_deref() != Some("none") {
        extras.ai = ai::load(cache);
    }
    let extra = extras.ai.as_ref().map(ai::AiAssets::weapon_tags).unwrap_or_default();
    let arsenal = weapons::assets::load_with(cache, &extra);
    let foot_tuning = match blam_geometry::movement::read(cache) {
        Ok(m) => {
            eprintln!(
                "on foot: {} runs {:.2} wu/s, jumps {:.2} wu/s ({:.2} wu high)",
                m.biped,
                m.run_forward,
                m.jump_velocity,
                m.jump_height()
            );
            Some(foot::physics::Tuning::from_movement(&m))
        }
        Err(e) => {
            eprintln!("on foot: no player numbers ({e}); using Valhalla's");
            None
        }
    };
    let sky = world::sky::load(cache, &scene);
    let water = world::water::load(cache, &scene);
    Ok(LoadedMap {
        scene,
        textures,
        chief,
        sky,
        water,
        map_file: Some(cache),
        arsenal,
        foot_tuning,
        plain_fog: None,
        plain_sound: None,
        extras,
    })
}

/// A mission's zone sets (indices in the scenario) and the BSPs they use.
fn mission_zone_sets(cache: &blam_cache::CacheFile, m: &Mission) -> anyhow::Result<(Vec<usize>, Vec<u32>)> {
    let mut layout = blam_geometry::Scene::default();
    blam_geometry::zones::decode(cache, &mut layout)?;
    let mut sets = Vec::new();
    let mut mask = 0u32;
    for name in m.zone_sets {
        let n = layout
            .zone_sets
            .iter()
            .position(|z| z.name == *name)
            .ok_or_else(|| anyhow::anyhow!("mission {}: {} has no zone set {name}", m.name, m.map))?;
        sets.push(n);
        mask |= layout.zone_sets[n].bsps;
    }
    Ok((sets, (0..32).filter(|b| mask & (1 << b) != 0).collect()))
}

/// Builds and runs the app: on `map`, or on the map menu when there is none.
fn run(args: &[String], map: Option<LoadedMap>) -> anyhow::Result<()> {
    let shot = match &map {
        Some(map) => shot_plan(args, &map.scene)?,
        None => None,
    };
    let title = match &map {
        Some(map) => format!("Halo 3 - {}", map.scene.name),
        None => "Halo 3".to_string(),
    };
    // Sound plays only in a window (`audio/`); `--shot` never opens the
    // sound device.
    let sound = audio::wanted(shot.is_some());
    let mut app = App::new();
    match &shot {
        // No window and no event loop: render into an image on a plain loop.
        Some(_) => app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: bevy::window::ExitCondition::DontExit,
                    ..default()
                })
                .disable::<bevy::winit::WinitPlugin>()
                .disable::<bevy::audio::AudioPlugin>(),
        )
        .add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(
            std::time::Duration::from_millis(1),
        )),
        None if menu::headless(args) => menu::add_headless_plugins(&mut app),
        None => {
            let plugins = DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title,
                    present_mode: PresentMode::AutoVsync,
                    // Borderless fullscreen on the monitor the window opens on.
                    mode: WindowMode::BorderlessFullscreen(MonitorSelection::Current),
                    ..default()
                }),
                ..default()
            });
            if sound {
                app.add_plugins(plugins)
            } else {
                app.add_plugins(plugins.disable::<bevy::audio::AudioPlugin>())
            }
        }
    };
    let sound = sound && !menu::headless(args);
    if sound || audio::trace_wanted() {
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
        app.add_plugins(audio::HaloAudioPlugin {
            output: sound,
            trace: audio::trace_wanted(),
            music: value("--music"),
        });
    }
    // Skating on or off (Settings; the menu changes it for the next map).
    // A `--skate` shot needs the board, whatever the setting says.
    let skate_shot = args.iter().any(|a| a == "--skate");
    app.insert_resource(halo3_core::skate::SkateEnabled(skate_shot || game_paths::skating_enabled()));
    let skating = shot.as_ref().is_none_or(|p| p.rides());
    // Off the menu every map gets every mode; a map named on the command
    // line without collision has no bike, feet or weapons.
    let grounded = map.as_ref().is_none_or(|m| !m.scene.collision.triangles.is_empty());
    if let Some(plan) = shot {
        app.add_plugins(shot::ShotPlugin(plan));
    }
    if skating {
        app.add_plugins((skate::SkatePlugin, rider::RiderPlugin, swim::SwimPlugin));
        if grounded {
            // The campaign's squads (only maps with AI bring any).
            app.add_plugins((bike::BikePlugin, foot::FootPlugin, weapons::WeaponsPlugin, ai::AiPlugin, halo1_anim::H1AnimPlugin));
        }
    }
    if args.iter().any(|a| a == "--anim-test") {
        app.add_plugins(anim_test::AnimTestPlugin);
    }
    app.add_plugins((
        halo3_core::map::MapPhasePlugin,
        pause::PausePlugin,
        speedo::SpeedoPlugin,
        // The map's scripts, and the completion screen when they win.
        halo3_script::ScriptPlugin,
        halo3_effects::EffectsPlugin,
        finish::FinishPlugin,
        countdown::CountdownPlugin,
        fly_camera::FlyCameraPlugin,
        flag::SessionFlagPlugin,
        world::WorldPlugin,
    ));
    match map {
        Some(map) => install(app.world_mut(), map, args)?,
        None => {
            app.add_plugins(menu::MenuPlugin::new(args));
        }
    }
    app.run();
    Ok(())
}

/// Puts a loaded map's resources into the world and marks it installed;
/// its start-up runs on the next frame (`halo3_core::map`).
pub(crate) fn install(w: &mut World, map: LoadedMap, args: &[String]) -> anyhow::Result<()> {
    let LoadedMap {
        scene,
        textures,
        chief,
        sky,
        water,
        map_file,
        arsenal,
        foot_tuning,
        plain_fog,
        plain_sound,
        extras,
    } = map;
    // What Restart on the completion screen loads again (`finish.rs`).
    w.insert_resource(finish::CurrentMap {
        arg: extras.arg.clone(),
        args: args.to_vec(),
    });
    // A mission's own arguments after the command line's.
    let args: Vec<String> = args.iter().cloned().chain(extras.mission_args.iter().cloned()).collect();
    let args = &args[..];
    let shot = w.get_resource::<shot::ShotPlan>().cloned();
    let shot_mode = shot.is_some();
    let mut zones = world::zones::Zones::of(&scene);
    zones.allowed = extras.zone_sets;
    let loaded = world::zones::initial(args, &zones)?;
    if !zones.layout.zone_sets.is_empty() {
        eprintln!(
            "{} zone sets, {} BSPs, {} player starting locations; loaded: {}",
            zones.layout.zone_sets.len(),
            zones.layout.bsps.len(),
            zones.layout.starts.len(),
            zones.describe(loaded.mask)
        );
    }
    match (map_file, plain_sound) {
        // Reach's sound tags are not read (`reach.rs`).
        (Some(cache), _) if cache.engine() != blam_cache::Engine::Halo3 => {}
        (Some(cache), _) => audio::install(w, audio::MapSound::Halo3(cache)),
        (None, Some(loader)) => audio::install(w, audio::MapSound::Plain(loader)),
        (None, None) => {}
    }
    let start_spot = scene
        .spawns
        .first()
        .map(|s| (s.position, s.yaw))
        .or_else(|| scene.starts.first().map(|s| (s.position, s.yaw)));
    if let Some(plan) = shot.as_ref().filter(|p| p.rides()) {
        // Ride from the first view, or where an insertion starts, or the
        // start spawn.
        let start = match plan.views.first() {
            Some(v) => Some((v.position, v.yaw_deg.to_radians())),
            None => extras.scripts.as_ref().and_then(|s| s.start).or(start_spot),
        };
        match start {
            Some((position, yaw)) if plan.bike => w.insert_resource(bike::AutoStart::at(position, yaw)),
            Some((position, yaw)) if plan.foot => w.insert_resource(foot::AutoStart::at(position, yaw)),
            Some((position, yaw)) => w.insert_resource(skate::AutoStart::at(position, yaw)),
            None => {}
        }
    }
    w.insert_resource(skate::SkateInput(
        (!scene.collision.triangles.is_empty()).then(|| scene.collision.clone()),
        scene.trigger_volumes.clone(),
        scene.water_triangles(),
    ));
    w.insert_resource(rider::PendingChief(chief));
    w.insert_resource(bike::BikeInput(std::sync::Arc::new(scene.collision.clone())));
    if let Some(t) = foot_tuning {
        w.insert_resource(foot::FootTuning(t));
    }
    // An insertion starts the run on the bike where it puts the player
    // (on foot with `--foot`); `--foot` alone starts on foot at the start
    // spawn.
    let foot_wanted = args.iter().any(|a| a == "--foot");
    let insertion_start = extras.scripts.as_ref().and_then(|s| s.start);
    if !shot_mode && !scene.collision.triangles.is_empty() {
        match insertion_start {
            Some((position, yaw)) if foot_wanted => w.insert_resource(foot::AutoStart::at(position, yaw)),
            Some((position, yaw)) => w.insert_resource(bike::AutoStart::player(position, yaw)),
            None if foot_wanted && let Some((position, yaw)) = start_spot => {
                w.insert_resource(foot::AutoStart::at(position, yaw))
            }
            None => {}
        }
    }
    // `--anim-test`: Halo 3's own animations on a row of the map's bipeds.
    if let Some(spec) = value_of(args, "--anim-test") {
        let Some(cache) = map_file else {
            anyhow::bail!("--anim-test needs a Halo 3 map");
        };
        // Three world units in front of the first view (or the start), on
        // the ground, facing back at it.
        let (eye, yaw, eye_height) = match shot.as_ref().and_then(|p| p.views.first()) {
            Some(v) => (v.position, v.yaw_deg.to_radians(), 0.6),
            None => scene
                .starts
                .first()
                .map(|s| (s.position, s.yaw, 0.0))
                .or_else(|| scene.spawns.first().map(|s| (s.position, s.yaw, 0.0)))
                .unwrap_or((Vec3::ZERO, 0.0, 0.0)),
        };
        let ahead = Vec3::new(yaw.cos(), yaw.sin(), 0.0) * 3.0;
        let fallback = (eye + ahead - Vec3::Z * eye_height, yaw + std::f32::consts::PI);
        w.insert_resource(anim_test::load(cache, &spec, value_of(args, "--anim-at").as_deref(), fallback)?);
    }
    // Kill volumes and the bottom of the world (`falls.rs`), water
    // surfaces and the ground under them, for sinking, falling and
    // respawning (`swim.rs`).
    let kill = falls::KillZones::new(&scene.trigger_volumes, &scene.collision);
    eprintln!(
        "kill volumes: {}; Guardian watch volumes: {}; out of the world below z {:.1} wu",
        kill.volumes.len(),
        kill.guard_count(),
        kill.floor
    );
    w.insert_resource(kill);
    // Gravity lifts (`halo3_core::lifts`): walking and the bike ride them.
    if !scene.lifts.is_empty() {
        eprintln!(
            "lifts: {}",
            scene.lifts.iter().map(|l| l.name.as_str()).collect::<Vec<_>>().join(", ")
        );
    }
    w.insert_resource(halo3_core::lifts::Lifts(scene.lifts.clone()));
    let mut water_materials: Vec<u32> = water
        .looks
        .iter()
        .filter(|(_, l)| matches!(l, world::water::WaterLook::Water(_)))
        .map(|(m, _)| *m)
        .collect();
    water_materials.sort_unstable();
    if !scene.collision.triangles.is_empty() {
        w.insert_resource(swim::WaterWorld::build(&scene, &water_materials));
    }
    if let Some((position, yaw)) = start_spot {
        w.insert_resource(swim::FallbackSpot(position, yaw));
    }
    w.insert_resource(weapons::PendingArsenal(arsenal));
    let scripted = extras.scripts.is_some();
    w.insert_resource(halo3_effects::PendingEffects(extras.effects));
    if let Some(ScriptStart { pending: s, devices, .. }) = extras.scripts {
        // The scripts' `ai_*` calls go to the enemies through the bridge
        // the AI plugin reads; the islands and pillars move by their own
        // animations.
        if extras.ai.is_some() {
            let bridge = ai::AiBridge::default();
            w.insert_resource(halo3_script::ScriptAi::new(halo3_script::BridgeAi::new(bridge.clone())));
            w.insert_resource(bridge);
        }
        w.insert_resource(halo3_script::ScriptAnimation::new(devices));
        w.insert_resource(s);
    }
    if extras.ai.is_some() {
        // `--ai nearby` (default): squads appear as you come near them;
        // `--ai zone`: every squad of the loaded zone set at once; `--ai
        // off`: only scripts place them; `--ai none`: no AI at all. While
        // the map's scripts run they place the squads.
        let mode = match value_of(args, "--ai").as_deref() {
            _ if scripted => ai::AutoPlace::Off,
            Some("zone") => ai::AutoPlace::ZoneSet,
            Some("off") => ai::AutoPlace::Off,
            _ => ai::AutoPlace::default(),
        };
        w.insert_resource(mode);
        w.insert_resource(ai::PendingAi(extras.ai));
        if let Some(m) = extras.h1_moves {
            w.insert_resource(m);
        }
    }
    w.insert_resource(zones);
    w.insert_resource(world::stream::ZoneTriggers::new(shot_mode));
    w.insert_resource(loaded);
    w.insert_resource(world::PendingScene(Some(scene), Some(textures), map_file));
    w.insert_resource(world::sky::PendingSky(Some(sky)));
    w.insert_resource(world::sky::PlainFog(plain_fog));
    w.insert_resource(world::water::PendingWater(Some(water)));
    w.insert_resource(halo3_core::map::MapInstalled);
    Ok(())
}

/// The map's scripts, when asked for: `--insertion NAME|N` or `--scripts`
/// (`halo3_script`). Marks the objects they move before the world is
/// built from the scene.
fn scripts_for(
    args: &[String],
    cache: &blam_cache::CacheFile,
    scene: &mut blam_geometry::Scene,
) -> Option<ScriptStart> {
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let insertion = value("--insertion");
    if insertion.is_none() && !args.iter().any(|a| a == "--scripts") {
        return None;
    }
    let data = halo3_script::prepare(cache, scene)?;
    let mut config = halo3_script::Config {
        difficulty: difficulty(args),
        ..Default::default()
    };
    let mut console = None;
    let mut start = None;
    let mut start_override = None;
    let mut passed = Vec::new();
    match insertion.as_deref().map(|i| (i, i.parse::<i16>())) {
        Some((_, Ok(n))) => config.insertion_point = n,
        Some((name, Err(_))) => match halo3_script::setup::insertion(&data, scene, name) {
            Some(ins) => {
                // As at the console in the editor: the mission's own
                // `start` stays out of the way.
                config.editor = true;
                console = Some(ins.script);
                start = ins.start;
                start_override = ins.start_override;
                passed = ins.passed;
                if let Some((at, yaw)) = start {
                    eprintln!("scripts: insertion {name} starts at halo [{:.2}, {:.2}, {:.2}] facing {:.0} degrees", at.x, at.y, at.z, yaw.to_degrees());
                }
            }
            None => {
                let mut known: Vec<&str> = data.scripts.iter().map(|s| s.name.as_str()).filter(|n| n.starts_with("ins_")).collect();
                if data.script_index("ins_trench_run").is_some() {
                    known.push(halo3_script::setup::TRENCH_END);
                }
                eprintln!("scripts: no insertion {name}; this map has {known:?}");
            }
        },
        None => {}
    }
    let mut runtime = halo3_script::setup::runtime(cache, scene, data, config);
    runtime.pass_volumes(&passed);
    eprintln!(
        "scripts: {} dialogue lines ({} lengths)",
        runtime.dialogue.lines.len(),
        runtime.dialogue.language.unwrap_or("the tags'")
    );
    // The animations of the machines scripts move (doors, the Warthog
    // run's islands and pillars).
    let started = std::time::Instant::now();
    let devices = halo3_anim::MachinePoser::load(cache, runtime.objects.info.iter().filter(|o| o.scene.is_some()).filter_map(|o| o.tag));
    eprintln!("scripts: {} machine animations in {:.1} s", devices.len(), started.elapsed().as_secs_f32());
    // The effects the scripts name and those of the damage sections of
    // the objects they damage.
    let mut effect_roots: Vec<u32> = runtime
        .data
        .references
        .iter()
        .filter(|(class, _)| class == "effe")
        .filter_map(|(_, tag)| *tag)
        .collect();
    for o in &runtime.objects.info {
        for section in &o.sections {
            effect_roots.extend(section.effects.iter().filter_map(|n| cache.tags.find("effe", n)).map(|t| t.index));
        }
    }
    effect_roots.sort_unstable();
    effect_roots.dedup();
    Some(ScriptStart {
        pending: halo3_script::PendingScripts(Some(runtime), console, start_override),
        devices,
        start,
        effect_roots,
    })
}

/// The map's scripts, ready to run, and where the insertion starts the
/// player (on the bike) when known.
struct ScriptStart {
    pending: halo3_script::PendingScripts,
    devices: halo3_anim::MachinePoser,
    start: Option<(Vec3, f32)>,
    /// `effe` tags the scripts and their objects' damage play.
    effect_roots: Vec<u32>,
}

/// `--difficulty easy|normal|heroic|legendary` as the scripts number it
/// (0 to 3); normal when not given.
fn difficulty(args: &[String]) -> i16 {
    match value_of(args, "--difficulty").as_deref() {
        Some("easy") => 0,
        Some("heroic") => 2,
        Some("legendary") => 3,
        _ => 1,
    }
}

/// The value after `flag`, if given.
fn value_of(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

/// `--shot <dir>` with its `--view` and `--size` options, if given.
fn shot_plan(args: &[String], scene: &blam_geometry::Scene) -> anyhow::Result<Option<shot::ShotPlan>> {
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1));
    let Some(dir) = value("--shot") else {
        return Ok(None);
    };
    let views = args
        .windows(2)
        .filter(|w| w[0] == "--view")
        .map(|w| match w[1].as_str() {
            "spawn" => shot::View::spawn(scene, 0).ok_or_else(|| anyhow::anyhow!("--view spawn: no spawn points")),
            "overview" => Ok(shot::View::overview(scene)),
            v if v.starts_with("spawn") => {
                let n: usize = v["spawn".len()..].parse()?;
                shot::View::spawn(scene, n).ok_or_else(|| anyhow::anyhow!("--view {v}: no such spawn point"))
            }
            v => v.parse::<shot::View>().map_err(anyhow::Error::msg),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let size = match value("--size") {
        Some(s) => {
            let (w, h) = s.split_once('x').ok_or_else(|| anyhow::anyhow!("--size wants WxH"))?;
            UVec2::new(w.parse()?, h.parse()?)
        }
        None => UVec2::new(1280, 720),
    };
    Ok(Some(shot::ShotPlan {
        dir: PathBuf::from(dir),
        name: scene.name.clone(),
        views,
        size,
        skate: args.iter().any(|a| a == "--skate"),
        bike: args.iter().any(|a| a == "--bike"),
        foot: args.iter().any(|a| a == "--foot"),
        bench: value("--bench").map(|n| n.parse()).transpose()?.unwrap_or(0),
    }))
}
