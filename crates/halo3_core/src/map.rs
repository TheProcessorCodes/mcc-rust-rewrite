//! A map as a phase of the app. The window can open on the map menu with no
//! map at all; the map's systems wait until one is in. Loading a map puts
//! its resources in the world and adds [`MapInstalled`]; the next frame runs
//! [`MapStartup`] then [`MapPostStartup`] once (what `Startup` is for an app
//! that opens on its map) and adds [`MapReady`], which [`ready`] tests.
//!
//! Every system of the map crates is registered with `.run_if(ready)`, and
//! their start-up systems go in [`MapStartup`] instead of `Startup`.
//!
//! [`unload`] takes the map out again (back to the map menu): it despawns
//! everything drawn in the world or on screen, removes the resources the map
//! put in and puts the plugins' own back to their first state, as each
//! plugin registered them with [`MapAppExt`].

use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;

/// The map's resources are in the world; its start-up has not run yet.
#[derive(Resource)]
pub struct MapInstalled;

/// The map's start-up has run: its systems run from now on.
#[derive(Resource)]
pub struct MapReady;

/// The app has the map menu: leaving a map goes back to it instead of
/// closing the game.
#[derive(Resource)]
pub struct MapMenu;

/// Asks for the map to be unloaded and the map menu shown (the menu does
/// it at the end of the frame).
#[derive(Resource)]
pub struct BackToMenu;

/// Run once when the map is in, in place of `Startup`.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct MapStartup;

/// Run once after [`MapStartup`], in place of `PostStartup`.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct MapPostStartup;

/// A run condition: a map is in and started.
pub fn ready(ready: Option<Res<MapReady>>) -> bool {
    ready.is_some()
}

/// What puts each per-map resource back to its first state on [`unload`].
#[derive(Resource, Default)]
struct MapResets(Vec<fn(&mut World)>);

pub trait MapAppExt {
    /// `init_resource`, and the same again each time a map is unloaded.
    fn init_map_resource<T: Resource + FromWorld>(&mut self) -> &mut Self;
    /// Removes the resource each time a map is unloaded (one the map's
    /// start-up or systems insert).
    fn remove_on_map_unload<T: Resource>(&mut self) -> &mut Self;
    /// Runs `reset` each time a map is unloaded.
    fn on_map_unload(&mut self, reset: fn(&mut World)) -> &mut Self;
}

impl MapAppExt for App {
    fn init_map_resource<T: Resource + FromWorld>(&mut self) -> &mut Self {
        self.init_resource::<T>().on_map_unload(reset::<T>)
    }

    fn remove_on_map_unload<T: Resource>(&mut self) -> &mut Self {
        self.on_map_unload(remove::<T>)
    }

    fn on_map_unload(&mut self, reset: fn(&mut World)) -> &mut Self {
        self.world_mut().get_resource_or_init::<MapResets>().0.push(reset);
        self
    }
}

fn remove<T: Resource>(world: &mut World) {
    world.remove_resource::<T>();
}

fn reset<T: Resource + FromWorld>(world: &mut World) {
    world.remove_resource::<T>();
    world.init_resource::<T>();
}

pub struct MapPhasePlugin;

impl Plugin for MapPhasePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MapResets>()
            .init_schedule(MapStartup)
            .init_schedule(MapPostStartup)
            .add_systems(First, start_map);
    }
}

fn start_map(world: &mut World) {
    if !world.contains_resource::<MapInstalled>() || world.contains_resource::<MapReady>() {
        return;
    }
    world.run_schedule(MapStartup);
    world.run_schedule(MapPostStartup);
    world.insert_resource(MapReady);
}

/// Takes the map out: its entities, its resources, its sound state. The
/// app is then as it was before the map went in, with no map systems
/// running.
pub fn unload(world: &mut World) {
    // Everything placed in the world or on screen: the map's meshes, its
    // cameras, lights, the rider, the HUD and menus. Windows, gamepads,
    // observers and the sound output have none of these.
    let roots: Vec<Entity> = world
        .query_filtered::<Entity, (Or<(With<Transform>, With<Node>, With<Camera>)>, Without<ChildOf>)>()
        .iter(world)
        .collect();
    let count = roots.len();
    for e in roots {
        if let Ok(e) = world.get_entity_mut(e) {
            e.despawn();
        }
    }
    // The resources the map put in, and the ones every map starts from
    // fresh (registered by the plugins with `MapAppExt`).
    let resets = world.resource::<MapResets>().0.clone();
    for reset in resets {
        reset(world);
    }
    world.remove_resource::<MapInstalled>();
    world.remove_resource::<MapReady>();
    world.resource_mut::<Time<Virtual>>().unpause();
    eprintln!("map unloaded: {count} entities despawned");
}
