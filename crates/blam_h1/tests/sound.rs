//! Halo CE's sound on The Maw (`d40`): the Warthog run's music and lines
//! are found in MCC's banks by tag path and decode (`docs/format/halo1.md`
//! 13). Skips quietly when the map or the banks are not installed.

use blam_h1::Map;
use blam_h1::sound::{self, Banks};

fn setup() -> Option<(Map, Banks)> {
    let map = Map::open(&game_paths::halo1_map("d40")?).ok()?;
    let dir = game_paths::halo1_sound()?;
    if !dir.join("sounds_music.fsb").is_file() {
        return None;
    }
    let banks = Banks::open(&dir, "english").unwrap();
    Some((map, banks))
}

fn index(map: &Map, class: &[u8; 4], name: &str) -> u32 {
    map.find(class, name).unwrap_or_else(|| panic!("no {name}")).id & 0xFFFF
}

#[test]
fn the_run_music_tags() {
    let Some((map, _)) = setup() else { return };
    let l = sound::looping(&map, index(&map, b"lsnd", r"levels\d40\music\d40_07")).unwrap();
    assert_eq!(l.flags, 0);
    assert_eq!(l.tracks.len(), 1);
    let t = &l.tracks[0];
    // Fade out on stop (6 s), cross-fade to the alternate.
    assert_eq!(t.flags, 6);
    assert_eq!(t.fade_out, 6.0);
    let name = |i: Option<u32>| i.map(|i| map.tags[i as usize].name.clone());
    assert_eq!(name(t.start).as_deref(), Some(r"sound\music\perc\in"));
    assert_eq!(name(t.looped).as_deref(), Some(r"sound\music\perc\loops"));
    assert_eq!(name(t.alt_loop).as_deref(), Some(r"sound\music\misc\string_idea"));
    assert_eq!(t.end, None);
    // d40_09 is the finale: its loop plays once.
    let finale = sound::looping(&map, index(&map, b"lsnd", r"levels\d40\music\d40_09")).unwrap();
    assert_eq!(finale.flags & 2, 2);

    let loops = sound::sound(&map, t.looped.unwrap()).unwrap();
    assert_eq!(loops.class, 32);
    // Five bars, each split in two in the original release.
    assert_eq!(loops.pitch_ranges[0].actual, 5);
    assert_eq!(loops.pitch_ranges[0].permutations.len(), 10);
    assert_eq!(loops.actual_permutations().count(), 5);
    // A bar is two pieces of the original release; 9.2 s together.
    assert!((loops.seconds() - 9.24).abs() < 0.05, "{}", loops.seconds());

    let scripts = sound::script_sounds(&map).unwrap();
    for n in [r"levels\d40\music\d40_07", r"levels\d40\music\d40_08", r"levels\d40\music\d40_09"] {
        assert!(scripts.contains(&index(&map, b"lsnd", n)), "{n}");
    }
    assert!(scripts.contains(&index(&map, b"snd!", r"sound\dialog\d40\d40_440_cortana")));
}

#[test]
fn the_run_music_decodes() {
    let Some((map, banks)) = setup() else { return };
    let tag = &map.tags[index(&map, b"snd!", r"sound\music\perc\in") as usize];
    let ids = banks.samples_of(&tag.name);
    assert_eq!(ids.len(), 1);
    let s = banks.sample(ids[0]).unwrap();
    assert_eq!((s.rate, s.channels, s.frames), (44_100, 2, 204_288));
    let t = std::time::Instant::now();
    let pcm = banks.decode(ids[0]).unwrap();
    let rms = (pcm.samples.iter().map(|v| (*v as f64 / 32768.0).powi(2)).sum::<f64>() / pcm.samples.len() as f64).sqrt();
    println!(
        "perc\\in: {} frames, {:.2} s, peak {:.3}, rms {:.1} dBFS, decoded in {:.0} ms",
        pcm.frames(),
        pcm.seconds(),
        pcm.peak(),
        20.0 * rms.log10(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    assert_eq!(pcm.frames(), 204_288);
    let full = pcm.samples.iter().filter(|v| v.unsigned_abs() >= 32767).count();
    println!("{full} samples at full scale of {}", pcm.samples.len());
    assert!(pcm.peak() > 0.1 && pcm.peak() <= 1.0, "peak {}", pcm.peak());
    assert!(rms > 0.01, "rms {rms}");
    // Smooth, not noise: neighbouring samples agree far more than random.
    let l: Vec<f64> = pcm.samples.iter().step_by(2).map(|v| *v as f64).collect();
    let diff = l.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / l.len() as f64;
    let level = l.iter().map(|v| v.abs()).sum::<f64>() / l.len() as f64;
    println!("mean step {diff:.0} against mean level {level:.0}");
    assert!(diff < level * 0.5, "step {diff} level {level}");

    // Every loop bar decodes to its header's length.
    let loops = &map.tags[index(&map, b"snd!", r"sound\music\perc\loops") as usize];
    for id in banks.samples_of(&loops.name) {
        let p = banks.decode(id).unwrap();
        assert_eq!(p.frames(), banks.sample(id).unwrap().frames as usize);
    }
}

#[test]
fn the_run_lines_decode() {
    let Some((map, banks)) = setup() else { return };
    let mut total = 0.0;
    let mut n = 0;
    for t in map.tags.iter().filter(|t| t.is(b"snd!") && t.name.starts_with(r"sound\dialog\d40\d40_")) {
        let ids = banks.samples_of(&t.name);
        assert!(!ids.is_empty(), "{} is in no bank", t.name);
        total += banks.seconds(ids[0]);
        n += 1;
    }
    println!("{n} d40 lines, {total:.0} s");
    assert!(n >= 50);
    let tag = &map.tags[index(&map, b"snd!", r"sound\dialog\d40\d40_440_cortana") as usize];
    let snd = sound::sound(&map, tag.id & 0xFFFF).unwrap();
    assert_eq!(snd.class, 44);
    // The tag's own length agrees with MCC's take.
    assert!((snd.seconds() - banks.seconds(banks.samples_of(&tag.name)[0])).abs() < 0.05, "{}", snd.seconds());
    let lengths = sound::script_seconds(&map);
    assert!((lengths[&(tag.id & 0xFFFF)] - 13.19).abs() < 0.05, "{:?}", lengths.get(&(tag.id & 0xFFFF)));
    assert!((snd.gain - 0.85).abs() < 1e-6);
    let id = banks.samples_of(&tag.name)[0];
    assert_eq!(banks.sample(id).unwrap().name, "D40_440_Cortana");
    let pcm = banks.decode(id).unwrap();
    println!("d40_440_cortana: {:.2} s, peak {:.3}", pcm.seconds(), pcm.peak());
    assert_eq!(pcm.channels, 1);
    assert!(pcm.seconds() > 1.0 && pcm.peak() > 0.05);
}

#[test]
fn the_language_banks() {
    let Some((map, _)) = setup() else { return };
    let dir = game_paths::halo1_sound().unwrap();
    if !dir.join("sounds_dialogs_french.fsb").is_file() {
        return;
    }
    let fr = Banks::open(&dir, "french").unwrap();
    assert_eq!(fr.language, Some("french"));
    let tag = &map.tags[index(&map, b"snd!", r"sound\dialog\d40\d40_440_cortana") as usize];
    let id = fr.samples_of(&tag.name)[0];
    // The French bank comes first.
    assert_eq!(fr.bank_of(id).unwrap().name, "sounds_dialogs_french");
    let pcm = fr.decode(id).unwrap();
    println!("French d40_440_cortana: {:.2} s, peak {:.3}", pcm.seconds(), pcm.peak());
    assert!(pcm.seconds() > 1.0 && pcm.peak() > 0.05);
    // No Chinese bank: English.
    assert_eq!(sound::dialog_language("mexican"), Some("spanish"));
    assert_eq!(sound::dialog_language("english"), None);
}

#[test]
fn every_d40_sound_is_found_and_decodes() {
    let Some((map, banks)) = setup() else { return };
    let (mut found, mut missing, mut failed, mut skipped) = (0, Vec::new(), Vec::new(), Vec::new());
    for t in map.tags.iter().filter(|t| t.is(b"snd!")) {
        let ids = banks.samples_of(&t.name);
        if ids.is_empty() {
            missing.push(t.name.clone());
            continue;
        }
        found += 1;
        // The first sample of each (all of them would take a while).
        match banks.decode(ids[0]) {
            Ok(p) if p.frames() == banks.sample(ids[0]).unwrap().frames as usize => {}
            Ok(p) => failed.push(format!("{}: {} frames of {}", t.name, p.frames(), banks.sample(ids[0]).unwrap().frames)),
            // The cutscenes' 5.1 mixes are not CELT frames (not decoded;
            // the run has none).
            Err(_) if banks.sample(ids[0]).unwrap().name.contains("5.1mix") => skipped.push(t.name.clone()),
            Err(e) => failed.push(format!("{}: {e}", t.name)),
        }
    }
    println!("{found} found, missing {missing:?}, failed {failed:?}, 5.1 mixes skipped {skipped:?}");
    assert!(failed.is_empty(), "{failed:?}");
    assert!(missing.len() <= 4, "{missing:?}");
}

#[test]
fn the_run_background_sound() {
    let Some(map) = game_paths::halo1_map("d40").and_then(|p| Map::open(&p).ok()) else { return };
    let bsps = sound::bsp_sounds(&map, Some(&[5, 6, 7])).unwrap();
    assert_eq!(bsps.len(), 3);
    let name = |i: Option<u32>| i.map_or("-".to_string(), |i| map.tags[i as usize].name.clone());
    for b in &bsps {
        println!(
            "bsp {}: {} clusters, palette {:?}",
            b.bsp,
            b.clusters.len(),
            b.palette.iter().map(|p| name(*p)).collect::<Vec<_>>()
        );
    }
    // The first Warthog's bay is in a cluster of d40f with the trench's
    // own background.
    let start = blam_h1::object_named(&map, "trench_jeep1").unwrap().position;
    let f = &bsps[0];
    let c = f.finder.cluster(start + glam::Vec3::Z * 0.3).expect("the start is in a cluster");
    let bg = f.clusters[c as usize].and_then(|e| f.palette[e as usize]);
    println!("start: cluster {c}, background {}", name(bg));
    assert!(name(bg).contains("trench"), "{}", name(bg));
    // Along the run every point of the road is in some cluster of a BSP.
    let scene_volumes = ["enc6_1", "enc6_5_4", "enc6_6_1", "enc6_9_1", "enc7_7"];
    let opts = blam_h1::Options { textures: false, objects: false, collision: false, bsps: Some(vec![5, 6, 7]), ..Default::default() };
    let scene = blam_h1::decode(&map, &opts).unwrap().scene;
    for v in scene.trigger_volumes.iter().filter(|v| scene_volumes.contains(&v.name.as_str())) {
        let left = v.up.cross(v.forward);
        let mid = v.position + (v.forward * v.extents.x + left * v.extents.y + v.up * v.extents.z) * 0.5;
        let found: Vec<String> = bsps
            .iter()
            .filter_map(|b| {
                let c = b.finder.cluster(mid)?;
                Some(format!("bsp {} cluster {c} {}", b.bsp, name(b.clusters[c as usize].and_then(|e| b.palette[e as usize]))))
            })
            .collect();
        println!("{}: {found:?}", v.name);
        assert!(!found.is_empty(), "{} is in no cluster", v.name);
    }
}
