//! Lockout's ambience from the user's own MCC install: the background
//! looping sound, its tracks, and the Xbox ADPCM samples in
//! `sounds_neutral.dat` (`docs/format/halo2.md` section 13). Skips quietly
//! when the map is not installed.

use std::path::PathBuf;

use blam_h2::sound::{self, Compression, SoundFiles};

fn map(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("HALO2_MAPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo2\h2_maps_win64_dx11")
    });
    let p = dir.join(format!("{name}.map"));
    (p.exists() && dir.join("sounds_neutral.dat").exists()).then_some(p)
}

#[test]
fn lockout_ambience_decodes() {
    let Some(path) = map("lockout") else { return };
    let m = blam_h2::Map::open(&path).expect("opens");
    let a = sound::ambience(&m).expect("ambience");

    // Every used palette entry plays the same background.
    let loops = sound::background_loops(&a);
    assert_eq!(loops.len(), 1);
    assert_eq!(m.tag_name(loops[0]), Some(r"sound\ambience\lockout\lockout\lockout"));
    assert_eq!(a.backgrounds.iter().map(|b| b.clusters).sum::<usize>(), 16);
    assert_eq!(a.emitters.len(), 4);

    let l = sound::looping(&m, loops[0]).expect("lsnd");
    let names: Vec<_> = l.tracks.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["front_speakers", "rear_speakers"]);
    assert!(l.tracks.iter().all(|t| t.gain_db == -3.0));
    assert_eq!(l.details.len(), 1);
    assert_eq!(l.details[0].period, (40.0, 50.0));

    let files = SoundFiles::beside(&path);
    let front = sound::sound(&m, l.tracks[0].looped.unwrap()).expect("snd!");
    assert_eq!((front.rate, front.channels, front.compression), (44_100, 2, Compression::XboxAdpcm));
    assert_eq!(front.permutations.len(), 6);
    // The first permutation is played first only.
    assert_eq!(front.permutations[0].skip, 1.0);
    let mut seconds = 0.0;
    for k in 0..front.permutations.len() {
        let pcm = files.decode(&front, k).expect("decodes");
        // Wind, not silence and not noise at full scale.
        let rms = pcm.rms();
        assert!((0.005..0.1).contains(&rms), "permutation {k}: RMS {rms}");
        seconds += pcm.seconds();
    }
    assert!((seconds - 51.6).abs() < 0.1, "{seconds} s");

    // The gusts: mono, 7 permutations.
    let gust = sound::sound(&m, l.details[0].sound.unwrap()).expect("snd!");
    assert_eq!((gust.channels, gust.permutations.len()), (1, 7));
    let pcm = files.decode(&gust, 1).expect("decodes");
    assert!((pcm.seconds() - 8.70).abs() < 0.01, "{}", pcm.seconds());
}

#[test]
fn shared_sound_scenery_decodes() {
    let Some(path) = map("lockout") else { return };
    let m = blam_h2::Map::open(&path).expect("opens");
    let a = sound::ambience(&m).expect("ambience");
    let files = SoundFiles::beside(&path);
    // green_hum's sound lives in shared.map and its gestalt.
    let hum = a
        .emitters
        .iter()
        .find(|e| m.tag_name(e.looping).is_some_and(|n| n.ends_with("green_hum")))
        .expect("green hum");
    let l = sound::looping(&m, hum.looping).expect("lsnd");
    let s = sound::sound(&m, l.tracks[0].looped.unwrap()).expect("snd!");
    assert_eq!(s.permutations.len(), 4);
    for k in 0..4 {
        files.decode(&s, k).expect("decodes");
    }
}
