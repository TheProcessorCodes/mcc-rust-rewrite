//! Halo 2 shaders (`shad`) resolved to what a first renderer needs: the
//! base colour map with its tiling, the detail maps, and how it blends.
//! Follows `docs/format/halo2.md` section 8.
//!
//! A Halo 2 shader stores no slot names. It lists the bitmaps and vertex
//! constants its template's passes read; the template's first lightmap pass
//! says, through its implementation and remapping tables, which of them
//! each pass slot is. What a slot means comes from the pass's name
//! ([`pass_slots`], our own table from the maps' shaders).

use blam_texture::{Blend, Xform};

use crate::cache::{Map, Reader};
use crate::Result;

/// What a pass slot holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Usage {
    Bump,
    Base,
    Detail,
    SecondaryDetail,
    OverlayDetail,
    Environment,
    Specular,
    SelfIllum,
    AlphaBlend,
    Noise,
    Alpha,
    Other,
}

/// A bitmap tag (by tag index, the key of the decoded bitmaps) and its tiling.
pub type MapRef = (u32, Xform);

/// How a shader draws, as far as the viewer cares.
#[derive(Clone, Debug)]
pub struct Look {
    pub shader: String,
    pub template: String,
    /// The first lightmap pass of the template, the one the slots come from.
    pub pass: String,
    pub base: Option<MapRef>,
    pub detail: Option<MapRef>,
    pub secondary_detail: Option<MapRef>,
    pub bump: Option<MapRef>,
    pub blend: Blend,
    pub alpha_test: bool,
    pub two_sided: bool,
    /// Never seen in game (invisible walls, portals).
    pub hidden: bool,
    pub water: bool,
    /// A sky template (`sky_*`).
    pub sky: bool,
    /// Ground that blends two detail maps by the base map's alpha
    /// (`*detail_blend*` templates): colour = base x lerp(detail,
    /// secondary, base alpha) x 2.
    pub detail_blend: bool,
}

/// The slots of a pass, in order (`docs/format/halo2.md` 8.3).
pub fn pass_slots(pass: &str) -> &'static [Usage] {
    use Usage::*;
    match pass {
        "lightmap_tex" => &[Bump, Base],
        "lightmap_tex_detail" | "lightmap_tex_detail_alpha_test" | "lightmap_tex_detail_shiny" => &[Bump, Base, Detail],
        "lightmap_tex_detail_alpha_test_no_bump" | "lightmap_tex_detail_no_bump" => &[Base, Detail],
        "lightmap_tex_detail_blend_detail" => &[Bump, Base, Detail, SecondaryDetail, OverlayDetail],
        "lightmap_tex_detail_blend" | "lightmap_tex_detail_blend_no_bump" => &[Bump, Base, Detail, SecondaryDetail],
        "lightmap_tex_detail_overlay" => &[Bump, Base, Detail, OverlayDetail],
        "lightmap_tex_env" => &[Bump, Base, Environment],
        "lightmap_tex_env_detail" => &[Bump, Base, Environment, Detail],
        "default_transparent_one_sided" => &[Environment, Specular, SelfIllum, AlphaBlend],
        "plasma_alpha" => &[Noise, Noise, Alpha],
        p if p.starts_with("lightmap_tex") && p.contains("no_bump") => &[Base, Detail, SecondaryDetail],
        p if p.starts_with("lightmap_tex") => &[Bump, Base, Detail, SecondaryDetail],
        _ => &[],
    }
}

/// A 9-bit start, 7-bit count range into another block.
fn range(v: u16) -> (usize, usize) {
    ((v & 0x1FF) as usize, (v >> 9) as usize)
}

fn is_default_bitmap(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("shaders\\default_bitmaps\\") || n.starts_with("rasterizer\\")
}

/// Which runtime bitmap and vertex constant each slot of the template's
/// chosen pass reads.
struct PassMap {
    name: String,
    bitmaps: Vec<usize>,
    constants: Vec<usize>,
}

fn pass_map(map: &Map, template: u32) -> Result<Option<PassMap>> {
    let Some(t) = map.tag_data(template) else {
        return Ok(None);
    };
    let Some(pp) = t.block(0x58, 0x28)?.into_iter().next() else {
        return Ok(None);
    };
    let passes = pp.block(0x10, 0xA)?;
    let impls = pp.block(0x18, 6)?;
    let remaps = pp.block(0x20, 4)?;
    // The first lightmap pass paints the base colour; else the first pass.
    let mut named = Vec::with_capacity(passes.len());
    for p in &passes {
        let name = map.tag_name(p.tag_ref(0)?).unwrap_or("");
        named.push((name.rsplit('\\').next().unwrap_or(name).to_string(), p.u16(8)?));
    }
    let chosen = named.iter().position(|(n, _)| n.starts_with("lightmap")).unwrap_or(0);
    let Some((name, impl_range)) = named.into_iter().nth(chosen) else {
        return Ok(None);
    };
    let Some(imp) = impls.get(range(impl_range).0) else {
        return Ok(None);
    };
    let slots = |v: u16| -> Result<Vec<usize>> {
        let (start, count) = range(v);
        (start..start + count)
            .map(|k| match remaps.get(k) {
                Some(r) => Ok(r.u8(3)? as usize),
                None => Ok(usize::MAX),
            })
            .collect()
    };
    Ok(Some(PassMap {
        name,
        bitmaps: slots(imp.u16(0)?)?,
        constants: slots(imp.u16(4)?)?,
    }))
}

/// Resolves a shader tag (looked up in `map`, or its `shared.map`).
pub fn resolve(map: &Map, shader: u32) -> Result<Option<Look>> {
    let Some(name) = map.tag_name(shader).map(str::to_string) else {
        return Ok(None);
    };
    let Some(s) = map.tag_data(shader) else {
        return Ok(None);
    };
    let template_id = s.tag_ref(0)?;
    let template = map.tag_name(template_id).unwrap_or("").to_string();
    let tl = template.to_ascii_lowercase();
    let nl = name.to_ascii_lowercase();

    // The runtime bitmaps and vertex constants.
    let runtime: Option<Reader<'_>> = s.block(0x20, 0x7C)?.into_iter().next();
    let mut bitmaps: Vec<u32> = Vec::new();
    let mut constants: Vec<[f32; 4]> = Vec::new();
    if let Some(r) = runtime {
        for b in r.block(0x4, 0xC)? {
            bitmaps.push(b.u32(0)?);
        }
        for c in r.block(0x14, 0x10)? {
            constants.push([c.f32(0)?, c.f32(4)?, c.f32(8)?, c.f32(12)?]);
        }
    }

    let mut look = Look {
        shader: name.clone(),
        template: template.clone(),
        pass: String::new(),
        base: None,
        detail: None,
        secondary_detail: None,
        bump: None,
        blend: Blend::Opaque,
        alpha_test: tl.contains("alpha_test") || tl.contains("foliage"),
        two_sided: tl.contains("two_sided") || tl.contains("alpha_test") || tl.contains("foliage"),
        hidden: ["invisible", "portal", "z_only", "collision_only", "player_clip", "sky_blocker"]
            .iter()
            .any(|w| nl.contains(w) || tl.contains(w))
            || nl == "shaders\\invalid",
        water: tl.contains("\\water\\"),
        sky: tl.contains("\\sky_") || tl.contains("\\sky\\"),
        detail_blend: tl.contains("detail_blend"),
    };
    if tl.contains("\\transparent\\") {
        look.blend = if tl.contains("plasma") || tl.contains("_add") || tl.contains("add_") || tl.contains("illum") {
            Blend::Additive
        } else if tl.contains("multiply") {
            Blend::Multiply
        } else {
            Blend::AlphaBlend
        };
    }
    if look.water {
        look.blend = Blend::AlphaBlend;
    }
    if tl.contains("\\opaque\\overlay") || tl.contains("decal") {
        look.blend = Blend::Multiply;
    }

    let real = |id: u32| map.tag_name(id).is_some_and(|n| !is_default_bitmap(n));
    if let Some(pm) = pass_map(map, template_id)? {
        let lightmap_pass = pm.name.starts_with("lightmap");
        let slots = pass_slots(&pm.name);
        // In the lightmap passes the vertex constants tile the slots in
        // order, the environment map (which takes none) left out.
        let mut constant = 0;
        for (j, &usage) in slots.iter().enumerate() {
            let tiled = usage != Usage::Environment;
            let c = constant;
            if tiled {
                constant += 1;
            }
            let Some(&bi) = pm.bitmaps.get(j) else { break };
            let Some(&bitmap) = bitmaps.get(bi) else { continue };
            if !real(bitmap) {
                continue;
            }
            let xform = match pm.constants.get(c).and_then(|&c| constants.get(c)) {
                Some(c) if lightmap_pass && tiled => tiling(*c),
                _ => Xform::IDENTITY,
            };
            let r = Some(((bitmap & 0xFFFF), xform));
            match usage {
                Usage::Base => look.base = r,
                Usage::Detail => look.detail = r,
                Usage::SecondaryDetail => look.secondary_detail = r,
                Usage::Bump => look.bump = r,
                Usage::SelfIllum | Usage::AlphaBlend | Usage::Alpha if look.base.is_none() && look.blend != Blend::Opaque => {
                    look.base = r
                }
                _ => {}
            }
        }
        look.pass = pm.name;
    }
    // Detail tiling is relative to the base map's.
    if let Some((_, b)) = look.base {
        for d in [&mut look.detail, &mut look.secondary_detail] {
            if let Some((_, x)) = d {
                x.scale = [x.scale[0] * b.scale[0], x.scale[1] * b.scale[1]];
            }
        }
    }
    // Templates whose passes are not in the table: the runtime properties'
    // diffuse map, or the first real bitmap that is not a bump map.
    if look.base.is_none() {
        let diffuse = s.block(0xC, 0x50)?.into_iter().next().map(|p| p.tag_ref(0)).transpose()?;
        if let Some(d) = diffuse.filter(|&d| real(d)) {
            look.base = Some((d & 0xFFFF, Xform::IDENTITY));
        } else if let Some(&b) = bitmaps.iter().find(|&&b| {
            let n = map.tag_name(b).unwrap_or("");
            real(b) && !n.contains("bump") && !n.contains("reflection") && !n.contains("cube")
        }) {
            look.base = Some((b & 0xFFFF, Xform::IDENTITY));
        }
    }
    Ok(Some(look))
}

/// A lightmap pass's vertex constant as a tiling: scale in x, y and offset
/// in z, w; overlay constants keep their scale in z, w (`0 0 10 10`).
fn tiling(c: [f32; 4]) -> Xform {
    if c[0] == 0.0 && c[1] == 0.0 && c[2] != 0.0 && c[3] != 0.0 {
        return Xform {
            scale: [c[2], c[3]],
            offset: [0.0, 0.0],
        };
    }
    let s = |v: f32| if v == 0.0 { 1.0 } else { v };
    Xform {
        scale: [s(c[0]), s(c[1])],
        offset: [c[2], c[3]],
    }
}
