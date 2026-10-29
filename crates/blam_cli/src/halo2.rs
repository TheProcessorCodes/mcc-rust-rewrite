//! `blam` on an MCC Halo 2 map (`docs/format/halo2.md`): the commands that
//! make sense there, on top of `blam_h2`.
//!
//! ```text
//! blam info     <h2 map>
//! blam classes  <h2 map>
//! blam tags     <h2 map> [--class CODE] [--grep TEXT]
//! blam strings  <h2 map> [--grep TEXT]
//! blam geometry <h2 map> [--no-objects] [--obj PATH]
//! blam rails    <h2 map> [--obj PATH]
//! blam shader   <h2 map> <shad tag>
//! blam bitmap   <h2 map> <bitm tag> [--index N] [--png PATH]
//! blam textures <h2 map>              every material's look and its bitmaps
//! ```

use std::path::Path;

use anyhow::{Context, Result, bail};
use blam_h2::{Map, Options};

use crate::{Args, usage};

pub fn run(path: &Path, args: &Args) -> Result<()> {
    let started = std::time::Instant::now();
    let map = Map::open(path).with_context(|| format!("opening {}", path.display()))?;
    match args.command.as_str() {
        "info" => info(&map, started),
        "classes" => {
            for (i, c) in map.classes.iter().enumerate() {
                let n = map.tags.iter().filter(|t| &t.class == c && t.id != u32::MAX).count();
                println!("{i:>4} {} {n:>6}", String::from_utf8_lossy(c));
            }
            Ok(())
        }
        "tags" => {
            let class = args.flag("class");
            let grep = args.flag("grep").map(str::to_ascii_lowercase);
            for (i, t) in map.tags.iter().enumerate() {
                if t.id == u32::MAX
                    || class.is_some_and(|c| t.class_str().trim() != c.trim())
                    || grep.as_ref().is_some_and(|g| !t.name.to_ascii_lowercase().contains(g))
                {
                    continue;
                }
                let place = if t.offset.is_some() { "local " } else { "shared" };
                println!("{i:>6} {:#010x} {} {place} {:>8} {}", t.id, t.class_str(), t.size, t.name);
            }
            Ok(())
        }
        "strings" => {
            let grep = args.flag("grep").map(str::to_ascii_lowercase);
            for (i, s) in map.strings.iter().enumerate() {
                if grep.as_ref().is_none_or(|g| s.to_ascii_lowercase().contains(g)) {
                    println!("{i:>6} {s}");
                }
            }
            Ok(())
        }
        "geometry" => {
            let options = Options {
                objects: !args.has("no-objects"),
                textures: false,
                verbose: true,
                ..Default::default()
            };
            let loaded = blam_h2::decode(&map, &options)?;
            eprintln!("decoded in {:.2} s", started.elapsed().as_secs_f32());
            print_stats(&loaded.stats);
            crate::geometry::report(&loaded.scene, args.flag("obj").map(Path::new))
        }
        "rails" => {
            let options = Options {
                objects: false,
                textures: false,
                ..Default::default()
            };
            let loaded = blam_h2::decode(&map, &options)?;
            crate::rails_report(&loaded.scene, started, args.flag("obj").map(Path::new))
        }
        "shader" => {
            let Some(what) = args.positional.get(1) else { usage() };
            let id = find(&map, what, b"shad")?;
            match blam_h2::shader::resolve(&map, id)? {
                Some(l) => println!("{l:#?}"),
                None => println!("not resolved"),
            }
            Ok(())
        }
        "bitmap" => {
            let Some(what) = args.positional.get(1) else { usage() };
            let id = find(&map, what, b"bitm")?;
            for (k, im) in blam_h2::bitmap::images(&map, id)?.iter().enumerate() {
                println!(
                    "image {k}: {}x{} depth {} type {} {} flags {:#x} mips {} pointer {:#x} stored {} bytes",
                    im.width,
                    im.height,
                    im.depth,
                    im.kind,
                    blam_h2::bitmap::format_name(im.format),
                    im.flags,
                    im.mipmaps,
                    im.pointer,
                    im.compressed_size
                );
            }
            let index: usize = args.flag("index").map_or(Ok(0), str::parse)?;
            let tex = blam_h2::bitmap::decode(&map, id, index)?;
            println!("decoded {} levels, {} bytes, as {}", tex.mips.len(), tex.byte_len(), tex.format.halo_name());
            if let Some(png) = args.flag("png") {
                crate::texture::write_png(&tex, Path::new(png))?;
                println!("wrote {png}");
            }
            Ok(())
        }
        "textures" => {
            let loaded = blam_h2::decode(&map, &Options { verbose: true, ..Default::default() })?;
            let name = |b: Option<(u32, blam_texture::Xform)>| match b {
                Some((b, x)) => format!(
                    "{}{} x{:.3},{:.3}",
                    map.tags.get(b as usize).map_or("?", |t| t.short_name()),
                    if loaded.bitmaps.contains_key(&b) { "" } else { " (not decoded)" },
                    x.scale[0],
                    x.scale[1]
                ),
                None => "-".into(),
            };
            for (m, look) in loaded.scene.materials.iter().zip(&loaded.looks) {
                match look {
                    Some(l) => println!(
                        "{:<55} {:<11} {:?}{}{}  pass {}\n    base {}  detail {}  secondary {}",
                        m.tag_path,
                        format!("{:?}", m.shading).to_lowercase(),
                        l.blend,
                        if l.alpha_test { " alpha-test" } else { "" },
                        if l.detail_blend { " detail-blend" } else { "" },
                        l.pass,
                        name(l.base),
                        name(l.detail),
                        name(l.secondary_detail)
                    ),
                    None => println!("{:<55} unresolved", m.tag_path),
                }
            }
            Ok(())
        }
        _ => usage(),
    }
}

fn find(map: &Map, what: &str, class: &[u8; 4]) -> Result<u32> {
    if let Ok(i) = what.parse::<usize>() {
        return map.tags.get(i).map(|t| t.id).context("no such tag index");
    }
    let wanted = what.to_ascii_lowercase();
    let hits: Vec<_> = map
        .tags
        .iter()
        .filter(|t| t.is(class) && t.id != u32::MAX)
        .filter(|t| t.name.to_ascii_lowercase() == wanted || t.short_name().eq_ignore_ascii_case(what))
        .collect();
    match hits.as_slice() {
        [t] => Ok(t.id),
        [] => bail!("no {} tag named {what}", String::from_utf8_lossy(class)),
        _ => bail!("{} {} tags named {what}; give the full path", hits.len(), String::from_utf8_lossy(class)),
    }
}

fn info(map: &Map, started: std::time::Instant) -> Result<()> {
    println!("file          {}", map.path.display());
    println!("game          Halo 2 (MCC, classic graphics)");
    println!("map name      {}", map.map_name);
    println!("scenario      {}", map.scenario_path);
    println!(
        "stored        {}; {:#x} bytes inflated in {:.2} s",
        if map.compressed { "zlib chunks" } else { "uncompressed" },
        map.bytes.len(),
        started.elapsed().as_secs_f32()
    );
    let live = map.tags.iter().filter(|t| t.id != u32::MAX).count();
    let local = map.tags.iter().filter(|t| t.offset.is_some()).count();
    println!(
        "tags          {} slots, {live} named ({local} here, {} in shared.map), {} classes",
        map.tags.len(),
        live - local,
        map.classes.len()
    );
    println!("strings       {}", map.strings.len());
    println!("tag index     {:#x}", map.index_address);
    for code in [b"scnr", b"sbsp", b"ltmp", b"sky ", b"matg"] {
        match map.first_of(code) {
            Some(t) => println!(
                "{:<13} {} ({})",
                String::from_utf8_lossy(code),
                t.name,
                if t.offset.is_some() { "here" } else { "shared.map" }
            ),
            None => println!("{:<13} -", String::from_utf8_lossy(code)),
        }
    }
    Ok(())
}

fn print_stats(s: &blam_h2::Stats) {
    eprintln!(
        "halo 2: {} clusters, {} instances of {} definitions; collision {} bsp + {} instanced triangles, {} corrupt loops",
        s.clusters, s.instances, s.instance_definitions, s.bsp_collision_triangles, s.instance_collision_triangles, s.corrupt_loops
    );
    let mut objects: Vec<_> = s.objects.iter().map(|(k, n)| format!("{n} {k:?}")).collect();
    objects.sort();
    eprintln!("  objects: {}", objects.join(", "));
    for f in &s.models_failed {
        eprintln!("  model not decoded: {f}");
    }
}
