//! `halo3`: open a Halo 3 map and fly around it.
//!
//! ```text
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
//! `<map>` is looked up in `%HALO3_MAPS%`, or the default MCC install; Halo 2
//! maps (`coagulation`, or `h2:NAME`) in `%HALO2_MAPS%` or MCC's
//! `halo2\h2_maps_win64_dx11` (`halo2.rs`).

mod halo2;
mod pause;
mod shot;

use halo3_audio as audio;
use halo3_bike as bike;
use halo3_core::{falls, fly_camera};
use halo3_foot as foot;
use halo3_rider as rider;
use halo3_skate as skate;
use halo3_swim as swim;
use halo3_textures as textures;
use halo3_weapons as weapons;
use halo3_world as world;

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::PresentMode;

use halo3_core::DEFAULT_MAPS;

/// MCC's Halo 2 (classic) maps, tried when a bare name is not a Halo 3 map.
pub(crate) const DEFAULT_H2_MAPS: &str =
    r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo2\h2_maps_win64_dx11";

/// A path, or a bare name: a Halo 3 map in `%HALO3_MAPS%` (or the MCC
/// install), else a Halo 2 map in `%HALO2_MAPS%` (or the MCC install).
/// `h2:NAME` picks the Halo 2 map when both games have one by that name
/// (`lockout`, `zanzibar`).
fn resolve_map(arg: &str) -> PathBuf {
    let p = PathBuf::from(arg);
    if p.exists() {
        return p;
    }
    let (h2_only, bare) = match arg.strip_prefix("h2:") {
        Some(rest) => (true, rest),
        None => (false, arg),
    };
    let mut name = bare.to_string();
    if !name.ends_with(".map") {
        name.push_str(".map");
    }
    let dir = |var: &str, default: &str| std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(default));
    let h3 = dir("HALO3_MAPS", DEFAULT_MAPS).join(&name);
    let h2 = dir("HALO2_MAPS", DEFAULT_H2_MAPS).join(&name);
    if h2_only || (!h3.exists() && h2.exists()) { h2 } else { h3 }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut foot_tuning = None;
    let mut arsenal = None;
    let (scene, textures, chief, sky, water, map_file) = match args.first().map(String::as_str) {
        None | Some("--help") | Some("-h") => {
            eprintln!("usage:
  halo3 <map> [--zone N|NAME | --bsps 0,3,5 | --all-bsps]   e.g. riverworld, 120_halo
  halo3 <map> [--foot]
  halo3 <map> --shot DIR [--view x,y,z,yaw,pitch]... [--size WxH] [--skate | --bike | --foot] [--bench N]
  halo3 --test-scene");
            std::process::exit(2);
        }
        Some("--test-scene") => (
            world::test_scene(),
            textures::MaterialTextures::default(),
            None,
            world::sky::SkyData::default(),
            world::water::WaterData::default(),
            None,
        ),
        Some(map) if blam_h2::is_halo2(&resolve_map(map)) => {
            // Halo 2: everything decoded up front, no Halo 3 cache file
            // behind it (`halo2.rs`).
            let (scene, textures) = halo2::load(&resolve_map(map))?;
            // The rider is Halo 3's Spartan, borrowed from Valhalla when
            // Halo 3 is installed (Halo 2's Master Chief is not decoded).
            let chief = blam_cache::CacheFile::open(resolve_map("riverworld"))
                .ok()
                .and_then(|c| rider::load_chief(&c));
            (
                scene,
                textures,
                chief,
                world::sky::SkyData::default(),
                world::water::WaterData::default(),
                None,
            )
        }
        Some(map) => {
            let path = resolve_map(map);
            let started = std::time::Instant::now();
            // Kept open for the whole run: zone sets decode their textures
            // from it when they load (`world/stream.rs`).
            let cache: &'static blam_cache::CacheFile = Box::leak(Box::new(blam_cache::CacheFile::open(&path)?));
            let mut scene = blam_geometry::Scene {
                name: cache.header.map_name.clone(),
                ..Default::default()
            };
            blam_geometry::decode_into(cache, &blam_geometry::Options::default(), &mut scene)?;
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
            let chief = rider::load_chief(cache);
            arsenal = weapons::assets::load(cache);
            match blam_geometry::movement::read(cache) {
                Ok(m) => {
                    eprintln!(
                        "on foot: {} runs {:.2} wu/s, jumps {:.2} wu/s ({:.2} wu high)",
                        m.biped,
                        m.run_forward,
                        m.jump_velocity,
                        m.jump_height()
                    );
                    foot_tuning = Some(foot::physics::Tuning::from_movement(&m));
                }
                Err(e) => eprintln!("on foot: no player numbers ({e}); using Valhalla's"),
            }
            let sky = world::sky::load(cache, &scene);
            let water = world::water::load(cache, &scene);
            (scene, textures, chief, sky, water, Some(cache))
        }
    };
    let shot = shot_plan(&args, &scene)?;
    let zones = world::zones::Zones::of(&scene);
    let loaded = world::zones::initial(&args, &zones)?;
    if !zones.layout.zone_sets.is_empty() {
        eprintln!(
            "{} zone sets, {} BSPs, {} player starting locations; loaded: {}",
            zones.layout.zone_sets.len(),
            zones.layout.bsps.len(),
            zones.layout.starts.len(),
            zones.describe(loaded.mask)
        );
    }
    let title = format!("Halo 3 - {}", scene.name);
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
        None => {
            let plugins = DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title,
                    present_mode: PresentMode::AutoVsync,
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
    if sound || audio::trace_wanted() {
        let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
        app.add_plugins(audio::HaloAudioPlugin {
            cache: map_file,
            output: sound,
            trace: audio::trace_wanted(),
            music: value("--music"),
        });
    }
    let skating = shot.as_ref().is_none_or(|p| p.rides());
    let collision = scene.collision.clone();
    let shot_mode = shot.is_some();
    if let Some(plan) = shot {
        if plan.rides() {
            // Ride from the first view, or the start spawn.
            let start = match plan.views.first() {
                Some(v) => Some((v.position, v.yaw_deg.to_radians())),
                None => scene
                    .spawns
                    .first()
                    .map(|s| (s.position, s.yaw))
                    .or_else(|| scene.starts.first().map(|s| (s.position, s.yaw))),
            };
            match start {
                Some((position, yaw)) if plan.bike => {
                    app.insert_resource(bike::AutoStart::at(position, yaw));
                }
                Some((position, yaw)) if plan.foot => {
                    app.insert_resource(foot::AutoStart::at(position, yaw));
                }
                Some((position, yaw)) => {
                    app.insert_resource(skate::AutoStart::at(position, yaw));
                }
                None => {}
            }
        }
        app.add_plugins(shot::ShotPlugin(plan));
    }
    if skating {
        app.insert_resource(skate::SkateInput(
            (!scene.collision.triangles.is_empty()).then(|| scene.collision.clone()),
            scene.trigger_volumes.clone(),
            scene.water_triangles(),
        ))
        .insert_resource(rider::PendingChief(chief))
        .add_plugins((skate::SkatePlugin, rider::RiderPlugin, swim::SwimPlugin));
        if !collision.triangles.is_empty() {
            app.insert_resource(bike::BikeInput(collision))
                .add_plugins((bike::BikePlugin, foot::FootPlugin, weapons::WeaponsPlugin));
            if let Some(t) = foot_tuning {
                app.insert_resource(foot::FootTuning(t));
            }
            // `--foot`: start on foot at the start spawn.
            if !shot_mode
                && args.iter().any(|a| a == "--foot")
                && let Some((position, yaw)) = scene
                    .spawns
                    .first()
                    .map(|s| (s.position, s.yaw))
                    .or_else(|| scene.starts.first().map(|s| (s.position, s.yaw)))
            {
                app.insert_resource(foot::AutoStart::at(position, yaw));
            }
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
        app.insert_resource(kill);
        let mut water_materials: Vec<u32> = water
            .looks
            .iter()
            .filter(|(_, l)| matches!(l, world::water::WaterLook::Water(_)))
            .map(|(m, _)| *m)
            .collect();
        water_materials.sort_unstable();
        if !scene.collision.triangles.is_empty() {
            app.insert_resource(swim::WaterWorld::build(&scene, &water_materials));
        }
        if let Some(s) = scene.spawns.first() {
            app.insert_resource(swim::FallbackSpot(s.position, s.yaw));
        } else if let Some(s) = scene.starts.first() {
            app.insert_resource(swim::FallbackSpot(s.position, s.yaw));
        }
    }
    app.insert_resource(weapons::PendingArsenal(arsenal));
    app.insert_resource(zones)
        .insert_resource(world::stream::ZoneTriggers::new(shot_mode))
        .insert_resource(loaded)
        .insert_resource(world::PendingScene(Some(scene), Some(textures), map_file))
        .insert_resource(world::sky::PendingSky(Some(sky)))
        .insert_resource(world::water::PendingWater(Some(water)))
        .add_plugins((
            pause::PausePlugin,
            fly_camera::FlyCameraPlugin,
            world::WorldPlugin,
        ))
        .run();
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
