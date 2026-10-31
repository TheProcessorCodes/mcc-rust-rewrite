//! `halo3`: open a Halo 3 map and fly around it.
//!
//! ```text
//! halo3                  the map menu: pick a map, Start Game (`menu.rs`)
//! halo3 <map>            riverworld, or a path to a .map
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

mod halo1;
mod halo2;
mod menu;
mod reach;
mod pause;
mod shot;

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
    // asked once and saved (`halo3_setup`); `--setup` asks again. Without
    // MCC and Skate 3 the game does not start. Headless runs (`--shot`, `--menu-shot`) never show
    // a dialog: they need the setup done (or the dev overrides).
    let redo = args.iter().any(|a| a == "--setup");
    args.retain(|a| a != "--setup");
    if !help && args.first().map(String::as_str) != Some("--test-scene") {
        let interactive = !args.iter().any(|a| a == "--shot" || a == "--menu-shot");
        if let Err(e) = halo3_setup::prepare(interactive, redo) {
            if interactive {
                halo3_setup::fail(&e);
            }
            anyhow::bail!(e);
        }
    }
    match args.first().map(String::as_str) {
        Some("--help") | Some("-h") => {
            eprintln!("usage:
  halo3 [--foot]                                             the map menu
  halo3 --menu-shot FILE [--menu-tab N] [--menu-pick N] [--menu-loading | --menu-missing] [--size WxH]
  halo3 <map> [--zone N|NAME | --bsps 0,3,5 | --all-bsps]   e.g. riverworld, 120_halo
  halo3 <map> [--foot]
  halo3 <map> --shot DIR [--view x,y,z,yaw,pitch]... [--size WxH] [--skate | --bike | --foot] [--bench N]
  halo3 --setup [<map>]                                      choose MCC, Skate 3 and Reflex again
  halo3 --test-scene");
            std::process::exit(2);
        }
        // No map (only options, or nothing): the map menu, which loads the
        // chosen map into the same window (`menu.rs`).
        None => run(&args, None),
        Some(a) if a.starts_with("--") && a != "--test-scene" => run(&args, None),
        Some(map) => {
            let map = load(map)?;
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
}

impl LoadedMap {
    pub(crate) fn scene_name(&self) -> &str {
        &self.scene.name
    }
}

/// Decodes a map (a name or a path, see [`resolve_map`]), or the test scene.
pub(crate) fn load(map: &str) -> anyhow::Result<LoadedMap> {
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
        });
    }
    let path = resolve_map(map);
    // Told by the file's own header, not its name: Halo CE's `sidewinder`
    // and `prisoner` share their file names with other games' maps.
    if blam_h1::is_halo1(&path) {
        // Halo CE: decoded up front like Halo 2 (`halo1.rs`).
        let h1 = halo1::load(&path)?;
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
            arsenal: None,
            foot_tuning: None,
            plain_fog: h1.fog,
            plain_sound: None,
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
    if reach {
        reach::decode(cache, &mut scene)?;
    } else {
        blam_geometry::decode_into(cache, &blam_geometry::Options::default(), &mut scene)?;
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
    let arsenal = weapons::assets::load(cache);
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
    })
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
            app.add_plugins((bike::BikePlugin, foot::FootPlugin, weapons::WeaponsPlugin));
        }
    }
    app.add_plugins((
        halo3_core::map::MapPhasePlugin,
        pause::PausePlugin,
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
    } = map;
    let shot = w.get_resource::<shot::ShotPlan>().cloned();
    let shot_mode = shot.is_some();
    let zones = world::zones::Zones::of(&scene);
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
        // Ride from the first view, or the start spawn.
        let start = match plan.views.first() {
            Some(v) => Some((v.position, v.yaw_deg.to_radians())),
            None => start_spot,
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
    w.insert_resource(bike::BikeInput(scene.collision.clone()));
    if let Some(t) = foot_tuning {
        w.insert_resource(foot::FootTuning(t));
    }
    // `--foot`: start on foot at the start spawn.
    if !shot_mode
        && args.iter().any(|a| a == "--foot")
        && !scene.collision.triangles.is_empty()
        && let Some((position, yaw)) = start_spot
    {
        w.insert_resource(foot::AutoStart::at(position, yaw));
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
