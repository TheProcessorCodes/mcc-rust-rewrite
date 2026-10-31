//! Halo CE shaders resolved to what a first renderer needs, in the same
//! [`blam_h2::Look`] the Halo 2 reader makes (so the viewer turns both into
//! its looks the same way). Follows `docs/format/halo1.md` section 10.
//!
//! Halo CE has one tag class per kind of shader, each with its maps at
//! fixed offsets: `senv` (the level's ground and walls), `soso` (objects),
//! `sotr` / `schi` / `scex` (transparent, stacks of maps), `swat` (water),
//! `sgla` (glass), `smet` (meters) and `spla` (plasma).

use blam_h2::Look;
use blam_texture::{Blend, Xform};

use crate::cache::{Map, Reader};
use crate::Result;

/// A bitmap tag (by tag index) and its tiling.
pub type MapRef = (u32, Xform);

/// A resolved shader: its look, and for the transparent kinds every 2D map
/// of its stack in order (the sky picks from those).
#[derive(Clone, Debug)]
pub struct Shading {
    pub look: Look,
    pub maps: Vec<MapRef>,
}

/// A bitmap reference at `off` as a [`MapRef`], when it names a bitmap.
fn map_at(map: &Map, r: Reader<'_>, off: usize, xform: Xform) -> Result<Option<MapRef>> {
    let id = r.tag_ref(off)?;
    Ok(map.tag(id).filter(|t| t.is(b"bitm")).map(|_| (id & 0xFFFF, xform)))
}

/// A tiling scale as stored: zero means one.
fn scale(v: f32) -> f32 {
    if v == 0.0 || !v.is_finite() { 1.0 } else { v }
}

fn tiling(u: f32, v: f32) -> Xform {
    Xform {
        scale: [scale(u), scale(v)],
        offset: [0.0, 0.0],
    }
}

/// The framebuffer blend function of the transparent shaders (`+0x2C`):
/// alpha blend, multiply, double multiply, add, subtract, component min,
/// component max, alpha-multiply add. `None`: not drawn.
fn framebuffer_blend(v: u16) -> Option<Blend> {
    match v {
        0 => Some(Blend::AlphaBlend),
        1 | 2 => Some(Blend::Multiply),
        3 | 7 => Some(Blend::Additive),
        _ => None,
    }
}

fn empty(map: &Map, shader: u32, class: &[u8; 4]) -> Look {
    Look {
        shader: map.tag_name(shader).unwrap_or("").to_string(),
        template: String::from_utf8_lossy(class).into_owned(),
        pass: String::new(),
        base: None,
        detail: None,
        secondary_detail: None,
        bump: None,
        blend: Blend::Opaque,
        alpha_test: false,
        two_sided: false,
        hidden: false,
        water: false,
        sky: false,
        detail_blend: false,
    }
}

/// Resolves a shader tag. `None` for a null reference or a class not
/// handled (logged by the caller as a flat colour).
pub fn resolve(map: &Map, shader: u32) -> Result<Option<Shading>> {
    let Some(tag) = map.tag(shader) else { return Ok(None) };
    let class = tag.class;
    let Some(r) = map.tag_data(shader) else { return Ok(None) };
    let mut look = empty(map, shader, &class);
    let mut maps = Vec::new();
    match &class {
        b"senv" => {
            // Flags `+0x28` (bit 0: alpha tested), type `+0x2A` (0 normal,
            // 1 blended, 2 blended base specular). The base map is drawn
            // with the BSP's own texcoords; each detail map's scale is
            // relative to them.
            look.alpha_test = r.u16(0x28)? & 1 != 0;
            let kind = r.u16(0x2A)?;
            look.base = map_at(map, r, 0x88, Xform::IDENTITY)?;
            let primary = map_at(map, r, 0xB8, tiling(r.f32(0xB4)?, r.f32(0xB4)?))?;
            let secondary = map_at(map, r, 0xCC, tiling(r.f32(0xC8)?, r.f32(0xC8)?))?;
            look.bump = map_at(map, r, 0x128, tiling(r.f32(0x124)?, r.f32(0x124)?))?;
            if kind != 0 && primary.is_some() && secondary.is_some() {
                // Blended: the base map's alpha picks between the two
                // detail maps (white: the secondary).
                look.detail_blend = true;
                look.detail = primary;
                look.secondary_detail = secondary;
            } else {
                look.detail = primary.or(secondary);
            }
        }
        b"soso" => {
            // Flags `+0x28`: bit 1 two-sided, bit 2 not alpha tested, bit 3
            // alpha-blended decal. Map scale `+0x9C` / `+0xA0`; detail
            // scale `+0xD8`, its extra v scale `+0xEC`.
            let flags = r.u16(0x28)?;
            look.two_sided = flags & 2 != 0;
            look.alpha_test = flags & 4 == 0;
            if flags & 8 != 0 {
                look.blend = Blend::AlphaBlend;
            }
            look.base = map_at(map, r, 0xA4, tiling(r.f32(0x9C)?, r.f32(0xA0)?))?;
            let d = scale(r.f32(0xD8)?);
            look.detail = map_at(map, r, 0xDC, tiling(d, d * scale(r.f32(0xEC)?)))?;
        }
        b"sotr" | b"schi" | b"scex" => {
            // Stacks of maps (`+0x54`): `sotr` maps are 0x64 bytes (scale
            // u, v at +4, +8, offset at +0xC, +0x10, bitmap at +0x1C);
            // `schi` and `scex` maps 0xDC (scale +0x54, +0x58, offset
            // +0x5C, +0x60, bitmap +0x6C). The first map type (`+0x2A`)
            // other than 0 makes the first map a cube map.
            let (size, at) = if &class == b"sotr" { (0x64, [0x4, 0x8, 0xC, 0x10, 0x1C]) } else { (0xDC, [0x54, 0x58, 0x5C, 0x60, 0x6C]) };
            let first_is_cube = r.u16(0x2A)? != 0;
            for (k, m) in r.block(0x54, size)?.into_iter().enumerate() {
                if k == 0 && first_is_cube {
                    continue;
                }
                let x = Xform {
                    scale: [scale(m.f32(at[0])?), scale(m.f32(at[1])?)],
                    offset: [m.f32(at[2])?, m.f32(at[3])?],
                };
                maps.extend(map_at(map, m, at[4], x)?);
            }
            look.base = maps.first().copied();
            match framebuffer_blend(r.u16(0x2C)?) {
                Some(b) => look.blend = b,
                None => look.hidden = true,
            }
            look.alpha_test = r.u8(0x29)? & 1 != 0;
            look.two_sided = true;
        }
        b"swat" => {
            // Water: the base map at `+0x4C`, drawn see-through.
            look.base = map_at(map, r, 0x4C, Xform::IDENTITY)?;
            look.water = true;
            look.blend = Blend::AlphaBlend;
            look.two_sided = true;
        }
        b"sgla" => {
            // Glass: its diffuse map (`+0x158`, scale `+0x154`) added over
            // what is behind; the reflection cube map is not drawn.
            look.base = map_at(map, r, 0x158, tiling(r.f32(0x154)?, r.f32(0x154)?))?;
            look.blend = Blend::Additive;
            look.two_sided = true;
        }
        // Meters and plasma shields are drawn by game state; left out.
        b"smet" | b"spla" => look.hidden = true,
        _ => return Ok(None),
    }
    Ok(Some(Shading { look, maps }))
}
