//! `blam lightmap <map> [--bsp N] [--samples N]`: print what a structure BSP's
//! lightmap BSP data tag (`Lbsp`) holds, how its meshes find their lightmap
//! texcoords, and decode a few texels with the per-pixel lightmap formula.
//! See `docs/format/lightmaps.md`.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use blam_cache::{Block, CacheFile, ResourceId};
use blam_geometry::render_geometry::{self, GeometryResource};
use blam_geometry::lightmap::{ShLight, luvw};
use blam_geometry::sbsp;
use blam_texture::{bitmap, rgba};
use glam::Vec3;

/// Min uv, max uv and vertex count of one lightmap texcoord buffer.
type UvRange = ([f32; 2], [f32; 2], u32);

/// One vertex buffer's declaration in the render geometry resource definition.
#[derive(Clone, Copy)]
struct VbDecl {
    count: i32,
    kind: i16,
    stride: i16,
}

fn i16_at(b: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([b[o], b[o + 1]])
}
fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// A decoded 1024x1024 DXT5 array: RGBA8 per layer.
struct Layers {
    width: u32,
    height: u32,
    rgba: Vec<Vec<u8>>,
}

impl Layers {
    fn load(cache: &CacheFile, tag: u32) -> Result<Layers> {
        let resources = cache.resources()?;
        let tex = bitmap::decode(cache, &resources, tag, 0)?;
        let rgba = (0..tex.layers)
            .map(|l| {
                let level = tex.level(0, l).context("layer")?;
                Ok(rgba::decode_level(
                    tex.format, tex.width, tex.height, level,
                )?)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Layers {
            width: tex.width,
            height: tex.height,
            rgba,
        })
    }

    /// Nearest texel of layer `l` at normalised `uv`, as 0..1 floats.
    fn texel(&self, l: usize, uv: [f32; 2]) -> [f32; 4] {
        let x = ((uv[0] * self.width as f32) as i64).clamp(0, self.width as i64 - 1) as usize;
        let y = ((uv[1] * self.height as f32) as i64).clamp(0, self.height as i64 - 1) as usize;
        let o = 4 * (y * self.width as usize + x);
        let p = &self.rgba[l][o..o + 4];
        [
            p[0] as f32 / 255.0,
            p[1] as f32 / 255.0,
            p[2] as f32 / 255.0,
            p[3] as f32 / 255.0,
        ]
    }
}

/// Per-pixel lighting at one texel: four linear SH coefficients and the
/// dominant light intensity, then the diffuse irradiance for `normal`
/// (`blam_geometry::lightmap`, the same code the viewer uses).
fn irradiance(
    sh: &Layers,
    dom: &Layers,
    scale: &[f32; 5],
    uv: [f32; 2],
    normal: Vec3,
) -> (Vec3, [Vec3; 4], Vec3) {
    let light = ShLight {
        c: std::array::from_fn(|i| luvw(sh.texel(2 * i, uv), sh.texel(2 * i + 1, uv), scale[i])),
        dominant: luvw(dom.texel(0, uv), dom.texel(1, uv), scale[4]),
    };
    (light.irradiance(normal), light.c, light.dominant)
}

pub fn run(cache: &CacheFile, bsp_index: u32, samples: usize) -> Result<()> {
    let bsps = sbsp::structure_bsps(cache)?;
    let Some(bsp) = bsps.iter().find(|b| b.index == bsp_index) else {
        bail!("no BSP {bsp_index} (map has {})", bsps.len());
    };
    let Some(lbsp) = bsp.lightmap_bsp else {
        bail!("BSP {bsp_index} has no Lbsp tag")
    };
    let lb = cache.tag_offset(lbsp)?;
    let sb = cache.tag_offset(bsp.sbsp)?;
    let r = |o: usize| cache.reader_at(lb + o);
    println!(
        "#{} Lbsp {}  (sbsp #{} {})",
        lbsp.index,
        lbsp.name(),
        bsp.sbsp.index,
        bsp.sbsp.name()
    );
    println!(
        "  +0x00 flags {:#06x}  +0x02 bsp index {}  +0x04 checksum {:#010x}",
        r(0).u16()?,
        r(2).i16()?,
        r(4).u32()?
    );
    let mut scale = [0f32; 5];
    for i in 0..9 {
        let mut rr = r(8 + 24 * i);
        let v: Vec<f32> = (0..6)
            .map(|_| rr.f32())
            .collect::<blam_cache::Result<_>>()?;
        if let Some(s) = scale.get_mut(i) {
            *s = v[0];
        }
        println!("  +{:#05x} coefficient map scale [{i}] {:?}", 8 + 24 * i, v);
    }
    let mut bitmaps = [None, None];
    for (k, (o, what)) in [
        (0xE0usize, "SH coefficients"),
        (0xF0, "dominant light intensity"),
    ]
    .iter()
    .enumerate()
    {
        let t = r(*o).tag_ref()?;
        bitmaps[k] = t.index;
        let name = t
            .index
            .and_then(|i| cache.tags.get(i))
            .map_or("(none)", |t| t.name());
        println!("  +{o:#05x} {what} bitmap: {:?} {name}", t.index);
    }
    let blocks: Vec<(usize, &str)> = vec![
        (0x100, "static per-vertex lighting buffers (16 B)"),
        (0x10C, "cluster static per-vertex lighting (4 B)"),
        (0x118, "instanced geometry lighting (8 B)"),
        (0x124, "instanced geometry light probes (72 B)"),
        (0x134, "render geometry: meshes (76 B)"),
        (
            0x1A0,
            "render geometry: instanced geometry per-pixel lighting (16 B)",
        ),
        (0x1B4, "airprobes (92 B)"),
        (0x1C0, "scenery light probes (80 B)"),
        (0x1CC, "machine light probes (44 B)"),
        (0x1D8, "unused (4 B)"),
    ];
    for (o, what) in &blocks {
        println!("  +{o:#05x} {:>5} x {what}", r(*o).block()?.count);
    }
    println!("  +0x1ac geometry resource {:#010x}", r(0x1AC).u32()?);

    // Vertex buffer declarations: 28 bytes each at the start of the definition.
    let resources = cache.resources()?;
    let rid = ResourceId(r(0x1AC).u32()?);
    let entry = resources.entry(rid)?;
    let def = resources.definition(&entry)?;
    let vb_count = i32_at(def, def.len() - 24).max(0) as usize;
    let decls: Vec<VbDecl> = (0..vb_count)
        .map(|i| VbDecl {
            count: i32_at(def, 28 * i),
            kind: i16_at(def, 28 * i + 4),
            stride: i16_at(def, 28 * i + 6),
        })
        .collect();
    let mut kinds: BTreeMap<(i16, i16), (u32, i64)> = BTreeMap::new();
    for d in &decls {
        let e = kinds.entry((d.kind, d.stride)).or_default();
        e.0 += 1;
        e.1 += d.count as i64;
    }
    println!("\nvertex buffers: {vb_count}");
    for ((k, s), (n, v)) in &kinds {
        println!("  declared type {k:>2} stride {s:>3}: {n:>4} buffers, {v:>7} vertices");
    }
    let geometry = GeometryResource::load(&resources, rid)?;

    // Sections: which slots of the 8 vertex buffer indices are used, and by what.
    let meshes: Block = r(0x134).block()?;
    let mut slots: BTreeMap<(usize, i16), u32> = BTreeMap::new();
    let mut section_vbs = Vec::new();
    for o in meshes.elements(76) {
        let vbs: Vec<i16> = (0..8)
            .map(|k| cache.reader_at(o + 0x18 + 2 * k).i16())
            .collect::<blam_cache::Result<_>>()?;
        for (k, &v) in vbs.iter().enumerate() {
            if v >= 0 {
                *slots
                    .entry((k, decls.get(v as usize).map_or(-1, |d| d.kind)))
                    .or_default() += 1;
            }
        }
        section_vbs.push(vbs);
    }
    println!("\nsection vertex buffer slots in use (slot, declared type): count");
    for ((k, t), n) in &slots {
        println!("  slot {k} type {t:>2}: {n}");
    }

    let uv_range = |vb: i16| -> Result<Option<UvRange>> {
        if vb < 0 {
            return Ok(None);
        }
        let (info, bytes) = geometry.vertex_bytes(vb as u16)?;
        let mut lo = [f32::MAX; 2];
        let mut hi = [f32::MIN; 2];
        for c in bytes.as_chunks::<4>().0.iter().take(info.count as usize) {
            for k in 0..2 {
                let v = u16::from_le_bytes([c[2 * k], c[2 * k + 1]]) as f32 / 65535.0;
                lo[k] = lo[k].min(v);
                hi[k] = hi[k].max(v);
            }
        }
        Ok(Some((lo, hi, info.count)))
    };
    let uvs_of = |vb: i16| -> Result<Vec<[f32; 2]>> {
        let (info, bytes) = geometry.vertex_bytes(vb as u16)?;
        Ok(bytes
            .as_chunks::<4>()
            .0
            .iter()
            .take(info.count as usize)
            .map(|c| {
                [
                    u16::from_le_bytes([c[0], c[1]]) as f32 / 65535.0,
                    u16::from_le_bytes([c[2], c[3]]) as f32 / 65535.0,
                ]
            })
            .collect())
    };

    // Clusters.
    let clusters: Block = cache.reader_at(sb + 0xC4).block()?;
    let cluster_light: Block = r(0x10C).block()?;
    println!(
        "\nclusters: {} (Lbsp cluster lighting entries {})",
        clusters.count, cluster_light.count
    );
    let mut cluster_rows = Vec::new();
    for (c, o) in clusters.elements(280).enumerate() {
        let section = cache.reader_at(o + 216).i16()?;
        let (img, spv) = match cluster_light.element(c as u32, 4) {
            Some(e) => (cache.reader_at(e).i16()?, cache.reader_at(e + 2).i16()?),
            None => (-9, -9),
        };
        let vbs = usize::try_from(section)
            .ok()
            .and_then(|s| section_vbs.get(s))
            .cloned()
            .unwrap_or_default();
        let vb0 = vbs.first().copied().unwrap_or(-1);
        let vb1 = vbs.get(1).copied().unwrap_or(-1);
        let n0 = decls.get(vb0.max(0) as usize).map_or(0, |d| d.count);
        let range = uv_range(vb1)?;
        if c < 6 || range.is_none() || range.is_some_and(|r| r.2 as i32 != n0) {
            println!(
                "  cluster {c:>3}: section {section:>3} vb {vbs:?}  image {img} spv {spv}  world verts {n0} lightmap uv {:?}",
                range
            );
        }
        cluster_rows.push((c, section, vb0, vb1, img, spv));
    }

    // Instances.
    let instances: Block = cache.reader_at(sb + 0x1C0).block()?;
    let inst_light: Block = r(0x118).block()?;
    let per_pixel: Block = r(0x1A0).block()?;
    let spv: Block = r(0x100).block()?;
    let mut policies: BTreeMap<(i16, bool, bool, bool, bool), u32> = BTreeMap::new();
    let mut inst_rows = Vec::new();
    let mut shown = 0;
    println!(
        "\ninstances: {} (Lbsp instance lighting entries {})",
        instances.count, inst_light.count
    );
    for (i, o) in instances.elements(120).enumerate() {
        let def_index = cache.reader_at(o + 0x34).i16()?;
        let lm_tc = cache.reader_at(o + 0x38).i16()?;
        let policy = cache.reader_at(o + 0x5A).i16()?;
        let e = inst_light
            .element(i as u32, 8)
            .context("instance lighting")?;
        let (img, spv_i, probe) = (
            cache.reader_at(e).i16()?,
            cache.reader_at(e + 2).i16()?,
            cache.reader_at(e + 4).i16()?,
        );
        let pp_vb = match per_pixel.element(lm_tc.max(0) as u32, 16) {
            Some(p) if lm_tc >= 0 => cache.reader_at(p + 12).i16()?,
            _ => -1,
        };
        let spv_vb = match spv.element(spv_i.max(0) as u32, 16) {
            Some(p) if spv_i >= 0 => cache.reader_at(p + 12).i32()?,
            _ => -1,
        };
        *policies
            .entry((policy, lm_tc >= 0, img >= 0, spv_i >= 0, probe >= 0))
            .or_default() += 1;
        // Mesh of the definition is the section with the same index on these maps.
        let mesh_vb0 = section_vbs
            .get(def_index.max(0) as usize)
            .map_or(-1, |v| v[0]);
        let n0 = decls.get(mesh_vb0.max(0) as usize).map_or(0, |d| d.count);
        let pp_decl = decls.get(pp_vb.max(0) as usize).filter(|_| pp_vb >= 0);
        let spv_decl = decls.get(spv_vb.max(0) as usize).filter(|_| spv_vb >= 0);
        let mismatch =
            pp_decl.is_some_and(|d| d.count != n0) || spv_decl.is_some_and(|d| d.count != n0);
        if shown < 8 || mismatch {
            shown += 1;
            println!(
                "  instance {i:>3}: def {def_index:>3} policy {policy} lm texcoord block {lm_tc:>4} -> vb {pp_vb:>4} {}  image {img} spv {spv_i:>3} -> vb {spv_vb:>4} {}  probe {probe:>3}  mesh verts {n0}{}",
                pp_decl.map_or(String::new(), |d| format!(
                    "(type {} stride {} n {})",
                    d.kind, d.stride, d.count
                )),
                spv_decl.map_or(String::new(), |d| format!(
                    "(type {} stride {} n {})",
                    d.kind, d.stride, d.count
                )),
                if mismatch { "  COUNT MISMATCH" } else { "" }
            );
        }
        inst_rows.push((i, def_index, pp_vb, img, spv_vb));
    }
    println!("  (policy, has lm texcoord, has image, has per-vertex, has probe): count");
    for (k, n) in &policies {
        println!("    {k:?}: {n}");
    }

    // Per-pixel UV range over all per-pixel buffers.
    let mut lo = [f32::MAX; 2];
    let mut hi = [f32::MIN; 2];
    for vb in cluster_rows
        .iter()
        .map(|r| r.3)
        .chain(inst_rows.iter().map(|r| r.2))
    {
        if let Some((a, b, _)) = uv_range(vb)? {
            for k in 0..2 {
                lo[k] = lo[k].min(a[k]);
                hi[k] = hi[k].max(b[k]);
            }
        }
    }
    println!(
        "\nall lightmap texcoords (ushort2 / 65535): u {:.4}..{:.4}  v {:.4}..{:.4}",
        lo[0], hi[0], lo[1], hi[1]
    );

    // Static per-vertex buffers: raw first vertex, and the mean of the five
    // decoded RGBE terms over every vertex of every buffer, for both byte orders.
    let mut sums = [[Vec3::ZERO; 5]; 2];
    let mut n_spv = 0u32;
    for (k, o) in spv.elements(16).enumerate() {
        let vb = cache.reader_at(o + 12).i32()?;
        let d = decls[vb as usize];
        let (info, bytes) = geometry.vertex_bytes(vb as u16)?;
        if k < 3 {
            let head: Vec<String> = bytes
                .iter()
                .take(d.stride.max(0) as usize)
                .map(|b| format!("{b:02x}"))
                .collect();
            println!(
                "static per-vertex buffer {k}: vb {vb} type {} stride {} n {}  first vertex {}",
                d.kind,
                d.stride,
                d.count,
                head.join(" ")
            );
        }
        for v in bytes.as_chunks::<20>().0.iter().take(info.count as usize) {
            n_spv += 1;
            for t in 0..5 {
                let p = &v[4 * t..4 * t + 4];
                let f = |x: u8| 2.0 * x as f32 / 255.0 - 1.0;
                // order 0: bytes are R G B A; order 1: bytes are B G R A.
                let e = (f(p[3]) * 31.75).exp2();
                sums[0][t] += Vec3::new(f(p[0]), f(p[1]), f(p[2])) * e;
                sums[1][t] += Vec3::new(f(p[2]), f(p[1]), f(p[0])) * e;
            }
        }
    }
    if n_spv > 0 {
        for (order, name) in [(0, "RGBA"), (1, "BGRA")] {
            let m: Vec<[f32; 3]> = sums[order]
                .iter()
                .map(|s| (*s / n_spv as f32).to_array())
                .collect();
            println!(
                "per-vertex mean over {n_spv} vertices as {name}: intensity {:.3?} c0 {:.3?} c1 {:.3?} c2 {:.3?} c3 {:.3?}",
                m[0], m[1], m[2], m[3], m[4]
            );
        }
    }

    // Half-float light probes (72 bytes: dir 3+pad, intensity 3+pad, SH r[9] g[9] b[9], pad).
    let half = blam_geometry::lightmap::f16;
    let probe_line = |o: usize| -> Result<String> {
        let h: Vec<f32> = (0..36)
            .map(|k| cache.reader_at(o + 2 * k).u16().map(half))
            .collect::<blam_cache::Result<_>>()?;
        Ok(format!(
            "dir {:.3?} intensity {:.3?} SH r {:.3?} g[0] {:.3} b[0] {:.3}",
            &h[0..3],
            &h[4..7],
            &h[8..17],
            h[17],
            h[26]
        ))
    };
    if let Some(o) = r(0x124).block()?.element(0, 72) {
        println!("instanced geometry light probe 0: {}", probe_line(o)?);
    }
    if let Some(o) = r(0x1C0).block()?.element(0, 80) {
        let mut rr = cache.reader_at(o);
        println!(
            "scenery light probe 0: object id {:#010x} {:#010x}; {}",
            rr.u32()?,
            rr.u32()?,
            probe_line(o + 8)?
        );
    }

    // Decode texels at a few cluster vertices with the game's formula.
    let (Some(sh_tag), Some(dom_tag)) = (bitmaps[0], bitmaps[1]) else {
        return Ok(());
    };
    let sh = Layers::load(cache, sh_tag)?;
    let dom = Layers::load(cache, dom_tag)?;
    println!(
        "\nSH array {} layers, intensity array {} layers, {}x{}",
        sh.rgba.len(),
        dom.rgba.len(),
        sh.width,
        sh.height
    );
    // Orientation check: how many per-pixel texcoords land on an empty texel
    // (layer 0 all zero) as stored, with v flipped, and with u/v swapped.
    let mut empty = [0u32; 3];
    let mut total_uv = 0u32;
    for vb in cluster_rows
        .iter()
        .map(|r| r.3)
        .chain(inst_rows.iter().map(|r| r.2))
        .filter(|&v| v >= 0)
    {
        for uv in uvs_of(vb)? {
            total_uv += 1;
            for (k, t) in [uv, [uv[0], 1.0 - uv[1]], [uv[1], uv[0]]]
                .iter()
                .enumerate()
            {
                if sh.texel(0, *t) == [0.0; 4] {
                    empty[k] += 1;
                }
            }
        }
    }
    println!(
        "texcoords on an empty layer-0 texel out of {total_uv}: as stored {}, v flipped {}, u/v swapped {}",
        empty[0], empty[1], empty[2]
    );
    let used = sh.rgba[0]
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| **p != [0, 0, 0, 0])
        .count();
    println!(
        "non-empty layer-0 texels: {used} of {}",
        sh.width * sh.height
    );
    let sections = render_geometry::read_sections(cache, meshes)?;
    let bounds = render_geometry::read_bounds(cache, cache.reader_at(sb + 0x300).block()?)?;
    let mut total = Vec3::ZERO;
    let mut count = 0u32;
    let mut printed = 0;
    for &(c, section, _, vb1, _, _) in &cluster_rows {
        if vb1 < 0 || section < 0 {
            continue;
        }
        let s = section as usize;
        let Some(dec) = render_geometry::decode_section(&sections[s], &geometry, bounds.get(s))?
        else {
            continue;
        };
        let uvs = uvs_of(vb1)?;
        let step = (uvs.len() / samples.max(1)).max(1);
        for v in (0..uvs.len().min(dec.vertices.normals.len())).step_by(step) {
            let n = dec.vertices.normals[v];
            let (e, coeffs, dl) = irradiance(&sh, &dom, &scale, uvs[v], n);
            total += e;
            count += 1;
            if printed < samples {
                printed += 1;
                println!(
                    "  cluster {c} v{v}: pos {:.1?} n {:.2?} uv {:.4?} -> texel ({}, {})  c0 {:.3?} c1 {:.3?} c2 {:.3?} c3 {:.3?} dominant {:.3?}  diffuse E {:.3?}",
                    dec.vertices.positions[v].to_array(),
                    n.to_array(),
                    uvs[v],
                    (uvs[v][0] * sh.width as f32) as u32,
                    (uvs[v][1] * sh.height as f32) as u32,
                    coeffs[0].to_array(),
                    coeffs[1].to_array(),
                    coeffs[2].to_array(),
                    coeffs[3].to_array(),
                    dl.to_array(),
                    e.to_array()
                );
            }
        }
    }
    if count > 0 {
        println!(
            "mean diffuse irradiance over {count} sampled cluster vertices: {:.3?}",
            (total / count as f32).to_array()
        );
    }

    // Byte-order check for the per-vertex buffers: compare the colour of the
    // per-vertex c0 term with the per-pixel c0 of the nearest cluster vertex.
    let mut grid: std::collections::HashMap<(i32, i32, i32), Vec<(Vec3, Vec3)>> =
        Default::default();
    let cell = |p: Vec3| {
        (
            (p.x / 2.0).floor() as i32,
            (p.y / 2.0).floor() as i32,
            (p.z / 2.0).floor() as i32,
        )
    };
    for &(_, section, _, vb1, _, _) in &cluster_rows {
        if vb1 < 0 || section < 0 {
            continue;
        }
        let s = section as usize;
        let Some(dec) = render_geometry::decode_section(&sections[s], &geometry, bounds.get(s))?
        else {
            continue;
        };
        for (p, uv) in dec.vertices.positions.iter().zip(uvs_of(vb1)?) {
            let c0 = luvw(sh.texel(0, uv), sh.texel(1, uv), scale[0]);
            grid.entry(cell(*p)).or_default().push((*p, c0));
        }
    }
    // Chroma agreement: sum over pairs of dot(normalised chroma) for each order.
    let chroma = |c: Vec3| {
        let m = (c.x + c.y + c.z) / 3.0;
        let d = c - Vec3::splat(m);
        if m > 0.02 { d / m } else { Vec3::ZERO }
    };
    let mut agree = [0f32; 2];
    let mut pairs = 0u32;
    for (i, o) in instances.elements(120).enumerate() {
        let (_, def_index, _, _, spv_vb) = inst_rows[i];
        if spv_vb < 0 || def_index < 0 {
            continue;
        }
        let s = def_index as usize;
        let Some(dec) = render_geometry::decode_section(&sections[s], &geometry, bounds.get(s))?
        else {
            continue;
        };
        let mut rr = cache.reader_at(o);
        let sc = rr.f32()?;
        let (f, l, u, p) = (
            Vec3::from(rr.f32x3()?),
            Vec3::from(rr.f32x3()?),
            Vec3::from(rr.f32x3()?),
            Vec3::from(rr.f32x3()?),
        );
        let (info, bytes) = geometry.vertex_bytes(spv_vb as u16)?;
        for (v, raw) in bytes
            .as_chunks::<20>()
            .0
            .iter()
            .take(info.count as usize)
            .enumerate()
        {
            let Some(local) = dec.vertices.positions.get(v) else {
                break;
            };
            let w = p + (f * local.x + l * local.y + u * local.z) * sc;
            let (cx, cy, cz) = cell(w);
            let mut best: Option<(f32, Vec3)> = None;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        for (q, c0) in grid
                            .get(&(cx + dx, cy + dy, cz + dz))
                            .map_or(&[][..], |v| v)
                        {
                            let d = q.distance(w);
                            if d < 1.0 && best.is_none_or(|b| d < b.0) {
                                best = Some((d, *c0));
                            }
                        }
                    }
                }
            }
            let Some((_, pix)) = best else { continue };
            let t = &raw[4..8];
            let fb = |x: u8| 2.0 * x as f32 / 255.0 - 1.0;
            let rgba = Vec3::new(fb(t[0]), fb(t[1]), fb(t[2]));
            let bgra = Vec3::new(fb(t[2]), fb(t[1]), fb(t[0]));
            let cp = chroma(pix);
            agree[0] += chroma(rgba).dot(cp);
            agree[1] += chroma(bgra).dot(cp);
            pairs += 1;
        }
    }
    println!(
        "per-vertex vs nearest per-pixel c0 chroma agreement over {pairs} pairs: as RGBA {:.4}, as BGRA {:.4} (higher wins)",
        agree[0], agree[1]
    );
    Ok(())
}
