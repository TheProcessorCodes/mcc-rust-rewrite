//! `blam movement <map>`: the player's on-foot movement numbers
//! (`blam_geometry::movement`, `docs/format/movement.md`).

use anyhow::Result;
use blam_cache::CacheFile;
use blam_geometry::WORLD_UNIT_METRES;

pub(crate) fn run(cache: &CacheFile) -> Result<()> {
    let m = blam_geometry::movement::read(cache)?;
    let wu = |name: &str, v: f32, unit: &str| {
        println!("{name:<28} {v:>9.4} wu{unit:<5} {:>9.3} m{unit}", v * WORLD_UNIT_METRES);
    };
    let deg = |name: &str, v: f32| println!("{name:<28} {:>9.2} deg", v.to_degrees());
    let plain = |name: &str, v: f32, unit: &str| println!("{name:<28} {v:>9.4} {unit}");

    println!("biped {}", m.biped);
    println!("-- speeds (globals, player information)");
    wu("run forward", m.run_forward, "/s");
    wu("run backward", m.run_backward, "/s");
    wu("run sideways", m.run_sideways, "/s");
    wu("run acceleration", m.run_acceleration, "/s2");
    wu("crouch forward", m.crouch_forward, "/s");
    wu("crouch backward", m.crouch_backward, "/s");
    wu("crouch sideways", m.crouch_sideways, "/s");
    wu("crouch acceleration", m.crouch_acceleration, "/s2");
    wu("airborne acceleration", m.airborne_acceleration, "/s2");
    wu("walk speed (inferred)", m.walk_speed, "/s");
    println!("-- jump and gravity");
    wu("jump velocity", m.jump_velocity, "/s");
    wu("gravity (derived)", m.gravity, "/s2");
    wu("jump height (derived)", m.jump_height(), "");
    println!("-- body (biped, character physics)");
    wu("height standing", m.height_standing, "");
    wu("height crouching", m.height_crouching, "");
    wu("radius", m.radius, "");
    plain("mass", m.mass, "");
    wu("standing camera height", m.standing_camera_height, "");
    wu("crouching camera height", m.crouching_camera_height, "");
    plain("crouch transition time", m.crouch_transition_time, "s");
    deg("field of view", m.field_of_view);
    println!("-- ground");
    deg("max slope", m.max_slope_angle);
    deg("downhill falloff", m.downhill_falloff_angle);
    deg("downhill cutoff", m.downhill_cutoff_angle);
    deg("uphill falloff", m.uphill_falloff_angle);
    deg("uphill cutoff", m.uphill_cutoff_angle);
    plain("downhill velocity scale", m.downhill_velocity_scale, "");
    plain("uphill velocity scale", m.uphill_velocity_scale, "");
    deg("climb inflection", m.climb_inflection_angle);
    println!("-- landing and falling");
    wu("min soft landing speed", m.min_soft_landing_velocity, "/s");
    wu("min hard landing speed", m.min_hard_landing_velocity, "/s");
    wu("max hard landing speed", m.max_hard_landing_velocity, "/s");
    wu("harmful fall from", m.harmful_fall_distance[0], "");
    wu("harmful fall to", m.harmful_fall_distance[1], "");
    wu("fatal fall", m.max_fall_distance, "");
    println!("-- look and aim");
    plain("look yaw rate", m.look_yaw_rate_deg, "deg/s");
    plain("look pitch rate", m.look_pitch_rate_deg, "deg/s");
    deg("moving turn speed /s", m.moving_turn_speed);
    plain("magnetism friction", m.magnetism_friction, "");
    plain("magnetism adhesion", m.magnetism_adhesion, "");
    wu("lock-on distance", m.lock_on_distance, "");
    Ok(())
}
