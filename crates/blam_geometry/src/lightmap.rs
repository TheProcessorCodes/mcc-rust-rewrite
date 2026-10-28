//! Halo 3's baked lighting for structure BSPs: the lightmap BSP data tag
//! (`Lbsp`) records that say how each cluster and instance is lit, the
//! per-vertex lighting and light probe decoders, and the irradiance formula
//! shared by the CPU and the viewer's shader. Follows
//! `docs/format/lightmaps.md`; the two lightmap bitmaps themselves are
//! decoded by `blam_texture`.

use glam::Vec3;

use blam_cache::{Block, CacheFile};

use crate::Result;
use crate::render_geometry::GeometryResource;

/// One structure BSP's lightmap: the two DXT5 array bitmaps and the ranges
/// that scale their texels (`lightmaps.md` 2).
#[derive(Clone, Debug)]
pub struct Lightmap {
    /// The BSP's position in the scenario.
    pub bsp: u32,
    /// 8-layer array: four linear SH coefficients, base + residual each.
    pub coefficients_bitmap: Option<u32>,
    /// 2-layer array: the dominant light intensity, base + residual.
    pub intensity_bitmap: Option<u32>,
    /// Ranges of c0, c1, c2, c3 and the dominant light intensity.
    pub scales: [f32; 5],
}

/// How one placement is lit.
#[derive(Clone, Debug, Default)]
pub enum Lighting {
    /// No baked data: the renderer picks a fallback.
    #[default]
    None,
    /// Per-pixel lightmap: one atlas texcoord per mesh vertex, into
    /// `Scene::lightmaps[lightmap]` (absolute, v down, no transform).
    PerPixel { lightmap: u32, uvs: Vec<[f32; 2]> },
    /// Static per-vertex lighting: one set of coefficients per mesh vertex.
    PerVertex(Vec<ShLight>),
    /// One light probe for the whole placement.
    Probe(Probe),
}

/// Linear SH lighting at a point, as the lightmap stores it: four RGB
/// coefficients and the dominant light's intensity. Halo frame (Z up).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShLight {
    /// c0 (constant), c1 (~ -y), c2 (~ z), c3 (~ -x).
    pub c: [Vec3; 4],
    /// Dominant light intensity.
    pub dominant: Vec3,
}

/// Luminance weights the engine uses to find the dominant direction.
const LUMINANCE: Vec3 = Vec3::new(0.212656, 0.715158, 0.0721856);

impl ShLight {
    /// Direction toward the dominant light: luminance-weighted (-c3, -c1, c2).
    pub fn dominant_direction(&self) -> Vec3 {
        let c = &self.c;
        Vec3::new(
            -c[3].dot(LUMINANCE),
            -c[1].dot(LUMINANCE),
            c[2].dot(LUMINANCE),
        )
        .normalize_or_zero()
    }

    /// Diffuse irradiance for a unit normal (`lightmaps.md` 5.3).
    pub fn irradiance(&self, normal: Vec3) -> Vec3 {
        irradiance_with_dominant(&self.c, self.dominant_direction(), self.dominant, normal)
    }

    pub fn is_zero(&self) -> bool {
        self.c.iter().all(|c| c.abs().max_element() < 1e-3)
            && self.dominant.abs().max_element() < 1e-3
    }
}

/// `ravi_order_2_with_dominant_light`: linear SH with the dominant light taken
/// out and added back as a clamped directional term.
pub fn irradiance_with_dominant(c: &[Vec3; 4], dir: Vec3, dominant: Vec3, n: Vec3) -> Vec3 {
    let mut k0 = c[0];
    let mut kr = Vec3::new(c[3].x, c[1].x, -c[2].x);
    let mut kg = Vec3::new(c[3].y, c[1].y, -c[2].y);
    let mut kb = Vec3::new(c[3].z, c[1].z, -c[2].z);
    let e = Vec3::new(-0.4886025 * dir.y, -0.4886025 * dir.z, -0.4886025 * dir.x);
    let ezxy = Vec3::new(e.z, e.x, e.y);
    kr -= ezxy * dominant.x;
    kg -= ezxy * dominant.y;
    kb -= ezxy * dominant.z;
    k0 -= dominant * 0.2820948;
    let x1 = Vec3::new(n.dot(kr), n.dot(kg), n.dot(kb));
    let ambient = (k0 * 0.886227 + x1 * (-2.0 * 0.511664)) / std::f32::consts::PI;
    let direct = dominant * dir.dot(n).max(0.0) * 0.281;
    (ambient + direct).max(Vec3::ZERO)
}

/// The game's `decode_bpp16_luvw`: a base and a residual texel (RGBA, 0..1,
/// linear) and the value's range to one signed RGB value.
pub fn luvw(a: [f32; 4], b: [f32; 4], range: f32) -> Vec3 {
    let l = a[3] * b[3] * range;
    (Vec3::new(a[0] + b[0], a[1] + b[1], a[2] + b[2]) * 2.0 - Vec3::splat(2.0)) * l
}

/// One RGBA8 per-vertex lighting value: `t = 2 * byte / 255 - 1`, then
/// `rgb * exp2(a * 31.75)` (`lightmaps.md` 6.1).
pub fn per_vertex_value(p: [u8; 4]) -> Vec3 {
    let t = |x: u8| 2.0 * x as f32 / 255.0 - 1.0;
    Vec3::new(t(p[0]), t(p[1]), t(p[2])) * (t(p[3]) * 31.75).exp2()
}

/// One vertex of a static per-vertex lighting buffer (20 bytes: dominant
/// intensity, c0, c1, c2, c3).
pub fn per_vertex_light(v: &[u8; 20]) -> ShLight {
    let value = |k: usize| per_vertex_value([v[4 * k], v[4 * k + 1], v[4 * k + 2], v[4 * k + 3]]);
    ShLight {
        dominant: value(0),
        c: [value(1), value(2), value(3), value(4)],
    }
}

/// A `HalfRGBLightProbe` (72 bytes of f16): dominant direction and
/// intensity, and order-2 SH per channel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Probe {
    pub direction: Vec3,
    pub intensity: Vec3,
    /// Red, green, blue; nine coefficients each.
    pub sh: [[f32; 9]; 3],
}

impl Probe {
    pub fn parse(b: &[u8]) -> Option<Probe> {
        let h = |i: usize| -> Option<f32> {
            Some(f16(u16::from_le_bytes([
                *b.get(2 * i)?,
                *b.get(2 * i + 1)?,
            ])))
        };
        let v3 = |i: usize| -> Option<Vec3> { Some(Vec3::new(h(i)?, h(i + 1)?, h(i + 2)?)) };
        let mut sh = [[0f32; 9]; 3];
        for (ch, row) in sh.iter_mut().enumerate() {
            for (k, x) in row.iter_mut().enumerate() {
                *x = h(8 + 9 * ch + k)?;
            }
        }
        let p = Probe {
            direction: v3(0)?,
            intensity: v3(4)?,
            sh,
        };
        let finite = p.direction.is_finite()
            && p.intensity.is_finite()
            && p.sh.iter().flatten().all(|x| x.is_finite());
        finite.then_some(p)
    }

    /// The probe's L0 and L1 terms as linear SH coefficients.
    pub fn linear(&self) -> [Vec3; 4] {
        std::array::from_fn(|k| Vec3::new(self.sh[0][k], self.sh[1][k], self.sh[2][k]))
    }

    /// Diffuse irradiance: the linear part of the probe with its stored
    /// dominant light, by the same formula as the lightmap. The L2 terms are
    /// left out (an approximation; see `lightmaps.md` 8).
    pub fn irradiance(&self, normal: Vec3) -> Vec3 {
        let dir = self.direction.normalize_or_zero();
        irradiance_with_dominant(&self.linear(), dir, self.intensity, normal)
    }
}

/// IEEE half to f32.
pub fn f16(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1F) as i32;
    let m = (h & 0x3FF) as f32;
    s * match e {
        0 => m * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

/// Reads an `Lbsp` tag's header: bitmaps and scales.
pub fn read_lightmap(cache: &CacheFile, lbsp_offset: usize, bsp: u32) -> Result<Lightmap> {
    let mut scales = [0f32; 5];
    for (i, s) in scales.iter_mut().enumerate() {
        *s = cache.reader_at(lbsp_offset + 8 + 24 * i).f32()?;
    }
    let coefficients = cache.reader_at(lbsp_offset + 0xE0).tag_ref()?;
    let intensity = cache.reader_at(lbsp_offset + 0xF0).tag_ref()?;
    Ok(Lightmap {
        bsp,
        coefficients_bitmap: coefficients.index,
        intensity_bitmap: intensity.index,
        scales,
    })
}

/// Lightmap texcoords (2 x u16 / 65535) of one vertex buffer, if it has
/// exactly `count` vertices.
pub fn texcoords(
    geometry: &GeometryResource,
    buffer: u16,
    count: usize,
) -> Result<Option<Vec<[f32; 2]>>> {
    let (info, bytes) = geometry.vertex_bytes(buffer)?;
    if info.count as usize != count || bytes.len() < 4 * count {
        return Ok(None);
    }
    Ok(Some(
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .take(count)
            .map(|c| {
                [
                    u16::from_le_bytes([c[0], c[1]]) as f32 / 65535.0,
                    u16::from_le_bytes([c[2], c[3]]) as f32 / 65535.0,
                ]
            })
            .collect(),
    ))
}

/// Static per-vertex lighting of one vertex buffer, if it has exactly
/// `count` 20-byte vertices and is not blank (some buffers are all
/// `7f 7f 7f 00`, which decodes to no light at all).
pub fn per_vertex_buffer(
    geometry: &GeometryResource,
    buffer: u16,
    count: usize,
) -> Result<Option<Vec<ShLight>>> {
    let (info, bytes) = geometry.vertex_bytes(buffer)?;
    if info.count as usize != count || bytes.len() < 20 * count {
        return Ok(None);
    }
    let lights: Vec<ShLight> = bytes
        .as_chunks::<20>()
        .0
        .iter()
        .take(count)
        .map(per_vertex_light)
        .collect();
    if lights.iter().all(ShLight::is_zero) {
        return Ok(None);
    }
    Ok(Some(lights))
}

/// The lighting records of one `Lbsp`, for looking up clusters and instances.
pub struct Records {
    pub lightmap: u32,
    cluster: Block,
    instance: Block,
    per_vertex: Block,
    probes: Block,
    per_pixel: Block,
}

impl Records {
    pub fn read(cache: &CacheFile, lbsp_offset: usize, lightmap: u32) -> Result<Records> {
        let b = |o: usize| cache.reader_at(lbsp_offset + o).block();
        Ok(Records {
            lightmap,
            per_vertex: b(0x100)?,
            cluster: b(0x10C)?,
            instance: b(0x118)?,
            probes: b(0x124)?,
            per_pixel: b(0x1A0)?,
        })
    }

    /// A cluster's lighting: its section's slot-1 texcoords when the
    /// cluster record names an image.
    pub fn cluster(
        &self,
        cache: &CacheFile,
        geometry: &GeometryResource,
        cluster: u32,
        texcoord_buffer: Option<u16>,
        vertex_count: usize,
    ) -> Result<Lighting> {
        let Some(e) = self.cluster.element(cluster, 4) else {
            return Ok(Lighting::None);
        };
        let image = cache.reader_at(e).i16()?;
        let (true, Some(vb)) = (image >= 0, texcoord_buffer) else {
            return Ok(Lighting::None);
        };
        Ok(
            texcoords(geometry, vb, vertex_count)?.map_or(Lighting::None, |uvs| {
                Lighting::PerPixel {
                    lightmap: self.lightmap,
                    uvs,
                }
            }),
        )
    }

    /// An instance's lighting: per-pixel texcoords (`instance + 0x38`), else
    /// per-vertex lighting, else its probe.
    pub fn instance(
        &self,
        cache: &CacheFile,
        geometry: &GeometryResource,
        instance: u32,
        texcoord_block: i16,
        vertex_count: usize,
    ) -> Result<Lighting> {
        let Some(e) = self.instance.element(instance, 8) else {
            return Ok(Lighting::None);
        };
        let mut r = cache.reader_at(e);
        let (image, per_vertex, probe) = (r.i16()?, r.i16()?, r.i16()?);
        if image >= 0
            && texcoord_block >= 0
            && let Some(p) = self.per_pixel.element(texcoord_block as u32, 16)
        {
            let vb = cache.reader_at(p + 0xC).i16()?;
            if vb >= 0
                && let Some(uvs) = texcoords(geometry, vb as u16, vertex_count)?
            {
                return Ok(Lighting::PerPixel {
                    lightmap: self.lightmap,
                    uvs,
                });
            }
        }
        if per_vertex >= 0
            && let Some(p) = self.per_vertex.element(per_vertex as u32, 16)
        {
            let vb = cache.reader_at(p + 0xC).i32()?;
            if (0..=u16::MAX as i32).contains(&vb)
                && let Some(lights) = per_vertex_buffer(geometry, vb as u16, vertex_count)?
            {
                return Ok(Lighting::PerVertex(lights));
            }
        }
        if probe >= 0
            && let Some(p) = self.probes.element(probe as u32, 72)
            && let Some(probe) = Probe::parse(cache.reader_at(p).take(72)?)
        {
            return Ok(Lighting::Probe(probe));
        }
        Ok(Lighting::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves() {
        assert_eq!(f16(0x3C00), 1.0);
        assert_eq!(f16(0xC000), -2.0);
        assert_eq!(f16(0x0000), 0.0);
        assert!((f16(0x3555) - 0.333).abs() < 1e-3);
    }

    /// `lightmaps.md` 5.4: riverworld cluster 2 vertex 0.
    #[test]
    fn worked_example() {
        let light = ShLight {
            c: [
                Vec3::new(1.067, 0.998, 0.984),
                Vec3::new(-0.448, -0.424, -0.448),
                Vec3::new(0.929, 0.857, 0.857),
                Vec3::new(-0.894, -0.793, -0.760),
            ],
            dominant: Vec3::new(2.735, 2.493, 2.354),
        };
        let e = light.irradiance(Vec3::new(0.91, 0.39, 0.16).normalize());
        assert!(
            (e - Vec3::new(0.728, 0.667, 0.653)).abs().max_element() < 0.01,
            "{e}"
        );
        let d = light.dominant_direction();
        assert!(
            (d - Vec3::new(0.64, 0.34, 0.69)).abs().max_element() < 0.02,
            "{d}"
        );
    }

    #[test]
    fn blank_per_vertex_is_zero() {
        let mut v = [0u8; 20];
        for k in 0..5 {
            v[4 * k..4 * k + 4].copy_from_slice(&[0x7f, 0x7f, 0x7f, 0x00]);
        }
        let l = per_vertex_light(&v);
        assert!(l.is_zero(), "{l:?}");
    }
}
