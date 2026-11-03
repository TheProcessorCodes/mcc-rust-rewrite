//! The Flood on the Warthog run, the way the game starts it: 120_halo's own
//! scripts from `ins_trench_run` place the squads through the AI bridge as
//! the player rides through the run's trigger volumes in order, and the AI
//! (with the weapons) runs in a bare app on the mission's collision (BSPs
//! 040, 100, 110). Checks the Flood squads the scripts place along the
//! first stretch are there, alive, standing on something and drawn, while
//! the rider is near them.
//!
//! Skips quietly when the map is not installed.

use std::sync::Arc;
use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::mesh::skinning::SkinnedMeshInverseBindposes;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use halo3_ai::{ActorActivity, ActorState, ActorTag, AiBridge, AiCensus, AutoPlace, PendingAi};
use halo3_bike::SharedWorld;
use halo3_bike::physics::World as Collision;
use halo3_core::bike::BikeMotion;
use halo3_core::coords::{to_halo, to_render};
use halo3_core::zones::LoadedBsps;
use halo3_script::vm::{Config, WorldApi};
use halo3_script::{BridgeAi, setup};

const DT: f32 = 1.0 / 30.0;
/// The mission's first zone set, `120_04_100_110`: BSPs 4, 7 and 8.
const MASK: u32 = 0x190;

struct Player {
    at: Vec3,
    zone_set: usize,
}

impl WorldApi for Player {
    fn player_position(&self) -> Option<Vec3> {
        Some(self.at)
    }
    fn teleport_player(&mut self, position: Vec3, _yaw: f32) {
        self.at = position;
    }
    fn current_zone_set(&self) -> Option<usize> {
        Some(self.zone_set)
    }
    fn switch_zone_set(&mut self, zone_set: usize) {
        self.zone_set = zone_set;
    }
    fn effect(&mut self, _tag: &str, _at: Vec3) {}
}

/// Everything about one Flood actor that says whether the rider can see it.
#[derive(Debug)]
struct Seen {
    squad: String,
    halo: Vec3,
    state: ActorState,
    visible: bool,
    /// Metres down to the floor under its feet (`None`: nothing below).
    floor: Option<f32>,
}

fn flood(app: &mut App, world: &Collision) -> Vec<Seen> {
    let mut q = app.world_mut().query::<(&ActorTag, &Transform, &ActorActivity, &Visibility)>();
    q.iter(app.world())
        .filter(|(t, ..)| t.team == halo3_core::ai::team::FLOOD)
        .map(|(t, tr, a, v)| Seen {
            squad: t.squad_name.clone(),
            halo: to_halo(tr.translation),
            state: a.state,
            visible: *v != Visibility::Hidden,
            floor: world
                .raycast_far(tr.translation + Vec3::Y * 0.5, Vec3::NEG_Y, 30.0)
                .map(|h| tr.translation.y - h.point.y),
        })
        .collect()
}

#[test]
fn flood_on_the_warthog_run() {
    let Some(path) = game_paths::halo3_map("120_halo") else { return };
    let cache = blam_cache::CacheFile::open(&path).expect("120_halo opens");
    let options = blam_geometry::Options {
        bsps: Some(vec![4, 7, 8]),
        ..Default::default()
    };
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(&cache, &options, &mut scene).expect("BSPs decode");
    blam_geometry::objects::add_collision(&cache, &mut scene);
    let data = Arc::new(blam_script::ScriptData::read(&cache).expect("scripts"));
    setup::mark_dynamic(&mut scene, &data);
    let mut rt = setup::runtime(&cache, &scene, Arc::clone(&data), Config { editor: true, ..Default::default() });
    let ai = halo3_ai::load(&cache).expect("120_halo has AI");
    let arsenal = halo3_weapons::assets::load_with(&cache, &ai.weapon_tags());
    let mut shared = SharedWorld::default();
    let world: Arc<Collision> = shared.wait(&scene.collision, MASK);

    // The run's early infection and carrier squads (and the patrol points
    // their command scripts swarm to) stand in the open air over the
    // trench, with no geometry under them in the mission's BSPs: they are
    // not something the rider can meet (see HANDOFF).
    for at in [Vec3::new(1.12, 16.02, 3.64), Vec3::new(-20.63, -5.28, 2.88), Vec3::new(-10.76, -2.01, 5.8)] {
        assert!(world.raycast_far(to_render(at) + Vec3::Y, Vec3::NEG_Y, 300.0).is_none(), "ground under {at}");
    }
    let bridge = AiBridge::default();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::log::LogPlugin::default(), AssetPlugin::default(), bevy::input::InputPlugin, halo3_core::map::MapPhasePlugin))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_asset::<SkinnedMeshInverseBindposes>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(DT)))
        .init_resource::<halo3_core::pause::Paused>()
        .init_resource::<halo3_foot::OnFoot>()
        .init_resource::<BikeMotion>()
        .init_resource::<halo3_core::skate::SkateMotion>()
        .init_resource::<halo3_core::skate::RiderPose>()
        .insert_resource(shared)
        .insert_resource(LoadedBsps {
            mask: MASK,
            shown: MASK,
            zone_set: Some(4),
            sky: None,
        })
        .add_plugins((halo3_weapons::WeaponsPlugin, halo3_ai::AiPlugin))
        .insert_resource(PendingAi(Some(ai)))
        .insert_resource(halo3_weapons::PendingArsenal(arsenal))
        .insert_resource(AutoPlace::Off)
        .insert_resource(halo3_core::map::MapInstalled);
    for _ in 0..3 {
        app.update();
    }
    // The script side holds the same bridge the plugin reads.
    app.insert_resource(bridge.clone());
    app.world_mut().resource_mut::<halo3_core::ai::PlayerVitality>().god = true;
    let mut hooks = BridgeAi::new(bridge);

    assert!(rt.console("ins_trench_run"));
    let mut player = Player {
        at: Vec3::new(-263.64, -433.77, -5.05),
        zone_set: 0,
    };
    let centre = |name: &str| {
        let v = scene.trigger_volumes.iter().find(|v| v.name == name).unwrap_or_else(|| panic!("{name}"));
        let left = v.up.cross(v.forward);
        v.position + v.forward * v.extents.x * 0.5 + left * v.extents.y * 0.5 + v.up * v.extents.z * 0.5
    };
    // One tick of the scripts and one frame of the game, the rider at the
    // player's place on the floor under it.
    let mut tick = |app: &mut App, rt: &mut halo3_script::Runtime, player: &mut Player| {
        rt.step(player, &mut hooks, None);
        let feet = world
            .raycast_far(to_render(player.at) + Vec3::Y * 2.0, Vec3::NEG_Y, 40.0)
            .map_or(to_render(player.at), |h| h.point);
        let mut m = app.world_mut().resource_mut::<BikeMotion>();
        m.riding = true;
        m.position = feet + Vec3::Y * halo3_bike::physics::rest_height();
        m.velocity = Vec3::ZERO;
        m.upright = true;
        app.update();
    };
    for _ in 0..90 {
        tick(&mut app, &mut rt, &mut player);
    }
    let route: &[(&str, f32)] = &[
        ("trench_run_plate_100_trig", 1.0),
        ("trench_run_100_trig00", 1.0),
        ("trench_run_100_trig01", 1.0),
        ("trench_run_plate_100_hw_trig", 1.0),
        ("trench_run_100_trig04", 1.0),
        ("100_game_save_trig", 4.0),
        ("100_island_trig00", 4.0),
        ("100_island_trig01", 4.0),
        ("trench_run_100_island_trig01", 4.0),
        ("100_island_trig02", 62.0),
        ("trench_run_100_trig07", 1.0),
        ("trench_run_100_trig08", 6.0),
        // The tunnel's fight is placed here; riding to it at about 90 km/h
        // takes some six seconds.
        ("tunnel01_trig01", 6.0),
        ("tunnel01_trig03", 2.0),
    ];
    let mut report = Vec::new();
    for (volume, dwell) in route {
        player.at = centre(volume);
        for _ in 0..(dwell / DT) as usize {
            tick(&mut app, &mut rt, &mut player);
        }
        let census = app.world().resource::<AiCensus>();
        let placed: Vec<String> = ["trench_infection_100", "trench_infection_100_03", "trench_carrier_100_01", "trench_infection_100_02", "trench_combat_forms01", "trench_combat_forms02", "trench_combat_forms03", "trench_combat_forms04"]
            .iter()
            .map(|s| format!("{s} {}/{}", census.living_count(s), census.spawn_count(s)))
            .collect();
        {
            let mut q = app.world_mut().query::<(&ActorTag, &Transform, &ActorActivity)>();
            for (t, tr, a) in q.iter(app.world()) {
                if t.team == halo3_core::ai::team::SENTINEL {
                    eprintln!("    sentinel {} at {:?} {:?}", t.squad_name, to_halo(tr.translation), a.state);
                }
            }
        }
        let here = flood(&mut app, &world);
        eprintln!("at {volume} (halo {:?}): {}", player.at, placed.join(", "));
        for s in &here {
            eprintln!("    {} at {:?} {:?} visible {} floor {:?}", s.squad, s.halo, s.state, s.visible, s.floor);
        }
        report.push((volume.to_string(), player.at, here));
    }
    // The Flood the scripts place in the first tunnel (`trench_combat_forms01..03`,
    // amid `trench_sentinel_100_4`) are still fighting when the rider gets
    // there: alive, drawn, on the floor and near him.
    let (_, at, here) = report.iter().find(|(v, ..)| v == "tunnel01_trig03").expect("reached the tunnel");
    let live: Vec<&Seen> = here
        .iter()
        .filter(|s| s.squad.starts_with("trench_combat_forms0") && s.squad != "trench_combat_forms04")
        .filter(|s| !matches!(s.state, ActorState::Dying | ActorState::Dead))
        .filter(|s| s.visible && s.floor.is_some_and(|f| f > -0.5 && f < 3.0) && s.halo.distance(*at) < 40.0)
        .collect();
    eprintln!("{} combat forms alive by the rider in the tunnel", live.len());
    assert!(live.len() >= 4, "only {} live combat forms by the rider in the tunnel", live.len());
    // The next tunnel's squad is placed ahead of him, standing.
    let (_, _, here) = report.last().unwrap();
    assert!(
        here.iter().any(|s| s.squad == "trench_combat_forms04" && s.visible && s.floor.is_some_and(|f| f < 3.0)),
        "trench_combat_forms04 placed and standing"
    );
}
