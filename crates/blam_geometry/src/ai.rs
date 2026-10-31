//! The scenario's AI: squad groups, squads, their cells and starting
//! locations, AI zones with their firing positions and areas, and the
//! character palette resolved through each character's parents to the
//! numbers an actor needs (unit, vitality, perception, movement, melee,
//! weapons and firing patterns), plus the unit (`bipd` / `crea`) behind
//! each character. Follows `docs/format/ai.md`; every offset there was
//! found and checked against the bytes of the user's `120_halo.map` and
//! cross-checked on `030_outskirts.map`.
//!
//! Units stay Halo's: world units, radians, seconds, +Z up.

use glam::Vec3;

use blam_cache::{Block, CacheFile, StringId, TagEntry};

use crate::{Error, Result};

/// Scenario blocks (offsets from the start of the `scnr` tag data).
const SQUAD_GROUPS: usize = 0x378;
const SQUADS: usize = 0x384;
const ZONES: usize = 0x390;
const CHARACTER_PALETTE: usize = 0x3A8;
const VEHICLE_PALETTE: usize = 0xF0;
const EQUIPMENT_PALETTE: usize = 0x108;
const WEAPON_PALETTE: usize = 0x120;

const SQUAD_GROUP_SIZE: usize = 40;
const SQUAD_SIZE: usize = 64;
const CELL_SIZE: usize = 96;
const LOCATION_SIZE: usize = 136;
const ZONE_SIZE: usize = 64;
const FIRING_POSITION_SIZE: usize = 40;
const AREA_SIZE: usize = 108;

/// A squad group: a named folder of squads (and of other groups), which
/// scripts address as one (`ai_place`, `ai_living_count` on a group).
#[derive(Clone, Debug)]
pub struct SquadGroup {
    pub name: String,
    /// The group it belongs to.
    pub parent: Option<u16>,
    /// Objective index, not used.
    pub objective: i16,
}

/// Where one actor of a cell starts.
#[derive(Clone, Debug)]
pub struct StartingLocation {
    pub name: String,
    /// Feet, Halo world units.
    pub position: Vec3,
    /// The structure BSP it lies in (index into the scenario's BSPs).
    pub bsp: Option<u16>,
    /// Yaw, pitch, roll as stored (radians).
    pub facing: [f32; 3],
    /// Overrides of the cell's character and weapon (palette indices).
    pub character: Option<u16>,
    pub weapon: Option<u16>,
    /// Inferred: actors spawned here at once (infection swarms); 0 = one.
    pub swarm: i16,
}

/// A cell: one kind of actor in a squad, how many, carrying what, and where
/// they start. Squads on 120_halo have one cell each, a few two.
#[derive(Clone, Debug)]
pub struct Cell {
    /// The first u32, not decoded (0, 12 or 15 on 120_halo).
    pub flags: u32,
    /// Actors placed.
    pub count: i16,
    /// Rank upgrade (inferred: 0 normal, 3 none, 4 all).
    pub major_upgrade: i16,
    /// Character palette index.
    pub character: Option<u16>,
    /// Weapon palette indices.
    pub weapon: Option<u16>,
    pub secondary_weapon: Option<u16>,
    /// Inferred grenade type (0 .. 3).
    pub grenades: i16,
    pub equipment: Option<u16>,
    /// Vehicle palette index (warthogs, choppers).
    pub vehicle: Option<u16>,
    /// The placement script's name, empty for none.
    pub placement_script: String,
    pub locations: Vec<StartingLocation>,
}

/// One squad.
#[derive(Clone, Debug)]
pub struct Squad {
    pub index: usize,
    pub name: String,
    /// The u32 after the name; 0 on every 120_halo squad.
    pub flags: u32,
    /// Team as stored: 0 = the character's unit's own team (every squad on
    /// 120_halo).
    pub team: i16,
    pub group: Option<u16>,
    /// The AI zone it fights in (index into [`AiData::zones`]).
    pub zone: Option<u16>,
    pub objective: Option<u16>,
    pub objective_task: Option<u16>,
    pub cells: Vec<Cell>,
}

impl Squad {
    /// The BSPs its starting locations lie in, as a mask.
    pub fn bsp_mask(&self) -> u32 {
        self.cells
            .iter()
            .flat_map(|c| &c.locations)
            .filter_map(|l| l.bsp)
            .filter(|b| *b < 32)
            .fold(0, |m, b| m | (1 << b))
    }

    /// Actors it places: each cell's count.
    pub fn actor_count(&self) -> usize {
        self.cells.iter().map(|c| c.count.max(0) as usize).sum()
    }
}

/// A point an actor may stand and fight from.
#[derive(Clone, Copy, Debug)]
pub struct FiringPosition {
    pub position: Vec3,
    pub bsp: Option<u16>,
    pub flags: u16,
    /// The area (within the zone) it belongs to.
    pub area: Option<u16>,
    /// The cluster of its BSP.
    pub cluster: Option<u16>,
    /// The ground normal as yaw and pitch (radians); pitch pi/2 = flat.
    pub normal: [f32; 2],
}

/// A named run of a zone's firing positions.
#[derive(Clone, Debug)]
pub struct Area {
    pub name: String,
    pub centre: Vec3,
    pub radius: f32,
    pub first_firing_position: u16,
    pub firing_position_count: u16,
}

/// An AI zone: where a squad may move, as firing positions and areas.
#[derive(Clone, Debug)]
pub struct AiZone {
    pub name: String,
    pub flags: u32,
    /// The structure BSP the zone is in.
    pub bsp: Option<u16>,
    pub firing_positions: Vec<FiringPosition>,
    pub areas: Vec<Area>,
}

/// A character's vitality: hit points of body and shield on normal and
/// legendary.
#[derive(Clone, Copy, Debug, Default)]
pub struct Vitality {
    pub body: f32,
    pub shield: f32,
    pub legendary_body: f32,
    pub legendary_shield: f32,
}

/// How a character sees and hears.
#[derive(Clone, Copy, Debug)]
pub struct Perception {
    /// World units.
    pub max_vision_distance: f32,
    /// Radians.
    pub central_vision_angle: f32,
    pub max_vision_angle: f32,
    pub peripheral_vision_angle: f32,
    /// World units: inside this it notices whatever the angle.
    pub peripheral_distance: f32,
    pub hearing_distance: f32,
}

impl Default for Perception {
    fn default() -> Self {
        // `ai\generic`'s numbers.
        Perception {
            max_vision_distance: 30.0,
            central_vision_angle: 20f32.to_radians(),
            max_vision_angle: 50f32.to_radians(),
            peripheral_vision_angle: 105f32.to_radians(),
            peripheral_distance: 4.0,
            hearing_distance: 40.0,
        }
    }
}

/// Close combat: when a character charges in and hits.
#[derive(Clone, Copy, Debug, Default)]
pub struct Charge {
    /// World units: inside this it considers a melee charge.
    pub melee_consider_range: f32,
    /// Chance it charges when it considers it.
    pub melee_chance: f32,
    /// World units: inside this it swings.
    pub melee_attack_range: f32,
    /// World units: beyond this it gives up the charge.
    pub melee_abort_range: f32,
    /// Seconds a charge may last.
    pub melee_attack_timeout: f32,
    /// Seconds between swings.
    pub melee_attack_delay: f32,
    /// Leap range (min, max), world units, and its chance.
    pub leap_range: [f32; 2],
    pub leap_chance: f32,
}

/// How a character uses one weapon.
#[derive(Clone, Debug)]
pub struct CharacterWeapon {
    pub weapon: Option<u32>,
    pub weapon_path: String,
    /// World units.
    pub maximum_firing_range: f32,
    pub minimum_firing_range: f32,
    /// Where it likes to stand (min, max), world units.
    pub normal_combat_range: [f32; 2],
    /// Seconds before its first burst (min, max).
    pub first_burst_delay: [f32; 2],
}

/// How a character fires one weapon on one difficulty.
#[derive(Clone, Copy, Debug, Default)]
pub struct FiringPattern {
    /// Shots per second.
    pub rate_of_fire: f32,
    /// 0..1: how well it follows a moving target, and leads it.
    pub target_tracking: f32,
    pub target_leading: f32,
    /// A burst starts within this of the target (world units, radians).
    pub burst_origin_radius: f32,
    pub burst_origin_angle: f32,
    /// Seconds (min, max).
    pub burst_duration: [f32; 2],
    pub burst_separation: [f32; 2],
    /// Radians.
    pub projectile_error: f32,
    pub max_error_angle: f32,
}

/// The firing patterns of one weapon: element 0 is normal, 1 legendary
/// (inferred from the doubled rates).
#[derive(Clone, Debug)]
pub struct WeaponPatterns {
    pub weapon: Option<u32>,
    pub patterns: Vec<FiringPattern>,
}

/// A character (`char`) with its parents' blocks filled in.
#[derive(Clone, Debug)]
pub struct Character {
    pub tag_index: u32,
    pub tag_path: String,
    /// The parent chain, nearest first, not including itself.
    pub parents: Vec<String>,
    /// Its unit: a `bipd` (or `crea` for infection forms) tag index.
    pub unit: Option<u32>,
    /// General properties: the actor type (9 flood combat, 10 infection, 11
    /// carrier, 12 monitor, 13 sentinel, 20 stalker, 21 tank, 22 ranged
    /// pure form, 7 marine, 0 elite on 120_halo).
    pub actor_type: i16,
    pub vitality: Vitality,
    pub perception: Perception,
    /// Movement: pathfinding radius, world units.
    pub pathfinding_radius: f32,
    pub charge: Charge,
    pub weapons: Vec<CharacterWeapon>,
    pub firing_patterns: Vec<WeaponPatterns>,
}

impl Character {
    /// How it uses `weapon`, or its first entry.
    pub fn weapon(&self, weapon: Option<u32>) -> Option<&CharacterWeapon> {
        self.weapons
            .iter()
            .find(|w| w.weapon.is_some() && w.weapon == weapon)
            .or_else(|| self.weapons.iter().find(|w| w.weapon.is_some()))
    }

    /// Its normal-difficulty firing pattern for `weapon`.
    pub fn firing_pattern(&self, weapon: Option<u32>) -> Option<FiringPattern> {
        self.firing_patterns
            .iter()
            .find(|p| p.weapon.is_some() && p.weapon == weapon)
            .or_else(|| self.firing_patterns.iter().find(|p| p.weapon.is_some()))
            .and_then(|p| p.patterns.first().copied())
    }
}

/// Teams as the unit tags store them (`+0xFC`).
pub mod team {
    pub const DEFAULT: i16 = 0;
    pub const PLAYER: i16 = 1;
    pub const HUMAN: i16 = 2;
    pub const COVENANT: i16 = 3;
    pub const FLOOD: i16 = 4;
    pub const SENTINEL: i16 = 5;
    pub const HERETIC: i16 = 6;
    pub const PROPHET: i16 = 7;
    pub const GUILTY: i16 = 8;
}

/// The unit an actor is: its model and body.
#[derive(Clone, Debug)]
pub struct Unit {
    pub tag_index: u32,
    pub tag_path: String,
    /// `bipd` or `crea`.
    pub class: String,
    /// Default team ([`team`]).
    pub team: i16,
    /// The model chain.
    pub model: Option<u32>,
    pub render_model: Option<u32>,
    pub collision_model: Option<u32>,
    pub animations: Option<u32>,
    /// Character physics (bipeds only; zero for creatures): flags, standing
    /// and crouching height, radius, world units.
    pub physics_flags: u32,
    pub height_standing: f32,
    pub height_crouching: f32,
    pub radius: f32,
    /// Eye height above the feet (bipeds).
    pub eye_height: f32,
    /// Inferred: flies rather than walks (physics flag bit 0; set only on
    /// the monitor and the sentinel).
    pub flies: bool,
    /// The melee damage effect (`jpt!`): the biped's own melee, the
    /// creature's bite.
    pub melee_damage: Option<u32>,
    /// Weapons the unit is built with (the sentinel's beam).
    pub weapons: Vec<u32>,
    /// Hit points from the model's damage info (body, shield).
    pub body_vitality: f32,
    pub shield_vitality: f32,
}

/// Everything the scenario says about AI.
#[derive(Clone, Debug, Default)]
pub struct AiData {
    pub squad_groups: Vec<SquadGroup>,
    pub squads: Vec<Squad>,
    pub zones: Vec<AiZone>,
    /// The character palette, resolved (`None` for an empty slot).
    pub characters: Vec<Option<Character>>,
    /// The scenario's weapon, vehicle and equipment palettes (tag indices).
    pub weapon_palette: Vec<Option<u32>>,
    pub vehicle_palette: Vec<Option<u32>>,
    pub equipment_palette: Vec<Option<u32>>,
}

impl AiData {
    pub fn squad(&self, name: &str) -> Option<&Squad> {
        self.squads.iter().find(|s| s.name == name)
    }

    pub fn group(&self, name: &str) -> Option<usize> {
        self.squad_groups.iter().position(|g| g.name == name)
    }

    /// The BSPs squad `s` lives in: those of its starting locations, else
    /// (trench squads store no BSP on their locations) its AI zone's.
    pub fn squad_bsps(&self, s: &Squad) -> u32 {
        let own = s.bsp_mask();
        if own != 0 {
            return own;
        }
        s.zone
            .and_then(|z| self.zones.get(z as usize))
            .and_then(|z| z.bsp)
            .filter(|b| *b < 32)
            .map_or(0, |b| 1 << b)
    }

    /// Every squad in group `g` or any group under it.
    pub fn squads_in_group(&self, g: usize) -> Vec<usize> {
        let mut groups = vec![g];
        let mut i = 0;
        while i < groups.len() {
            let parent = groups[i];
            for (k, sg) in self.squad_groups.iter().enumerate() {
                if sg.parent == Some(parent as u16) && !groups.contains(&k) {
                    groups.push(k);
                }
            }
            i += 1;
        }
        self.squads
            .iter()
            .filter(|s| s.group.is_some_and(|p| groups.contains(&(p as usize))))
            .map(|s| s.index)
            .collect()
    }

    /// The character a cell (or one of its locations) places.
    pub fn character_of(&self, cell: &Cell, location: Option<&StartingLocation>) -> Option<&Character> {
        let i = location.and_then(|l| l.character).or(cell.character)?;
        self.characters.get(i as usize)?.as_ref()
    }

    /// The weapon tag a cell (or location) hands its actor.
    pub fn weapon_of(&self, cell: &Cell, location: Option<&StartingLocation>) -> Option<u32> {
        let i = location.and_then(|l| l.weapon).or(cell.weapon)?;
        *self.weapon_palette.get(i as usize)?
    }
}

fn index(v: i16) -> Option<u16> {
    u16::try_from(v).ok()
}

fn tag_at(cache: &CacheFile, offset: usize) -> Result<Option<&TagEntry>> {
    Ok(cache.reader_at(offset).tag_ref()?.index.and_then(|i| cache.tags.get(i)))
}

fn palette(cache: &CacheFile, block: Block) -> Result<Vec<Option<u32>>> {
    block
        .elements(16)
        .map(|o| Ok(tag_at(cache, o)?.map(|t| t.index)))
        .collect()
}

fn name32(cache: &CacheFile, offset: usize) -> String {
    blam_cache::header_cstr(cache.bytes(), offset, 32)
}

fn sid(cache: &CacheFile, id: StringId) -> String {
    cache.string(id).unwrap_or_default().to_string()
}

/// Reads the scenario's AI.
pub fn read(cache: &CacheFile) -> Result<AiData> {
    let scnr = cache.scenario().ok_or_else(|| {
        Error::Invalid(format!("no scenario tag named {}", cache.header.scenario_path))
    })?;
    let s = cache.tag_offset(scnr)?;
    let block = |off: usize| cache.reader_at(s + off).block();

    let squad_groups = block(SQUAD_GROUPS)?
        .elements(SQUAD_GROUP_SIZE)
        .map(|o| -> Result<SquadGroup> {
            let mut r = cache.reader_at(o + 0x20);
            Ok(SquadGroup {
                name: name32(cache, o),
                parent: index(r.i16()?),
                objective: {
                    r.skip(2);
                    r.i16()?
                },
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let mut squads = Vec::new();
    for (i, o) in block(SQUADS)?.elements(SQUAD_SIZE).enumerate() {
        let mut r = cache.reader_at(o + 0x20);
        let flags = r.u32()?;
        let team = r.i16()?;
        let group = index(r.i16()?);
        let zone = index(r.i16()?);
        r.skip(2);
        let objective = index(r.i16()?);
        let objective_task = index(r.i16()?);
        let cells = cache
            .reader_at(o + 0x30)
            .block()?
            .elements(CELL_SIZE)
            .map(|c| read_cell(cache, c))
            .collect::<Result<Vec<_>>>()?;
        squads.push(Squad {
            index: i,
            name: name32(cache, o),
            flags,
            team,
            group,
            zone,
            objective,
            objective_task,
            cells,
        });
    }

    let zones = block(ZONES)?
        .elements(ZONE_SIZE)
        .map(|o| read_zone(cache, o))
        .collect::<Result<Vec<_>>>()?;

    let characters = block(CHARACTER_PALETTE)?
        .elements(16)
        .map(|o| -> Result<Option<Character>> {
            match tag_at(cache, o)? {
                Some(t) if cache.tags.class_of(t).code.as_str() == "char" => read_character(cache, t).map(Some),
                _ => Ok(None),
            }
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(AiData {
        squad_groups,
        squads,
        zones,
        characters,
        weapon_palette: palette(cache, block(WEAPON_PALETTE)?)?,
        vehicle_palette: palette(cache, block(VEHICLE_PALETTE)?)?,
        equipment_palette: palette(cache, block(EQUIPMENT_PALETTE)?)?,
    })
}

fn read_cell(cache: &CacheFile, o: usize) -> Result<Cell> {
    let mut r = cache.reader_at(o);
    let flags = r.u32()?;
    let count = r.i16()?;
    let major_upgrade = r.i16()?;
    let character = index(r.i16()?);
    let weapon = index(r.i16()?);
    let secondary_weapon = index(r.i16()?);
    let grenades = r.i16()?;
    let equipment = index(r.i16()?);
    let vehicle = index(r.i16()?);
    let locations = cache
        .reader_at(o + 0x54)
        .block()?
        .elements(LOCATION_SIZE)
        .map(|l| read_location(cache, l))
        .collect::<Result<Vec<_>>>()?;
    Ok(Cell {
        flags,
        count,
        major_upgrade,
        character,
        weapon,
        secondary_weapon,
        grenades,
        equipment,
        vehicle,
        placement_script: name32(cache, o + 0x18),
        locations,
    })
}

fn read_location(cache: &CacheFile, o: usize) -> Result<StartingLocation> {
    let mut r = cache.reader_at(o + 4);
    let name = r.string_id()?;
    let position = Vec3::from(r.f32x3()?);
    let _reference_frame = r.i16()?;
    let bsp = index(r.i16()?);
    let facing = r.f32x3()?;
    let mut r = cache.reader_at(o + 0x28);
    let character = index(r.i16()?);
    let weapon = index(r.i16()?);
    Ok(StartingLocation {
        name: sid(cache, name),
        position,
        bsp,
        facing,
        character,
        weapon,
        swarm: cache.reader_at(o + 0x36).i16()?,
    })
}

fn read_zone(cache: &CacheFile, o: usize) -> Result<AiZone> {
    let firing_positions = cache
        .reader_at(o + 0x28)
        .block()?
        .elements(FIRING_POSITION_SIZE)
        .map(|f| -> Result<FiringPosition> {
            let mut r = cache.reader_at(f);
            let position = Vec3::from(r.f32x3()?);
            let _reference_frame = r.i16()?;
            let bsp = index(r.i16()?);
            let flags = r.u16()?;
            r.skip(4);
            let area = index(r.i16()?);
            let cluster = index(r.i16()?);
            r.skip(2);
            Ok(FiringPosition {
                position,
                bsp,
                flags,
                area,
                cluster,
                normal: r.f32x2()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let areas = cache
        .reader_at(o + 0x34)
        .block()?
        .elements(AREA_SIZE)
        .map(|a| -> Result<Area> {
            let mut r = cache.reader_at(a + 0x38);
            Ok(Area {
                name: name32(cache, a),
                centre: Vec3::from(cache.reader_at(a + 0x24).f32x3()?),
                radius: cache.reader_at(a + 0x34).f32()?,
                first_firing_position: r.u16()?,
                firing_position_count: r.u16()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut r = cache.reader_at(o + 0x20);
    Ok(AiZone {
        name: name32(cache, o),
        flags: r.u32()?,
        bsp: index(r.i16()?),
        firing_positions,
        areas,
    })
}

/// Character root offsets.
const CHAR_PARENT: usize = 0x04;
const CHAR_UNIT: usize = 0x14;
const CHAR_CREATURE: usize = 0x24;
const CHAR_GENERAL: usize = 0x6C;
const CHAR_VITALITY: usize = 0x78;
const CHAR_PERCEPTION: usize = 0x90;
const CHAR_MOVEMENT: usize = 0xA8;
const CHAR_CHARGE: usize = 0xE4;
const CHAR_WEAPONS: usize = 0x174;
const CHAR_FIRING_PATTERNS: usize = 0x180;

const CHAR_WEAPON_SIZE: usize = 224;
const CHAR_PATTERN_SIZE: usize = 28;
const FIRING_PATTERN_SIZE: usize = 64;

/// A character and its parents, nearest first (at most 8, no loops).
fn chain<'a>(cache: &'a CacheFile, tag: &'a TagEntry) -> Result<Vec<&'a TagEntry>> {
    let mut out = vec![tag];
    while out.len() < 8 {
        let last = out[out.len() - 1];
        let Some(parent) = tag_at(cache, cache.tag_offset(last)? + CHAR_PARENT)? else {
            break;
        };
        if out.iter().any(|t| t.index == parent.index) {
            break;
        }
        out.push(parent);
    }
    Ok(out)
}

/// The first non-empty block at `off` along the chain, as Halo inherits
/// character properties.
fn inherited(cache: &CacheFile, chain: &[&TagEntry], off: usize) -> Result<Option<Block>> {
    for t in chain {
        let b = cache.reader_at(cache.tag_offset(t)? + off).block()?;
        if !b.is_empty() {
            return Ok(Some(b));
        }
    }
    Ok(None)
}

/// Reads one character, filling its blocks from its parents.
pub fn read_character(cache: &CacheFile, tag: &TagEntry) -> Result<Character> {
    let chain = chain(cache, tag)?;
    let mut unit = None;
    for t in &chain {
        let base = cache.tag_offset(t)?;
        if let Some(u) = tag_at(cache, base + CHAR_UNIT)?.or(tag_at(cache, base + CHAR_CREATURE)?) {
            unit = Some(u.index);
            break;
        }
    }
    let first = |off: usize| inherited(cache, &chain, off).map(|b| b.and_then(|b| b.offset));

    let actor_type = match first(CHAR_GENERAL)? {
        Some(o) => cache.reader_at(o + 4).i16()?,
        None => -1,
    };
    let vitality = match first(CHAR_VITALITY)? {
        Some(o) => {
            let mut r = cache.reader_at(o + 4);
            Vitality {
                body: r.f32()?,
                shield: r.f32()?,
                legendary_body: r.f32()?,
                legendary_shield: r.f32()?,
            }
        }
        None => Vitality::default(),
    };
    let perception = match first(CHAR_PERCEPTION)? {
        Some(o) => {
            let mut r = cache.reader_at(o + 4);
            Perception {
                max_vision_distance: r.f32()?,
                central_vision_angle: r.f32()?,
                max_vision_angle: r.f32()?,
                peripheral_vision_angle: r.f32()?,
                peripheral_distance: r.f32()?,
                hearing_distance: r.f32()?,
            }
        }
        None => Perception::default(),
    };
    let pathfinding_radius = match first(CHAR_MOVEMENT)? {
        Some(o) => cache.reader_at(o + 4).f32()?,
        None => 0.3,
    };
    let charge = match first(CHAR_CHARGE)? {
        Some(o) => {
            let mut r = cache.reader_at(o + 4);
            Charge {
                melee_consider_range: r.f32()?,
                melee_chance: r.f32()?,
                melee_attack_range: r.f32()?,
                melee_abort_range: r.f32()?,
                melee_attack_timeout: r.f32()?,
                melee_attack_delay: r.f32()?,
                leap_range: r.f32x2()?,
                leap_chance: r.f32()?,
            }
        }
        None => Charge::default(),
    };
    let weapons = match inherited(cache, &chain, CHAR_WEAPONS)? {
        Some(b) => b
            .elements(CHAR_WEAPON_SIZE)
            .map(|o| -> Result<CharacterWeapon> {
                let w = tag_at(cache, o + 4)?;
                let mut r = cache.reader_at(o + 0x14);
                let maximum_firing_range = r.f32()?;
                let minimum_firing_range = r.f32()?;
                let normal_combat_range = r.f32x2()?;
                Ok(CharacterWeapon {
                    weapon: w.map(|t| t.index),
                    weapon_path: w.map_or_else(String::new, |t| t.name().to_string()),
                    maximum_firing_range,
                    minimum_firing_range,
                    normal_combat_range,
                    first_burst_delay: cache.reader_at(o + 0x50).f32x2()?,
                })
            })
            .collect::<Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    let firing_patterns = match inherited(cache, &chain, CHAR_FIRING_PATTERNS)? {
        Some(b) => b
            .elements(CHAR_PATTERN_SIZE)
            .map(|o| -> Result<WeaponPatterns> {
                let patterns = cache
                    .reader_at(o + 0x10)
                    .block()?
                    .elements(FIRING_PATTERN_SIZE)
                    .map(|p| -> Result<FiringPattern> {
                        let mut r = cache.reader_at(p);
                        let rate_of_fire = r.f32()?;
                        let target_tracking = r.f32()?;
                        let target_leading = r.f32()?;
                        let burst_origin_radius = r.f32()?;
                        let burst_origin_angle = r.f32()?;
                        let mut r = cache.reader_at(p + 0x20);
                        let burst_duration = r.f32x2()?;
                        let burst_separation = r.f32x2()?;
                        let mut r = cache.reader_at(p + 0x34);
                        let projectile_error = r.f32()?;
                        r.skip(4);
                        Ok(FiringPattern {
                            rate_of_fire,
                            target_tracking,
                            target_leading,
                            burst_origin_radius,
                            burst_origin_angle,
                            burst_duration,
                            burst_separation,
                            projectile_error,
                            max_error_angle: r.f32()?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(WeaponPatterns {
                    weapon: tag_at(cache, o)?.map(|t| t.index),
                    patterns,
                })
            })
            .collect::<Result<Vec<_>>>()?,
        None => Vec::new(),
    };
    Ok(Character {
        tag_index: tag.index,
        tag_path: tag.name().to_string(),
        parents: chain[1..].iter().map(|t| t.name().to_string()).collect(),
        unit,
        actor_type,
        vitality,
        perception,
        pathfinding_radius,
        charge,
        weapons,
        firing_patterns,
    })
}

/// Unit offsets (object, then unit, then biped fields; `movement.md`).
const UNIT_TEAM: usize = 0xFC;
const UNIT_MELEE_DAMAGE: usize = 0x1A8;
const UNIT_WEAPONS: usize = 0x24C;
const BIPED_EYE: usize = 0x2E8;
const BIPED_PHYSICS_FLAGS: usize = 0x33C;
const BIPED_HEIGHT: usize = 0x340;
const CREATURE_MELEE_DAMAGE: usize = 0x1C4;
/// The model's damage info block, and its body and shield hit points.
const MODEL_DAMAGE_INFO: usize = 0x88;
const DAMAGE_BODY: usize = 0x0C;
const DAMAGE_SHIELD: usize = 0x20;

/// Body and shield hit points from a model's (`hlmt`) damage info.
pub fn model_vitality(cache: &CacheFile, hlmt: &TagEntry) -> Result<(f32, f32)> {
    let b = cache.reader_at(cache.tag_offset(hlmt)? + MODEL_DAMAGE_INFO).block()?;
    match b.offset {
        Some(o) => Ok((
            cache.reader_at(o + DAMAGE_BODY).f32()?,
            cache.reader_at(o + DAMAGE_SHIELD).f32()?,
        )),
        None => Ok((0.0, 0.0)),
    }
}

/// Reads a unit (`bipd`) or creature (`crea`) tag.
pub fn read_unit(cache: &CacheFile, tag: &TagEntry) -> Result<Unit> {
    let class = cache.tags.class_of(tag).code.as_str().to_string();
    let b = cache.tag_offset(tag)?;
    let model = tag_at(cache, b + 0x34)?;
    let (mut render_model, mut collision_model, mut animations) = (None, None, None);
    let (mut body_vitality, mut shield_vitality) = (0.0, 0.0);
    if let Some(m) = model {
        let h = cache.tag_offset(m)?;
        render_model = tag_at(cache, h)?.map(|t| t.index);
        collision_model = tag_at(cache, h + 0x10)?.map(|t| t.index);
        animations = tag_at(cache, h + 0x20)?.map(|t| t.index);
        (body_vitality, shield_vitality) = model_vitality(cache, m)?;
    }
    let team = cache.reader_at(b + UNIT_TEAM).i16()?;
    let biped = class == "bipd";
    let (physics_flags, height_standing, height_crouching, radius, eye_height) = if biped {
        let mut r = cache.reader_at(b + BIPED_HEIGHT);
        (
            cache.reader_at(b + BIPED_PHYSICS_FLAGS).u32()?,
            r.f32()?,
            r.f32()?,
            r.f32()?,
            cache.reader_at(b + BIPED_EYE).f32()?,
        )
    } else {
        (0, 0.0, 0.0, 0.0, 0.0)
    };
    let melee = tag_at(cache, b + if biped { UNIT_MELEE_DAMAGE } else { CREATURE_MELEE_DAMAGE })?
        .filter(|t| cache.tags.class_of(t).code.as_str() == "jpt!")
        .map(|t| t.index);
    let weapons = if biped {
        cache
            .reader_at(b + UNIT_WEAPONS)
            .block()?
            .elements(16)
            .filter_map(|o| tag_at(cache, o).ok().flatten())
            .filter(|t| cache.tags.class_of(t).code.as_str() == "weap")
            .map(|t| t.index)
            .collect()
    } else {
        Vec::new()
    };
    Ok(Unit {
        tag_index: tag.index,
        tag_path: tag.name().to_string(),
        class,
        team,
        model: model.map(|t| t.index),
        render_model,
        collision_model,
        animations,
        physics_flags,
        height_standing,
        height_crouching,
        radius,
        eye_height,
        flies: biped && physics_flags & 1 != 0,
        melee_damage: melee,
        weapons,
        body_vitality,
        shield_vitality,
    })
}
