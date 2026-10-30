//! Actors move with Halo 3's own animations (`halo3_anim`): each actor
//! with an animation graph gets an [`Animator`], and every frame its
//! `ActorActivity` picks what plays: idle, a move in the direction it goes
//! (sped up or slowed to its ground speed so the feet do not slide), a
//! melee swing, a death, the stance's aim overlay towards what it looks at,
//! and `fire_1` while it fires.
//!
//! The pose is written into the actor's joint entities' local transforms
//! (`ActorSkeleton::joints`, node relative to parent in world units, the
//! same frame a `jmad` frame is in), so the skinned meshes, the gun in the
//! right hand and the head the rounds aim for all follow the posed
//! skeleton through bevy's own transform propagation. That replaces the
//! HANDOFF's first sketch (a child entity with `AnimJoints` writing global
//! transforms after propagation), which would have needed the weapons
//! re-parented by hand.

use std::sync::Arc;

use bevy::prelude::*;

use halo3_anim::{AnimModel, Animator};
use halo3_core::ai::{ActorActivity, ActorSkeleton, ActorState};
use halo3_weapons::Armory;

use crate::AiSet;
use crate::place::{Actor, Ai};

pub(crate) struct AnimatePlugin;

impl Plugin for AnimatePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (start, animate)
                .chain()
                .after(AiSet::Hurt)
                .run_if(halo3_core::pause::running),
        );
    }
}

/// One actor's animation state beyond the [`Animator`].
#[derive(Component)]
pub(crate) struct Animated {
    last: ActorState,
    /// The death plays as an animation (the AI does not tip the body over).
    pub has_death: bool,
    /// Seconds until the next `fire_1`.
    fire_wait: f32,
}

/// The weapon class and type an actor's graph has animations for, given
/// its weapon's (`rifle` / `ar`). Flood combat forms keep their rifles under
/// `pistol`; the Flood elite has only `any`; the Arbiter's elite graph has
/// the real classes.
pub fn pick_stance(model: &AnimModel, weapon: Option<(&str, &str)>) -> (String, String) {
    let combat: Vec<_> = model
        .library
        .graphs
        .values()
        .flat_map(|g| g.modes.iter())
        .filter(|m| m.label == "combat")
        .collect();
    let has = |class: &str, kind: &str| {
        combat.iter().any(|m| {
            m.classes
                .iter()
                .any(|c| c.label == class && c.types.iter().any(|t| t.label == kind && !t.actions.is_empty()))
        })
    };
    let Some((class, kind)) = weapon else {
        for class in ["unarmed", "any"] {
            if has(class, "any") {
                return (class.into(), "any".into());
            }
        }
        return ("any".into(), "any".into());
    };
    if has(class, kind) {
        return (class.into(), kind.into());
    }
    // Another class with this weapon type.
    for m in &combat {
        for c in &m.classes {
            if c.types.iter().any(|t| t.label == kind && !t.actions.is_empty()) {
                return (c.label.clone(), kind.into());
            }
        }
    }
    for class in [class, "rifle", "pistol", "any", "unarmed"] {
        if has(class, "any") {
            return (class.into(), "any".into());
        }
    }
    ("any".into(), "any".into())
}

/// The ground speed of the graph's run (`move_front`), wu/s: Halo moves
/// actors by it.
pub fn run_speed(model: &AnimModel) -> Option<f32> {
    ["any", "unarmed", "rifle", "pistol"]
        .iter()
        .filter_map(|c| model.action("combat", c, "any", "move_front"))
        .find_map(|id| model.root_speed(id))
        .filter(|s| (0.3..6.0).contains(s))
}

/// A death the graph has, by its full name.
fn death_animation(model: &AnimModel, class: &str) -> Option<&'static str> {
    const NAMES: [&str; 8] = [
        "h_kill:front:gut",
        "h_kill:front:gut:var1",
        "s_kill:front:gut",
        "s_kill:front:gut:var1",
        "h_kill:back:gut",
        "s_kill:back:gut",
        "landing_dead",
        "airborne_dead",
    ];
    NAMES.into_iter().find(|n| {
        [class, "any", "unarmed"]
            .iter()
            .any(|c| model.named(&format!("combat:{c}:{n}")).is_some())
    })
}

fn play_death(a: &mut Animator, name: &str) -> bool {
    let class = a.stance.class.clone();
    for c in [class.as_str(), "any", "unarmed"] {
        if a.play_named(&format!("combat:{c}:{name}"), false) {
            return true;
        }
    }
    false
}

/// Gives newly placed actors their animator, when their unit has a graph
/// whose skeleton matches the drawn one.
fn start(
    mut commands: Commands,
    ai: Option<Res<Ai>>,
    armory: Option<Res<Armory>>,
    mut actors: Query<(Entity, &Actor, &mut ActorSkeleton), (Added<ActorSkeleton>, Without<Animated>)>,
) {
    let Some(ai) = ai else { return };
    for (e, actor, mut skeleton) in &mut actors {
        let Some(model) = ai.assets.kinds.get(actor.kind).and_then(Option::as_ref).and_then(|k| k.anim.clone()) else {
            continue;
        };
        if skeleton.joints.is_empty()
            || model.skeleton.len() != skeleton.joints.len()
            || model.skeleton.names.iter().zip(&skeleton.names).any(|(a, b)| a != b)
        {
            continue;
        }
        let weapon = actor
            .gun
            .as_ref()
            .and_then(|g| armory.as_deref()?.kinds.get(g.kind))
            .map(|k| (k.weapon.class.as_str(), k.weapon.short_name.as_str()));
        let (class, kind) = pick_stance(&model, weapon);
        let has_death = !actor.flying && death_animation(&model, &class).is_some();
        let mut a = Animator::new(Arc::clone(&model)).with_seed(e.to_bits() as u32 ^ 0x5bd1_e995);
        a.set_stance("combat", &class, &kind);
        a.play("idle");
        skeleton.posed_externally = true;
        commands.entity(e).insert((
            a,
            Animated {
                last: ActorState::Idle,
                has_death,
                fire_wait: 0.0,
            },
        ));
    }
}

/// Signed angle `a - b`, wrapped to -pi..pi.
fn wrap(a: f32) -> f32 {
    let t = std::f32::consts::TAU;
    let mut d = a % t;
    if d > std::f32::consts::PI {
        d -= t;
    } else if d < -std::f32::consts::PI {
        d += t;
    }
    d
}

/// The action for a ground velocity relative to the facing (Halo frame).
fn move_action(activity: &ActorActivity) -> (&'static str, f32) {
    let v = Vec2::new(activity.velocity.x, activity.velocity.y);
    let (s, c) = activity.yaw.sin_cos();
    let front = v.dot(Vec2::new(c, s));
    let left = v.dot(Vec2::new(-s, c));
    let action = if front.abs() >= left.abs() {
        if front >= 0.0 { "move_front" } else { "move_back" }
    } else if left > 0.0 {
        "move_left"
    } else {
        "move_right"
    };
    (action, v.length())
}

fn animate(
    time: Res<Time>,
    mut actors: Query<(&ActorActivity, &ActorSkeleton, &mut Animator, &mut Animated)>,
    mut joints: Query<&mut Transform>,
) {
    let dt = time.delta_secs();
    for (activity, skeleton, mut a, mut anim) in &mut actors {
        let state = activity.state;
        let entered = state != anim.last;
        anim.last = state;
        let (action, speed) = move_action(activity);
        let moving = !activity.flying && speed > 0.1;
        match state {
            ActorState::Dying | ActorState::Dead => {
                if entered && state == ActorState::Dying && anim.has_death {
                    let name = death_animation(&a.model, &a.stance.class.clone());
                    if let Some(n) = name {
                        play_death(&mut a, n);
                    }
                    a.clear_overlays();
                    a.stop_aiming();
                }
            }
            ActorState::Melee if entered => {
                if !a.play_once("melee") {
                    a.overlay("melee", false);
                }
            }
            _ if a.current() == Some("melee") && !a.finished() => {}
            _ => {
                if moving {
                    if !a.play(action) {
                        a.play("move_front");
                    }
                    // Played at the actor's own speed: no sliding feet.
                    let rate = a.base_root_speed().map_or(1.0, |r| (speed / r.max(0.05)).clamp(0.3, 2.5));
                    a.set_speed(rate);
                } else {
                    a.play("idle");
                    a.set_speed(1.0);
                }
            }
        }
        if !matches!(state, ActorState::Dying | ActorState::Dead) {
            // Aim at what it looks at, relative to the body.
            let aim = activity.aim;
            if aim.length_squared() > 1e-6 {
                let yaw = wrap(aim.y.atan2(aim.x) - activity.yaw);
                let pitch = aim.z.clamp(-1.0, 1.0).asin();
                a.aim(yaw, pitch, moving);
            }
            anim.fire_wait -= dt;
            if state == ActorState::Firing && anim.fire_wait <= 0.0 {
                a.overlay("fire_1", false);
                anim.fire_wait = 0.25;
            }
        }
        a.advance(dt);
        // Root motion is replaced by the playback rate above.
        a.take_root_motion();
        let pose = a.pose();
        for (x, &e) in pose.local.iter().zip(&skeleton.joints) {
            if let Ok(mut t) = joints.get_mut(e) {
                let n = Transform {
                    translation: x.translation,
                    rotation: x.rotation,
                    scale: Vec3::splat(x.scale),
                };
                if *t != n {
                    *t = n;
                }
            }
        }
    }
}
