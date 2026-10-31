//! What each scene material looks like: its colour texture, tiling and
//! blending, worked out from the map before the window opens. CPU only;
//! `world.rs` turns the result into bevy images and materials.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

pub mod reach;

use blam_cache::CacheFile;
use blam_geometry::Scene;
use blam_texture::shader::{self, AnimationKind, Shader};
use blam_texture::{Blend, Texture, TextureSlot, Xform, bitmap, rgba};

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
    /// Everything past the colour map: detail, bump, specular, environment
    /// and self-illumination (`docs/format/shaders.md` section 11).
    pub surface: SurfaceLook,
}

/// A bitmap tag index and the tiling it is sampled with.
pub type MapRef = (u32, Xform);

/// How a lit surface reflects light, from the shader's `material_model`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Model {
    /// `diffuse_only`, `foliage`, `none` ...: no specular.
    #[default]
    Diffuse,
    /// `two_lobe_phong`, `single_lobe_phong` and their kin: a normal and a
    /// glancing lobe blended by a fresnel curve.
    Phong,
    /// `cook_torrance*`: roughness and a Schlick fresnel from `fresnel_color`.
    CookTorrance,
    /// `glass`: a fresnel-weighted reflection.
    Glass,
}

/// Where the specular mask comes from (`specular_mask` option).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpecMask {
    #[default]
    None,
    /// The base map's alpha.
    BaseAlpha,
    /// `specular_mask_texture` (red, or the colour for
    /// `specular_mask_from_color_texture`).
    Texture,
}

/// Self-illumination kinds the viewer draws (`self_illumination` option).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Illum {
    #[default]
    Off,
    /// `self_illum_map * colour * intensity` (`simple`, `simple_with_alpha_mask`).
    Map,
    /// `illum_detail`: the map times its detail map (scaled like a detail map).
    MapDetail,
    /// `from_diffuse`: the albedo times the colour.
    FromDiffuse,
    /// `self_illum_times_diffuse`: the map times the albedo.
    TimesDiffuse,
    /// `3_channel_self_illum`: the map's red, green and blue each tint a colour.
    ThreeChannel,
    /// `plasma` (Guardian's jump pads and minilift, Narrows' light strips
    /// and energy walls, Sandtrap's shields): the glow lives where
    /// `alpha_mask_map`'s alpha is (its red when the alpha is flat,
    /// `SurfaceLook::illum_mask_in_colour`); it is tinted by the three band
    /// colours (`color_medium`, `color_wide`, `color_sharp`), each weighted
    /// by how much of its band shows on average (`plasma_band_weights`),
    /// times the intensity. The scrolling noise that makes the bands crawl
    /// is not drawn.
    Plasma,
}

/// A shader's parameters and extra maps beyond the base colour.
#[derive(Clone, Debug, Default)]
pub struct SurfaceLook {
    /// The alpha test's own map (`alpha_test_map`) when it is not the base
    /// map.
    pub alpha_map: Option<MapRef>,
    /// The albedo detail map, multiplied over the base (`albedo` default,
    /// detail_blend, two_detail ...).
    pub detail: Option<MapRef>,
    pub bump: Option<MapRef>,
    /// `bump_detail_map` and `bump_detail_coefficient`.
    pub bump_detail: Option<(u32, Xform, f32)>,
    pub spec_mask: SpecMask,
    pub spec_mask_map: Option<MapRef>,
    pub model: Model,
    /// `specular_coefficient` (times `diffuse`-free models: 0).
    pub specular: f32,
    /// Phong: normal and glancing powers; Cook-Torrance: both from roughness.
    pub power: [f32; 2],
    /// Normal and glancing specular tints (linear RGB as stored).
    pub tint: [[f32; 3]; 2],
    /// `fresnel_curve_steepness`.
    pub fresnel_steepness: f32,
    /// Cook-Torrance `fresnel_color` / glass `fresnel_coefficient` (F0).
    pub f0: [f32; 3],
    pub analytical: f32,
    pub area: f32,
    /// `environment_mapping` is not `none`: the reflection's strength
    /// (`environment_map_specular_contribution`, or 1 for glass).
    pub env: f32,
    pub env_tint: [f32; 3],
    pub illum: Illum,
    pub illum_map: Option<MapRef>,
    /// Plasma only: the mask's pattern is in its red channel (its alpha is
    /// flat white), so the shader reads red, and the map is uploaded linear.
    pub illum_mask_in_colour: bool,
    pub illum_detail: Option<MapRef>,
    /// `self_illum_color * self_illum_intensity`; three colours for
    /// `3_channel_self_illum` (`channel_a/b/c`).
    pub illum_colour: [[f32; 3]; 3],
}

/// A terrain shader's textures (`docs/format/shaders.md` 7.5): the blend
/// map, sampled at the raw UV through its own xform, weights layer N by its
/// channel N (red, green, blue, alpha).
#[derive(Clone, Debug)]
pub struct TerrainLook {
    /// Tag index of the blend map (decoded into `MaterialTextures::bitmaps`;
    /// its texels are weights, not colours).
    /// `None` for a one-layer terrain whose template has no blend map
    /// (Sandbox's sand): every layer that is on then weighs the same.
    pub blend_map: Option<u32>,
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
    /// `bump_map_m_N`, when it decoded.
    pub bump: Option<(u32, Xform)>,
    /// `specular_coefficient_m_N` when the layer is `diffuse_plus_specular*`
    /// (its base alpha is the mask), else 0.
    pub specular: f32,
    /// `specular_power_m_N`.
    pub power: f32,
    /// `specular_tint_m_N`.
    pub tint: [f32; 3],
    /// `analytical_specular_contribution_m_N`.
    pub analytical: f32,
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
) -> Option<TextureSlot> {
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

/// The engine's stand-in bitmaps that change nothing when sampled (flat
/// normal, mid-grey detail): treated as absent.
fn is_neutral(name: &str) -> bool {
    let file = name.rsplit('\\').next().unwrap_or(name);
    matches!(file, "default_vector" | "default_detail" | "gray_50_percent")
}

/// Shaders of things a player never sees outside Forge or their own game
/// type: the spawn markers (black discs, white plasma columns), the
/// capture-the-flag plates and the hill generator, and the engine's
/// missing-shader grid.
fn is_game_type_marker(shader: &str) -> bool {
    shader.starts_with("objects\\multi\\shaders\\spawn_point")
        || shader.starts_with("objects\\multi\\models\\shaders\\multiplayer_")
        || shader == "objects\\multi\\shaders\\koth_shield_generator"
        || shader == "shaders\\invalid"
}

/// A shader map as a texture slot, unless it is missing or neutral.
fn useful_slot(m: &shader::ShaderMap) -> Option<TextureSlot> {
    let bitmap = m.bitmap?;
    (!is_neutral(&m.bitmap_name)).then(|| TextureSlot {
        bitmap,
        name: m.bitmap_name.clone(),
        usage: m.usage.clone(),
        xform: m.xform,
    })
}

/// `self_illum_intensity`. When a function of time drives it and the stored
/// value is 0, the top of the function's range instead: Isolation's energy
/// walls `plasma_wall_b` and `plasma_wall_c` store 0 and pulse 0 -> 55 (8 s
/// and 10 s), so drawn at the stored value they never lit, while
/// `plasma_wall_a`, the same pulse with 55 stored, did. The pulse itself is
/// not drawn. A stored value other than 0 is kept: on the maps checked it is
/// usually one end of the function's range, and those glows were matched by
/// eye at it.
fn illum_intensity(s: &Shader) -> f32 {
    let stored = s.param("self_illum_intensity").map_or(1.0, |v| v[0]);
    match s.animation("self_illum_intensity", AnimationKind::Value) {
        Some(a) if stored == 0.0 => a.from.max(a.to),
        _ => stored,
    }
}

#[cfg(test)]
mod intensity_tests {
    use super::illum_intensity;
    use blam_texture::Blend;
    use blam_texture::shader::{Animation, AnimationKind, Shader};

    fn shader(stored: f32, pulse: Option<(f32, f32)>) -> Shader {
        Shader {
            tag_index: 0,
            name: "plasma_wall".into(),
            class: "rmsh".into(),
            definition: r"shaders\shader".into(),
            options: vec![("self_illumination".into(), "plasma".into())],
            maps: Vec::new(),
            params: vec![("self_illum_intensity".into(), [stored; 4])],
            base: None,
            alpha_map: None,
            albedo_color: None,
            blend: Blend::Additive,
            alpha_test: false,
            two_sided: false,
            special: false,
            terrain: None,
            animations: pulse
                .map(|(from, to)| Animation {
                    argument: "self_illum_intensity".into(),
                    kind: AnimationKind::Value,
                    period: 8.0,
                    from,
                    to,
                })
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn a_pulse_stored_as_zero_glows_at_its_top() {
        // Isolation's plasma_wall_b: 0 stored, 0 -> 55.
        assert_eq!(illum_intensity(&shader(0.0, Some((0.0, 55.0)))), 55.0);
        // plasma_wall_a: 55 stored, the same pulse.
        assert_eq!(illum_intensity(&shader(55.0, Some((0.0, 55.0)))), 55.0);
        // Narrows' man cannons: 0.5 stored, 1.5 -> 0.5, kept.
        assert_eq!(illum_intensity(&shader(0.5, Some((1.5, 0.5)))), 0.5);
        // Not animated: the stored value, even 0.
        assert_eq!(illum_intensity(&shader(0.0, None)), 0.0);
        assert_eq!(illum_intensity(&shader(3.0, None)), 3.0);
    }
}

/// A Blinn-Phong power with the lobe width of a roughness.
fn power_of_roughness(r: f32) -> f32 {
    let r = r.clamp(0.02, 1.0);
    (2.0 / (r * r) - 2.0).clamp(1.0, 2000.0)
}

/// How much of each plasma band (medium, wide, sharp) shows on average.
///
/// A plasma glow is lit where two scrolling noise maps nearly agree: with
/// `d` one minus their difference, a band of thinness `n` is `d^n`; the
/// wide band is what `d^wide` leaves outside the medium one, the medium
/// band what `d^medium` leaves outside the sharp one, and the sharp band is
/// `d^sharp` (`docs/format/shaders.md` 11.6). The viewer does not draw the
/// crawling bands, only their mean: for two independent noises spread evenly
/// over 0..1, `d` has density `2d`, so `d^n` averages `2 / (n + 2)`. Thin
/// bands (large `n`) come out faint: Narrows' energy walls (sharp thinness
/// 100 and 256, intensity 40 and 150) glow softly instead of blowing out.
fn plasma_band_weights(thinness: [f32; 3]) -> [f32; 3] {
    let mean = |n: f32| 2.0 / (n.max(0.0) + 2.0);
    let [medium, wide, sharp] = thinness.map(mean);
    [(medium - sharp).max(0.0), (wide - medium).max(0.0), sharp]
}

/// Whether a plasma `alpha_mask_map` keeps its pattern in the colour rather
/// than in the alpha. Guardian's masks are A8 (alpha only); Narrows'
/// `plasma_mask` (borrowed from The Covenant) is A8R8G8B8 with every alpha
/// 255 and its streaks in the colour, which read as a solid sheet through
/// the alpha. A mask whose alpha is flat white is read from its red channel.
fn mask_in_colour(cache: &CacheFile, resources: &blam_cache::Resources, bitmap: u32) -> bool {
    let Ok(tex) = bitmap::decode(cache, resources, bitmap, 0) else {
        return false;
    };
    let Some(level) = tex.level(0, 0) else {
        return false;
    };
    let Ok(px) = rgba::decode_level(tex.format, tex.width, tex.height, level) else {
        return false;
    };
    let means = rgba::channel_means(&px);
    means[3] > 0.99 && means[0] < 0.99
}

/// Reads the material model, maps and parameters of an ordinary (`rmsh`
/// and kin) shader (`docs/format/shaders.md` section 11). Terrain and
/// water get the default (nothing extra).
fn surface_look(s: &Shader, fetch: &mut dyn FnMut(&TextureSlot) -> Option<u32>) -> SurfaceLook {
    let mut out = SurfaceLook::default();
    if s.class == "rmw " {
        // Water: a fresnel-weighted reflection of the light around it and
        // a sharp sun glint; the waves and refraction are not drawn.
        out.model = Model::Glass;
        out.specular = 1.0;
        out.power = [400.0; 2];
        out.tint = [[1.0; 3]; 2];
        out.f0 = [s.param("fresnel_coefficient").map_or(0.1, |v| v[0]); 3];
        out.fresnel_steepness = 5.0;
        out.analytical = 1.0;
        out.env = 1.0;
        out.env_tint = [1.0; 3];
        return out;
    }
    if s.class == "rmtr" || s.special {
        return out;
    }
    out.alpha_map = s.alpha_map.as_ref().and_then(|a| Some((fetch(a)?, a.xform)));
    let mut map = |usage: &str| -> Option<MapRef> {
        let slot = s.map(usage).and_then(useful_slot)?;
        Some((fetch(&slot)?, slot.xform))
    };
    let p1 = |name: &str, default: f32| s.param(name).map_or(default, |v| v[0]);
    let rgb = |name: &str, default: [f32; 3]| s.param(name).map_or(default, |v| [v[0], v[1], v[2]]);

    let albedo = s.option("albedo").unwrap_or("default");
    if matches!(
        albedo,
        "default" | "detail_blend" | "two_detail" | "two_detail_overlay" | "two_detail_black_point" | "three_detail_blend"
    ) {
        out.detail = map("detail_map");
    }
    match s.option("bump_mapping").unwrap_or("off") {
        // Decals' `leave` keeps what is under them.
        "off" | "leave" => {}
        b => {
            out.bump = map("bump_map");
            if b != "standard" {
                out.bump_detail = map("bump_detail_map").map(|(t, x)| (t, x, p1("bump_detail_coefficient", 1.0)));
            }
        }
    }
    out.spec_mask = match s.option("specular_mask").unwrap_or("no_specular_mask") {
        "specular_mask_from_diffuse" => SpecMask::BaseAlpha,
        "specular_mask_from_texture" | "specular_mask_from_color_texture" => {
            out.spec_mask_map = map("specular_mask_texture");
            if out.spec_mask_map.is_some() { SpecMask::Texture } else { SpecMask::None }
        }
        _ => SpecMask::None,
    };
    out.model = match s.option("material_model").unwrap_or("diffuse_only") {
        m if m.starts_with("cook_torrance") => Model::CookTorrance,
        m if m.contains("phong") => Model::Phong,
        "glass" => Model::Glass,
        _ => Model::Diffuse,
    };
    out.specular = p1("specular_coefficient", 0.0);
    out.analytical = p1("analytical_specular_contribution", 0.5);
    out.area = p1("area_specular_contribution", 0.0);
    match out.model {
        Model::Diffuse => out.specular = 0.0,
        Model::Phong => {
            let normal = p1("normal_specular_power", p1("specular_power", 10.0));
            out.power = [normal, p1("glancing_specular_power", normal)];
            let tint = rgb("normal_specular_tint", rgb("specular_tint", [1.0; 3]));
            out.tint = [tint, rgb("glancing_specular_tint", tint)];
            out.fresnel_steepness = p1("fresnel_curve_steepness", 5.0);
        }
        Model::CookTorrance => {
            let power = power_of_roughness(p1("roughness", 0.3));
            out.power = [power, power];
            let tint = rgb("specular_tint", [1.0; 3]);
            out.tint = [tint, tint];
            out.f0 = rgb("fresnel_color", [0.5; 3]);
            out.fresnel_steepness = 5.0;
        }
        Model::Glass => {
            let power = power_of_roughness(p1("roughness", 0.0));
            out.power = [power, power];
            out.tint = [[1.0; 3]; 2];
            out.f0 = [p1("fresnel_coefficient", 0.1); 3];
            out.fresnel_steepness = p1("fresnel_curve_steepness", 5.0);
        }
    }
    if s.option("environment_mapping").unwrap_or("none") != "none" && out.model != Model::Diffuse {
        out.env = if out.model == Model::Glass {
            1.0
        } else {
            p1("environment_map_specular_contribution", 0.0)
        };
        out.env_tint = rgb("env_tint_color", [1.0; 3]);
    }
    let intensity = illum_intensity(s);
    out.illum_colour = [rgb("self_illum_color", [1.0; 3]).map(|x| x * intensity); 3];
    out.illum = match s.option("self_illumination").unwrap_or("off") {
        "off" => Illum::Off,
        "from_diffuse" => Illum::FromDiffuse,
        "self_illum_times_diffuse" => Illum::TimesDiffuse,
        "illum_detail" => Illum::MapDetail,
        "plasma" => {
            let colours = ["color_medium", "color_wide", "color_sharp"].map(|n| rgb(n, [0.0; 3]));
            let thinness = [
                p1("thinness_medium", 16.0),
                p1("thinness_wide", 4.0),
                p1("thinness_sharp", 32.0),
            ];
            let weights = plasma_band_weights(thinness);
            let glow = [0, 1, 2].map(|c| (0..3).map(|b| colours[b][c] * weights[b]).sum::<f32>() * intensity);
            out.illum_colour = [glow; 3];
            Illum::Plasma
        }
        "3_channel_self_illum" => {
            out.illum_colour = [
                rgb("channel_a", [1.0; 3]).map(|x| x * intensity),
                rgb("channel_b", [1.0; 3]).map(|x| x * intensity),
                rgb("channel_c", [1.0; 3]).map(|x| x * intensity),
            ];
            Illum::ThreeChannel
        }
        _ => Illum::Map,
    };
    if !matches!(out.illum, Illum::Off | Illum::FromDiffuse) {
        out.illum_map = map(if out.illum == Illum::Plasma { "alpha_mask_map" } else { "self_illum_map" });
        if out.illum_map.is_none() {
            out.illum = Illum::Off;
        }
        if out.illum == Illum::MapDetail {
            out.illum_detail = map("self_illum_detail_map");
        }
    }
    out
}

/// Resolves every material's shader and decodes the colour bitmaps they
/// need. Failures are logged and leave that material flat-coloured.
pub fn load(cache: &CacheFile, scene: &Scene) -> MaterialTextures {
    load_with(cache, scene, true)
}

/// [`load`], decoding the bitmaps now (`decode`) or leaving that to
/// [`decode_bitmaps`] when a zone set needs them (`bitmaps` stays empty;
/// a bitmap that later fails to decode leaves its slot empty then).
/// Lightmaps are always decoded (the surface lights need them all).
pub fn load_with(cache: &CacheFile, scene: &Scene, decode: bool) -> MaterialTextures {
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
        let mut fetch = |slot: &TextureSlot| -> Option<u32> {
            if !decode {
                return Some(slot.bitmap);
            }
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
            let blend_map = match t.blend_map.as_ref() {
                Some(b) => Some(fetch(b)?),
                None => None,
            };
            let blend_xform = t.blend_map.as_ref().map_or(Xform::IDENTITY, |b| b.xform);
            let mut n = 0u8;
            let layers = t.layers.clone().map(|l| {
                let k = n;
                n += 1;
                let l = l?;
                let layer_map = |usage: &str| {
                    s.maps
                        .iter()
                        .find(|m| m.layer == Some(k) && m.base_usage == usage)
                        .and_then(useful_slot)
                };
                let specular = s
                    .option(&format!("material_{k}"))
                    .is_some_and(|o| o.starts_with("diffuse_plus_specular"));
                let param = |name: &str, default: f32| {
                    s.param(&format!("{name}_m_{k}")).map_or(default, |v| v[0])
                };
                Some(TerrainLayerLook {
                    base: fetch(&l.base)?,
                    base_xform: l.base.xform,
                    detail: l.detail.as_ref().and_then(|d| Some((fetch(d)?, d.xform))),
                    bump: layer_map("bump_map").and_then(|b| Some((fetch(&b)?, b.xform))),
                    specular: if specular { param("specular_coefficient", 0.0) } else { 0.0 },
                    power: param("specular_power", 10.0),
                    tint: s
                        .param(&format!("specular_tint_m_{k}"))
                        .map_or([1.0; 3], |v| [v[0], v[1], v[2]]),
                    analytical: param("analytical_specular_contribution", 0.5),
                })
            });
            layers.iter().any(Option::is_some).then_some(TerrainLook {
                blend_map,
                blend_xform,
                layers,
            })
        });
        let mut surface = surface_look(&s, &mut fetch);
        if surface.illum == Illum::Plasma
            && let Some((mask, _)) = surface.illum_map
        {
            surface.illum_mask_in_colour = mask_in_colour(cache, &resources, mask);
        }
        out.looks.push(Some(Look {
            texture,
            xform: slot.as_ref().map_or(Xform::IDENTITY, |s| s.xform),
            colour: s.albedo_color,
            blend: s.blend,
            alpha_test: s.alpha_test,
            two_sided: s.two_sided,
            // Cut-outs with no colour map are light-emitting quads tested
            // against `color_black` (shaders.md 8.6): nothing is visible.
            hidden: s.special || (s.alpha_test && s.base.is_none()) || is_game_type_marker(&s.name),
            water,
            terrain,
            surface,
        }));
    }
    for lm in &scene.lightmaps {
        match lightmap_textures(cache, &resources, lm) {
            Ok(t) => out.lightmaps.push(Some(t)),
            Err(e) => {
                eprintln!("lightmap of bsp {}: {e}; fallback lighting there", lm.bsp);
                out.lightmaps.push(None);
            }
        }
    }
    // The 90 MB local page and the shared.map pages are no longer needed.
    resources.clear_page_cache();
    out
}

/// One BSP's two lightmap arrays (`docs/format/lightmaps.md` 2.2).
pub fn lightmap_textures(
    cache: &CacheFile,
    resources: &blam_cache::Resources,
    lm: &blam_geometry::Lightmap,
) -> Result<LightmapTextures, String> {
    if cache.engine() == blam_cache::Engine::Reach {
        return reach::lightmap(cache, resources, lm);
    }
    let load = |tag: Option<u32>, layers: u32| -> Result<Texture, String> {
        let tag = tag.ok_or("no bitmap")?;
        let t = bitmap::decode(cache, resources, tag, 0).map_err(|e| e.to_string())?;
        if t.layers < layers || t.mips.is_empty() {
            return Err(format!("{}: {} layers, {} levels", t.name, t.layers, t.mips.len()));
        }
        Ok(t)
    };
    Ok(LightmapTextures {
        coefficients: load(lm.coefficients_bitmap, 8)?,
        intensity: load(lm.intensity_bitmap, 2)?,
    })
}

/// Every bitmap a material's look samples.
pub fn bitmaps_of(look: &Look) -> Vec<u32> {
    let s = &look.surface;
    let mut out: Vec<u32> = look.texture.into_iter().collect();
    if let Some(t) = &look.terrain {
        out.extend(t.blend_map);
        for l in t.layers.iter().flatten() {
            out.push(l.base);
            out.extend(l.detail.map(|d| d.0));
            out.extend(l.bump.map(|b| b.0));
        }
    }
    out.extend(
        [s.alpha_map, s.detail, s.bump, s.spec_mask_map, s.illum_map, s.illum_detail]
            .iter()
            .flatten()
            .map(|m| m.0),
    );
    out.extend(s.bump_detail.map(|b| b.0));
    out.sort_unstable();
    out.dedup();
    out
}

/// Decodes `bitmaps` (and the lightmaps `lightmaps` of `scene`) now, for
/// a zone set's BSP being built; failures are logged and left out.
pub fn decode_bitmaps(
    cache: &CacheFile,
    scene: &Scene,
    bitmaps: &[u32],
    lightmaps: &[u32],
) -> (HashMap<u32, Texture>, HashMap<u32, LightmapTextures>) {
    let Ok(resources) = cache.resources() else {
        return Default::default();
    };
    let mut out = HashMap::new();
    for &b in bitmaps {
        match bitmap::decode(cache, &resources, b, 0) {
            Ok(t) => {
                out.insert(b, t);
            }
            Err(e) => eprintln!("bitmap {b}: {e}"),
        }
    }
    let mut maps = HashMap::new();
    for &l in lightmaps {
        let Some(lm) = scene.lightmaps.get(l as usize) else {
            continue;
        };
        match lightmap_textures(cache, &resources, lm) {
            Ok(t) => {
                maps.insert(l, t);
            }
            Err(e) => eprintln!("lightmap of bsp {}: {e}", lm.bsp),
        }
    }
    resources.clear_page_cache();
    (out, maps)
}

#[cfg(test)]
mod tests {
    use super::plasma_band_weights;

    #[test]
    fn plasma_bands_average_their_coverage() {
        // The usual thinness (medium 16, wide 4, sharp 32): d^n averages
        // 2 / (n + 2), so 1/9, 1/3 and 1/17.
        let [medium, wide, sharp] = plasma_band_weights([16.0, 4.0, 32.0]);
        assert!((medium - (1.0 / 9.0 - 1.0 / 17.0)).abs() < 1e-6);
        assert!((wide - (1.0 / 3.0 - 1.0 / 9.0)).abs() < 1e-6);
        assert!((sharp - 1.0 / 17.0).abs() < 1e-6);
        // Narrows' energy wall (`plasma_wall_b`): a sharp band of thinness
        // 256 at intensity 150 adds about 1.16 times its colour, not 50.
        let [_, _, sharp] = plasma_band_weights([16.0, 4.0, 256.0]);
        assert!((sharp * 150.0 - 1.163).abs() < 0.01);
        // Never negative, whatever order the thinnesses come in.
        assert!(plasma_band_weights([4.0, 16.0, 2.0]).iter().all(|w| *w >= 0.0));
    }
}
