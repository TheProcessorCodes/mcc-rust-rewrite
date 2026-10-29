//! What a weapon sounds like: from a `weap` tag to the `snd!` / `lsnd` tags
//! it plays when it fires, clicks empty, reloads, is readied, charges, and
//! when its round lands. `docs/format/weapon-sounds.md` has the layouts;
//! every offset was found by walking the user's `riverworld.map` with
//! `docs/format/scripts/probe_weapon_sounds.py`. Nothing here comes from
//! other projects' code.

use std::collections::HashMap;

use blam_cache::{CacheFile, TagEntry};

use crate::tags::ATTACHMENT_SIZE;
use crate::{Error, Result};

/// The sound tags of one weapon, by what it is doing. `Vec`s hold `snd!`
/// tag indices to start together; `Option<u32>`s are `lsnd` tags.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WeaponSounds {
    /// The `weap` tag index.
    pub weapon: u32,
    pub name: String,
    /// One shot of the primary barrel: its firing effect's sounds (the
    /// near shot, often a far-away version and a shell ejection).
    pub fire: Vec<u32>,
    /// The secondary barrel's shot (the plasma pistol's overcharge, the
    /// Spartan laser's beam), when it has its own.
    pub fire_charged: Vec<u32>,
    /// Heard only from the player's own gun: the first-person fire
    /// animation's sounds (the shotgun's pump).
    pub fire_first_person: Vec<u32>,
    /// Looping sounds held while the trigger is (the SMG has no per-shot
    /// sound, it loops instead): attachments driven by `loop_fire_single`.
    pub fire_loops: Vec<u32>,
    /// The trigger pulled with nothing loaded.
    pub empty: Vec<u32>,
    /// The first-person reload animation's sounds.
    pub reload: Vec<u32>,
    /// The first-person ready animation's sounds.
    pub ready: Vec<u32>,
    /// The looping sound attached for charging (`primary_charged`).
    pub charge: Option<u32>,
    /// The primary barrel's round landing: its detonation and impact
    /// effects, and its material effects for [`IMPACT_MATERIALS`].
    pub impact: Vec<u32>,
    /// The same for the secondary barrel's round.
    pub impact_charged: Vec<u32>,
    /// Picking up ammunition for it (not tied to a message yet).
    pub ammo_pickup: Option<u32>,
}

/// The surface impacts are heard on, best first: the messages carry no
/// material, and Valhalla is mostly grass. A material effects entry is
/// looked up by this name, then its parents by name, then the default.
pub const IMPACT_MATERIALS: [&str; 4] = [
    "tough_terrain_grass",
    "tough_terrain",
    "tough",
    "default_material",
];

// weap (weapons.md 1, 4, 5 and weapon-sounds.md 1)
const WEAP_ATTACHMENTS: usize = 0xBC;
const WEAP_AMMO_PICKUP: usize = 0x3A0;
const WEAP_FIRST_PERSON: usize = 0x3FC;
const WEAP_TRIGGERS: usize = 0x430;
const WEAP_BARRELS: usize = 0x43C;
const FIRST_PERSON_SIZE: usize = 32;
const TRIGGER_SIZE: usize = 144;
const BARREL_SIZE: usize = 308;
const BARREL_PROJECTILE: usize = 0x98;
const BARREL_FIRING_EFFECTS: usize = 0x128;
const FIRING_EFFECT_SIZE: usize = 0x80;
// effe (weapon-sounds.md 2)
const EFFECT_EVENTS: usize = 0x2C;
const EVENT_SIZE: usize = 0x44;
const EVENT_PARTS: usize = 0x20;
const PART_SIZE: usize = 0x60;
const PART_TYPE: usize = 0x10;
// jmad (weapon-sounds.md 3)
const JMAD_SOUNDS: usize = 0x20;
const JMAD_SOUND_SIZE: usize = 20;
const JMAD_ANIMATIONS: usize = 0x50;
const ANIMATION_SIZE: usize = 136;
const ANIMATION_SOUND_EVENTS: usize = 0x38;
const SOUND_EVENT_SIZE: usize = 8;
// proj (weapon-sounds.md 4)
const PROJ_MATERIAL_EFFECTS: usize = 0x80;
const PROJ_DETONATION: usize = 0x148;
const PROJ_IMPACT: usize = 0x1D0;
// foot (weapon-sounds.md 5)
const FOOT_TYPE_SIZE: usize = 36;
const FOOT_IMPACT_TYPE: u32 = 15;
const FOOT_ENTRY_SIZE: usize = 40;

/// Reads weapon sounds from one map.
pub struct WeaponSoundReader<'a> {
    cache: &'a CacheFile,
}

impl<'a> WeaponSoundReader<'a> {
    pub fn new(cache: &'a CacheFile) -> WeaponSoundReader<'a> {
        WeaponSoundReader { cache }
    }

    /// Every `weap` tag of the map, by tag index.
    pub fn all(&self) -> Result<HashMap<u32, WeaponSounds>> {
        let mut out = HashMap::new();
        for t in self.cache.tags.of_class("weap") {
            out.insert(t.index, self.weapon(t.index)?);
        }
        Ok(out)
    }

    fn tag(&self, index: u32, class: &str) -> Result<(&'a TagEntry, usize)> {
        let c = self.cache;
        let t = c
            .tags
            .get(index)
            .filter(|t| c.tags.class_of(t).code == class)
            .ok_or_else(|| Error::Invalid(format!("tag {index} is not a {class}")))?;
        Ok((t, c.tag_offset(t)?))
    }

    fn class_of(&self, index: u32) -> &str {
        self.cache
            .tags
            .get(index)
            .map_or("", |t| self.cache.tags.class_of(t).code.as_str())
    }

    /// A tag reference's index when it names a tag (any class).
    fn reference(&self, at: usize) -> Result<Option<u32>> {
        Ok(self.cache.reader_at(at).tag_ref()?.index)
    }

    fn name(&self, at: usize) -> Result<String> {
        let id = self.cache.reader_at(at).string_id()?;
        Ok(self.cache.string(id).unwrap_or("").to_string())
    }

    /// The `snd!` tags an effect starts: the sound parts of each of its
    /// events. A reference straight to a `snd!` (some empty clicks) is
    /// that sound.
    pub fn effect_sounds(&self, tag: Option<u32>) -> Result<Vec<u32>> {
        let mut out = Vec::new();
        let Some(tag) = tag else { return Ok(out) };
        match self.class_of(tag) {
            "snd!" => out.push(tag),
            "effe" => {
                let (_, o) = self.tag(tag, "effe")?;
                let c = self.cache;
                for ev in c.reader_at(o + EFFECT_EVENTS).block()?.elements(EVENT_SIZE) {
                    for p in c.reader_at(ev + EVENT_PARTS).block()?.elements(PART_SIZE) {
                        if let Some(s) = self.reference(p + PART_TYPE)?
                            && self.class_of(s) == "snd!"
                        {
                            push_new(&mut out, s);
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(out)
    }

    /// The sounds of the first animation of a first-person graph whose
    /// name (after `first_person:`) passes `wanted`, trying each test in
    /// turn. Dual-wield animations are skipped.
    fn animation_sounds(&self, jmad: u32, wanted: &[&dyn Fn(&str) -> bool]) -> Result<Vec<u32>> {
        let (_, o) = self.tag(jmad, "jmad")?;
        let c = self.cache;
        let refs: Vec<Option<u32>> = c
            .reader_at(o + JMAD_SOUNDS)
            .block()?
            .elements(JMAD_SOUND_SIZE)
            .map(|e| self.reference(e))
            .collect::<Result<_>>()?;
        let animations = c.reader_at(o + JMAD_ANIMATIONS).block()?;
        for test in wanted {
            for a in animations.elements(ANIMATION_SIZE) {
                let name = self.name(a)?;
                let Some(short) = name.strip_prefix("first_person:") else {
                    continue;
                };
                if !test(short) {
                    continue;
                }
                let mut out = Vec::new();
                for ev in c
                    .reader_at(a + ANIMATION_SOUND_EVENTS)
                    .block()?
                    .elements(SOUND_EVENT_SIZE)
                {
                    let i = c.reader_at(ev).i16()?;
                    if let Some(Some(s)) = usize::try_from(i).ok().and_then(|i| refs.get(i))
                        && self.class_of(*s) == "snd!"
                    {
                        push_new(&mut out, *s);
                    }
                }
                if !out.is_empty() {
                    return Ok(out);
                }
            }
        }
        Ok(Vec::new())
    }

    /// What a projectile sounds like landing on [`IMPACT_MATERIALS`].
    pub fn impact_sounds(&self, proj: u32) -> Result<Vec<u32>> {
        let (_, o) = self.tag(proj, "proj")?;
        let mut out = Vec::new();
        for at in [PROJ_DETONATION, PROJ_IMPACT] {
            for s in self.effect_sounds(self.reference(o + at)?)? {
                push_new(&mut out, s);
            }
        }
        let Some(foot) = self.reference(o + PROJ_MATERIAL_EFFECTS)? else {
            return Ok(out);
        };
        if self.class_of(foot) != "foot" {
            return Ok(out);
        }
        let (_, f) = self.tag(foot, "foot")?;
        let c = self.cache;
        let Some(kind) = c
            .reader_at(f)
            .block()?
            .element(FOOT_IMPACT_TYPE, FOOT_TYPE_SIZE)
        else {
            return Ok(out);
        };
        // Sounds (+0x0C) and effects (+0x18) are looked up separately.
        for (block, is_effect) in [(0x0C, false), (0x18, true)] {
            let entries: Vec<(String, usize)> = c
                .reader_at(kind + block)
                .block()?
                .elements(FOOT_ENTRY_SIZE)
                .map(|e| Ok((self.name(e + 0x20)?, e)))
                .collect::<Result<_>>()?;
            let Some(&(_, e)) = IMPACT_MATERIALS
                .iter()
                .find_map(|m| entries.iter().find(|(n, _)| n == m))
            else {
                continue;
            };
            if is_effect {
                for s in self.effect_sounds(self.reference(e)?)? {
                    push_new(&mut out, s);
                }
            } else {
                // The sound, then a second layer (sand sweeteners).
                for at in [e, e + 0x10] {
                    if let Some(s) = self.reference(at)?
                        && self.class_of(s) == "snd!"
                    {
                        push_new(&mut out, s);
                    }
                }
            }
        }
        Ok(out)
    }

    /// One weapon's sounds.
    pub fn weapon(&self, index: u32) -> Result<WeaponSounds> {
        let (t, o) = self.tag(index, "weap")?;
        let c = self.cache;
        let mut out = WeaponSounds {
            weapon: index,
            name: t.name().to_string(),
            ammo_pickup: self
                .reference(o + WEAP_AMMO_PICKUP)?
                .filter(|s| self.class_of(*s) == "snd!"),
            ..Default::default()
        };

        let barrels = c.reader_at(o + WEAP_BARRELS).block()?;
        let triggers = c.reader_at(o + WEAP_TRIGGERS).block()?;
        let (primary, secondary) = match triggers.element(0, TRIGGER_SIZE) {
            Some(tr) => (c.reader_at(tr + 0x08).i16()?, c.reader_at(tr + 0x0A).i16()?),
            None => (0, -1),
        };
        // (fire, empty, impact) of a barrel's first firing effect.
        let barrel = |i: i16| -> Result<(Vec<u32>, Vec<u32>, Vec<u32>)> {
            let Some(b) = u32::try_from(i)
                .ok()
                .and_then(|i| barrels.element(i, BARREL_SIZE))
            else {
                return Ok(Default::default());
            };
            let (mut fire, mut empty) = (Vec::new(), Vec::new());
            if let Some(fe) = c
                .reader_at(b + BARREL_FIRING_EFFECTS)
                .block()?
                .element(0, FIRING_EFFECT_SIZE)
            {
                fire = self.effect_sounds(self.reference(fe + 0x04)?)?;
                empty = self.effect_sounds(self.reference(fe + 0x24)?)?;
            }
            let impact = match self.reference(b + BARREL_PROJECTILE)? {
                Some(p) if self.class_of(p) == "proj" => self.impact_sounds(p)?,
                _ => Vec::new(),
            };
            Ok((fire, empty, impact))
        };
        let (fire, empty, impact) = barrel(primary)?;
        out.fire = fire;
        out.empty = empty;
        out.impact = impact;
        if secondary >= 0 && secondary != primary {
            let (fire, empty, impact) = barrel(secondary)?;
            out.fire_charged = fire;
            out.impact_charged = impact;
            if out.empty.is_empty() {
                out.empty = empty;
            }
        }

        for e in c
            .reader_at(o + WEAP_ATTACHMENTS)
            .block()?
            .elements(ATTACHMENT_SIZE)
        {
            let Some(l) = self.reference(e)?.filter(|l| self.class_of(*l) == "lsnd") else {
                continue;
            };
            match self.name(e + 0x18)?.as_str() {
                "primary_charged" if out.charge.is_none() => out.charge = Some(l),
                "loop_fire_single" => push_new(&mut out.fire_loops, l),
                _ => {}
            }
        }

        // Element 0 is the Spartan's first-person graph.
        if let Some(fp) = c
            .reader_at(o + WEAP_FIRST_PERSON)
            .block()?
            .element(0, FIRST_PERSON_SIZE)
            && let Some(jmad) = self.reference(fp + 0x10)?
            && self.class_of(jmad) == "jmad"
        {
            out.ready = self.animation_sounds(
                jmad,
                &[&|n: &str| n == "ready", &|n: &str| n.starts_with("ready:")],
            )?;
            out.reload = self.animation_sounds(
                jmad,
                &[
                    &|n: &str| n.starts_with("reload_full"),
                    &|n: &str| n.starts_with("reload_empty"),
                    // The shotgun loads a shell at a time.
                    &|n: &str| n.starts_with("reload_continue"),
                ],
            )?;
            out.fire_first_person = self.animation_sounds(
                jmad,
                &[&|n: &str| n == "fire_1" || n.starts_with("fire_1:")],
            )?;
        }
        Ok(out)
    }
}

fn push_new(v: &mut Vec<u32>, x: u32) {
    if !v.contains(&x) {
        v.push(x);
    }
}

impl WeaponSounds {
    /// Every `snd!` this weapon can play directly (not counting the
    /// looping sounds' own tracks).
    pub fn all_sounds(&self) -> impl Iterator<Item = u32> + '_ {
        self.fire
            .iter()
            .chain(&self.fire_charged)
            .chain(&self.fire_first_person)
            .chain(&self.empty)
            .chain(&self.reload)
            .chain(&self.ready)
            .chain(&self.impact)
            .chain(&self.impact_charged)
            .copied()
    }

    /// Every `lsnd` this weapon can play.
    pub fn all_loops(&self) -> impl Iterator<Item = u32> + '_ {
        self.fire_loops.iter().copied().chain(self.charge)
    }
}
