//! `blam bitmap`, `blam shader` and `blam textures`: look at a map's
//! bitmaps and what its shaders draw with.

use std::collections::BTreeMap;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result, bail};
use blam_cache::{CacheFile, TagEntry};
use blam_texture::bitmap::{self, PageLocation, curve_name};
use blam_texture::{Shader, Texture, TextureSlot, rgba};

fn page_line(p: &PageLocation) -> String {
    format!(
        "page {} in {} +{:#x}, {} ({:#x}) bytes; page {} bytes inflated ({} stored)",
        p.page,
        p.file.as_deref().unwrap_or("this map"),
        p.offset,
        p.size,
        p.size,
        p.page_decompressed,
        p.page_compressed
    )
}

/// `blam bitmap <map> <bitm tag> [--index N] [--png PATH]`
pub fn bitmap(cache: &CacheFile, tag: &TagEntry, index: u32, png: Option<&Path>) -> Result<()> {
    let resources = cache.resources()?;
    let tag_data = bitmap::read_tag(cache, tag.index)?;
    println!(
        "#{} {}: {} bitmaps, {} resources, {} interleaved",
        tag.index,
        tag.name(),
        tag_data.bitmaps.len(),
        tag_data.resources.len(),
        tag_data.interleaved_count
    );
    let Some(h) = tag_data.bitmaps.get(index as usize) else {
        bail!("no bitmap {index}");
    };
    println!(
        "bitmap {index}: {}x{} depth {}, type {}, format {}, {} mips, curve {} ({}), flags {:#04x}",
        h.width,
        h.height,
        h.depth,
        h.kind,
        h.format,
        h.mipmap_count,
        h.curve,
        curve_name(h.curve),
        h.flags
    );
    let located = match bitmap::locate(&resources, &tag_data, index) {
        Ok(l) => l,
        Err(e) => {
            println!("no pixels: {e}");
            return Ok(());
        }
    };
    let d = &located.definition;
    println!(
        "resource {:#010x} -> entry {}, segment {}; definition: primary {:#x}, secondary {:#x}, {} levels, DXGI {}",
        located.resource.0,
        located.entry,
        located.segment,
        d.primary_size,
        d.secondary_size,
        d.levels,
        d.dxgi
    );
    println!("  primary   {}", page_line(&located.primary));
    if let Some(s) = &located.secondary {
        println!("  secondary {}  (mip 0)", page_line(s));
    }

    let started = std::time::Instant::now();
    let tex = bitmap::decode_from(&resources, &tag_data, index)?;
    println!(
        "decoded in {:.0} ms: {:?} {} {}x{} x{} layers, {}, {} levels, {} bytes",
        started.elapsed().as_secs_f64() * 1000.0,
        tex.kind,
        tex.format.halo_name(),
        tex.width,
        tex.height,
        tex.layers,
        if tex.srgb { "sRGB" } else { "linear" },
        tex.mips.len(),
        tex.byte_len()
    );
    for (l, m) in tex.mips.iter().enumerate() {
        let (w, hh) = tex.level_dims(l as u32);
        let head: String = m
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        println!("  mip {l:>2}: {w:>4}x{hh:<4} {:>8} bytes  {head}", m.len());
    }
    if let Some(level) = tex.level(0, 0)
        && let Ok(px) = rgba::decode_level(tex.format, tex.width, tex.height, level)
    {
        let m = rgba::channel_means(&px);
        println!(
            "mip 0 mean RGBA (decoded): {:.3} {:.3} {:.3} {:.3}",
            m[0], m[1], m[2], m[3]
        );
    }
    if let Some(path) = png {
        write_png(&tex, path)?;
        println!("wrote mip 0 (layer 0) to {}", path.display());
        if tex.layers > 1 {
            let strip = path.with_extension("layers.png");
            write_layers_png(&tex, &strip)?;
            println!("wrote mip 0 of all {} layers side by side, opaque, to {}", tex.layers, strip.display());
        }
    }
    Ok(())
}

/// Writes level 0 of every layer (cube faces, array slices) side by side,
/// alpha forced opaque so colour stored under a small alpha stays visible;
/// prints each layer's mean RGBA.
pub fn write_layers_png(tex: &Texture, path: &Path) -> Result<()> {
    let (w, h) = (tex.width as usize, tex.height as usize);
    let n = tex.layers as usize;
    let mut out = vec![0u8; w * n * h * 4];
    for l in 0..n {
        let level = tex.level(0, l as u32).context("short layer")?;
        let px = rgba::decode_level(tex.format, tex.width, tex.height, level)?;
        let m = rgba::channel_means(&px);
        println!("  layer {l}: mean RGBA {:.3} {:.3} {:.3} {:.3}", m[0], m[1], m[2], m[3]);
        for y in 0..h {
            for x in 0..w {
                let s = 4 * (y * w + x);
                let d = 4 * (y * w * n + l * w + x);
                out[d..d + 3].copy_from_slice(&px[s..s + 3]);
                out[d + 3] = 255;
            }
        }
    }
    let file =
        std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut enc = png::Encoder::new(BufWriter::new(file), (w * n) as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&out)?;
    Ok(())
}

/// Writes level 0 of layer 0 as an RGBA PNG.
pub fn write_png(tex: &Texture, path: &Path) -> Result<()> {
    let level = tex.level(0, 0).context("no mip 0")?;
    let pixels = rgba::decode_level(tex.format, tex.width, tex.height, level)?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let file =
        std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut enc = png::Encoder::new(BufWriter::new(file), tex.width, tex.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&pixels)?;
    Ok(())
}

fn slot_line(s: &TextureSlot) -> String {
    format!(
        "#{} {} (scale {} {}, offset {} {})",
        s.bitmap, s.name, s.xform.scale[0], s.xform.scale[1], s.xform.offset[0], s.xform.offset[1]
    )
}

pub fn print_shader(s: &Shader) {
    println!("#{} {} {}", s.tag_index, s.class, s.name);
    println!("definition {}", s.definition);
    println!("options:");
    for (c, o) in &s.options {
        println!("  {c:<22} {o}");
    }
    println!("maps:");
    for m in &s.maps {
        println!(
            "  {:<24} {:<70} scale {} {} offset {} {}",
            m.usage,
            if m.bitmap_name.is_empty() {
                "(none)"
            } else {
                m.bitmap_name.as_str()
            },
            m.xform.scale[0],
            m.xform.scale[1],
            m.xform.offset[0],
            m.xform.offset[1]
        );
    }
    println!("params:");
    for (n, v) in &s.params {
        println!("  {n:<36} {:?}", v);
    }
    if !s.animations.is_empty() {
        println!("animations:");
        for a in &s.animations {
            println!(
                "  {:<28} {:?} period {} s, {} -> {} ({:.4}/s)",
                a.argument,
                a.kind,
                a.period,
                a.from,
                a.to,
                a.rate()
            );
        }
    }
    println!(
        "=> blend {:?}, alpha test {}, two-sided {}, special {}",
        s.blend, s.alpha_test, s.two_sided, s.special
    );
    match &s.base {
        Some(b) => println!("=> base colour {}", slot_line(b)),
        None => println!("=> no base colour map"),
    }
    if let Some(a) = &s.alpha_map {
        println!("=> alpha from {}", slot_line(a));
    }
    if let Some(c) = s.albedo_color {
        println!("=> colour {c:?}");
    }
    if let Some(t) = &s.terrain {
        match &t.blend_map {
            Some(b) => println!("=> terrain blend map {}", slot_line(b)),
            None => println!("=> terrain without a blend map"),
        }
        for (n, l) in t.layers.iter().enumerate() {
            match l {
                Some(l) => println!(
                    "   layer {n} ({}): {}",
                    ["red", "green", "blue", "alpha"][n],
                    slot_line(&l.base)
                ),
                None => println!("   layer {n}: off"),
            }
        }
    }
}

/// `blam shader <map> <shader tag>`
pub fn shader(cache: &CacheFile, tag: &TagEntry) -> Result<()> {
    let s = blam_texture::shader::resolve(cache, tag.index)?;
    print_shader(&s);
    if let Some(t) = &s.terrain
        && let Some(b) = &t.blend_map
    {
        let resources = cache.resources()?;
        let tex = bitmap::decode(cache, &resources, b.bitmap, 0)?;
        let px = rgba::decode_level(
            tex.format,
            tex.width,
            tex.height,
            tex.level(0, 0).context("mip 0")?,
        )?;
        let w = rgba::channel_means(&px);
        println!(
            "   blend map mean weights r {:.4} g {:.4} b {:.4} a {:.4}",
            w[0], w[1], w[2], w[3]
        );
    }
    Ok(())
}

/// `blam textures <map> [--png-dir DIR]`: decode every base texture the
/// map's drawn materials use and report counts, bytes and time.
pub fn textures(cache: &CacheFile, png_dir: Option<&Path>) -> Result<()> {
    let started = std::time::Instant::now();
    let mut scene = blam_geometry::Scene::default();
    let options = blam_geometry::Options {
        collision: false,
        ..Default::default()
    };
    blam_geometry::decode_into(cache, &options, &mut scene)?;
    let geometry = started.elapsed().as_secs_f64();

    let resources = cache.resources()?;
    let mut wanted: BTreeMap<u32, String> = BTreeMap::new();
    let mut shaders = 0;
    let mut failed_shaders = 0;
    let mut no_base = 0;
    let mut hidden = Vec::new();
    for m in &scene.materials {
        let Some(tag) = m.shader_tag else { continue };
        match blam_texture::shader::resolve(cache, tag) {
            Ok(s) => {
                shaders += 1;
                if s.special || (s.alpha_test && s.base.is_none()) {
                    hidden.push(format!("{} {}", s.class, s.name));
                }
                let mut slots: Vec<&TextureSlot> = s.base.iter().collect();
                if let Some(t) = &s.terrain {
                    slots.extend(t.blend_map.iter());
                    slots.extend(t.layers.iter().flatten().map(|l| &l.base));
                }
                if slots.is_empty() {
                    no_base += 1;
                }
                for sl in slots {
                    wanted.insert(sl.bitmap, sl.name.clone());
                }
            }
            Err(e) => {
                failed_shaders += 1;
                eprintln!("shader {}: {e}", m.tag_path);
            }
        }
    }
    let resolved = started.elapsed().as_secs_f64();

    let mut bytes = 0usize;
    let mut ok = 0;
    let mut by_format: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (&tag, name) in &wanted {
        match bitmap::decode(cache, &resources, tag, 0) {
            Ok(t) => {
                ok += 1;
                bytes += t.byte_len();
                let e = by_format
                    .entry(t.format.halo_name().to_string())
                    .or_default();
                e.0 += 1;
                e.1 += t.byte_len();
                if let Some(dir) = png_dir {
                    let file = name.rsplit('\\').next().unwrap_or(name);
                    write_png(&t, &dir.join(format!("{file}-{tag}.png")))?;
                }
            }
            Err(e) => eprintln!("bitmap #{tag} {name}: {e}"),
        }
    }
    let decoded = started.elapsed().as_secs_f64();
    println!(
        "{}: {} materials, {shaders} shaders resolved ({failed_shaders} failed, {no_base} without a colour map)",
        cache.header.map_name,
        scene.materials.len()
    );
    println!(
        "not drawn by the viewer (special, or cut out with no colour map): {}",
        hidden.len()
    );
    for h in &hidden {
        println!("  {h}");
    }
    println!(
        "{} distinct colour/terrain bitmaps, {ok} decoded, {:.1} MiB of texel data",
        wanted.len(),
        bytes as f64 / (1 << 20) as f64
    );
    for (f, (n, b)) in &by_format {
        println!(
            "  {f:<18} {n:>4} bitmaps {:>8.1} MiB",
            *b as f64 / (1 << 20) as f64
        );
    }
    println!(
        "time: geometry {geometry:.2} s, shaders {:.2} s, bitmaps {:.2} s",
        resolved - geometry,
        decoded - resolved
    );
    Ok(())
}
