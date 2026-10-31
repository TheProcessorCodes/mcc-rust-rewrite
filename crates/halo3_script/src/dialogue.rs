//! The map's dialogue as the scripts see it: lines by name (the mission
//! dialogue tag, `mdlg`) and how long each sound runs, so that
//! `(sleep (ai_play_line_on_object none 120mg_030))` and
//! `(sleep (sound_impulse_language_time snd))` wait as long as the line
//! plays. `docs/format/sound.md` 11.
//!
//! The lengths come from the language bank's `.info` sidecar (the player's
//! language, `game_paths::language`), so a French line waits as long as the
//! French take; without it, from the `snd!` tag's own longest permutation
//! (`snd! + 0x14`, the English length).

use std::collections::HashMap;
use std::path::Path;

use blam_cache::CacheFile;
use blam_sound::tags::SoundTags;

/// One line the scripts can play by name.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// The `snd!` tag index.
    pub sound: u32,
    /// The tag path (`sound\dialog\120_halo\mission\120mg_030_cor`).
    pub sound_name: String,
    /// The speaker's code (`cor`, `jon`, `arb`, `gsp`, ...).
    pub speaker: String,
}

#[derive(Clone, Debug, Default)]
pub struct Dialogue {
    /// Mission dialogue lines by name (`120mg_030`).
    pub lines: HashMap<String, Line>,
    /// Seconds each dialogue or script `snd!` plays, by tag index.
    pub seconds: HashMap<u32, f32>,
    /// The language the lengths are of, `None` when they are the tags'.
    pub language: Option<&'static str>,
}

impl Dialogue {
    /// Reads the map's mission dialogue and the lengths of its lines and of
    /// the scripts' one-shots, in the player's language when its bank is
    /// installed beside the map.
    pub fn read(cache: &CacheFile) -> Dialogue {
        let language = game_paths::language();
        let lengths = cache
            .path()
            .parent()
            .and_then(Path::parent)
            .and_then(|halo3| blam_sound::language_bank(halo3, language).ok())
            .and_then(|bank| match blam_sound::info_seconds(&bank) {
                Ok(s) => Some(s),
                Err(e) => {
                    bevy::log::warn!("scripts: no {language} dialogue lengths ({e}); using the tags'");
                    None
                }
            });
        Dialogue::read_with(cache, lengths.as_ref(), language)
    }

    /// Like [`Dialogue::read`], with the lengths by FMOD id given (`None`:
    /// the tags' own).
    pub fn read_with(cache: &CacheFile, lengths: Option<&HashMap<u32, f32>>, language: &'static str) -> Dialogue {
        let mut out = Dialogue {
            language: lengths.map(|_| language),
            ..Default::default()
        };
        let Ok(tags) = SoundTags::new(cache) else { return out };
        let lines = tags.mission_dialogue().unwrap_or_default();
        let mut wanted: Vec<u32> = tags.map_sounds().map(|m| m.script_sounds).unwrap_or_default();
        for l in &lines {
            let Some((sound, speaker)) = l.sound() else { continue };
            wanted.push(sound);
            out.lines.entry(l.name.clone()).or_insert_with(|| Line {
                sound,
                sound_name: cache.tags.get(sound).map(|t| t.name().to_string()).unwrap_or_default(),
                speaker: speaker.to_string(),
            });
        }
        for t in wanted {
            if out.seconds.contains_key(&t) {
                continue;
            }
            let Ok(snd) = tags.sound(t) else { continue };
            let from_bank = lengths.and_then(|l| {
                snd.permutations
                    .iter()
                    .filter_map(|p| l.get(&p.fmod_id).copied())
                    .reduce(f32::max)
            });
            let seconds = from_bank.unwrap_or(snd.max_play_ms.max(0) as f32 / 1000.0);
            out.seconds.insert(t, seconds);
        }
        out
    }

    /// The line a script names, by its name.
    pub fn line(&self, name: &str) -> Option<&Line> {
        self.lines.get(name)
    }

    /// How long a sound plays, seconds (0 when unknown).
    pub fn seconds_of(&self, sound: u32) -> f32 {
        self.seconds.get(&sound).copied().unwrap_or(0.0)
    }
}
