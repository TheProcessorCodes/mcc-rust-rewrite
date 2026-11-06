//! Engine-independent reimplementation of the MX vs ATV Reflex bike simulation.
//!
//! Everything runs in Reflex units and frames (spec section 1): ft, slug, lbf, s, rad; +X right,
//! +Y up, +Z forward (left-handed). Engine adapters convert only at the boundary.
//!
//! No Reflex code or data ships with this crate: vehicle values are loaded from the user's own
//! files (see [`vehicle`]).

pub mod air;
pub mod bike;
pub mod chassis;
pub mod contact;
pub mod controls;
pub mod curve;
pub mod drivetrain;
pub mod ground;
pub mod lean;
pub mod math;
pub mod multibody;
pub mod params;
pub mod sim;
pub mod suspension;
pub mod tyre;
pub mod vehicle;
pub mod wreck;

pub use curve::{blend3, Curve};
pub use math::{v3, Quat, Vec3};
pub use multibody::{Body, Joint, System};
pub use params::{Garage, Params};
pub use sim::{Controls, Ground, Hold, Inputs, NoGround, PlaneGround, Sim, Triangle};
pub use tyre::Tyre;
pub use vehicle::{Value, Vehicle, SENTINEL};

/// Fixed substep, s (133.3 Hz).
pub const DT: f64 = 0.0075;

/// rpm per rad/s as the original uses it (9.55, not 30/pi).
pub const RPM_PER_RAD_S: f64 = 9.55;

/// Feet to metres, for engine adapters.
pub const FT_TO_M: f64 = 0.3048;

/// First-order filter coefficient used everywhere: `y += (target - y) * filter_alpha(dt, tau)`.
pub fn filter_alpha(dt: f64, tau: f64) -> f64 {
    dt.min(tau) / tau
}
