//! `blam sound <map> list | show <tag> | wav <tag>... [--out DIR] | check`
//!
//! - `list`: the map's sound scenery, BSP acoustics and music
//! - `show <tag>`: a `snd!` or `lsnd` tag, with each permutation's bank sample
//! - `wav <tag>...`: decode every permutation of the sounds (an `lsnd`
//!   decodes all its sounds) to WAV in `--out` (default `.scratch/sound`),
//!   printing duration, rate, channels, RMS and peak
//! - `check`: decode every sound the viewer would play and summarise

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context, Result};
use blam_cache::CacheFile;
use blam_sound::Library;
use blam_sound::tags::{Looping, SoundTags};

use crate::{Args, find_tag, maps_dir};

fn library(cache: &CacheFile) -> Result<Library> {
    let dir = cache
        .path()
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(maps_dir);
    let started = std::time::Instant::now();
    let lib = Library::open_beside(&dir).context("opening the FMOD bank beside the maps")?;
    eprintln!(
        "bank {}: {} samples; {} Vorbis setups from halo3.dll, {} rebuilt to their CRC ({:.2} s)",
        lib.sfx.path().display(),
        lib.sfx.samples.len(),
        lib.setups.len(),
        lib.setups.verified_count(),
        started.elapsed().as_secs_f32()
    );
    Ok(lib)
}

pub fn run(cache: &CacheFile, args: &Args) -> Result<()> {
    let what = args.positional.get(1).map(String::as_str).unwrap_or("list");
    let tags = SoundTags::new(cache)?;
    match what {
        "list" => list(&tags),
        "show" => {
            let lib = library(cache)?;
            for t in &args.positional[2..] {
                show(&tags, &lib, find_tag(cache, t)?.index)?;
            }
            Ok(())
        }
        "wav" => {
            let lib = library(cache)?;
            let out = PathBuf::from(args.flag("out").unwrap_or(".scratch/sound"));
            std::fs::create_dir_all(&out)?;
            for t in &args.positional[2..] {
                let tag = find_tag(cache, t)?;
                for s in sounds_of(&tags, tag.index)? {
                    wav(&tags, &lib, s, &out)?;
                }
            }
            Ok(())
        }
        "check" => check(cache, &tags),
        _ => crate::usage(),
    }
}

fn name_of(cache: &CacheFile, i: Option<u32>) -> String {
    i.and_then(|i| cache.tags.get(i))
        .map_or_else(|| "-".to_string(), |t| t.name().to_string())
}

fn list(tags: &SoundTags<'_>) -> Result<()> {
    let c = tags.cache();
    let m = tags.map_sounds()?;
    println!("sound scenery: {}", m.scenery.len());
    for (i, s) in m.scenery.iter().enumerate() {
        println!(
            "  {i:3} ({:8.2} {:8.2} {:7.2}) bsp {:?} vol {} h {:.1} dist {:?}  {} -> {}",
            s.position[0],
            s.position[1],
            s.position[2],
            s.origin_bsp,
            s.volume_type,
            s.height,
            s.distance,
            s.object.rsplit('\\').next().unwrap_or(""),
            name_of(c, s.looping)
        );
    }
    for b in &m.bsps {
        println!("bsp {} acoustics palette: {}", b.bsp, b.palette.len());
        for (i, a) in b.palette.iter().enumerate() {
            println!(
                "  {i}: {} env {} background {} inside {} fade {}",
                a.name,
                name_of(c, a.environment),
                name_of(c, a.background),
                name_of(c, a.inside),
                a.fade
            );
        }
    }
    println!("music: {}", m.music.len());
    for (i, (t, n)) in m.music.iter().enumerate() {
        println!("  {i}: #{t} {n}");
    }
    Ok(())
}

fn print_looping(c: &CacheFile, l: &Looping) {
    println!("lsnd #{} {}", l.tag, l.name);
    for t in &l.tracks {
        println!(
            "  track {} flags {:#x} gain {} dB fade {}/{}",
            t.name, t.flags, t.gain_db, t.fade_in, t.fade_out
        );
        for (n, s) in [
            ("in", t.intro),
            ("loop", t.looped),
            ("out", t.outro),
            ("alt loop", t.alt_loop),
            ("alt out", t.alt_out),
            ("alt trans in", t.alt_trans_in),
            ("alt trans out", t.alt_trans_out),
        ] {
            if s.is_some() {
                println!("    {n:13} {}", name_of(c, s));
            }
        }
    }
    for d in &l.details {
        println!(
            "  detail {} every {:?} s gain {} dB flags {:#x} yaw {:?} pitch {:?} dist {:?} -> {}",
            d.name,
            d.period,
            d.gain_db,
            d.flags,
            d.yaw,
            d.pitch,
            d.distance,
            name_of(c, d.sound)
        );
    }
}

fn show(tags: &SoundTags<'_>, lib: &Library, index: u32) -> Result<()> {
    let c = tags.cache();
    let class = c
        .tags
        .get(index)
        .map(|t| c.tags.class_of(t).code.to_string());
    if class.as_deref() == Some("lsnd") {
        print_looping(c, &tags.looping(index)?);
        return Ok(());
    }
    let s = tags.sound(index)?;
    println!(
        "snd! #{} {}: flags {:#x} class {} encoding {} max {} ms, playback {:?}",
        s.tag, s.name, s.flags, s.class, s.encoding, s.max_play_ms, s.playback
    );
    for p in &s.permutations {
        match lib.sample(p.fmod_id) {
            Some(i) => {
                let smp = &lib.sfx.samples[i as usize];
                let info = &lib.sfx.infos[i as usize];
                println!(
                    "  {} id {:#010x} -> sample {i}: {} Hz x{} {} frames ({:.2} s) loop end {}  {}",
                    p.name,
                    p.fmod_id,
                    smp.rate,
                    smp.channels,
                    smp.frames,
                    smp.frames as f32 / smp.rate as f32,
                    info.loop_end,
                    info.path
                );
            }
            None => println!("  {} id {:#010x} -> not in the sfx bank", p.name, p.fmod_id),
        }
    }
    Ok(())
}

/// The `snd!` tags a tag stands for: itself, or every sound of an `lsnd`.
fn sounds_of(tags: &SoundTags<'_>, index: u32) -> Result<Vec<u32>> {
    let c = tags.cache();
    let class = c
        .tags
        .get(index)
        .map(|t| c.tags.class_of(t).code.to_string());
    Ok(match class.as_deref() {
        Some("lsnd") => {
            let l = tags.looping(index)?;
            let mut v: Vec<u32> = l
                .tracks
                .iter()
                .flat_map(|t| {
                    [
                        t.intro,
                        t.looped,
                        t.outro,
                        t.alt_loop,
                        t.alt_out,
                        t.alt_trans_in,
                        t.alt_trans_out,
                    ]
                })
                .chain(l.details.iter().map(|d| d.sound))
                .flatten()
                .collect();
            v.dedup();
            v
        }
        _ => vec![index],
    })
}

fn wav(tags: &SoundTags<'_>, lib: &Library, index: u32, out: &std::path::Path) -> Result<()> {
    let s = tags.sound(index)?;
    let base: String = s
        .name
        .rsplit('\\')
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("_");
    for p in &s.permutations {
        let started = std::time::Instant::now();
        match lib.decode(p.fmod_id) {
            Ok(pcm) => {
                let (rms, peak) = pcm.rms_peak();
                let path = out.join(format!("{base}_{}.wav", p.name));
                blam_sound::write_wav(&path, &pcm)?;
                println!(
                    "{}: {:.2} s, {} Hz x{}, rms {:.4} ({:.1} dBFS), peak {:.3}, decoded in {:.0} ms",
                    path.display(),
                    pcm.seconds(),
                    pcm.rate,
                    pcm.channels,
                    rms,
                    20.0 * rms.max(1e-9).log10(),
                    peak,
                    started.elapsed().as_secs_f64() * 1000.0
                );
            }
            Err(e) => println!("{} {}: {e}", s.name, p.name),
        }
    }
    Ok(())
}

/// Decodes everything the viewer would play: scenery loops, backgrounds,
/// their details, music.
fn check(cache: &CacheFile, tags: &SoundTags<'_>) -> Result<()> {
    let lib = library(cache)?;
    let m = tags.map_sounds()?;
    let mut loops: BTreeSet<u32> = m.scenery.iter().filter_map(|s| s.looping).collect();
    for b in &m.bsps {
        loops.extend(
            b.palette
                .iter()
                .flat_map(|a| [a.background, a.inside])
                .flatten(),
        );
    }
    loops.extend(m.music.iter().map(|(t, _)| *t));
    let (mut ok, mut failed, mut missing, mut seconds) = (0, 0, 0, 0.0f64);
    let mut silent = Vec::new();
    let started = std::time::Instant::now();
    for l in &loops {
        for s in sounds_of(tags, *l)? {
            let snd = tags.sound(s)?;
            for p in &snd.permutations {
                if lib.sample(p.fmod_id).is_none() {
                    missing += 1;
                    continue;
                }
                match lib.decode(p.fmod_id) {
                    Ok(pcm) => {
                        ok += 1;
                        seconds += pcm.seconds() as f64;
                        if pcm.rms_peak().1 < 1e-3 {
                            silent.push(format!("{} {}", snd.name, p.name));
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        println!("{} {}: {e}", snd.name, p.name);
                    }
                }
            }
        }
    }
    println!(
        "{} looping sounds: {ok} permutations decoded ({seconds:.0} s of audio) in {:.1} s, {failed} failed, {missing} not in the bank, {} silent {:?}",
        loops.len(),
        started.elapsed().as_secs_f32(),
        silent.len(),
        silent
    );
    Ok(())
}
