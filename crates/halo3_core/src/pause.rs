//! Whether the game is paused. The menu itself is `halo3`'s `pause.rs`;
//! this is the flag every input system checks.

use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct Paused(pub bool);

/// A run condition for systems that should stop behind the menu, and
/// wait behind the loading screen until play begins
/// ([`crate::map::Playing`]).
pub fn running(paused: Res<Paused>, playing: Option<Res<crate::map::Playing>>) -> bool {
    !paused.0 && playing.is_some()
}


/// A menu drawn over the game without pausing it (the dirt bike's tuning
/// menu) is up: it has the mouse, and Escape closes it, not the game.
#[derive(Resource, Default)]
pub struct Overlay(pub bool);

/// The systems that open and close overlay menus. The pause menu reads
/// Escape after them, so an Escape that closed an overlay is gone by then.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct OverlayInput;
