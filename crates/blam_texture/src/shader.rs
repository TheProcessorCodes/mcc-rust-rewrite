//! Shader tags (`rmsh`, `rmtr`, `rmfl`, `rmw `, `rmd `, ...) resolved to the
//! bitmaps they sample, each with its tiling, plus the options that say how
//! the surface blends. Follows `docs/format/shaders.md` sections 2 to 7;
//! MCC Reach's render methods have the same chain at other offsets
//! (`docs/format/reach.md` 9), picked by the cache file's [`Engine`].

use blam_cache::{Block, CacheFile, Engine, Reader, StringId};

use crate::{Error, Result};

/// Most entries any shader block is allowed to have before it is treated
/// as garbage (the largest seen is a few dozen).
const MAX_ENTRIES: u32 = 4096;

/// A texture's UV transform: `uv' = uv * scale + offset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xform {
    pub scale: [f32; 2],
    pub offset: [f32; 2],
}

impl Xform {
    pub const IDENTITY: Xform = Xform {
        scale: [1.0, 1.0],
        offset: [0.0, 0.0],
    };
}

/// One texture slot of the shader's template, filled in.
#[derive(Clone, Debug)]
pub struct ShaderMap {
    /// The usage name, e.g. `base_map`, `detail_map_m_2`.
    pub usage: String,
    /// The usage without a terrain `_m_N` suffix.
    pub base_usage: String,
    /// The terrain layer `N` of a `_m_N` usage.
    pub layer: Option<u8>,
    /// The `bitm` tag index, or `None` for a null reference.
    pub bitmap: Option<u32>,
    pub bitmap_name: String,
    pub xform: Xform,
}

/// A bitmap and the tiling it is sampled with.
#[derive(Clone, Debug, PartialEq)]
pub struct TextureSlot {
    pub bitmap: u32,
    pub name: String,
    pub usage: String,
    pub xform: Xform,
}

/// How the surface is composited, from the `blend_mode` option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Opaque,
    /// `dst + src`: glows, plasma.
    Additive,
    /// `dst * src` (and `double_multiply`).
    Multiply,
    /// `src * a + dst * (1 - a)`.
    AlphaBlend,
    /// `src + dst * (1 - a)`: glass.
    PreMultiplied,
}

/// One terrain layer: the base map that layer `N` paints with.
#[derive(Clone, Debug)]
pub struct TerrainLayer {
    pub base: TextureSlot,
    pub detail: Option<TextureSlot>,
}

/// A terrain shader's blend map and its four layers (red, green, blue,
/// alpha of the blend map). A layer whose material is `off` is `None`.
#[derive(Clone, Debug)]
pub struct Terrain {
    pub blend_map: Option<TextureSlot>,
    pub layers: [Option<TerrainLayer>; 4],
}

/// What an animated shader argument changes (`docs/format/water.md` 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationKind {
    /// A scalar argument (`time_warp`).
    Value,
    Color,
    ScaleUniform,
    ScaleX,
    ScaleY,
    /// A texture's xform offset x (the map scrolls).
    TranslationX,
    TranslationY,
    FrameIndex,
    Alpha,
    Other(i32),
}

impl AnimationKind {
    fn of(v: i32) -> AnimationKind {
        match v {
            0 => AnimationKind::Value,
            1 => AnimationKind::Color,
            2 => AnimationKind::ScaleUniform,
            3 => AnimationKind::ScaleX,
            4 => AnimationKind::ScaleY,
            5 => AnimationKind::TranslationX,
            6 => AnimationKind::TranslationY,
            7 => AnimationKind::FrameIndex,
            8 => AnimationKind::Alpha,
            v => AnimationKind::Other(v),
        }
    }
}

/// One argument driven by a function of time: the shader property's
/// function list (`+0x5C`) joined to its routing list (`+0x50`).
#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    /// The template argument it drives (a texture usage for xforms).
    pub argument: String,
    pub kind: AnimationKind,
    /// Seconds per cycle.
    pub period: f32,
    /// The function's output range over one cycle, read as
    /// `from + (to - from) * fract(t / period)` (a sawtooth; see water.md).
    pub from: f32,
    pub to: f32,
}

impl Animation {
    /// Units per second the value moves by.
    pub fn rate(&self) -> f32 {
        if self.period > 1e-4 {
            (self.to - self.from) / self.period
        } else {
            0.0
        }
    }
}

/// Everything the renderer needs from one shader tag.
#[derive(Clone, Debug)]
pub struct Shader {
    pub tag_index: u32,
    pub name: String,
    /// `rmsh`, `rmtr`, `rmfl`, `rmw `, `rmd `, ...
    pub class: String,
    /// The render method definition's path, e.g. `shaders\shader`.
    pub definition: String,
    /// `(category, option)` for every category of the definition.
    pub options: Vec<(String, String)>,
    pub maps: Vec<ShaderMap>,
    /// The template's non-texture arguments: colours and scalars.
    pub params: Vec<(String, [f32; 4])>,
    /// The colour texture (for terrain: layer 0's, or the first present).
    pub base: Option<TextureSlot>,
    /// Where an alpha-tested shader takes its alpha from, when it is not
    /// the base map (`alpha_test_map`).
    pub alpha_map: Option<TextureSlot>,
    /// Tint multiplied into the base colour, or the flat colour when there
    /// is no base map (`albedo_color`, `water_diffuse`, ...).
    pub albedo_color: Option<[f32; 4]>,
    pub blend: Blend,
    /// Pixels below an alpha threshold are discarded.
    pub alpha_test: bool,
    pub two_sided: bool,
    /// Never seen by a player: invisible blockers, portals, z-only
    /// geometry, effects classes (HaloExport's "special").
    pub special: bool,
    pub terrain: Option<Terrain>,
    /// Arguments animated over time (scrolling maps, `time_warp`).
    pub animations: Vec<Animation>,
}

impl Shader {
    /// The animation of `kind` on `argument`, if any.
    pub fn animation(&self, argument: &str, kind: AnimationKind) -> Option<&Animation> {
        self.animations
            .iter()
            .find(|a| a.argument == argument && a.kind == kind)
    }

    /// How fast a texture's xform offset moves, in UV units per second.
    pub fn scroll(&self, usage: &str) -> [f32; 2] {
        let rate = |k| self.animation(usage, k).map_or(0.0, Animation::rate);
        [rate(AnimationKind::TranslationX), rate(AnimationKind::TranslationY)]
    }

    pub fn option(&self, category: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(c, _)| c == category)
            .map(|(_, o)| o.as_str())
    }

    pub fn map(&self, usage: &str) -> Option<&ShaderMap> {
        self.maps.iter().find(|m| m.usage == usage)
    }

    pub fn param(&self, name: &str) -> Option<[f32; 4]> {
        self.params.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }

    /// Whether the surface is drawn see-through (blended, not cut out).
    pub fn is_translucent(&self) -> bool {
        !matches!(self.blend, Blend::Opaque) || self.class == "rmw "
    }
}

fn string(cache: &CacheFile, id: StringId) -> String {
    cache.string(id).unwrap_or("").to_string()
}

fn bounded(block: Block, what: &str, name: &str) -> Result<Block> {
    if block.count > MAX_ENTRIES {
        return Err(Error::Invalid(format!("{name}: {what} has {} entries", block.count)));
    }
    Ok(block)
}

fn tag_of<'c>(cache: &'c CacheFile, r: &mut Reader<'c>, want: &str, name: &str) -> Result<&'c blam_cache::TagEntry> {
    let reference = r.tag_ref()?;
    let tag = reference
        .index
        .and_then(|i| cache.tags.get(i))
        .ok_or_else(|| Error::Invalid(format!("{name}: null {want} reference")))?;
    let class = cache.tags.class_of(tag).code.as_str();
    if class != want {
        return Err(Error::Invalid(format!(
            "{name}: expected a {want} tag, found {class} {}",
            tag.name()
        )));
    }
    Ok(tag)
}

/// Splits a terrain usage `name_m_N` into `(name, N)`.
pub fn split_layer(usage: &str) -> (&str, Option<u8>) {
    let b = usage.as_bytes();
    if b.len() > 4 && &b[b.len() - 4..b.len() - 1] == b"_m_" && b[b.len() - 1].is_ascii_digit() {
        return (&usage[..usage.len() - 4], Some(b[b.len() - 1] - b'0'));
    }
    (usage, None)
}

/// HaloExport's "special" path rule: a path segment that is, or starts with
/// `<word>_`, one of these words.
fn special_path(path: &str) -> bool {
    const WORDS: [&str; 7] = ["invisible", "sky_portal", "skyportal", "portal", "z_only", "zonly", "black"];
    path.to_ascii_lowercase().split(['\\', '/']).any(|seg| {
        WORDS.iter().any(|w| {
            seg.strip_prefix(w)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('_'))
        })
    })
}

fn blend_of(option: Option<&str>) -> Blend {
    match option.unwrap_or("opaque") {
        "additive" | "add_src_times_dstalpha" | "add_src_times_srcalpha" | "multiply_add" | "maximum" => {
            Blend::Additive
        }
        "multiply" | "double_multiply" => Blend::Multiply,
        "alpha_blend" | "inv_alpha_blend" => Blend::AlphaBlend,
        "pre_multiplied_alpha" => Blend::PreMultiplied,
        _ => Blend::Opaque,
    }
}

/// Resolves a shader tag (any render method class).
pub fn resolve(cache: &CacheFile, tag_index: u32) -> Result<Shader> {
    let tag = cache
        .tags
        .get(tag_index)
        .ok_or_else(|| Error::Invalid(format!("no tag #{tag_index}")))?;
    let name = tag.name().to_string();
    let class = cache.tags.class_of(tag).code.as_str().to_string();
    let n = name.as_str();
    let s = cache.tag_offset(tag)?;
    let mut r = cache.reader_at(s);
    // Where the option list and property block sit, a property's size, a
    // texture entry's size and its tiling index byte.
    let (options_at, props_at, prop_size, map_size, tiling_byte) = match cache.engine() {
        Engine::Halo3 => (0x10, 0x28, 132, 28, 0x16),
        Engine::Reach => (0x20, 0x38, 0xB4, 24, 0x15),
    };

    // 1. options, named by the definition's categories.
    let rmdf = tag_of(cache, &mut r, "rmdf", n)?;
    let option_block = bounded(r.at(s + options_at).block()?, "the option list", n)?;
    let props = bounded(r.at(s + props_at).block()?, "the property block", n)?;
    let rmdf_off = cache.tag_offset(rmdf)?;
    let categories = bounded(cache.reader_at(rmdf_off + 0x10).block()?, "the category list", n)?;
    let mut options = Vec::new();
    for (i, o) in option_block.elements(2).enumerate() {
        let chosen = cache.reader_at(o).i16()?;
        let Some(cat) = categories.element(i as u32, 24) else { break };
        let mut cr = cache.reader_at(cat);
        let cat_name = string(cache, cr.string_id()?);
        let opts = bounded(cr.block()?, "an option list", n)?;
        if let Some(opt) = u32::try_from(chosen).ok().and_then(|c| opts.element(c, 28)) {
            options.push((cat_name, string(cache, cache.reader_at(opt).string_id()?)));
        }
    }

    // 2. the template's usage and argument names.
    let prop = props
        .element(0, prop_size)
        .ok_or_else(|| Error::Invalid(format!("{n}: no shader properties")))?;
    let mut pr = cache.reader_at(prop);
    let rmt2 = tag_of(cache, &mut pr, "rmt2", n)?;
    let maps_block = bounded(pr.at(prop + 0x10).block()?, "the shader maps", n)?;
    let tiling_block = bounded(pr.at(prop + 0x1C).block()?, "the tiling data", n)?;
    let t = cache.tag_offset(rmt2)?;
    let names = |at: usize, what: &str| -> Result<Vec<String>> {
        bounded(cache.reader_at(t + at).block()?, what, n)?
            .elements(4)
            .map(|o| Ok(string(cache, cache.reader_at(o).string_id()?)))
            .collect()
    };
    let arguments = names(0x48, "the template arguments")?;
    let usages = names(0x6C, "the template usages")?;
    let tiling: Vec<[f32; 4]> = tiling_block
        .elements(16)
        .map(|o| cache.reader_at(o).f32x4())
        .collect::<std::result::Result<_, _>>()?;

    // 3. textures, one per usage, each with its xform.
    let mut maps = Vec::new();
    for (i, usage) in usages.iter().enumerate() {
        let Some(o) = maps_block.element(i as u32, map_size) else { break };
        let reference = cache.reader_at(o).tag_ref()?;
        let bitmap = reference.index.and_then(|b| cache.tags.get(b));
        let bitmap = bitmap.filter(|b| cache.tags.class_of(b).code.as_str() == "bitm");
        let by_byte = cache.reader_at(o + tiling_byte).u8()?;
        let k = if by_byte != 255 && (by_byte as usize) < tiling.len() {
            Some(by_byte as usize)
        } else {
            arguments.iter().position(|a| a == usage).filter(|&k| k < tiling.len())
        };
        let xform = k.map_or(Xform::IDENTITY, |k| {
            let v = tiling[k];
            Xform {
                scale: [v[0], v[1]],
                offset: [v[2], v[3]],
            }
        });
        let (base_usage, layer) = split_layer(usage);
        maps.push(ShaderMap {
            usage: usage.clone(),
            base_usage: base_usage.to_string(),
            layer,
            bitmap: bitmap.map(|b| b.index),
            bitmap_name: bitmap.map_or_else(String::new, |b| b.name().to_string()),
            xform,
        });
    }

    // 4. the remaining arguments are colours and scalars.
    let params: Vec<(String, [f32; 4])> = arguments
        .iter()
        .zip(&tiling)
        .filter(|(a, _)| !usages.contains(a))
        .map(|(a, v)| (a.clone(), *v))
        .collect();

    // 5. animated arguments: functions (36 bytes: kind, two string ids,
    // period, data reference to the function bytes) and the routing entries
    // (4 bytes: register, register type, function index, argument index)
    // that tie each function to the argument it drives.
    let mut animations = Vec::new();
    let routes = bounded(pr.at(prop + 0x50).block()?, "the routing list", n)?;
    let functions = bounded(pr.at(prop + 0x5C).block()?, "the function list", n)?;
    let mut seen = std::collections::HashSet::new();
    for o in routes.elements(4) {
        let mut rr = cache.reader_at(o + 2);
        let (function, argument) = (rr.u8()? as u32, rr.u8()? as usize);
        if !seen.insert((function, argument)) {
            continue;
        }
        let (Some(f), Some(arg)) = (functions.element(function, 36), arguments.get(argument)) else {
            continue;
        };
        let mut fr = cache.reader_at(f);
        let kind = AnimationKind::of(fr.i32()?);
        let period = fr.at(f + 0xC).f32()?;
        let data = fr.at(f + 0x10).data_ref()?;
        let (from, to) = match data.offset.filter(|_| data.size >= 12) {
            Some(d) => (cache.reader_at(d + 4).f32()?, cache.reader_at(d + 8).f32()?),
            None => (0.0, 0.0),
        };
        animations.push(Animation {
            argument: arg.clone(),
            kind,
            period,
            from,
            to,
        });
    }

    let mut shader = Shader {
        tag_index,
        name: name.clone(),
        class,
        definition: rmdf.name().to_string(),
        options,
        maps,
        params,
        base: None,
        alpha_map: None,
        albedo_color: None,
        blend: Blend::Opaque,
        alpha_test: false,
        two_sided: false,
        special: false,
        terrain: None,
        animations,
    };
    classify(&mut shader);
    Ok(shader)
}

fn slot(m: &ShaderMap) -> Option<TextureSlot> {
    Some(TextureSlot {
        bitmap: m.bitmap?,
        name: m.bitmap_name.clone(),
        usage: m.usage.clone(),
        xform: m.xform,
    })
}

/// An additive halogram (`rmhg`) with no colour map of its own (`albedo
/// constant_color`): what shows is its glow and its overlay, never its flat
/// albedo colour. Sandbox's Forge grids (`sandbox_grid`, `sandbox_grid_dim`:
/// albedo 0.75 grey, plasma glow, an `overlay_map` of grid lines tiled 62
/// times) drew as a solid white sheet over the floor when that grey was
/// added. An overlay that adds (`additive`, `additive_detail`), tinted by
/// `overlay_tint`, becomes the colour map (the grid lines); otherwise the
/// colour is black, leaving the self-illumination alone (shaders.md 8.7).
/// A `multiply` overlay (Blackout's aurora, drawn by the sky) only tints
/// the glow and is left to the sky shader.
fn halogram_glow(s: &mut Shader) {
    let overlay = matches!(s.option("overlay"), Some("additive" | "additive_detail"));
    s.base = if overlay { s.map("overlay_map").and_then(slot) } else { None };
    s.albedo_color = match &s.base {
        Some(_) => s.param("overlay_tint").map(|t| [t[0], t[1], t[2], 1.0]),
        None => Some([0.0, 0.0, 0.0, 1.0]),
    };
}

/// Fills in the derived fields (shaders.md section 7).
fn classify(s: &mut Shader) {
    let class = s.class.clone();
    let class = class.as_str();
    s.blend = blend_of(s.option("blend_mode"));
    s.alpha_test = s.option("alpha_test").is_some_and(|o| o != "none");
    s.special = special_path(&s.name)
        || !matches!(class, "rmsh" | "rmtr" | "rmfl" | "rmw " | "rmd " | "rmhg" | "rmcs" | "rmtd");
    s.two_sided = matches!(class, "rmfl" | "rmd ") || s.alpha_test;
    let find = |s: &Shader, usage: &str| s.map(usage).and_then(slot);

    s.albedo_color = s.param("albedo_color");
    match class {
        "rmtr" => {
            let mut layers: [Option<TerrainLayer>; 4] = Default::default();
            for (n, layer) in layers.iter_mut().enumerate() {
                let of = |usage: &str| {
                    s.maps
                        .iter()
                        .find(|m| m.layer == Some(n as u8) && m.base_usage == usage)
                        .and_then(slot)
                };
                *layer = of("base_map").map(|base| TerrainLayer {
                    base,
                    detail: of("detail_map"),
                });
            }
            s.base = layers.iter().flatten().next().map(|l| l.base.clone());
            s.albedo_color = s.param("global_albedo_tint");
            s.terrain = Some(Terrain {
                blend_map: find(s, "blend_map"),
                layers,
            });
        }
        "rmw " => {
            if s.option("watercolor") == Some("texture") {
                s.base = find(s, "watercolor_texture");
            }
            s.albedo_color = s.param("water_diffuse");
        }
        _ => {
            let albedo = s.option("albedo").unwrap_or("default");
            s.base = if matches!(albedo, "change_color" | "emblem_change_color") {
                find(s, "change_color_map").or_else(|| find(s, "base_map"))
            } else {
                find(s, "base_map")
            };
            if s.base.is_none() && s.blend == Blend::Additive {
                s.base = find(s, "self_illum_map");
                if let Some(c) = s.param("self_illum_color") {
                    s.albedo_color = Some(c);
                }
            }
            if class == "rmhg" && s.base.is_none() && s.blend == Blend::Additive {
                halogram_glow(s);
            }
            if s.alpha_test {
                let alpha = find(s, "alpha_test_map");
                s.alpha_map = alpha.filter(|a| s.base.as_ref().is_none_or(|b| b.bitmap != a.bitmap));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_suffixes() {
        assert_eq!(split_layer("base_map_m_2"), ("base_map", Some(2)));
        assert_eq!(split_layer("detail_bump_m_0"), ("detail_bump", Some(0)));
        assert_eq!(split_layer("base_map"), ("base_map", None));
        assert_eq!(split_layer("_m_1"), ("_m_1", None));
        assert_eq!(split_layer("m_1"), ("m_1", None));
    }

    #[test]
    fn special_paths() {
        assert!(special_path(r"levels\shared\shaders\invisible"));
        assert!(special_path(r"levels\multi\riverworld\shaders\sky_portal_blocker"));
        assert!(special_path(r"shaders\black"));
        assert!(!special_path(r"levels\multi\riverworld\shaders\blackrock"));
        assert!(!special_path(r"levels\multi\snowbound\shaders\z_invis_lightquad"));
        assert!(!special_path(r"levels\multi\riverworld\shaders\riverworld_rock"));
    }

    #[test]
    fn blend_modes() {
        assert_eq!(blend_of(None), Blend::Opaque);
        assert_eq!(blend_of(Some("pre_multiplied_alpha")), Blend::PreMultiplied);
        assert_eq!(blend_of(Some("double_multiply")), Blend::Multiply);
        assert_eq!(blend_of(Some("add_src_times_srcalpha")), Blend::Additive);
    }
}
