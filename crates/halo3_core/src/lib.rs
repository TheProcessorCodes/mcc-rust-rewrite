//! What the `halo3` crates share: the coordinate frames, the pause flag, the
//! flying camera, the zone-set state, the rider's pose, the session marker's
//! flag, the bike's engine state, the places that kill and the lifts that push. Kept small and
//! slow-changing: every other `halo3_*` crate depends on it, so an edit here
//! rebuilds them all.

pub mod bike;
pub mod coords;
pub mod falls;
pub mod flag;
pub mod lifts;
pub mod fly_camera;
pub mod pause;
pub mod skate;
pub mod weapons;
pub mod zones;

/// The default MCC Halo 3 maps folder; `%HALO3_MAPS%` overrides it.
pub const DEFAULT_MAPS: &str =
    r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps";
