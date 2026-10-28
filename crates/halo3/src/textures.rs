//! What each scene material looks like: its colour texture, tiling and
//! blending, worked out from the map before the window opens. CPU only;
//! `world.rs` turns the result into bevy images and materials.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use blam_cache::CacheFile;
use blam_geometry::Scene;
use blam_texture::shader::{self, Shader};
use blam_texture::{Blend, Texture, Xform, bitmap, rgba};

/// How one scene material is drawn.
#[derive(Clone, Debug)]
pub struct Look {
    /// The colour bitmap's tag index, when it decoded.
    pub texture: Option<u32>,
    pub xform: Xform,
    /// Multiplied into the texture, or the whole colour without one
    /// (linear RGBA as the shader stores it).
    pub colour: Option<[f32; 4]>,
    pub blend: Blend,
    pub alpha_test: bool,
    pub two_sided: bool,
    /// Never visible in the game: not drawn.
    pub hidden: bool,
    /// Water: drawn see-through in a flat colour.
    pub water: bool,
    /// Terrain (`rmtr`): the blend map and the four layers, when the blend
    /// map and at least one layer decoded. `texture` then still holds the
    /// dominant layer, for drawing paths that take a single texture.
    pub terrain: Option<TerrainLook>,
}

/// A terrain shader's textures (`docs/format/shaders.md` 7.5): the blend
/// map, sampled at the raw UV through its own xform, weights layer N by its
/// channel N (red, green, blue, alpha).
#[derive(Clone, Debug)]
pub struct TerrainLook {
    /// Tag index of the blend map (decoded into `MaterialTextures::bitmaps`;
    /// its texels are weights, not colours).
    pub blend_map: u32,
    pub blend_xform: Xform,
    /// `None` when the layer is `off` or its base map did not decode.
    pub layers: [Option<TerrainLayerLook>; 4],
}

/// One terrain layer: base map times detail map, each with its own tiling.
#[derive(Clone, Debug)]
pub struct TerrainLayerLook {
    pub base: u32,
    pub base_xform: Xform,
    /// `None` when the layer has no detail map or it did not decode.
    pub detail: Option<(u32, Xform)>,
}

#[derive(Default)]
pub struct MaterialTextures {
    /// One entry per scene material, `None` when the shader did not
    /// resolve (drawn in a flat colour).
    pub looks: Vec<Option<Look>>,
    /// Decoded colour bitmaps by tag index, each once.
    pub bitmaps: HashMap<u32, Texture>,
    /// The two lightmap arrays of each `Scene::lightmaps` entry, when both
    /// decoded (`docs/format/lightmaps.md` 2.2).
    pub lightmaps: Vec<Option<LightmapTextures>>,
}

/// One BSP's lightmap arrays, still BC3 as stored.
pub struct LightmapTextures {
    /// 8 layers: SH coefficients c0..c3, base + residual each.
    pub coefficients: Texture,
    /// 2 layers: dominant light intensity, base + residual.
    pub intensity: Texture,
}

/// The terrain layer the blend map weights most, over the whole map (what
/// HaloExport picks when it needs one texture for a terrain).
fn dominant_layer(
    cache: &CacheFile,
    resources: &blam_cache::Resources,
    s: &Shader,
    means: &mut HashMap<u32, [f32; 4]>,
) -> Option<blam_texture::TextureSlot> {
    let terrain = s.terrain.as_ref()?;
    let weights = terrain.blend_map.as_ref().and_then(|b| {
        if let Some(w) = means.get(&b.bitmap) {
            return Some(*w);
        }
        let tex = bitmap::decode(cache, resources, b.bitmap, 0).ok()?;
        let px = rgba::decode_level(tex.format, tex.width, tex.height, tex.level(0, 0)?).ok()?;
        let w = rgba::channel_means(&px);
        means.insert(b.bitmap, w);
        Some(w)
    });
    let present = terrain.layers.iter().enumerate().filter_map(|(n, l)| Some((n, l.as_ref()?)));
    let best = match weights {
        Some(w) => present.max_by(|a, b| w[a.0].total_cmp(&w[b.0])),
        None => present.into_iter().next(),
    };
    best.map(|(_, l)| l.base.clone())
}

/// Resolves every material's shader and decodes the colour bitmaps they
/// need. Failures are logged and leave that material flat-coloured.
pub fn load(cache: &CacheFile, scene: &Scene) -> MaterialTextures {
    let mut out = MaterialTextures::default();
    let resources = match cache.resources() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("textures: no resource tables ({e}); drawing flat colours");
            out.looks = vec![None; scene.materials.len()];
            return out;
        }
    };
    let mut blend_means = HashMap::new();
    let mut failed: HashSet<u32> = HashSet::new();
    for m in &scene.materials {
        let Some(tag) = m.shader_tag else {
            out.looks.push(None);
            continue;
        };
        let s = match shader::resolve(cache, tag) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("shader {}: {e}", m.tag_path);
                out.looks.push(None);
                continue;
            }
        };
        let water = s.class == "rmw ";
        let mut fetch = |slot: &blam_texture::TextureSlot| -> Option<u32> {
            match out.bitmaps.entry(slot.bitmap) {
                Entry::Occupied(_) => Some(slot.bitmap),
                Entry::Vacant(v) if !failed.contains(&slot.bitmap) => {
                    match bitmap::decode(cache, &resources, slot.bitmap, 0) {
                        Ok(t) => {
                            v.insert(t);
                            Some(slot.bitmap)
                        }
                        Err(e) => {
                            eprintln!("bitmap {} (for {}): {e}", slot.name, m.tag_path);
                            failed.insert(slot.bitmap);
                            None
                        }
                    }
                }
                Entry::Vacant(_) => None,
            }
        };
        let slot = if s.terrain.is_some() {
            dominant_layer(cache, &resources, &s, &mut blend_means)
        } else if water {
            // The watercolor texture's alpha is not opacity and its colour
            // is the water_diffuse colour; draw the colour alone.
            None
        } else {
            s.base.clone()
        };
        let texture = slot.as_ref().and_then(&mut fetch);
        let terrain = s.terrain.as_ref().and_then(|t| {
            let blend = t.blend_map.as_ref()?;
            let blend_map = fetch(blend)?;
            let layers = t.layers.clone().map(|l| {
                let l = l?;
                Some(TerrainLayerLook {
                    base: fetch(&l.base)?,
                    base_xform: l.base.xform,
                    detail: l.detail.as_ref().and_then(|d| Some((fetch(d)?, d.xform))),
                })
            });
            layers.iter().any(Option::is_some).then_some(TerrainLook {
                blend_map,
                blend_xform: blend.xform,
                layers,
            })
        });
        out.looks.push(Some(Look {
            texture,
            xform: slot.as_ref().map_or(Xform::IDENTITY, |s| s.xform),
            colour: s.albedo_color,
            blend: s.blend,
            alpha_test: s.alpha_test,
            two_sided: s.two_sided,
            // Cut-outs with no colour map are light-emitting quads tested
            // against `color_black` (shaders.md 8.6): nothing is visible.
            hidden: s.special || (s.alpha_test && s.base.is_none()),
            water,
            terrain,
        }));
    }
    for lm in &scene.lightmaps {
        let load = |tag: Option<u32>, layers: u32| -> Result<Texture, String> {
            let tag = tag.ok_or("no bitmap")?;
            let t = bitmap::decode(cache, &resources, tag, 0).map_err(|e| e.to_string())?;
            if t.layers < layers || t.mips.is_empty() {
                return Err(format!("{}: {} layers, {} levels", t.name, t.layers, t.mips.len()));
            }
            Ok(t)
        };
        match (load(lm.coefficients_bitmap, 8), load(lm.intensity_bitmap, 2)) {
            (Ok(coefficients), Ok(intensity)) => out.lightmaps.push(Some(LightmapTextures {
                coefficients,
                intensity,
            })),
            (a, b) => {
                let e = a.err().or(b.err()).unwrap_or_default();
                eprintln!("lightmap of bsp {}: {e}; fallback lighting there", lm.bsp);
                out.lightmaps.push(None);
            }
        }
    }
    // The 90 MB local page and the shared.map pages are no longer needed.
    resources.clear_page_cache();
    out
}
