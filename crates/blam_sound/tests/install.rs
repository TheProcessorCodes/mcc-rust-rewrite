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
