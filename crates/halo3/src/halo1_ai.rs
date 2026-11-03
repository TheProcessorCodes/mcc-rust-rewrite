//! Halo CE's encounters as `halo3_ai` actors: `blam_h1::ai` reads them,
//! and here they become the same `AiAssets` Halo 3's squads make, so the
//! map's scripts place, kill and count them through `halo3_core::ai`
//! unchanged (`ai_place enc6_1_flood/squada`, `ai_kill enc6_0_flood`,
//! `ai_living_count`).
//!
//! The mapping (`docs/format/halo1.md` 14):
//! - each encounter is a squad group and an AI zone (its firing positions),
//!   named as stored, lower case;
//! - each squad is a squad named `encounter/squad` with one cell: the
//!   squad's actor variant, its count for the difficulty, its starting
//!   locations (each may name its own actor variant) in the encounter's BSP;
//! - each platoon is a squad group under its encounter's, holding its
//!   squads (`ai_defend enc6_5_cov/evacs`);
//! - each actor variant (`actv`) is a character and its biped a unit: model
//!   (skinned on its nodes, at the default pose), team, size, vitality
//!   (the variant's, else the collision model's), melee damage, weapon
//!   with the variant's firing numbers.
//!
//! The weapons the actors carry become an arsenal of their own
//! (`halo3_weapons::assets::from_scene`, as Halo 2's): model, rate of
//! fire, spread, projectile speed and damage from the `weap`, `proj` and
//! `jpt!` tags. Walkers run at their run animation's root motion
//! (`halo1_anim.rs`); our own numbers: flying speeds and melee charge
//! ranges.

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use bevy::math::{Mat4, Vec3};
use blam_geometry::ai::{
    self as tags, AiData, Cell, Character, CharacterWeapon, Charge, FiringPattern, Perception, Squad, SquadGroup,
    StartingLocation, Vitality, WeaponPatterns,
};
use blam_geometry::render_model::{ModelParts, Piece};
use blam_geometry::weapons::{Barrel, Damage, Marker, Projectile, Trigger, Weapon};
use blam_geometry::{Bone, Mesh, Part};
use blam_h1::ai as h1;
use blam_h1::animation::Graph;
use halo3_ai::{ActorKind, AiAssets, ModelData, Style};
use halo3_textures::MaterialTextures;
use halo3_weapons::assets::Arsenal;

use crate::halo1_anim::{H1Moves, Moves};

/// A Halo CE datum as the tag index the AI and weapons code keys on.
fn index(datum: u32) -> u32 {
    datum & 0xFFFF
}

/// The map's encounters as actors, read for a mission.
pub(crate) struct H1Ai {
    pub assets: AiAssets,
    /// The weapons they carry.
    pub arsenal: Option<Arsenal>,
    /// Their animations, by actor variant (`halo1_anim.rs`).
    pub moves: H1Moves,
}

/// The map's encounters as actors. `None` (logged) when the map has no
/// encounters.
pub(crate) fn load(map: &blam_h1::Map, difficulty: i16) -> Option<H1Ai> {
    let started = std::time::Instant::now();
    let ai = match h1::read(map) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("ai: not read: {e}");
            return None;
        }
    };
    if ai.encounters.is_empty() {
        return None;
    }
    let data = ai_data(&ai, difficulty);

    // Models, one per biped model, and the actor kinds.
    let mut models: Vec<ModelData> = Vec::new();
    let mut by_mode: HashMap<u32, Option<usize>> = HashMap::new();
    let mut kinds = Vec::with_capacity(ai.palette.len());
    let mut characters = Vec::with_capacity(ai.palette.len());
    let mut graphs: HashMap<u32, Option<Arc<Graph>>> = HashMap::new();
    let mut moves = H1Moves::default();
    for v in &ai.palette {
        let mut variant_moves = None;
        let kind = v.as_ref().and_then(|v| {
            let unit = v.unit.and_then(|u| h1::unit(map, u).ok())?;
            let model = unit.model.and_then(|mode| {
                *by_mode.entry(mode).or_insert_with(|| {
                    let m = model_data(map, mode)?;
                    models.push(m);
                    Some(models.len() - 1)
                })
            });
            let graph = unit.animations.and_then(|antr| {
                graphs
                    .entry(antr)
                    .or_insert_with(|| match blam_h1::animation::read(map, antr) {
                        Ok(g) => Some(Arc::new(g)),
                        Err(e) => {
                            eprintln!("ai: {}: animations not read ({e})", map.tag_name(antr).unwrap_or("?"));
                            None
                        }
                    })
                    .clone()
            });
            let weapon = v.weapon.and_then(|w| map.tag_name(w)).unwrap_or("");
            let launcher = weapon.contains("fuel rod") || weapon.contains("rocket");
            variant_moves = graph.map(|g| Arc::new(Moves::pick(g, v.weapon.is_some(), launcher)));
            let mut k = kind(map, v, &unit, model, &models);
            // Walkers run at their run's own root motion, as Halo CE moves
            // them; flyers keep ours.
            let run = variant_moves
                .as_ref()
                .and_then(|m| m.moves[0].and_then(|i| m.graph.animations[i].as_ref()))
                .map_or(0.0, |a| a.root_speed());
            if !k.unit.flies && run > 0.3 {
                k.speed = run;
            }
            Some(k)
        });
        characters.push(kind.as_ref().map(|k| k.character.clone()));
        kinds.push(kind);
        moves.0.push(variant_moves);
    }
    let mut data = data;
    data.characters = characters;

    // The player's shields and health: the cyborg's collision model.
    let player_vitality = map
        .find(b"bipd", r"characters\cyborg\cyborg")
        .and_then(|t| h1::unit(map, t.id).ok())
        .map(|u| (u.body_vitality, u.shield_vitality))
        .filter(|(b, s)| *b > 0.0 && *s >= 0.0);

    let weapon_tags: Vec<u32> = {
        let mut w: Vec<u32> = ai.palette.iter().flatten().filter_map(|v| v.weapon).collect();
        w.sort_unstable();
        w.dedup();
        w
    };
    let arsenal = arsenal(map, &weapon_tags);
    eprintln!(
        "ai: halo ce: {} encounters, {} squads, {} actor variants ({} drawn from {} models, {} animated), {} weapons, in {:.1} s",
        ai.encounters.len(),
        data.squads.len(),
        kinds.iter().flatten().count(),
        kinds.iter().flatten().filter(|k| k.model.is_some()).count(),
        models.len(),
        moves.0.iter().flatten().count(),
        arsenal.as_ref().map_or(0, |a| a.kinds.len()),
        started.elapsed().as_secs_f32()
    );
    Some(H1Ai {
        assets: AiAssets {
            data,
            kinds,
            models,
            player_vitality,
            // Not read from Halo CE yet: the bike splatters with the
            // default numbers, enemies deal their damage unscaled, and
            // there is no shield meter.
            splatter: None,
            enemy_damage: None,
            hud: None,
        },
        arsenal,
        moves,
    })
}

/// Encounters, squads and platoons as Halo 3's squad groups, squads and
/// zones.
fn ai_data(ai: &h1::Ai, difficulty: i16) -> AiData {
    let mut data = AiData::default();
    // Encounters first (group e is encounter e), then their platoons.
    for e in &ai.encounters {
        data.squad_groups.push(SquadGroup {
            name: e.name.to_ascii_lowercase(),
            parent: None,
            objective: -1,
        });
    }
    let mut platoon_group: HashMap<(usize, u16), u16> = HashMap::new();
    for (ei, e) in ai.encounters.iter().enumerate() {
        for (pi, p) in e.platoons.iter().enumerate() {
            platoon_group.insert((ei, pi as u16), data.squad_groups.len() as u16);
            data.squad_groups.push(SquadGroup {
                name: format!("{}/{}", e.name, p).to_ascii_lowercase(),
                parent: Some(ei as u16),
                objective: -1,
            });
        }
        data.zones.push(tags::AiZone {
            name: e.name.to_ascii_lowercase(),
            flags: 0,
            bsp: Some(e.bsp),
            firing_positions: e
                .firing_positions
                .iter()
                .map(|f| tags::FiringPosition {
                    position: f.position,
                    bsp: Some(e.bsp),
                    flags: 0,
                    area: None,
                    cluster: f.cluster,
                    normal: [0.0, FRAC_PI_2],
                })
                .collect(),
            areas: Vec::new(),
        });
        for s in &e.squads {
            let index = data.squads.len();
            let group = s
                .platoon
                .and_then(|p| platoon_group.get(&(ei, p)).copied())
                .unwrap_or(ei as u16);
            data.squads.push(Squad {
                index,
                name: format!("{}/{}", e.name, s.name).to_ascii_lowercase(),
                flags: s.flags,
                team: e.team as i16,
                group: Some(group),
                zone: Some(ei as u16),
                objective: None,
                objective_task: None,
                cells: vec![Cell {
                    flags: 0,
                    count: s.count(difficulty),
                    major_upgrade: 0,
                    character: s.actor,
                    weapon: None,
                    secondary_weapon: None,
                    grenades: 0,
                    equipment: None,
                    vehicle: None,
                    placement_script: String::new(),
                    locations: s
                        .locations
                        .iter()
                        .map(|l| StartingLocation {
                            name: String::new(),
                            position: l.position,
                            bsp: Some(e.bsp),
                            facing: [l.facing, 0.0, 0.0],
                            character: l.actor.or(s.actor),
                            weapon: None,
                            swarm: 0,
                        })
                        .collect(),
                }],
            });
        }
    }
    data
}

/// Our own walking speeds by Halo CE actor type, wu/s (Halo CE moves its
/// actors by their animations' root motion, which is not read).
fn speed_of(actor_type: u16) -> f32 {
    match actor_type {
        10 => 2.8,      // infection form
        11 => 1.0,      // carrier form
        9 => 2.0,       // combat form
        12 | 13 => 1.6, // monitor, sentinel (fly)
        3 => 1.0,       // hunter
        2 => 1.6,       // grunt
        _ => 1.8,
    }
}

/// One actor variant as an actor kind.
fn kind(map: &blam_h1::Map, v: &h1::ActorVariant, u: &h1::Unit, model: Option<usize>, models: &[ModelData]) -> ActorKind {
    let flies = matches!(v.actor_type, 12 | 13);
    let bounds = model.and_then(|m| models.get(m)).map(|m| (m.min, m.max));
    // Bipeds carry their capsule; the infection form and the sentinel get
    // theirs from the model's bounds.
    let (height, radius) = match (u.height > 0.0, bounds) {
        (true, _) => (u.height, u.radius.max(0.05)),
        (false, Some((lo, hi))) => (
            (hi.z - lo.z).max(0.1),
            if u.radius > 0.0 { u.radius } else { ((hi.x - lo.x).max(hi.y - lo.y) * 0.5).clamp(0.05, 0.5) },
        ),
        (false, None) => (0.3, 0.15),
    };
    let eye = if u.eye_height > 0.0 { u.eye_height } else { height * 0.6 };
    let body = if v.body_vitality > 0.0 { v.body_vitality } else { u.body_vitality.max(1.0) };
    let shield = if v.shield_vitality > 0.0 { v.shield_vitality } else { u.shield_vitality.max(0.0) };
    let melee = u.melee.and_then(|t| h1::damage(map, t).ok()).map(damage);
    let style = match v.actor_type {
        10 | 11 => Style::Melee,
        9 => Style::Flood,
        _ => Style::Ranged,
    };
    // Our own charge numbers: the Flood rush in, everyone else swings only
    // when the foe is on top of them.
    let charge = match style {
        Style::Melee => Charge {
            melee_consider_range: 8.0,
            melee_chance: 1.0,
            melee_attack_range: if v.actor_type == 10 { 0.3 } else { 0.6 },
            melee_abort_range: 12.0,
            melee_attack_timeout: 5.0,
            melee_attack_delay: 1.0,
            ..Default::default()
        },
        Style::Flood => Charge {
            melee_consider_range: 4.0,
            melee_chance: 0.6,
            melee_attack_range: 0.7,
            melee_abort_range: 6.0,
            melee_attack_timeout: 4.0,
            melee_attack_delay: 1.2,
            ..Default::default()
        },
        Style::Ranged => Charge {
            melee_consider_range: 1.0,
            melee_chance: 0.3,
            melee_attack_range: 0.6,
            melee_abort_range: 1.5,
            melee_attack_timeout: 2.0,
            melee_attack_delay: 1.5,
            ..Default::default()
        },
    };
    let weapon = v.weapon.map(index);
    let character = Character {
        tag_index: index(v.tag),
        tag_path: v.path.clone(),
        parents: Vec::new(),
        unit: Some(index(u.tag)),
        actor_type: v.actor_type as i16,
        vitality: Vitality {
            body,
            shield,
            legendary_body: body,
            legendary_shield: shield,
        },
        perception: Perception {
            max_vision_distance: v.vision_distance,
            central_vision_angle: v.central_vision_angle,
            max_vision_angle: v.max_vision_angle,
            peripheral_vision_angle: v.peripheral_vision_angle,
            peripheral_distance: v.peripheral_distance,
            hearing_distance: v.hearing_distance,
        },
        pathfinding_radius: radius,
        charge,
        weapons: weapon
            .map(|w| CharacterWeapon {
                weapon: Some(w),
                weapon_path: map.tag_name(v.weapon.unwrap_or(u32::MAX)).unwrap_or("").to_string(),
                maximum_firing_range: v.max_firing_distance,
                minimum_firing_range: 0.0,
                normal_combat_range: v.desired_combat_range,
                first_burst_delay: v.first_burst_delay,
            })
            .into_iter()
            .collect(),
        firing_patterns: weapon
            .map(|w| WeaponPatterns {
                weapon: Some(w),
                patterns: vec![FiringPattern {
                    rate_of_fire: v.rate_of_fire,
                    target_tracking: v.target_tracking,
                    target_leading: v.target_leading,
                    burst_origin_radius: v.burst_origin_radius,
                    burst_origin_angle: v.burst_origin_angle,
                    burst_duration: v.burst_duration,
                    burst_separation: v.burst_separation,
                    projectile_error: v.projectile_error,
                    max_error_angle: 0.0,
                }],
            })
            .into_iter()
            .collect(),
    };
    let unit = tags::Unit {
        tag_index: index(u.tag),
        tag_path: u.path.clone(),
        class: String::from_utf8_lossy(&u.class).into_owned(),
        team: u.team as i16,
        model: None,
        render_model: u.model.map(index),
        collision_model: u.collision.map(index),
        animations: u.animations.map(index),
        physics_flags: 0,
        height_standing: height,
        height_crouching: height * 0.7,
        radius,
        eye_height: eye,
        flies,
        melee_damage: u.melee.map(index),
        weapons: weapon.into_iter().collect(),
        body_vitality: body,
        shield_vitality: shield,
    };
    ActorKind {
        character,
        unit,
        model,
        height,
        radius,
        eye,
        melee,
        body,
        shield,
        style,
        bursts: v.actor_type == 11,
        speed: speed_of(v.actor_type),
        anim: None,
    }
}

fn damage(d: h1::Damage) -> Damage {
    Damage {
        tag: d.path,
        radius: d.radius,
        lower_bound: d.lower,
        upper_bound: d.upper,
    }
}

/// Converts the looks of a decoded Halo CE model scene.
fn textures(looks: &[Option<blam_h1::ShaderLook>], mut bitmaps: HashMap<u32, blam_texture::Texture>) -> MaterialTextures {
    let looks = looks
        .iter()
        .enumerate()
        .map(|(i, l)| l.as_ref().map(|l| crate::halo2::convert(&l.look, i as u32, &mut bitmaps)))
        .collect();
    MaterialTextures {
        looks,
        bitmaps,
        lightmaps: Vec::new(),
    }
}

/// A marker as Halo 3's code names them (`right hand` -> `right_hand`).
fn marker(m: &blam_h1::skeleton::Marker, nodes: &[blam_h1::skeleton::Node], frames: &[Mat4]) -> Marker {
    let node = (m.node as usize).min(frames.len().saturating_sub(1));
    let local = Mat4::from_rotation_translation(m.rotation, m.translation);
    Marker {
        name: m.name.replace(' ', "_"),
        node: node as u16,
        node_name: nodes.get(node).map_or(String::new(), |n| n.name.clone()),
        position: m.translation,
        rotation: m.rotation,
        region: m.region as u8,
        permutation: m.permutation as u8,
        model: frames.get(node).copied().unwrap_or(Mat4::IDENTITY) * local,
    }
}

/// A biped's model: its meshes skinned on its nodes, at the default pose.
fn model_data(map: &blam_h1::Map, mode: u32) -> Option<ModelData> {
    let nodes = blam_h1::skeleton::nodes(map, mode).ok()?;
    let frames = blam_h1::skeleton::model_frames(&nodes);
    let models = blam_h1::decode_models(map, &[mode]);
    for f in &models.failed {
        eprintln!("ai: model {f}");
    }
    let blam_h1::Models {
        scene,
        looks,
        bitmaps,
        meshes,
        ..
    } = models;
    let meshes = meshes.into_iter().next().unwrap_or_default();
    if meshes.is_empty() {
        return None;
    }
    let bones = nodes
        .iter()
        .zip(&frames)
        .map(|(n, w)| Bone {
            // The head hit sphere looks for `head`.
            name: if n.name == "bip01 head" { "head".to_string() } else { n.name.clone() },
            parent: n.parent,
            position: n.translation,
            rotation: n.rotation,
            world: *w,
        })
        .collect();
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for &m in &meshes {
        for p in &scene.meshes[m as usize].positions {
            min = min.min(*p);
            max = max.max(*p);
        }
    }
    let markers = blam_h1::skeleton::markers(map, mode)
        .unwrap_or_default()
        .iter()
        .map(|m| marker(m, &nodes, &frames))
        .collect();
    Some(ModelData {
        mode: index(mode),
        parts: ModelParts {
            name: map.tag_name(mode).unwrap_or("").to_string(),
            pieces: meshes
                .iter()
                .map(|&m| Piece {
                    mesh: m,
                    transform: Mat4::IDENTITY,
                    mirrored: false,
                    bone: None,
                })
                .collect(),
            bones,
        },
        textures: textures(&looks, bitmaps),
        scene,
        markers,
        min,
        max,
    })
}

/// The actors' weapons as an arsenal: no placements, no loadout (the
/// player starts with none on a Halo CE map).
fn arsenal(map: &blam_h1::Map, tags: &[u32]) -> Option<Arsenal> {
    let read: Vec<h1::Weapon> = tags
        .iter()
        .filter_map(|&t| match h1::weapon(map, t) {
            Ok(w) => Some(w),
            Err(e) => {
                eprintln!("weapons: {} not read: {e}", map.tag_name(t).unwrap_or("?"));
                None
            }
        })
        .collect();
    if read.is_empty() {
        return None;
    }
    let modes: Vec<u32> = read.iter().map(|w| w.model.unwrap_or(u32::MAX)).collect();
    let models = blam_h1::decode_models(map, &modes);
    for f in models.failed.iter().filter(|f| !f.starts_with('?')) {
        eprintln!("  weapon model: {f}");
    }
    let blam_h1::Models {
        mut scene,
        looks,
        bitmaps,
        meshes,
        ..
    } = models;
    // Weapons without a model (the sentinel's beam, fired from inside its
    // body) get an empty triangle, so the arsenal keeps them.
    let none = scene.meshes.len() as u32;
    scene.meshes.push(Mesh {
        name: "no model".into(),
        positions: vec![Vec3::ZERO; 3],
        normals: vec![Vec3::Z; 3],
        uvs: vec![[0.0; 2]; 3],
        parts: vec![Part {
            material: u32::MAX,
            indices: vec![0, 1, 2],
        }],
        ..Default::default()
    });
    let weapons = read
        .into_iter()
        .zip(meshes)
        .map(|(w, meshes)| {
            let meshes = if meshes.is_empty() { vec![none] } else { meshes };
            (weapon(map, &w), meshes)
        })
        .collect();
    halo3_weapons::assets::from_scene(weapons, &scene, Vec::new(), &[], textures(&looks, bitmaps))
}

/// A Halo CE weapon in Halo 3's terms: one trigger on one barrel.
fn weapon(map: &blam_h1::Map, w: &h1::Weapon) -> Weapon {
    let markers = match w.model {
        Some(mode) => {
            let nodes = blam_h1::skeleton::nodes(map, mode).unwrap_or_default();
            let frames = blam_h1::skeleton::model_frames(&nodes);
            blam_h1::skeleton::markers(map, mode)
                .unwrap_or_default()
                .iter()
                .map(|m| marker(m, &nodes, &frames))
                .collect()
        }
        None => Vec::new(),
    };
    let barrels: Vec<Barrel> = w
        .triggers
        .iter()
        .map(|t| {
            // Charged weapons (the plasma pistol) store no rate: ours.
            let rps = if t.rounds_per_second[1] > 0.0 { t.rounds_per_second } else { [2.0, 2.0] };
            // One round carries all of a shot's pellets (the shotgun's 15).
            let pellets = t.projectiles_per_shot.max(1) as f32;
            Barrel {
                rounds_per_second: rps,
                shots_per_fire: [1, 1],
                fire_recovery_time: 0.0,
                magazine: None,
                rounds_per_shot: 1,
                minimum_error: t.minimum_error,
                error_angle: t.error_angle,
                projectile: t.projectile.as_ref().map(|p| Projectile {
                    tag: p.path.clone(),
                    initial_velocity: p.initial_velocity,
                    final_velocity: p.final_velocity,
                    maximum_range: p.maximum_range,
                    damage: p.damage.clone().map(|d| {
                        let mut d = damage(d);
                        d.lower_bound *= pellets;
                        d.upper_bound = d.upper_bound.map(|x| x * pellets);
                        d
                    }),
                }),
            }
        })
        .collect();
    Weapon {
        tag_index: index(w.tag),
        tag_path: w.path.clone(),
        class: String::new(),
        short_name: w.path.rsplit('\\').next().unwrap_or("").to_string(),
        model: String::new(),
        render_model: w.model.map(index),
        first_person: Vec::new(),
        zoom_levels: 0,
        zoom_range: [1.0, 1.0],
        holster_marker: String::new(),
        magazines: Vec::new(),
        triggers: barrels
            .iter()
            .enumerate()
            .map(|(i, b)| Trigger {
                // Held down at two rounds a second or more, else one a pull.
                behavior: if b.rounds_per_second[1] >= 2.0 { 0 } else { 1 },
                primary_barrel: Some(i as u16),
                secondary_barrel: None,
            })
            .collect(),
        barrels,
        markers,
        root: Mat4::IDENTITY,
    }
}

#[cfg(test)]
mod tests {
    //! The Warthog run's scripts driven through the run's volumes (as
    //! `halo3_script/tests/h1_warthog_run.rs` does) with the AI bridge:
    //! every squad, platoon or encounter they place or kill is one of ours,
    //! and places actors with a model. Skips quietly without Halo CE.

    use bevy::math::Vec3;
    use blam_geometry::TriggerVolume;
    use halo3_core::ai::{AiBridge, AiCommand, AiRef};
    use halo3_script::h1::{THE_MAW, runtime};
    use halo3_script::vm::WorldApi;
    use halo3_script::{AiHooks, BridgeAi};

    struct World(Vec3);

    impl WorldApi for World {
        fn player_position(&self) -> Option<Vec3> {
            Some(self.0)
        }
        fn teleport_player(&mut self, position: Vec3, _yaw: f32) {
            self.0 = position;
        }
        fn current_zone_set(&self) -> Option<usize> {
            Some(0)
        }
        fn switch_zone_set(&mut self, _zone_set: usize) {}
        fn effect(&mut self, _tag: &str, _at: Vec3) {}
    }

    fn middle(v: &TriggerVolume) -> Vec3 {
        let left = v.up.cross(v.forward);
        v.position + (v.forward * v.extents.x + left * v.extents.y + v.up * v.extents.z) * 0.5
    }

    #[test]
    fn the_runs_scripts_place_our_squads() {
        let Some(path) = game_paths::halo1_maps().map(|p| p.join("d40.map")).filter(|p| p.exists()) else {
            return;
        };
        let map = blam_h1::Map::open(&path).unwrap();
        let super::H1Ai { assets, arsenal, moves } = super::load(&map, 1).unwrap();
        let data = &assets.data;
        // Every actor variant the run places is animated.
        for (i, k) in assets.kinds.iter().enumerate() {
            if k.is_some() {
                let m = moves.0[i].as_ref().unwrap_or_else(|| panic!("variant {i} has no animations"));
                assert!(m.idle.is_some(), "variant {i}: no idle");
            }
        }
        let options = blam_h1::Options {
            textures: false,
            objects: false,
            bsps: Some(THE_MAW.bsps.to_vec()),
            ..Default::default()
        };
        let scene = blam_h1::decode(&map, &options).unwrap().scene;
        let mut rt = runtime(blam_h1::script::read(&map).unwrap(), &scene, &THE_MAW, 1);
        let start = blam_h1::object_named(&map, THE_MAW.start_at).unwrap().position;
        let bridge = AiBridge::default();
        let mut hooks = BridgeAi::new(bridge.clone());
        let mut world = World(start);
        let mut route: Vec<&TriggerVolume> = scene
            .trigger_volumes
            .iter()
            .filter(|v| v.name == "enc6_0" || v.name.starts_with("enc6_") || v.name.starts_with("enc7_") || v.name == "section7")
            .filter(|v| middle(v).z > 20.0 && !v.name.contains("retreat"))
            .collect();
        route.sort_by(|a, b| middle(a).x.total_cmp(&middle(b).x));
        let mut placed: Vec<String> = Vec::new();
        let mut killed: Vec<String> = Vec::new();
        for v in route {
            world.0 = middle(v);
            for _ in 0..40 {
                rt.step(&mut world, &mut hooks as &mut dyn AiHooks, None);
            }
            for c in std::mem::take(&mut bridge.0.lock().unwrap().queue) {
                match c {
                    AiCommand::Place { who: AiRef::Named(n), .. } => placed.push(n),
                    AiCommand::Kill { who: AiRef::Named(n) } => killed.push(n),
                    _ => {}
                }
            }
        }
        let squads = |name: &str| -> Vec<usize> {
            match (data.squad(name), data.group(name)) {
                (Some(s), _) => vec![s.index],
                (None, Some(g)) => data.squads_in_group(g),
                _ => Vec::new(),
            }
        };
        assert!(placed.iter().any(|n| n == "enc6_0_flood"), "{placed:?}");
        assert!(placed.iter().any(|n| n == "enc6_1_flood/squada"), "{placed:?}");
        assert!(placed.iter().any(|n| n == "enc7_7_flood"), "{placed:?}");
        for n in placed.iter().chain(&killed) {
            assert!(!squads(n).is_empty(), "{n} names no squad");
        }
        // What each placement puts down: actors whose kind has a model.
        let mut actors = 0;
        for n in &placed {
            for s in squads(n) {
                for cell in &data.squads[s].cells {
                    for (i, loc) in cell.locations.iter().take(cell.count.max(0) as usize).enumerate() {
                        let k = loc.character.or(cell.character).and_then(|c| assets.kinds[c as usize].as_ref());
                        let k = k.unwrap_or_else(|| panic!("{n}: location {i} has no actor kind"));
                        assert!(k.model.is_some(), "{n}: {} has no model", k.character.tag_path);
                        actors += 1;
                    }
                }
            }
        }
        println!("{} placements ({actors} actors), {} kills along the run", placed.len(), killed.len());
        // Their weapons are in the arsenal.
        let arsenal = arsenal.unwrap();
        for k in assets.kinds.iter().flatten() {
            for w in &k.unit.weapons {
                assert!(arsenal.kinds.iter().any(|a| a.weapon.tag_index == *w), "{}: weapon {w} missing", k.character.tag_path);
            }
        }
    }
}
