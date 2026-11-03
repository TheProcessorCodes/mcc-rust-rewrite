//! Halo CE's actors move with their own animation graphs (`antr`,
//! `blam_h1::animation`): idle, the move in the direction they go (played
//! at their ground speed so the feet do not slide), the melee swing. The
//! pose is written into the actor's joints (`ActorSkeleton`, node relative
//! to parent in world units, the frame an `antr` frame is in), so the
//! skinned meshes and the gun in the hand follow.
//!
//! Which animation: Halo CE names them `<mode> <class> <action>`
//! (`stand pistol move-front`, `alert unarmed idle%1`); for each actor
//! variant the first mode (`stand`, `alert`, `combat`, `crouch`) and class
//! (its weapon's: the Flood and the Covenant keep their guns under
//! `pistol`, the grunt's fuel rod under `missle`, the sentinel's beam under
//! `fixed`; `unarmed` without one) that has the action. Deaths are not
//! played: the dead keep their last pose and the AI lays them down.

use std::sync::Arc;

use bevy::prelude::*;
use blam_h1::animation::{Animation, Graph, NodePose};
use halo3_core::ai::{ActorActivity, ActorSkeleton, ActorState, ActorTag};

/// One actor variant's animations, picked once.
pub(crate) struct Moves {
    pub graph: Arc<Graph>,
    pub idle: Option<usize>,
    /// Front, back, left, right.
    pub moves: [Option<usize>; 4],
    pub melee: Option<usize>,
}

impl Moves {
    /// Picks `variant`'s animations from `graph` (`armed`: it carries a
    /// weapon; `missile`: a launcher).
    pub fn pick(graph: Arc<Graph>, armed: bool, missile: bool) -> Moves {
        let classes: &[&str] = match (armed, missile) {
            (true, true) => &["missle", "pistol", "rifle", "fixed", "unarmed"],
            (true, false) => &["pistol", "rifle", "fixed", "unarmed"],
            (false, _) => &["unarmed", "fixed", "pistol"],
        };
        let find = |action: &str| {
            for class in classes {
                for mode in ["stand", "alert", "combat", "crouch"] {
                    if let Some(i) = graph.find(&format!("{mode} {class} {action}")) {
                        let a = graph.animations[i].as_ref()?;
                        if a.kind != 1 {
                            return Some(i);
                        }
                    }
                }
            }
            None
        };
        let front = find("move-front");
        Moves {
            idle: find("idle"),
            moves: [front, find("move-back").or(front), find("move-left").or(front), find("move-right").or(front)],
            melee: find("melee"),
            graph,
        }
    }

    fn anim(&self, i: usize) -> Option<&Animation> {
        self.graph.animations.get(i)?.as_ref()
    }
}

/// Each actor variant's (character palette index) animations, for the
/// actors of a Halo CE mission.
#[derive(Resource, Default)]
pub(crate) struct H1Moves(pub Vec<Option<Arc<Moves>>>);

/// One actor's playback.
#[derive(Component)]
struct Playing {
    moves: Arc<Moves>,
    current: usize,
    /// Frames into it (30 a second).
    frame: f32,
    once: bool,
    last: ActorState,
}

pub(crate) struct H1AnimPlugin;

impl Plugin for H1AnimPlugin {
    fn build(&self, app: &mut App) {
        use halo3_core::map::MapAppExt;
        app.remove_on_map_unload::<H1Moves>().add_systems(
            Update,
            (start, animate)
                .chain()
                .after(halo3_ai::AiSet::Hurt)
                .run_if(resource_exists::<H1Moves>)
                .run_if(halo3_core::pause::running)
                .run_if(halo3_core::map::ready),
        );
    }
}

/// New actors with a graph whose nodes match their skeleton start playing.
fn start(
    mut commands: Commands,
    moves: Res<H1Moves>,
    mut added: Query<(Entity, &ActorTag, &mut ActorSkeleton), Added<ActorSkeleton>>,
) {
    for (e, tag, mut skeleton) in &mut added {
        let Some(m) = moves.0.get(tag.character).and_then(Option::as_ref) else {
            continue;
        };
        if skeleton.joints.is_empty() || skeleton.joints.len() != m.graph.nodes.len() {
            continue;
        }
        let Some(idle) = m.idle.or(m.moves[0]) else {
            continue;
        };
        skeleton.posed_externally = true;
        commands.entity(e).insert(Playing {
            moves: Arc::clone(m),
            current: idle,
            // Not all in step.
            frame: (e.to_bits() % 23) as f32,
            once: false,
            last: ActorState::Idle,
        });
    }
}

/// Front, back, left or right of the facing, and the ground speed (wu/s).
fn direction(a: &ActorActivity) -> (usize, f32) {
    let v = Vec2::new(a.velocity.x, a.velocity.y);
    let (s, c) = a.yaw.sin_cos();
    let front = v.dot(Vec2::new(c, s));
    let left = v.dot(Vec2::new(-s, c));
    let d = if front.abs() >= left.abs() {
        if front >= 0.0 { 0 } else { 1 }
    } else if left > 0.0 {
        2
    } else {
        3
    };
    (d, v.length())
}

fn blend(a: &NodePose, b: &NodePose, t: f32) -> Transform {
    Transform {
        translation: a.translation.lerp(b.translation, t),
        rotation: a.rotation.slerp(b.rotation, t),
        scale: Vec3::splat(a.scale + (b.scale - a.scale) * t),
    }
}

fn animate(
    time: Res<Time>,
    mut actors: Query<(&ActorActivity, &ActorSkeleton, &mut Playing)>,
    mut joints: Query<&mut Transform>,
) {
    let dt = time.delta_secs();
    for (activity, skeleton, mut p) in &mut actors {
        let state = activity.state;
        let entered = state != p.last;
        p.last = state;
        if matches!(state, ActorState::Dying | ActorState::Dead) {
            continue;
        }
        let m = Arc::clone(&p.moves);
        let (dir, speed) = direction(activity);
        let mut rate = 1.0;
        let busy = p.once && m.anim(p.current).is_some_and(|a| p.frame < a.frame_count as f32 - 1.0);
        let want = if state == ActorState::Melee && entered && m.melee.is_some() {
            p.once = true;
            m.melee
        } else if busy {
            Some(p.current)
        } else if !activity.flying && speed > 0.1 {
            p.once = false;
            let i = m.moves[dir].or(m.idle);
            // Played at the actor's own speed: no sliding feet.
            if let Some(a) = i.and_then(|i| m.anim(i)) {
                let r = a.root_speed();
                if r > 0.05 {
                    rate = (speed / r).clamp(0.3, 2.5);
                }
            }
            i
        } else {
            p.once = false;
            m.idle.or(m.moves[0])
        };
        let Some(want) = want else { continue };
        if want != p.current {
            p.current = want;
            p.frame = 0.0;
        }
        let Some(a) = m.anim(p.current) else { continue };
        p.frame += dt * 30.0 * rate;
        let n = a.frame_count as f32;
        if p.frame >= n - 1.0 {
            if p.once {
                p.frame = n - 1.0;
            } else {
                let start = (a.loop_frame as f32).min(n - 1.0);
                let span = (n - 1.0 - start).max(1.0);
                p.frame = start + (p.frame - start).rem_euclid(span);
            }
        }
        let f = p.frame.floor() as usize;
        let t = p.frame - f as f32;
        let (pa, pb) = (a.pose(f), a.pose((f + 1).min(a.frame_count - 1)));
        for ((x, y), &e) in pa.iter().zip(pb).zip(&skeleton.joints) {
            if let Ok(mut j) = joints.get_mut(e) {
                let n = blend(x, y, t);
                if *j != n {
                    *j = n;
                }
            }
        }
    }
}
