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
//!
//! halo3 <map> --zone N|NAME | --bsps 0,3,5 | --all-bsps
//!                        which structure BSPs to load first (default: the
//!                        zone set the player starts in; `world/zones.rs`)
//!
//! In a map, J switches between flying and skating (see `skate.rs`), B
//! between flying and the dirt bike (`bike/mod.rs`), PageDown / PageUp step
//! through the zone sets, Home loads every BSP.
//! ```
//!
//! `<map>` is looked up in `%HALO3_MAPS%`, or the default MCC install.

mod bike;
mod fly_camera;
mod rider;
mod shot;
mod skate;
mod swim;
mod textures;
mod world;

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::PresentMode;

pub(crate) const DEFAULT_MAPS: &str =
    r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps";

fn resolve_map(arg: &str) -> PathBuf {
    let p = PathBuf::from(arg);
    if p.exists() {
        return p;
    }
    let dir = std::env::var_os("HALO3_MAPS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MAPS));
    let mut name = arg.to_string();
    if !name.ends_with(".map") {
        name.push_str(".map");
    }
    dir.join(name)
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (scene, textures, chief, sky, water) = match args.first().map(String::as_str) {
        None | Some("--help") | Some("-h") => {
            eprintln!("usage:
  halo3 <map> [--zone N|NAME | --bsps 0,3,5 | --all-bsps]   e.g. riverworld, 120_halo
  halo3 <map> --shot DIR [--view x,y,z,yaw,pitch]... [--size WxH] [--skate | --bike]
  halo3 --test-scene");
            std::process::exit(2);
        }
        Some("--test-scene") => (
            world::test_scene(),
            textures::MaterialTextures::default(),
            None,
            world::sky::SkyData::default(),
            world::water::WaterData::default(),
        ),
        Some(map) => {
            let path = resolve_map(map);
            let started = std::time::Instant::now();
            let cache = blam_cache::CacheFile::open(&path)?;
            let mut scene = blam_geometry::Scene {
                name: cache.header.map_name.clone(),
                ..Default::default()
            };
            blam_geometry::decode_into(&cache, &blam_geometry::Options::default(), &mut scene)?;
            eprintln!(
                "{}: {} meshes, {} placements, {} triangles, decoded in {:.1} s",
                scene.name,
                scene.meshes.len(),
                scene.placements.len(),
                scene.triangle_count(),
                started.elapsed().as_secs_f32()
            );
            let started = std::time::Instant::now();
            let textures = textures::load(&cache, &scene);
            let bytes: usize = textures.bitmaps.values().map(|t| t.byte_len()).sum();
            eprintln!(
                "textures: {} colour bitmaps for {} materials, {:.1} MiB, loaded in {:.1} s",
                textures.bitmaps.len(),
                scene.materials.len(),
                bytes as f64 / (1 << 20) as f64,
                started.elapsed().as_secs_f32()
            );
            let mean = world::lighting::scene_mean_irradiance(&scene, &textures.lightmaps);
            if let Some(mean) = mean {
                world::lighting::set_scene_mean(mean);
            }
            match blam_geometry::camera_fx::read(&cache) {
                Ok(Some(fx)) => {
                    world::lighting::set_map_exposure(fx.exposure);
                    eprintln!(
                        "camera fx {}: exposure {:.2} stops, mean irradiance {:.3} -> x{:.2}",
                        fx.tag,
                        fx.exposure,
                        mean.unwrap_or(0.0),
                        world::lighting::exposure()
                    );
                }
                Ok(None) => eprintln!("no camera fx: exposure x{}", world::lighting::exposure()),
                Err(e) => eprintln!("camera fx: {e}; exposure x{}", world::lighting::exposure()),
            }
            let chief = rider::load_chief(&cache);
            let sky = world::sky::load(&cache, &scene);
            let water = world::water::load(&cache, &scene);
            (scene, textures, chief, sky, water)
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
                .disable::<bevy::winit::WinitPlugin>(),
        )
        .add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(
            std::time::Duration::from_millis(1),
        )),
        None => app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title,
                present_mode: PresentMode::AutoVsync,
                ..default()
            }),
            ..default()
        })),
    };
    let skating = shot.as_ref().is_none_or(|p| p.rides());
    let collision = scene.collision.clone();
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
        ))
        .insert_resource(rider::PendingChief(chief))
        .add_plugins((skate::SkatePlugin, rider::RiderPlugin, swim::SwimPlugin));
        if !collision.triangles.is_empty() {
            app.insert_resource(bike::BikeInput(collision))
                .add_plugins(bike::BikePlugin);
        }
        // Water surfaces and the ground under them, for sinking and
        // respawning (`swim.rs`).
        let mut water_materials: Vec<u32> = water
            .looks
            .iter()
            .filter(|(_, l)| matches!(l, world::water::WaterLook::Water(_)))
            .map(|(m, _)| *m)
            .collect();
        water_materials.sort_unstable();
        if !water_materials.is_empty() {
            app.insert_resource(swim::WaterWorld::build(&scene, &water_materials));
            if let Some(s) = scene.spawns.first() {
                app.insert_resource(swim::FallbackSpot(s.position, s.yaw));
            }
        }
    }
    app.insert_resource(zones)
        .insert_resource(loaded)
        .insert_resource(world::PendingScene(Some(scene), Some(textures)))
        .insert_resource(world::sky::PendingSky(Some(sky)))
        .insert_resource(world::water::PendingWater(Some(water)))
        .add_plugins((fly_camera::FlyCameraPlugin, world::WorldPlugin))
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
    }))
}
