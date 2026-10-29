//! The scenario's squads as actors: placed at their starting locations,
//! drawn with their units' real models (bind pose until the animation code
//! poses them), carrying their weapons, hurt by the player's rounds, and
//! fighting with a first brain: they see and hear the player, charge and
//! swing (the Flood), hover and fire beams (Sentinels, the monitor), and
//! keep their weapons' combat range and fire bursts with the character's
//! own firing patterns. The player gets shields and health from his model
//! and comes back at the last quiet spot when he dies.
//!
//! Everything numeric comes from the tags where a tag has it
//! (`docs/format/ai.md`): vitality, sight, hearing, melee ranges and
//! damage, weapon ranges, burst lengths and spread, capsules, teams. Our own
//! numbers are marked where they are used (walking speeds live in the
//! animations, which are not read).
//!
//! Files: `model.rs` the bodies, `place.rs` squads in and out (auto
//! placement, script commands, the census), `brain.rs` seeing, moving and
//! fighting, `vitality.rs` damage, death and the player's shields.
//!
//! The script runtime drives it with `halo3_core::ai::AiCommand` and reads
//! `AiCensus`; the animation code poses `ActorSkeleton` joints from
//! `ActorActivity`. `--ai nearby|zone|off` picks the stand-in placement
//! while scripts do not place squads (`AutoPlace`).

mod brain;
mod model;
mod place;
mod vitality;

use bevy::prelude::*;

use blam_cache::CacheFile;
use blam_geometry::ai::{self as tags, AiData, Character, Unit};
use blam_geometry::weapons::Damage as DamageEffect;

pub use halo3_core::ai::{
    ActorActivity, ActorSkeleton, ActorState, ActorTag, AiBridge, AiCensus, AiCommand, AiRef, Allegiances, AutoPlace, Damage,
    PlayerVitality, Targets,
};

/// World units to metres.
pub(crate) const WU: f32 = blam_geometry::WORLD_UNIT_METRES;

/// How an actor fights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Runs in and hits: infection forms, stalkers, tanks, carriers, and
    /// combat forms without a gun.
    Melee,
    /// Combat forms with a gun: shoot, and charge when close.
    Flood,
    /// Keeps its weapon's range and fires.
    Ranged,
}

/// One character of the palette, ready to place.
pub struct ActorKind {
    pub character: Character,
    pub unit: Unit,
    /// Index into [`AiAssets::models`].
    pub model: Option<usize>,
    /// Body capsule, world units.
    pub height: f32,
    pub radius: f32,
    pub eye: f32,
    /// Melee damage (min, max) and its reach, world units.
    pub melee: Option<DamageEffect>,
    /// Hit points: body and shield.
    pub body: f32,
    pub shield: f32,
    pub style: Style,
    /// Carriers burst when they reach their target.
    pub bursts: bool,
    /// Our own: running speed, wu/s (Halo moves actors by their
    /// animations' root motion, which is not read).
    pub speed: f32,
}

/// The map's AI, CPU side, handed from `main`.
pub struct AiAssets {
    pub data: AiData,
    /// By character palette index.
    pub kinds: Vec<Option<ActorKind>>,
    pub models: Vec<model::ModelData>,
    /// The player's own model damage info (body, shield), when found.
    pub player_vitality: Option<(f32, f32)>,
}

impl AiAssets {
    /// The `weap` tags actors carry: cell and location weapons, and the
    /// units' built-in ones (the sentinel beam). Pass to
    /// `halo3_weapons::assets::load_with` so they have models and numbers.
    pub fn weapon_tags(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for s in &self.data.squads {
            for c in &s.cells {
                out.extend(self.data.weapon_of(c, None));
                for l in &c.locations {
                    out.extend(self.data.weapon_of(c, Some(l)));
                }
            }
        }
        for k in self.kinds.iter().flatten() {
            out.extend(k.unit.weapons.iter().copied());
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// Our own running speeds by actor type (wu/s; the player runs 2.25).
fn speed_of(actor_type: i16, flies: bool) -> f32 {
    match actor_type {
        _ if flies => 1.6,
        10 => 2.8, // infection form
        11 => 1.0, // carrier
        20 => 2.6, // stalker
        21 => 1.0, // tank
        22 => 0.0, // ranged pure form: rooted
        9 => 2.0,  // combat form
        _ => 1.8,
    }
}

fn read_kind(cache: &CacheFile, c: &Character, models: &mut Vec<model::ModelData>) -> Option<ActorKind> {
    let unit_tag = cache.tags.get(c.unit?)?;
    let unit = match tags::read_unit(cache, unit_tag) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("ai: {}: unit not read: {e}", c.tag_path);
            return None;
        }
    };
    let model = unit.render_model.and_then(|mode| {
        if let Some(i) = models.iter().position(|m| m.mode == mode) {
            return Some(i);
        }
        let m = model::decode(cache, mode)?;
        models.push(m);
        Some(models.len() - 1)
    });
    let bounds = model.map(|i| (models[i].min, models[i].max));
    // Bipeds carry their capsule; creatures (infection forms) get theirs
    // from the model's bounds.
    let (height, radius) = if unit.height_standing > 0.0 {
        (unit.height_standing, unit.radius.max(0.05))
    } else {
        let (lo, hi) = bounds.unwrap_or((Vec3::ZERO, Vec3::splat(0.2)));
        let r = ((hi.x - lo.x).max(hi.y - lo.y) * 0.5).clamp(0.05, 0.5);
        ((hi.z - lo.z).max(0.1), r)
    };
    let eye = if unit.eye_height > 0.0 { unit.eye_height } else { height * 0.6 };
    let melee = unit
        .melee_damage
        .and_then(|t| cache.tags.get(t))
        .and_then(|t| blam_geometry::weapons::read_damage(cache, t).ok());
    // Bipeds use the character's vitality; creatures their model's (the
    // infection form's character inherits `ai\generic`'s 50, its model says 1).
    let (body, shield) = if unit.class == "bipd" && c.vitality.body > 0.0 {
        (c.vitality.body, c.vitality.shield)
    } else {
        (unit.body_vitality.max(1.0), unit.shield_vitality)
    };
    let gun = !unit.weapons.is_empty();
    let style = match c.actor_type {
        10 | 11 | 20 | 21 => Style::Melee,
        9 => Style::Flood,
        _ if unit.flies || gun => Style::Ranged,
        _ => Style::Ranged,
    };
    Some(ActorKind {
        speed: speed_of(c.actor_type, unit.flies),
        bursts: c.actor_type == 11,
        character: c.clone(),
        model,
        height,
        radius,
        eye,
        melee,
        body,
        shield,
        style,
        unit,
    })
}

/// Reads the map's AI and decodes every character's model. `None` (logged)
/// when the map has no squads (multiplayer maps).
pub fn load(cache: &CacheFile) -> Option<AiAssets> {
    let started = std::time::Instant::now();
    let data = match tags::read(cache) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("ai: not read: {e}");
            return None;
        }
    };
    if data.squads.is_empty() {
        return None;
    }
    let mut models = Vec::new();
    let kinds: Vec<Option<ActorKind>> = data
        .characters
        .iter()
        .map(|c| c.as_ref().and_then(|c| read_kind(cache, c, &mut models)))
        .collect();
    if let Ok(r) = cache.resources() {
        r.clear_page_cache();
    }
    // The player's shields and health: the campaign Chief's model.
    let player_vitality = [
        r"objects\characters\masterchief\masterchief",
        r"objects\characters\masterchief\mp_masterchief\mp_masterchief",
    ]
    .iter()
    .find_map(|n| cache.tags.find("hlmt", n))
    .and_then(|h| tags::model_vitality(cache, h).ok())
    .filter(|(b, s)| *b > 0.0 && *s >= 0.0);
    eprintln!(
        "ai: {} squads, {} characters ({} drawn from {} models), in {:.1} s",
        data.squads.len(),
        kinds.iter().flatten().count(),
        kinds.iter().flatten().filter(|k| k.model.is_some()).count(),
        models.len(),
        started.elapsed().as_secs_f32()
    );
    Some(AiAssets {
        data,
        kinds,
        models,
        player_vitality,
    })
}

/// The map's AI, handed over by `main`.
#[derive(Resource)]
pub struct PendingAi(pub Option<AiAssets>);

/// The AI's systems, in order each frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum AiSet {
    Place,
    Think,
    Hurt,
}

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<AiCommand>()
            .add_message::<halo3_core::weapons::WeaponEvent>()
            .add_message::<Damage>()
            .init_resource::<AiCensus>()
            .init_resource::<halo3_core::ai::AiBridge>()
            .init_resource::<AutoPlace>()
            .init_resource::<Allegiances>()
            .init_resource::<Targets>()
            .init_resource::<PlayerVitality>()
            .configure_sets(Update, (AiSet::Place, AiSet::Think, AiSet::Hurt).chain())
            .add_plugins((place::PlacePlugin, brain::BrainPlugin, vitality::VitalityPlugin));
    }
}
