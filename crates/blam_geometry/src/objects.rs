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

/// `phmo +0x34`: phantom types, 0x68 each, laid out as Halo 2's
/// (`docs/format/halo2.md` 12; Halo 3's offsets in 12.4).
const PHYSICS_PHANTOM_TYPES: usize = 0x34;
const PHANTOM_TYPE_SIZE: usize = 0x68;
/// Phantom type flags, as Halo 2's: gravity off inside, players left
/// alone, vehicles left alone.
const PHANTOM_NEGATES_GRAVITY: u32 = 1 << 2;
const PHANTOM_IGNORES_PLAYERS: u32 = 1 << 3;
const PHANTOM_IGNORES_VEHICLES: u32 = 1 << 6;
/// The shape blocks lifts use, (offset, element size): spheres (radius
/// +0x40, translation +0x90), pills (radius +0x40, ends +0x50 and +0x60),
/// boxes (half sizes +0x50, rotation rows +0xA0 / +0xB0 / +0xC0,
/// translation +0xD0) and polyhedra (bounding box half sizes +0x50 about
/// the centre +0x60). Every shape starts with its name (+0x00) and has its
/// phantom type as an i8 at +0x1E (-1: an ordinary collision shape).
const PHYSICS_SPHERES: (usize, usize) = (0x70, 0xA0);
const PHYSICS_PILLS: (usize, usize) = (0x88, 0x70);
const PHYSICS_BOXES: (usize, usize) = (0x94, 0xE0);
const PHYSICS_POLYHEDRA: (usize, usize) = (0xAC, 0xA0);

/// Every pushing volume the placed objects carry (Valhalla's man cannons,
/// Construct's lifts, the Forge man cannon): each shape of an object's
/// physics model that uses a phantom type which does not ignore players,
/// aimed by the render model marker the phantom type names (none: the
/// object's origin and up). Objects that wait for a script are left out.
/// `docs/format/halo2.md` 12.4.
pub fn lifts(cache: &CacheFile, scene: &Scene) -> Vec<crate::LiftVolume> {
    let mut out = Vec::new();
    let mut models: HashMap<u32, Vec<LocalLift>> = HashMap::new();
    for o in &scene.objects {
        if matches!(o.kind, Kind::Sky | Kind::Cluster | Kind::Instance) || o.starts_absent() {
            continue;
        }
        let local = models.entry(o.tag).or_insert_with(|| {
            let Some(tag) = cache.tags.get(o.tag) else {
                return Vec::new();
            };
            local_lifts(cache, tag).unwrap_or_else(|e| {
                eprintln!("lifts of {}: {e}", tag.name());
                Vec::new()
            })
        });
        let (scale, rotation, _) = o.transform.to_scale_rotation_translation();
        let short = o.tag_path.rsplit('\\').next().unwrap_or(&o.tag_path);
        let point = |p: Vec3| o.transform.transform_point3(p);
        for l in local.iter() {
            let shape = match l.shape {
                crate::LiftShape::Capsule { a, b, radius } => crate::LiftShape::Capsule {
                    a: point(a),
                    b: point(b),
                    radius: radius * scale.x,
                },
                crate::LiftShape::Box { centre, axes, half } => crate::LiftShape::Box {
                    centre: point(centre),
                    axes: axes.map(|a| (rotation * a).normalize_or(Vec3::Z)),
                    half: half * scale.x,
                },
            };
            out.push(crate::LiftVolume {
                name: format!("{short} {} {}", o.placement, l.name),
                shape,
                origin: point(l.origin),
                direction: (rotation * l.direction).normalize_or(Vec3::Z),
                push: l.push,
            });
        }
    }
    out
}

/// A lift volume in its object's own frame.
#[derive(Clone, Debug)]
struct LocalLift {
    name: String,
    shape: crate::LiftShape,
    origin: Vec3,
    direction: Vec3,
    push: crate::LiftPush,
}

/// The lift volumes of an object tag, in its own frame. Shapes are in the
/// physics model's node space; every lift seen has one node, at the
/// object's origin.
fn local_lifts(cache: &CacheFile, object: &TagEntry) -> Result<Vec<LocalLift>> {
    let Some(hlmt) = model_of(cache, object)? else {
        return Ok(Vec::new());
    };
    let hlmt_at = cache.tag_offset(hlmt)?;
    let Some(phmo) = cache.reader_at(hlmt_at + MODEL_PHYSICS).tag_ref()?.index.and_then(|i| cache.tags.get(i)) else {
        return Ok(Vec::new());
    };
    if cache.tags.class_of(phmo).code != "phmo" {
        return Ok(Vec::new());
    }
    let base = cache.tag_offset(phmo)?;
    let mut types = Vec::new();
    for t in cache.reader_at(base + PHYSICS_PHANTOM_TYPES).block()?.elements(PHANTOM_TYPE_SIZE) {
        let flags = cache.reader_at(t).u32()?;
        let marker = cache.string(cache.reader_at(t + 0x08).string_id()?).unwrap_or("").to_string();
        let f = |o: usize| cache.reader_at(t + o).f32();
        let pair = |o: usize| -> Result<(f32, f32)> { Ok((f(o)?, f(o + 4)?)) };
        let push = crate::LiftPush {
            dead_radius: f(0x1C)?,
            center: pair(0x20)?,
            axis: pair(0x28)?,
            direction: pair(0x30)?,
            negates_gravity: flags & PHANTOM_NEGATES_GRAVITY != 0,
            ignores_vehicles: flags & PHANTOM_IGNORES_VEHICLES != 0,
        };
        // A type that leaves players alone, or pushes nothing, is no lift.
        let pushes = push.direction.0 != 0.0 || push.axis.0 != 0.0 || push.center.0 != 0.0;
        types.push((flags & PHANTOM_IGNORES_PLAYERS == 0 && pushes, marker, push));
    }
    if !types.iter().any(|t| t.0) {
        return Ok(Vec::new());
    }
    let v = |o: usize| -> Result<Vec3> { Ok(Vec3::from(cache.reader_at(o).f32x3()?)) };
    let radius = |s: usize| cache.reader_at(s + 0x40).f32();
    // (phantom type, the shape's offset, the shape)
    let mut shapes: Vec<(usize, usize, crate::LiftShape)> = Vec::new();
    let mut read = |(at, size): (usize, usize), shape: &dyn Fn(usize) -> Result<crate::LiftShape>| -> Result<()> {
        for s in cache.reader_at(base + at).block()?.elements(size) {
            let phantom = cache.reader_at(s + 0x1E).i8()?;
            if let Ok(p) = usize::try_from(phantom)
                && types.get(p).is_some_and(|t| t.0)
            {
                shapes.push((p, s, shape(s)?));
            }
        }
        Ok(())
    };
    read(PHYSICS_SPHERES, &|s| {
        let c = v(s + 0x90)?;
        Ok(crate::LiftShape::Capsule { a: c, b: c, radius: radius(s)? })
    })?;
    read(PHYSICS_PILLS, &|s| {
        Ok(crate::LiftShape::Capsule {
            a: v(s + 0x50)?,
            b: v(s + 0x60)?,
            radius: radius(s)?,
        })
    })?;
    read(PHYSICS_BOXES, &|s| {
        let rows = [v(s + 0xA0)?, v(s + 0xB0)?, v(s + 0xC0)?];
        Ok(crate::LiftShape::Box {
            centre: v(s + 0xD0)?,
            axes: [
                Vec3::new(rows[0].x, rows[1].x, rows[2].x),
                Vec3::new(rows[0].y, rows[1].y, rows[2].y),
                Vec3::new(rows[0].z, rows[1].z, rows[2].z),
            ]
            .map(|a| a.normalize_or(Vec3::Z)),
            half: v(s + 0x50)?,
        })
    })?;
    read(PHYSICS_POLYHEDRA, &|s| {
        Ok(crate::LiftShape::Box {
            centre: v(s + 0x60)?,
            axes: [Vec3::X, Vec3::Y, Vec3::Z],
            half: v(s + 0x50)?,
        })
    })?;
    let markers = match cache.reader_at(hlmt_at + MODEL_RENDER).tag_ref()?.index.and_then(|i| cache.tags.get(i)) {
        Some(mode) => crate::weapons::markers(cache, mode).unwrap_or_default(),
        None => Vec::new(),
    };
    let mut out = Vec::new();
    for (phantom, at, shape) in shapes {
        let (_, marker, push) = &types[phantom];
        let name = cache.string(cache.reader_at(at).string_id()?).unwrap_or("").to_string();
        // The marker's +X is the way it pushes (as on Halo 2).
        let (origin, direction) = match markers.iter().find(|m| !marker.is_empty() && m.name == *marker) {
            Some(m) => (m.model.w_axis.truncate(), m.model.transform_vector3(Vec3::X)),
            None => (Vec3::ZERO, Vec3::Z),
        };
        out.push(LocalLift {
            name,
            shape,
            origin,
            direction: direction.normalize_or(Vec3::Z),
            push: *push,
        });
    }
    Ok(out)
}
