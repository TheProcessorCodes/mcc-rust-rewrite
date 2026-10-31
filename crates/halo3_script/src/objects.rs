//! The state of the objects scripts touch: whether each exists, is
//! hidden, is falling, and where its device is. Pure data, stepped by the
//! runtime at 30 ticks a second; the bevy layer (`lib.rs`) draws it.

use glam::{Mat4, Quat, Vec3};

use crate::ground::Ground;
use crate::hooks::DevicePoser;

/// Halo's gravity, world units per second squared (9.81 m/s^2).
pub const GRAVITY: f32 = 9.81 / 3.048;
/// A dropped plate falls this long before it is gone.
pub const FALL_SECONDS: f32 = 6.0;
/// An animated falling device (a Warthog run island) loses its collision
/// once its root has dropped this far, world units (about 4.5 m): it stays
/// a floor while it creaks and tilts.
pub const DEVICE_DROP: f32 = 1.5;
/// A crate settling where it landed tips over at most this far, radians
/// (about 57 degrees; the tunnels' girders tip up to 31 when one end comes
/// down on a raised plate), looked at in steps of [`TILT_STEP`].
pub const MAX_TILT: f32 = 1.0;
/// A settling crate is held up when it touches the floor within this far
/// of its middle, either side (world units).
const HELD: f32 = 0.3;
const TILT_STEP: f32 = 0.002;

/// One damage section of an object's model, as the runtime needs it.
#[derive(Clone, Debug)]
pub struct Section {
    pub name: String,
    /// The effect tag the section's response plays, by path.
    pub effect: Option<String>,
}

/// What does not change about an object.
#[derive(Clone, Debug)]
pub struct ObjectInfo {
    pub name: Option<String>,
    /// `Scene::objects` index, when the object is drawn.
    pub scene: Option<u32>,
    pub object_type: i16,
    /// Where the scenario places it, Halo frame.
    pub base: Mat4,
    pub folder: Option<u16>,
    pub starts_absent: bool,
    pub sections: Vec<Section>,
    pub tag_path: String,
    /// The object tag index (`mach` for devices), when drawn.
    pub tag: Option<u32>,
    /// The corners of its collision where the scenario places it, Halo
    /// frame, for a crate scripts can drop onto the floor
    /// ([`crate::ground`]); empty for anything else.
    pub contacts: Vec<Vec3>,
}

#[derive(Clone, Debug, Default)]
pub enum Motion {
    #[default]
    Still,
    /// Dropped by a damage section: waits `delay`, then falls from `from`
    /// (where it was when it let go). Over the chasm it tumbles away and is
    /// gone after [`FALL_SECONDS`]; a crate with a floor under it
    /// (`lands`) drops straight onto it.
    Falling {
        delay: f32,
        t: f32,
        from: Mat4,
        velocity: Vec3,
        spin_axis: Vec3,
        spin_rate: f32,
        lands: bool,
    },
    /// A dropped crate that has touched the floor tipping over, from where
    /// it touched (`from`) to where it rests (`to`) in `seconds`; not
    /// solid until it is there.
    Settling { from: Mat4, to: Mat4, t: f32, seconds: f32 },
    /// A dropped crate resting on the floor it fell onto: solid again
    /// where it lies, and falling on if that floor drops away.
    Landed,
}

/// A device machine's position (0..1) and the animation driving it.
#[derive(Clone, Debug, Default)]
pub struct Device {
    pub position: f32,
    from: f32,
    target: f32,
    duration: f32,
    elapsed: f32,
    /// The position track `device_set_position_track` picked.
    pub track: Option<String>,
    /// The overlay track and its value (0..1).
    pub overlay_track: Option<String>,
    pub overlay: f32,
    overlay_from: f32,
    overlay_target: f32,
    overlay_duration: f32,
    overlay_elapsed: f32,
    pub power: f32,
}

impl Device {
    /// Moves to `target` over `seconds` (immediately when zero).
    pub fn animate(&mut self, target: f32, seconds: f32) {
        self.from = self.position;
        self.target = target.clamp(0.0, 1.0);
        self.duration = seconds.max(0.0);
        self.elapsed = 0.0;
        if self.duration == 0.0 {
            self.position = self.target;
        }
    }
    pub fn animate_overlay(&mut self, target: f32, seconds: f32) {
        self.overlay_from = self.overlay;
        self.overlay_target = target.clamp(0.0, 1.0);
        self.overlay_duration = seconds.max(0.0);
        self.overlay_elapsed = 0.0;
        if self.overlay_duration == 0.0 {
            self.overlay = self.overlay_target;
        }
    }
    fn step(&mut self, dt: f32) -> bool {
        let mut moved = false;
        if self.duration > 0.0 && self.elapsed < self.duration {
            self.elapsed = (self.elapsed + dt).min(self.duration);
            let s = smooth(self.elapsed / self.duration);
            self.position = self.from + (self.target - self.from) * s;
            moved = true;
        }
        if self.overlay_duration > 0.0 && self.overlay_elapsed < self.overlay_duration {
            self.overlay_elapsed = (self.overlay_elapsed + dt).min(self.overlay_duration);
            let s = smooth(self.overlay_elapsed / self.overlay_duration);
            self.overlay = self.overlay_from + (self.overlay_target - self.overlay_from) * s;
            moved = true;
        }
        moved
    }
    /// Whether the position track drops the device out of the world (the
    /// Warthog run's islands: `other_fall`).
    pub fn falls(&self) -> bool {
        self.track.as_deref().is_some_and(|t| t.contains("fall"))
    }
}

/// A crate's fall onto the floor under it: straight down from `from`, no
/// push, no spin (it lands the way up it hung).
fn straight_down(delay: f32, from: Mat4) -> Motion {
    Motion::Falling {
        delay,
        t: 0.0,
        from,
        velocity: Vec3::ZERO,
        spin_axis: Vec3::Z,
        spin_rate: 0.0,
        lands: true,
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What changes.
#[derive(Clone, Debug)]
pub struct ObjectState {
    pub exists: bool,
    pub hidden: bool,
    /// Halo frame, where it is now.
    pub transform: Mat4,
    pub motion: Motion,
    pub device: Device,
    /// Its collision is solid (exists, not falling, not dropped by its device).
    pub solid: bool,
    pub cannot_die: bool,
}

/// Every object scripts can reach: the scene's placed objects first (by
/// `Scene::objects` index), then named objects nothing draws (sound
/// scenery, bipeds, vehicles the AI places).
#[derive(Default)]
pub struct Objects {
    pub info: Vec<ObjectInfo>,
    pub state: Vec<ObjectState>,
    /// Object name index -> entry.
    pub by_name: Vec<Option<u32>>,
    /// Bumped whenever something visible changed.
    pub generation: u64,
    seed: u32,
    /// What dropped crates land on ([`crate::ground`]); without it every
    /// dropped object tumbles away.
    pub ground: Option<Ground>,
}

impl Objects {
    /// `by_name`: the entry of each object name index.
    pub fn new(info: Vec<ObjectInfo>, by_name: Vec<Option<u32>>) -> Objects {
        let state = info
            .iter()
            .map(|o| ObjectState {
                exists: !o.starts_absent,
                hidden: false,
                transform: o.base,
                motion: Motion::Still,
                device: Device {
                    power: 1.0,
                    ..Default::default()
                },
                solid: !o.starts_absent,
                cannot_die: false,
            })
            .collect();
        Objects {
            info,
            state,
            by_name,
            generation: 1,
            seed: 0x9E37_79B9,
            ground: None,
        }
    }

    pub fn len(&self) -> usize {
        self.info.len()
    }
    pub fn is_empty(&self) -> bool {
        self.info.is_empty()
    }

    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn position(&self, i: usize) -> Vec3 {
        self.state[i].transform.w_axis.truncate()
    }

    /// Puts an object (back) in the world where the scenario placed it.
    pub fn create(&mut self, i: usize, anew: bool) {
        let s = &mut self.state[i];
        if s.exists && !anew {
            return;
        }
        s.exists = true;
        s.hidden = false;
        s.transform = self.info[i].base;
        s.motion = Motion::Still;
        s.device.animate(0.0, 0.0);
        s.solid = true;
        self.generation += 1;
    }

    pub fn destroy(&mut self, i: usize) {
        let s = &mut self.state[i];
        if !s.exists {
            return;
        }
        s.exists = false;
        s.solid = false;
        s.motion = Motion::Still;
        self.generation += 1;
    }

    pub fn hide(&mut self, i: usize, hidden: bool) {
        if self.state[i].hidden != hidden {
            self.state[i].hidden = hidden;
            self.generation += 1;
        }
    }

    /// Drops an object: its collision goes at once, it falls after `delay`.
    pub fn drop_object(&mut self, i: usize, delay: f32) {
        if !self.state[i].exists || matches!(self.state[i].motion, Motion::Falling { .. } | Motion::Settling { .. } | Motion::Landed) {
            return;
        }
        let lands = self.clearance(i).is_some();
        let (a, b, c, d) = (self.random(), self.random(), self.random(), self.random());
        let s = &mut self.state[i];
        s.solid = false;
        s.motion = if lands {
            straight_down(delay, s.transform)
        } else {
            Motion::Falling {
                delay,
                t: 0.0,
                from: s.transform,
                // A little push, mostly down.
                velocity: Vec3::new(a - 0.5, b - 0.5, -0.5) * 1.5,
                spin_axis: Vec3::new(c - 0.5, d - 0.5, 0.15).normalize_or(Vec3::X),
                spin_rate: 0.6 + a * 1.4,
                lands: false,
            }
        };
        self.generation += 1;
    }

    /// How far a crate that can land ([`ObjectInfo::contacts`]) would drop
    /// straight down from where it is now before it touches the floor: the
    /// structure BSPs and solid objects still where the scenario put them
    /// (ones a script moved are not looked at). `None` without a floor
    /// within [`crate::ground::LAND_REACH`], and for anything else.
    pub fn clearance(&self, i: usize) -> Option<f32> {
        let at = self.state[i].transform;
        self.clearance_at(i, at, at).filter(|c| *c <= crate::ground::LAND_REACH)
    }

    /// The same for the crate placed at `transform`, however far down;
    /// negative when it is sunk into the floor. The floor under each corner
    /// is looked for below where that corner is at `free` (a pose known to
    /// be clear of the floor) or at `transform`, whichever is higher.
    fn clearance_at(&self, i: usize, transform: Mat4, free: Mat4) -> Option<f32> {
        self.gaps_at(i, transform, free).into_iter().map(|(_, c)| c).reduce(f32::min)
    }

    /// Per corner of a crate placed at `transform` that has floor under it:
    /// where the corner is and how far above the floor (as
    /// [`Self::clearance_at`]).
    fn gaps_at(&self, i: usize, transform: Mat4, free: Mat4) -> Vec<(Vec3, f32)> {
        let (Some(ground), info) = (self.ground.as_ref(), &self.info[i]) else {
            return Vec::new();
        };
        let skip = |o: u32| {
            o as usize == i
                || match (self.state.get(o as usize), self.info.get(o as usize)) {
                    (Some(s), Some(n)) => !(s.exists && s.solid && s.transform == n.base),
                    _ => true,
                }
        };
        let local = info.base.inverse();
        let (motion, clear) = (transform * local, free * local);
        info.contacts
            .iter()
            .filter_map(|p| {
                let q = motion.transform_point3(*p);
                let top = q.z.max(clear.transform_point3(*p).z);
                ground.below(Vec3::new(q.x, q.y, top), skip).map(|z| (q, q.z - z))
            })
            .collect()
    }

    /// Where a crate that has just touched the floor at `at` comes to rest.
    /// Held up under its middle (the middle of its collision, along its
    /// length) it stays as it is. Touching only to one side of its middle (a
    /// long girder come down with one end on a raised edge) it tips over
    /// that way, about the axis across its length, until it touches on the
    /// other side too: at most [`MAX_TILT`]; if nothing is there by then it
    /// stays as it touched.
    fn rest_pose(&self, i: usize, at: Mat4) -> Mat4 {
        let info = &self.info[i];
        if info.contacts.is_empty() {
            return at;
        }
        let local = info.base.inverse();
        let (lo, hi) = info.contacts.iter().map(|p| local.transform_point3(*p)).fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(p), hi.max(p)),
        );
        let size = hi - lo;
        let (length, across) = if size.x >= size.y { (Vec3::X, Vec3::Y) } else { (Vec3::Y, Vec3::X) };
        let mut axis = at.transform_vector3(across);
        axis.z = 0.0;
        let Some(axis) = axis.try_normalize() else { return at };
        let middle = (lo + hi) * 0.5;
        // Where along its length (from its middle) it touches the floor at
        // `pose`: the nearest touch either side.
        let touches = |pose: Mat4| -> Option<(f32, f32)> {
            let c = pose.transform_point3(middle);
            let along = pose.transform_vector3(length).normalize_or_zero();
            self.gaps_at(i, pose, at)
                .into_iter()
                .filter(|(_, g)| *g < 0.03)
                .map(|(p, _)| (p - c).dot(along))
                .fold(None, |r: Option<(f32, f32)>, d| Some(r.map_or((d, d), |(a, b)| (a.min(d), b.max(d)))))
        };
        let held = |(a, b): (f32, f32)| a <= HELD && b >= -HELD;
        let Some(first) = touches(at) else { return at };
        if held(first) {
            return at;
        }
        // The free end goes down: the side of its middle away from the touch.
        let centre = at.transform_point3(middle);
        let free_end = centre + at.transform_vector3(length).normalize_or_zero() * if first.1 < 0.0 { 5.0 } else { -5.0 };
        let sign = if Mat4::from_axis_angle(axis, TILT_STEP).transform_vector3(free_end - centre).z < (free_end - centre).z {
            1.0
        } else {
            -1.0
        };
        let steps = (MAX_TILT / TILT_STEP).round() as i32;
        for k in 1..=steps {
            let pose = Mat4::from_translation(centre)
                * Mat4::from_axis_angle(axis, sign * k as f32 * TILT_STEP)
                * Mat4::from_translation(-centre)
                * at;
            let Some(drop) = self.clearance_at(i, pose, at) else { continue };
            let pose = Mat4::from_translation(Vec3::new(0.0, 0.0, -drop)) * pose;
            if touches(pose).is_some_and(held) {
                return pose;
            }
        }
        at
    }

    /// Advances falls and devices by `dt` seconds.
    pub fn step(&mut self, dt: f32, poser: Option<&dyn DevicePoser>) {
        let mut changed = false;
        // Crates that touched the floor this tick, and where.
        let mut settle: Vec<(usize, Mat4)> = Vec::new();
        for i in 0..self.state.len() {
            let base = self.info[i].base;
            let scene = self.info[i].scene;
            let tag = self.info[i].tag;
            // The room under a landing or landed crate, before it moves.
            let room = match &self.state[i].motion {
                _ if !self.state[i].exists => None,
                Motion::Falling { lands: true, delay, .. } if *delay <= 0.0 => self.clearance(i),
                Motion::Landed => self.clearance(i),
                _ => None,
            };
            let s = &mut self.state[i];
            if !s.exists {
                continue;
            }
            if matches!(s.motion, Motion::Landed) {
                // The floor under it dropped away: on down.
                if room.is_none_or(|r| r > 0.02) {
                    s.solid = false;
                    s.motion = straight_down(0.0, s.transform);
                    changed = true;
                }
                continue;
            }
            if let Motion::Settling { from, to, t, seconds } = &mut s.motion {
                *t += dt;
                let k = (*t / *seconds).min(1.0);
                if k >= 1.0 {
                    s.transform = *to;
                    s.motion = Motion::Landed;
                    s.solid = true;
                } else {
                    // Tipping over, faster and faster.
                    let (scale, r0, p0) = from.to_scale_rotation_translation();
                    let (_, r1, p1) = to.to_scale_rotation_translation();
                    let e = k * k;
                    s.transform = Mat4::from_scale_rotation_translation(scale, r0.slerp(r1, e), p0.lerp(p1, e));
                }
                changed = true;
                continue;
            }
            if let Motion::Falling {
                delay,
                t,
                from,
                velocity,
                spin_axis,
                spin_rate,
                lands,
            } = &mut s.motion
            {
                if *delay > 0.0 {
                    *delay -= dt;
                    continue;
                }
                let before = *t;
                *t += dt;
                let gone = *t > FALL_SECONDS;
                if *lands {
                    let (d0, d1) = (0.5 * GRAVITY * before * before, 0.5 * GRAVITY * *t * *t);
                    match room {
                        // It reaches the floor this tick: it settles there
                        // and is solid again.
                        Some(r) if r <= d1 - d0 => {
                            let at = Mat4::from_translation(Vec3::new(0.0, 0.0, -(d0 + r))) * *from;
                            s.transform = at;
                            settle.push((i, at));
                            changed = true;
                            continue;
                        }
                        _ => s.transform = Mat4::from_translation(Vec3::new(0.0, 0.0, -d1)) * *from,
                    }
                } else {
                    let fall = *velocity * *t + Vec3::new(0.0, 0.0, -0.5 * GRAVITY * *t * *t);
                    let spin = Quat::from_axis_angle(*spin_axis, *spin_rate * *t);
                    let origin = from.w_axis.truncate();
                    // Tumble about the object's own origin.
                    s.transform = Mat4::from_translation(origin + fall)
                        * Mat4::from_quat(spin)
                        * Mat4::from_translation(-origin)
                        * *from;
                }
                if gone {
                    s.exists = false;
                    s.motion = Motion::Still;
                }
                changed = true;
                continue;
            }
            if s.device.step(dt) || s.device.position > 0.0 || s.device.overlay > 0.0 {
                let pose = scene.and_then(|o| {
                    poser.and_then(|p| p.pose(o, tag?, s.device.track.as_deref(), s.device.position, s.device.overlay_track.as_deref(), s.device.overlay))
                });
                let (t, solid) = match pose {
                    // Its own animation: solid while it tilts, gone once it
                    // has dropped (the collision does not move with it).
                    Some(m) => {
                        let t = base * m;
                        let anchor = tag.and_then(|t| poser?.anchor(t)).unwrap_or(Vec3::ZERO);
                        let drop = base.transform_point3(anchor).z - t.transform_point3(anchor).z;
                        (t, !(s.device.falls() && drop > DEVICE_DROP))
                    }
                    None => (fallback_device_pose(base, &s.device), !(s.device.falls() && s.device.position > 0.02)),
                };
                if t != s.transform {
                    s.transform = t;
                    changed = true;
                }
                if solid != s.solid {
                    s.solid = solid;
                    changed = true;
                }
            }
        }
        for (i, at) in settle {
            let to = self.rest_pose(i, at);
            let (_, r0, _) = at.to_scale_rotation_translation();
            let (_, r1, _) = to.to_scale_rotation_translation();
            let angle = r0.angle_between(r1);
            let s = &mut self.state[i];
            if angle < 1e-3 {
                s.transform = to;
                s.motion = Motion::Landed;
                s.solid = true;
            } else {
                s.motion = Motion::Settling {
                    from: at,
                    to,
                    t: 0.0,
                    seconds: 0.2 + angle * 1.2,
                };
            }
        }
        if changed {
            self.generation += 1;
        }
    }
}

/// A made-up device pose for when no animation is known: a falling track
/// drops the device away and tips it; an overlay tilts it a little. Any
/// other track leaves it where it stands.
pub fn fallback_device_pose(base: Mat4, d: &Device) -> Mat4 {
    let mut m = base;
    if d.overlay > 0.0 {
        m *= Mat4::from_rotation_y(-0.08 * d.overlay);
    }
    if d.falls() && d.position > 0.0 {
        let p = d.position;
        let origin = base.w_axis.truncate();
        m = Mat4::from_translation(origin + Vec3::new(0.0, 0.0, -60.0 * p * p))
            * Mat4::from_rotation_x(0.6 * p)
            * Mat4::from_translation(-origin)
            * m;
    }
    m
}
