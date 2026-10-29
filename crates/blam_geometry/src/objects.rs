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
/// Collision BSPs are in their node's frame; every node is taken at the
/// origin, which is exact for the one-node objects (plates, crates, rocks)
/// and approximate for machines with moving parts.
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
            triangulate(
                &blocks,
                bsp + 4,
                NORMAL,
                None,
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
                effect: cache.reader_at(r + 0x10).tag_ref()?.index,
                effect2: cache.reader_at(r + 0x20).tag_ref()?.index,
                unknown_60: cache.reader_at(r + 0x60).f32()?,
            });
        }
        out.push(DamageSection { name, responses });
    }
    Ok(out)
}

/// The size of an object's collision, for a quick check: the box round
/// its triangles.
pub fn bounds(triangles: &[CollisionTriangle]) -> Option<(Vec3, Vec3)> {
    let mut it = triangles.iter().flat_map(|t| t.points);
    let first = it.next()?;
    Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
}
