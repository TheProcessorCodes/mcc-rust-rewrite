//! Debug drawing with gizmos (feature `gizmos`): a stick model of every bike (chassis box, rider, wheels with a
//! spoke, fork and swingarm) and, per wheel on the ground, its contact normal coloured by grip (green holds,
//! orange slides). Add [`ReflexDebugDrawPlugin`]; toggle with the [`ReflexDebugDraw`] resource.

use bevy::color::palettes::css;
use bevy::prelude::*;

use crate::{Part, ReflexBike, ReflexBikeState, ReflexSet};

/// Draws every bike with gizmos after [`ReflexSet::Sync`].
pub struct ReflexDebugDrawPlugin;

impl Plugin for ReflexDebugDrawPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ReflexDebugDraw>().add_systems(Update, draw.after(ReflexSet::Sync));
    }
}

/// What the debug drawing shows.
#[derive(Resource, Clone, Copy, Debug)]
pub struct ReflexDebugDraw {
    pub bike: bool,
    pub contacts: bool,
}

impl Default for ReflexDebugDraw {
    fn default() -> Self {
        ReflexDebugDraw { bike: true, contacts: true }
    }
}

fn draw(cfg: Res<ReflexDebugDraw>, bikes: Query<(&ReflexBike, &ReflexBikeState)>, mut gizmos: Gizmos) {
    for (bike, state) in &bikes {
        if cfg.bike {
            let chassis = bike.part_transform(Part::Chassis);
            let colour = if state.crashed { css::RED } else { css::ORANGE };
            gizmos.cube(chassis.with_scale(Vec3::new(0.28, 0.35, 1.15)), colour);
            let rider = bike.part_transform(Part::Rider);
            gizmos.sphere(Isometry3d::new(rider.translation, rider.rotation), 0.2, css::STEEL_BLUE);
            for (part, r) in [(Part::FrontWheel, bike.wheel_radii()[0]), (Part::RearWheel, bike.wheel_radii()[1])] {
                let w = bike.part_transform(part);
                // the wheel turns about its local X: circle in the local YZ plane, plus one spoke to show the spin
                let iso = Isometry3d::new(w.translation, w.rotation * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
                gizmos.circle(iso, r, css::WHITE);
                gizmos.line(w.translation, w.translation + w.rotation * Vec3::Y * r, css::LIGHT_GRAY);
            }
            let steering = bike.part_transform(Part::Steering);
            gizmos.line(steering.translation, bike.part_transform(Part::FrontHub).translation, css::SILVER);
            let bar = steering.rotation * Vec3::X * 0.4;
            gizmos.line(steering.translation - bar, steering.translation + bar, css::SILVER);
            let swingarm = bike.part_transform(Part::SwingArm);
            gizmos.line(swingarm.translation, bike.part_transform(Part::RearHub).translation, css::SILVER);
        }
        if cfg.contacts {
            for w in state.wheels.iter().filter(|w| w.contact) {
                let c = if w.side_grip { css::LIME } else { css::ORANGE_RED };
                gizmos.line(w.point, w.point + w.normal * (0.2 + w.load / 2000.0), c);
            }
        }
    }
}
