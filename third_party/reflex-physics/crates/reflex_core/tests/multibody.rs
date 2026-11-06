//! Milestone 2 acceptance: bodies, joints, integrator. Synthetic bike numbers (round values, not Reflex data).

use reflex_core::bike::{CHASSIS, FRONT, REAR, RIDER};
use reflex_core::{v3, Body, Joint, Quat, System, Vec3, DT};

const G: Vec3 = v3(0.0, -32.2, 0.0);

/// A bike-shaped assembly: chassis, rider weld, raked front slider, vertical rear slider.
fn assembly(gravity: Vec3) -> System {
    let mut s = System::new(gravity);
    s.add_body(Body::new(7.0, v3(10.0, 14.0, 10.0)));
    s.add_body(Body::new(4.0, v3(2.0, 2.0, 2.0)));
    s.add_body(Body::new(0.5, v3(0.75, 0.25, 0.25)));
    s.add_body(Body::new(0.75, v3(1.0, 0.25, 0.25)));
    let rake: f64 = 0.45;
    let fork_axis = v3(0.0, rake.cos(), -rake.sin());
    let fork_top = v3(0.0, 2.0, 0.6);
    let rear_anchor = v3(0.0, 3.0, -2.5);
    s.joints.push(Joint::Fixed { parent: 0, child: 1, offset: v3(0.0, 1.5, -0.5), rel: Quat::IDENTITY });
    s.joints.push(Joint::SlidingRotor { parent: 0, child: 2, anchor: fork_top, axis: fork_axis, base: Quat::IDENTITY });
    s.joints.push(Joint::SlidingRotor { parent: 0, child: 3, anchor: rear_anchor, axis: Vec3::Y, base: Quat::IDENTITY });
    let q0 = Quat::from_axis_angle(v3(0.3, 1.0, -0.2), 0.7);
    let x0 = v3(10.0, 50.0, -3.0);
    let local = [Vec3::ZERO, v3(0.0, 1.5, -0.5), fork_top - fork_axis * 3.4, rear_anchor - Vec3::Y * 4.0];
    for (b, l) in s.bodies.iter_mut().zip(local) {
        b.x = x0 + q0.rotate(l);
        b.q = q0;
    }
    s
}

/// Tumbling, spinning, sliding: chassis w (body), wheel spins and slide speeds, made joint-consistent.
/// Harsh rates (6 rad/s chassis): use only with the ideal rider joint (see `rider_only`).
fn tumbling(gravity: Vec3) -> System {
    tumbling_at(gravity, v3(3.0, -5.0, 2.0))
}

fn tumbling_at(gravity: Vec3, chassis_w: Vec3) -> System {
    let mut s = assembly(gravity);
    s.bodies[CHASSIS].v = v3(5.0, 2.0, 30.0);
    s.bodies[CHASSIS].w = chassis_w;
    let q = s.bodies[CHASSIS].q;
    for (i, spin, slide) in [(FRONT, 60.0, 1.5), (REAR, -45.0, -2.0)] {
        let axis = match s.joints[i - 1] {
            Joint::SlidingRotor { axis, .. } => axis,
            _ => unreachable!(),
        };
        let b = s.bodies[CHASSIS].clone();
        let r = q.inv_rotate(s.bodies[i].x - b.x);
        s.bodies[i].v = b.point_velocity(r) + q.rotate(axis) * slide;
        s.bodies[i].w = b.w + v3(spin, 0.0, 0.0);
    }
    s.project_velocities();
    s
}

/// Riding-like rates: chassis about 1.3 rad/s, wheels spinning 45-60 rad/s.
fn riding(gravity: Vec3) -> System {
    tumbling_at(gravity, v3(0.6, -1.0, 0.5))
}

/// Chassis + rider only: the fixed joint is an ideal, exact constraint (the wheel joints are not; see
/// `wheel_joint_is_the_originals`).
fn rider_only(gravity: Vec3) -> System {
    let mut s = tumbling(gravity);
    s.bodies.truncate(2);
    s.joints.truncate(1);
    s
}

/// One substep with the chassis rate damped as the air step does (x0.985); the original's wheel joint needs it.
fn step_damped(s: &mut System) {
    s.save_prev_poses();
    s.solve_and_integrate_velocities(DT);
    s.bodies[CHASSIS].w = s.bodies[CHASSIS].w * 0.985;
    s.project_velocities();
    s.integrate_positions(DT);
    s.project_positions();
}

#[test]
fn free_fall_single_body() {
    let mut s = System::new(G);
    s.add_body(Body::new(2.0, v3(1.0, 2.0, 3.0)));
    let n = 400;
    for _ in 0..n {
        s.step(DT);
    }
    let b = &s.bodies[0];
    let nf = n as f64;
    assert!((b.v.y - (-32.2 * DT * nf)).abs() < 1e-9, "v {}", b.v.y);
    // semi-implicit Euler: x_n = g dt^2 n(n+1)/2
    assert!((b.x.y - (-32.2 * DT * DT * nf * (nf + 1.0) / 2.0)).abs() < 1e-9, "x {}", b.x.y);
}

#[test]
fn free_fall_assembly_keeps_shape() {
    let mut s = assembly(G);
    let rel0: Vec<Vec3> = s.bodies.iter().map(|b| b.x - s.bodies[0].x).collect();
    let n = 133;
    for _ in 0..n {
        s.step(DT);
    }
    for (i, b) in s.bodies.iter().enumerate() {
        assert!((b.v - G * (DT * n as f64)).length() < 1e-9, "body {i} v {:?}", b.v);
        assert!((b.x - s.bodies[0].x - rel0[i]).length() < 1e-9, "body {i} moved relative to chassis");
        assert!(b.w.length() < 1e-12);
    }
    for (f, t) in &s.joint_force {
        assert!(f.length() < 1e-9 && t.length() < 1e-9, "joint force in free fall: {f:?} {t:?}");
    }
}

#[test]
fn solve_is_exact_velocity_drift_is_second_order() {
    // An exact acceleration-level solve leaves only O(dt^2) velocity-constraint error after one step
    // (measured at the new positions; against the old positions any scheme shows O(dt)).
    let err = |dt: f64| {
        let mut s = rider_only(G);
        s.solve_and_integrate_velocities(dt);
        s.integrate_positions(dt);
        s.velocity_residual()
    };
    let (e1, e2) = (err(DT), err(DT / 2.0));
    eprintln!("velocity residual after one step: dt {e1:e}, dt/2 {e2:e}, ratio {:.2}", e1 / e2);
    assert!(e1 / e2 > 3.5, "residual is not second order: ratio {}", e1 / e2);
}

fn drift_one_step(dt: f64) -> f64 {
    let mut s = rider_only(G);
    s.solve_and_integrate_velocities(dt);
    s.integrate_positions(dt);
    s.position_residual()
}

#[test]
fn position_drift_is_second_order() {
    let (d1, d2) = (drift_one_step(DT), drift_one_step(DT / 2.0));
    eprintln!("position drift after one step: dt {d1:e}, dt/2 {d2:e}, ratio {:.2}", d1 / d2);
    assert!(d1 / d2 > 3.5, "drift is not second order: ratio {}", d1 / d2);
}

#[test]
fn projection_removes_drift() {
    let mut s = riding(Vec3::ZERO);
    let mut worst_before: f64 = 0.0;
    for _ in 0..2000 {
        s.save_prev_poses();
        s.solve_and_integrate_velocities(DT);
        s.bodies[CHASSIS].w = s.bodies[CHASSIS].w * 0.985;
        s.project_velocities();
        s.integrate_positions(DT);
        worst_before = worst_before.max(s.position_residual());
        s.project_positions();
        assert!(s.position_residual() < 1e-9, "position residual {}", s.position_residual());
    }
    s.project_velocities();
    assert!(s.velocity_residual() < 1e-12, "velocity residual {}", s.velocity_residual());
    eprintln!("worst position drift within one step (before projection): {worst_before:e}");
    assert!(worst_before < 1e-2);
}

/// Max |dE/E|, |dP/P|, |dL/L| over 10 s of zero-gravity tumbling at step `dt`.
fn conservation(dt: f64) -> (f64, f64, f64) {
    let mut s = rider_only(Vec3::ZERO);
    let (e0, p0, l0) = (s.kinetic_energy(), s.linear_momentum(), s.angular_momentum());
    let mut worst_e: f64 = 0.0;
    for _ in 0..(10.0 / dt).round() as usize {
        s.step(dt);
        worst_e = worst_e.max((s.kinetic_energy() / e0 - 1.0).abs());
    }
    let dp = (s.linear_momentum() - p0).length() / p0.length();
    let dl = (s.angular_momentum() - l0).length() / l0.length();
    (worst_e, dp, dl)
}

#[test]
fn energy_and_momentum_sanity() {
    // Chassis + rider tumbling at 6 rad/s. The scheme is first order (semi-implicit Euler, explicit gyroscopic
    // term, projections), so the drift must be small and shrink steadily with dt; a modelling error would not.
    let runs: Vec<_> = [DT, DT / 2.0, DT / 4.0].into_iter().map(conservation).collect();
    for (dt, (e, p, l)) in [1, 2, 4].iter().zip(&runs) {
        eprintln!("10 s tumbling, dt/{dt}: |dE/E| {e:.2e}, |dP/P| {p:.2e}, |dL/L| {l:.2e}");
    }
    let (e, p, l) = runs[0];
    assert!(e < 0.05 && p < 0.01 && l < 0.02, "drift too large at dt");
    for w in runs.windows(2) {
        let (a, b) = (w[0], w[1]);
        for (name, x, y) in [("energy", a.0, b.0), ("momentum", a.1, b.1), ("angular momentum", a.2, b.2)] {
            assert!(x / y > 1.2, "{name} drift does not shrink with dt: {x:e} -> {y:e}");
        }
    }
}

#[test]
fn axle_spin_and_slide_are_free() {
    // Torque about the wheel axle spins only the wheel.
    let mut s = assembly(Vec3::ZERO);
    s.bodies[FRONT].torque = v3(10.0, 0.0, 0.0);
    s.solve_and_integrate_velocities(DT);
    assert!((s.bodies[FRONT].w.x - 10.0 / 0.75 * DT).abs() < 1e-12);
    assert!(s.bodies[CHASSIS].w.length() < 1e-12 && s.bodies[CHASSIS].v.length() < 1e-12);

    // Torque about the wheel's Y is shared with the whole bike.
    let mut s = assembly(Vec3::ZERO);
    s.bodies[FRONT].torque = v3(0.0, 10.0, 0.0);
    s.solve_and_integrate_velocities(DT);
    assert!(s.bodies[CHASSIS].w.length() > 1e-4);
    // the original's angular rows constrain the sum of the rates (same sign on both bodies), so the wheel's
    // y / z rate is only restored by the velocity projection
    s.project_velocities();
    assert!(s.velocity_residual() < 1e-12, "{}", s.velocity_residual());

    // A force along the slider moves only the wheel; across it, the bike.
    let mut s = assembly(Vec3::ZERO);
    let q = s.bodies[CHASSIS].q;
    let u = q.rotate(v3(0.0, 0.45f64.cos(), -0.45f64.sin()));
    s.bodies[FRONT].force = u * 5.0;
    s.solve_and_integrate_velocities(DT);
    assert!((s.bodies[FRONT].v - u * (5.0 / 0.5 * DT)).length() < 1e-12);
    assert!(s.bodies[CHASSIS].v.length() < 1e-12 && s.bodies[RIDER].v.length() < 1e-12);
}

#[test]
fn rider_follows_weld() {
    let mut s = riding(G);
    for _ in 0..500 {
        step_damped(&mut s);
    }
    let (c, r) = (&s.bodies[CHASSIS], &s.bodies[RIDER]);
    assert!((r.x - c.x - c.q.rotate(v3(0.0, 1.5, -0.5))).length() < 1e-9);
    assert!((r.omega() - c.omega()).length() < 1e-12);
}




/// The wheel joint as the original builds it (`00859f90`): the angular rows put the same sign on wheel and
/// chassis and use the base frame, so it is not energy conserving on its own. In the game the chassis rate is
/// always damped (air step: x0.985 per step; ground: the lean constraint), and that keeps it bounded; a bare
/// system without damping runs away. Here the air damping is applied as the air step does, for 10 s.
#[test]
fn wheel_joint_is_the_originals() {
    let mut s = riding(Vec3::ZERO);
    let mut max_w: f64 = 0.0;
    for _ in 0..1333 {
        step_damped(&mut s);
        max_w = max_w.max(s.bodies[CHASSIS].w.length());
    }
    eprintln!("10 s at riding rates: max chassis |w| {max_w:.3} rad/s");
    assert!(s.bodies.iter().all(|b| b.x.length().is_finite() && b.w.length().is_finite()));
    assert!(max_w < 20.0, "chassis rate ran away: {max_w}");
    assert!(s.velocity_residual() < 1e-9 && s.position_residual() < 1e-9);
}
