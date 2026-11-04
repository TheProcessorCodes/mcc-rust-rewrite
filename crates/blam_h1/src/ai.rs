//! Halo CE's AI as the scenario stores it: encounters (named groups of
//! squads with a team and a BSP), their squads (an actor variant, counts
//! per difficulty, starting locations), firing positions, the actor
//! variant palette (`actv`: actor `actr`, unit `bipd`, weapon `weap`,
//! firing numbers, vitality), and the units, weapons, projectiles and
//! damage effects behind them. Written from `docs/format/halo1.md` 14,
//! checked on The Maw (`d40`) with `docs/format/scripts/probe_h1_ai.py`.
//!
//! Scripts name a squad `encounter/squad` and a whole encounter by its own
//! name ([`Ai::resolve`]); names compare without case (the scripts write
//! `enc6_1_flood/squada` for the squad stored as `squadA`).

use glam::Vec3;

use crate::cache::{Map, Reader};
use crate::{Error, Result};

/// Scenario offsets (`halo1.md` 14).
mod offsets {
    /// Actor variant palette, 0x10 each: an `actv` reference.
    pub const ACTOR_PALETTE: usize = 0x420;
    /// Encounters, 0xB0 each.
    pub const ENCOUNTERS: usize = 0x42C;
    /// Command lists, 0x60 each: the name first.
    pub const COMMAND_LISTS: usize = 0x438;
}

/// Ticks per second: projectile speeds are stored per tick.
const TICKS: f32 = 30.0;

/// Where one actor of a squad starts.
#[derive(Clone, Debug)]
pub struct Location {
    /// Feet, world units.
    pub position: Vec3,
    /// Yaw, radians.
    pub facing: f32,
    /// The BSP cluster it stands in.
    pub cluster: Option<u16>,
    /// Actor variant (palette index) overriding the squad's.
    pub actor: Option<u16>,
    /// The command list it runs when placed (index into the scenario's).
    pub command_list: Option<u16>,
}

/// One squad of an encounter.
#[derive(Clone, Debug)]
pub struct Squad {
    pub name: String,
    /// The platoon it belongs to (index into [`Encounter::platoons`]).
    pub platoon: Option<u16>,
    /// Actor variant (palette index); `None`: each location says.
    pub actor: Option<u16>,
    /// Initial and return state as stored (2 guarding, 9 ... inferred).
    pub initial_state: u16,
    pub return_state: u16,
    pub flags: u32,
    /// Actors placed on normal and on legendary ("insane").
    pub normal_count: i16,
    pub insane_count: i16,
    pub locations: Vec<Location>,
}

impl Squad {
    /// Actors placed on `difficulty` (0 easy, 1 normal, 2 heroic, 3
    /// legendary): the normal count on easy and normal, the legendary count
    /// on legendary, halfway (rounded up) on heroic.
    pub fn count(&self, difficulty: i16) -> i16 {
        let (n, i) = (self.normal_count.max(0), self.insane_count.max(0));
        match difficulty {
            ..=1 => n,
            2 => (n + i + 1) / 2,
            _ => i,
        }
    }
}

/// A point an encounter's actors fight from.
#[derive(Clone, Copy, Debug)]
pub struct FiringPosition {
    pub position: Vec3,
    /// The group letter, 0 = A.
    pub group: u16,
    pub cluster: Option<u16>,
}

/// One encounter.
#[derive(Clone, Debug)]
pub struct Encounter {
    pub name: String,
    /// Bit 0: not created when its BSP loads (scripts place it).
    pub flags: u32,
    /// As stored: 0 = each actor's unit's own team (every `d40` encounter).
    pub team: u16,
    /// The structure BSP it lives in (scenario index): the manual one
    /// (`+0x2A`) when flag 0x40 is set, else the precomputed one (`+0x7E`).
    pub bsp: u16,
    pub squads: Vec<Squad>,
    /// Platoon names (`+0x8C`, 0xAC each): groups of the squads.
    pub platoons: Vec<String>,
    pub firing_positions: Vec<FiringPosition>,
}

impl Encounter {
    /// Scripts place it; the engine does not create it on its own.
    pub fn scripted(&self) -> bool {
        self.flags & 1 != 0
    }
}

/// How an actor variant fights and how tough it is (`actv`).
#[derive(Clone, Debug)]
pub struct ActorVariant {
    pub tag: u32,
    pub path: String,
    /// `actr`, `bipd` (or a vehicle for turret gunners), `weap`.
    pub actor: Option<u32>,
    pub unit: Option<u32>,
    pub weapon: Option<u32>,
    /// From the `actr`: 0 elite, 1 jackal, 2 grunt, 3 hunter, 4 engineer,
    /// 5 assassin, 6 player, 7 marine, 8 crew, 9 combat form, 10 infection
    /// form, 11 carrier form, 12 monitor, 13 sentinel, 14 none, 15 mounted
    /// weapon.
    pub actor_type: u16,
    /// From the `actr`, world units and radians.
    pub vision_distance: f32,
    pub central_vision_angle: f32,
    pub max_vision_angle: f32,
    pub peripheral_vision_angle: f32,
    pub peripheral_distance: f32,
    pub hearing_distance: f32,
    /// Ranged combat (`actv +0x74`..): world units, shots a second,
    /// radians, seconds.
    pub max_firing_distance: f32,
    pub rate_of_fire: f32,
    pub projectile_error: f32,
    pub first_burst_delay: [f32; 2],
    pub desired_combat_range: [f32; 2],
    pub target_tracking: f32,
    pub target_leading: f32,
    pub burst_origin_radius: f32,
    pub burst_origin_angle: f32,
    pub burst_duration: [f32; 2],
    pub burst_separation: [f32; 2],
    /// Overrides of the unit's vitality (`+0x200`, `+0x204`); 0: the
    /// unit's own.
    pub body_vitality: f32,
    pub shield_vitality: f32,
}

/// A unit (`bipd`): its model, animations, size, team and vitality.
#[derive(Clone, Debug)]
pub struct Unit {
    pub tag: u32,
    pub path: String,
    pub class: [u8; 4],
    /// `mod2`, `antr`, `coll`.
    pub model: Option<u32>,
    pub animations: Option<u32>,
    pub collision: Option<u32>,
    /// Default team (unit `+0x180`): 1 player, 2 human, 3 covenant, 4
    /// flood, 5 sentinel.
    pub team: u16,
    /// Melee damage (`jpt!`, unit `+0x288`).
    pub melee: Option<u32>,
    /// Biped standing camera height (`+0x400`), standing collision height
    /// (`+0x424`) and radius (`+0x42C`), world units; 0 when not a biped.
    pub eye_height: f32,
    pub height: f32,
    pub radius: f32,
    /// From the collision model (`coll +0x8`, `+0xCC`).
    pub body_vitality: f32,
    pub shield_vitality: f32,
}

/// A damage effect (`jpt!`): radius (min, max, world units), damage lower
/// bound and upper bounds.
#[derive(Clone, Debug)]
pub struct Damage {
    pub path: String,
    pub radius: [f32; 2],
    pub lower: f32,
    pub upper: [f32; 2],
}

/// A projectile (`proj`): speeds in world units a second (stored per tick),
/// range, and the damage of a hit (its impact damage, else its
/// detonation's).
#[derive(Clone, Debug)]
pub struct Projectile {
    pub path: String,
    pub initial_velocity: f32,
    pub final_velocity: f32,
    pub maximum_range: f32,
    pub damage: Option<Damage>,
}

/// One trigger of a weapon (`weap +0x4FC`, 0x114 each).
#[derive(Clone, Debug)]
pub struct Trigger {
    pub rounds_per_second: [f32; 2],
    pub projectiles_per_shot: i16,
    pub minimum_error: f32,
    pub error_angle: [f32; 2],
    pub projectile: Option<Projectile>,
}

/// A weapon (`weap`).
#[derive(Clone, Debug)]
pub struct Weapon {
    pub tag: u32,
    pub path: String,
    pub model: Option<u32>,
    pub triggers: Vec<Trigger>,
}

/// The scenario's AI.
#[derive(Clone, Debug, Default)]
pub struct Ai {
    pub encounters: Vec<Encounter>,
    /// The actor variant palette (`None`: an empty or unreadable slot).
    pub palette: Vec<Option<ActorVariant>>,
    pub command_lists: Vec<String>,
}

/// What a script's AI name points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiName {
    Encounter(usize),
    Squad(usize, usize),
    /// An encounter's platoon: the squads whose platoon it is.
    Platoon(usize, usize),
}

impl Ai {
    /// `encounter`, `encounter/squad` or `encounter/platoon`, without
    /// case.
    pub fn resolve(&self, name: &str) -> Option<AiName> {
        let (enc, squad) = match name.split_once('/') {
            Some((e, s)) => (e, Some(s)),
            None => (name, None),
        };
        let e = self.encounters.iter().position(|x| x.name.eq_ignore_ascii_case(enc))?;
        match squad {
            None => Some(AiName::Encounter(e)),
            Some(s) => {
                let enc = &self.encounters[e];
                if let Some(k) = enc.squads.iter().position(|x| x.name.eq_ignore_ascii_case(s)) {
                    return Some(AiName::Squad(e, k));
                }
                let p = enc.platoons.iter().position(|x| x.eq_ignore_ascii_case(s))?;
                Some(AiName::Platoon(e, p))
            }
        }
    }
}

fn opt(v: i16) -> Option<u16> {
    u16::try_from(v).ok()
}

/// A tag reference that names a tag of `class`.
fn tag_of(map: &Map, r: &Reader<'_>, off: usize, class: &[u8; 4]) -> Result<Option<u32>> {
    let t = r.tag_ref(off)?;
    Ok(map.tag(t).filter(|t| t.is(class)).map(|t| t.id))
}

/// Reads the scenario's encounters, actor variant palette and command list
/// names.
pub fn read(map: &Map) -> Result<Ai> {
    let scnr = map
        .tag_data(map.scenario)
        .ok_or_else(|| Error::Invalid("scenario has no data".into()))?;
    let mut out = Ai::default();
    for p in scnr.block(offsets::ACTOR_PALETTE, 0x10)? {
        let t = p.tag_ref(0)?;
        out.palette.push(match map.tag(t).filter(|t| t.is(b"actv")) {
            Some(t) => actor_variant(map, t.id).ok(),
            None => None,
        });
    }
    for e in scnr.block(offsets::ENCOUNTERS, 0xB0)? {
        let flags = e.u32(0x20)?;
        let bsp = if flags & 0x40 != 0 { e.u16(0x2A)? } else { e.u16(0x7E)? };
        let mut squads = Vec::new();
        for s in e.block(0x80, 0xE8)? {
            let mut locations = Vec::new();
            for l in s.block(0xD0, 0x1C)? {
                locations.push(Location {
                    position: l.vec3(0)?,
                    facing: l.f32(0xC)?,
                    cluster: opt(l.i16(0x10)?),
                    actor: opt(l.i16(0x18)?),
                    command_list: opt(l.i16(0x1A)?),
                });
            }
            squads.push(Squad {
                name: s.str(0, 32),
                actor: opt(s.i16(0x20)?),
                platoon: opt(s.i16(0x22)?),
                initial_state: s.u16(0x24)?,
                return_state: s.u16(0x26)?,
                flags: s.u32(0x28)?,
                normal_count: s.i16(0x7C)?,
                insane_count: s.i16(0x7E)?,
                locations,
            });
        }
        let mut firing_positions = Vec::new();
        for f in e.block(0x98, 0x18)? {
            firing_positions.push(FiringPosition {
                position: f.vec3(0)?,
                group: f.u16(0xC)?,
                cluster: opt(f.i16(0xE)?),
            });
        }
        out.encounters.push(Encounter {
            name: e.str(0, 32),
            flags,
            team: e.u16(0x24)?,
            bsp,
            squads,
            platoons: e.block(0x8C, 0xAC)?.iter().map(|p| p.str(0, 32)).collect(),
            firing_positions,
        });
    }
    for c in scnr.block(offsets::COMMAND_LISTS, 0x60)? {
        out.command_lists.push(c.str(0, 32));
    }
    Ok(out)
}

/// One actor variant with its actor's perception and type.
pub fn actor_variant(map: &Map, tag: u32) -> Result<ActorVariant> {
    let path = map.tag_name(tag).unwrap_or("?").to_string();
    let v = map.tag_data(tag).ok_or_else(|| Error::Invalid(format!("{path}: no data")))?;
    let actor = tag_of(map, &v, 0x4, b"actr")?;
    let a = actor.and_then(|a| map.tag_data(a));
    let af = |off: usize, default: f32| a.and_then(|a| a.f32(off).ok()).filter(|x| x.is_finite() && *x > 0.0).unwrap_or(default);
    let pair = |off: usize| -> Result<[f32; 2]> { Ok([v.f32(off)?, v.f32(off + 4)?]) };
    Ok(ActorVariant {
        tag,
        path,
        actor,
        unit: {
            let t = v.tag_ref(0x14)?;
            map.tag(t).map(|t| t.id)
        },
        weapon: tag_of(map, &v, 0x64, b"weap")?,
        actor_type: a.and_then(|a| a.u16(0x14).ok()).unwrap_or(14),
        vision_distance: af(0x18, 30.0),
        central_vision_angle: af(0x1C, 0.873),
        max_vision_angle: af(0x20, 1.222),
        peripheral_vision_angle: af(0x28, 2.094),
        peripheral_distance: af(0x2C, 4.0),
        hearing_distance: af(0x4C, 40.0),
        max_firing_distance: v.f32(0x74)?,
        rate_of_fire: v.f32(0x78)?,
        projectile_error: v.f32(0x7C)?,
        first_burst_delay: pair(0x80)?,
        desired_combat_range: pair(0x9C)?,
        target_tracking: v.f32(0xBC)?,
        target_leading: v.f32(0xC0)?,
        burst_origin_radius: v.f32(0xCC)?,
        burst_origin_angle: v.f32(0xD0)?,
        burst_duration: pair(0xE0)?,
        burst_separation: pair(0xE8)?,
        body_vitality: v.f32(0x200)?,
        shield_vitality: v.f32(0x204)?,
    })
}

/// A unit (biped) tag.
pub fn unit(map: &Map, tag: u32) -> Result<Unit> {
    let t = map.tag(tag).ok_or_else(|| Error::Invalid(format!("unit {tag:#x} not found")))?;
    let path = t.name.clone();
    let class = t.class;
    let u = map.tag_data(tag).ok_or_else(|| Error::Invalid(format!("{path}: no data")))?;
    let biped = class == *b"bipd";
    let bf = |off: usize| if biped { u.f32(off).ok().filter(|x| x.is_finite() && *x > 0.0).unwrap_or(0.0) } else { 0.0 };
    let collision = tag_of(map, &u, 0x70, b"coll")?;
    let (body, shield) = collision
        .and_then(|c| map.tag_data(c))
        .map(|c| (c.f32(0x8).unwrap_or(0.0), c.f32(0xCC).unwrap_or(0.0)))
        .unwrap_or((0.0, 0.0));
    Ok(Unit {
        tag,
        path,
        class,
        model: tag_of(map, &u, 0x28, b"mod2")?,
        animations: tag_of(map, &u, 0x38, b"antr")?,
        collision,
        team: u.u16(0x180)?,
        melee: tag_of(map, &u, 0x288, b"jpt!")?,
        eye_height: bf(0x400),
        height: bf(0x424),
        radius: bf(0x42C),
        body_vitality: body,
        shield_vitality: shield,
    })
}

/// A damage effect.
pub fn damage(map: &Map, tag: u32) -> Result<Damage> {
    let path = map.tag_name(tag).unwrap_or("?").to_string();
    let d = map.tag_data(tag).ok_or_else(|| Error::Invalid(format!("{path}: no data")))?;
    Ok(Damage {
        path,
        radius: [d.f32(0)?, d.f32(4)?],
        lower: d.f32(0x1D0)?,
        upper: [d.f32(0x1D4)?, d.f32(0x1D8)?],
    })
}

/// A projectile: object `+0x17C` onwards; maximum range `+0x1C8`, initial
/// and final velocity `+0x1E4`, `+0x1E8` (per tick), attached detonation
/// damage `+0x214`, impact damage `+0x224`.
pub fn projectile(map: &Map, tag: u32) -> Result<Projectile> {
    let path = map.tag_name(tag).unwrap_or("?").to_string();
    let p = map.tag_data(tag).ok_or_else(|| Error::Invalid(format!("{path}: no data")))?;
    let read = |off: usize| -> Option<Damage> { tag_of(map, &p, off, b"jpt!").ok().flatten().and_then(|t| damage(map, t).ok()) };
    let damage = read(0x224).filter(|d| d.upper[1] > 0.0).or_else(|| read(0x214));
    Ok(Projectile {
        path,
        initial_velocity: p.f32(0x1E4)? * TICKS,
        final_velocity: p.f32(0x1E8)? * TICKS,
        maximum_range: p.f32(0x1C8)?,
        damage,
    })
}

/// A weapon and its triggers: rounds a second `+0x4`, `+0x8`, projectiles
/// per shot (i16 `+0x6E`), minimum error `+0x78`, error angle `+0x7C`,
/// `+0x80`, projectile `+0x94`.
pub fn weapon(map: &Map, tag: u32) -> Result<Weapon> {
    let path = map.tag_name(tag).unwrap_or("?").to_string();
    let w = map.tag_data(tag).ok_or_else(|| Error::Invalid(format!("{path}: no data")))?;
    let mut triggers = Vec::new();
    for t in w.block(0x4FC, 0x114)? {
        let projectile = tag_of(map, &t, 0x94, b"proj")?.and_then(|p| projectile(map, p).ok());
        triggers.push(Trigger {
            rounds_per_second: [t.f32(0x4)?, t.f32(0x8)?],
            projectiles_per_shot: t.i16(0x6E)?,
            minimum_error: t.f32(0x78)?,
            error_angle: [t.f32(0x7C)?, t.f32(0x80)?],
            projectile,
        });
    }
    Ok(Weapon {
        tag,
        path,
        model: tag_of(map, &w, 0x28, b"mod2")?,
        triggers,
    })
}
