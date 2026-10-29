//! Whether the game is paused. The menu itself is `halo3`'s `pause.rs`;
//! this is the flag every input system checks.

use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct Paused(pub bool);

/// A run condition for systems that should stop behind the menu.
pub fn running(paused: Res<Paused>) -> bool {
    !paused.0
}

/// The pause menu's key handling (`halo3`'s `pause.rs`), for menus that must
/// see Escape first (the bike tuning menu runs before it).
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PauseKeys;
