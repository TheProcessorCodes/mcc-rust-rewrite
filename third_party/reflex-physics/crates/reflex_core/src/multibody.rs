//! Rigid bodies, joints and the integrator (spec 7; `model\loop-integrator.md` 3-5).
//!
//! - Body state: position x (world), velocity v (world), orientation q (body to world), angular velocity w
//!   (**body frame**), diagonal body-frame inertia. External forces are world-frame, torques body-frame.
//! - Joints are equality constraints solved exactly at the acceleration level (dense KKT; any exact solver
//!   gives the original's body-tree result). No Baumgarte terms; drift is removed by projection.
//! - Semi-implicit Euler: velocities first, then (after the caller's kinematic edits and fix-ups) positions.
//!
//! The substep calls the stages separately so controllers can run between them (spec 4):
//! [`System::clear_accumulators`], [`System::solve_and_integrate_velocities`] (stages 5-9),
//! [`System::project_velocities`] (11), [`System::integrate_positions`] (12), [`System::project_positions`] (14).

use crate::math::{Quat, Vec3};

/// Sign of the slider-joint bias as the solver uses it (+1: J a + b = 0). See `rows`.
pub const SLIDER_SIGN: f64 = 1.0;

#[derive(Clone, Debug)]
pub struct Body {
    pub x: Vec3,
    pub v: Vec3,
    pub q: Quat,
    /// Angular velocity in the body frame.
    pub w: Vec3,
    pub mass: f64,
    /// Diagonal inertia in the body frame.
    pub inertia: Vec3,
    /// External force, world frame (`+0x70`).
    pub force: Vec3,
    /// External torque, body frame (`+0x6b0`).
    pub torque: Vec3,
    /// Impulse accumulators, applied as impulse / dt (`+0x80`, `+0x6c0`).
    pub impulse: Vec3,
    pub ang_impulse: Vec3,
    /// Pose before the current substep, for render interpolation.
    pub prev_x: Vec3,
    pub prev_q: Quat,
}

impl Body {
    pub fn new(mass: f64, inertia: Vec3) -> Self {
        Body {
            x: Vec3::ZERO,
            v: Vec3::ZERO,
            q: Quat::IDENTITY,
            w: Vec3::ZERO,
            mass,
            inertia,
            force: Vec3::ZERO,
            torque: Vec3::ZERO,
            impulse: Vec3::ZERO,
            ang_impulse: Vec3::ZERO,
            prev_x: Vec3::ZERO,
            prev_q: Quat::IDENTITY,
        }
    }

    /// `0085c770`: world force `f` applied at body-frame offset `r` from the body origin (CM).
    pub fn apply_force(&mut self, f: Vec3, r: Vec3) {
        self.force += f;
        self.torque += r.cross(self.q.inv_rotate(f));
    }

    /// `0085c8b0`: body-frame force through the CM.
    pub fn apply_force_body(&mut self, f: Vec3) {
        self.force += self.q.rotate(f);
    }

    /// World angular velocity.
    pub fn omega(&self) -> Vec3 {
        self.q.rotate(self.w)
    }

    /// World velocity of the body-frame point `r`.
    pub fn point_velocity(&self, r: Vec3) -> Vec3 {
        self.v + self.q.rotate(self.w.cross(r))
    }

    pub fn kinetic_energy(&self) -> f64 {
        0.5 * self.mass * self.v.length_squared() + 0.5 * self.w.dot(self.inertia.mul_elem(self.w))
    }
}

#[derive(Clone, Debug)]
pub enum Joint {
    /// `ConstraintFixedBody` (rider): child pose = parent pose x (offset, rel). 6 rows.
    Fixed { parent: usize, child: usize, offset: Vec3, rel: Quat },
    /// `ConstraintSlidingRotor` (wheels): the child may slide along `axis` through `anchor` and spin about
    /// its own X (the axle), whose rest frame is `base`. All in the parent frame. 4 rows.
    SlidingRotor { parent: usize, child: usize, anchor: Vec3, axis: Vec3, base: Quat },
}

/// One constraint row: J a = bias, a = (linear acc world, angular acc body) per body.
struct Row {
    terms: [(usize, Vec3, Vec3); 2],
    bias: f64,
}

#[derive(Clone, Debug)]
pub struct System {
    pub bodies: Vec<Body>,
    pub joints: Vec<Joint>,
    pub gravity: Vec3,
    /// Joint forces / torques from the last solve (world force, body torque), per body.
    pub joint_force: Vec<(Vec3, Vec3)>,
}

impl System {
    pub fn new(gravity: Vec3) -> Self {
        System { bodies: vec![], joints: vec![], gravity, joint_force: vec![] }
    }

    pub fn add_body(&mut self, b: Body) -> usize {
        self.bodies.push(b);
        self.bodies.len() - 1
    }

    /// Stage 2 (`0085c9d0`).
    pub fn clear_accumulators(&mut self) {
        for b in &mut self.bodies {
            b.force = Vec3::ZERO;
            b.torque = Vec3::ZERO;
            b.impulse = Vec3::ZERO;
            b.ang_impulse = Vec3::ZERO;
        }
    }

    pub fn save_prev_poses(&mut self) {
        for b in &mut self.bodies {
            b.prev_x = b.x;
            b.prev_q = b.q;
        }
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = vec![];
        for j in &self.joints {
            match *j {
                Joint::Fixed { parent, child, offset: r, .. } => {
                    let (p, c) = (&self.bodies[parent], &self.bodies[child]);
                    // a_c - a_p - R_p(dw_p x r) = R_p(w_p x (w_p x r))
                    let centripetal = p.q.rotate(p.w.cross(p.w.cross(r)));
                    for k in 0..3 {
                        let e = axis(k);
                        let ep = p.q.inv_rotate(e);
                        rows.push(Row {
                            terms: [(child, e, Vec3::ZERO), (parent, -e, ep.cross(r))],
                            bias: centripetal[k],
                        });
                    }
                    // R_c dw_c - R_p dw_p = 0
                    for k in 0..3 {
                        let e = axis(k);
                        rows.push(Row {
                            terms: [
                                (child, Vec3::ZERO, c.q.inv_rotate(e)),
                                (parent, Vec3::ZERO, -p.q.inv_rotate(e)),
                            ],
                            bias: 0.0,
                        });
                    }
                }
                Joint::SlidingRotor { parent, child, axis: u, base, .. } => {
                    // Rows as the original builds them (`00859f90`, local notes sliding_rotor.md), written as
                    // J a + b = 0, i.e. J a = -b:
                    //   0, 1: n1 = X, n2 = X x axis (parent frame, without steer); parent lin +a, parent ang r x n,
                    //         child lin -a; b = a.(Wp x (Wp x d - 2 (vc - vp)))
                    //   2, 3: e = base-frame Z, Y in world (Q = q_p base); parent ang -Rp^T e, child ang -Rc^T e
                    //         (same sign on both bodies); b = (Wp x e).(Wc - Wp)
                    let (p, c) = (&self.bodies[parent], &self.bodies[child]);
                    let d = c.x - p.x;
                    let r = p.q.inv_rotate(d);
                    let (wp, wc) = (p.omega(), c.omega());
                    let dv = c.v - p.v;
                    let acc = wp.cross(wp.cross(d) - dv * 2.0);
                    for n in [Vec3::X, Vec3::X.cross(u)] {
                        let a = p.q.rotate(n);
                        rows.push(Row {
                            terms: [(parent, a, r.cross(n)), (child, -a, Vec3::ZERO)],
                            bias: -SLIDER_SIGN * a.dot(acc),
                        });
                    }
                    let qb = p.q * base;
                    for e_local in [Vec3::Z, Vec3::Y] {
                        let e = qb.rotate(e_local);
                        rows.push(Row {
                            terms: [(parent, Vec3::ZERO, -base.rotate(e_local)), (child, Vec3::ZERO, -c.q.inv_rotate(e))],
                            bias: -SLIDER_SIGN * wp.cross(e).dot(wc - wp),
                        });
                    }
                }
            }
        }
        rows
    }

    /// Stages 5-9: gravity + gyroscopic + external + impulse/dt, exact joint solve, then
    /// `v += F/m dt`, `w += (T ./ I) dt`.
    pub fn solve_and_integrate_velocities(&mut self, dt: f64) {
        let n = self.bodies.len();
        let mut f: Vec<(Vec3, Vec3)> = self
            .bodies
            .iter()
            .map(|b| {
                let lin = b.mass * self.gravity + b.force + b.impulse / dt;
                let ang = b.torque - b.w.cross(b.inertia.mul_elem(b.w)) + b.ang_impulse / dt;
                (lin, ang)
            })
            .collect();

        let rows = self.rows();
        let m = rows.len();
        let mut joint_force = vec![(Vec3::ZERO, Vec3::ZERO); n];
        if m > 0 {
            let inv = |bi: usize, lin: Vec3, ang: Vec3| {
                let b = &self.bodies[bi];
                (lin / b.mass, ang.div_elem(b.inertia))
            };
            // A = J M^-1 J^T, rhs = bias - J M^-1 F
            let mut a = vec![0.0; m * m];
            let mut rhs = vec![0.0; m];
            for (i, ri) in rows.iter().enumerate() {
                let mut acc = 0.0;
                for &(bi, lin, ang) in &ri.terms {
                    let (il, ia) = inv(bi, f[bi].0, f[bi].1);
                    acc += lin.dot(il) + ang.dot(ia);
                }
                rhs[i] = ri.bias - acc;
                for (k, rk) in rows.iter().enumerate().skip(i) {
                    let mut s = 0.0;
                    for &(bi, lin, ang) in &ri.terms {
                        for &(bk, lk, ak) in &rk.terms {
                            if bi == bk {
                                let (il, ia) = inv(bk, lk, ak);
                                s += lin.dot(il) + ang.dot(ia);
                            }
                        }
                    }
                    a[i * m + k] = s;
                    a[k * m + i] = s;
                }
            }
            let lambda = solve_dense(&mut a, &mut rhs, m);
            for (ri, l) in rows.iter().zip(&lambda) {
                for &(bi, lin, ang) in &ri.terms {
                    joint_force[bi].0 += lin * *l;
                    joint_force[bi].1 += ang * *l;
                }
            }
            for (fb, jf) in f.iter_mut().zip(&joint_force) {
                fb.0 += jf.0;
                fb.1 += jf.1;
            }
        }
        for (b, (lin, ang)) in self.bodies.iter_mut().zip(&f) {
            b.v += *lin / b.mass * dt;
            b.w += ang.div_elem(b.inertia) * dt;
        }
        self.joint_force = joint_force;
    }

    /// Stage 11 part (`0085d330`): joint velocity projection. Rider: child w = parent w, child v = weld point
    /// velocity. Wheels: v keeps only the slide; w keeps its x (spin) and takes y, z from the parent's angular
    /// velocity in the joint's base frame (measured: matches the recordings, the wheel's spin angle is ignored).
    pub fn project_velocities(&mut self) {
        for j in self.joints.clone() {
            match j {
                Joint::Fixed { parent, child, offset, rel } => {
                    let p = self.bodies[parent].clone();
                    let c = &mut self.bodies[child];
                    c.w = rel.inv_rotate(p.w);
                    c.v = p.point_velocity(offset);
                }
                Joint::SlidingRotor { parent, child, axis, base, .. } => {
                    let p = self.bodies[parent].clone();
                    let c = &mut self.bodies[child];
                    let u = p.q.rotate(axis);
                    let pc = p.q.inv_rotate(c.x - p.x);
                    let vp = p.point_velocity(pc);
                    c.v = vp + u * (c.v - vp).dot(u);
                    // `0085ac20`: w_c = Q^-1 (Wp + ((Wc - Wp).ex) ex), Q = q_p base, written into the wheel's
                    // body-frame slots through Q (the wheel's own spin angle is ignored)
                    let qb = p.q * base;
                    let ex = qb.rotate(Vec3::X);
                    let (wp, wc) = (p.omega(), c.omega());
                    c.w = qb.inv_rotate(wp + ex * (wc - wp).dot(ex));
                }
            }
        }
    }

    /// Stage 12 (`0085d100`): `x += v dt`; `q += 0.5 dt q (x) (w, 0)`; normalise.
    pub fn integrate_positions(&mut self, dt: f64) {
        for b in &mut self.bodies {
            b.x += b.v * dt;
            let dq = b.q * Quat::new(b.w.x, b.w.y, b.w.z, 0.0);
            let h = 0.5 * dt;
            b.q = Quat::new(b.q.x + h * dq.x, b.q.y + h * dq.y, b.q.z + h * dq.z, b.q.w + h * dq.w).normalize();
        }
    }

    /// Stage 14 (`0085d300`): rider snapped to the weld (`0085b750`); wheels onto their slider line, keeping only
    /// the twist about the axle of their relative rotation (`0085a8f0`).
    pub fn project_positions(&mut self) {
        for j in self.joints.clone() {
            match j {
                Joint::Fixed { parent, child, offset, rel } => {
                    let p = self.bodies[parent].clone();
                    let c = &mut self.bodies[child];
                    c.q = p.q * rel;
                    c.x = p.x + p.q.rotate(offset);
                }
                Joint::SlidingRotor { parent, child, anchor, axis, base } => {
                    let p = self.bodies[parent].clone();
                    let c = &mut self.bodies[child];
                    let a = p.x + p.q.rotate(anchor);
                    let u = p.q.rotate(axis);
                    c.x = a + u * (c.x - a).dot(u);
                    let qb = p.q * base;
                    let r = qb.conj() * c.q;
                    let twist = if r.x == 0.0 && r.w == 0.0 {
                        Quat::IDENTITY
                    } else {
                        Quat::new(r.x, 0.0, 0.0, r.w).normalize()
                    };
                    c.q = qb * twist;
                }
            }
        }
    }

    /// Plain substep for tests and free bodies: the stages in order with no controllers.
    pub fn step(&mut self, dt: f64) {
        self.save_prev_poses();
        self.solve_and_integrate_velocities(dt);
        self.project_velocities();
        self.integrate_positions(dt);
        self.project_positions();
    }

    /// Largest joint position error (ft, and rad-ish quaternion error) over all joints.
    pub fn position_residual(&self) -> f64 {
        let mut worst: f64 = 0.0;
        for j in &self.joints {
            match *j {
                Joint::Fixed { parent, child, offset, rel } => {
                    let (p, c) = (&self.bodies[parent], &self.bodies[child]);
                    worst = worst.max((c.x - p.x - p.q.rotate(offset)).length());
                    worst = worst.max(1.0 - (p.q * rel).dot(c.q).abs());
                }
                Joint::SlidingRotor { parent, child, anchor, axis, base } => {
                    let (p, c) = (&self.bodies[parent], &self.bodies[child]);
                    let d = p.q.inv_rotate(c.x - p.x) - anchor;
                    worst = worst.max((d - axis * d.dot(axis)).length());
                    let r = (p.q * base).conj() * c.q;
                    worst = worst.max(r.y.abs().max(r.z.abs()));
                }
            }
        }
        worst
    }

    /// Largest joint velocity error (ft/s and rad/s).
    pub fn velocity_residual(&self) -> f64 {
        let mut worst: f64 = 0.0;
        for j in &self.joints {
            match *j {
                Joint::Fixed { parent, child, offset, .. } => {
                    let (p, c) = (&self.bodies[parent], &self.bodies[child]);
                    worst = worst.max((c.v - p.point_velocity(offset)).length());
                    worst = worst.max((c.omega() - p.omega()).length());
                }
                Joint::SlidingRotor { parent, child, axis, base, .. } => {
                    let (p, c) = (&self.bodies[parent], &self.bodies[child]);
                    let u = p.q.rotate(axis);
                    let dv = c.v - p.point_velocity(p.q.inv_rotate(c.x - p.x));
                    worst = worst.max((dv - u * dv.dot(u)).length());
                    let wb = base.inv_rotate(p.w);
                    worst = worst.max((c.w.y - wb.y).abs().max((c.w.z - wb.z).abs()));
                }
            }
        }
        worst
    }

    pub fn kinetic_energy(&self) -> f64 {
        self.bodies.iter().map(Body::kinetic_energy).sum()
    }

    pub fn linear_momentum(&self) -> Vec3 {
        self.bodies.iter().fold(Vec3::ZERO, |s, b| s + b.v * b.mass)
    }

    pub fn total_mass(&self) -> f64 {
        self.bodies.iter().map(|b| b.mass).sum()
    }

    pub fn center_of_mass(&self) -> Vec3 {
        self.bodies.iter().fold(Vec3::ZERO, |s, b| s + b.x * b.mass) / self.total_mass()
    }

    /// Angular momentum about the world origin.
    pub fn angular_momentum(&self) -> Vec3 {
        self.bodies.iter().fold(Vec3::ZERO, |s, b| {
            s + b.x.cross(b.v * b.mass) + b.q.rotate(b.inertia.mul_elem(b.w))
        })
    }
}

fn axis(k: usize) -> Vec3 {
    match k {
        0 => Vec3::X,
        1 => Vec3::Y,
        _ => Vec3::Z,
    }
}

/// Gaussian elimination with partial pivoting; `a` is m x m row-major. Singular pivots give 0.
fn solve_dense(a: &mut [f64], b: &mut [f64], m: usize) -> Vec<f64> {
    for col in 0..m {
        let piv = (col..m).max_by(|&i, &k| a[i * m + col].abs().total_cmp(&a[k * m + col].abs())).unwrap();
        if piv != col {
            for k in 0..m {
                a.swap(col * m + k, piv * m + k);
            }
            b.swap(col, piv);
        }
        let d = a[col * m + col];
        if d.abs() < 1e-300 {
            continue;
        }
        for i in col + 1..m {
            let f = a[i * m + col] / d;
            if f != 0.0 {
                for k in col..m {
                    a[i * m + k] -= f * a[col * m + k];
                }
                b[i] -= f * b[col];
            }
        }
    }
    let mut x = vec![0.0; m];
    for i in (0..m).rev() {
        let d = a[i * m + i];
        if d.abs() < 1e-300 {
            continue;
        }
        let s: f64 = (i + 1..m).map(|k| a[i * m + k] * x[k]).sum();
        x[i] = (b[i] - s) / d;
    }
    x
}
