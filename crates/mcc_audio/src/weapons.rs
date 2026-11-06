//! Weapon sounds: Halo 3's own firing, empty-click, reload, ready, charge
//! and impact sounds, played when the weapons send their messages
//! (`mcc_core::weapons`). Which tags those are comes from
//! `blam_sound::weapons` (`docs/format/weapon-sounds.md`).
//!
//! - The player's own gun is heard at the listener, both ears equal; a
//!   sound whose distances keep it silent close up (the far-away shot
//!   layers) stays silent. Other guns and every impact are placed in the
//!   world and fade by their sound's distances.
//! - Each (weapon, own, sound) plays at most as many copies as its sound
//!   class allows one object (two for gunfire): a new shot fades out the
//!   oldest, so 15 rounds a second never pile up.
//! - Clips decode on a worker thread the first time a weapon sends
//!   anything (all of its sounds at once) and stay cached; a sound not
//!   decoded yet is skipped.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use bevy::prelude::*;
use blam_sound::tags::distance_gain;

use super::mixer::{Bus, Clip, Mixer, Play, VoiceId};
use super::{Listener, Sounds, db};
use mcc_core::weapons::{WeaponAction, WeaponEvent, WeaponImpact};

/// Weapon sounds' lift over their class level (`SoundInfo::gain_db` has
/// the class's -12 dB for gunfire): at the bare class level a rifle burst
/// peaks barely above the music's average, much weaker than in the game,
/// where first-person sounds get their own mix (`snmx`, not decoded). +6
/// dB keeps gunfire punchy with the music still on top: 3 dB under where it
/// was before the class gains were read.
const WEAPON_GAIN: f32 = 2.0;
/// A charge held this long makes the next shot the charged one (plasma
/// pistol); a weapon with no plain shot (the Spartan laser) always fires
/// charged. Not read from the tags.
const CHARGED_AFTER: f32 = 0.6;
/// A held fire loop (SMG) ends this long after its last round.
const LOOP_HOLD: f32 = 0.2;
/// A charge nobody released stops after this long.
const CHARGE_MAX: f32 = 10.0;

/// A looping sound being held: its voice and its outro.
struct Held {
    voice: VoiceId,
    outro: Option<u32>,
    gain_db: f32,
}

struct FireLoop {
    held: Vec<Held>,
    last: f32,
    at: Vec3,
}

struct Charging {
    held: Option<Held>,
    since: f32,
    at: Vec3,
}

#[derive(Resource, Default)]
pub(super) struct WeaponVoices {
    /// Voices playing, oldest first, by (weapon, own, sound tag).
    playing: HashMap<(u32, bool, u32), VecDeque<VoiceId>>,
    loops: HashMap<(u32, bool), FireLoop>,
    charging: HashMap<(u32, bool), Charging>,
    /// Whether each weapon's last shot was the charged one (for its
    /// impact).
    charged_shot: HashMap<u32, bool>,
    /// Weapons whose clips have been asked for.
    warmed: HashSet<u32>,
    rng: u32,
}

impl WeaponVoices {
    fn random(&mut self, n: usize) -> usize {
        if self.rng == 0 {
            self.rng = 0x2545_F491;
        }
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng as usize % n.max(1)
    }
}

/// What the systems below share.
struct Ctx<'a> {
    sounds: &'a Sounds,
    mixer: &'a Mixer,
    listener: &'a Listener,
}

impl Ctx<'_> {
    /// Distance from the listener (0 for the player's own gun) and the
    /// left / right gains for a sound of `gain` there.
    fn place(&self, at: Vec3, own: bool, distances: [f32; 4], gain: f32) -> Option<[f32; 2]> {
        let d = if own || !self.listener.valid {
            0.0
        } else {
            at.distance(self.listener.position)
        };
        let g = WEAPON_GAIN * gain * distance_gain(d, distances);
        if g < 1e-3 {
            return None;
        }
        Some(if own || !self.listener.valid {
            [g; 2]
        } else {
            self.listener.pan(at, g)
        })
    }

    /// Decoded clips of a sound's permutations (those ready).
    fn clips(&self, snd: u32) -> Vec<Arc<Clip>> {
        self.sounds
            .data
            .sounds
            .get(&snd)
            .map(|s| {
                s.permutations
                    .iter()
                    .filter_map(|p| self.sounds.clips.get(*p))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Starts one permutation of `snd`, keeping to its per-object limit.
fn one_shot(v: &mut WeaponVoices, cx: &Ctx, weapon: u32, own: bool, snd: u32, at: Vec3) {
    let Some(info) = cx.sounds.data.sounds.get(&snd) else {
        return;
    };
    let Some(gain) = cx.place(at, own, info.distances, db(info.gain_db)) else {
        return;
    };
    let clips = cx.clips(snd);
    if clips.is_empty() {
        return;
    }
    let clip = clips[v.random(clips.len())].clone();
    let limit = info.per_object.clamp(1, 4);
    let voices = v.playing.entry((weapon, own, snd)).or_default();
    voices.retain(|id| cx.mixer.is_playing(*id));
    while voices.len() >= limit {
        if let Some(old) = voices.pop_front() {
            cx.mixer.stop(old);
        }
    }
    voices.push_back(cx.mixer.play(Play {
        bus: Bus::Effects,
        queue: vec![clip],
        looping: Vec::new(),
        gain,
        rate: 1.0,
    }));
}

/// Starts a looping sound's first track: intro, then its loop until
/// stopped. `None` until its clips are decoded, or when it is out of
/// earshot.
fn hold(v: &mut WeaponVoices, cx: &Ctx, lsnd: u32, own: bool, at: Vec3) -> Option<Held> {
    let track = cx.sounds.data.loops.get(&lsnd)?.tracks.first()?;
    let looped = track.looped?;
    let info = cx.sounds.data.sounds.get(&looped)?;
    let gain_db = track.gain_db + info.gain_db;
    let gain = cx.place(at, own, info.distances, db(gain_db))?;
    let looping = cx.sounds.clips.all(&info.permutations)?;
    if looping.is_empty() {
        return None;
    }
    let intro = track.intro.map(|s| cx.clips(s)).unwrap_or_default();
    let queue = if intro.is_empty() {
        Vec::new()
    } else {
        vec![intro[v.random(intro.len())].clone()]
    };
    Some(Held {
        voice: cx.mixer.play(Play {
            bus: Bus::Effects,
            queue,
            looping,
            gain,
            rate: 1.0,
        }),
        outro: track.outro,
        gain_db: track.gain_db,
    })
}

/// Stops a held loop, playing its outro where it was.
fn release(v: &mut WeaponVoices, cx: &Ctx, own: bool, held: Held, at: Vec3) {
    cx.mixer.stop(held.voice);
    if let Some(out) = held.outro {
        // The outro's own gain plus the track's.
        if let Some(info) = cx.sounds.data.sounds.get(&out)
            && let Some(gain) = cx.place(at, own, info.distances, db(info.gain_db + held.gain_db))
        {
            let clips = cx.clips(out);
            if !clips.is_empty() {
                let clip = clips[v.random(clips.len())].clone();
                cx.mixer.play(Play {
                    bus: Bus::Effects,
                    queue: vec![clip],
                    looping: Vec::new(),
                    gain,
                    rate: 1.0,
                });
            }
        }
    }
}

/// Asks for every clip a weapon can play, so they are decoded by the time
/// it fires again.
fn warm(v: &mut WeaponVoices, cx: &Ctx, weapon: u32) {
    if !v.warmed.insert(weapon) {
        return;
    }
    let Some(w) = cx.sounds.data.weapons.get(&weapon) else {
        return;
    };
    let data = &cx.sounds.data;
    let mut snds: Vec<u32> = w.all_sounds().collect();
    for l in w.all_loops() {
        if let Some(t) = data.loops.get(&l).and_then(|l| l.tracks.first()) {
            snds.extend([t.intro, t.looped, t.outro].into_iter().flatten());
        }
    }
    for s in snds {
        for p in data.sounds.get(&s).map_or(&[][..], |i| &i.permutations[..]) {
            // Asking queues the decode.
            let _ = cx.sounds.clips.get(*p);
        }
    }
}

fn stop_charge(
    v: &mut WeaponVoices,
    cx: &Ctx,
    key: (u32, bool),
    now: f32,
    with_outro: bool,
) -> Option<f32> {
    let c = v.charging.remove(&key)?;
    if let Some(h) = c.held {
        if with_outro {
            release(v, cx, key.1, h, c.at);
        } else {
            cx.mixer.stop(h.voice);
        }
    }
    Some(now - c.since)
}

pub(super) fn weapon_sounds(
    time: Res<Time>,
    mut events: MessageReader<WeaponEvent>,
    mut impacts: MessageReader<WeaponImpact>,
    sounds: Option<Res<Sounds>>,
    mixer: Res<Mixer>,
    listener: Res<Listener>,
    mut v: ResMut<WeaponVoices>,
) {
    let Some(sounds) = sounds else {
        // Not loaded yet: what happened now is not heard.
        events.clear();
        impacts.clear();
        return;
    };
    let cx = Ctx {
        sounds: &sounds,
        mixer: &mixer,
        listener: &listener,
    };
    let v = &mut *v;
    let now = time.elapsed_secs();

    for e in events.read() {
        let Some(w) = sounds.data.weapons.get(&e.weapon) else {
            continue;
        };
        warm(v, &cx, e.weapon);
        let at = mcc_core::coords::to_halo(e.at);
        let key = (e.weapon, e.own);
        match e.action {
            WeaponAction::Fire => {
                let held_for = stop_charge(v, &cx, key, now, true);
                let plain = !w.fire.is_empty() || !w.fire_loops.is_empty();
                let charged = !w.fire_charged.is_empty()
                    && (!plain || held_for.is_some_and(|t| t >= CHARGED_AFTER));
                v.charged_shot.insert(e.weapon, charged);
                let shot = if charged { &w.fire_charged } else { &w.fire };
                for s in shot {
                    one_shot(v, &cx, e.weapon, e.own, *s, at);
                }
                if e.own {
                    for s in &w.fire_first_person {
                        one_shot(v, &cx, e.weapon, e.own, *s, at);
                    }
                }
                if !charged && !w.fire_loops.is_empty() {
                    let started = v.loops.get(&key).is_some_and(|l| !l.held.is_empty());
                    if !started {
                        let held: Vec<Held> = w
                            .fire_loops
                            .iter()
                            .filter_map(|l| hold(v, &cx, *l, e.own, at))
                            .collect();
                        v.loops.insert(
                            key,
                            FireLoop {
                                held,
                                last: now,
                                at,
                            },
                        );
                    } else if let Some(l) = v.loops.get_mut(&key) {
                        l.last = now;
                        l.at = at;
                    }
                }
            }
            WeaponAction::Charge => {
                stop_charge(v, &cx, key, now, false);
                let held = w.charge.and_then(|l| hold(v, &cx, l, e.own, at));
                v.charging.insert(
                    key,
                    Charging {
                        held,
                        since: now,
                        at,
                    },
                );
            }
            WeaponAction::Empty | WeaponAction::Reload | WeaponAction::Ready => {
                stop_charge(v, &cx, key, now, false);
                let list = match e.action {
                    WeaponAction::Empty => &w.empty,
                    WeaponAction::Reload => &w.reload,
                    _ => &w.ready,
                };
                for s in list {
                    one_shot(v, &cx, e.weapon, e.own, *s, at);
                }
            }
        }
    }

    for i in impacts.read() {
        let Some(w) = sounds.data.weapons.get(&i.weapon) else {
            continue;
        };
        warm(v, &cx, i.weapon);
        let charged = v.charged_shot.get(&i.weapon).copied().unwrap_or(false);
        let list = if charged && !w.impact_charged.is_empty() {
            &w.impact_charged
        } else {
            &w.impact
        };
        let at = mcc_core::coords::to_halo(i.at);
        for s in list {
            one_shot(v, &cx, i.weapon, false, *s, at);
        }
    }

    // Fire loops end when the rounds stop; the rest follow the listener.
    let ended: Vec<(u32, bool)> = v
        .loops
        .iter()
        .filter(|(_, l)| now - l.last > LOOP_HOLD)
        .map(|(k, _)| *k)
        .collect();
    for key in ended {
        if let Some(l) = v.loops.remove(&key) {
            for h in l.held {
                release(v, &cx, key.1, h, l.at);
            }
        }
    }
    let stale: Vec<(u32, bool)> = v
        .charging
        .iter()
        .filter(|(_, c)| now - c.since > CHARGE_MAX)
        .map(|(k, _)| *k)
        .collect();
    for key in stale {
        stop_charge(v, &cx, key, now, true);
    }
    v.playing.retain(|_, voices| {
        voices.retain(|id| mixer.is_playing(*id));
        !voices.is_empty()
    });
}
