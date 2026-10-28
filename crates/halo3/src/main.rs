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
//!
//! In a map, J switches between flying and skating (see `skate.rs`).
//! ```
//!
//! `<map>` is looked up in `%HALO3_MAPS%`, or the default MCC install.

mod fly_camera;
mod rider;
mod shot;
mod skate;
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
    let (scene, textures, chief, sky) = match args.first().map(String::as_str) {
        None | Some("--help") | Some("-h") => {
            eprintln!("usage:
  halo3 <map>          e.g. riverworld
  halo3 --test-scene");
            std::process::exit(2);
        }
        Some("--test-scene") => (
            world::test_scene(),
            textures::MaterialTextures::default(),
            None,
            world::sky::SkyData::default(),
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
            let chief = rider::load_chief(&cache);
            let sky = world::sky::load(&cache, &scene);
            (scene, textures, chief, sky)
        }
    };
    let shot = shot_plan(&args, &scene.name)?;
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
    let skating = shot.as_ref().is_none_or(|p| p.skate);
    if let Some(plan) = shot {
        if plan.skate {
            // Ride from the first view, or the start spawn.
            let start = match plan.views.first() {
                Some(v) => Some((v.position, v.yaw_deg.to_radians())),
                None => scene.spawns.first().map(|s| (s.position, s.yaw)),
            };
            if let Some((position, yaw)) = start {
                app.insert_resource(skate::AutoStart::at(position, yaw));
            }
        }
        app.add_plugins(shot::ShotPlugin(plan));
    }
    if skating {
        app.insert_resource(skate::SkateInput(
            (!scene.collision.triangles.is_empty()).then(|| scene.collision.clone()),
        ))
        .insert_resource(rider::PendingChief(chief))
        .add_plugins((skate::SkatePlugin, rider::RiderPlugin));
    }
    app.insert_resource(world::PendingScene(Some(scene), Some(textures)))
        .insert_resource(world::sky::PendingSky(Some(sky)))
        .add_plugins((fly_camera::FlyCameraPlugin, world::WorldPlugin))
        .run();
    Ok(())
}

/// `--shot <dir>` with its `--view` and `--size` options, if given.
fn shot_plan(args: &[String], name: &str) -> anyhow::Result<Option<shot::ShotPlan>> {
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1));
    let Some(dir) = value("--shot") else {
        return Ok(None);
    };
    let views = args
        .windows(2)
        .filter(|w| w[0] == "--view")
        .map(|w| w[1].parse::<shot::View>().map_err(anyhow::Error::msg))
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
        name: name.to_string(),
        views,
        size,
        skate: args.iter().any(|a| a == "--skate"),
    }))
}
