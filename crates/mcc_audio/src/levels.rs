//! The player's volumes: master, music, effects, ambience and dialogue, set
//! on the pause menu's Sound tab (`mcc-rust-rewrite` `pause.rs`) and by the `-` / `=`
//! (master) and K (mute) keys, saved to `sound-levels.json` in the
//! data folder and read back at start. `MCC_VOLUME` overrides the saved master
//! (a developer override).
//!
//! Each slider scales its bus in the mixer on top of everything else: a
//! sound's tag and class gains, and the scripts' `sound_class_set_gain`.

use bevy::prelude::*;

use super::mixer::{Bus, Mixer};

/// One slider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Master,
    Music,
    Effects,
    Ambience,
    Dialogue,
}

impl Channel {
    /// In the order the menu lists them.
    pub const ALL: [Channel; 5] = [
        Channel::Master,
        Channel::Music,
        Channel::Effects,
        Channel::Ambience,
        Channel::Dialogue,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Channel::Master => "Master",
            Channel::Music => "Music",
            Channel::Effects => "Effects",
            Channel::Ambience => "Ambience",
            Channel::Dialogue => "Dialogue",
        }
    }

    /// What it covers, for the menu.
    pub fn about(self) -> &'static str {
        match self {
            Channel::Master => "everything",
            Channel::Music => "the mission music",
            Channel::Effects => "weapons, impacts, vehicles, board and script sounds",
            Channel::Ambience => "wind, birds, machinery, sound scenery",
            Channel::Dialogue => "radio and voices (most lines are not read yet)",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Channel::Master => "master",
            Channel::Music => "music",
            Channel::Effects => "effects",
            Channel::Ambience => "ambience",
            Channel::Dialogue => "dialogue",
        }
    }
}

/// The volumes, 0 to [`SoundLevels::MAX`] each (1 = as the game mixes it).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct SoundLevels {
    pub master: f32,
    pub music: f32,
    pub effects: f32,
    pub ambience: f32,
    pub dialogue: f32,
    /// K: everything silent, the levels kept.
    pub muted: bool,
}

impl Default for SoundLevels {
    fn default() -> Self {
        SoundLevels {
            master: 0.8,
            music: 1.0,
            effects: 1.0,
            ambience: 1.0,
            dialogue: 1.0,
            muted: false,
        }
    }
}

const FILE: &str = "sound-levels.json";

impl SoundLevels {
    pub const MAX: f32 = 2.0;
    /// One press of a key or the d-pad.
    pub const STEP: f32 = 0.05;

    pub fn get(&self, c: Channel) -> f32 {
        match c {
            Channel::Master => self.master,
            Channel::Music => self.music,
            Channel::Effects => self.effects,
            Channel::Ambience => self.ambience,
            Channel::Dialogue => self.dialogue,
        }
    }

    /// Sets a volume, clamped and rounded to whole percent. Changing the
    /// master unmutes.
    pub fn set(&mut self, c: Channel, v: f32) {
        let v = (v.clamp(0.0, Self::MAX) * 100.0).round() / 100.0;
        match c {
            Channel::Master => {
                self.master = v;
                self.muted = false;
            }
            Channel::Music => self.music = v,
            Channel::Effects => self.effects = v,
            Channel::Ambience => self.ambience = v,
            Channel::Dialogue => self.dialogue = v,
        }
    }

    /// A bus's volume.
    pub fn bus(&self, b: Bus) -> f32 {
        match b {
            Bus::Music => self.music,
            Bus::Ambience => self.ambience,
            Bus::Effects => self.effects,
            Bus::Dialogue => self.dialogue,
        }
    }

    /// The master volume the mixer gets (0 while muted).
    pub fn master_out(&self) -> f32 {
        if self.muted { 0.0 } else { self.master }
    }

    fn from_json(text: &str) -> Option<SoundLevels> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let o = v.as_object()?;
        let mut l = SoundLevels::default();
        for c in Channel::ALL {
            if let Some(x) = o.get(c.key()).and_then(serde_json::Value::as_f64) {
                l.set(c, x as f32);
            }
        }
        l.muted = o.get("muted").and_then(serde_json::Value::as_bool).unwrap_or(false);
        Some(l)
    }

    fn to_json(&self) -> String {
        let mut o = serde_json::Map::new();
        for c in Channel::ALL {
            o.insert(c.key().into(), serde_json::json!(self.get(c)));
        }
        o.insert("muted".into(), serde_json::json!(self.muted));
        serde_json::to_string_pretty(&serde_json::Value::Object(o)).unwrap_or_default()
    }

    /// The saved levels (defaults without a file), then `MCC_VOLUME` for
    /// the master.
    pub fn load() -> SoundLevels {
        let mut l = match game_paths::read_saved(FILE) {
            Some(text) => SoundLevels::from_json(&text).unwrap_or_else(|| {
                warn!("sound: {FILE} is not valid; using the default volumes");
                SoundLevels::default()
            }),
            None => SoundLevels::default(),
        };
        if let Some(v) = std::env::var("MCC_VOLUME").ok().and_then(|v| v.parse::<f32>().ok()) {
            l.set(Channel::Master, v);
        }
        l
    }

    fn save(&self) {
        if let Err(e) = game_paths::write_saved(FILE, &self.to_json()) {
            warn!("sound: {e}");
        }
    }

    /// For the log: `master 80 %, music 100 %, ...`.
    pub fn describe(&self) -> String {
        let v: Vec<String> = Channel::ALL
            .iter()
            .map(|c| format!("{} {:.0} %", c.key(), self.get(*c) * 100.0))
            .collect();
        format!("{}{}", v.join(", "), if self.muted { " (muted)" } else { "" })
    }
}

/// Seconds (real time: the menu stops the game clock) until the changed
/// levels are written, so a slider being dragged is saved once.
#[derive(Resource, Default)]
pub(crate) struct SaveDelay(Option<f32>);

/// Hands changed levels to the mixer and saves them a moment later. While
/// a map is in but play has not begun (behind the loading screen,
/// `mcc_core::map::Playing`) the master is silent.
pub(crate) fn apply(
    time: Res<Time<Real>>,
    levels: Res<SoundLevels>,
    mixer: Res<Mixer>,
    mut delay: ResMut<SaveDelay>,
    installed: Option<Res<mcc_core::map::MapInstalled>>,
    playing: Option<Res<mcc_core::map::Playing>>,
    mut was_silent: Local<bool>,
) {
    let silent = installed.is_some() && playing.is_none();
    if levels.is_changed() || silent != *was_silent {
        *was_silent = silent;
        mixer.set_master(if silent { 0.0 } else { levels.master_out() });
        for b in Bus::ALL {
            mixer.set_bus(b, levels.bus(b));
        }
        if !levels.is_added() {
            delay.0 = Some(0.5);
        }
    }
    if let Some(t) = delay.0.as_mut() {
        *t -= time.delta_secs();
        if *t <= 0.0 {
            delay.0 = None;
            levels.save();
            info!("sound: volumes saved ({})", levels.describe());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_and_clamping() {
        let mut l = SoundLevels::default();
        l.set(Channel::Music, 1.5);
        l.set(Channel::Effects, 7.0);
        l.muted = true;
        let back = SoundLevels::from_json(&l.to_json()).unwrap();
        assert_eq!(back, l);
        assert_eq!(back.effects, SoundLevels::MAX);
        // Missing keys keep their defaults; junk is not a file.
        let partial = SoundLevels::from_json(r#"{"music": 0.25}"#).unwrap();
        assert_eq!(partial.music, 0.25);
        assert_eq!(partial.master, SoundLevels::default().master);
        assert!(SoundLevels::from_json("not json").is_none());
    }

    #[test]
    fn master_unmutes() {
        let mut l = SoundLevels { muted: true, ..Default::default() };
        assert_eq!(l.master_out(), 0.0);
        l.set(Channel::Master, 0.5);
        assert_eq!(l.master_out(), 0.5);
    }
}
