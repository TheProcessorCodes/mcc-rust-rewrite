//! Halo CE's sound (MCC): the map's `snd!` and `lsnd` tags read by
//! `blam_h1::sound`, the audio decoded from MCC's FMOD banks
//! (`halo1\sound\pc`), handed to the same player as Halo 3's: the tags are
//! turned into Halo 3's [`SoundData`] (looping sounds into
//! `blam_sound::tags::Looping`, sounds into [`SoundInfo`]s whose
//! "permutations" are bank sample ids), so `music.rs` plays the scripts'
//! music, one-shots and lines, with ducking, buses and the trace, unchanged.
//! `docs/format/halo1.md` 13 has the formats and 13.5 the rules.
//!
//! **Playing a Halo CE sound tag at a point** (the effects' explosions):
//! push `halo3_core::script_sound::SoundRequest::Impulse { tag, name, at:
//! Some(position), scale: 1.0 }` into the `ScriptSounds` resource, `tag`
//! being the `snd!`'s tag index in the map (the low 16 bits of its datum)
//! and `position` in Halo world units. Every `snd!` of the map is known, not
//! only the scripts'; it is heard within its tag's distances.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use blam_h1::Map;
use blam_h1::sound::{self as h1, Banks};
use bevy::math::Vec3;
use blam_sound::tags::{Acoustics, BspAcoustics, Detail, Looping, MapSounds, Track};

use super::library::{Gameplay, Loaded, SoundData, SoundInfo};
use super::mixer::{Bus, Clip, RATE};

/// Where a Halo CE map's sound comes from.
pub struct Halo1Sound {
    /// The `.map` (read again on the loader thread).
    pub map: PathBuf,
    /// `game_paths::halo1_sound()`.
    pub banks: Option<PathBuf>,
    /// `game_paths::language()`.
    pub language: &'static str,
}

impl Halo1Sound {
    /// The map at `map`, the banks and language from the setup.
    pub fn of(map: impl Into<PathBuf>) -> Halo1Sound {
        Halo1Sound {
            map: map.into(),
            banks: game_paths::halo1_sound(),
            language: game_paths::language(),
        }
    }
}

/// Starts loading on a thread; the receiver gets one message.
pub(crate) fn load(src: Halo1Sound) -> Receiver<Result<Loaded, String>> {
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new().name("halo1-sound-load".into()).spawn(move || {
        let _ = tx.send(load_now(src));
    });
    if let Err(e) = spawned {
        bevy::log::warn!("sound: no loader thread ({e})");
    }
    rx
}

fn db(linear: f32) -> f32 {
    if linear > 0.0 { 20.0 * linear.log10() } else { -96.0 }
}

/// Halo CE's sound classes as Halo 3's numbers, which pick the bus
/// (`bus_of_class`): 44 (scripted dialogue, player) and 47 (forced
/// unspatialised) go to 47, 35 (ambient computers) to 34; the rest have
/// the same numbers and meanings in both games (19 unit dialogue, 32 music,
/// 33 and 34 ambience, 46 scripted dialogue).
fn class_as_halo3(class: u16) -> u8 {
    match class {
        44 | 47 => 47,
        35 => 34,
        c => c.min(255) as u8,
    }
}

/// Our levels per bus, dB, on top of the tags' linear gains: Halo CE keeps
/// its class levels in the engine, not in a tag. Music and lines as tagged;
/// effects 6 dB down and ambience 12 dB down (our choice: measured headless
/// at the run's start, the trench's background loop at its tag gain sat
/// 1 to 2 dB over the music; Halo 3's classes put ambience 12 dB under it).
fn class_gain_db(class: u8) -> f32 {
    match super::bus_of_class(class as i32) {
        Bus::Music | Bus::Dialogue => 0.0,
        Bus::Effects => -6.0,
        Bus::Ambience => -12.0,
    }
}

fn load_now(src: Halo1Sound) -> Result<Loaded, String> {
    let started = std::time::Instant::now();
    let dir = src.banks.ok_or("no MCC folder for the Halo CE sound banks")?;
    let banks = Banks::open(&dir, src.language).map_err(|e| format!("Halo CE sound banks in {}: {e}", dir.display()))?;
    match (h1::dialog_language(src.language), banks.language) {
        (Some(want), None) => bevy::log::warn!("sound: no Halo CE {want} dialogue bank; lines play in English"),
        (_, Some(l)) => bevy::log::info!("sound: Halo CE {l} dialogue"),
        _ => {}
    }
    let map = Map::open(&src.map).map_err(|e| e.to_string())?;
    let (data, missing) = sound_data(&map, &banks);
    if !missing.is_empty() {
        bevy::log::info!(
            "sound: {} of the map's sounds are in no bank (not played): {}",
            missing.len(),
            missing.iter().take(6).cloned().collect::<Vec<_>>().join(", ")
        );
    }
    bevy::log::info!(
        "sound: Halo CE {}: {} looping sounds, {} sounds, {} music pieces, {} banks; read in {:.2} s",
        map.map_name,
        data.loops.len(),
        data.sounds.len(),
        data.map.music.len(),
        banks.banks.len(),
        started.elapsed().as_secs_f32()
    );
    let banks = Arc::new(banks);
    let clips = super::library::clips_from("halo1-sound-decode", move |id| {
        match banks.decode(id) {
            Ok(pcm) => to_clip(pcm),
            Err(e) => {
                bevy::log::warn!("sound: sample {id:#x}: {e}");
                None
            }
        }
    })
    .map_err(|e| e.to_string())?;
    Ok(Loaded { data: Arc::new(data), clips })
}

/// The map's sounds as Halo 3's player wants them; and the paths of the
/// `snd!`s no bank has.
pub(crate) fn sound_data(map: &Map, banks: &Banks) -> (SoundData, Vec<String>) {
    let mut sounds = HashMap::new();
    let mut missing = Vec::new();
    // Every sound of the map: the scripts', the loops', and any an effect
    // may ask for by tag.
    for (i, t) in map.tags.iter().enumerate() {
        if !t.is(b"snd!") {
            continue;
        }
        let Ok(snd) = h1::sound(map, i as u32) else { continue };
        let samples = banks.samples_of(&t.name);
        if samples.is_empty() {
            missing.push(t.name.clone());
        }
        sounds.insert(i as u32, info(&snd, samples));
    }
    let mut loops = HashMap::new();
    let mut music = Vec::new();
    for (i, t) in map.tags.iter().enumerate() {
        if !t.is(b"lsnd") {
            continue;
        }
        let Ok(l) = h1::looping(map, i as u32) else { continue };
        let looping = as_looping(&l, &sounds);
        if looping.is_music() {
            music.push((i as u32, t.name.clone()));
        }
        loops.insert(i as u32, looping);
    }
    music.sort_by(|a, b| a.1.cmp(&b.1));
    // Each BSP's background sound per cluster, played by the ambience
    // (`background` in lib.rs) through [`place`].
    let h1_bsps = h1::bsp_sounds(map, None).unwrap_or_else(|e| {
        bevy::log::warn!("sound: no background sounds ({e})");
        Vec::new()
    });
    let bsps = h1_bsps
        .iter()
        .map(|b| BspAcoustics {
            bsp: b.bsp,
            palette: b
                .palette
                .iter()
                .map(|bg| Acoustics {
                    background: *bg,
                    fade: 1.0,
                    ..Default::default()
                })
                .collect(),
        })
        .collect();
    let data = SoundData {
        map: MapSounds {
            bsps,
            music,
            script_sounds: h1::script_sounds(map).unwrap_or_default(),
            ..Default::default()
        },
        sounds,
        loops,
        gameplay: Gameplay::default(),
        weapons: HashMap::new(),
        // Under a line: music -3 dB, effects -6 dB, ambience as it is,
        // over half a second (Halo 3's classes' numbers; Halo CE's are in
        // its engine).
        ducking: vec![(Bus::Music, -3.0, [0.5, 0.5]), (Bus::Ambience, 0.0, [0.5, 0.5]), (Bus::Effects, -6.0, [0.5, 0.5])],
        h1_bsps,
    };
    (data, missing)
}

/// The (BSP, cluster, background palette entry) a point is in, for Halo
/// CE, whose scene BSPs have no cluster locator: the first loaded BSP
/// (`mask`, bit = scenario index) whose collision tree puts it in a cluster.
pub(crate) fn place(data: &SoundData, mask: u32, p: Vec3) -> Option<(u32, u32, Option<u32>)> {
    data.h1_bsps.iter().filter(|b| b.bsp < 32 && mask & (1 << b.bsp) != 0).find_map(|b| {
        let c = b.finder.cluster(p)?;
        Some((b.bsp, c, b.clusters.get(c as usize).copied().flatten()))
    })
}

fn info(snd: &h1::Sound, samples: Vec<u32>) -> SoundInfo {
    let class = class_as_halo3(snd.class);
    let (near, far) = snd.distance;
    SoundInfo {
        permutations: samples,
        bounds: (near.min(far), far),
        gain_db: db(snd.gain) + class_gain_db(class),
        class,
        // Full volume within the tag's minimum distance, silent past its
        // maximum (`distance_gain`'s last two).
        distances: [0.0, 0.0, near.min(far), far],
        per_object: 1,
    }
}

/// An `lsnd` as Halo 3's: the same header and track flags; Halo CE has no
/// alternate transitions and no cross-fade time, so a cross-fading track
/// (bit 2) cross-fades over its fade-out time, or 2 s (our choice).
fn as_looping(l: &h1::Looping, sounds: &HashMap<u32, SoundInfo>) -> Looping {
    let first = l.tracks.iter().find_map(|t| t.looped.or(t.start)).and_then(|s| sounds.get(&s));
    let class = first.map_or(0, |s| s.class) as i16;
    let distance = first.map_or((1.0, l.max_distance.max(1.0)), |s| s.bounds);
    Looping {
        tag: l.index,
        name: l.name.clone(),
        flags: l.flags & (Looping::NOT_A_LOOP | Looping::STOPS_MUSIC),
        music_time: 0.0,
        distance,
        class,
        tracks: l
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| Track {
                name: format!("track{}", i + 1),
                flags: t.flags & (Track::FADE_IN | Track::FADE_OUT | Track::CROSSFADE_ALT),
                gain_db: db(t.gain),
                fade_in: t.fade_in,
                fade_out: t.fade_out,
                intro: t.start,
                looped: t.looped,
                outro: t.end,
                alt_loop: t.alt_loop,
                alt_out: t.alt_end,
                alt_trans_in: None,
                alt_trans_out: None,
                alt_fade: if t.fade_out > 0.0 { t.fade_out } else { 2.0 },
                alt_fade_out: 0.0,
            })
            .collect(),
        details: l
            .details
            .iter()
            .enumerate()
            .map(|(i, d)| Detail {
                name: format!("detail{}", i + 1),
                sound: d.sound,
                period: d.period,
                // A further 6 dB down: the trench's background scatters
                // `muffled_explosions` (an effects-class sound) every 0.5
                // to 2 s, which at its own level rose over the music
                // (headless, -12 against -13 dBFS). Our choice.
                gain_db: db(d.gain) - 6.0,
                flags: d.flags,
                yaw: d.yaw,
                pitch: d.pitch,
                distance: d.distance,
            })
            .collect(),
    }
}

/// Decoded PCM to a mixer clip at 48 kHz (Halo CE's banks are 44.1 and
/// 22.05 kHz): Catmull-Rom interpolation per channel. More than two
/// channels fold to stereo (front pair plus half the rest).
pub(crate) fn to_clip(pcm: h1::Pcm) -> Option<Clip> {
    let ch = pcm.channels.max(1) as usize;
    let frames = pcm.frames();
    if frames == 0 || pcm.rate == 0 {
        return None;
    }
    let out_ch = ch.min(2);
    let src = |f: isize, c: usize| -> f32 {
        let f = f.clamp(0, frames as isize - 1) as usize;
        let s = &pcm.samples[f * ch..f * ch + ch];
        if ch <= 2 {
            s[c] as f32
        } else {
            // FL FR then the rest, assumed paired left / right.
            let mut v = s[c] as f32;
            for k in (2 + c..ch).step_by(2) {
                v += 0.5 * s[k] as f32;
            }
            v
        }
    };
    let step = pcm.rate as f64 / RATE as f64;
    let n = ((frames as f64) / step).floor() as usize;
    let mut out = Vec::with_capacity(n * out_ch);
    for i in 0..n {
        let x = i as f64 * step;
        let k = x.floor() as isize;
        let t = (x - k as f64) as f32;
        for c in 0..out_ch {
            let (p0, p1, p2, p3) = (src(k - 1, c), src(k, c), src(k + 1, c), src(k + 2, c));
            let v = p1 + 0.5 * t * (p2 - p0 + t * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3 + t * (3.0 * (p1 - p2) + p3 - p0)));
            out.push(v.clamp(-32768.0, 32767.0) as i16);
        }
    }
    Some(Clip {
        channels: out_ch as u16,
        samples: out.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resamples_to_48k() {
        // One second of a 441 Hz sine at 44.1 kHz becomes one second at 48.
        let pcm = h1::Pcm {
            rate: 44_100,
            channels: 1,
            samples: (0..44_100).map(|i| ((i as f32 * 441.0 / 44_100.0 * std::f32::consts::TAU).sin() * 16000.0) as i16).collect(),
        };
        let c = to_clip(pcm).unwrap();
        assert_eq!(c.channels, 1);
        assert!((c.frames() as i32 - 48_000).abs() <= 1, "{}", c.frames());
        // Still a sine of the same pitch: 441 zero crossings up, about.
        let ups = c.samples.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count();
        assert!((440..=442).contains(&ups), "{ups}");
    }

    #[test]
    fn classes() {
        assert_eq!(super::super::bus_of_class(class_as_halo3(44) as i32), Bus::Dialogue);
        assert_eq!(super::super::bus_of_class(class_as_halo3(32) as i32), Bus::Music);
        assert_eq!(super::super::bus_of_class(class_as_halo3(35) as i32), Bus::Ambience);
        assert_eq!(super::super::bus_of_class(class_as_halo3(1) as i32), Bus::Effects);
    }

    /// The Maw's tags as the player sees them, and the run's first bar of
    /// music through the decoder and the resampler. Skips quietly without
    /// MCC.
    #[test]
    fn the_maw() {
        let (Some(path), Some(dir)) = (game_paths::halo1_map("d40"), game_paths::halo1_sound()) else { return };
        let Ok(banks) = Banks::open(&dir, "english") else { return };
        let map = Map::open(&path).unwrap();
        let (data, missing) = sound_data(&map, &banks);
        assert!(missing.len() <= 4, "{missing:?}");
        let tag = |name: &str| map.tags.iter().position(|t| t.name == name).unwrap() as u32;
        let d40_07 = &data.loops[&tag(r"levels\d40\music\d40_07")];
        assert!(d40_07.is_music() && !d40_07.stops_music() && !d40_07.not_a_loop());
        let t = &d40_07.tracks[0];
        assert!(t.fades_out() && t.crossfades_alt() && !t.fades_in());
        assert_eq!(t.alt_fade, 6.0);
        assert!(data.loops[&tag(r"levels\d40\music\d40_09")].not_a_loop());
        assert!(data.map.music.iter().any(|(i, _)| *i == tag(r"levels\d40\music\d40_08")));
        let bars = &data.sounds[&t.looped.unwrap()];
        assert_eq!(bars.permutations.len(), 5);
        assert_eq!(bars.class, 32);
        let line = &data.sounds[&tag(r"sound\dialog\d40\d40_440_cortana")];
        assert_eq!(super::super::bus_of_class(line.class as i32), Bus::Dialogue);
        assert!((line.gain_db - db(0.85)).abs() < 1e-3);
        let intro = data.sounds[&t.intro.unwrap()].permutations[0];
        let clip = to_clip(banks.decode(intro).unwrap()).unwrap();
        // 204,288 frames at 44.1 kHz: 4.632 s.
        assert_eq!(clip.channels, 2);
        assert!((clip.seconds() - 4.632).abs() < 0.002, "{}", clip.seconds());
    }
}
