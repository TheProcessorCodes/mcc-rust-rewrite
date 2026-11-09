//! The session marker the board (`mcc_skate`) and the bike (`mcc_bike`)
//! share, and its flag: a small red flag on a pole stuck in the ground where
//! the last marker was set. One at a time: setting a marker on either ride
//! moves it, and either ride goes back to it (LB + d-pad up held), so a
//! marker set on the bike takes the board back there too, and the other way
//! round. The flag stays up through every mode (flying, on foot, skating,
//! riding).

use bevy::prelude::*;
use crate::map::MapAppExt;
use crate::skate::Ride;

/// The last session marker set, and the ride that set it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marker {
    /// Where the flag's pole meets the ground, renderer frame (metres).
    pub ground: Vec3,
    /// The way the ride faced: a heading about +Y, forward being
    /// `(sin, 0, cos)` (the bike's `Bike::heading`, the skate engine's
    /// `Session::activate`).
    pub heading: f32,
    pub by: Ride,
}

/// The shared session marker; `None` until one is set on this map.
#[derive(Resource, Default)]
pub struct SessionFlag {
    marker: Option<Marker>,
    /// Counts the markers set, so a ride can tell a new one from the one it
    /// already has.
    generation: u64,
}

impl SessionFlag {
    /// A marker set on `marker.by`: the flag moves there, and the other
    /// ride takes it up through [`SessionFlag::set_since`].
    pub fn set(&mut self, marker: Marker) {
        self.marker = Some(marker);
        self.generation += 1;
    }

    pub fn marker(&self) -> Option<Marker> {
        self.marker
    }

    /// The marker the other ride set since `seen` was last brought up to
    /// date, for `me` to go back to; `None` when nothing new was set or
    /// `me` set it. Brings `seen` up to date.
    pub fn set_since(&self, seen: &mut u64, me: Ride) -> Option<Marker> {
        if *seen == self.generation {
            return None;
        }
        *seen = self.generation;
        self.marker.filter(|m| m.by != me)
    }
}

const POLE_HEIGHT: f32 = 0.48;
const POLE_RADIUS: f32 = 0.008;
/// Pole below the ground, hiding a small error in where the ground is.
const POLE_SUNK: f32 = 0.15;
const FLAG_SIZE: Vec2 = Vec2::new(0.15, 0.096);

pub struct SessionFlagPlugin;

impl Plugin for SessionFlagPlugin {
    fn build(&self, app: &mut App) {
        app.init_map_resource::<SessionFlag>()
            .add_systems(crate::map::MapStartup, spawn_flag)
            .add_systems(Update, place_flag.run_if(crate::map::ready));
    }
}

#[derive(Component)]
struct FlagRoot;

fn spawn_flag(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let unlit = |r, g, b| StandardMaterial {
        base_color: Color::srgb(r, g, b),
        unlit: true,
        ..default()
    };
    let pole_length = POLE_HEIGHT + POLE_SUNK;
    commands
        .spawn((FlagRoot, Transform::default(), Visibility::Hidden))
        .with_children(|root| {
            root.spawn((
                Mesh3d(meshes.add(Cylinder::new(POLE_RADIUS, pole_length))),
                MeshMaterial3d(materials.add(unlit(0.75, 0.75, 0.72))),
                Transform::from_xyz(0.0, pole_length / 2.0 - POLE_SUNK, 0.0),
            ));
            root.spawn((
                Mesh3d(meshes.add(Cuboid::new(FLAG_SIZE.x, FLAG_SIZE.y, 0.01))),
                MeshMaterial3d(materials.add(unlit(0.85, 0.05, 0.05))),
                Transform::from_xyz(
                    POLE_RADIUS + FLAG_SIZE.x / 2.0,
                    POLE_HEIGHT - FLAG_SIZE.y / 2.0,
                    0.0,
                ),
            ));
        });
}

fn place_flag(
    flag: Res<SessionFlag>,
    mut roots: Query<(&mut Transform, &mut Visibility), With<FlagRoot>>,
) {
    if !flag.is_changed() {
        return;
    }
    for (mut transform, mut visibility) in &mut roots {
        match flag.marker {
            Some(m) => {
                transform.translation = m.ground;
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(x: f32, by: Ride) -> Marker {
        Marker {
            ground: Vec3::new(x, 0.0, 0.0),
            heading: 0.5,
            by,
        }
    }

    #[test]
    fn each_ride_takes_up_the_other_rides_marker_once() {
        let mut flag = SessionFlag::default();
        let (mut bike, mut board) = (0, 0);
        assert_eq!(flag.set_since(&mut bike, Ride::Bike), None);
        assert_eq!(flag.set_since(&mut board, Ride::Board), None);

        // Set on the bike: the board takes it up, the bike already has it.
        flag.set(marker(1.0, Ride::Bike));
        assert_eq!(flag.set_since(&mut bike, Ride::Bike), None);
        assert_eq!(flag.set_since(&mut board, Ride::Board), Some(marker(1.0, Ride::Bike)));
        assert_eq!(flag.set_since(&mut board, Ride::Board), None);

        // Set twice on the board before the bike looks: the bike takes the latest.
        flag.set(marker(2.0, Ride::Board));
        flag.set(marker(3.0, Ride::Board));
        assert_eq!(flag.set_since(&mut board, Ride::Board), None);
        assert_eq!(flag.set_since(&mut bike, Ride::Bike), Some(marker(3.0, Ride::Board)));
        assert_eq!(flag.set_since(&mut bike, Ride::Bike), None);
        assert_eq!(flag.marker().map(|m| m.ground.x), Some(3.0));
    }
}
