//! Looping sounds the way Halo 3 plays them, for music and for anything
//! else a script starts: the intro, then the loop's permutations back to
//! back at random, the alternate loop on a loop boundary (through its
//! transition) or by cross-fading, and on stop the outro (the alternate
//! outro when on the alternate loop) or a fade. Pieces that "stop music"
//! end the others; a piece with a music time ends by itself after it.
//! `docs/format/sound.md` 5 and 8 have the tag fields and the rules.
//!
//! Driven by the map's scripts (`halo3_core::script_sound`, queued by
//! `halo3_script`) and by hand: M starts the map's showpiece (or ends what
//! is playing), Shift+M the next piece, N toggles the alternate loop of the
//! newest piece, `--music NAME|N` starts one at launch. One-shots a script
//! starts (`sound_impulse_start`) play here too, and so do the lines of
//! dialogue (`ai_play_line*`, from the language bank), with the other
//! buses ducked under them (`sound.md` 11).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::prelude::*;
use blam_sound::tags::{Looping, Track, distance_gain};
use halo3_core::script_sound::{ScriptSounds, SoundRequest};

use super::mixer::{Clip, Mixer, Play, VoiceId};
use super::{Listener, Sounds, attenuation, bus_of_class, db};

/// The clips of one track, once all are decoded.
struct TrackClips {
    intro: Vec<Arc<Clip>>,
    looped: Vec<Arc<Clip>>,
    outro: Vec<Arc<Clip>>,
    alt_loop: Vec<Arc<Clip>>,
    alt_out: Vec<Arc<Clip>>,
    alt_in: Vec<Arc<Clip>>,
    alt_back: Vec<Arc<Clip>>,
}

/// Every track's clips, `None` while any is still decoding.
fn piece_clips(sounds: &Sounds, looping: &Looping) -> Option<Vec<TrackClips>> {
    let get = |s: Option<u32>| -> Option<Vec<Arc<Clip>>> {
        match s.and_then(|s| sounds.data.sounds.get(&s)) {
            Some(snd) => sounds.clips.all(&snd.permutations),
            None => Some(Vec::new()),
        }
    };
    let mut out = Vec::new();
    let mut ready = true;
    for t in &looping.tracks {
        // Ask for every slot at once so they decode together.
        let [intro, looped, outro, alt_loop, alt_out, alt_in, alt_back] = t.sounds().map(get);
        let (
            Some(intro),
            Some(looped),
            Some(outro),
            Some(alt_loop),
            Some(alt_out),
            Some(alt_in),
            Some(alt_back),
        ) = (intro, looped, outro, alt_loop, alt_out, alt_in, alt_back)
        else {
            ready = false;
            continue;
        };
        out.push(TrackClips {
            intro,
            looped,
            outro,
            alt_loop,
            alt_out,
            alt_in,
            alt_back,
        });
    }
    ready.then_some(out)
}

/// A voice and the level it is ramping to.
struct Fader {
    id: VoiceId,
    level: f32,
    target: f32,
    /// Level change per second.
    rate: f32,
    /// Stop the voice once the level is 0.
    end_at_zero: bool,
}

impl Fader {
    fn new(id: VoiceId, level: f32) -> Fader {
        Fader {
            id,
            level,
            target: 1.0,
            rate: 0.0,
            end_at_zero: false,
        }
    }
    fn fade_to(&mut self, target: f32, seconds: f32) {
        self.target = target;
        self.rate = if seconds > 0.0 { 1.0 / seconds } else { f32::INFINITY };
    }
    fn step(&mut self, dt: f32) {
        let d = self.target - self.level;
        let max = self.rate * dt;
        self.level = if d.abs() <= max { self.target } else { self.level + max * d.signum() };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Waiting for its clips to decode.
    Waiting,
    Playing,
    /// Stopped: playing its outro, fading, or finishing its last loop.
    Ending,
}

/// What started a piece.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    Script,
    Hand,
}

struct Piece {
    lsnd: u32,
    name: String,
    from: Origin,
    state: State,
    alt: bool,
    /// Per track: its voices, the newest (the one that plays on) last.
    tracks: Vec<Vec<Fader>>,
    /// Seconds since it started playing.
    age: f32,
    at: Option<Vec3>,
    scale: f32,
    music: bool,
    /// When it started, by the player's clock (newest piece for N).
    serial: u64,
}

impl Piece {
    fn voices(&self) -> impl Iterator<Item = &Fader> {
        self.tracks.iter().flatten()
    }
}

/// A one-shot a script started, for placing it and stopping it.
struct Impulse {
    id: VoiceId,
    tag: u32,
    at: Option<Vec3>,
    gain: f32,
    distances: [f32; 4],
    /// A line of dialogue: the other buses duck under it.
    dialogue: bool,
}

/// A one-shot waiting for its clip to decode.
struct PendingImpulse {
    tag: u32,
    /// The tag path, for the log.
    name: String,
    at: Option<Vec3>,
    scale: f32,
    waited: f32,
}

/// Script sound class gains (`sound_class_set_gain`), 1 unless a script
/// changed them. `music` scales the music, `amb` the ambience.
#[derive(Resource, Default)]
pub(crate) struct ClassGains(HashMap<String, Fader>);

impl ClassGains {
    pub(crate) fn get(&self, class: &str) -> f32 {
        self.0.get(class).map_or(1.0, |f| f.level)
    }
    fn set(&mut self, class: &str, gain: f32, seconds: f32) {
        let f = self.0.entry(class.to_string()).or_insert_with(|| Fader::new(0, 1.0));
        f.fade_to(gain.clamp(0.0, 4.0), seconds);
    }
    fn step(&mut self, dt: f32) {
        for f in self.0.values_mut() {
            f.step(dt);
        }
    }
}

/// The classes `sound_class_set_gain` changes here; the others (`object`,
/// `vehicle`, `dialog`, `""` for everything) are logged and left alone.
const CLASSES: [&str; 2] = ["music", "amb"];

#[derive(Resource, Default)]
pub(crate) struct MusicPlayer {
    pieces: Vec<Piece>,
    impulses: Vec<Impulse>,
    pending: Vec<PendingImpulse>,
    /// `--music NAME|N`, started once the sounds are in.
    wanted_at_start: Option<String>,
    started_once: bool,
    /// The piece M / Shift+M last started, by index in the map's music.
    hand_track: Option<usize>,
    serial: u64,
    rng: u32,
    warned: HashSet<String>,
    /// The music, ambience and effects buses' ducking under dialogue
    /// (`SoundData::ducking`), once the sounds are in.
    ducks: Vec<(super::mixer::Bus, Fader)>,
}

impl MusicPlayer {
    /// `wanted_at_start`: `--music NAME|N`.
    pub(crate) fn new(wanted_at_start: Option<String>) -> MusicPlayer {
        MusicPlayer {
            wanted_at_start,
            ..Default::default()
        }
    }

    /// A new player for the next map, with the same `--music`.
    pub(crate) fn fresh(&self) -> MusicPlayer {
        MusicPlayer::new(self.wanted_at_start.clone())
    }

    fn random(&mut self) -> u32 {
        if self.rng == 0 {
            self.rng = 0x2F6B_1A3D;
        }
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    fn pick(&mut self, v: &[Arc<Clip>]) -> Vec<Arc<Clip>> {
        if v.is_empty() {
            return Vec::new();
        }
        let i = self.random() as usize % v.len();
        vec![v[i].clone()]
    }

    fn warn_once(&mut self, key: String, message: impl FnOnce() -> String) {
        if self.warned.insert(key) {
            info!("{}", message());
        }
    }

    /// What is playing, for the trace: `name (state, alt)`.
    pub(crate) fn describe(&self) -> String {
        let v: Vec<String> = self
            .pieces
            .iter()
            .map(|p| {
                format!(
                    "{}{}{}",
                    p.name,
                    match p.state {
                        State::Waiting => " (decoding)",
                        State::Playing => "",
                        State::Ending => " (ending)",
                    },
                    if p.alt { " alt" } else { "" }
                )
            })
            .collect();
        v.join(", ")
    }

    /// Starts a looping sound (it begins once its clips are decoded).
    fn start(&mut self, sounds: &Sounds, lsnd: u32, from: Origin, at: Option<Vec3>, scale: f32) {
        let Some(looping) = sounds.data.loops.get(&lsnd) else {
            self.warn_once(format!("lsnd {lsnd}"), || format!("music: looping sound #{lsnd} was not loaded; not played"));
            return;
        };
        if self.pieces.iter().any(|p| p.lsnd == lsnd && p.state != State::Ending) {
            return;
        }
        self.serial += 1;
        self.pieces.push(Piece {
            lsnd,
            name: sounds.data.tag_name(lsnd).to_string(),
            from,
            state: State::Waiting,
            alt: false,
            tracks: Vec::new(),
            age: 0.0,
            at,
            scale,
            music: looping.is_music(),
            serial: self.serial,
        });
    }

    /// The piece's voices start: the intro, then the loop (`Waiting` ->
    /// `Playing`).
    fn begin(&mut self, i: usize, sounds: &Sounds, mixer: &Mixer) -> bool {
        let lsnd = self.pieces[i].lsnd;
        let Some(looping) = sounds.data.loops.get(&lsnd) else { return false };
        let Some(clips) = piece_clips(sounds, looping) else { return false };
        let alt = self.pieces[i].alt;
        let once = looping.not_a_loop();
        let mut tracks = Vec::new();
        for (t, c) in looping.tracks.iter().zip(&clips) {
            let mut queue = self.pick(&c.intro);
            let mut looped = if alt && !c.alt_loop.is_empty() { c.alt_loop.clone() } else { c.looped.clone() };
            if once {
                queue.extend(self.pick(&looped));
                looped = Vec::new();
            }
            if queue.is_empty() && looped.is_empty() {
                tracks.push(Vec::new());
                continue;
            }
            let id = mixer.play(Play {
                bus: bus_of_class(looping.class as i32),
                queue,
                looping: looped,
                gain: [0.0; 2],
                rate: 1.0,
            });
            let mut f = Fader::new(id, 1.0);
            if t.fades_in() {
                f.level = 0.0;
                f.fade_to(1.0, t.fade_in);
            }
            tracks.push(vec![f]);
        }
        let first = &clips[0];
        let p = &mut self.pieces[i];
        p.tracks = tracks;
        p.state = State::Playing;
        p.age = 0.0;
        info!(
            "music: start {} ({}; {:.0} s intro, {} loop permutations{}{}{})",
            p.name,
            match p.from {
                Origin::Script => "script",
                Origin::Hand => "by hand",
            },
            first.intro.first().map_or(0.0, |c| c.seconds()),
            first.looped.len(),
            if first.alt_loop.is_empty() { "" } else { ", an alternate loop" },
            if once { ", plays once" } else { "" },
            if looping.music_time > 0.0 {
                format!(", ends by itself after {:.0} s", looping.music_time)
            } else {
                String::new()
            }
        );
        if looping.stops_music() {
            // Indices may shift (a piece still decoding is dropped), so
            // go by start order.
            let me = self.pieces[i].serial;
            let others: Vec<u64> = self
                .pieces
                .iter()
                .filter(|p| p.serial != me && p.music && p.state != State::Ending)
                .map(|p| p.serial)
                .collect();
            for s in others {
                if let Some(j) = self.pieces.iter().position(|p| p.serial == s) {
                    info!("music: {} stops {}", self.pieces.iter().find(|p| p.serial == me).map_or("?", |p| p.name.as_str()), self.pieces[j].name);
                    self.end(j, sounds, mixer, false);
                }
            }
        }
        true
    }

    /// Ends a piece by its rules: its outro (the alternate outro on the
    /// alternate loop) after the playing permutation, else a fade when the
    /// track fades out, else the playing permutation is its last.
    fn end(&mut self, i: usize, sounds: &Sounds, mixer: &Mixer, immediately: bool) {
        let p = &self.pieces[i];
        if p.state == State::Waiting {
            info!("music: {} stopped before it started", p.name);
            self.pieces.remove(i);
            return;
        }
        if p.state == State::Ending && !immediately {
            return;
        }
        let lsnd = p.lsnd;
        let alt = p.alt;
        let Some(looping) = sounds.data.loops.get(&lsnd) else { return };
        let clips = piece_clips(sounds, looping).unwrap_or_default();
        let mut how = Vec::new();
        for t in 0..self.pieces[i].tracks.len() {
            let track: Option<&Track> = looping.tracks.get(t);
            let c = clips.get(t);
            if immediately {
                for f in &mut self.pieces[i].tracks[t] {
                    f.fade_to(0.0, 0.05);
                    f.end_at_zero = true;
                }
                how.push("at once");
                continue;
            }
            let out = c.map_or(Vec::new(), |c| {
                if alt && !c.alt_out.is_empty() { c.alt_out.clone() } else { c.outro.clone() }
            });
            let out = self.pick(&out);
            let Some(main) = self.pieces[i].tracks[t].last_mut() else { continue };
            if !out.is_empty() {
                mixer.set_next(main.id, out, Vec::new());
                how.push(if alt && c.is_some_and(|c| !c.alt_out.is_empty()) { "alternate outro" } else { "outro" });
            } else if let Some(track) = track.filter(|t| t.fades_out()) {
                main.fade_to(0.0, track.fade_out);
                main.end_at_zero = true;
                how.push("fade");
            } else {
                mixer.set_next(main.id, Vec::new(), Vec::new());
                how.push("end of loop");
            }
        }
        let p = &mut self.pieces[i];
        p.state = State::Ending;
        info!("music: stop {} ({})", p.name, how.join(", "));
    }

    /// Switches a piece to or from its alternate loop.
    fn set_alt(&mut self, i: usize, on: bool, sounds: &Sounds, mixer: &Mixer) {
        if self.pieces[i].alt == on {
            return;
        }
        self.pieces[i].alt = on;
        if self.pieces[i].state != State::Playing {
            return;
        }
        let lsnd = self.pieces[i].lsnd;
        let Some(looping) = sounds.data.loops.get(&lsnd) else { return };
        let Some(clips) = piece_clips(sounds, looping) else { return };
        let mut how = "no alternate loop";
        for (t, (track, c)) in looping.tracks.iter().zip(&clips).enumerate() {
            if c.alt_loop.is_empty() {
                continue;
            }
            let (trans, next) = if on { (&c.alt_in, &c.alt_loop) } else { (&c.alt_back, &c.looped) };
            let queue = self.pick(trans);
            let Some(voices) = self.pieces[i].tracks.get_mut(t) else { continue };
            if track.crossfades_alt() {
                for f in voices.iter_mut() {
                    f.fade_to(0.0, track.alt_fade);
                    f.end_at_zero = true;
                }
                let id = mixer.play(Play {
                    bus: bus_of_class(looping.class as i32),
                    queue,
                    looping: next.clone(),
                    gain: [0.0; 2],
                    rate: 1.0,
                });
                let mut f = Fader::new(id, 0.0);
                f.fade_to(1.0, track.alt_fade);
                voices.push(f);
                how = "cross-fade";
            } else if let Some(main) = voices.last() {
                mixer.set_next(main.id, queue, next.clone());
                how = if c.alt_in.is_empty() || !on { "at the end of the playing loop" } else { "transition at the end of the playing loop" };
            }
        }
        info!(
            "music: {} {} the alternate loop ({how})",
            self.pieces[i].name,
            if on { "to" } else { "back from" }
        );
    }

    fn find(&self, lsnd: u32) -> Option<usize> {
        // The live one, not one still playing its outro.
        self.pieces
            .iter()
            .position(|p| p.lsnd == lsnd && p.state != State::Ending)
    }

    fn request(&mut self, r: SoundRequest, sounds: &Sounds, mixer: &Mixer, gains: &mut ClassGains) {
        match r {
            SoundRequest::LoopStart { tag, at, scale, .. } => self.start(sounds, tag, Origin::Script, at, scale),
            SoundRequest::LoopStop { tag, immediately } => {
                if let Some(i) = self.find(tag) {
                    self.end(i, sounds, mixer, immediately);
                }
            }
            SoundRequest::LoopAlternate { tag, on } => {
                if let Some(i) = self.find(tag) {
                    self.set_alt(i, on, sounds, mixer);
                }
            }
            SoundRequest::Impulse { tag, name, at, scale } => {
                let playable = sounds.data.sounds.get(&tag).is_some_and(|s| !s.permutations.is_empty());
                if playable {
                    self.pending.push(PendingImpulse { tag, name, at, scale, waited: 0.0 });
                } else {
                    self.warn_once(format!("impulse {tag}"), || {
                        format!("sound: {name} has no samples in the sfx bank or the dialogue bank; not played")
                    });
                }
            }
            SoundRequest::ImpulseStop { tag } => {
                self.pending.retain(|p| p.tag != tag);
                for v in self.impulses.iter().filter(|v| v.tag == tag) {
                    mixer.stop(v.id);
                }
            }
            SoundRequest::ClassGain { class, gain, seconds } => {
                if CLASSES.contains(&class.as_str()) {
                    gains.set(&class, gain, seconds);
                    info!("sound: class {class} gain {gain} over {seconds:.1} s");
                } else {
                    self.warn_once(format!("class {class}"), || {
                        format!("sound: class gain for {class:?} is not applied (only music and amb are)")
                    });
                }
            }
        }
    }
}

/// Takes the scripts' sound calls once the sounds are loaded.
pub(crate) fn script_requests(
    sounds: Option<Res<Sounds>>,
    queue: Option<ResMut<ScriptSounds>>,
    mixer: Res<Mixer>,
    mut player: ResMut<MusicPlayer>,
    mut gains: ResMut<ClassGains>,
) {
    let (Some(sounds), Some(mut queue)) = (sounds, queue) else { return };
    if queue.0.is_empty() {
        return;
    }
    for r in std::mem::take(&mut queue.0) {
        player.request(r, &sounds, &mixer, &mut gains);
    }
}

/// M, Shift+M, N and `--music`.
pub(crate) fn hand_controls(
    keys: Res<ButtonInput<KeyCode>>,
    sounds: Option<Res<Sounds>>,
    mixer: Res<Mixer>,
    mut player: ResMut<MusicPlayer>,
) {
    let Some(sounds) = sounds else { return };
    let tracks = &sounds.data.map.music;
    if tracks.is_empty() {
        return;
    }
    if !player.started_once {
        player.started_once = true;
        if let Some(w) = player.wanted_at_start.clone() {
            let found = w
                .parse::<usize>()
                .ok()
                .filter(|n| *n < tracks.len())
                .or_else(|| tracks.iter().position(|(_, n)| n.ends_with(w.as_str())));
            match found {
                Some(n) => {
                    player.hand_track = Some(n);
                    player.start(&sounds, tracks[n].0, Origin::Hand, None, 1.0);
                }
                None => warn!(
                    "--music {w}: no such track; the map has {:?}",
                    tracks.iter().map(|t| &t.1).collect::<Vec<_>>()
                ),
            }
        }
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    // The showpiece: the Warthog run on 120_halo, else the first track.
    let default_track = tracks.iter().position(|(_, n)| n.ends_with("120_music_15")).unwrap_or(0);
    if keys.just_pressed(KeyCode::KeyM) {
        let live: Vec<usize> = (0..player.pieces.len()).filter(|i| player.pieces[*i].state != State::Ending).collect();
        if shift {
            let next = player.hand_track.map_or(default_track, |t| (t + 1) % tracks.len());
            for i in (0..player.pieces.len()).rev() {
                player.end(i, &sounds, &mixer, true);
            }
            player.hand_track = Some(next);
            player.start(&sounds, tracks[next].0, Origin::Hand, None, 1.0);
        } else if !live.is_empty() {
            for i in live.into_iter().rev() {
                player.end(i, &sounds, &mixer, false);
            }
        } else {
            let t = player.hand_track.unwrap_or(default_track);
            player.hand_track = Some(t);
            player.start(&sounds, tracks[t].0, Origin::Hand, None, 1.0);
        }
    }
    if keys.just_pressed(KeyCode::KeyN)
        && let Some(i) = (0..player.pieces.len())
            .filter(|i| player.pieces[*i].state == State::Playing)
            .max_by_key(|i| player.pieces[*i].serial)
    {
        let on = !player.pieces[i].alt;
        player.set_alt(i, on, &sounds, &mixer);
    }
}

/// Starts decoded pieces, ends timed-out ones, moves the fades, places
/// positional sounds, and plays and places one-shots.
pub(crate) fn update(
    time: Res<Time>,
    sounds: Option<Res<Sounds>>,
    l: Res<Listener>,
    mixer: Res<Mixer>,
    mut player: ResMut<MusicPlayer>,
    mut gains: ResMut<ClassGains>,
) {
    let Some(sounds) = sounds else { return };
    let dt = time.delta_secs();
    gains.step(dt);
    let mut i = 0;
    while i < player.pieces.len() {
        match player.pieces[i].state {
            State::Waiting => {
                player.begin(i, &sounds, &mixer);
            }
            State::Playing => {
                player.pieces[i].age += dt;
                let time_up = sounds
                    .data
                    .loops
                    .get(&player.pieces[i].lsnd)
                    .is_some_and(|l| l.music_time > 0.0 && player.pieces[i].age >= l.music_time);
                if time_up {
                    info!("music: {} reached its music time", player.pieces[i].name);
                    player.end(i, &sounds, &mixer, false);
                }
            }
            State::Ending => {}
        }
        i += 1;
    }
    let music_gain = gains.get("music");
    let amb_gain = gains.get("amb");
    let pieces = &mut player.pieces;
    for p in pieces.iter_mut() {
        let looping = sounds.data.loops.get(&p.lsnd);
        let class = if p.music { music_gain } else { amb_gain };
        let place = p.at.filter(|_| l.valid).map(|at| {
            let d = at.distance(l.position);
            let bounds = looping.map_or((2.0, 20.0), |l| l.distance);
            (at, attenuation(d, bounds))
        });
        for (t, voices) in p.tracks.iter_mut().enumerate() {
            let track_gain = looping
                .and_then(|l| l.tracks.get(t))
                .map(|tr| {
                    let snd_db = tr.looped.or(tr.intro).and_then(|s| sounds.data.sounds.get(&s)).map_or(0.0, |s| s.gain_db);
                    db(tr.gain_db + snd_db)
                })
                .unwrap_or(1.0);
            voices.retain_mut(|f| {
                f.step(dt);
                if !mixer.is_playing(f.id) {
                    return false;
                }
                if f.end_at_zero && f.level <= 0.0 {
                    mixer.stop(f.id);
                    return false;
                }
                let g = track_gain * f.level * p.scale * class;
                let lr = match place {
                    Some((at, a)) => l.pan(at, g * a),
                    None => [g; 2],
                };
                mixer.set_gain(f.id, lr);
                true
            });
        }
    }
    pieces.retain(|p| {
        let done = p.state != State::Waiting && p.voices().next().is_none();
        if done {
            info!("music: {} ended", p.name);
        }
        !done
    });

    // One-shots.
    let mut pending = std::mem::take(&mut player.pending);
    pending.retain_mut(|p| {
        p.waited += dt;
        let Some(info) = sounds.data.sounds.get(&p.tag) else { return false };
        let pick = player.random() as usize % info.permutations.len().max(1);
        let Some(clip) = info.permutations.get(pick).and_then(|id| sounds.clips.get(*id)) else {
            // Decoding; give up after a few seconds.
            return p.waited < 3.0;
        };
        let bus = bus_of_class(info.class as i32);
        let dialogue = bus == super::mixer::Bus::Dialogue;
        // A script's `amb` class gain is for ambience and effects, not for
        // the lines.
        let gain = db(info.gain_db) * p.scale * if dialogue { 1.0 } else { amb_gain };
        if dialogue {
            info!(
                "dialogue: {} ({:.2} s, {:+.1} dB, {})",
                p.name.rsplit('\\').next().unwrap_or(&p.name),
                clip.seconds(),
                info.gain_db,
                if p.at.is_some() { "at its speaker" } else { "in the player's ear" }
            );
        }
        let id = mixer.play(Play {
            bus,
            queue: vec![clip],
            looping: Vec::new(),
            gain: [0.0; 2],
            rate: 1.0,
        });
        player.impulses.push(Impulse {
            id,
            tag: p.tag,
            at: p.at,
            gain,
            distances: info.distances,
            dialogue,
        });
        false
    });
    player.pending = pending;
    player.impulses.retain(|v| mixer.is_playing(v.id));
    // While a line plays the other buses duck by their classes' dialogue
    // ducking (music -3 dB, effects -6 dB on 120_halo), fading down and
    // back up over the classes' times.
    let talking = player.impulses.iter().any(|v| v.dialogue);
    if player.ducks.is_empty() {
        player.ducks = sounds.data.ducking.iter().map(|(bus, _, _)| (*bus, Fader::new(0, 1.0))).collect();
    }
    for ((bus, f), (_, duck_db, [down, up])) in player.ducks.iter_mut().zip(&sounds.data.ducking) {
        let target = if talking { db(*duck_db) } else { 1.0 };
        if f.target != target {
            f.fade_to(target, if talking { *down } else { *up });
        }
        f.step(dt);
        mixer.set_duck(*bus, f.level);
    }
    for v in &player.impulses {
        let lr = match v.at.filter(|_| l.valid) {
            Some(at) => l.pan(at, v.gain * distance_gain(at.distance(l.position), v.distances)),
            None => [v.gain; 2],
        };
        mixer.set_gain(v.id, lr);
    }
}

#[cfg(test)]
mod tests {
    use super::Fader;

    #[test]
    fn fader_ramps_and_holds() {
        let mut f = Fader::new(1, 0.0);
        f.fade_to(1.0, 2.0);
        f.step(0.5);
        assert!((f.level - 0.25).abs() < 1e-6);
        f.step(10.0);
        assert_eq!(f.level, 1.0);
        f.fade_to(0.0, 0.0);
        f.step(0.001);
        assert_eq!(f.level, 0.0);
    }
}
