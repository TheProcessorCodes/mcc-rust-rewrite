//! Reach's lightmaps as the viewer's Halo 3 lightmap arrays.
//!
//! Reach bakes a "dual vMF" lightmap (`docs/format/reach.md` 10): a
//! direction bitmap (BC5) and a three-layer DXT5 intensity array per BSP.
//! Layer 0 holds the colour of the main light lobe, layer 2 a second,
//! softer one; layer 1 is a near-constant grey (the second lobe's
//! direction, as far as can be told). Each colour layer's alpha scales it
//! (the colours are near full brightness, the alphas follow the shadows).
//! How the game weighs the lobes by the surface normal and turns alpha
//! into brightness is not worked out, so this takes the light as the two
//! colours times their alphas, added, the same from every direction,
//! scales the map so its lit texels average [`TARGET_MEAN`], and writes
//! it as the SH constant term alone the way Halo 2's lightmaps are
//! (`mcc-rust-rewrite/src/halo2.rs`). The light then follows the baked shadows and
//! colours, without the normal-mapped shading.

use blam_cache::{CacheFile, Resources};
use blam_texture::{Format, Kind, Texture, bitmap, rgba};

use crate::LightmapTextures;

/// The mean light of a map's lit texels, before the viewer's exposure.
/// Checked by eye on Sword Base, Timberland and Boneyard;
/// `HALOREACH_LIGHTMAP_SCALE` overrides it.
pub const TARGET_MEAN: f32 = 0.4;

/// The brightest light written, as a multiple of the mean.
const HEADROOM: f32 = 12.0;

/// The SH constant term that gives irradiance `e` on every normal.
const C0_PER_IRRADIANCE: f32 = std::f32::consts::PI / 0.886227;

fn scale() -> f32 {
    std::env::var("HALOREACH_LIGHTMAP_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|s: &f32| s.is_finite() && *s > 0.0)
        .unwrap_or(TARGET_MEAN)
}

/// The range of c0 every Reach lightmap is written with: the scene's
/// `Lightmap::scales[0]` must be this.
pub fn range() -> f32 {
    HEADROOM * scale() * C0_PER_IRRADIANCE
}

/// One BSP's lightmap: its intensity array decoded, the two lobes added
/// and encoded as c0 (base and residual layers, `blam_geometry::lightmap::luvw`).
pub fn lightmap(cache: &CacheFile, resources: &Resources, lm: &blam_geometry::Lightmap) -> Result<LightmapTextures, String> {
    let tag = lm.intensity_bitmap.ok_or("no intensity bitmap")?;
    let t = bitmap::decode(cache, resources, tag, 0).map_err(|e| e.to_string())?;
    if t.layers < 3 || t.mips.is_empty() {
        return Err(format!("{}: {} layers, {} levels", t.name, t.layers, t.mips.len()));
    }
    let layer = |k: u32| -> Result<Vec<u8>, String> {
        let bytes = t.level(0, k).ok_or("missing layer")?;
        rgba::decode_level(t.format, t.width, t.height, bytes).map_err(|e| e.to_string())
    };
    let (main, soft) = (layer(0)?, layer(2)?);
    let range = range();
    let f = |x: u8| x as f32 / 255.0;
    let texel = |a: &[u8; 4], b: &[u8; 4], i: usize| f(a[i]) * f(a[3]) + f(b[i]) * f(b[3]);
    // The mean over texels some light reaches (the atlas's gaps are black).
    let (mut sum, mut lit) = (0.0f64, 0usize);
    for (a, b) in main.as_chunks::<4>().0.iter().zip(soft.as_chunks::<4>().0) {
        let v = (0..3).map(|i| texel(a, b, i)).fold(0.0, f32::max);
        if v > 1.0 / 255.0 {
            sum += v as f64;
            lit += 1;
        }
    }
    let mean = if lit > 0 { (sum / lit as f64) as f32 } else { 1.0 };
    let k = scale() / mean.max(1e-3) * C0_PER_IRRADIANCE;
    const MID: u8 = 128;
    let mid = MID as f32 / 255.0;
    let n = (t.width * t.height) as usize;
    let mut base = Vec::with_capacity(n * 8);
    let mut residual = Vec::with_capacity(n * 4);
    for (a, b) in main.as_chunks::<4>().0.iter().zip(soft.as_chunks::<4>().0) {
        let c = [0, 1, 2].map(|i| texel(a, b, i) * k);
        let m = c[0].max(c[1]).max(c[2]) / range;
        let qa = (m.sqrt() * 255.0).round().clamp(0.0, 255.0);
        let qb = if qa > 0.0 { (m * 255.0 * 255.0 / qa).round().clamp(0.0, 255.0) } else { 0.0 };
        let l = qa / 255.0 * qb / 255.0 * range;
        let enc = |v: f32| {
            if l > 0.0 { ((v / (2.0 * l) + 1.0 - mid) * 255.0).round().clamp(0.0, 255.0) as u8 } else { MID }
        };
        // Bytes B G R A.
        base.extend_from_slice(&[enc(c[2]), enc(c[1]), enc(c[0]), qa as u8]);
        residual.extend_from_slice(&[MID, MID, MID, qb as u8]);
    }
    base.extend_from_slice(&residual);
    let array = |name: String, width, height, layers, pixels| Texture {
        name,
        tag_index: u32::MAX,
        bitmap_index: 0,
        kind: Kind::Array,
        width,
        height,
        layers,
        format: Format::A8R8G8B8,
        srgb: false,
        curve: 0,
        mips: vec![pixels],
    };
    Ok(LightmapTextures {
        coefficients: array(format!("{} (c0)", t.name), t.width, t.height, 2, base),
        intensity: array(format!("{} (no dominant light)", t.name), 1, 1, 1, vec![MID, MID, MID, 0]),
    })
}
