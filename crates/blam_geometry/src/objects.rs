//! What an object tag says about the object itself: its collision model
//! (`coll`) and its damage sections, the named parts scripts damage
//! (`object_damage_damage_section`). Follows `docs/format/halo-script.md`
//! sections 7 and 8.

use std::collections::HashMap;

use blam_cache::{CacheFile, TagEntry};
use glam::Vec3;

use crate::collision::{NORMAL, TagBlocks, triangulate};
use crate::scene::{CollisionSource, CollisionTriangle, Kind, Scene};
use crate::{Error, Result};

/// `obje +0x34`: the model (`hlmt`).
const OBJECT_MODEL: usize = 0x34;
/// `hlmt +0x00`: the render model (`mode`).
const MODEL_RENDER: usize = 0x00;
/// `hlmt +0x10`: the collision model (`coll`).
const MODEL_COLLISION: usize = 0x10;
/// `hlmt +0x88`: "new damage info", one element; its damage sections block
/// sits at `+0x68`.
const MODEL_DAMAGE_INFO: usize = 0x88;
const DAMAGE_SECTIONS: usize = 0x68;
const DAMAGE_SECTION_SIZE: usize = 44;
const RESPONSE_SIZE: usize = 0x88;
/// `coll +0x20`: regions (16 bytes: name, permutations block).
const COLL_REGIONS: usize = 0x20;
const REGION_SIZE: usize = 16;
/// A permutation: name, then its collision BSPs (100 bytes each: node
/// index, two bytes, the 96-byte collision geometry).
const PERMUTATION_SIZE: usize = 40;
const COLL_BSP_SIZE: usize = 100;

/// The `hlmt` tag of an object tag.
pub fn model_of<'a>(cache: &'a CacheFile, object: &TagEntry) -> Result<Option<&'a TagEntry>> {
    let r = cache.reader_at(cache.tag_offset(object)? + OBJECT_MODEL).tag_ref()?;
    Ok(r.index.and_then(|i| cache.tags.get(i)))
}

/// The collision model of an object tag, in the object's own frame: the
/// first permutation of every region. Empty when it has none.
///
/// Collision BSPs are in their node's frame; each is put on its render
/// model node in the bind pose, so it lines up with the drawn mesh (exact
/// for objects that do not move; a machine's moving parts keep their rest
/// position).
pub fn collision_model(cache: &CacheFile, object: &TagEntry) -> Result<Vec<CollisionTriangle>> {
    let Some(hlmt) = model_of(cache, object)? else {
        return Ok(Vec::new());
    };
    let coll = cache.reader_at(cache.tag_offset(hlmt)? + MODEL_COLLISION).tag_ref()?;
    let Some(coll) = coll.index.and_then(|i| cache.tags.get(i)) else {
        return Ok(Vec::new());
    };
    if cache.tags.class_of(coll).code != "coll" {
        return Err(Error::Invalid(format!("{}: collision model is not a coll tag", object.name())));
    }
    // Each collision BSP sits on a node (`+0x00`); its triangles are in
    // that node's frame, which for machines like the Warthog run's islands
    // is far from the object's origin. Put them on the render model's
    // node in its bind pose, where the drawn mesh is.
    let mode = cache.reader_at(cache.tag_offset(hlmt)? + MODEL_RENDER).tag_ref()?;
    let nodes = match mode.index.and_then(|i| cache.tags.get(i)) {
        Some(mode) => crate::render_model::node_frames(cache, mode).unwrap_or_default(),
        None => Vec::new(),
    };
    let base = cache.tag_offset(coll)?;
    let blocks = TagBlocks(cache);
    let mut out = Vec::new();
    let mut stats = Default::default();
    let regions = cache.reader_at(base + COLL_REGIONS).block()?;
    for region in regions.elements(REGION_SIZE) {
        let perms = cache.reader_at(region + 4).block()?;
        let Some(perm) = perms.element(0, PERMUTATION_SIZE) else {
            continue;
        };
        let bsps = cache.reader_at(perm + 4).block()?;
        for bsp in bsps.elements(COLL_BSP_SIZE) {
            let node = cache.reader_at(bsp).i16()?;
            let frame = usize::try_from(node)
                .ok()
                .and_then(|n| nodes.get(n))
                .filter(|m| **m != glam::Mat4::IDENTITY)
                .map(|m| (*m, m.determinant() < 0.0));
            triangulate(
                &blocks,
                bsp + 4,
                NORMAL,
                frame,
                CollisionSource::Object { object: u32::MAX, bsps: 0 },
                &mut out,
                &mut stats,
            )?;
        }
    }
    Ok(out)
}

/// Adds every placed object's collision model to the scene's collision,
/// placed by the object's transform. Returns the triangles added.
pub fn add_collision(cache: &CacheFile, scene: &mut Scene) -> usize {
    let mut models: HashMap<u32, Vec<CollisionTriangle>> = HashMap::new();
    let mut added = Vec::new();
    for (i, o) in scene.objects.iter().enumerate() {
        if matches!(o.kind, Kind::Sky | Kind::Cluster | Kind::Instance) || crate::scenario::spawn_kind(&o.tag_path).is_some() {
            continue;
        }
        let local = models.entry(o.tag).or_insert_with(|| {
            let Some(tag) = cache.tags.get(o.tag) else {
                return Vec::new();
            };
            collision_model(cache, tag).unwrap_or_else(|e| {
                eprintln!("collision model of {}: {e}", tag.name());
                Vec::new()
            })
        });
        let mirrored = o.transform.determinant() < 0.0;
        for t in local.iter() {
            let p = t.points.map(|v| o.transform.transform_point3(v));
            added.push(CollisionTriangle {
                points: if mirrored { [p[0], p[2], p[1]] } else { p },
                material: -1,
                flags: t.flags,
                source: CollisionSource::Object {
                    object: i as u32,
                    bsps: o.bsps,
                },
            });
        }
    }
    let n = added.len();
    scene.collision.triangles.append(&mut added);
    n
}

/// One instant response of a damage section.
#[derive(Clone, Debug)]
pub struct DamageResponse {
    /// `+0x00`: 2 on the plates' `destroy`, 0 on `fall` (meaning not pinned).
    pub kind: u16,
    /// `+0x02`: what it does to a constraint of the physics model, 0 for
    /// nothing: 1 on the tunnel girders' `destroy` (read as "destroy"), 3
    /// on their `fall` (read as "loosen").
    pub constraint_damage: u16,
    /// `+0x04`: the constraint it acts on, by name (the girders' `hinge`).
    pub constraint: Option<String>,
    /// `+0x10`: the effect it plays (`effe`), as a tag index.
    pub effect: Option<u32>,
    /// `+0x20`: a second effect (the plates' `destroy` names the
    /// explosion here).
    pub effect2: Option<u32>,
    /// `+0x60`: 1.0 on `fall`, 0.0 on `fall_immediate` (meaning not pinned).
    pub unknown_60: f32,
}

/// A named part of an object's model that scripts can damage.
#[derive(Clone, Debug)]
pub struct DamageSection {
    pub name: String,
    pub responses: Vec<DamageResponse>,
}

/// The damage sections of an object tag's model.
pub fn damage_sections(cache: &CacheFile, object: &TagEntry) -> Result<Vec<DamageSection>> {
    let Some(hlmt) = model_of(cache, object)? else {
        return Ok(Vec::new());
    };
    let info = cache.reader_at(cache.tag_offset(hlmt)? + MODEL_DAMAGE_INFO).block()?;
    let Some(info) = info.element(0, 1) else {
        return Ok(Vec::new());
    };
    let sections = cache.reader_at(info + DAMAGE_SECTIONS).block()?;
    let mut out = Vec::new();
    for o in sections.elements(DAMAGE_SECTION_SIZE) {
        let name = cache.string(cache.reader_at(o).string_id()?).unwrap_or("").to_string();
        let block = cache.reader_at(o + 0x0C).block()?;
        let mut responses = Vec::new();
        for r in block.elements(RESPONSE_SIZE) {
            responses.push(DamageResponse {
                kind: cache.reader_at(r).u16()?,
                constraint_damage: cache.reader_at(r + 0x02).u16()?,
                constraint: cache
                    .string(cache.reader_at(r + 0x04).string_id()?)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
                effect: cache.reader_at(r + 0x10).tag_ref()?.index,
                effect2: cache.reader_at(r + 0x20).tag_ref()?.index,
                unknown_60: cache.reader_at(r + 0x60).f32()?,
            });
        }
        out.push(DamageSection { name, responses });
    }
    Ok(out)
}

/// `hlmt +0x30`: the physics model (`phmo`).
const MODEL_PHYSICS: usize = 0x30;
/// `phmo +0x04`: the whole model's mass (kg).
const PHYSICS_MASS: usize = 0x04;
/// `phmo +0x70` and `+0xAC`: two blocks of shapes (the girder: one small
/// `phys_fake` sphere on its anchor, eleven `phys` boxes). Each starts with
/// the same header: name, material, mass scale (`+0x08`), friction
/// (`+0x0C`), restitution (`+0x10`), volume, mass.
const PHYSICS_SHAPES: [usize; 2] = [0xAC, 0x70];
/// `phmo +0x120`: constraints with twist, cone and plane limits (the
/// girder's one, `b_hinge`, ties node 0 to node 1, `hinge`). Element:
/// `+0x00` name, `+0x04` node A, `+0x06` node B, then for A at `+0x08` and
/// B at `+0x3C` a frame (scale, forward, left, up, position, in that
/// node's frame), `+0x78` twist min / max, `+0x80` cone min / max, `+0x88`
/// plane min / max (degrees), `+0x90` max friction torque.
const PHYSICS_CONSTRAINTS: usize = 0x120;
/// `phmo +0x138`: nodes, 12 bytes (name, ...).
const PHYSICS_NODES: usize = 0x138;
const PHYSICS_NODE_SIZE: usize = 12;

/// A constraint between two nodes of an object's physics model, in the
/// object's own frame with its render model in the bind pose.
#[derive(Clone, Debug)]
pub struct PhysicsConstraint {
    pub name: String,
    /// Node B's name (the girder's moving part: `hinge`).
    pub node_name: String,
    /// The pivot: node B's frame position.
    pub pivot: Vec3,
    /// Node A's twist axis (its frame's forward) and plane axis (its up).
    pub twist_axis: Vec3,
    pub plane_axis: Vec3,
    /// Limits, radians: cone (max angle of B's twist axis from A's) and
    /// plane (signed angle of B's twist axis off A's plane).
    pub cone: (f32, f32),
    pub plane: (f32, f32),
    pub twist: (f32, f32),
}

/// What an object's physics model says about how it moves.
#[derive(Clone, Debug)]
pub struct Physics {
    pub mass: f32,
    pub friction: f32,
    pub restitution: f32,
    pub constraints: Vec<PhysicsConstraint>,
}

/// The physics model of an object tag, when it has one. Reads only the
/// first constraint (120_halo's crates have at most one).
pub fn physics(cache: &CacheFile, object: &TagEntry) -> Result<Option<Physics>> {
    let Some(hlmt) = model_of(cache, object)? else {
        return Ok(None);
    };
    let hlmt_at = cache.tag_offset(hlmt)?;
    let Some(phmo) = cache.reader_at(hlmt_at + MODEL_PHYSICS).tag_ref()?.index.and_then(|i| cache.tags.get(i)) else {
        return Ok(None);
    };
    let base = cache.tag_offset(phmo)?;
    let mass = cache.reader_at(base + PHYSICS_MASS).f32()?;
    let (mut friction, mut restitution) = (0.0, 0.0);
    for off in PHYSICS_SHAPES {
        if let Some(e) = cache.reader_at(base + off).block()?.element(0, 1) {
            friction = cache.reader_at(e + 0x0C).f32()?;
            restitution = cache.reader_at(e + 0x10).f32()?;
            break;
        }
    }
    let nodes = cache.reader_at(base + PHYSICS_NODES).block()?;
    let node_name = |i: u16| -> String {
        nodes
            .element(i as u32, PHYSICS_NODE_SIZE)
            .and_then(|o| cache.reader_at(o).string_id().ok())
            .and_then(|s| cache.string(s))
            .unwrap_or("")
            .to_string()
    };
    let mode = cache.reader_at(hlmt_at + MODEL_RENDER).tag_ref()?;
    let frames = match mode.index.and_then(|i| cache.tags.get(i)) {
        Some(mode) => crate::render_model::node_frames(cache, mode).unwrap_or_default(),
        None => Vec::new(),
    };
    let frame = |i: u16| frames.get(i as usize).copied().unwrap_or(glam::Mat4::IDENTITY);
    let mut constraints = Vec::new();
    if let Some(c) = cache.reader_at(base + PHYSICS_CONSTRAINTS).block()?.element(0, 1) {
        let name = cache.string(cache.reader_at(c).string_id()?).unwrap_or("").to_string();
        let (a, b) = (cache.reader_at(c + 0x04).u16()?, cache.reader_at(c + 0x06).u16()?);
        let v = |o: usize| -> Result<Vec3> { Ok(Vec3::from(cache.reader_at(c + o).f32x3()?)) };
        let pair = |o: usize| -> Result<(f32, f32)> {
            let p = cache.reader_at(c + o).f32x2()?;
            Ok((p[0].to_radians(), p[1].to_radians()))
        };
        let (fa, fb) = (frame(a), frame(b));
        constraints.push(PhysicsConstraint {
            name,
            node_name: node_name(b),
            pivot: fb.transform_point3(v(0x3C + 0x28)?),
            twist_axis: fa.transform_vector3(v(0x08 + 0x04)?).normalize_or(Vec3::X),
            plane_axis: fa.transform_vector3(v(0x08 + 0x1C)?).normalize_or(Vec3::Z),
            twist: pair(0x78)?,
            cone: pair(0x80)?,
            plane: pair(0x88)?,
        });
    }
    Ok(Some(Physics {
        mass,
        friction,
        restitution,
        constraints,
    }))
}

/// The size of an object's collision, for a quick check: the box round
/// its triangles.
pub fn bounds(triangles: &[CollisionTriangle]) -> Option<(Vec3, Vec3)> {
    let mut it = triangles.iter().flat_map(|t| t.points);
    let first = it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
}
