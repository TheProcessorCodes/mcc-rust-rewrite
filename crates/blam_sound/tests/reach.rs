//! Reach's sound tags against the user's MCC install (skipped when Reach
//! is not there). The numbers are the ones in `docs/format/sound.md` 12.

use std::path::PathBuf;

use blam_cache::CacheFile;
use blam_sound::Library;
use blam_sound::tags::SoundTags;

fn maps() -> Option<PathBuf> {
    let dir = game_paths::reach_maps()?;
    dir.join("20_sword_slayer.map").exists().then_some(dir)
}

#[test]
fn reach_bank_and_setups() {
    let Some(maps) = maps() else { return };
    let lib = Library::open_beside(&maps).unwrap();
    assert_eq!(lib.sfx.samples.len(), 16162);
    // Every setup the bank uses was rebuilt from `haloreach.dll`.
    for crc in lib.sfx.setup_crcs() {
        assert!(lib.setups.verified(crc), "setup {crc:#010x}");
    }
}

#[test]
fn sword_base_ambience_resolves_and_decodes() {
    let Some(maps) = maps() else { return };
    let cache = CacheFile::open(maps.join("20_sword_slayer.map")).unwrap();
    let lib = Library::open_beside(&maps).unwrap();
    let tags = SoundTags::new(&cache).unwrap();
    assert!(tags.is_reach());

    let m = tags.map_sounds().unwrap();
    assert_eq!(m.scenery.len(), 363);
    assert!(m.scenery.iter().all(|s| s.looping.is_some() && s.origin_bsp.is_none()));
    let rapids = m.scenery.iter().find(|s| s.object.ends_with("riverworld_rapids")).unwrap();
    assert_eq!(rapids.distance, Some((3.0, 10.0)));
    assert_eq!(m.bsps.len(), 3);
    assert_eq!(m.bsps[0].palette.len(), 12);
    assert!(m.script_sounds.is_empty());
    assert!(tags.mission_dialogue().unwrap().is_empty());

    // What most of the map's clusters sound like.
    let interior = &m.bsps[0].palette[1];
    assert_eq!(interior.name, "interior");
    assert!(interior.environment.is_some());
    let looping = tags.looping(interior.background.unwrap()).unwrap();
    assert_eq!(looping.name, r"sound\levels\solo\m20\sword_base_int_small\sword_base_int_small");
    assert_eq!(looping.class, 33);
    assert_eq!(looping.distance, (0.9, 5.0));
    assert_eq!(looping.tracks.len(), 1);
    let track = &looping.tracks[0];
    assert_eq!((track.gain_db, track.fade_in, track.fade_out), (-19.0, 0.1, 0.1));
    assert!(track.fades_in() && track.fades_out());
    assert!(track.intro.is_none() && track.outro.is_none());
    let snd = tags.sound(track.looped.unwrap()).unwrap();
    assert!(snd.name.ends_with(r"sword_base_int_small\loop"));
    assert_eq!((snd.class, snd.max_play_ms, snd.playback.gain_db), (33, 4806, -3.0));
    assert_eq!(snd.permutations.len(), 3);
    assert_eq!(snd.permutations[0].name, "sword_base_int_small_01");
    for p in &snd.permutations {
        assert!(lib.find(p.fmod_id).is_some(), "{} is not in the bank", p.name);
        let pcm = lib.decode(p.fmod_id).unwrap();
        assert_eq!(pcm.rate, 48000);
        let seconds = pcm.frames() as f32 / 48000.0;
        assert!((3.0..9.0).contains(&seconds), "{}: {seconds} s", p.name);
        let rms = (pcm.samples.iter().map(|s| s * s).sum::<f32>() / pcm.samples.len() as f32).sqrt();
        assert!(rms > 1e-4, "{} is silent", p.name);
    }

    // A background with details, and the details' own distances.
    let large = tags.looping(m.bsps[0].palette[2].background.unwrap()).unwrap();
    assert_eq!(large.details.len(), 1);
    let d = &large.details[0];
    assert_eq!(d.name, "verby_int_details");
    assert_eq!((d.period, d.gain_db, d.distance), ((3.0, 7.0), -20.0, (5.0, 10.0)));
    assert!(tags.sound(d.sound.unwrap()).unwrap().permutations.len() > 1);

    // A track with an intro and an outro sits in the first and third slots.
    let with_in = cache
        .tags
        .of_class("lsnd")
        .filter_map(|t| tags.looping(t.index).ok())
        .flat_map(|l| l.tracks)
        .find(|t| t.intro.is_some())
        .expect("a track with an intro");
    let name = |t: u32| cache.tags.get(t).unwrap().name().to_string();
    assert!(name(with_in.intro.unwrap()).ends_with(r"\in"));
    assert!(name(with_in.looped.unwrap()).ends_with("loop"));

    // Classes: ambient loops 6 dB down, music at 0, and distances that
    // grow (full volume within the first, silent beyond the second).
    let classes = tags.classes().unwrap();
    assert_eq!(classes.len(), 71);
    assert_eq!(classes[33].gain_bounds_db, [-6.0, -6.0]);
    assert_eq!(classes[32].gain_bounds_db, [0.0, 0.0]);
    assert_eq!((classes[4].distances[2], classes[4].distances[3]), (8.0, 50.0));
}

#[test]
fn every_reach_map_reads_its_sounds() {
    let Some(maps) = maps() else { return };
    let lib = Library::open_beside(&maps).unwrap();
    for name in ["forge_halo", "cex_timberland", "30_settlement", "45_aftship", "70_boneyard"] {
        let path = maps.join(format!("{name}.map"));
        if !path.exists() {
            continue;
        }
        let cache = CacheFile::open(&path).unwrap();
        let tags = SoundTags::new(&cache).unwrap();
        let m = tags.map_sounds().unwrap();
        let mut loops: Vec<u32> = m.scenery.iter().filter_map(|s| s.looping).collect();
        loops.extend(m.bsps.iter().flat_map(|b| b.palette.iter().filter_map(|a| a.background)));
        loops.sort_unstable();
        loops.dedup();
        assert!(!loops.is_empty(), "{name}: no looping sounds");
        // MCC's bank lacks a few samples the tags name (Timberland's
        // `wind_trees` layer); the loader leaves those out.
        let (mut permutations, mut missing) = (0, 0);
        for l in loops {
            let looping = tags.looping(l).unwrap_or_else(|e| panic!("{name}: lsnd {l}: {e}"));
            assert!(!looping.tracks.is_empty(), "{name}: {} has no tracks", looping.name);
            for t in &looping.tracks {
                assert!((-64.0..=12.0).contains(&t.gain_db), "{name}: {} gain {}", looping.name, t.gain_db);
                for s in t.sounds().into_iter().flatten() {
                    let snd = tags.sound(s).unwrap();
                    for p in &snd.permutations {
                        permutations += 1;
                        missing += usize::from(lib.find(p.fmod_id).is_none());
                    }
                }
            }
        }
        assert!(permutations > 0 && missing * 10 < permutations, "{name}: {missing} of {permutations} not in the bank");
    }
}
