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
//! blam ambience <h2 map> [--wav DIR]  background sounds and sound scenery, decoded
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
        "ambience" => ambience(&map, args.flag("wav").map(Path::new)),
        _ => usage(),
    }
}

/// The map's ambience: background sounds, sound scenery, their tracks and
/// details, each permutation decoded (length, RMS). `--wav DIR` writes each
/// permutation and a mix of the backgrounds' loop tracks, permutations in
/// order, as the game strings them.
fn ambience(map: &Map, wav: Option<&Path>) -> Result<()> {
    use blam_h2::sound::{self, Pcm, SoundFiles};
    let a = sound::ambience(map)?;
    let name = |id: Option<u32>| id.and_then(|i| map.tag_name(i)).unwrap_or("-").to_string();
    for b in &a.backgrounds {
        println!("background {:<24} {} clusters: {} (inside: {})", b.name, b.clusters, name(b.sound), name(b.inside));
    }
    for e in &a.emitters {
        println!("sound scenery at {:.2?}: {}", e.position.to_array(), name(Some(e.looping)));
    }
    if let Some(dir) = wav {
        std::fs::create_dir_all(dir)?;
    }
    let files = SoundFiles::beside(&map.path);
    let backgrounds = sound::background_loops(&a);
    let mut loops = backgrounds.clone();
    for e in &a.emitters {
        if !loops.contains(&e.looping) {
            loops.push(e.looping);
        }
    }
    let mut mix: Option<Pcm> = None;
    for id in loops {
        let l = sound::looping(map, id)?;
        println!("\n{} flags {:#x} distance {:?}", l.name, l.flags, l.distance);
        let tracks = l.tracks.iter().map(|t| (format!("track {} gain {} dB", t.name, t.gain_db), t.looped, t.gain_db, true));
        let details = l.details.iter().map(|d| {
            (
                format!("detail {} every {:?} s, gain {} dB, distance {:?}", d.name, d.period, d.gain_db, d.distance),
                d.sound,
                d.gain_db,
                false,
            )
        });
        for (what, snd, gain_db, is_track) in tracks.chain(details).collect::<Vec<_>>() {
            println!("  {what}");
            let Some(snd) = snd else { continue };
            let s = sound::sound(map, snd)?;
            println!(
                "    {} : {} Hz, {} ch, {:?}, gain {} dB, distance {:?}, skip {}",
                s.name, s.rate, s.channels, s.compression, s.gain_db, s.distance, s.skip_fraction
            );
            let mut whole: Vec<i16> = Vec::new();
            for (k, p) in s.permutations.iter().enumerate() {
                match files.decode(&s, k) {
                    Ok(pcm) => {
                        println!(
                            "      {:<16} skip {:.2} {} chunks: {:.2} s, RMS {:.4}",
                            p.name,
                            p.skip,
                            p.chunks.len(),
                            pcm.seconds(),
                            pcm.rms()
                        );
                        if let Some(dir) = wav {
                            let file = format!("{}_{}.wav", s.name.rsplit('\\').nth(1).unwrap_or("sound"), p.name);
                            sound::write_wav(&dir.join(file), &pcm)?;
                        }
                        whole.extend_from_slice(&pcm.samples);
                    }
                    Err(e) => println!("      {:<16} {e}", p.name),
                }
            }
            if is_track && backgrounds.contains(&id) && !whole.is_empty() {
                let g = 10f32.powf((gain_db + s.gain_db) / 20.0);
                let m = mix.get_or_insert_with(|| Pcm {
                    rate: s.rate,
                    channels: s.channels,
                    samples: Vec::new(),
                });
                if m.rate == s.rate && m.channels == s.channels {
                    if m.samples.len() < whole.len() {
                        m.samples.resize(whole.len(), 0);
                    }
                    for (o, v) in m.samples.iter_mut().zip(&whole) {
                        *o = (*o as f32 + *v as f32 * g).clamp(-32768.0, 32767.0) as i16;
                    }
                }
            }
        }
    }
    if let Some(m) = mix {
        println!("\nbackground mix: {:.2} s at {} Hz, {} ch, RMS {:.4}", m.seconds(), m.rate, m.channels, m.rms());
        if let Some(dir) = wav {
            let path = dir.join(format!("{}_ambience.wav", map.map_name));
            sound::write_wav(&path, &m)?;
            println!("wrote {}", path.display());
        }
    }
    Ok(())
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
