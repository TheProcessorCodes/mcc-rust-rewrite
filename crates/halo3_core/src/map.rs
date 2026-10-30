//! A map as a phase of the app. The window can open on the map menu with no
//! map at all; the map's systems wait until one is in. Loading a map puts
//! its resources in the world and adds [`MapInstalled`]; the next frame runs
//! [`MapStartup`] then [`MapPostStartup`] once (what `Startup` is for an app
//! that opens on its map) and adds [`MapReady`], which [`ready`] tests.
//!
//! Every system of the map crates is registered with `.run_if(ready)`, and
//! their start-up systems go in [`MapStartup`] instead of `Startup`.

use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;

/// The map's resources are in the world; its start-up has not run yet.
#[derive(Resource)]
pub struct MapInstalled;

/// The map's start-up has run: its systems run from now on.
#[derive(Resource)]
pub struct MapReady;

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

pub struct MapPhasePlugin;

impl Plugin for MapPhasePlugin {
    fn build(&self, app: &mut App) {
        app.init_schedule(MapStartup)
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
