//! Halo world units (Z up) and renderer metres (Y up).

use bevy::prelude::*;

use blam_geometry::WORLD_UNIT_METRES;

/// Halo world units, Z up, to renderer metres, Y up.
pub fn to_render(h: Vec3) -> Vec3 {
    Vec3::new(h.x, h.z, -h.y) * WORLD_UNIT_METRES
}

/// Renderer metres, Y up, to Halo world units, Z up.
pub fn to_halo(r: Vec3) -> Vec3 {
    Vec3::new(r.x, -r.z, r.y) / WORLD_UNIT_METRES
}

pub fn to_halo_dir(r: Vec3) -> Vec3 {
    Vec3::new(r.x, -r.z, r.y)
}

pub fn to_render_dir(h: Vec3) -> Vec3 {
    Vec3::new(h.x, h.z, -h.y)
}
