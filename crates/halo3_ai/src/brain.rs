//! The first brain: see, choose, move, shoot, hit.
//!
//! * **Seeing.** A hostile (the Spartan, or an actor of an enemy team) is
//!   seen inside the character's vision distance and angle, or inside its
//!   peripheral distance whatever the angle, with a clear line from the
//!   eye to its chest on the collision. Gunfire within hearing distance and
//!   being hit make an actor aware too; it then searches where it last
//!   saw or heard the foe for a while.
//! * **Choosing.** Melee actors (infection forms, stalkers, tanks,
//!   carriers) run in and swing. Combat forms with a gun charge when the
//!   foe is inside the character's melee consider range (at its melee
//!   chance, rolled every few seconds), else fight at range. Ranged actors
//!   keep inside the weapon's normal combat range for that character:
//!   closing in by the zone's firing positions, backing off, strafing.
//!   Allies of the player with nothing to fight follow him.
//! * **Moving.** Walkers move as capsules on the collision with the
//!   Spartan's own body code (`halo3_foot::physics`), sized by the biped's
//!   character physics; flyers glide, keep clear of the ground and slide
//!   off walls.
//! * **Shooting.** Bursts as the character's firing pattern gives them:
//!   first-burst delay, burst length and gap, rate of fire (the weapon's
//!   when the pattern says 0), a burst that starts off the target by the
//!   burst origin radius and walks on, leading by the target-leading
//!   share, spread by the projectile error. Rounds are the weapons' own
//!   (`halo3_weapons::fire`), so they fly, hit walls and hurt.

use bevy::prelude::*;
use halo3_core::map::MapAppExt;

use halo3_bike::physics::World;
use halo3_core::ai::{
    ActorActivity, ActorState, Allegiances, Damage, DamageSource, DamageTarget, PlayerVitality, Target, Targets, team,
};
use halo3_core::coords::{to_halo, to_halo_dir, to_render};
use halo3_core::fly_camera::FlyCamera;
use halo3_core::weapons::{WeaponAction, WeaponEvent};
use halo3_core::zones::LoadedBsps;
use halo3_foot::OnFoot;
use halo3_foot::physics::{Controls, Spartan, Tuning};
use halo3_weapons::Armory;
use halo3_weapons::Shots;

use crate::place::{Actor, Ai, root_transform, yaw_of_heading};
use crate::{AiSet, Style, WU};

pub(crate) struct BrainPlugin;

impl Plugin for BrainPlugin {
    fn build(&self, app: &mut App) {
        app.init_map_resource::<AiWorld>().add_systems(
            Update,
            (follow_world, think, separate, place_roots, light, targets)
                .chain()
                .in_set(AiSet::Think)
                .run_if(halo3_core::pause::running)
                .run_if(halo3_core::map::ready)
                .after(halo3_foot::walk),
        );
    }
}

/// Actors further than this from the player (or the camera) stand still.
const ACTIVE_RANGE: f32 = 120.0;
/// Seconds between looks round.
const THINK: f32 = 0.3;
/// Seconds an actor keeps after a foe it lost sight of.
const MEMORY: f32 = 8.0;

/// Who an actor is after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Foe {
    Player,
    Actor(Entity),
}

/// An actor's mind.
pub(crate) struct Brain {
    rng: u32,
    pub foe: Option<Foe>,
    /// Can see the foe now.
    pub sees: bool,
    /// Where the foe was last seen or heard, and how long ago.
    pub last_seen: Option<Vec3>,
    pub since_seen: f32,
    /// Hit by this lately: look for it.
    pub provoked: Option<DamageSource>,
    think: f32,
    /// Charging in (flood combat forms), and when to roll again.
    charging: bool,
    charge_roll: f32,
    /// Bursts: seconds left of this one, until the next, between shots.
    burst_left: f32,
    burst_length: f32,
    burst_wait: f32,
    shot_wait: f32,
    /// Where the burst starts relative to the target, metres.
    burst_offset: Vec3,
    melee_wait: f32,
    /// Sideways while in range: +1 / -1, and seconds until it turns.
    strafe: f32,
    strafe_time: f32,
    /// Seconds it wanted to move but did not.
    stuck: f32,
    /// A firing position it is heading to, and when to choose again.
    post: Option<Vec3>,
    post_time: f32,
    pub state: ActorState,
    pub time_in_state: f32,
    /// Hover height over the ground, metres (flyers).
    hover: Option<f32>,
}

impl Brain {
    pub fn new(seed: u32) -> Brain {
        Brain {
            rng: seed.wrapping_mul(2_654_435_761).max(1),
            foe: None,
            sees: false,
            last_seen: None,
            since_seen: 100.0,
            provoked: None,
            think: 0.0,
            charging: false,
            charge_roll: 0.0,
            burst_left: 0.0,
            burst_length: 1.0,
            burst_wait: 1.0,
            shot_wait: 0.0,
            burst_offset: Vec3::ZERO,
            melee_wait: 0.0,
            strafe: 1.0,
            strafe_time: 0.0,
            stuck: 0.0,
            post: None,
            post_time: 0.0,
            state: ActorState::Idle,
            time_in_state: 0.0,
            hover: None,
        }
    }

    fn random(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32) / (u32::MAX as f32)
    }

    fn between(&mut self, r: [f32; 2]) -> f32 {
        let (a, b) = (r[0].min(r[1]), r[0].max(r[1]));
        a + (b - a) * self.random()
    }

    fn set_state(&mut self, s: ActorState) {
        if self.state != s {
            self.state = s;
            self.time_in_state = 0.0;
        }
    }
}

/// The collision the actors walk on while nobody is on foot (on foot they
/// share the Spartan's).
#[derive(Resource, Default)]
pub(crate) struct AiWorld {
    world: Option<World>,
    mask: Option<u32>,
}

fn follow_world(
    mut own: ResMut<AiWorld>,
    foot: Option<Res<OnFoot>>,
    input: Option<Res<halo3_bike::BikeInput>>,
    loaded: Res<LoadedBsps>,
    actors: Query<(), With<Actor>>,
) {
    let on_foot = foot.as_deref().is_some_and(|f| f.spartan.is_some() && f.world().is_some());
    if on_foot || actors.is_empty() {
        return;
    }
    let Some(input) = input else {
        return;
    };
    let mask = if loaded.mask == 0 { u32::MAX } else { loaded.mask };
    if own.mask != Some(mask) {
        own.world = Some(halo3_bike::build_world(&input.0, mask));
        own.mask = Some(mask);
    }
}

/// What a foe looks like this frame.
#[derive(Clone, Copy)]
struct Seen {
    foe: Foe,
    team: i16,
    /// Feet and chest, renderer frame.
    feet: Vec3,
    chest: Vec3,
    velocity: Vec3,
    radius: f32,
}

fn clear_line(world: Option<&World>, from: Vec3, to: Vec3) -> bool {
    let Some(world) = world else {
        return true;
    };
    let d = to - from;
    let len = d.length();
    if len < 0.05 {
        return true;
    }
    world.raycast_far(from, d / len, (len - 0.3).max(0.0)).is_none()
}

/// Our own tuning for an actor's body from its biped's capsule.
fn tuning_for(kind: &crate::ActorKind) -> Tuning {
    let base = Tuning::default();
    let height = kind.height * WU;
    let speed = kind.speed * WU;
    Tuning {
        forward: speed,
        backward: speed * 0.7,
        sideways: speed * 0.8,
        ground_accel: 8.0 * WU,
        air_accel: 1.0 * WU,
        jump: 2.2 * WU,
        step: (height * 0.35).clamp(0.1, 0.45),
        radius: (kind.radius * WU).clamp(0.1, 1.2),
        stand_height: height.max(0.3),
        crouch_height: height.max(0.3),
        crouch_speed: 1.0,
        stand_eye: kind.eye * WU,
        crouch_eye: kind.eye * WU,
        ..base
    }
}

fn forward_of(heading: f32) -> Vec3 {
    Vec3::new(heading.sin(), 0.0, heading.cos())
}

fn heading_to(d: Vec3) -> f32 {
    d.x.atan2(d.z)
}

/// Turns `from` towards `to` by at most `step` radians.
fn turn(from: f32, to: f32, step: f32) -> f32 {
    let mut d = (to - from) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    from + d.clamp(-step, step)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn think(
    ai: Option<Res<Ai>>,
    own: Res<AiWorld>,
    foot: Option<Res<OnFoot>>,
    armory: Option<Res<Armory>>,
    shots: Option<ResMut<Shots>>,
    allegiances: Res<Allegiances>,
    vitality: Res<PlayerVitality>,
    cameras: Query<&Transform, (With<FlyCamera>, Without<Actor>)>,
    mut actors: Query<(Entity, &mut Actor, &mut ActorActivity)>,
    globals: Query<&GlobalTransform>,
    mut events: MessageReader<WeaponEvent>,
    mut damage: MessageWriter<Damage>,
    time: Res<Time>,
) {
    let Some(ai) = ai else {
        events.clear();
        return;
    };
    let dt = time.delta_secs().min(0.1);
    let mut shots = shots;
    let spartan = foot.as_deref().and_then(|f| f.spartan.as_ref());
    let world: Option<&World> = match (spartan, foot.as_deref().and_then(|f| f.world())) {
        (Some(_), Some(w)) => Some(w),
        _ => own.world.as_ref(),
    };
    let viewer = spartan
        .map(|s| s.position)
        .or_else(|| cameras.iter().next().map(|t| t.translation));
    // The player's gunfire, heard.
    let mut noise: Option<Vec3> = None;
    for e in events.read() {
        if e.own && e.action == WeaponAction::Fire {
            noise = Some(e.at);
        }
    }

    // Everyone who can be fought, this frame.
    let mut bodies: Vec<Seen> = Vec::new();
    if let Some(s) = spartan
        && vitality.dead.is_none()
    {
        bodies.push(Seen {
            foe: Foe::Player,
            team: team::PLAYER,
            feet: s.position,
            chest: s.position + Vec3::Y * s.height() * 0.7,
            velocity: s.velocity,
            radius: s.tuning.radius,
        });
    }
    for (e, a, _) in &actors {
        if !a.alive() {
            continue;
        }
        let Some(kind) = ai.assets.kinds.get(a.kind).and_then(Option::as_ref) else {
            continue;
        };
        bodies.push(Seen {
            foe: Foe::Actor(e),
            team: a.team,
            feet: a.position,
            chest: a.position + Vec3::Y * kind.height * WU * 0.6,
            velocity: a.velocity,
            radius: kind.radius * WU,
        });
    }

    for (entity, mut actor, mut activity) in &mut actors {
        let actor = &mut *actor;
        if !actor.alive() {
            continue;
        }
        let Some(kind) = ai.assets.kinds.get(actor.kind).and_then(Option::as_ref) else {
            continue;
        };
        if viewer.is_some_and(|v| v.distance(actor.position) > ACTIVE_RANGE) {
            continue;
        }
        let ch = &kind.character;
        let brain = &mut actor.brain;
        brain.time_in_state += dt;
        brain.since_seen += dt;
        brain.melee_wait -= dt;
        let eye = actor.position + Vec3::Y * kind.eye * WU;
        let fwd = forward_of(actor.heading);

        // Hearing and pain.
        if let Some(at) = noise
            && at.distance(actor.position) < ch.perception.hearing_distance * WU
            && bodies.iter().any(|b| b.foe == Foe::Player && !allegiances.allied(actor.team, b.team))
        {
            if brain.foe.is_none() {
                brain.foe = Some(Foe::Player);
            }
            if !brain.sees {
                brain.last_seen = Some(at);
                brain.since_seen = brain.since_seen.min(1.0);
            }
        }
        if let Some(src) = brain.provoked.take() {
            let foe = match src {
                DamageSource::Player => Some(Foe::Player),
                DamageSource::Actor(e) if e != entity => Some(Foe::Actor(e)),
                _ => None,
            };
            if let Some(f) = foe
                && let Some(b) = bodies.iter().find(|b| b.foe == f)
                && !allegiances.allied(actor.team, b.team)
            {
                brain.foe = Some(f);
                brain.last_seen = Some(b.feet);
                brain.since_seen = 0.0;
            }
        }

        // Looking round.
        brain.think -= dt;
        if brain.think <= 0.0 {
            brain.think = THINK * (0.8 + 0.4 * brain.random());
            let p = &ch.perception;
            let aware = brain.foe.is_some() && brain.since_seen < MEMORY;
            let vision = p.max_vision_distance * WU * if aware { 1.5 } else { 1.0 };
            let angle = if aware { p.peripheral_vision_angle } else { p.max_vision_angle }.max(0.3);
            let mut candidates: Vec<(f32, Seen)> = bodies
                .iter()
                .filter(|b| b.foe != Foe::Actor(entity) && !allegiances.allied(actor.team, b.team))
                .map(|b| (b.chest.distance(eye), *b))
                .filter(|(d, _)| *d < vision)
                .filter(|(d, b)| {
                    let to = (b.chest - eye).normalize_or_zero();
                    *d < p.peripheral_distance * WU || fwd.dot(Vec3::new(to.x, 0.0, to.z).normalize_or_zero()) >= angle.cos()
                })
                .collect();
            candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
            // The current foe is kept when still seen.
            if let Some(cur) = brain.foe
                && let Some(i) = candidates.iter().position(|(_, b)| b.foe == cur)
            {
                let c = candidates.remove(i);
                candidates.insert(0, c);
            }
            let seen = candidates
                .iter()
                .take(3)
                .find(|(_, b)| clear_line(world, eye, b.chest))
                .map(|(_, b)| *b);
            match seen {
                Some(b) => {
                    if brain.foe != Some(b.foe) || brain.since_seen > MEMORY {
                        // A new foe: the first burst waits.
                        let w = ch.weapon(None).map_or([0.3, 1.0], |w| w.first_burst_delay);
                        brain.burst_wait = brain.between(w);
                        brain.burst_left = 0.0;
                    }
                    brain.foe = Some(b.foe);
                    brain.sees = true;
                    brain.last_seen = Some(b.feet);
                    brain.since_seen = 0.0;
                }
                None => {
                    brain.sees = false;
                    if brain.since_seen > MEMORY {
                        brain.foe = None;
                        brain.last_seen = None;
                    }
                }
            }
        }
        // Forget a foe that is gone.
        let foe_now = brain.foe.and_then(|f| bodies.iter().find(|b| b.foe == f).copied());
        if brain.foe.is_some() && foe_now.is_none() {
            brain.foe = None;
            brain.sees = false;
        }

        // Choosing what to do.
        // The gun's model entity (for its muzzle) and its kind.
        let gun = actor
            .gun
            .as_ref()
            .and_then(|g| armory.as_deref().and_then(|a| a.kinds.get(g.kind)).map(|k| (g.model, k)));
        let weapon_tag = gun.map(|(_, k)| k.weapon.tag_index);
        let usage = ch.weapon(weapon_tag);
        let combat = usage
            .map(|u| [u.normal_combat_range[0], u.normal_combat_range[1]])
            .filter(|r| r[1] > 0.0)
            .unwrap_or([3.0, 8.0])
            .map(|r| r * WU);
        let max_fire = usage.map_or(15.0, |u| u.maximum_firing_range.max(2.0)) * WU;
        let melee_reach = (ch.charge.melee_attack_range.max(0.6) + kind.radius) * WU;
        let mut goal: Option<Vec3> = None;
        let mut face: Option<Vec3> = None;
        let mut want_fire = false;
        let mut want_melee = false;
        let mut speed_scale = 1.0;
        match (foe_now, brain.sees) {
            (Some(f), seen) => {
                let d = f.feet.distance(actor.position);
                face = Some(f.chest);
                let style = if gun.is_none() && kind.style != Style::Ranged { Style::Melee } else { kind.style };
                let charge = match style {
                    Style::Melee => true,
                    Style::Flood => {
                        brain.charge_roll -= dt;
                        if brain.charge_roll <= 0.0 {
                            brain.charge_roll = 3.0;
                            brain.charging =
                                d < ch.charge.melee_consider_range * WU && brain.random() < ch.charge.melee_chance;
                        }
                        if d > ch.charge.melee_abort_range.max(2.0) * WU {
                            brain.charging = false;
                        }
                        brain.charging
                    }
                    Style::Ranged => false,
                };
                if charge {
                    goal = Some(f.feet);
                    want_melee = d < melee_reach + f.radius;
                    want_fire = seen && gun.is_some() && d > melee_reach * 2.0;
                } else {
                    want_fire = seen && d < max_fire;
                    if !seen {
                        goal = brain.last_seen;
                    } else if d > combat[1] {
                        brain.post_time -= dt;
                        if brain.post.is_none() || brain.post_time <= 0.0 {
                            brain.post_time = 2.0;
                            brain.post = firing_position(&ai, actor.squad, actor.position, f.chest, combat, world);
                        }
                        goal = Some(brain.post.unwrap_or(f.feet));
                    } else if d < combat[0] {
                        goal = Some(actor.position + (actor.position - f.feet).normalize_or_zero() * 3.0);
                    } else {
                        brain.post = None;
                        brain.strafe_time -= dt;
                        if brain.strafe_time <= 0.0 {
                            brain.strafe_time = 1.5 + 2.0 * brain.random();
                            brain.strafe = if brain.random() < 0.5 { -1.0 } else { 1.0 };
                            if brain.random() < 0.3 {
                                brain.strafe = 0.0;
                            }
                        }
                        let side = (f.feet - actor.position).cross(Vec3::Y).normalize_or_zero();
                        if brain.strafe != 0.0 {
                            goal = Some(actor.position + side * brain.strafe * 2.0);
                            speed_scale = 0.5;
                        }
                    }
                }
            }
            (None, _) => {
                // Searching where it last heard or saw something.
                if let Some(at) = brain.last_seen {
                    goal = Some(at);
                    speed_scale = 0.6;
                    if at.distance(actor.position) < 1.5 {
                        brain.last_seen = None;
                    }
                } else if let Some(s) = spartan
                    && allegiances.allied(actor.team, team::PLAYER)
                    && vitality.dead.is_none()
                {
                    // Allies keep up with the player.
                    let d = s.position.distance(actor.position);
                    if d > 8.0 && d < 60.0 {
                        goal = Some(s.position);
                    }
                }
            }
        }

        // Moving.
        let target_speed = kind.speed * WU * speed_scale;
        let mut wish_dir = Vec3::ZERO;
        if let Some(g) = goal {
            let mut d = g - actor.position;
            if !actor.flying {
                d.y = 0.0;
            }
            if d.length() > 0.6 {
                wish_dir = d.normalize();
            }
        }
        if want_melee {
            wish_dir = Vec3::ZERO;
        }
        let desired_heading = face
            .map(|f| heading_to(Vec3::new(f.x - actor.position.x, 0.0, f.z - actor.position.z)))
            .or_else(|| (wish_dir.length_squared() > 0.0).then(|| heading_to(wish_dir)))
            .unwrap_or(actor.heading);
        actor.heading = turn(actor.heading, desired_heading, 6.0 * dt);
        if actor.flying {
            fly(actor, kind, world, wish_dir, target_speed, face, dt);
        } else if let Some(world) = world {
            if actor.body.is_none() {
                let mut b = Spartan::dropped(world, actor.position + Vec3::Y * 0.5, actor.heading);
                b.tuning = tuning_for(kind);
                actor.body = Some(b);
            }
            let heading = actor.heading;
            let body = actor.body.as_mut().expect("made above");
            let f = forward_of(heading);
            let right = f.cross(Vec3::Y);
            let share = if kind.speed > 0.0 { (speed_scale).clamp(0.0, 1.0) } else { 0.0 };
            let controls = Controls {
                wish: Vec2::new(wish_dir.dot(right), wish_dir.dot(f)) * share,
                heading,
                jump: brain_stuck(&mut actor.brain, wish_dir, body.speed(), dt) && body.grounded,
                crouch: false,
            };
            body.step(world, controls, dt);
            if body.position.y < actor.position.y - 200.0 {
                // Fell out of the world: stay where it was.
                body.position = actor.position;
                body.velocity = Vec3::ZERO;
            }
            actor.velocity = body.velocity;
            actor.position = body.position;
        }

        // Shooting.
        if want_fire
            && let (Some((g, k)), Some(f), Some(shots)) = (gun, foe_now, shots.as_deref_mut())
        {
            let pattern = ch.firing_pattern(weapon_tag).unwrap_or_default();
            let barrel = k
                .weapon
                .triggers
                .first()
                .and_then(|t| t.primary_barrel)
                .and_then(|b| k.weapon.barrels.get(b as usize))
                .or(k.weapon.barrels.first());
            if let Some(barrel) = barrel {
                let brain = &mut actor.brain;
                let rate = if pattern.rate_of_fire > 0.0 {
                    pattern.rate_of_fire
                } else {
                    barrel.rounds_per_second[1].max(barrel.rounds_per_second[0]).max(1.0)
                };
                let single = k.weapon.triggers.first().is_some_and(|t| matches!(t.behavior, 1 | 2 | 6));
                if brain.burst_left > 0.0 {
                    brain.burst_left -= dt;
                    brain.shot_wait -= dt;
                    let mut fired = 0;
                    while brain.shot_wait <= 0.0 && fired < 4 {
                        fired += 1;
                        brain.shot_wait += if single { brain.burst_length.max(0.5) } else { 1.0 / rate };
                        let muzzle = g
                            .and_then(|m| globals.get(m).ok())
                            .map(|gt| (Mat4::from(gt.affine()) * k.muzzle).w_axis.truncate())
                            .unwrap_or(eye);
                        // Lead the target, and walk the burst onto it.
                        let speed = barrel
                            .projectile
                            .as_ref()
                            .map_or(300.0, |p| p.initial_velocity.max(1.0))
                            * WU;
                        let flight = muzzle.distance(f.chest) / speed;
                        let walk = if brain.burst_length > 0.0 { (brain.burst_left / brain.burst_length).clamp(0.0, 1.0) } else { 0.0 };
                        let aim = f.chest + f.velocity * flight * pattern.target_leading + brain.burst_offset * walk;
                        let spread = pattern.projectile_error.max(barrel.minimum_error)
                            + pattern.max_error_angle * 0.25 * brain.random();
                        shots.shoot_from(k, barrel, muzzle, aim - muzzle, spread, entity);
                    }
                } else {
                    brain.burst_wait -= dt;
                    if brain.burst_wait <= 0.0 {
                        brain.burst_length = brain.between(pattern.burst_duration).max(if single { 0.0 } else { 0.25 });
                        brain.burst_left = brain.burst_length.max(0.05);
                        brain.burst_wait = brain.between(pattern.burst_separation).max(0.4);
                        brain.shot_wait = 0.0;
                        let r = pattern.burst_origin_radius * WU;
                        let dir = Vec3::new(brain.random() - 0.5, brain.random() - 0.5, brain.random() - 0.5);
                        brain.burst_offset = dir.normalize_or_zero() * r;
                    }
                }
            }
        }

        // Swinging.
        let brain = &mut actor.brain;
        if want_melee
            && brain.melee_wait <= 0.0
            && let Some(f) = foe_now
        {
            brain.melee_wait = ch.charge.melee_attack_delay.max(1.0);
            let amount = kind.melee.as_ref().map_or(10.0, |m| {
                let lo = m.upper_bound[0].max(m.lower_bound);
                lo + (m.upper_bound[1].max(lo) - lo) * brain.random()
            });
            damage.write(Damage {
                target: match f.foe {
                    Foe::Player => DamageTarget::Player,
                    Foe::Actor(e) => DamageTarget::Actor(e),
                },
                source: DamageSource::Actor(entity),
                amount,
                at: f.chest,
                direction: (f.chest - actor.position).normalize_or_zero(),
                head: false,
                melee: true,
            });
            brain.set_state(ActorState::Melee);
            if kind.bursts {
                // A carrier bursts on its target.
                actor.health = 0.0;
                actor.dead = Some(0.0);
            }
        }

        // What it is doing, for the animations.
        let brain = &mut actor.brain;
        let moving = Vec3::new(actor.velocity.x, 0.0, actor.velocity.z).length() > 0.3;
        let state = if brain.state == ActorState::Melee && brain.time_in_state < 0.6 {
            ActorState::Melee
        } else if want_fire && brain.burst_left > 0.0 {
            ActorState::Firing
        } else if moving {
            ActorState::Moving
        } else if brain.foe.is_some() {
            ActorState::Alert
        } else {
            ActorState::Idle
        };
        brain.set_state(state);
        activity.state = brain.state;
        activity.time_in_state = brain.time_in_state;
        activity.velocity = to_halo_dir(actor.velocity) / WU;
        activity.yaw = yaw_of_heading(actor.heading);
        activity.aim = face.map_or(to_halo_dir(forward_of(actor.heading)), |f| to_halo_dir((f - eye).normalize_or_zero()));
        activity.flying = actor.flying;
    }
}

/// Tracks being stuck; true when it should hop.
fn brain_stuck(brain: &mut Brain, wish: Vec3, speed: f32, dt: f32) -> bool {
    if wish.length_squared() > 0.0 && speed < 0.3 {
        brain.stuck += dt;
    } else {
        brain.stuck = 0.0;
    }
    if brain.stuck > 0.8 {
        brain.stuck = 0.0;
        brain.post = None;
        brain.strafe = -brain.strafe;
        return true;
    }
    false
}

/// A flyer's step: towards the goal in three dimensions, holding a height
/// over the ground (and over its foe), sliding off walls.
fn fly(actor: &mut Actor, kind: &crate::ActorKind, world: Option<&World>, wish: Vec3, speed: f32, face: Option<Vec3>, dt: f32) {
    let r = kind.radius * WU;
    let ground = |p: Vec3| world.and_then(|w| w.raycast_far(p + Vec3::Y * 0.2, Vec3::NEG_Y, 40.0)).map(|h| h.point.y);
    let floor = ground(actor.position);
    let hover = *actor
        .brain
        .hover
        .get_or_insert_with(|| floor.map_or(2.5, |g| (actor.position.y - g).clamp(1.0, 6.0)));
    let mut want_y = floor.map_or(actor.position.y, |g| g + hover);
    if let Some(f) = face {
        // Over the foe's head height, a little above.
        want_y = want_y.max(f.y + 1.0).min(f.y + 6.0);
    }
    let mut v = Vec3::new(wish.x, 0.0, wish.z) * speed;
    v.y = ((want_y - actor.position.y) * 1.5).clamp(-speed, speed);
    // Smooth.
    actor.velocity += (v - actor.velocity) * (1.0 - (-4.0 * dt).exp());
    let step = actor.velocity * dt;
    let len = step.length();
    if len > 1e-5
        && let Some(w) = world
    {
        let centre = actor.position + Vec3::Y * kind.height * WU * 0.5;
        if let Some(hit) = w.raycast_far(centre, step / len, len + r) {
            // Slide along what it would hit.
            let n = hit.normal;
            actor.velocity -= n * actor.velocity.dot(n).min(0.0);
            actor.position += (step - n * step.dot(n)).clamp_length_max(len);
            return;
        }
    }
    actor.position += step;
}

/// A firing position of the squad's zone within the combat range of the
/// foe, with a clear line to it, near the actor.
fn firing_position(ai: &Ai, squad: usize, at: Vec3, foe: Vec3, range: [f32; 2], world: Option<&World>) -> Option<Vec3> {
    let data = &ai.assets.data;
    let zone = data.squads.get(squad)?.zone?;
    let fps = &data.zones.get(zone as usize)?.firing_positions;
    let mut best: Vec<(f32, Vec3)> = fps
        .iter()
        .map(|f| to_render(f.position))
        .filter(|p| {
            let d = p.distance(foe);
            d >= range[0] && d <= range[1] * 1.1
        })
        .map(|p| (p.distance(at), p))
        .filter(|(d, _)| *d < 30.0)
        .collect();
    best.sort_by(|a, b| a.0.total_cmp(&b.0));
    best.into_iter()
        .take(6)
        .map(|(_, p)| p)
        .find(|p| clear_line(world, *p + Vec3::Y * 1.5, foe))
}

/// Actors do not stand inside each other.
fn separate(ai: Option<Res<Ai>>, mut actors: Query<&mut Actor>) {
    let Some(ai) = ai else {
        return;
    };
    let mut list: Vec<(Vec3, f32)> = actors
        .iter()
        .map(|a| {
            let r = ai.assets.kinds.get(a.kind).and_then(Option::as_ref).map_or(0.3, |k| k.radius * WU);
            (if a.alive() { a.position } else { Vec3::splat(f32::MAX) }, r)
        })
        .collect();
    let n = list.len();
    let mut push = vec![Vec3::ZERO; n];
    for i in 0..n {
        for j in i + 1..n {
            let (a, ra) = list[i];
            let (b, rb) = list[j];
            let mut d = b - a;
            d.y = 0.0;
            let reach = ra + rb;
            let len = d.length();
            if len < reach && len > 1e-4 && (a.y - b.y).abs() < 1.5 {
                let p = d / len * (reach - len) * 0.5;
                push[i] -= p;
                push[j] += p;
            }
        }
    }
    for ((mut a, p), slot) in actors.iter_mut().zip(push).zip(list.iter_mut()) {
        if p != Vec3::ZERO {
            a.position += p;
            if let Some(b) = a.body.as_mut() {
                b.position += p;
            }
            slot.0 = a.position;
        }
    }
}

/// Root transforms follow the actors.
fn place_roots(mut actors: Query<(&Actor, &mut Transform)>) {
    for (a, mut t) in &mut actors {
        if a.dead.is_some() {
            continue;
        }
        *t = root_transform(a.position, a.heading, 0.0);
    }
}

/// Lights each actor (and its gun) by the baked light where it stands,
/// resampled as it moves.
fn light(
    ai: Option<Res<Ai>>,
    lights: Option<Res<halo3_world::lighting::SceneLights>>,
    loaded: Res<LoadedBsps>,
    mut actors: Query<&mut Actor>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (Some(ai), Some(lights)) = (ai, lights) else {
        return;
    };
    for mut a in &mut actors {
        if a.lit_at.is_some_and(|p| p.distance(a.position) < 1.5) {
            continue;
        }
        a.lit_at = Some(a.position);
        let e = halo3_rider::object_light(Some(&lights), to_halo(a.position) + Vec3::Z * 0.2, loaded.shown);
        if let Some(look) = a.model.and_then(|m| ai.looks.get(m)) {
            for (h, i) in &a.materials {
                if let (Some(mut m), Some(piece)) = (materials.get_mut(h), look.pieces.get(*i)) {
                    *m = piece.material(e);
                }
            }
        }
        if let Some(g) = &a.gun {
            for (h, look) in &g.materials {
                if let Some(mut m) = materials.get_mut(h) {
                    *m = look.lit(e);
                }
            }
        }
    }
}

/// The hit shapes for this frame's rounds.
fn targets(ai: Option<Res<Ai>>, actors: Query<(Entity, &Actor)>, globals: Query<&GlobalTransform>, mut out: ResMut<Targets>) {
    out.0.clear();
    let Some(ai) = ai else {
        return;
    };
    for (e, a) in &actors {
        if !a.alive() {
            continue;
        }
        let Some(kind) = ai.assets.kinds.get(a.kind).and_then(Option::as_ref) else {
            continue;
        };
        let r = (kind.radius * WU).max(0.1);
        let h = (kind.height * WU).max(r * 2.0);
        let head = a
            .head_joint
            .and_then(|j| globals.get(j).ok())
            .map(|g| (g.translation(), (r * 0.6).max(0.09)));
        out.0.push(Target {
            entity: e,
            team: a.team,
            base: a.position + Vec3::Y * r,
            top: a.position + Vec3::Y * (h - r),
            radius: r,
            head,
        });
    }
}
