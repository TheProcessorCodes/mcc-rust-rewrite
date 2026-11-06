//! The player as the actors see him, however he gets about, and the dirt
//! bike running them down.
//!
//! * **His body.** Every frame [`PlayerBody`] is written from the Spartan on
//!   foot, the dirt bike ([`BikeMotion`]) or the skater ([`SkateMotion`]):
//!   feet, a hit capsule, velocity. The brain sees, chases, leads and swings
//!   at that; the actors' rounds (`mcc_weapons`) hit that capsule; the
//!   damage goes to his shields and health the same way in all three.
//!   On the bike the capsule stands round the rider: hits on the bike count
//!   as hits on him (the bike has no health of its own; our choice).
//! * **Splatter.** The bike hurts what it runs into, as Halo 3's vehicles
//!   do. Numbers from the map where it has them
//!   (`blam_geometry::weapons::read_vehicle_collision`): the global
//!   `globals\collision_damage\collision` damage effect (100 on 120_halo)
//!   times the Mongoose's collision damage scale (`vehicle_small`, 1.2),
//!   ramped from nothing at the first of its two speeds (3 wu/s, 33 km/h)
//!   to all of it at the second (8 wu/s, 88 km/h) by the closing speed.
//!   The scale and the speed band are inferred field meanings (see
//!   `docs/format/weapons.md` 6a). Slower than the band the bike only
//!   shoves them aside. The bike itself keeps going (it does not collide
//!   with actors' bodies) and the rider takes nothing.

use bevy::prelude::*;

use mcc_core::ai::{Body, Damage, DamageSource, DamageTarget, PlayerBody, PlayerMode};
use mcc_core::bike::BikeMotion;
use mcc_core::skate::SkateMotion;
use mcc_foot::OnFoot;
use mcc_foot::physics::Tuning;

use crate::place::{Actor, Ai};
use crate::WU;

/// The rider stands on the pegs: his capsule is this much taller than the
/// Spartan's, and a little wider round the bike. Ours.
const RIDER_LIFT: f32 = 0.3;
const RIDER_WIDEN: f32 = 0.1;

/// Writes [`PlayerBody`]: the bike first, then the Spartan, then the board.
pub(crate) fn publish(
    foot: Option<Res<OnFoot>>,
    bike: Option<Res<BikeMotion>>,
    skate: Option<Res<SkateMotion>>,
    mut out: ResMut<PlayerBody>,
) {
    let base = Tuning::default();
    let spartan = foot.as_deref().and_then(|f| f.spartan.as_ref());
    out.0 = if let Some(b) = bike.as_deref().filter(|b| b.riding) {
        Some(Body {
            mode: PlayerMode::Bike,
            feet: b.position - Vec3::Y * mcc_bike::physics::rest_height(),
            height: base.stand_height + RIDER_LIFT,
            radius: base.radius + RIDER_WIDEN,
            velocity: b.velocity,
            heading: b.heading,
            steady: b.upright,
        })
    } else if let Some(s) = spartan {
        Some(Body {
            mode: PlayerMode::Foot,
            feet: s.position,
            height: s.height(),
            radius: s.tuning.radius,
            velocity: s.velocity,
            heading: s.heading,
            steady: s.grounded && s.ground_normal.y > 0.7,
        })
    } else if let Some(s) = skate.as_deref().filter(|s| s.riding) {
        let flat = Vec3::new(s.velocity.x, 0.0, s.velocity.z);
        Some(Body {
            mode: PlayerMode::Board,
            feet: s.feet,
            height: base.stand_height,
            radius: base.radius,
            velocity: s.velocity,
            heading: if flat.length() > 0.5 { flat.x.atan2(flat.z) } else { 0.0 },
            steady: s.velocity.y.abs() < 1.0,
        })
    } else {
        None
    };
}

/// The bike's splatter numbers, read once from the map (see the module
/// notes); our own when the map has no Mongoose.
#[derive(Resource, Clone, Copy, Debug)]
pub(crate) struct Splatter {
    /// Full damage of a hit at or above `speeds[1]`.
    pub damage: f32,
    /// Closing speeds (wu/s): no damage below the first, all of it from the
    /// second.
    pub speeds: [f32; 2],
}

impl Default for Splatter {
    /// Our stand-in: the same as 120_halo's.
    fn default() -> Self {
        Splatter {
            damage: 120.0,
            speeds: [3.0, 8.0],
        }
    }
}

impl Splatter {
    pub fn from_tags(c: &blam_geometry::weapons::VehicleCollision) -> Splatter {
        let scale = if c.scale > 0.0 { c.scale } else { 1.0 };
        let (lo, hi) = (c.speeds[0].max(0.0), c.speeds[1]);
        Splatter {
            damage: c.damage[1].max(c.damage[0]) * scale,
            speeds: if hi > lo { [lo, hi] } else { Splatter::default().speeds },
        }
    }

    /// The damage of a hit at `speed` m/s closing speed.
    pub fn at(&self, speed: f32) -> f32 {
        let wu = speed / WU;
        let t = ((wu - self.speeds[0]) / (self.speeds[1] - self.speeds[0])).clamp(0.0, 1.0);
        self.damage * t
    }
}

/// The bike's own hit shape: a capsule along it from rear to front axle,
/// this radius round the frame origin. Ours (the 450F is about 1.45 m
/// between the axles).
const BIKE_HALF_LENGTH: f32 = 0.75;
const BIKE_RADIUS: f32 = 0.45;
/// Seconds before the bike can hit the same actor again.
const RAM_AGAIN: f32 = 0.6;

/// Closest points of two segments `p0-p1` and `q0-q1`: their distance.
pub(crate) fn segment_distance(p0: Vec3, p1: Vec3, q0: Vec3, q1: Vec3) -> f32 {
    let d1 = p1 - p0;
    let d2 = q1 - q0;
    let r = p0 - q0;
    let a = d1.length_squared();
    let e = d2.length_squared();
    let f = d2.dot(r);
    let (s, t) = if a <= 1e-9 && e <= 1e-9 {
        (0.0, 0.0)
    } else if a <= 1e-9 {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1.dot(r);
        if e <= 1e-9 {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let mut s = if denom > 1e-9 { ((b * f - c * e) / denom).clamp(0.0, 1.0) } else { 0.0 };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t)
        }
    };
    ((p0 + d1 * s) - (q0 + d2 * t)).length()
}

/// Where the bike was last frame, and who it hit lately.
#[derive(Default)]
pub(crate) struct RamState {
    last: Option<Vec3>,
    recent: Vec<(Entity, f32)>,
}

/// The bike running into actors: damage by closing speed, and a shove out
/// of its way.
pub(crate) fn ram(
    ai: Option<Res<Ai>>,
    bike: Option<Res<BikeMotion>>,
    splatter: Res<Splatter>,
    mut actors: Query<(Entity, &mut Actor)>,
    mut damage: MessageWriter<Damage>,
    mut state: Local<RamState>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    state.recent.retain_mut(|(_, t)| {
        *t -= dt;
        *t > 0.0
    });
    let (Some(ai), Some(b)) = (ai, bike.as_deref().filter(|b| b.riding)) else {
        state.last = None;
        return;
    };
    let fwd = Vec3::new(b.heading.sin(), 0.0, b.heading.cos());
    let (rear, front) = (b.position - fwd * BIKE_HALF_LENGTH, b.position + fwd * BIKE_HALF_LENGTH);
    // The way it came this frame, so a fast bike does not pass through a
    // small actor between two frames.
    let last = state.last.replace(b.position).filter(|l| l.distance(b.position) < 10.0).unwrap_or(b.position);
    for (e, mut a) in &mut actors {
        if !a.alive() || state.recent.iter().any(|(r, _)| *r == e) {
            continue;
        }
        let Some(kind) = ai.assets.kinds.get(a.kind).and_then(Option::as_ref) else {
            continue;
        };
        let r = (kind.radius * WU).max(0.1);
        let h = (kind.height * WU).max(r * 2.0);
        let (lo, hi) = (a.position + Vec3::Y * r, a.position + Vec3::Y * (h - r));
        let reach = r + BIKE_RADIUS;
        let gap = segment_distance(rear, front, lo, hi).min(segment_distance(last, b.position, lo, hi));
        if gap >= reach {
            continue;
        }
        let closing = (b.velocity - a.velocity).length();
        let amount = splatter.at(closing);
        let away = {
            let d = a.position - b.position;
            let side = Vec3::new(d.x, 0.0, d.z) - fwd * Vec3::new(d.x, 0.0, d.z).dot(fwd);
            side.normalize_or(fwd.cross(Vec3::Y))
        };
        // Out of the bike's way, and carried along a little.
        let push = away * (reach - gap) + Vec3::new(b.velocity.x, 0.0, b.velocity.z) * dt * 0.5;
        a.position += push;
        let thrown = b.velocity * 0.4 + away * 1.5 + Vec3::Y * 1.0;
        if let Some(body) = a.body.as_mut() {
            body.position += push;
            if amount > 0.0 {
                body.velocity = thrown;
            }
        }
        if amount <= 0.0 {
            continue;
        }
        a.velocity = thrown;
        state.recent.push((e, RAM_AGAIN));
        damage.write(Damage {
            target: DamageTarget::Actor(e),
            source: DamageSource::Player,
            amount,
            at: a.position + Vec3::Y * h * 0.5,
            direction: b.velocity.normalize_or(fwd),
            head: false,
            melee: false,
        });
        if std::env::var("MCC_AI_TRACE").is_ok_and(|v| v == "1") {
            info!("ai: the bike hit {} at {:.1} m/s for {amount:.0}", kind.character.tag_path, closing);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments() {
        let d = segment_distance(Vec3::ZERO, Vec3::X * 2.0, Vec3::new(1.0, 1.0, 0.0), Vec3::new(1.0, 3.0, 0.0));
        assert!((d - 1.0).abs() < 1e-5, "{d}");
        let d = segment_distance(Vec3::ZERO, Vec3::X, Vec3::new(3.0, 0.0, 0.0), Vec3::new(3.0, 1.0, 0.0));
        assert!((d - 2.0).abs() < 1e-5, "{d}");
        // A point segment.
        let d = segment_distance(Vec3::Y, Vec3::Y, Vec3::ZERO, Vec3::Z);
        assert!((d - 1.0).abs() < 1e-5, "{d}");
    }

    #[test]
    fn splatter_ramps_with_speed() {
        let s = Splatter::default();
        assert_eq!(s.at(0.0), 0.0);
        assert_eq!(s.at(2.9 * WU), 0.0);
        assert!((s.at(8.0 * WU) - 120.0).abs() < 1e-3);
        assert!((s.at(30.0) - 120.0).abs() < 1e-3);
        let mid = s.at(5.5 * WU);
        assert!((mid - 60.0).abs() < 1e-2, "{mid}");
    }
}
