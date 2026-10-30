//! Loading the map's sounds off the main thread, and decoding clips on
//! demand on a worker thread (`blam_sound`).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use blam_cache::CacheFile;
use blam_sound::tags::{Looping, MapSounds, Sound, SoundClass, SoundTags};
use blam_sound::weapons::{WeaponSoundReader, WeaponSounds};
use blam_sound::{Library, Pcm};

use super::mixer::{Clip, RATE};

/// What the viewer needs to know about a `snd!` tag.
#[derive(Clone, Debug, Default)]
pub struct SoundInfo {
    pub permutations: Vec<u32>,
    /// (full-volume distance, silent distance), world units.
    pub bounds: (f32, f32),
    pub gain_db: f32,
    /// The four distances in effect (the sound's own, else its class's):
    /// `blam_sound::tags::distance_gain`.
    pub distances: [f32; 4],
    /// Copies one object may play at once (its class's limit), at least 1.
    pub per_object: usize,
}

impl SoundInfo {
    fn new(snd: &Sound, classes: &[SoundClass], library: &Library) -> SoundInfo {
        let class = classes.get(snd.class as usize);
        SoundInfo {
            permutations: snd
                .permutations
                .iter()
                .map(|p| p.fmod_id)
                .filter(|id| library.sample(*id).is_some())
                .collect(),
            bounds: snd.playback.bounds(),
            gain_db: snd.playback.gain_db,
            distances: snd.playback.distances_with(class),
            per_object: class.map_or(1, |c| c.max_per_object.max(1) as usize),
        }
    }
}

/// The gameplay one-shots and loops the viewer picks from the bank by
/// folder (they are not in every map's tags).
#[derive(Clone, Debug, Default)]
pub struct Gameplay {
    pub roll: Vec<u32>,
    pub grind: Vec<u32>,
    pub land: Vec<u32>,
    /// Played under `land`: the wheels hitting the ground.
    pub land_wheels: Vec<u32>,
}

/// The map's sounds, resolved from its tags.
#[derive(Default)]
pub struct SoundData {
    pub map: MapSounds,
    pub sounds: HashMap<u32, SoundInfo>,
    pub loops: HashMap<u32, Looping>,
    pub gameplay: Gameplay,
    /// Every `weap` tag's sounds, by tag index.
    pub weapons: HashMap<u32, WeaponSounds>,
}

impl SoundData {
    pub fn tag_name(&self, lsnd: u32) -> &str {
        self.loops
            .get(&lsnd)
            .map_or("?", |l| l.name.rsplit('\\').next().unwrap_or(&l.name))
    }
}

enum Slot {
    Pending,
    Ready(Arc<Clip>),
    Failed,
}

/// Decoded clips by FMOD id; asking for a missing one queues its decode.
pub struct Clips {
    slots: Mutex<HashMap<u32, Slot>>,
    requests: Mutex<Sender<u32>>,
}

impl Clips {
    /// The clip, or `None` while it is being decoded (or failed).
    pub fn get(&self, id: u32) -> Option<Arc<Clip>> {
        let mut slots = self.slots.lock().unwrap();
        match slots.get(&id) {
            Some(Slot::Ready(c)) => Some(c.clone()),
            Some(_) => None,
            None => {
                slots.insert(id, Slot::Pending);
                let _ = self.requests.lock().unwrap().send(id);
                None
            }
        }
    }

    /// Every clip of `ids` once all have decoded (failed ones left out),
    /// `None` until then.
    pub fn all(&self, ids: &[u32]) -> Option<Vec<Arc<Clip>>> {
        let mut out = Vec::with_capacity(ids.len());
        let mut waiting = false;
        for &id in ids {
            match self.get(id) {
                Some(c) => out.push(c),
                None => waiting |= !self.failed(id),
            }
        }
        (!waiting).then_some(out)
    }

    pub fn failed(&self, id: u32) -> bool {
        matches!(self.slots.lock().unwrap().get(&id), Some(Slot::Failed))
    }

    /// (decoded clips, MiB of PCM kept)
    pub fn stats(&self) -> (usize, f32) {
        let slots = self.slots.lock().unwrap();
        let mut n = 0;
        let mut bytes = 0;
        for s in slots.values() {
            if let Slot::Ready(c) = s {
                n += 1;
                bytes += c.samples.len() * 2;
            }
        }
        (n, bytes as f32 / (1 << 20) as f32)
    }
}

/// Turns decoded PCM into a clip: 48 kHz is all the bank holds; more than
/// two channels are folded to stereo.
fn to_clip(pcm: Pcm) -> Option<Clip> {
    if pcm.rate != RATE || pcm.samples.is_empty() {
        return None;
    }
    let pcm = if pcm.channels > 2 {
        pcm.to_stereo()
    } else {
        pcm
    };
    let samples: Vec<i16> = pcm
        .samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();
    Some(Clip {
        channels: pcm.channels,
        samples: samples.into(),
    })
}

/// Everything the loader thread hands back.
pub struct Loaded {
    pub data: Arc<SoundData>,
    pub clips: Arc<Clips>,
}

/// Starts loading on a thread; the receiver gets one message.
pub fn load(cache: &'static CacheFile) -> Receiver<Result<Loaded, String>> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("halo-sound-load".into())
        .spawn(move || {
            let _ = tx.send(load_now(cache).map_err(|e| format!("{e:#}")));
        })
        .expect("spawn the sound loader");
    rx
}

fn load_now(cache: &'static CacheFile) -> anyhow::Result<Loaded> {
    let started = std::time::Instant::now();
    let maps = cache.path().parent().unwrap_or(Path::new("."));
    let library = Library::open_beside(maps)?;
    let tags = SoundTags::new(cache)?;
    let map = tags.map_sounds()?;
    let classes = tags.classes().unwrap_or_default();
    let weapons = WeaponSoundReader::new(cache).all().unwrap_or_else(|e| {
        bevy::log::warn!("sound: weapons not read ({e})");
        HashMap::new()
    });

    let mut loops = HashMap::new();
    let mut wanted: Vec<u32> = map.scenery.iter().filter_map(|s| s.looping).collect();
    for b in &map.bsps {
        wanted.extend(
            b.palette
                .iter()
                .flat_map(|a| [a.background, a.inside])
                .flatten(),
        );
    }
    wanted.extend(map.music.iter().map(|(t, _)| *t));
    // What the scripts start: their looping sounds here, their one-shots
    // below.
    let is_class = |t: u32, code: &str| cache.tags.get(t).is_some_and(|e| cache.tags.class_of(e).code.as_str() == code);
    wanted.extend(map.script_sounds.iter().copied().filter(|t| is_class(*t, "lsnd")));
    wanted.extend(weapons.values().flat_map(|w| w.all_loops()));
    let mut seen = HashSet::new();
    let mut sounds = HashMap::new();
    for l in wanted {
        if !seen.insert(l) {
            continue;
        }
        let Ok(looping) = tags.looping(l) else {
            continue;
        };
        let snds = looping
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
            .chain(looping.details.iter().map(|d| d.sound))
            .flatten();
        for s in snds {
            if sounds.contains_key(&s) {
                continue;
            }
            if let Ok(snd) = tags.sound(s) {
                sounds.insert(s, SoundInfo::new(&snd, &classes, &library));
            }
        }
        loops.insert(l, looping);
    }
    let one_shots = map.script_sounds.iter().copied().filter(|t| is_class(*t, "snd!"));
    for s in weapons.values().flat_map(|w| w.all_sounds()).chain(one_shots) {
        if !sounds.contains_key(&s)
            && let Ok(snd) = tags.sound(s)
        {
            sounds.insert(s, SoundInfo::new(&snd, &classes, &library));
        }
    }

    let under = |folder: &str| -> Vec<u32> {
        library
            .sfx
            .infos
            .iter()
            .filter(|i| i.path.starts_with(folder))
            .map(|i| i.id)
            .collect()
    };
    let gameplay = Gameplay {
        roll: under(
            r"data\sound\materials\havok\tire\tire_looping_hard_surface\tire_looping_hard_surface\loop\",
        ),
        grind: under(r"data\sound\vehicles\damage_effects\vehicle_scrape_low\"),
        land: under(r"data\sound\materials\havok\wooden_palette_hits\"),
        land_wheels: under(r"data\sound\materials\havok\tire\tire_hits\"),
    };

    let (req_tx, req_rx) = channel::<u32>();
    let clips = Arc::new(Clips {
        slots: Mutex::new(HashMap::new()),
        requests: Mutex::new(req_tx),
    });
    let worker = clips.clone();
    std::thread::Builder::new()
        .name("halo-sound-decode".into())
        .spawn(move || {
            while let Ok(id) = req_rx.recv() {
                let slot = match library.decode(id).ok().and_then(to_clip) {
                    Some(c) => Slot::Ready(Arc::new(c)),
                    None => Slot::Failed,
                };
                worker.slots.lock().unwrap().insert(id, slot);
            }
        })?;
    bevy::log::info!(
        "sound: {} sound scenery, {} looping sounds, {} sounds, {} music tracks, {} weapons; bank and tags read in {:.2} s",
        map.scenery.len(),
        loops.len(),
        sounds.len(),
        map.music.len(),
        weapons.len(),
        started.elapsed().as_secs_f32()
    );
    Ok(Loaded {
        data: Arc::new(SoundData {
            map,
            sounds,
            loops,
            gameplay,
            weapons,
        }),
        clips,
    })
}
