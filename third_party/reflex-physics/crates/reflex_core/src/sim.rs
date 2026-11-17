//! The bike simulation: frame stepping (spec 3) and the substep in the original order (spec 4, 15).
//!
//! Stages are filled in subsystem by subsystem (milestone 4); a stage not implemented yet is a comment in
//! [`Sim::substep`] so the order stays visible.

use crate::air::{self, AirState, Sticks};
use crate::bike::{build_system, Geometry, CHASSIS, FRONT, REAR, RIDER_JOINT};
use crate::chassis::{self, ChassisContacts};
use crate::contact::{self, Penetration};
use crate::controls::{self, ControlInputs, ControlState};
use crate::drivetrain::{self, Drivetrain};
use crate::lean::{self, LeanInputs, LeanState};
use crate::math::{Quat, Vec3};
use crate::multibody::{Joint, System};
use crate::params::{Garage, Params};
use crate::suspension;
use crate::tyre::{tyre_step, Tyre, TyreInputs};
use crate::vehicle::Vehicle;
use crate::wreck::{WreckChecker, WreckInputs, MIN_WRECK_BEFORE_RESET, WRECK_DURATION};
use crate::DT;

/// Most substeps per rendered frame; beyond this, time is dropped (the game slows). S+b8 = 6 (OpenReflex
/// build_vehicle_sim.c `00430b50`).
pub const MAX_SUBSTEPS: usize = 6;

/// The sim's input block (sim+0x1c..0x48), latched once per rendered frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Inputs {
    /// 0..1
    pub throttle: f64,
    /// Brake / reverse button, 0..1.
    pub brake_button: f64,
    pub front_brake: f64,
    pub rear_brake: f64,
    /// Left stick, -1..1 after the deadzone.
    pub steer_x: f64,
    pub steer_y: f64,
    /// Right stick (rider lean), -1..1 after the deadzone.
    pub rider_x: f64,
    pub rider_y: f64,
    pub clutch: bool,
}

/// A world triangle for the wheel-disc contact test (spec 9). `normal` points out of the surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Triangle {
    pub a: Vec3,
    pub b: Vec3,
    pub c: Vec3,
    pub normal: Vec3,
}

/// Ground the bike rides on, in Reflex units and frame. Engine adapters implement this over their level
/// collision; the validation harness uses a plane from the recorded contact.
pub trait Ground {
    /// Triangles overlapping the axis-aligned box `center +- half_extent` (the original uses +-1.2 ft and gives
    /// up above 400 triangles).
    fn triangles(&self, center: Vec3, half_extent: f64, out: &mut Vec<Triangle>);

    /// First hit of the segment a -> b: (point, surface normal). Used by [`Ground::segment_hits`]; the chassis
    /// collision points query `triangles` (a box around the chassis CM) and test their segments themselves.
    fn raycast(&self, a: Vec3, b: Vec3) -> Option<(Vec3, Vec3)>;

    /// Does the segment a -> b hit the terrain? Used by the jump-lip helper (a horizontal 25 ft probe ahead of
    /// the front wheel when the bike rises faster than 15 ft/s); "no hit" means the bike is leaving a lip.
    fn segment_hits(&self, a: Vec3, b: Vec3) -> bool {
        self.raycast(a, b).is_some()
    }
}

/// An infinite plane, served as two large triangles around the query point.
#[derive(Clone, Copy, Debug)]
pub struct PlaneGround {
    pub point: Vec3,
    pub normal: Vec3,
}

impl Ground for PlaneGround {
    fn triangles(&self, center: Vec3, half_extent: f64, out: &mut Vec<Triangle>) {
        let n = self.normal.normalize();
        let c = center - n * (center - self.point).dot(n);
        let (t, b) = n.any_orthonormal_pair();
        let s = half_extent * 4.0;
        let p = |u: f64, v: f64| c + t * (u * s) + b * (v * s);
        let (p00, p10, p11, p01) = (p(-1.0, -1.0), p(1.0, -1.0), p(1.0, 1.0), p(-1.0, 1.0));
        // wound so (b - a) x (c - a) points along n
        let tri = |a: Vec3, b2: Vec3, c2: Vec3| {
            if (b2 - a).cross(c2 - a).dot(n) >= 0.0 {
                Triangle { a, b: b2, c: c2, normal: n }
            } else {
                Triangle { a, b: c2, c: b2, normal: n }
            }
        };
        out.push(tri(p00, p10, p11));
        out.push(tri(p00, p11, p01));
    }

    fn raycast(&self, a: Vec3, b: Vec3) -> Option<(Vec3, Vec3)> {
        let n = self.normal.normalize();
        let (da, db) = ((a - self.point).dot(n), (b - self.point).dot(n));
        if da <= 0.0 {
            Some((a, n))
        } else if db <= 0.0 {
            Some((a + (b - a) * (da / (da - db)), n))
        } else {
            None
        }
    }
}

/// No ground at all.
pub struct NoGround;

impl Ground for NoGround {
    fn triangles(&self, _: Vec3, _: f64, _: &mut Vec<Triangle>) {}

    fn raycast(&self, _: Vec3, _: Vec3) -> Option<(Vec3, Vec3)> {
        None
    }
}

/// Controller outputs (the ctl block, slot 9) that later stages read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    /// Processed throttle 0..1 (ctl+0x00).
    pub throttle: f64,
    /// Front wheel steer angle about the fork axis, rad (ctl+0x10).
    pub steer_angle: f64,
    /// Lean input u (ctl+0x40), torque feedback k (ctl+0x60), commanded lean phi_cmd (ctl+0x64).
    pub lean_input: f64,
    pub torque_fb: f64,
    pub lean_cmd: f64,
    /// ctl+0x74: rider fore/aft term read by the rear tyre.
    pub ctl_74: f64,
    /// Left stick X copy (ctl+0x68), read by the lean fix-ups and the air step.
    pub stick_lx: f64,
    /// Reverse request (ctl+0x04): the engine step pushes the chassis backwards.
    pub reverse: f64,
    /// Brake torques about the rear / front axle, lbf ft (ctl+0x08 / 0x0c).
    pub brake_rear: f64,
    pub brake_front: f64,
    /// Air / ground drag force on the chassis, world, lbf (ctl+0x20).
    pub drag: Vec3,
    /// 0 engaged, 1 slipping, 2 clutch in (ctl+0x84).
    pub clutch_mode: i32,
}

/// Spawn hold (`sim+0x2de0` with mode 2): the bike is pinned to `pos` (x, z) and `q` once per frame, its
/// angular velocities are zeroed every substep, the chassis velocity is kept along its own up axis, and the
/// lean / air step is skipped. The game releases it on throttle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hold {
    pub pos: Vec3,
    pub q: Quat,
    /// Steer angle held while pinned (sim+0x2e70).
    pub steer: f64,
}

#[derive(Clone, Debug)]
pub struct Sim {
    pub vehicle: Vehicle,
    pub params: Params,
    pub geometry: Geometry,
    pub sys: System,
    /// Latched for the current frame.
    pub inputs: Inputs,
    pub ctl: Controls,
    /// Slot 9 filter states.
    pub cstate: ControlState,
    /// Run slot 9 (default). The validator turns it off to feed recorded controller outputs instead.
    pub own_controls: bool,
    /// Time already simulated past the last rendered frame, s.
    pub carry: f64,
    pub drive: Drivetrain,
    /// Water drag coefficient (sim+0x2f00) and extra forces on the rear / front wheel (sim+0x8a40 / 0x8a50).
    pub water: f64,
    pub wheel_extra: [Vec3; 2],
    /// Both wheels locked by the brakes this step (sim+0xa8).
    pub parked: bool,
    /// At rest (sim+0x90) and the stored ground normal (sim+0x8af0) the chassis may move along.
    pub at_rest: bool,
    pub rest_axis: Vec3,
    /// Front / rear contact points saved on coming to rest (sim+0x8b10 / 0x8b00), held by the resting pin.
    pub rest_points: [Vec3; 2],
    /// Rolling-orientation filters of the wheel positions (`00445800`).
    pub roll: contact::RollState,
    /// Front / rear wheel spin relative to the chassis, rad/s (slot 17).
    pub spin_rel: [f64; 2],
    /// Front, rear.
    pub tyres: [Tyre; 2],
    pub airborne: bool,
    pub crashed: bool,
    pub hold: Option<Hold>,
    /// Low-passed chassis speed, ft/s (sim+0x94).
    pub speed_filt: f64,
    /// Filtered up vector the lean is measured against (sim+0x8b50).
    pub up_filt: Vec3,
    /// Ground lean state (`00441300`).
    pub lean: LeanState,
    /// sim+0x2ec0 (an input / assist vector; only "non-zero" matters to the lean step).
    pub input_vec: Vec3,
    /// Air step state, the takeoff edge (first airborne substep, sim+0x8d9c) and the stick-rotation inhibit
    /// (sim+0x3c).
    pub air: AirState,
    pub takeoff: bool,
    pub air_inhibit: bool,
    /// Chassis collision contacts from the last substep (consumed by the next one's impulses).
    pub chassis: ChassisContacts,
    /// Crash flags ORed over the frame's substeps: any crash point touching (slot 4), a crash-point hit (slot 5).
    pub frame_touch: bool,
    pub frame_hit: bool,
    /// Per-frame crash decision; `crashed` is the wreck state.
    pub wreck: WreckChecker,
    /// Respawn in place, upright, `WreckDuration` after a wreck. Off: the host decides (see `respawn_due`).
    pub auto_respawn: bool,
    /// Brake-hold latch of the input block (S+54, block +0x38): set when the simulation is built (`00430b50`);
    /// both brakes read 1.0 while it is set, and the first throttle or reverse input clears it (`004308a0`).
    /// A respawn does not set it again (`0044a650` leaves it).
    pub brake_hold: bool,
    tri_buf: Vec<Triangle>,
}

impl Sim {
    /// A bike at rest at chassis position `x` (ft) and orientation `q`, suspension at rest length.
    pub fn new(vehicle: Vehicle, x: Vec3, q: Quat) -> Self {
        let (sys, geometry) = build_system(&vehicle, x, q);
        let params = Params::from_vehicle(&vehicle);
        let cstate = ControlState::new(&params);
        let mut air_state = AirState::default();
        air_state.roll.tau = params.air.rpy_tau[0];
        air_state.pitch.tau = params.air.rpy_tau[1];
        air_state.yaw.tau = params.air.rpy_tau[2];
        let mut sim = Sim {
            vehicle,
            params,
            geometry,
            sys,
            inputs: Inputs::default(),
            ctl: Controls::default(),
            cstate,
            own_controls: true,
            carry: 0.0,
            drive: Drivetrain::default(),
            water: 0.0,
            wheel_extra: [Vec3::ZERO; 2],
            parked: false,
            at_rest: false,
            rest_axis: Vec3::Y,
            rest_points: [Vec3::ZERO; 2],
            roll: contact::RollState::default(),
            spin_rel: [0.0; 2],
            tyres: [Tyre::new(), Tyre::new()],
            airborne: true,
            crashed: false,
            hold: None,
            speed_filt: 0.0,
            up_filt: Vec3::Y,
            lean: LeanState::default(),
            input_vec: Vec3::ZERO,
            air: air_state,
            takeoff: false,
            air_inhibit: false,
            chassis: ChassisContacts::default(),
            frame_touch: false,
            frame_hit: false,
            wreck: WreckChecker::default(),
            auto_respawn: true,
            brake_hold: true,
            tri_buf: vec![],
        };
        sim.reset(x, q);
        sim
    }

    /// Respawns the bike at rest at chassis position `x` (ft) and orientation `q`, suspension at rest length,
    /// keeping the vehicle and garage values. As in the original spawn (`0044a650`): engine at MinimumSpeed and
    /// the clutch timer full, so a spawn on the ground starts with a clutch bite.
    pub fn reset(&mut self, x: Vec3, q: Quat) {
        let (sys, geometry) = build_system(&self.vehicle, x, q);
        self.sys = sys;
        self.geometry = geometry;
        let p = &self.params;
        self.drive = Drivetrain {
            engine_w: p.engine_min_speed,
            clutch_timer: p.clutch_reset_time,
            ..Drivetrain::default()
        };
        self.cstate = ControlState::new(p);
        self.air = AirState::default();
        self.air.roll.tau = p.air.rpy_tau[0];
        self.air.pitch.tau = p.air.rpy_tau[1];
        self.air.yaw.tau = p.air.rpy_tau[2];
        self.inputs = Inputs::default();
        self.ctl = Controls::default();
        self.carry = 0.0;
        self.spin_rel = [0.0; 2];
        self.tyres = [Tyre::new(), Tyre::new()];
        self.airborne = true;
        self.takeoff = false;
        self.crashed = false;
        self.hold = None;
        self.parked = false;
        self.at_rest = false;
        self.roll = contact::RollState::default();
        self.speed_filt = 0.0;
        self.up_filt = Vec3::Y;
        self.lean = LeanState::default();
        self.chassis = ChassisContacts::default();
        self.frame_touch = false;
        self.frame_hit = false;
        self.wreck.reset();
    }

    /// Respawns where the bike is, upright and facing its horizontal heading, 1 ft above its current chassis
    /// position (the original resets the player at the bike's spot after a wreck).
    pub fn respawn_in_place(&mut self) {
        let b0 = &self.sys.bodies[CHASSIS];
        let mut fwd = b0.q.rotate(Vec3::Z);
        fwd.y = 0.0;
        if fwd.length() < 0.1 {
            fwd = b0.q.rotate(Vec3::Y);
            fwd.y = 0.0;
        }
        let yaw = if fwd.length() > 1e-6 { fwd.x.atan2(fwd.z) } else { 0.0 };
        let x = b0.x + Vec3::Y;
        self.reset(x, Quat::from_axis_angle(Vec3::Y, yaw));
    }

    /// Wrecked long enough for the automatic respawn (when `auto_respawn` is off, the host respawns).
    pub fn respawn_due(&self) -> bool {
        self.crashed && self.wreck.wreck_timer <= 0.0
    }

    /// A manual respawn is allowed (not wrecked, or wrecked for at least 0.5 s).
    pub fn manual_respawn_allowed(&self) -> bool {
        !self.crashed || self.wreck.wreck_timer <= WRECK_DURATION - MIN_WRECK_BEFORE_RESET
    }

    /// Once per frame after the substeps: the crash decision, the wreck timer and the automatic respawn.
    fn after_substeps(&mut self, frame_dt: f64) {
        if self.crashed {
            self.wreck.wreck_timer -= frame_dt;
            if self.auto_respawn && self.respawn_due() {
                self.respawn_in_place();
            }
            return;
        }
        if self.hold.is_some() {
            return;
        }
        let b0 = &self.sys.bodies[CHASSIS];
        let up = b0.q.rotate(Vec3::Y);
        let rate = b0.w + self.lean.removed_roll;
        let (f, r) = (&self.tyres[0], &self.tyres[1]);
        let w = WreckInputs {
            touch: self.frame_touch,
            hit: self.frame_hit,
            vy: b0.v.y,
            rate_pitch: rate.x,
            rate_yaw: rate.y,
            n_front: f.n_filt,
            n_rear: r.n_filt,
            e_front: WreckChecker::alignment(f.contact, f.normal, up),
            e_rear: WreckChecker::alignment(r.contact, r.normal, up),
            airborne: self.airborne,
            doughnut: r.doughnut,
        };
        if self.wreck.frame(&w, frame_dt) {
            self.crashed = true;
        }
    }

    /// Forward speed of the chassis (chassis-frame z), ft/s.
    pub fn forward_speed(&self) -> f64 {
        let b = &self.sys.bodies[CHASSIS];
        b.q.inv_rotate(b.v).z
    }

    /// Interpolated render pose of body `i` (spec 3): lerp / slerp between the previous and current substep.
    pub fn render_pose(&self, i: usize) -> (Vec3, Quat) {
        let a = self.render_alpha().clamp(0.0, 1.0);
        let b = &self.sys.bodies[i];
        (b.prev_x.lerp(b.x, a), b.prev_q.slerp(b.q, a))
    }

    /// Latches a frame's inputs as the controller read (`004308a0`) and the set-input slot (`00433380`) leave
    /// them in the input block: non-finite axes read 0, the reverse button is 0 or 1, and the brake-hold latch
    /// forces both brakes to 1 until a throttle or reverse input clears it (after this frame's brakes are set).
    /// The lean input restarts from the steer stick once per frame (`0044bfb0` / `0044bff0`; the substeps then
    /// edit it in place).
    pub fn latch(&mut self, inputs: Inputs) {
        let mut i = inputs;
        for a in [
            &mut i.throttle,
            &mut i.brake_button,
            &mut i.front_brake,
            &mut i.rear_brake,
            &mut i.steer_x,
            &mut i.steer_y,
            &mut i.rider_x,
            &mut i.rider_y,
        ] {
            if !a.is_finite() {
                *a = 0.0;
            }
        }
        i.brake_button = if i.brake_button == 0.0 { 0.0 } else { 1.0 };
        if self.brake_hold {
            i.front_brake = 1.0;
            i.rear_brake = 1.0;
        }
        if i.throttle > 0.0 || i.brake_button > 0.0 {
            self.brake_hold = false;
        }
        self.inputs = i;
        self.ctl.lean_input = i.steer_x;
        self.cstate.latch(&i);
    }

    /// Applies garage multipliers (1.0 = stock) to the values the sim runs on.
    pub fn set_garage(&mut self, g: Garage) {
        self.params = Params::with_garage(&self.vehicle, g);
        self.cstate.front_brake.tau = self.params.brake_time_constant;
        self.cstate.rear_brake.tau = self.params.brake_time_constant;
        self.cstate.steer.tau = self.params.steering_tau;
    }

    /// Loads a vehicle file (stock JSON / XML dump or tuning JSON) and places the bike at rest.
    pub fn from_file(path: impl AsRef<std::path::Path>, x: Vec3, q: Quat) -> Result<Self, crate::vehicle::LoadError> {
        Ok(Sim::new(Vehicle::load(path)?, x, q))
    }

    /// One rendered frame (`00430ef0`): latch inputs, run the substeps the frame time calls for. Returns the
    /// number of substeps run. Render with [`Sim::render_alpha`]. As in the scheduler, a frame that runs no
    /// substep only shrinks the carry: the hold pin (slot 34) and the clearing of the crash flags (S+14 / S+18)
    /// happen only on a frame that steps.
    pub fn frame(&mut self, frame_dt: f64, inputs: Inputs, ground: &dyn Ground) -> usize {
        self.latch(inputs);
        let n = self.substeps_for(frame_dt);
        if n > 0 {
            self.apply_hold_pin();
            self.frame_touch = false;
            self.frame_hit = false;
        }
        for _ in 0..n {
            self.substep(ground);
        }
        self.after_substeps(frame_dt);
        n
    }

    /// Advances the carry accumulator by one frame and returns its substep count (spec 3).
    pub fn substeps_for(&mut self, frame_dt: f64) -> usize {
        let deficit = frame_dt - self.carry;
        if deficit <= 0.0 {
            self.carry -= frame_dt;
            return 0;
        }
        let n = (deficit / DT).ceil() as usize;
        if n > MAX_SUBSTEPS {
            self.carry = 0.0;
            MAX_SUBSTEPS
        } else {
            self.carry = n as f64 * DT - deficit;
            n
        }
    }

    /// Interpolation factor for the render pose: lerp/slerp(prev, current, alpha).
    pub fn render_alpha(&self) -> f64 {
        1.0 - self.carry / DT
    }

    /// Hold pin (slot 34 `0044e830`, mode 2), once per frame before the substeps: shift every body so the
    /// chassis x / z return to the anchor (y free), then turn the mechanism by R = conj(q_chassis) q_held the way
    /// OpenReflex phys_mech.c `0085c530` does: every orientation q = q R (the chassis lands on q_held), every
    /// position = root + R (x - root) R*. The joint projections of the substep put rider and wheels back.
    pub fn apply_hold_pin(&mut self) {
        let Some(h) = self.hold else { return };
        let x0 = self.sys.bodies[CHASSIS].x;
        let shift = Vec3 { x: h.pos.x - x0.x, y: 0.0, z: h.pos.z - x0.z };
        for b in &mut self.sys.bodies {
            b.x += shift;
        }
        let r = self.sys.bodies[CHASSIS].q.conj() * h.q;
        let root = self.sys.bodies[CHASSIS].x;
        for b in &mut self.sys.bodies {
            b.q = b.q * r;
            b.x = root + r.rotate(b.x - root);
        }
    }

    /// One 7.5 ms substep (OpenReflex bike_sim.c `00431d60`), stages in the original order: (the scheduler's
    /// prev := cur), slot 9 controls, force reset, slot 11 forces, the joint solve and velocity integration,
    /// slot 12 (skipped while held), slot 15, pose integration, slot 16, node slot 6 (joint pose projection),
    /// slot 13 contacts, slot 17, the speed filter. Left out: the replay follow, the external-object impulse
    /// (`004355d0`), the debug push keys, the scheduler's slots 6 / 14 (no motocross code) and slot 7 (a pose
    /// save for a debug restore toggle).
    pub fn substep(&mut self, ground: &dyn Ground) {
        self.sys.save_prev_poses();
        self.controls(ground); // 1
        self.sys.clear_accumulators(); // 2
        // 3 moving platform follow (no-op)
        // 4 forces (slot 11 `0043e100`): gearbox, engine, brakes, suspension, drag, tyres rear then front
        if !self.crashed {
            self.drive.shift_timer += DT;
            if !self.airborne {
                drivetrain::gearbox(&mut self.drive, &self.sys, &self.params, self.ctl.throttle);
            }
            drivetrain::engine(&mut self.drive, &mut self.sys, &self.params, &self.ctl, DT);
            drivetrain::brakes(&mut self.sys, &self.ctl);
        }
        suspension::forces(&mut self.sys, &self.geometry, &self.params.suspension, &mut self.tyres);
        if !self.crashed {
            drivetrain::drag(&mut self.sys, &self.params, &self.tyres, &self.ctl, self.water, self.wheel_extra, false);
        }
        self.tyre_forces();
        self.sys.solve_and_integrate_velocities(DT); // 5-9
        // 10 (slot 12): skipped while held; ground lean constraint; air and crashed steps not implemented yet
        // OpenReflex bike_slot12.c 00443610: the removed roll rate (S+8b60) is cleared every substep, so the
        // wreck check never sees a stale one in the air.
        self.lean.removed_roll = Vec3::ZERO;
        if self.hold.is_none() && self.crashed {
            // crashed step (0043f270): chassis |w| <= 10 rad/s, nothing else
            let w = self.sys.bodies[CHASSIS].w;
            if w.length() > 10.0 {
                self.sys.bodies[CHASSIS].w = w * (10.0 / w.length());
            }
        }
        let sticks = Sticks { lx: self.inputs.steer_x, ly: self.inputs.steer_y, rx: self.inputs.rider_x, ry: self.inputs.rider_y };
        if self.hold.is_none() && !self.crashed && self.airborne {
            air::step(&mut self.sys, &mut self.air, &self.params.air, sticks, self.air_inhibit, self.takeoff, DT);
        }
        if self.hold.is_none() && !self.crashed && !self.airborne {
            let p = &self.params;
            let li = LeanInputs {
                tyres: &self.tyres,
                up_filt: self.up_filt,
                lean_cmd: self.ctl.lean_cmd,
                stick_lx: self.ctl.stick_lx,
                rider_y: self.inputs.rider_y,
                input_active: self.input_vec.length() > 0.0,
                clutch_engagement: ((p.clutch_reset_time - self.drive.clutch_timer) / p.clutch_reset_time).clamp(0.0, 1.0),
            };
            lean::ground_step(&mut self.sys, &mut self.lean, &li, &mut self.ctl.lean_cmd, &mut self.cstate.lean, DT);
            // after the grounded correction, which turns the bodies and trims the velocity (`00443610`)
            air::ground_update(&mut self.air, &self.sys, sticks);
        }
        self.velocity_fixups(); // 11
        self.sys.integrate_positions(DT); // 12
        suspension::geometry(&mut self.sys, &self.geometry, self.ctl.steer_angle); // 13
        self.sys.project_positions(); // 14
        self.contacts(ground); // 15
        self.update_spin_rel(); // 16
        let a = DT.min(self.params.speed_filter_tau) / self.params.speed_filter_tau; // 17
        self.speed_filt += (self.sys.bodies[CHASSIS].v.length() - self.speed_filt) * a;
    }

    /// Stage 1 (slot 9): controls (see [`controls`]), rider weld offset (spec 5.6), up-vector filter.
    fn controls(&mut self, ground: &dyn Ground) {
        if self.own_controls {
            let zero = Inputs::default();
            let ci = ControlInputs {
                // wreck branch (0044e0d0): inputs zeroed
                inputs: if self.crashed { &zero } else { &self.inputs },
                sys: &self.sys,
                tyres: &self.tyres,
                airborne: self.airborne,
                crashed: self.crashed,
                spin_rel: self.spin_rel,
                up_filt: self.up_filt,
                ground,
            };
            let mods = controls::step(&mut self.cstate, &mut self.ctl, &mut self.drive.clutch_timer, &self.params, &ci);
            self.lean.hold = self.cstate.lean_hold;
            for (t, (side, roll)) in self.tyres.iter_mut().zip(mods) {
                t.side_mod = side;
                t.roll_mod = roll;
            }
            if let Some(h) = self.hold {
                self.ctl.steer_angle = h.steer;
            }
        }
        // rider weld S+6060 = S+8d80 = Rider_RiderCenterOfMass + (rx LeanLeftRight, 0, ry LeanForwardBack), or
        // + (0, -1, 0) with the rider off; no CM offset (OpenReflex bike_controls.c 0044c170 / 0044e0d0)
        let p = &self.params;
        let r = if self.crashed {
            p.rider_com + Vec3 { x: 0.0, y: -1.0, z: 0.0 }
        } else {
            Vec3 {
                x: p.rider_com.x + p.rider_lean_lr * self.inputs.rider_x,
                y: p.rider_com.y,
                z: p.rider_com.z + p.rider_lean_fb * self.inputs.rider_y,
            }
        };
        if let Joint::Fixed { offset, .. } = &mut self.sys.joints[RIDER_JOINT] {
            *offset = r;
        }
        let steer = if self.crashed { 0.0 } else { self.inputs.steer_x };
        let q0 = self.sys.bodies[CHASSIS].q;
        lean::step_up_vector(&mut self.up_filt, &self.tyres, self.airborne, steer, q0, self.params.up_vector_tau, DT);
    }

    fn tyre_inputs(&self) -> TyreInputs {
        let b0 = &self.sys.bodies[CHASSIS];
        TyreInputs {
            throttle: self.ctl.throttle,
            ctl_74: self.ctl.ctl_74,
            rear_brake: self.inputs.rear_brake,
            steer_x: self.inputs.steer_x,
            rider_x: self.inputs.rider_x,
            rider_y: self.inputs.rider_y,
            v_fwd: b0.q.inv_rotate(b0.v).z,
            crashed: self.crashed,
        }
    }

    /// Tyre law, rear then front.
    fn tyre_forces(&mut self) {
        let inp = self.tyre_inputs();
        for (i, wi) in [(1, REAR), (0, FRONT)] {
            let (lo, hi) = self.sys.bodies.split_at_mut(wi);
            tyre_step(&mut self.tyres[i], &self.params.tyres[i], i == 1, &mut lo[CHASSIS], &mut hi[0], &inp, DT);
        }
    }

    /// Stage 11 (slot 15, OpenReflex bike_response.c `00443c30`): the chassis contact response (`004360b0`), the
    /// angular speed clamps (`00443af0`: chassis 20 rad/s, the other bodies 120 rad/s); held (pose override mode
    /// 2): not at rest, chassis velocity kept along its up axis and chassis / wheel spins zeroed, then the joint
    /// velocity projection (node slot 7); otherwise the joint velocity projection, rolling friction / brake lock
    /// (`00447810`) and the rest state. At rest the wheels spin with the chassis. (The planar lock of S+5c is not
    /// used in free ride and left out.)
    fn velocity_fixups(&mut self) {
        chassis::impulses(&mut self.sys, &self.chassis);
        for (i, b) in self.sys.bodies.iter_mut().enumerate() {
            let max = if i == CHASSIS { 20.0 /* 00935aac */ } else { 120.0 /* 00977480 */ };
            let w = b.w.length();
            if w > max {
                b.w = b.w * (max / w);
            }
        }
        if self.hold.is_some() {
            self.at_rest = false;
            let b0 = &mut self.sys.bodies[CHASSIS];
            let up = b0.q.rotate(Vec3::Y);
            b0.v = up * b0.v.dot(up);
            for i in [CHASSIS, FRONT, REAR] {
                self.sys.bodies[i].w = Vec3::ZERO;
            }
            self.sys.project_velocities();
        } else {
            self.sys.project_velocities();
            self.parked = drivetrain::rolling_friction(&mut self.sys, &self.params, self.inputs.front_brake, self.inputs.rear_brake);
            self.rest_check();
        }
        if self.at_rest {
            // wheels_follow_chassis: wheel spin = the chassis's world spin in the wheel frame
            let w0 = self.sys.bodies[CHASSIS].omega();
            for i in [FRONT, REAR] {
                let b = &mut self.sys.bodies[i];
                b.w = b.q.inv_rotate(w0);
            }
        }
    }

    /// The rider drives (OpenReflex bike_sim.c, the input chains of slots 25 / 26 / 28): throttle with the clutch
    /// out and no brake, or reverse and no brake.
    fn driving_inputs(&self) -> bool {
        let i = &self.inputs;
        let brakes_idle = !(i.front_brake > 0.0) && !(i.rear_brake > 0.0);
        (i.throttle > 0.0 && !i.clutch && brakes_idle) || (i.brake_button > 0.0 && brakes_idle)
    }

    /// Slot 30 (`00447750`): both wheels touch (slot 29 `00447730`) and both keep their rolling and lateral
    /// grip (the tyre step's W+c0 / W+c4 flags).
    fn wheels_gripping(&self) -> bool {
        self.tyres.iter().all(|t| t.contact && t.roll_grip && t.side_grip)
    }

    /// A large external impulse (|S+2ec0| > 3, `0092fe48`); only its first vector exists here.
    fn large_impulse(&self) -> bool {
        self.input_vec.length() > 3.0
    }

    /// Slot 26, leave rest (`00432360`): driving inputs, a wheel losing contact or grip, the rider off, or a
    /// large external impulse.
    fn leave_rest_test(&self) -> bool {
        self.driving_inputs() || !self.wheels_gripping() || self.crashed || self.large_impulse()
    }

    /// Slot 25, enter rest (`004324d0`): never while driving, without both wheels gripping or with the rider
    /// off. Then either stopped (|v| < 1 ft/s with a brake pressed and both wheels locked by it, sim+0xa8), or
    /// settled (both wheels touching, the speed filter below 0.5 ft/s) on flat ground: the normalized sum of
    /// the two contact normals has y > 0.9962 (slot 31 `00447790`, `00977770`).
    fn enter_rest_test(&self) -> bool {
        let speed = self.sys.bodies[CHASSIS].v.length();
        let both = self.tyres[0].contact && self.tyres[1].contact;
        if self.driving_inputs() || !self.wheels_gripping() || self.crashed {
            return false;
        }
        let i = &self.inputs;
        if speed < 1.0 && (i.front_brake > 0.0 || i.rear_brake > 0.0) && self.parked {
            return true;
        }
        if !(both && self.speed_filt < 0.5) {
            return false;
        }
        let n = self.tyres[0].normal + self.tyres[1].normal;
        n.length_squared() > 0.0 && n.normalize().y > 0.9962
    }

    /// Rest (sim+0x90, slot 15 with slots 25 / 26): entering it stops every body (`0085c4b0`) and stores the
    /// normalized sum of the wheel contact normals (sim+0x8af0); while at rest the chassis velocity keeps only
    /// its component along that normal (the suspension still works, sliding along the ground does not). Without
    /// it the one-step support lag lets a parked bike creep down slopes (spec 10). Entering it also saves the
    /// wheel contact points for the rest pin of slot 13 (`00446f80`, `contact::rest_pin`).
    fn rest_check(&mut self) {
        if self.at_rest {
            if self.leave_rest_test() {
                self.at_rest = false;
            }
        } else if self.enter_rest_test() {
            self.at_rest = true;
            self.rest_points = [self.tyres[0].point, self.tyres[1].point];
            for b in &mut self.sys.bodies {
                b.v = Vec3::ZERO;
                b.w = Vec3::ZERO;
            }
            let n = self.tyres[0].normal + self.tyres[1].normal;
            self.rest_axis = if n.length_squared() > 0.0 { n.normalize() } else { Vec3::Y };
        }
        if self.at_rest {
            let a = self.rest_axis;
            let b0 = &mut self.sys.bodies[CHASSIS];
            b0.v = a * a.dot(b0.v);
        }
    }

    /// Stage 15 (slot 13 `00444b90`): contact per wheel (front, then rear), airborne flag, then the position
    /// correction of spec 10.
    fn contacts(&mut self, ground: &dyn Ground) {
        let mut pens: [Option<Penetration>; 2] = [None, None];
        for (i, wi) in [(0, FRONT), (1, REAR)] {
            let b = &self.sys.bodies[wi];
            let radius = self.params.tyres[i].radius;
            let t = &mut self.tyres[i];
            match contact::query(ground, b.x, b.q, radius, &mut self.tri_buf) {
                Some((p, n)) => {
                    t.set_contact(b.x, b.q.rotate(Vec3::X), radius, p, n, self.sys.bodies[CHASSIS].q);
                    pens[i] = Some(Penetration { point: t.point, rim: t.rim, normal: t.normal, bottomed: t.bottomed });
                }
                None => {
                    t.contact = false;
                    t.roll_grip = false;
                    t.side_grip = false;
                }
            }
        }
        let was_airborne = self.airborne;
        self.airborne = !(self.tyres[0].contact || self.tyres[1].contact);
        self.takeoff = self.airborne && !was_airborne;
        self.chassis = chassis::detect(&self.sys, ground, !self.crashed && !self.lean.hold);
        let speed = self.sys.bodies[CHASSIS].v.length();
        self.frame_touch |= chassis::crash_touch(&self.chassis, self.input_vec.length() > 0.0, speed, self.airborne);
        self.frame_hit |= chassis::crash_hit(&self.chassis, self.cstate.slide_assist);
        let up = self.sys.bodies[CHASSIS].q.rotate(Vec3::Y);
        let (shift, slide) = contact::depenetrate(&pens, up, self.crashed);
        for (i, wi) in [(0, FRONT), (1, REAR)] {
            self.sys.bodies[wi].x += up * slide[i];
        }
        for b in &mut self.sys.bodies {
            b.x += shift;
        }
        chassis::depenetrate(&mut self.sys, &self.chassis);
        // 00445800: the wheels touching with lateral grip roll with the filtered ground speed (resting or held they
        // keep following the chassis); 00446f80: the resting pin, rider on.
        let pos = [self.sys.bodies[FRONT].x, self.sys.bodies[REAR].x];
        let vel = contact::roll_velocities(&mut self.roll, pos, DT);
        if !self.at_rest && self.hold.is_none() {
            for (i, wi) in [(0, FRONT), (1, REAR)] {
                let t = &self.tyres[i];
                if t.contact && t.side_grip {
                    let b = &mut self.sys.bodies[wi];
                    b.q = contact::roll_wheel(b.prev_q, b.w, vel[i], t.roll_dir, self.params.tyres[i].radius, DT);
                }
            }
        }
        // The wheels end the step back on their slider lines: the recordings fit "slide along chassis up, then
        // project onto the line" to 1e-5 ft (static, coast, half_throttle), so the front wheel moves along the
        // fork by a cos(rake). Measured, not traced in the code (no projection call found after 004456d0).
        self.sys.project_positions();
        if self.at_rest && !self.crashed {
            contact::rest_pin(&mut self.sys, [self.tyres[0].arm_chassis, self.tyres[1].arm_chassis], self.rest_points);
        }
    }

    /// Slot 17: wheel spin about its axle relative to the chassis.
    fn update_spin_rel(&mut self) {
        let chassis = self.sys.bodies[CHASSIS].omega();
        for (i, b) in [FRONT, REAR].into_iter().enumerate() {
            let w = &self.sys.bodies[b];
            let axle = w.q.rotate(Vec3::X);
            self.spin_rel[i] = (w.omega() - chassis).dot(axle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substeps_at_60fps() {
        let mut s = Sim::new(Vehicle::default(), Vec3::ZERO, Quat::IDENTITY);
        let pattern: Vec<usize> = (0..6000).map(|_| s.substeps_for(1.0 / 60.0)).collect();
        assert_eq!(pattern[0], 3);
        assert!(pattern.iter().all(|&n| n == 2 || n == 3));
        let mean = pattern.iter().sum::<usize>() as f64 / pattern.len() as f64;
        assert!((mean - 2.2222).abs() < 1e-3, "mean {mean}");
        assert!(s.carry >= 0.0 && s.carry < DT);
    }

    #[test]
    fn brake_hold_latch() {
        let mut s = Sim::new(Vehicle::default(), Vec3::ZERO, Quat::IDENTITY);
        s.latch(Inputs::default());
        assert_eq!((s.inputs.front_brake, s.inputs.rear_brake), (1.0, 1.0));
        // the throttle frame still has the brakes forced, then the latch is clear
        s.latch(Inputs { throttle: 0.5, ..Inputs::default() });
        assert_eq!(s.inputs.rear_brake, 1.0);
        s.latch(Inputs::default());
        assert_eq!(s.inputs.rear_brake, 0.0);
        // a respawn does not set it again
        s.reset(Vec3::ZERO, Quat::IDENTITY);
        s.latch(Inputs::default());
        assert_eq!(s.inputs.rear_brake, 0.0);
        s.latch(Inputs { brake_button: 0.3, steer_x: f64::NAN, ..Inputs::default() });
        assert_eq!((s.inputs.brake_button, s.inputs.steer_x), (1.0, 0.0));
    }

    #[test]
    fn hold_pin_lands_on_the_held_pose() {
        let mut s = Sim::new(Vehicle::default(), Vec3::ZERO, Quat::IDENTITY);
        let q = Quat::from_axis_angle(Vec3::Y, 0.4);
        s.hold = Some(Hold { pos: Vec3 { x: 3.0, y: 9.0, z: -2.0 }, q, steer: 0.0 });
        s.apply_hold_pin();
        let b0 = &s.sys.bodies[CHASSIS];
        assert!((b0.x - Vec3 { x: 3.0, y: 0.0, z: -2.0 }).length() < 1e-12);
        assert!(1.0 - b0.q.dot(q).abs() < 1e-12);
    }

    #[test]
    fn long_frame_is_capped() {
        let mut s = Sim::new(Vehicle::default(), Vec3::ZERO, Quat::IDENTITY);
        assert_eq!(s.substeps_for(0.1), MAX_SUBSTEPS);
        assert_eq!(s.carry, 0.0);
        assert_eq!(s.substeps_for(0.001), 1);
        assert_eq!(s.substeps_for(0.001), 0);
    }
}
