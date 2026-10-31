//! What the map's scripts ask of the sound: `halo3_script` turns the
//! HaloScript sound functions into [`SoundRequest`]s and queues them in
//! [`ScriptSounds`]; `halo3_audio` takes them off the queue once the map's
//! sounds are loaded. Without scripts the queue stays empty.

use bevy::prelude::*;

/// One sound function call. Tags are tag indices (the low 16 bits of the
/// script constant's tag id); `name` is the tag path, for the log.
#[derive(Clone, Debug, PartialEq)]
pub enum SoundRequest {
    /// `sound_looping_start lsnd object scale`: `at` is the object's place
    /// (Halo world units), `None` for `none` (music: not placed anywhere).
    LoopStart {
        tag: u32,
        name: String,
        at: Option<Vec3>,
        scale: f32,
    },
    /// `sound_looping_stop` (the piece ends by its own rules: outro or fade)
    /// or `sound_looping_stop_immediately`.
    LoopStop { tag: u32, immediately: bool },
    /// `sound_looping_set_alternate lsnd on`.
    LoopAlternate { tag: u32, on: bool },
    /// `sound_impulse_start snd object scale`: a one-shot.
    Impulse {
        tag: u32,
        name: String,
        at: Option<Vec3>,
        scale: f32,
    },
    /// `sound_impulse_stop snd`.
    ImpulseStop { tag: u32 },
    /// `sound_class_set_gain class gain ticks`: `class` as the script names
    /// it (`music`, `amb`, `object`, `""` for all), ramped over `seconds`.
    ClassGain {
        class: String,
        gain: f32,
        seconds: f32,
    },
}

/// Requests waiting for the sound side, oldest first.
#[derive(Resource, Default, Debug)]
pub struct ScriptSounds(pub Vec<SoundRequest>);

impl ScriptSounds {
    /// Most requests kept while nobody takes them (no sound device, sound
    /// off): the oldest one-shots go first.
    pub const LIMIT: usize = 512;

    pub fn push(&mut self, r: SoundRequest) {
        if self.0.len() >= Self::LIMIT
            && let Some(i) = self.0.iter().position(|r| matches!(r, SoundRequest::Impulse { .. }))
        {
            self.0.remove(i);
        }
        if self.0.len() < Self::LIMIT {
            self.0.push(r);
        }
    }
}
