//! What the `mcc-rust-rewrite` crates share: the coordinate frames, the pause flag, the
//! flying camera, the zone-set state, the rider's pose, the session marker's
//! flag, the bike's engine state, the places that kill and the lifts that push. Kept small and
//! slow-changing: every other `halo3_*` crate depends on it, so an edit here
//! rebuilds them all.

pub mod ai;
pub mod bike;
pub mod coords;
pub mod effects;
pub mod falls;
pub mod flag;
pub mod lifts;
pub mod map;
pub mod fly_camera;
pub mod objects;
pub mod pause;
pub mod script_sound;
pub mod skate;
pub mod units;
pub mod weapons;
pub mod zones;
