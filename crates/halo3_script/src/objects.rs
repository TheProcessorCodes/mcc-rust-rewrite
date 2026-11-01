//! The state of the objects scripts touch: whether each exists, is
//! hidden, is falling, and where its device is. Pure data, stepped by the
//! runtime at 30 ticks a second; the bevy layer (`lib.rs`) draws it.

use glam::{Mat4, Quat, Vec3};

use crate::crate_physics::{Body, Floor, Swing};
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
/// One damage section of an object's model, as the runtime needs it.
#[derive(Clone, Debug)]
pub struct Section {
    pub name: String,
    /// The effect tags the section's responses play, by path: each
    /// response's effect and its second effect (a plate's `destroy` plays
    /// its chunks and the trench's `plate_explosion`).
    pub effects: Vec<String>,
    /// What its responses do to the physics model's constraint, if
    /// anything (the tunnel girders' `fall` loosens their hinge, `destroy`
    /// breaks it).
    pub constraint: Option<ConstraintDamage>,
}

/// A damage response's effect on a constraint of the physics model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstraintDamage {
    Loosen,
    Destroy,
}

/// The physics model's numbers for a crate that can be let go.
#[derive(Clone, Debug)]
pub struct CratePhysics {
    pub mass: f32,
    pub friction: f32,
    pub restitution: f32,
    /// The constraint holding it up, object frame at its placement
    /// (`base`): pivot, twist and plane axes, cone and plane limits.
    pub hinge: Option<Hinge>,
}

#[derive(Clone, Debug)]
pub struct Hinge {
    pub pivot: Vec3,
    pub twist_axis: Vec3,
    pub plane_axis: Vec3,
    pub cone: f32,
    pub plane: (f32, f32),
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
    /// Its physics model, for a crate that can be let go.
    pub physics: Option<CratePhysics>,
}

#[derive(Clone, Debug, Default)]
pub enum Motion {
    #[default]
    Still,
    /// Dropped by a damage section (a floor plate): waits `delay`, then
    /// falls from `from` (where it was when it let go), pushed and
    /// tumbling, and is gone after [`FALL_SECONDS`].
    Falling {
        delay: f32,
        t: f32,
        from: Mat4,
        velocity: Vec3,
        spin_axis: Vec3,
        spin_rate: f32,
    },
    /// A crate whose constraint was loosened, swinging on its hinge (and
    /// at rest hanging there once [`Swing::resting`]). Solid throughout.
    Swinging(Swing),
    /// A crate falling free, bouncing and sliding on the floor until it
    /// sleeps ([`Body::asleep`]); on again if the floor drops away. Solid
    /// throughout.
    Tumbling(Body),
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

    /// Drops an object: a crate with a floor under it falls free onto it
    /// ([`Self::release`]); anything else (a floor plate) loses its
    /// collision at once and tumbles away after `delay`.
    pub fn drop_object(&mut self, i: usize, delay: f32) {
        if !self.state[i].exists || !matches!(self.state[i].motion, Motion::Still) {
            return;
        }
        if self.clearance(i).is_some() {
            self.release(i);
            return;
        }
        let (a, b, c, d) = (self.random(), self.random(), self.random(), self.random());
        let s = &mut self.state[i];
        s.solid = false;
        s.motion = Motion::Falling {
            delay,
            t: 0.0,
            from: s.transform,
            // A little push, mostly down.
            velocity: Vec3::new(a - 0.5, b - 0.5, -0.5) * 1.5,
            spin_axis: Vec3::new(c - 0.5, d - 0.5, 0.15).normalize_or(Vec3::X),
            spin_rate: 0.6 + a * 1.4,
        };
        self.generation += 1;
    }

    /// Loosens the constraint holding a crate up: it swings down on its
    /// hinge. Without one it falls free instead.
    pub fn loosen(&mut self, i: usize) {
        if !self.state[i].exists || !matches!(self.state[i].motion, Motion::Still) {
            return;
        }
        let info = &self.info[i];
        let (Some(physics), false) = (info.physics.as_ref(), info.contacts.is_empty()) else {
            return self.drop_object(i, 0.0);
        };
        let Some(h) = physics.hinge.as_ref() else {
            return self.release(i);
        };
        let s = &self.state[i];
        let motion = s.transform * info.base.inverse();
        let corners: Vec<Vec3> = info.contacts.iter().map(|p| motion.transform_point3(*p)).collect();
        let frame = s.transform;
        let swing = Swing::new(
            s.transform,
            &corners,
            frame.transform_point3(h.pivot),
            frame.transform_vector3(h.twist_axis).normalize_or(Vec3::Z),
            frame.transform_vector3(h.plane_axis).normalize_or(Vec3::X),
            h.cone,
            h.plane,
            physics.restitution,
        );
        let s = &mut self.state[i];
        s.solid = true;
        s.motion = Motion::Swinging(swing);
        self.generation += 1;
    }

    /// Lets a crate fall free: off its hinge if it was swinging (keeping
    /// the swing's motion), else with a small push and spin of our own (the
    /// scripts give none).
    pub fn release(&mut self, i: usize) {
        let info = &self.info[i];
        if !self.state[i].exists || info.contacts.is_empty() || matches!(self.state[i].motion, Motion::Tumbling(_)) {
            return;
        }
        let (friction, restitution) = info.physics.as_ref().map_or((0.8, 0.35), |p| (p.friction, p.restitution));
        let s = &self.state[i];
        let motion = s.transform * info.base.inverse();
        let corners: Vec<Vec3> = info.contacts.iter().map(|p| motion.transform_point3(*p)).collect();
        let middle = crate::crate_physics::centre(&corners);
        let from = s.transform;
        let (velocity, spin) = match &s.motion {
            // Off its hinge: as it was swinging, nothing more.
            Motion::Swinging(w) => (w.velocity_at(middle), w.spin()),
            // Our own small sideways push and a spin about the vertical
            // (none that would lift an end up through the ceiling).
            _ => {
                let (a, b, c) = (self.random(), self.random(), self.random());
                (Vec3::new(a - 0.5, b - 0.5, 0.0) * 0.8, Vec3::new(0.0, 0.0, (c - 0.5) * 0.4))
            }
        };
        let body = Body::new(from, &corners, velocity, spin, friction, restitution);
        let s = &mut self.state[i];
        s.solid = true;
        s.motion = Motion::Tumbling(body);
        self.generation += 1;
    }

    /// How far a crate that can be let go ([`ObjectInfo::contacts`]) would
    /// drop straight down from where it is now before it touches the floor:
    /// the structure BSPs and solid objects still where the scenario put
    /// them (ones a script moved are not looked at). `None` without a floor
    /// within [`crate::ground::LAND_REACH`], and for anything else.
    pub fn clearance(&self, i: usize) -> Option<f32> {
        let at = self.state[i].transform;
        self.gaps_at(i, at, at)
            .into_iter()
            .map(|(_, g, _)| g)
            .reduce(f32::min)
            .filter(|c| *c <= crate::ground::LAND_REACH)
    }

    /// Per corner of a crate placed at `transform` that has floor under it:
    /// where the corner is, how far above the floor (negative when sunk
    /// in) and the floor's normal. The floor under each corner is looked
    /// for below where that corner is at `free` (a pose known to be clear
    /// of the floor) or at `transform`, whichever is higher.
    pub fn gaps_at(&self, i: usize, transform: Mat4, free: Mat4) -> Vec<(Vec3, f32, Vec3)> {
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
                ground.below_hit(Vec3::new(q.x, q.y, top), skip).map(|(z, _, n)| (q, q.z - z, n))
            })
            .collect()
    }

    /// Advances falls and devices by `dt` seconds.
    pub fn step(&mut self, dt: f32, poser: Option<&dyn DevicePoser>) {
        let mut changed = false;
        for i in 0..self.state.len() {
            let base = self.info[i].base;
            let scene = self.info[i].scene;
            let tag = self.info[i].tag;
            if !self.state[i].exists {
                continue;
            }
            // A crate let go: stepped against the floor as it is now.
            if let Motion::Swinging(_) | Motion::Tumbling(_) = &self.state[i].motion {
                let mut motion = self.state[i].motion.clone();
                let floor = FloorOf { objects: self, i };
                let (pose, gone) = match &mut motion {
                    Motion::Swinging(w) => {
                        w.step(dt, &floor);
                        (w.pose(), false)
                    }
                    Motion::Tumbling(b) => {
                        b.step(dt, &floor);
                        // Nothing under it within reach: gone, as a plate.
                        (b.pose(), !b.touched && b.t > FALL_SECONDS)
                    }
                    _ => unreachable!(),
                };
                let s = &mut self.state[i];
                if gone {
                    s.exists = false;
                    s.solid = false;
                    s.motion = Motion::Still;
                    changed = true;
                    continue;
                }
                if pose != s.transform {
                    s.transform = pose;
                    changed = true;
                }
                s.motion = motion;
                continue;
            }
            let s = &mut self.state[i];
            if let Motion::Falling {
                delay,
                t,
                from,
                velocity,
                spin_axis,
                spin_rate,
            } = &mut s.motion
            {
                if *delay > 0.0 {
                    *delay -= dt;
                    continue;
                }
                *t += dt;
                let fall = *velocity * *t + Vec3::new(0.0, 0.0, -0.5 * GRAVITY * *t * *t);
                let spin = Quat::from_axis_angle(*spin_axis, *spin_rate * *t);
                let origin = from.w_axis.truncate();
                // Tumble about the object's own origin.
                s.transform = Mat4::from_translation(origin + fall)
                    * Mat4::from_quat(spin)
                    * Mat4::from_translation(-origin)
                    * *from;
                if *t > FALL_SECONDS {
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

/// The floor under crate `i` as the objects are now.
struct FloorOf<'a> {
    objects: &'a Objects,
    i: usize,
}

impl Floor for FloorOf<'_> {
    fn gaps(&self, pose: Mat4, free: Mat4) -> Vec<(Vec3, f32, Vec3)> {
        self.objects.gaps_at(self.i, pose, free)
    }
}
