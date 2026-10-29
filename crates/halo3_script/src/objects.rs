//! The state of the objects scripts touch: whether each exists, is
//! hidden, is falling, and where its device is. Pure data, stepped by the
//! runtime at 30 ticks a second; the bevy layer (`lib.rs`) draws it.

use glam::{Mat4, Quat, Vec3};

use crate::hooks::DevicePoser;

/// Halo's gravity, world units per second squared (9.81 m/s^2).
pub const GRAVITY: f32 = 9.81 / 3.048;
/// A dropped plate falls this long before it is gone.
pub const FALL_SECONDS: f32 = 6.0;

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
}

#[derive(Clone, Debug, Default)]
pub enum Motion {
    #[default]
    Still,
    /// Dropped by a damage section: waits `delay`, then falls and tumbles.
    Falling {
        delay: f32,
        t: f32,
        velocity: Vec3,
        spin_axis: Vec3,
        spin_rate: f32,
    },
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
        if !self.state[i].exists || matches!(self.state[i].motion, Motion::Falling { .. }) {
            return;
        }
        let (a, b, c, d) = (self.random(), self.random(), self.random(), self.random());
        let s = &mut self.state[i];
        s.solid = false;
        s.motion = Motion::Falling {
            delay,
            t: 0.0,
            // A little push, mostly down.
            velocity: Vec3::new(a - 0.5, b - 0.5, -0.5) * 1.5,
            spin_axis: Vec3::new(c - 0.5, d - 0.5, 0.15).normalize_or(Vec3::X),
            spin_rate: 0.6 + a * 1.4,
        };
        self.generation += 1;
    }

    /// Advances falls and devices by `dt` seconds.
    pub fn step(&mut self, dt: f32, poser: Option<&dyn DevicePoser>) {
        let mut changed = false;
        for i in 0..self.state.len() {
            let base = self.info[i].base;
            let scene = self.info[i].scene;
            let tag = self.info[i].tag;
            let s = &mut self.state[i];
            if !s.exists {
                continue;
            }
            if let Motion::Falling {
                delay,
                t,
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
                let origin = base.w_axis.truncate();
                // Tumble about the object's own origin.
                s.transform = Mat4::from_translation(origin + fall)
                    * Mat4::from_quat(spin)
                    * Mat4::from_translation(-origin)
                    * base;
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
                let t = match pose {
                    Some(m) => base * m,
                    None => fallback_device_pose(base, &s.device),
                };
                if t != s.transform {
                    s.transform = t;
                    changed = true;
                }
                let solid = !(s.device.falls() && s.device.position > 0.02);
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
