//! The Spartan's melee: Q (B on a pad) strikes what stands in front of him,
//! an actor or the enemy Spartan. Something a little further off within
//! the lunge range and in front pulls him in towards it, as Halo's melee
//! lunge does, and the blow lands. It takes the weapon out of action for a
//! moment (no firing, a reload stops).
//!
//! The numbers are ours: Halo keeps a melee's damage in the biped's melee
//! damage effect and its reach and lunge in the globals, which are not read
//! for the player.

use bevy::input::gamepad::GamepadButton as B;
use bevy::prelude::*;

use mcc_core::ai::{Damage, DamageSource, DamageTarget, Targets};
use mcc_foot::OnFoot;

use super::Inventory;
use super::enemy::{Enemy, EnemyKilled};
use super::fire::{self, Looks, Shots};

/// Metres from his eye to a body's surface that a blow reaches.
const REACH: f32 = 1.2;
/// Metres within which he lunges at a body in front of him.
const LUNGE: f32 = 3.2;
/// How fast the lunge carries him, metres per second.
const LUNGE_SPEED: f32 = 9.0;
/// Within this angle of where he looks (radians, either side).
const CONE: f32 = 0.75;
/// What a blow takes off: enough to kill a grunt or take an elite's shield.
pub const DAMAGE: f32 = 70.0;
/// Seconds before he can strike (or fire) again.
const RECOVER: f32 = 0.8;
/// The kill feed's weapon number for a melee kill.
pub const KILL: u32 = u32::MAX;

/// The nearest body in front of him within the lunge range: what to hit,
/// the point on it, and how far its surface is from his eye.
fn target(eye: Vec3, forward: Vec3, targets: Option<&Targets>, enemy: &Enemy) -> Option<(DamageTarget, Vec3, f32)> {
    let ahead = |point: Vec3| {
        let to = point - eye;
        to.length() < 1e-3 || forward.angle_between(to) < CONE
    };
    let actors = targets.map_or(&[][..], |t| &t.0[..]).iter().map(|t| {
        // The nearest point of its capsule's axis to his eye.
        let axis = t.top - t.base;
        let k = ((eye - t.base).dot(axis) / axis.length_squared().max(1e-6)).clamp(0.0, 1.0);
        let point = t.base + axis * k;
        (DamageTarget::Actor(t.entity), point, (eye.distance(point) - t.radius).max(0.0))
    });
    // The enemy Spartan takes his blows through `Enemy`, not `Damage`.
    let spartan = enemy.nearest(eye).map(|(p, d)| (DamageTarget::Player, p, d));
    actors
        .chain(spartan)
        .filter(|(_, p, d)| *d <= LUNGE && ahead(*p))
        .min_by(|a, b| a.2.total_cmp(&b.2))
}

/// Q / B: a blow at what is in front, lunging at it when it is a step or
/// two away.
#[allow(clippy::too_many_arguments)]
pub(crate) fn strike(
    mut commands: Commands,
    mut inv: ResMut<Inventory>,
    mut foot: ResMut<OnFoot>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    time: Res<Time>,
    targets: Option<Res<Targets>>,
    looks: Option<Res<Looks>>,
    mut shots: ResMut<Shots>,
    mut damage: MessageWriter<Damage>,
    (mut enemy, mut kills): (ResMut<Enemy>, MessageWriter<EnemyKilled>),
) {
    inv.busy = (inv.busy - time.delta_secs()).max(0.0);
    let pressed = keys.just_pressed(KeyCode::KeyQ) || gamepads.iter().next().is_some_and(|p| p.just_pressed(B::East));
    let aim = foot.aim_rotation();
    let Some(spartan) = foot.spartan.as_mut() else {
        return;
    };
    if !pressed || inv.busy > 0.0 {
        return;
    }
    inv.busy = RECOVER;
    let active = inv.active;
    if let Some(h) = inv.slots[active].as_mut() {
        h.reload = 0.0;
        h.zoom = 0;
    }
    let eye = spartan.eye();
    let forward = aim * Vec3::NEG_Z;
    let Some((who, point, distance)) = target(eye, forward, targets.as_deref(), &enemy) else {
        return;
    };
    if distance > REACH {
        // The lunge: towards it, along the ground.
        let flat = Vec3::new(point.x - eye.x, 0.0, point.z - eye.z).normalize_or_zero();
        spartan.velocity = flat * LUNGE_SPEED + Vec3::Y * spartan.velocity.y.max(0.0);
    }
    let direction = (point - eye).normalize_or(forward);
    match who {
        DamageTarget::Player => fire::hurt(&mut enemy, &mut kills, KILL, "melee", DAMAGE, false),
        target => {
            damage.write(Damage {
                target,
                source: DamageSource::Player,
                amount: DAMAGE,
                at: point,
                direction,
                head: false,
                melee: true,
            });
        }
    }
    if let Some(looks) = looks {
        let spark = looks.spark.clone();
        fire::impact(&mut commands, &looks, &mut shots, &spark, point - direction * 0.2, -direction, 0.0, false, false);
    }
    info!("melee: hit at {distance:.1} m");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor(at: Vec3) -> mcc_core::ai::Target {
        mcc_core::ai::Target {
            entity: Entity::PLACEHOLDER,
            team: 2,
            base: at,
            top: at + Vec3::Y * 1.6,
            radius: 0.3,
            head: None,
        }
    }

    #[test]
    fn a_blow_finds_the_nearest_body_in_front() {
        let eye = Vec3::new(0.0, 1.7, 0.0);
        let forward = Vec3::NEG_Z;
        let enemy = Enemy::default();
        let near = Targets(vec![actor(Vec3::new(0.0, 0.0, -1.0))]);
        let (who, _, d) = target(eye, forward, Some(&near), &enemy).expect("in reach");
        assert!(matches!(who, DamageTarget::Actor(_)));
        assert!(d < REACH, "{d}");
        let lunge = Targets(vec![actor(Vec3::new(0.0, 0.0, -2.8))]);
        let (_, _, d) = target(eye, forward, Some(&lunge), &enemy).expect("in lunge range");
        assert!(d > REACH && d <= LUNGE, "{d}");
        let behind = Targets(vec![actor(Vec3::new(0.0, 0.0, 1.0))]);
        assert!(target(eye, forward, Some(&behind), &enemy).is_none());
        let far = Targets(vec![actor(Vec3::new(0.0, 0.0, -6.0))]);
        assert!(target(eye, forward, Some(&far), &enemy).is_none());
    }
}
