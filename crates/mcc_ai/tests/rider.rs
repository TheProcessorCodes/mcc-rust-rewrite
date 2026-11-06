//! The run's enemies fight the player on the dirt bike, headless, on
//! 120_halo's own data: the AI and the weapons plugins in a bare app with
//! the bike's collision world of the Warthog run's BSPs, and the rider
//! stood in by `BikeMotion` (what `mcc_bike` publishes every frame).
//!
//! 1. A Sentinel squad (`trench_sentinel_100_1`) hears the bike going past
//!    behind it, turns, sees the rider, fires its beams at him and takes his
//!    shields and health.
//! 2. He dies on the bike, the bike's controls go dead, and after the wait
//!    he is sent back to his last quiet checkpoint on the bike with full
//!    shields and health.
//! 3. Riding into a Flood combat form (`trench_combat_forms01`) at 20 m/s
//!    kills it (the Mongoose's splatter numbers).
//!
//! One test: loading the map's AI, weapons and collision is the slow part.
//! Skips quietly when the map is not installed.

use std::sync::Arc;
use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use mcc_ai::{ActorActivity, ActorState, ActorTag, AiCensus, AiCommand, AiRef, AutoPlace, PendingAi, PlayerRespawn, PlayerVitality};
use mcc_bike::SharedWorld;
use mcc_bike::physics::World as Collision;
use mcc_core::bike::BikeMotion;
use mcc_core::coords::{to_halo, to_render};
use mcc_core::zones::LoadedBsps;

const DT: f32 = 1.0 / 30.0;
/// BSPs 100 and 110 (scenario BSPs 7 and 8): the run's first stretch.
const MASK: u32 = 0x180;

fn ground(world: &Collision, halo_x: f32, halo_y: f32) -> Vec3 {
    let from = to_render(Vec3::new(halo_x, halo_y, 0.0));
    world
        .raycast_far(from, Vec3::NEG_Y, 60.0 * 3.048)
        .unwrap_or_else(|| panic!("no ground under halo ({halo_x}, {halo_y})"))
        .point
}

/// Puts the stand-in bike with its wheels on `feet`, going `velocity`.
fn ride(app: &mut App, feet: Vec3, velocity: Vec3) {
    let mut m = app.world_mut().resource_mut::<BikeMotion>();
    m.riding = true;
    m.position = feet + Vec3::Y * mcc_bike::physics::rest_height();
    m.velocity = velocity;
    let flat = Vec3::new(velocity.x, 0.0, velocity.z);
    if flat.length() > 0.1 {
        m.heading = flat.x.atan2(flat.z);
    }
    m.upright = true;
    m.speed = velocity.length();
}

fn actors_of(app: &mut App, squad: &str) -> Vec<(Entity, Vec3, ActorState)> {
    let mut q = app.world_mut().query::<(Entity, &ActorTag, &Transform, &ActorActivity)>();
    q.iter(app.world())
        .filter(|(_, t, _, _)| t.squad_name == squad)
        .map(|(e, _, tr, a)| (e, tr.translation, a.state))
        .collect()
}

#[test]
fn enemies_fight_the_rider() {
    let Some(path) = game_paths::halo3_map("120_halo") else { return };
    let cache = blam_cache::CacheFile::open(&path).expect("120_halo opens");
    let options = blam_geometry::Options {
        bsps: Some(vec![7, 8]),
        ..Default::default()
    };
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(&cache, &options, &mut scene).expect("BSPs decode");
    blam_geometry::objects::add_collision(&cache, &mut scene);
    let ai = mcc_ai::load(&cache).expect("120_halo has AI");
    let splatter = ai.splatter.clone().expect("the Mongoose's collision damage is read");
    let arsenal = mcc_weapons::assets::load_with(&cache, &ai.weapon_tags());

    let mut shared = SharedWorld::default();
    let world: Arc<Collision> = shared.wait(&scene.collision, MASK);

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::input::InputPlugin,
        mcc_core::map::MapPhasePlugin,
    ))
    .init_asset::<Mesh>()
    .init_asset::<Image>()
    .init_asset::<StandardMaterial>()
    .init_asset::<SkinnedMeshInverseBindposes>()
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(DT)))
    .init_resource::<mcc_core::pause::Paused>()
    .init_resource::<mcc_foot::OnFoot>()
    .init_resource::<BikeMotion>()
    .init_resource::<mcc_core::skate::SkateMotion>()
    .init_resource::<mcc_core::skate::RiderPose>()
    .insert_resource(shared)
    .insert_resource(LoadedBsps {
        mask: MASK,
        shown: MASK,
        zone_set: Some(4),
        sky: None,
    })
    .add_plugins((mcc_weapons::WeaponsPlugin, mcc_ai::AiPlugin))
    .insert_resource(PendingAi(Some(ai)))
    .insert_resource(mcc_weapons::PendingArsenal(arsenal))
    .insert_resource(AutoPlace::Off)
    .insert_resource(mcc_core::map::MapInstalled);
    // Start-up, then play begins.
    for _ in 0..3 {
        app.update();
    }

    // ---- 1. Sentinels fire on the rider going past. ----
    let a = ground(&world, 62.0, -170.0);
    let b = ground(&world, 62.0, -130.0);
    ride(&mut app, a, Vec3::ZERO);
    app.world_mut().write_message(AiCommand::Place {
        who: AiRef::named("trench_sentinel_100_1"),
        count: None,
    });
    app.update();
    let placed = app.world().resource::<AiCensus>().living_count("trench_sentinel_100_1");
    assert!(placed > 0, "sentinels placed");
    let full = {
        let v = app.world().resource::<PlayerVitality>();
        v.shield + v.body
    };
    let speed = 8.0;
    let (mut t, mut fired, mut hurt_at) = (0.0f32, false, None);
    let mut forward = true;
    let mut at = a;
    while t < 30.0 {
        let to = if forward { b } else { a };
        let d = to - at;
        if d.length() < 1.0 {
            forward = !forward;
        }
        let v = d.normalize_or_zero() * speed;
        at += v * DT;
        let feet = ground(&world, to_halo(at).x, to_halo(at).y);
        at = feet;
        ride(&mut app, feet, v);
        app.update();
        t += DT;
        fired |= actors_of(&mut app, "trench_sentinel_100_1").iter().any(|(_, _, s)| *s == ActorState::Firing);
        let v = app.world().resource::<PlayerVitality>();
        if hurt_at.is_none() && v.shield + v.body < full - 1.0 {
            hurt_at = Some(t);
        }
        if v.dead.is_some() {
            break;
        }
    }
    let launched = app.world().resource::<mcc_weapons::Shots>().launched;
    eprintln!("sentinels: {placed} placed, fired {fired}, {launched} rounds, first hurt at {hurt_at:?} s");
    assert!(fired, "a sentinel fired at the rider");
    let hurt_at = hurt_at.expect("the sentinels hurt the rider");
    assert!(hurt_at > 0.5, "not at once: {hurt_at}");

    // ---- 2. He dies on the bike and comes back at his checkpoint. ----
    let mut died = app.world().resource::<PlayerVitality>().dead.is_some();
    while !died && t < 120.0 {
        ride(&mut app, at, Vec3::ZERO);
        app.update();
        t += DT;
        died = app.world().resource::<PlayerVitality>().dead.is_some();
    }
    eprintln!("the rider died at {t:.1} s");
    assert!(died, "the sentinels killed the rider within 120 s");
    app.update();
    assert!(app.world().resource::<PlayerRespawn>().dead, "the bike's controls go dead");
    let mut back = None;
    for _ in 0..(5.0 / DT) as usize {
        app.update();
        if let Some(r) = app.world_mut().resource_mut::<PlayerRespawn>().bike.take() {
            back = Some(r);
            break;
        }
    }
    let (spot, _) = back.expect("the bike is sent back to the checkpoint");
    let v = app.world().resource::<PlayerVitality>();
    assert!(v.dead.is_none() && v.body >= v.max_body && v.shield >= v.max_shield, "full health again");
    assert!(!app.world().resource::<PlayerRespawn>().dead);
    assert!(spot.distance(a) < 60.0, "the checkpoint is on the run: {spot}");
    eprintln!("back at halo {:?}", to_halo(spot));

    // ---- 3. Riding into a Flood combat form kills it. ----
    app.world_mut().write_message(AiCommand::Erase {
        who: AiRef::named("trench_sentinel_100_1"),
    });
    app.world_mut().write_message(AiCommand::Place {
        who: AiRef::named("trench_combat_forms01"),
        count: None,
    });
    app.world_mut().resource_mut::<PlayerVitality>().god = true;
    app.update();
    let forms = actors_of(&mut app, "trench_combat_forms01");
    assert!(!forms.is_empty(), "combat forms placed");
    let (target, start, _) = forms[0];
    let mut at = ground(&world, to_halo(start).x, to_halo(start).y - 7.0);
    let speed = 20.0;
    let mut killed = false;
    for _ in 0..(3.0 / DT) as usize {
        let Some((_, now, state)) = actors_of(&mut app, "trench_combat_forms01").into_iter().find(|(e, _, _)| *e == target) else {
            killed = true;
            break;
        };
        if matches!(state, ActorState::Dying | ActorState::Dead) {
            killed = true;
            break;
        }
        let d = Vec3::new(now.x - at.x, 0.0, now.z - at.z);
        let v = d.normalize_or(Vec3::Z) * speed;
        at += v * DT;
        at.y = now.y;
        ride(&mut app, at, v);
        app.update();
    }
    assert!(killed, "the bike at 20 m/s killed the combat form ({splatter:?})");
}
