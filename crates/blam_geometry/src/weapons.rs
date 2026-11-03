//! Weapons: the `weap` tags (model, first-person model, magazines, triggers,
//! barrels, projectile and damage numbers, zoom), the markers on their
//! render models and on the Spartan's, and where the scenario puts weapons
//! and equipment on a map. Follows `docs/format/weapons.md`.
//!
//! Everything stays in Halo's units (world units, radians, seconds) and
//! frame (+Z up). Multiplayer maps place their weapons through the
//! scenario's weapon block; Halo 3 maps carry no item collections (`itmc`).

use glam::{Mat4, Quat, Vec3};

use blam_cache::{CacheFile, Resources, StringId, TagEntry};

use crate::materials::MaterialTable;
use crate::render_model::{self, ModelParts};
use crate::scenario;
use crate::scene::Scene;
use crate::Result;

/// The biped marker a held weapon is attached to, and the one the
/// off hand reaches for (on the weapon).
pub const RIGHT_HAND_MARKER: &str = "right_hand";
pub const LEFT_HAND_MARKER: &str = "left_hand";
/// The multiplayer Spartan's render model.
pub const SPARTAN_MODEL: &str = r"objects\characters\masterchief\mp_masterchief\mp_masterchief";

/// One marker of a render model: a named frame on a node.
#[derive(Clone, Debug)]
pub struct Marker {
    /// The marker group's name, e.g. `primary_trigger`, `right_hand`.
    pub name: String,
    /// The node it rides on (index into the model's nodes), and its name.
    pub node: u16,
    pub node_name: String,
    /// Relative to the node.
    pub position: Vec3,
    pub rotation: Quat,
    /// Region and permutation it belongs to; 255 = any.
    pub region: u8,
    pub permutation: u8,
    /// Marker frame in model space at the bind pose: node world * local.
    pub model: Mat4,
}

impl Marker {
    /// The marker frame relative to its node.
    pub fn local(&self) -> Mat4 {
        Mat4::from_rotation_translation(self.rotation, self.position)
    }
}

/// One node of a render model at its bind pose.
#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub parent: Option<u16>,
    /// Bind pose in model space.
    pub world: Mat4,
}

/// The nodes of a `mode` tag with their bind-pose model-space frames.
pub fn nodes(cache: &CacheFile, mode: &TagEntry) -> Result<Vec<Node>> {
    let base = cache.tag_offset(mode)?;
    let block = cache.reader_at(base + 0x30).block()?;
    let mut out: Vec<Node> = Vec::with_capacity(block.count as usize);
    for (i, o) in block.elements(96).enumerate() {
        let mut r = cache.reader_at(o);
        let name = r.string_id()?;
        let parent = r.i16()?;
        let position = Vec3::from(r.at(o + 12).f32x3()?);
        let q = r.at(o + 24).f32x4()?;
        let local = Mat4::from_rotation_translation(
            Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize(),
            position,
        );
        let parent = u16::try_from(parent).ok().filter(|p| (*p as usize) < out.len());
        let world = parent.map_or(Mat4::IDENTITY, |p| out[p as usize].world) * local;
        out.push(Node {
            name: string(cache, name, || format!("node {i}")),
            parent,
            world,
        });
    }
    Ok(out)
}

/// Every marker of a `mode` tag, group by group.
pub fn markers(cache: &CacheFile, mode: &TagEntry) -> Result<Vec<Marker>> {
    let nodes = nodes(cache, mode)?;
    let base = cache.tag_offset(mode)?;
    let groups = cache.reader_at(base + 0x3C).block()?;
    let mut out = Vec::new();
    for g in groups.elements(16) {
        let name = string(cache, cache.reader_at(g).string_id()?, String::new);
        let list = cache.reader_at(g + 4).block()?;
        for o in list.elements(36) {
            let mut r = cache.reader_at(o);
            let region = r.u8()?;
            let permutation = r.u8()?;
            let node = u16::from(r.u8()?);
            let position = Vec3::from(r.at(o + 4).f32x3()?);
            let q = r.at(o + 16).f32x4()?;
            let rotation = Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize();
            let (node_name, node_world) = nodes
                .get(node as usize)
                .map_or((String::new(), Mat4::IDENTITY), |n| (n.name.clone(), n.world));
            out.push(Marker {
                name: name.clone(),
                node,
                node_name,
                position,
                rotation,
                region,
                permutation,
                model: node_world * Mat4::from_rotation_translation(rotation, position),
            });
        }
    }
    Ok(out)
}

/// The first marker named `name`.
pub fn find_marker<'a>(markers: &'a [Marker], name: &str) -> Option<&'a Marker> {
    markers.iter().find(|m| m.name == name)
}

/// The multiplayer Spartan's markers (its `right_hand`, `left_hand`,
/// `weapon_back`, `weapon_thigh` ...), or `None` when the map lacks the
/// model.
pub fn spartan_markers(cache: &CacheFile) -> Result<Option<Vec<Marker>>> {
    match cache.tags.find("mode", SPARTAN_MODEL) {
        Some(mode) => markers(cache, mode).map(Some),
        None => Ok(None),
    }
}

/// Where a held weapon goes: the weapon model's origin (which is its grip:
/// the weapons that carry their own `right_hand` marker have it within a
/// few centimetres of the origin, see [`Weapon::grip`]) is put on the
/// biped's `right_hand` marker, which rides on the `r_hand` node. The
/// weapon's +X (towards `primary_trigger`) then runs along the marker's +X.
///
/// `hand_bone` is the current model-space frame of the marker's node
/// (`right_hand.node`; pass its bind-pose frame for a figure that does not
/// animate, which gives exactly `right_hand.model`). The result maps weapon
/// model space into the biped's model space; multiply the biped's world
/// transform on the left.
pub fn held_weapon_transform(hand_bone: Mat4, right_hand: &Marker) -> Mat4 {
    hand_bone * right_hand.local()
}

/// One magazine of a weapon.
#[derive(Clone, Debug, Default)]
pub struct Magazine {
    /// Rounds in the weapon when it spawns, loaded and spare together.
    pub rounds_total_initial: i16,
    /// The most rounds it can carry, loaded and spare together.
    pub rounds_total_maximum: i16,
    /// Magazine size.
    pub rounds_loaded_maximum: i16,
    /// Rounds put in per reload (1 for the shotgun, else the magazine size).
    pub rounds_reloaded: i16,
}

/// One trigger: what pulling it does.
#[derive(Clone, Debug, Default)]
pub struct Trigger {
    /// Firing behaviour as stored. Seen on riverworld: 0 held-down
    /// automatic (assault rifle, SMG, needler, plasma rifle), 1 one shot
    /// per pull (magnum, shotgun, sniper, rocket launcher), 4 one burst per
    /// pull (battle rifle, carbine), 2 and 6 charge-up (plasma pistol,
    /// Spartan laser). Names are inferred from which weapons use them.
    pub behavior: i16,
    pub primary_barrel: Option<u16>,
    pub secondary_barrel: Option<u16>,
}

/// One barrel: rate of fire, spread and what comes out.
#[derive(Clone, Debug, Default)]
pub struct Barrel {
    /// Rounds per second (min, max; they differ when the barrel spins up).
    pub rounds_per_second: [f32; 2],
    /// Shots per trigger pull (min, max): 3 for the battle rifle's burst.
    pub shots_per_fire: [i16; 2],
    /// Seconds before the next pull fires.
    pub fire_recovery_time: f32,
    /// The magazine it draws from; `None` for energy weapons.
    pub magazine: Option<u16>,
    /// Rounds used per shot.
    pub rounds_per_shot: i16,
    /// Spread: minimum error, and error angle (min, max), radians.
    pub minimum_error: f32,
    pub error_angle: [f32; 2],
    pub projectile: Option<Projectile>,
}

/// What a barrel fires. Halo 3 bullets are all projectiles; the sniper
/// rifle's travels at 1200 wu/s, which is near enough to a hitscan.
#[derive(Clone, Debug, Default)]
pub struct Projectile {
    pub tag: String,
    /// World units per second at launch and at the end.
    pub initial_velocity: f32,
    pub final_velocity: f32,
    /// The float before the velocities (40 wu for the assault rifle, 120
    /// for the sniper); read as the maximum range, name inferred.
    pub maximum_range: f32,
    pub damage: Option<Damage>,
}

impl Projectile {
    /// Fast enough to treat as an instant hit (laser, sniper, shotgun pellets).
    pub fn is_near_hitscan(&self) -> bool {
        self.initial_velocity >= 400.0
    }
}

/// A damage effect (`jpt!`), the numbers as the tag stores them before the
/// target's material and shield scaling.
#[derive(Clone, Debug, Default)]
pub struct Damage {
    pub tag: String,
    /// Area of effect (min, max), world units; zero for a bullet.
    pub radius: [f32; 2],
    pub lower_bound: f32,
    /// Damage (min, max).
    pub upper_bound: [f32; 2],
}

/// A first-person model: the weapon as its holder sees it.
#[derive(Clone, Debug)]
pub struct FirstPerson {
    /// The first-person `mode` tag index and path.
    pub render_model: Option<u32>,
    pub render_model_path: String,
    /// The first-person animation graph (`jmad`) path; index 0 is the
    /// Spartan's (`masterchief\fp`), index 1 the Elite's (`dervish\fp`).
    pub animations: String,
}

/// One `weap` tag.
#[derive(Clone, Debug)]
pub struct Weapon {
    pub tag_index: u32,
    pub tag_path: String,
    /// Weapon class and short name as the tag stores them, e.g. `rifle` / `ar`.
    pub class: String,
    pub short_name: String,
    /// `hlmt` path, and the third-person render model (`mode`) tag index.
    pub model: String,
    pub render_model: Option<u32>,
    pub first_person: Vec<FirstPerson>,
    /// Zoom: levels (0 none, 1, 2) and magnification range (min, max).
    pub zoom_levels: i16,
    pub zoom_range: [f32; 2],
    /// The biped marker it hangs on when not held: `weapon_back` or
    /// `weapon_thigh`; empty for turrets and objectives.
    pub holster_marker: String,
    pub magazines: Vec<Magazine>,
    pub triggers: Vec<Trigger>,
    pub barrels: Vec<Barrel>,
    /// Markers of the third-person model (`primary_trigger` = muzzle,
    /// `left_hand`, `muzzle_flash`, `primary_ejection`, ...).
    pub markers: Vec<Marker>,
    /// The model's root node frame (the `gun` node) at the bind pose, model
    /// space. Not always the origin: the sniper's sits 0.056 wu forward and
    /// 0.037 up. Weapon animations move the model's nodes from here; the
    /// meshes are stored in model space either way.
    pub root: Mat4,
}

impl Weapon {
    pub fn marker(&self, name: &str) -> Option<&Marker> {
        find_marker(&self.markers, name)
    }
    /// The muzzle: `primary_trigger` where the model has it, else the first
    /// `muzzle_flash` / `barrel` marker.
    pub fn muzzle(&self) -> Option<&Marker> {
        self.marker("primary_trigger")
            .or_else(|| self.marker("muzzle_flash"))
            .or_else(|| self.marker("barrel"))
    }
    /// The weapon's own `right_hand` marker (the grip), where it has one.
    pub fn grip(&self) -> Option<&Marker> {
        self.marker(RIGHT_HAND_MARKER)
    }
    /// Rounds per second of the first barrel, at its fastest.
    pub fn rate_of_fire(&self) -> f32 {
        self.barrels.first().map_or(0.0, |b| b.rounds_per_second[1])
    }
}

fn string(cache: &CacheFile, id: StringId, fallback: impl FnOnce() -> String) -> String {
    cache.string(id).map_or_else(fallback, str::to_string)
}

fn tag_at<'a>(cache: &'a CacheFile, offset: usize) -> Result<Option<&'a TagEntry>> {
    Ok(cache.reader_at(offset).tag_ref()?.index.and_then(|i| cache.tags.get(i)))
}

fn opt_index(v: i16) -> Option<u16> {
    u16::try_from(v).ok()
}

/// Reads a damage effect (`jpt!`) tag's radius and damage numbers.
pub fn read_damage(cache: &CacheFile, jpt: &TagEntry) -> Result<Damage> {
    let b = cache.tag_offset(jpt)?;
    Ok(Damage {
        tag: jpt.name().to_string(),
        radius: cache.reader_at(b).f32x2()?,
        lower_bound: cache.reader_at(b + 0x1C).f32()?,
        upper_bound: cache.reader_at(b + 0x20).f32x2()?,
    })
}

/// What a vehicle running into a unit deals (`docs/format/weapons.md` 6a):
/// the global collision damage effect's numbers, and the vehicle's
/// collision damage (`cddf`) scale and speed band.
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleCollision {
    /// `globals\collision_damage\collision` (`jpt!`): damage (min, max).
    pub damage: [f32; 2],
    /// The vehicle's `cddf` +0x00 (inferred: damage scale).
    pub scale: f32,
    /// The vehicle's `cddf` +0x08, +0x0C (inferred: the speed band, wu/s,
    /// over which a hit goes from nothing to the full damage).
    pub speeds: [f32; 2],
    /// The two tags read.
    pub effect_tag: String,
    pub vehicle_tag: String,
}

/// The splatter numbers for a vehicle (`vehi` tag path, e.g. the Mongoose):
/// the collision damage reference at +0x54 of its object block, and the
/// global `globals\collision_damage\collision` damage effect.
pub fn read_vehicle_collision(cache: &CacheFile, vehicle: &str) -> Result<Option<VehicleCollision>> {
    let Some(effect) = cache.tags.find("jpt!", r"globals\collision_damage\collision") else {
        return Ok(None);
    };
    let Some(vehi) = cache.tags.find("vehi", vehicle) else {
        return Ok(None);
    };
    let Some(cddf) = tag_at(cache, cache.tag_offset(vehi)? + 0x54)? else {
        return Ok(None);
    };
    if cache.tags.class_of(cddf).code.as_str() != "cddf" {
        return Ok(None);
    }
    let d = read_damage(cache, effect)?;
    let c = cache.tag_offset(cddf)?;
    Ok(Some(VehicleCollision {
        damage: [d.upper_bound[0].max(d.lower_bound), d.upper_bound[1].max(d.upper_bound[0])],
        scale: cache.reader_at(c).f32()?,
        speeds: cache.reader_at(c + 0x08).f32x2()?,
        effect_tag: effect.name().to_string(),
        vehicle_tag: cddf.name().to_string(),
    }))
}

fn read_projectile(cache: &CacheFile, proj: &TagEntry) -> Result<Projectile> {
    let b = cache.tag_offset(proj)?;
    let damage = match tag_at(cache, b + 0x1F0)? {
        Some(t) if cache.tags.class_of(t).code.as_str() == "jpt!" => Some(read_damage(cache, t)?),
        _ => None,
    };
    Ok(Projectile {
        tag: proj.name().to_string(),
        maximum_range: cache.reader_at(b + 0x22C).f32()?,
        initial_velocity: cache.reader_at(b + 0x23C).f32()?,
        final_velocity: cache.reader_at(b + 0x240).f32()?,
        damage,
    })
}

/// Reads one `weap` tag (its numbers and markers, no meshes).
pub fn read_weapon(cache: &CacheFile, weap: &TagEntry) -> Result<Weapon> {
    let b = cache.tag_offset(weap)?;
    let hlmt = tag_at(cache, b + 0x34)?;
    let mode = scenario::render_model_of(cache, weap)?;
    let first_person = cache
        .reader_at(b + 0x3FC)
        .block()?
        .elements(32)
        .map(|o| -> Result<FirstPerson> {
            let m = tag_at(cache, o)?;
            let a = tag_at(cache, o + 16)?;
            Ok(FirstPerson {
                render_model: m.map(|t| t.index),
                render_model_path: m.map_or_else(String::new, |t| t.name().to_string()),
                animations: a.map_or_else(String::new, |t| t.name().to_string()),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let magazines = cache
        .reader_at(b + 0x424)
        .block()?
        .elements(128)
        .map(|o| -> Result<Magazine> {
            let mut r = cache.reader_at(o + 6);
            let rounds_total_initial = r.i16()?;
            let rounds_total_maximum = r.i16()?;
            let rounds_loaded_maximum = r.i16()?;
            Ok(Magazine {
                rounds_total_initial,
                rounds_total_maximum,
                rounds_loaded_maximum,
                rounds_reloaded: cache.reader_at(o + 0x14).i16()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let triggers = cache
        .reader_at(b + 0x430)
        .block()?
        .elements(144)
        .map(|o| -> Result<Trigger> {
            let mut r = cache.reader_at(o + 6);
            Ok(Trigger {
                behavior: r.i16()?,
                primary_barrel: opt_index(r.i16()?),
                secondary_barrel: opt_index(r.i16()?),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let barrels = cache
        .reader_at(b + 0x43C)
        .block()?
        .elements(308)
        .map(|o| -> Result<Barrel> {
            let projectile = match tag_at(cache, o + 0x98)? {
                Some(t) if cache.tags.class_of(t).code.as_str() == "proj" => Some(read_projectile(cache, t)?),
                _ => None,
            };
            let mut r = cache.reader_at(o + 0x1C);
            let shots_per_fire = [r.i16()?, r.i16()?];
            let fire_recovery_time = r.f32()?;
            let mut r = cache.reader_at(o + 0x28);
            let magazine = opt_index(r.i16()?);
            let rounds_per_shot = r.i16()?;
            Ok(Barrel {
                rounds_per_second: cache.reader_at(o + 4).f32x2()?,
                shots_per_fire,
                fire_recovery_time,
                magazine,
                rounds_per_shot,
                minimum_error: cache.reader_at(o + 0x7C).f32()?,
                error_angle: cache.reader_at(o + 0x80).f32x2()?,
                projectile,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let (markers, root) = match mode {
        Some(m) => (
            markers(cache, m)?,
            nodes(cache, m)?.first().map_or(Mat4::IDENTITY, |n| n.world),
        ),
        None => (Vec::new(), Mat4::IDENTITY),
    };
    let sid = |off: usize| -> Result<String> {
        Ok(string(cache, cache.reader_at(b + off).string_id()?, String::new))
    };
    Ok(Weapon {
        tag_index: weap.index,
        tag_path: weap.name().to_string(),
        class: sid(0x3DC)?,
        short_name: sid(0x3E0)?,
        model: hlmt.map_or_else(String::new, |t| t.name().to_string()),
        render_model: mode.map(|t| t.index),
        first_person,
        zoom_levels: cache.reader_at(b + 0x31E).i16()?,
        zoom_range: cache.reader_at(b + 0x320).f32x2()?,
        holster_marker: sid(0x4D8)?,
        magazines,
        triggers,
        barrels,
        markers,
        root,
    })
}

/// Every `weap` tag in the map, in tag order.
pub fn weapons(cache: &CacheFile) -> Result<Vec<Weapon>> {
    cache.tags.of_class("weap").map(|t| read_weapon(cache, t)).collect()
}

/// Decodes a render model (a weapon's third-person `render_model` or a
/// first-person one) into `scene`, first permutation of every region.
/// `materials` must belong to `scene` (share one table per scene, not
/// across scenes). Vertices come out in weapon model space, Z up, wu.
pub fn decode_model(
    cache: &CacheFile,
    resources: &Resources<'_>,
    materials: &mut MaterialTable,
    mode_index: u32,
    scene: &mut Scene,
) -> Result<ModelParts> {
    let mode = cache
        .tags
        .get(mode_index)
        .ok_or_else(|| crate::Error::Invalid(format!("no tag {mode_index}")))?;
    render_model::decode(cache, resources, materials, mode, scene)
}

/// What a scenario item placement is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemKind {
    Weapon,
    /// Grenades, power-ups and deployable equipment (`eqip`).
    Equipment,
}

/// A weapon or equipment item the scenario puts on the map.
#[derive(Clone, Debug)]
pub struct ItemPlacement {
    pub kind: ItemKind,
    /// Position in the scenario's block.
    pub index: usize,
    /// The `weap` / `eqip` tag index and path; `None` / empty for a bad
    /// palette entry.
    pub tag: Option<u32>,
    pub tag_path: String,
    pub position: Vec3,
    /// Yaw, pitch, roll as stored (radians), and the rotation they make
    /// ([`scenario::rotation`]).
    pub yaw_pitch_roll: [f32; 3],
    pub rotation: Quat,
    pub scale: f32,
    /// Seconds before it comes back after being taken; 0 for items placed
    /// once (name inferred from the values: 180 sniper and laser, 150
    /// missile pod, 90 shotgun, 15 / 30 grenades).
    pub respawn_seconds: i16,
}

impl ItemPlacement {
    /// Object frame to world frame.
    pub fn transform(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(Vec3::splat(self.scale), self.rotation, self.position)
    }
}

/// The scenario's weapon and equipment placements, weapons first.
pub fn item_placements(cache: &CacheFile) -> Result<Vec<ItemPlacement>> {
    let scnr = cache.scenario().ok_or_else(|| {
        crate::Error::Invalid(format!("no scenario tag named {}", cache.header.scenario_path))
    })?;
    let s = cache.tag_offset(scnr)?;
    // (kind, placements, palette, stride, respawn field)
    let blocks = [
        (ItemKind::Weapon, 0x114, 0x120, 168, 0x80),
        (ItemKind::Equipment, 0xFC, 0x108, 140, 0x64),
    ];
    let mut out = Vec::new();
    for (kind, placements, palette, stride, respawn) in blocks {
        let palette: Vec<Option<&TagEntry>> = cache
            .reader_at(s + palette)
            .block()?
            .elements(16)
            .map(|o| tag_at(cache, o))
            .collect::<Result<Vec<_>>>()?;
        for (index, o) in cache.reader_at(s + placements).block()?.elements(stride).enumerate() {
            let tag = usize::try_from(cache.reader_at(o).i16()?)
                .ok()
                .and_then(|p| palette.get(p).copied().flatten());
            let position = Vec3::from(cache.reader_at(o + 0x08).f32x3()?);
            let ypr = cache.reader_at(o + 0x14).f32x3()?;
            let scale = cache.reader_at(o + 0x20).f32()?;
            out.push(ItemPlacement {
                kind,
                index,
                tag: tag.map(|t| t.index),
                tag_path: tag.map_or_else(String::new, |t| t.name().to_string()),
                position,
                yaw_pitch_roll: ypr,
                rotation: scenario::rotation(ypr[0], ypr[1], ypr[2]),
                scale: if scale == 0.0 { 1.0 } else { scale },
                respawn_seconds: cache.reader_at(o + respawn).i16()?,
            });
        }
    }
    Ok(out)
}

/// Only the weapon placements.
pub fn weapon_placements(cache: &CacheFile) -> Result<Vec<ItemPlacement>> {
    Ok(item_placements(cache)?
        .into_iter()
        .filter(|p| p.kind == ItemKind::Weapon)
        .collect())
}
