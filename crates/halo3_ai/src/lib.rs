//! The scenario's squads as actors: placed at their starting locations,
//! drawn with their units' real models (bind pose until the animation code
//! poses them), carrying their weapons, hurt by the player's rounds, and
//! fighting with a first brain: they see and hear the player, charge and
//! swing (the Flood), hover and fire beams (Sentinels, the monitor), and
//! keep their weapons' combat range and fire bursts with the character's
//! own firing patterns. The player gets shields and health from his model
//! and comes back at the last quiet spot when he dies. They fight him the
//! same way on foot, on the dirt bike and on the skateboard
//! (`halo3_core::ai::PlayerBody`), and the bike runs them down (`player.rs`).
//!
//! Everything numeric comes from the tags where a tag has it
//! (`docs/format/ai.md`): vitality, sight, hearing, melee ranges and
//! damage, weapon ranges, burst lengths and spread, capsules, teams. Our own
//! numbers are marked where they are used (walking speeds live in the
//! animations, which are not read).
//!
//! Files: `model.rs` the bodies, `place.rs` squads in and out (auto
//! placement, script commands, the census), `brain.rs` seeing, moving and
//! fighting, `vitality.rs` damage, death and the player's shields,
//! `player.rs` the player's body however he gets about, and splatter.
//!
//! The script runtime drives it with `halo3_core::ai::AiCommand` and reads
//! `AiCensus`; the animation code poses `ActorSkeleton` joints from
//! `ActorActivity`. `--ai nearby|zone|off` picks the stand-in placement
//! while scripts do not place squads (`AutoPlace`).

mod animate;
mod brain;
pub mod hud;
mod model;
mod place;
mod player;
mod vitality;

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use halo3_core::map::MapAppExt;

use blam_cache::CacheFile;
use blam_geometry::ai::{self as tags, AiData, Character, Unit};
use blam_geometry::weapons::Damage as DamageEffect;

/// A unit's model, CPU side: Halo CE's bipeds are built outside
/// (`halo3::halo1_ai`).
pub use model::ModelData;

pub use halo3_core::ai::{
    ActorActivity, ActorSkeleton, ActorState, ActorTag, AiBridge, AiCensus, AiCommand, AiRef, Allegiances, AutoPlace, Damage,
    Difficulty, PlayerBody, PlayerMode, PlayerRespawn, PlayerVitality, Targets,
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
    /// Running speed, wu/s: the run animation's root motion where the
    /// unit has one, else our own number.
    pub speed: f32,
    /// The unit's animations (`halo3_anim`), shared by its actors.
    pub anim: Option<std::sync::Arc<halo3_anim::AnimModel>>,
}

/// The map's AI, CPU side, handed from `main`.
pub struct AiAssets {
    pub data: AiData,
    /// By character palette index.
    pub kinds: Vec<Option<ActorKind>>,
    pub models: Vec<model::ModelData>,
    /// The player's own model damage info (body, shield), when found.
    pub player_vitality: Option<(f32, f32)>,
    /// What the dirt bike deals running into actors: the Mongoose's
    /// collision damage, when the map has it (`player.rs`).
    pub splatter: Option<blam_geometry::weapons::VehicleCollision>,
    /// The globals' scale on the damage enemies deal the player, by
    /// difficulty (easy .. legendary).
    pub enemy_damage: Option<[f32; 4]>,
    /// The shield meter's bitmaps and the shield sounds (`hud.rs`).
    pub hud: Option<hud::HudAssets>,
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

fn read_kind(
    cache: &CacheFile,
    c: &Character,
    models: &mut Vec<model::ModelData>,
    anims: &mut HashMap<u32, Option<Arc<halo3_anim::AnimModel>>>,
) -> Option<ActorKind> {
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
    let anim = anims
        .entry(unit_tag.index)
        .or_insert_with(|| match halo3_anim::AnimModel::load(cache, unit_tag) {
            Ok(m) => Some(m),
            Err(e) => {
                eprintln!("ai: {}: no animations ({e})", c.tag_path);
                None
            }
        })
        .clone();
    let gun = !unit.weapons.is_empty();
    let style = match c.actor_type {
        10 | 11 | 20 | 21 => Style::Melee,
        9 => Style::Flood,
        _ if unit.flies || gun => Style::Ranged,
        _ => Style::Ranged,
    };
    // Halo moves walkers by their run's root motion; the rooted ranged
    // pure form and the flyers keep ours.
    let own = speed_of(c.actor_type, unit.flies);
    let speed = match anim.as_deref().and_then(animate::run_speed) {
        Some(s) if own > 0.0 && !unit.flies => s,
        _ => own,
    };
    Some(ActorKind {
        speed,
        anim,
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
    let mut anims = HashMap::new();
    let kinds: Vec<Option<ActorKind>> = data
        .characters
        .iter()
        .map(|c| c.as_ref().and_then(|c| read_kind(cache, c, &mut models, &mut anims)))
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
    let splatter = match blam_geometry::weapons::read_vehicle_collision(cache, r"objects\vehicles\mongoose\mongoose") {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ai: vehicle collision damage not read: {e}");
            None
        }
    };
    let enemy_damage = tags::enemy_damage_scales(cache).ok().flatten();
    let hud = hud::read(cache);
    eprintln!(
        "ai: {} squads, {} characters ({} drawn from {} models, {} animated), in {:.1} s",
        data.squads.len(),
        kinds.iter().flatten().count(),
        kinds.iter().flatten().filter(|k| k.model.is_some()).count(),
        models.len(),
        kinds.iter().flatten().filter(|k| k.anim.is_some()).count(),
        started.elapsed().as_secs_f32()
    );
    Some(AiAssets {
        data,
        kinds,
        models,
        player_vitality,
        splatter,
        enemy_damage,
        hud,
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
            .init_map_resource::<AiCensus>()
            .init_map_resource::<halo3_core::ai::AiBridge>()
            .init_map_resource::<AutoPlace>()
            .init_map_resource::<Allegiances>()
            .init_map_resource::<Targets>()
            .init_map_resource::<PlayerVitality>()
            .init_map_resource::<PlayerBody>()
            .init_map_resource::<Difficulty>()
            .init_map_resource::<PlayerRespawn>()
            .init_map_resource::<player::Splatter>()
            .remove_on_map_unload::<PendingAi>()
            .remove_on_map_unload::<place::Ai>()
            .configure_sets(Update, (AiSet::Place, AiSet::Think, AiSet::Hurt).chain())
            .add_systems(
                Update,
                player::publish
                    .after(halo3_foot::walk)
                    .after(halo3_bike::BikeSet)
                    .before(AiSet::Place)
                    .before(halo3_weapons::FireSet)
                    .run_if(halo3_core::map::ready),
            )
            .add_systems(
                Update,
                player::ram
                    .in_set(AiSet::Hurt)
                    .before(vitality::HurtSet)
                    .run_if(halo3_core::pause::running)
                    .run_if(halo3_core::map::ready),
            )
            .add_plugins((place::PlacePlugin, brain::BrainPlugin, vitality::VitalityPlugin, animate::AnimatePlugin, hud::HudPlugin));
    }
}
