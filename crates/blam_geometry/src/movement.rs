//! The on-foot movement numbers for the player: speeds and accelerations
//! from the globals tag (`matg`), and jump, camera, collision capsule and
//! slope limits from the player's biped tag (`bipd`) and its character
//! physics. Offsets and how each was checked are in
//! `docs/format/movement.md`; `docs/format/scripts/probe_movement.py`
//! prints every one of them.
//!
//! All lengths are world units (wu), speeds wu/s, accelerations wu/s²,
//! angles radians, times seconds. [`crate::WORLD_UNIT_METRES`] converts.

use blam_cache::CacheFile;

use crate::{Error, Result};

/// `matg` +0x404: the player control block (look rates, aim assist).
pub const MATG_PLAYER_CONTROL: usize = 0x404;
/// `matg` +0x434: the player information block (run and crouch speeds).
pub const MATG_PLAYER_INFORMATION: usize = 0x434;
/// `matg` +0x440: the player representation block (which biped).
pub const MATG_PLAYER_REPRESENTATION: usize = 0x440;
/// Size of one player representation element.
pub const PLAYER_REPRESENTATION_SIZE: usize = 0x5C;
/// Third-person unit (`bipd`) reference inside a representation element.
pub const REPRESENTATION_BIPED: usize = 0x28;
/// `matg` +0x458: the falling damage block.
pub const MATG_FALLING_DAMAGE: usize = 0x458;

/// `bipd` +0x10C: the unit's camera field of view.
pub const BIPD_FIELD_OF_VIEW: usize = 0x10C;
/// `bipd` +0x2BC: start of the biped's own fields (turning speed first).
pub const BIPD_BIPED: usize = 0x2BC;
/// `bipd` +0x340: start of the character physics heights and radius.
pub const BIPD_PHYSICS: usize = 0x340;
/// `bipd` +0x368: the character physics pill (capsule) shapes block.
pub const BIPD_PILLS: usize = 0x368;
/// Size of one pill shape element.
pub const PILL_SIZE: usize = 0x70;
/// `bipd` +0x380: the ground physics angles and scales.
pub const BIPD_GROUND: usize = 0x380;

/// Halo's gravity in wu/s². Not a tag field: the engine's constant. We
/// recover it from the falling damage block, whose runtime velocities are
/// `sqrt(2 g h)` for its three distances; all three give this value on every
/// map checked, and it equals Halo 1's documented 0.0035651 wu/tick² at
/// 30 ticks/s.
pub const GRAVITY: f32 = 3.208_66;

/// The player's on-foot movement numbers for one map.
#[derive(Clone, Debug, PartialEq)]
pub struct Movement {
    /// The biped the numbers come from (the first player representation
    /// that has one: `mp_masterchief` on multiplayer maps).
    pub biped: String,

    // Globals, player information.
    /// `+0x00`. The slow walk speed (label inferred; Halo 3 players have no
    /// walk key, so probably AI or scripted walking).
    pub walk_speed: f32,
    /// `+0x04`..`+0x0C`: full stick run speeds.
    pub run_forward: f32,
    pub run_backward: f32,
    pub run_sideways: f32,
    /// `+0x10`: ground acceleration while running.
    pub run_acceleration: f32,
    /// `+0x14`..`+0x1C`: crouched ("sneak") speeds.
    pub crouch_forward: f32,
    pub crouch_backward: f32,
    pub crouch_sideways: f32,
    /// `+0x20`: ground acceleration while crouched.
    pub crouch_acceleration: f32,
    /// `+0x24`: air control acceleration.
    pub airborne_acceleration: f32,

    // Globals, player control.
    /// `+0x2C`, `+0x30`: full-stick look rates in degrees per second.
    pub look_pitch_rate_deg: f32,
    pub look_yaw_rate_deg: f32,
    /// `+0x00`, `+0x04`: aim assist magnetism friction and adhesion.
    pub magnetism_friction: f32,
    pub magnetism_adhesion: f32,

    // Globals, falling damage.
    /// `+0x00`: falls shorter than the first distance do no damage; the
    /// damage ramps up to the second.
    pub harmful_fall_distance: [f32; 2],
    /// `+0x58`: falls this far or more kill.
    pub max_fall_distance: f32,
    /// Gravity implied by the falling damage block's runtime velocities.
    pub gravity: f32,

    // Biped.
    /// Unit `+0x10C`: first-person field of view (70 degrees).
    pub field_of_view: f32,
    /// `+0x2BC`: turning speed while moving, rad/s.
    pub moving_turn_speed: f32,
    /// `+0x2C8`: upward speed at take-off.
    pub jump_velocity: f32,
    /// `+0x2D4`, `+0x2D8`, `+0x2DC`: landing speed thresholds: above the first
    /// a soft landing, above the second a hard landing (stun), the third
    /// the top of the hard landing range.
    pub min_soft_landing_velocity: f32,
    pub min_hard_landing_velocity: f32,
    pub max_hard_landing_velocity: f32,
    /// `+0x2E8`, `+0x2EC`: eye heights above the feet.
    pub standing_camera_height: f32,
    pub crouching_camera_height: f32,
    /// `+0x2F0`: seconds to crouch or stand.
    pub crouch_transition_time: f32,
    /// `+0x314`: aim lock-on distance.
    pub lock_on_distance: f32,

    // Character physics.
    /// `+0x340`, `+0x344`, `+0x348`: capsule heights and radius. The two
    /// pill shapes span exactly 0..height with this radius.
    pub height_standing: f32,
    pub height_crouching: f32,
    pub radius: f32,
    /// `+0x34C`.
    pub mass: f32,
    /// `+0x380`: steepest walkable ground.
    pub max_slope_angle: f32,
    /// `+0x384`..`+0x390`: slopes between falloff and cutoff slow (uphill)
    /// or speed up (downhill) the runner.
    pub downhill_falloff_angle: f32,
    pub downhill_cutoff_angle: f32,
    pub uphill_falloff_angle: f32,
    pub uphill_cutoff_angle: f32,
    /// `+0x394`, `+0x398`.
    pub downhill_velocity_scale: f32,
    pub uphill_velocity_scale: f32,
    /// `+0x3B0`.
    pub climb_inflection_angle: f32,
}

impl Movement {
    /// Height of the jump's apex above take-off, `v² / 2g`.
    pub fn jump_height(&self) -> f32 {
        self.jump_velocity * self.jump_velocity / (2.0 * self.gravity)
    }
}

/// Reads the player's movement numbers from an open map.
pub fn read(cache: &CacheFile) -> Result<Movement> {
    let matg = cache
        .global_tag("matg")
        .ok_or_else(|| Error::Invalid("no globals (matg) tag".into()))?;
    let m = cache.tag_offset(matg)?;
    let one = |offset: usize, what: &str| -> Result<usize> {
        let b = cache.reader_at(m + offset).block()?;
        b.element(0, 0)
            .ok_or_else(|| Error::Invalid(format!("globals: no {what} block")))
    };

    let pi = one(MATG_PLAYER_INFORMATION, "player information")?;
    let mut r = cache.reader_at(pi);
    let [walk_speed, run_forward, run_backward, run_sideways, run_acceleration] =
        [r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?];
    let [crouch_forward, crouch_backward, crouch_sideways, crouch_acceleration, airborne_acceleration] =
        [r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?];

    let pc = one(MATG_PLAYER_CONTROL, "player control")?;
    let [magnetism_friction, magnetism_adhesion] = cache.reader_at(pc).f32x2()?;
    let [look_pitch_rate_deg, look_yaw_rate_deg] = cache.reader_at(pc + 0x2C).f32x2()?;

    let fd = one(MATG_FALLING_DAMAGE, "falling damage")?;
    let harmful_fall_distance = cache.reader_at(fd).f32x2()?;
    let max_fall_distance = cache.reader_at(fd + 0x58).f32()?;
    let max_fall_velocity = cache.reader_at(fd + 0x6C).f32()?;
    let gravity = if max_fall_distance > 0.0 && max_fall_velocity > 0.0 {
        max_fall_velocity * max_fall_velocity / (2.0 * max_fall_distance)
    } else {
        GRAVITY
    };

    let reps = cache.reader_at(m + MATG_PLAYER_REPRESENTATION).block()?;
    let biped = reps
        .elements(PLAYER_REPRESENTATION_SIZE)
        .filter_map(|e| {
            let r = cache.reader_at(e + REPRESENTATION_BIPED).tag_ref().ok()?;
            (r.class.as_str() == "bipd").then_some(r.index?)
        })
        .find_map(|i| cache.tags.get(i))
        .ok_or_else(|| Error::Invalid("globals: no player biped".into()))?;
    let b = cache.tag_offset(biped)?;
    let f = |offset: usize| cache.reader_at(b + offset).f32();

    let movement = Movement {
        biped: biped.name().to_string(),
        walk_speed,
        run_forward,
        run_backward,
        run_sideways,
        run_acceleration,
        crouch_forward,
        crouch_backward,
        crouch_sideways,
        crouch_acceleration,
        airborne_acceleration,
        look_pitch_rate_deg,
        look_yaw_rate_deg,
        magnetism_friction,
        magnetism_adhesion,
        harmful_fall_distance,
        max_fall_distance,
        gravity,
        field_of_view: f(BIPD_FIELD_OF_VIEW)?,
        moving_turn_speed: f(BIPD_BIPED)?,
        jump_velocity: f(BIPD_BIPED + 0x0C)?,
        min_soft_landing_velocity: f(BIPD_BIPED + 0x18)?,
        min_hard_landing_velocity: f(BIPD_BIPED + 0x1C)?,
        max_hard_landing_velocity: f(BIPD_BIPED + 0x20)?,
        standing_camera_height: f(BIPD_BIPED + 0x2C)?,
        crouching_camera_height: f(BIPD_BIPED + 0x30)?,
        crouch_transition_time: f(BIPD_BIPED + 0x34)?,
        lock_on_distance: f(BIPD_BIPED + 0x58)?,
        height_standing: f(BIPD_PHYSICS)?,
        height_crouching: f(BIPD_PHYSICS + 4)?,
        radius: f(BIPD_PHYSICS + 8)?,
        mass: f(BIPD_PHYSICS + 0x0C)?,
        max_slope_angle: f(BIPD_GROUND)?,
        downhill_falloff_angle: f(BIPD_GROUND + 0x04)?,
        downhill_cutoff_angle: f(BIPD_GROUND + 0x08)?,
        uphill_falloff_angle: f(BIPD_GROUND + 0x0C)?,
        uphill_cutoff_angle: f(BIPD_GROUND + 0x10)?,
        downhill_velocity_scale: f(BIPD_GROUND + 0x14)?,
        uphill_velocity_scale: f(BIPD_GROUND + 0x18)?,
        climb_inflection_angle: f(BIPD_GROUND + 0x30)?,
    };
    check(cache, b, &movement)?;
    Ok(movement)
}

/// Guards against a layout we were not written for: the ground physics
/// keeps `cos(max slope)` at `+0x39C`, and the standing pill must reach the
/// standing height.
fn check(cache: &CacheFile, b: usize, m: &Movement) -> Result<()> {
    let bad = |what: &str| Err(Error::Invalid(format!("{}: {what}", m.biped)));
    let cos_slope = cache.reader_at(b + BIPD_GROUND + 0x1C).f32()?;
    if (cos_slope - m.max_slope_angle.cos()).abs() > 1e-3 {
        return bad("ground physics does not match its stored cosine");
    }
    let pills = cache.reader_at(b + BIPD_PILLS).block()?;
    if let Some(p) = pills.element(0, PILL_SIZE) {
        let radius = cache.reader_at(p + 0x40).f32()?;
        let top = cache.reader_at(p + 0x58).f32()?;
        if (top + radius - m.height_standing).abs() > 1e-3 {
            return bad("standing pill does not reach the standing height");
        }
    }
    let speeds = [m.run_forward, m.jump_velocity, m.height_standing, m.gravity];
    if speeds.iter().any(|v| !v.is_finite() || *v <= 0.0 || *v > 100.0) {
        return bad("implausible movement numbers");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(name: &str) -> Option<CacheFile> {
        let path = std::path::Path::new(
            r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps",
        )
        .join(format!("{name}.map"));
        path.exists().then(|| CacheFile::open(path).unwrap())
    }

    /// Valhalla's numbers, as in `docs/format/movement.md` section 6.
    #[test]
    fn riverworld_movement() {
        let Some(cache) = map("riverworld") else { return };
        let m = read(&cache).unwrap();
        assert_eq!(m.biped, r"objects\characters\masterchief\mp_masterchief\mp_masterchief");
        let close = |a: f32, b: f32| assert!((a - b).abs() < 1e-4, "{a} != {b}");
        close(m.walk_speed, 0.512);
        close(m.run_forward, 2.25);
        close(m.run_backward, 2.0);
        close(m.run_sideways, 2.0);
        close(m.run_acceleration, 9.6);
        close(m.crouch_forward, 0.9);
        close(m.crouch_backward, 0.65);
        close(m.crouch_sideways, 0.6);
        close(m.crouch_acceleration, 4.8);
        close(m.airborne_acceleration, 1.05);
        close(m.look_pitch_rate_deg, 60.0);
        close(m.look_yaw_rate_deg, 120.0);
        close(m.magnetism_friction, 0.6);
        close(m.magnetism_adhesion, 0.6);
        assert_eq!(m.harmful_fall_distance, [6.0, 10.0]);
        close(m.max_fall_distance, 14.0);
        close(m.gravity, GRAVITY);
        close(m.field_of_view.to_degrees(), 70.0);
        close(m.moving_turn_speed, 8.48579);
        close(m.jump_velocity, 3.08);
        close(m.min_soft_landing_velocity, 0.5);
        close(m.min_hard_landing_velocity, 3.8);
        close(m.max_hard_landing_velocity, 10.0);
        close(m.standing_camera_height, 0.62);
        close(m.crouching_camera_height, 0.45);
        close(m.crouch_transition_time, 0.2);
        close(m.lock_on_distance, 15.0);
        close(m.height_standing, 0.725);
        close(m.height_crouching, 0.5);
        close(m.radius, 0.175);
        close(m.mass, 125.0);
        close(m.max_slope_angle.to_degrees(), 50.0);
        close(m.downhill_falloff_angle.to_degrees(), 45.0);
        close(m.downhill_cutoff_angle.to_degrees(), 50.0);
        close(m.uphill_falloff_angle.to_degrees(), 45.0);
        close(m.uphill_cutoff_angle.to_degrees(), 50.0);
        close(m.downhill_velocity_scale, 1.5);
        close(m.uphill_velocity_scale, 0.5);
        close(m.climb_inflection_angle.to_degrees(), -20.0);
        assert!((m.jump_height() - 1.4782).abs() < 1e-3);
    }
}
