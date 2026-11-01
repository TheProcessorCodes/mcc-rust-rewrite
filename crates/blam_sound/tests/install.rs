//! Checks against the user's MCC install (skipped when it is not there).
//! The numbers are the ones in `docs/format/sound.md`.

use std::path::PathBuf;

use blam_cache::CacheFile;
use blam_sound::Library;
use blam_sound::tags::SoundTags;

fn maps() -> Option<PathBuf> {
    let dir = game_paths::halo3_maps()?;
    dir.join("riverworld.map").exists().then_some(dir)
}

#[test]
fn bank_and_setups() {
    let Some(maps) = maps() else { return };
    let lib = Library::open_beside(&maps).unwrap();
    assert_eq!(lib.sfx.samples.len(), 16421);
    assert_eq!(lib.sfx.setup_crcs(), vec![0x632B_1FAC, 0xAD11_D38D]);
    assert!(lib.setups.verified(0x632B_1FAC) && lib.setups.verified(0xAD11_D38D));
    // The rebuilt packets are the lengths FMOD's table says.
    assert_eq!(lib.setups.get(0xAD11_D38D).unwrap().len(), 3484);
    assert_eq!(lib.setups.get(0x632B_1FAC).unwrap().len(), 3006);
    let s0 = &lib.sfx.samples[0];
    assert_eq!(
        (s0.rate, s0.channels, s0.frames, s0.offset),
        (48000, 1, 13725, 0)
    );
    assert_eq!(lib.sfx.samples[1].offset, 2528);
    let info = &lib.sfx.infos[0];
    assert_eq!(
        info.path,
        r"data\sound\characters\bodyfalls\bodythud\bodythud1.aif"
    );
    assert_eq!(info.id, blam_sound::fsb::id_of_path(&info.path));
    assert_eq!(info.id, 0x2071_32B0);
}

#[test]
fn riverworld_tags_resolve_and_decode() {
    let Some(maps) = maps() else { return };
    let cache = CacheFile::open(maps.join("riverworld.map")).unwrap();
    let lib = Library::open_beside(&maps).unwrap();
    let tags = SoundTags::new(&cache).unwrap();

    let bird = tags.sound(6786).unwrap();
    assert_eq!(
        bird.name,
        r"sound\levels\riverworld\halo_ext\details\weird_bird"
    );
    assert_eq!(bird.permutations.len(), 9);
    assert_eq!(bird.permutations[2].name, "weird_bird2");
    assert_eq!(bird.permutations[2].fmod_id, 0x073D_1E2E);
    assert_eq!(bird.playback.bounds(), (35.0, 45.0));
    assert_eq!(bird.max_play_ms, 3752);

    let ext = tags.looping(6784).unwrap();
    assert_eq!(ext.tracks.len(), 1);
    assert_eq!(ext.tracks[0].looped, Some(6785));
    assert_eq!(ext.tracks[0].gain_db, -13.0);
    assert_eq!(ext.details.len(), 6);
    assert_eq!(ext.details[0].sound, Some(6786));
    assert_eq!(ext.details[0].period, (20.0, 45.0));

    let m = tags.map_sounds().unwrap();
    assert_eq!(m.scenery.len(), 77);
    assert_eq!(m.bsps.len(), 1);
    assert_eq!(m.bsps[0].palette.len(), 3);
    assert_eq!(m.bsps[0].palette[0].background, Some(6784));
    assert_eq!(m.scenery[0].distance, Some((50.0, 120.0)));

    // Loop permutations are cut to the info's loop end; stereo is clean
    // (a wrong setup tail gave peaks of 7 here).
    let loop_snd = tags.sound(6785).unwrap();
    let pcm = lib.decode(loop_snd.permutations[1].fmod_id).unwrap();
    assert_eq!((pcm.rate, pcm.channels), (48000, 2));
    assert_eq!(pcm.frames(), 257_984 + 1);
    let (rms, peak) = pcm.rms_peak();
    assert!(peak < 1.2 && rms > 0.05, "rms {rms} peak {peak}");
}

#[test]
fn halo_music_and_quad_ambience() {
    let Some(maps) = maps() else { return };
    let Ok(cache) = CacheFile::open(maps.join("120_halo.map")) else {
        return;
    };
    let lib = Library::open_beside(&maps).unwrap();
    let tags = SoundTags::new(&cache).unwrap();
    let m = tags.map_sounds().unwrap();
    let run = m
        .music
        .iter()
        .find(|(_, n)| n.ends_with("120_music_15"))
        .unwrap();
    let l = tags.looping(run.0).unwrap();
    let t = &l.tracks[0];
    assert!(t.intro.is_some() && t.looped.is_some() && t.alt_loop.is_some());
    let snd = tags.sound(t.looped.unwrap()).unwrap();
    assert_eq!(snd.permutations.len(), 4);
    let pcm = lib.decode(snd.permutations[0].fmod_id).unwrap();
    assert_eq!(pcm.frames(), 1_474_560);
    assert!(pcm.rms_peak().1 < 1.2);
    // 4-layer "mono" sample: one Vorbis stream of 4 channels.
    let quad = tags.sound(8845).unwrap();
    let pcm = lib.decode(quad.permutations[0].fmod_id).unwrap();
    assert_eq!(pcm.channels, 4);
    assert_eq!(pcm.frames(), 320_832 + 1);
}

/// 120_halo's music pieces as `sound.md` 8 lists them: the header and
/// track fields, which slots each piece has, and the sounds its scripts
/// name.
#[test]
fn halo_music_pieces() {
    let Some(maps) = maps() else { return };
    let Ok(cache) = CacheFile::open(maps.join("120_halo.map")) else {
        return;
    };
    let tags = SoundTags::new(&cache).unwrap();
    let m = tags.map_sounds().unwrap();
    assert_eq!(m.music.len(), 16);
    let piece = |n: &str| {
        let (t, _) = m.music.iter().find(|(_, name)| name.ends_with(n)).unwrap();
        tags.looping(*t).unwrap()
    };
    // (piece, header flags, music time s, track flags, fade out s,
    //  slots in / loop / out / alt loop / alt out / alt trans in / out)
    let expected: [(&str, u32, f32, u32, f32, &str); 16] = [
        ("120_music_01", 0, 180.0, 0x0, 0.0, "111...."),
        ("120_music_02", 0, 300.0, 0x2, 30.0, "11....."),
        ("120_music_03", 0, 300.0, 0x2, 15.0, "11.1..."),
        ("120_music_04", 0, 120.0, 0x0, 0.0, "111...."),
        ("120_music_045", 0, 180.0, 0x2, 10.0, ".1....."),
        ("120_music_05", 0, 300.0, 0x0, 0.0, "111...."),
        ("120_music_06", 0, 300.0, 0x2, 15.0, "11....."),
        ("120_music_07", 0, 300.0, 0x2, 15.0, "11....."),
        ("120_music_08", 0, 300.0, 0x2, 10.0, ".1....."),
        ("120_music_09", 0, 300.0, 0x2, 10.0, "11....."),
        ("120_music_10", 0, 300.0, 0x2, 10.0, "11....."),
        ("120_music_11", 0, 300.0, 0x2, 6.0, "11....."),
        ("120_music_12", 2, 0.0, 0x0, 6.0, ".1....."),
        ("120_music_13", 0, 300.0, 0x6, 10.0, "11.1..."),
        ("120_music_14", 0, 180.0, 0x0, 0.0, "111...."),
        ("120_music_15", 4, 0.0, 0x2, 10.0, "11.111."),
    ];
    for (name, flags, time, track_flags, fade_out, slots) in expected {
        let l = piece(name);
        assert_eq!(l.tracks.len(), 1, "{name}");
        assert!(l.details.is_empty(), "{name}");
        assert_eq!((l.flags, l.music_time, l.class), (flags, time, 32), "{name}");
        assert!(l.is_music());
        let t = &l.tracks[0];
        assert_eq!(t.name, "track1");
        assert_eq!((t.flags, t.fade_in, t.fade_out), (track_flags, 0.0, fade_out), "{name}");
        let got: String = t.sounds().iter().map(|s| if s.is_some() { '1' } else { '.' }).collect();
        assert_eq!(got, slots, "{name}");
    }
    let stinger = piece("120_music_12");
    assert!(stinger.not_a_loop() && !stinger.stops_music());
    let run = piece("120_music_15");
    assert!(run.stops_music() && !run.not_a_loop());
    assert!(run.tracks[0].fades_out() && !run.tracks[0].crossfades_alt());
    assert_eq!(run.tracks[0].gain_db, -3.0);
    let thirteen = piece("120_music_13");
    assert!(thirteen.tracks[0].crossfades_alt());
    assert_eq!(thirteen.tracks[0].alt_fade, 10.0);
    // Its permutations: the Warthog run's intro, four loops, three
    // alternate loops, the 15 s transition and the 77 s finale.
    let perms = |s: Option<u32>| tags.sound(s.unwrap()).unwrap().permutations.len();
    let t = &run.tracks[0];
    assert_eq!(
        [perms(t.intro), perms(t.looped), perms(t.alt_loop), perms(t.alt_out), perms(t.alt_trans_in)],
        [1, 4, 3, 1, 1]
    );
    // The scripts' tag references hold all 16 pieces and 30 `snd!`s
    // (dialogue, sparks, the run's island creaks and tower crash).
    let lsnds = m.script_sounds.iter().filter(|t| m.music.iter().any(|(i, _)| i == *t)).count();
    assert_eq!(lsnds, 16);
    assert_eq!(m.script_sounds.len(), 46);
}

/// The sound classes' gain ranges (`sound.md` 9): music plays at 0 dB,
/// dialogue at -3, weapons, ambience and vehicles 12 to 15 dB down.
#[test]
fn halo_class_gains() {
    let Some(maps) = maps() else { return };
    let Ok(cache) = CacheFile::open(maps.join("120_halo.map")) else {
        return;
    };
    let classes = SoundTags::new(&cache).unwrap().classes().unwrap();
    assert_eq!(classes.len(), 54);
    let gain = |i: usize| classes[i].gain_bounds_db;
    assert_eq!(gain(32), [0.0, 0.0]); // music
    assert_eq!(gain(19), [-3.0, -3.0]); // unit dialogue
    assert_eq!(gain(47), [-3.0, -3.0]); // mission dialogue
    assert_eq!(gain(4), [-12.0, -12.0]); // weapon fire
    assert_eq!(gain(33), [-12.0, -12.0]); // ambience (the run's plates)
    assert_eq!(gain(23), [-15.0, -15.0]); // vehicle engines
    assert_eq!(gain(13), [-30.0, -12.0]); // physics impacts: a range
}

/// The English dialogue bank (`sound.md` 11): laid out like `sfx.fsb`, its
/// ids are what the dialogue `snd!` tags store, and 120_halo's mission
/// dialogue tag names the Warthog run's lines.
#[test]
fn english_dialogue_bank_and_mission_dialogue() {
    let Some(maps) = maps() else { return };
    let Ok(cache) = CacheFile::open(maps.join("120_halo.map")) else {
        return;
    };
    let lib = Library::open_beside_with(&maps, Some("english")).unwrap();
    let d = lib.dialog.as_ref().unwrap();
    assert_eq!(d.samples.len(), 32_984);
    assert_eq!(d.setup_crcs(), vec![0x632B_1FAC, 0xAD11_D38D]);
    // No id is in both banks.
    assert!(d.infos.iter().all(|i| lib.sfx.find(i.id).is_none()));

    let tags = SoundTags::new(&cache).unwrap();
    let lines = tags.mission_dialogue().unwrap();
    assert_eq!(lines.len(), 213);
    assert_eq!(lines.iter().filter(|l| l.variants.len() == 2).count(), 1);
    let l = lines.iter().find(|l| l.name == "120mg_030").unwrap();
    assert_eq!(l.effect, "cortana_mission");
    let (snd, speaker) = l.sound().unwrap();
    assert_eq!(speaker, "cor");
    let snd = tags.sound(snd).unwrap();
    assert_eq!(snd.name, r"sound\dialog\120_halo\mission\120mg_030_cor");
    assert_eq!((snd.class, snd.max_play_ms), (47, 2133));
    assert_eq!(snd.permutations.len(), 1);
    let id = snd.permutations[0].fmod_id;
    assert_eq!(id, 0x87AA_D115);
    assert!(lib.sample(id).is_none(), "not in sfx.fsb");
    let (bank, i) = lib.find(id).unwrap();
    assert!(bank.path().ends_with("english.fsb"));
    assert!(bank.infos[i as usize].path.ends_with(r"120mg_030_cor.aif"));
    let pcm = lib.decode(id).unwrap();
    assert_eq!((pcm.rate, pcm.channels, pcm.frames()), (48_000, 1, 102_400));
    let (rms, peak) = pcm.rms_peak();
    // About -16 dBFS RMS; lossy decoding overshoots full scale a little.
    assert!(rms > 0.1 && peak < 1.2, "rms {rms} peak {peak}");
    // The length from the `.info` alone.
    let secs = blam_sound::info_seconds(bank.path()).unwrap();
    assert!((secs[&id] - 102_400.0 / 48_000.0).abs() < 1e-4);
}
