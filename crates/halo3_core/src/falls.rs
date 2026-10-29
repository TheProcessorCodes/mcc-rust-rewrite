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
//!
//! Sandtrap has no kill volumes at all: its Guardians (defenders on six
//! obelisks round the desert) shoot anyone who leaves the play space
//! (`blam_geometry::scene::guarded`). Riding out over the outer wall or the
//! dunes counts as a fall too.

use bevy::prelude::*;

use blam_geometry::{Collision, TriggerVolume};
use blam_geometry::scene::guarded;

/// Kill volumes a map's scripts switched off (`kill_volume_disable`, by
/// name): 120_halo turns the Warthog run's off when its zone set loads and
/// back on behind the player; its start flag lies inside `kill_100_01`.
static SCRIPT_OFF: std::sync::LazyLock<std::sync::RwLock<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(Default::default);

/// Switches a kill volume on or off, as a script does.
pub fn set_kill_volume_enabled(name: &str, enabled: bool) {
    let mut off = SCRIPT_OFF.write().unwrap();
    if enabled {
        off.remove(name);
    } else {
        off.insert(name.to_string());
    }
}

/// Whether a script switched this kill volume off.
pub fn kill_volume_disabled(name: &str) -> bool {
    SCRIPT_OFF.read().unwrap().contains(name)
}

/// How far under the lowest collision surface counts as out of the world.
pub const BELOW_WORLD_WU: f32 = 20.0;

/// Where the player dies, Halo world units.
#[derive(Resource, Clone, Default)]
pub struct KillZones {
    pub volumes: Vec<TriggerVolume>,
    /// Below this height everything is out of the world.
    pub floor: f32,
    /// The Guardians' watch volumes and the safe zones they spare, on maps
    /// that have Guardians (Sandtrap, Sandbox); empty elsewhere.
    pub guards: Vec<TriggerVolume>,
}

impl KillZones {
    pub fn new(volumes: &[TriggerVolume], collision: &Collision) -> KillZones {
        let lowest = collision
            .triangles
            .iter()
            .flat_map(|t| t.points.map(|p| p.z))
            .fold(f32::INFINITY, f32::min);
        let guards = if volumes.iter().any(TriggerVolume::guards) {
            volumes.iter().filter(|v| v.guards() || v.is_safe_zone()).cloned().collect()
        } else {
            Vec::new()
        };
        KillZones {
            volumes: volumes.iter().filter(|v| v.kills()).cloned().collect(),
            floor: if lowest.is_finite() { lowest - BELOW_WORLD_WU } else { f32::NEG_INFINITY },
            guards,
        }
    }

    /// How many Guardian watch volumes there are.
    pub fn guard_count(&self) -> usize {
        self.guards.iter().filter(|v| v.guards()).count()
    }

    /// The kill volume `p` (Halo units) is in, "below the world", or the
    /// Guardian watch volume whose defender shoots there.
    pub fn deadly(&self, p: Vec3) -> Option<&str> {
        if p.z < self.floor {
            return Some("below the world");
        }
        self.volumes
            .iter()
            .find(|v| v.contains(p) && !kill_volume_disabled(&v.name))
            .or_else(|| guarded(&self.guards, p))
            .map(|v| v.name.as_str())
    }
}

/// Where to put the skater when it sinks before ever touching dry ground:
/// the map's start spawn (Halo position, yaw).
#[derive(Resource, Clone, Copy)]
pub struct FallbackSpot(pub Vec3, pub f32);

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
            guards: Vec::new(),
        };
        assert_eq!(zones.deadly(Vec3::new(0.0, 0.0, 5.0)), Some("kill_drop"));
        assert_eq!(zones.deadly(Vec3::new(0.0, 0.0, 16.0)), None);
        assert_eq!(zones.deadly(Vec3::new(0.0, 0.0, -70.0)), Some("below the world"));
    }

    /// Sandtrap's east Guardian: its watch volume reaches from x 27 out to
    /// x 126; the safe zone ends at x 89. Heights do not matter.
    #[test]
    fn guardians_shoot_outside_the_safe_zone() {
        let watch = TriggerVolume {
            name: "tv_turret_side1".into(),
            forward: Vec3::X,
            up: Vec3::Z,
            position: Vec3::new(27.08, -131.50, -26.61),
            extents: Vec3::new(99.35, 234.35, 19.64),
        };
        let safe = TriggerVolume {
            name: "safe_zone".into(),
            forward: Vec3::X,
            up: Vec3::Z,
            position: Vec3::new(-94.41, -100.08, -31.52),
            extents: Vec3::new(183.33, 181.43, 53.40),
        };
        let zones = KillZones::new(&[watch, safe], &Collision::default());
        assert_eq!(zones.guard_count(), 1);
        assert_eq!(zones.deadly(Vec3::new(50.0, -10.0, -20.0)), None);
        assert_eq!(zones.deadly(Vec3::new(50.0, -10.0, 40.0)), None);
        assert_eq!(zones.deadly(Vec3::new(100.0, -12.8, -20.3)), Some("tv_turret_side1"));
        assert_eq!(zones.deadly(Vec3::new(100.0, -12.8, 10.0)), Some("tv_turret_side1"));
        assert_eq!(zones.deadly(Vec3::new(140.0, -12.8, -20.0)), None);
    }
}
