//! The session marker's flag: a small red flag on a pole stuck in the ground
//! where the last session marker was set, on the board (`halo3_skate`) or
//! the bike (`halo3_bike`). One at a time: setting a marker moves it. It
//! stays up through every mode (flying, on foot, skating, riding).

use bevy::prelude::*;

/// Where the flag's pole meets the ground, renderer frame (metres); `None`
/// until a marker is set.
#[derive(Resource, Default)]
pub struct SessionFlag(pub Option<Vec3>);

const POLE_HEIGHT: f32 = 0.48;
const POLE_RADIUS: f32 = 0.008;
/// Pole below the ground, hiding a small error in where the ground is.
const POLE_SUNK: f32 = 0.15;
const FLAG_SIZE: Vec2 = Vec2::new(0.15, 0.096);

pub struct SessionFlagPlugin;

impl Plugin for SessionFlagPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SessionFlag>()
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
        match flag.0 {
            Some(at) => {
                transform.translation = at;
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}
