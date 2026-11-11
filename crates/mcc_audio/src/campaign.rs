//! Halo 3's campaign music on any map, by hand: U starts it (or stops
//! it, fading out over a second), Shift+U skips to the next piece, Ctrl+U to a random one. When a
//! piece ends by itself (its music time) the next one starts. The piece's
//! name shows at the top of the screen for a few seconds.
//!
//! The pieces are the `lsnd` tags under a `music` folder of the campaign
//! maps in the player's MCC (`halo3\maps\005_intro.map` ... `130_epilogue`),
//! opened read-only beside the loaded map the first time U is pressed, on
//! a thread (a second or two), and kept for the rest of the session. Their
//! audio is in the same FMOD bank as every Halo 3 sound and decodes as a
//! piece starts, so this plays on Halo CE, Halo 2 and Reach maps too. The
//! pieces play by the same rules as the map's own music (`music.rs`), on
//! the music bus, with the map's own music player left alone.

use std::collections::HashSet;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use blam_cache::CacheFile;
use blam_sound::Library;
use blam_sound::tags::{Looping, SoundTags};

use super::library::{SoundData, SoundInfo, clips_from, to_clip};
use super::mixer::Mixer;
use super::music::{ClassGains, MusicPlayer};
use super::{Listener, Sounds};

/// The campaign maps whose music is offered, in the order they are played.
pub const MISSIONS: [&str; 11] = [
    "005_intro",
    "010_jungle",
    "020_base",
    "030_outskirts",
    "040_voi",
    "050_floodvoi",
    "070_waste",
    "100_citadel",
    "110_hc",
    "120_halo",
    "130_epilogue",
];

/// Seconds the music takes to fade out when U stops it.
const STOP_FADE: f32 = 1.0;

/// Seconds a piece's name stays on screen.
const LABEL_SECONDS: f32 = 4.0;

/// The campaign's pieces, read.
pub(crate) struct Side {
    sounds: Sounds,
    /// (key into `sounds`' loops, what the label says), in mission order.
    tracks: Vec<(u32, String)>,
}

enum State {
    Idle,
    Loading(Mutex<Receiver<Result<Side, String>>>),
    Ready(Arc<Side>),
    Failed,
}

/// The campaign music: what is read, its own player, and what plays.
#[derive(Resource)]
pub(crate) struct CampaignMusic {
    state: State,
    player: MusicPlayer,
    /// Wanted on (U); the next piece starts when one ends by itself.
    on: bool,
    /// The piece playing or last played, by index in `Side::tracks`.
    track: Option<usize>,
    /// Asked for while loading: the piece to start once read.
    pending: Option<Pick>,
    /// A piece just started: show its name.
    announce: bool,
    rng: u32,
}

impl Default for CampaignMusic {
    fn default() -> Self {
        CampaignMusic {
            state: State::Idle,
            player: MusicPlayer::default(),
            on: false,
            track: None,
            pending: None,
            announce: false,
            rng: 0x9E37_79B9,
        }
    }
}

/// Which piece a key asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    /// The one playing last (the first, the first time).
    Same,
    Next,
    Random,
}

/// The label at the top of the screen: the piece's name.
#[derive(Component)]
pub(crate) struct TrackLabel(f32);

impl CampaignMusic {
    /// A new map: what plays stops (the mixer has stopped every voice), what
    /// is read stays.
    pub(crate) fn reset_player(&mut self) {
        self.player = MusicPlayer::default();
        self.on = false;
        self.pending = None;
    }

    fn random(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    fn choose(&mut self, pick: Pick, count: usize) -> usize {
        match (pick, self.track) {
            (_, _) if count == 0 => 0,
            (Pick::Same, Some(t)) => t % count,
            (Pick::Same, None) => 0,
            (Pick::Next, Some(t)) => (t + 1) % count,
            (Pick::Next, None) => 0,
            (Pick::Random, current) => {
                let mut n = self.random() as usize % count;
                if count > 1 && Some(n) == current {
                    n = (n + 1) % count;
                }
                n
            }
        }
    }
}

/// Reads the campaign maps' music beside `maps` (the Halo 3 maps folder).
fn read(maps: std::path::PathBuf) -> Result<Side, String> {
    let started = std::time::Instant::now();
    let library = Library::open_beside(&maps).map_err(|e| e.to_string())?;
    let mut data = SoundData::default();
    let mut tracks = Vec::new();
    let mut names = HashSet::new();
    let mut classes = Vec::new();
    for (n, mission) in MISSIONS.iter().enumerate() {
        let path = maps.join(format!("{mission}.map"));
        if !path.is_file() {
            continue;
        }
        let cache = match CacheFile::open(&path) {
            Ok(c) => c,
            Err(e) => {
                warn!("campaign music: {mission} not read ({e})");
                continue;
            }
        };
        let Ok(tags) = SoundTags::new(&cache) else { continue };
        if classes.is_empty() {
            classes = tags.classes().unwrap_or_default();
        }
        let Ok(map) = tags.map_sounds() else { continue };
        // Tag indices are per map: the keys here are the mission's number
        // over its tag index.
        let key = |t: u32| (n as u32 + 1) << 20 | t;
        for (lsnd, name) in &map.music {
            if !names.insert(name.to_ascii_lowercase()) {
                continue;
            }
            let Ok(looping) = tags.looping(*lsnd) else { continue };
            if looping.not_a_loop() || looping.tracks.is_empty() {
                continue;
            }
            let mut sounds = Vec::new();
            for t in &looping.tracks {
                sounds.extend(
                    [t.intro, t.looped, t.outro, t.alt_loop, t.alt_out, t.alt_trans_in, t.alt_trans_out]
                        .into_iter()
                        .flatten(),
                );
            }
            for s in &sounds {
                if let Ok(snd) = tags.sound(*s) {
                    data.sounds.insert(key(*s), SoundInfo::new(&snd, &classes, &library));
                }
            }
            // What it plays: the first track's loop (`misc\halo3_trench`).
            let heard = looping.tracks[0]
                .looped
                .or(looping.tracks[0].intro)
                .and_then(|s| cache.tags.get(s))
                .map(|t| heard_name(t.name()))
                .unwrap_or_default();
            let piece = name.rsplit('\\').next().unwrap_or(name);
            let label = if heard.is_empty() { piece.to_string() } else { format!("{piece} ({heard})") };
            data.loops.insert(key(*lsnd), rekey(looping, key));
            tracks.push((key(*lsnd), label));
        }
    }
    if tracks.is_empty() {
        return Err("no campaign maps with music in the Halo 3 maps folder".into());
    }
    let clips = clips_from("halo-campaign-music", move |id| library.decode(id).ok().and_then(to_clip)).map_err(|e| e.to_string())?;
    info!("campaign music: {} pieces read in {:.1} s", tracks.len(), started.elapsed().as_secs_f32());
    Ok(Side {
        sounds: Sounds {
            data: Arc::new(data),
            clips,
        },
        tracks,
    })
}

/// A music sound's name for the label: its folder, past the track and
/// slot it is filed under (`sound\music\misc\halo3_trench\track1\loop` is
/// `halo3_trench`).
fn heard_name(path: &str) -> String {
    const SLOTS: [&str; 7] = ["in", "loop", "out", "alt_loop", "alt_out", "alt_trans_in", "alt_trans_out"];
    path.rsplit('\\')
        .find(|p| !SLOTS.contains(p) && !p.starts_with("track"))
        .unwrap_or("")
        .to_string()
}

/// A looping sound with its sounds' tag indices turned into `key`s.
fn rekey(mut looping: Looping, key: impl Fn(u32) -> u32) -> Looping {
    for t in &mut looping.tracks {
        for s in [
            &mut t.intro,
            &mut t.looped,
            &mut t.outro,
            &mut t.alt_loop,
            &mut t.alt_out,
            &mut t.alt_trans_in,
            &mut t.alt_trans_out,
        ] {
            *s = s.map(&key);
        }
    }
    for d in &mut looping.details {
        d.sound = d.sound.map(&key);
    }
    looping.tag = key(looping.tag);
    looping
}

/// U, Shift+U and Ctrl+U; reads the music the first time.
pub(crate) fn keys(keys: Res<ButtonInput<KeyCode>>, mixer: Res<Mixer>, mut music: ResMut<CampaignMusic>) {
    if !keys.just_pressed(KeyCode::KeyU) {
        return;
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let pick = if ctrl {
        Pick::Random
    } else if shift {
        Pick::Next
    } else {
        Pick::Same
    };
    let music = &mut *music;
    match &music.state {
        State::Idle => {
            let Some(maps) = game_paths::halo3_maps() else {
                warn!("campaign music: no Halo 3 maps folder");
                music.state = State::Failed;
                return;
            };
            let (tx, rx) = channel();
            let spawned = std::thread::Builder::new().name("halo-campaign-music".into()).spawn(move || {
                let _ = tx.send(read(maps));
            });
            match spawned {
                Ok(_) => {
                    info!("campaign music: reading the campaign maps' music");
                    music.state = State::Loading(Mutex::new(rx));
                    music.pending = Some(pick);
                    music.on = true;
                }
                Err(e) => {
                    warn!("campaign music: {e}");
                    music.state = State::Failed;
                }
            }
        }
        State::Loading(_) => {
            // Pressed again while reading: off, or another pick.
            if pick == Pick::Same {
                music.on = !music.on;
                music.pending = music.on.then_some(Pick::Same);
            } else {
                music.on = true;
                music.pending = Some(pick);
            }
        }
        State::Ready(side) => {
            let side = side.clone();
            if pick == Pick::Same && music.on {
                // Off: a fade, not the piece's own ending, which lets a
                // loop play on to its end (minutes) and sounds like U did
                // nothing.
                music.on = false;
                music.pending = None;
                music.player.fade_all(STOP_FADE);
                info!("campaign music: off");
                return;
            }
            music.on = true;
            music.player.end_all(&side.sounds, &mixer, true);
            let n = music.choose(pick, side.tracks.len());
            start(music, &side, n);
        }
        State::Failed => warn!("campaign music: not available"),
    }
}

/// Starts piece `n`.
fn start(music: &mut CampaignMusic, side: &Side, n: usize) {
    let Some((lsnd, label)) = side.tracks.get(n) else { return };
    music.track = Some(n);
    music.announce = true;
    music.player.start_by_hand(&side.sounds, *lsnd);
    info!("campaign music: {label} ({} of {})", n + 1, side.tracks.len());
}

/// Takes in what was read, plays on, starts the next piece when one ends,
/// and shows the name of a piece as it starts.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update(
    mut commands: Commands,
    time: Res<Time>,
    mixer: Res<Mixer>,
    listener: Res<Listener>,
    gains: Option<Res<ClassGains>>,
    mut music: ResMut<CampaignMusic>,
    mut labels: Query<(Entity, &mut TrackLabel, &mut Text, &mut TextColor)>,
) {
    let music = &mut *music;
    if let State::Loading(rx) = &music.state {
        let got = rx.lock().unwrap_or_else(|e| e.into_inner()).try_recv();
        match got {
            Ok(Ok(side)) => {
                music.state = State::Ready(Arc::new(side));
            }
            Ok(Err(e)) => {
                warn!("campaign music: {e}");
                music.state = State::Failed;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(_) => music.state = State::Failed,
        }
    }
    let State::Ready(side) = &music.state else { return };
    let side = side.clone();
    let dt = time.delta_secs();
    if let Some(pick) = music.pending.take()
        && music.on
    {
        let n = music.choose(pick, side.tracks.len());
        start(music, &side, n);
    } else if music.on && !music.player.busy() {
        // The piece ended by itself: the next.
        let n = music.choose(Pick::Next, side.tracks.len());
        start(music, &side, n);
    }
    let default_gains = ClassGains::default();
    let gains = gains.as_deref().unwrap_or(&default_gains);
    music.player.advance(dt, &side.sounds, &listener, &mixer, gains);

    // The label.
    if std::mem::take(&mut music.announce) && let Some((_, name)) = music.track.and_then(|t| side.tracks.get(t)) {
        let text = format!("Halo 3 music: {name}");
        match labels.iter_mut().next() {
            Some((_, mut l, mut t, _)) => {
                l.0 = LABEL_SECONDS;
                t.0 = text;
            }
            None => {
                commands.spawn((
                    TrackLabel(LABEL_SECONDS),
                    Text::new(text),
                    TextFont::from_font_size(22.0),
                    TextColor(Color::srgba(0.95, 0.97, 1.0, 1.0)),
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(18.0),
                        left: Val::Px(24.0),
                        ..default()
                    },
                ));
            }
        }
    }
    for (e, mut l, _, mut colour) in &mut labels {
        l.0 -= dt;
        if l.0 <= 0.0 {
            commands.entity(e).despawn();
        } else {
            colour.0 = colour.0.with_alpha(l.0.min(1.0));
        }
    }
}

/// The pieces a player can pick, for tests: (label) in order.
#[cfg(test)]
pub(crate) fn labels(side: &Side) -> Vec<&str> {
    side.tracks.iter().map(|(_, l)| l.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// The campaign's music reads from the player's maps; 120_halo's
    /// Warthog run is among it, and every piece's sounds are keyed apart
    /// from other missions'.
    #[test]
    fn reads_the_campaign_music() {
        let Some(maps) = game_paths::halo3_maps().filter(|m| m.join("120_halo.map").is_file()) else {
            eprintln!("skipped: Halo 3 is not installed");
            return;
        };
        let side = read(maps).unwrap();
        let labels = labels(&side);
        assert!(labels.len() > 50, "{} pieces", labels.len());
        assert!(labels.iter().any(|l| l.starts_with("120_music_15 (halo3_trench")), "{labels:?}");
        let mut keys: HashMap<u32, usize> = HashMap::new();
        for (k, _) in &side.tracks {
            *keys.entry(*k).or_default() += 1;
            let l = &side.sounds.data.loops[k];
            for t in &l.tracks {
                for s in [t.intro, t.looped].into_iter().flatten() {
                    assert_eq!(s >> 20, k >> 20, "a sound keyed into another mission");
                }
            }
        }
        assert!(keys.values().all(|n| *n == 1));
        // The Warthog run's loop has its bank samples.
        let (k, _) = side.tracks.iter().find(|(_, l)| l.starts_with("120_music_15")).unwrap();
        let looped = side.sounds.data.loops[k].tracks[0].looped.unwrap();
        assert!(!side.sounds.data.sounds[&looped].permutations.is_empty());
    }
}
