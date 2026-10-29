//! Weapons: the map's guns lying where the scenario puts them, the one in
//! the Spartan's hands, and shooting it.

pub mod events;

use bevy::prelude::*;

pub use events::{WeaponAction, WeaponEvent, WeaponImpact};

pub struct WeaponsPlugin;

impl Plugin for WeaponsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<WeaponEvent>().add_message::<WeaponImpact>();
    }
}
