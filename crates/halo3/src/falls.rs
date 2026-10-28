//! Falling out of the map. Guardian floats over a misty abyss: the scenario
//! puts a kill volume (`kill_drop`) a few world units under the walkways, and
//! the game kills a player who drops into it. The skater and the dirt bike
//! are put back where they last stood instead (`swim.rs` for the skater,
//! `bike/mod.rs` for the bike).
//!
//! A point is deadly inside any of the scenario's kill volumes
//! (`TriggerVolume::kills`) or further than [`BELOW_WORLD_WU`] under the
//! lowest collision triangle: a catch-all for maps whose drops have no kill
//! volume, so nothing falls for ever.

use bevy::prelude::*;

use blam_geometry::{Collision, TriggerVolume};

/// How far under the lowest collision surface counts as out of the world.
pub const BELOW_WORLD_WU: f32 = 20.0;

/// Where the player dies, Halo world units.
#[derive(Resource, Clone, Default)]
pub struct KillZones {
    pub volumes: Vec<TriggerVolume>,
    /// Below this height everything is out of the world.
    pub floor: f32,
}

impl KillZones {
    pub fn new(volumes: &[TriggerVolume], collision: &Collision) -> KillZones {
        let lowest = collision
            .triangles
            .iter()
            .flat_map(|t| t.points.map(|p| p.z))
            .fold(f32::INFINITY, f32::min);
        KillZones {
            volumes: volumes.iter().filter(|v| v.kills()).cloned().collect(),
            floor: if lowest.is_finite() { lowest - BELOW_WORLD_WU } else { f32::NEG_INFINITY },
        }
    }

    /// The kill volume `p` (Halo units) is in, or "below the world".
    pub fn deadly(&self, p: Vec3) -> Option<&str> {
        if p.z < self.floor {
            return Some("below the world");
        }
        self.volumes.iter().find(|v| v.contains(p)).map(|v| v.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_volume_and_floor() {
        let drop = TriggerVolume {
            name: "kill_drop".into(),
            forward: Vec3::X,
            up: Vec3::Z,
            position: Vec3::new(-50.0, -50.0, -1.0),
            extents: Vec3::new(100.0, 100.0, 14.0),
        };
        let other = TriggerVolume {
            name: "safe_zone".into(),
            ..drop.clone()
        };
        let zones = KillZones {
            volumes: [drop, other].into_iter().filter(TriggerVolume::kills).collect(),
            floor: -60.0,
        };
        assert_eq!(zones.deadly(Vec3::new(0.0, 0.0, 5.0)), Some("kill_drop"));
        assert_eq!(zones.deadly(Vec3::new(0.0, 0.0, 16.0)), None);
        assert_eq!(zones.deadly(Vec3::new(0.0, 0.0, -70.0)), Some("below the world"));
    }
}
