//! What the weapons do, as messages for other systems (sound, effects).
//! Positions are in the renderer frame (metres, Y up).

use bevy::prelude::*;

/// What happened to a weapon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponAction {
    /// A round left the barrel (one message per shot; a burst sends several).
    Fire,
    /// The trigger was pulled on an empty weapon.
    Empty,
    /// A reload started.
    Reload,
    /// The weapon was picked up or switched to.
    Ready,
    /// A charged weapon started charging (plasma pistol, Spartan laser).
    Charge,
}

/// A weapon did something. `weapon` is the `weap` tag's index in the map
/// (`blam_geometry::weapons::Weapon::tag_index`).
#[derive(Message, Clone, Copy, Debug)]
pub struct WeaponEvent {
    pub weapon: u32,
    pub action: WeaponAction,
    /// Where: the muzzle for `Fire`, the hand otherwise.
    pub at: Vec3,
    /// The player's own weapon (heard close, not placed in the world).
    pub own: bool,
}

/// A round hit the world.
#[derive(Message, Clone, Copy, Debug)]
pub struct WeaponImpact {
    /// The `weap` tag that fired it.
    pub weapon: u32,
    pub at: Vec3,
    /// The surface's normal.
    pub normal: Vec3,
}
