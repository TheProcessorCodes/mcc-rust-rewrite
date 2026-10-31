//! Weapons on a Halo 2 multiplayer map (`docs/format/halo2.md` 11): where
//! the scenario's netgame equipment puts them, what a player starts with,
//! and each `weap` tag's numbers and markers, in the same
//! [`blam_geometry::weapons`] types the Halo 3 reader fills, so the viewer's
//! weapons run on either game.
//!
//! Halo 2 multiplayer maps place no weapons through the scenario's weapon
//! block (it is empty on Lockout); the items come from **netgame equipment**
//! (+0x120), each naming an item collection (`itmc`) whose first entry is
//! the weapon or equipment.

use blam_geometry::weapons::{Barrel, FirstPerson, ItemKind, ItemPlacement, Magazine, Marker, Projectile, Trigger, Weapon};
use glam::{Mat4, Quat, Vec3};

use crate::{Error, Map, Reader, Result};

/// One netgame equipment entry (+0x120, 0x90 each).
#[derive(Clone, Debug)]
pub struct NetgameItem {
    /// Position in the scenario's block.
    pub index: usize,
    /// The item collection (`itmc`) it names, and the collection's first
    /// item (a `weap`, an `eqip`, ...); `None` when empty.
    pub collection: Option<u32>,
    pub collection_path: String,
    pub item: Option<u32>,
    pub item_class: Option<[u8; 4]>,
    pub item_path: String,
    /// The u8 at +0x14: 0 for weapons and 8 for grenades on Lockout; the two
    /// entries with no collection have 6 and 7. Meaning inferred.
    pub classification: u8,
    pub position: Vec3,
    /// Yaw, pitch, roll (radians).
    pub yaw_pitch_roll: [f32; 3],
    /// Seconds before it comes back after being taken (+0x0E; 30 for most,
    /// 45 battle rifle, 25 sword, 15 shotgun, 39 sniper on Lockout).
    pub respawn_seconds: i16,
}

impl NetgameItem {
    pub fn is_weapon(&self) -> bool {
        self.item_class == Some(*b"weap")
    }
}

fn scenario(map: &Map) -> Result<Reader<'_>> {
    let scnr = map
        .find(b"scnr", &map.scenario_path)
        .or_else(|| map.first_of(b"scnr"))
        .ok_or_else(|| Error::Invalid("no scenario tag".into()))?;
    map.tag_data(scnr.id).ok_or_else(|| Error::Invalid("scenario has no data".into()))
}

/// A tag reference's datum, when it names a tag in the index.
fn tag_at(map: &Map, r: Reader<'_>, off: usize) -> Result<Option<u32>> {
    let id = r.tag_ref(off)?;
    Ok(map.tag(id).map(|t| t.id))
}

/// An item collection's first item (`itmc` +0x00: entries of 0x10, weight
/// f32 +0x00, item reference +0x04, variant string id +0x0C).
fn collection_item(map: &Map, itmc: u32) -> Result<Option<u32>> {
    let Some(r) = map.tag_data(itmc) else { return Ok(None) };
    match r.block(0, 0x10)?.into_iter().next() {
        Some(e) => tag_at(map, e, 4),
        None => Ok(None),
    }
}

/// The scenario's netgame equipment, in block order.
pub fn netgame_items(map: &Map) -> Result<Vec<NetgameItem>> {
    let s = scenario(map)?;
    let mut out = Vec::new();
    for (index, e) in s.block(0x120, 0x90)?.into_iter().enumerate() {
        let collection = tag_at(map, e, 0x58)?;
        let item = match collection {
            Some(c) => collection_item(map, c)?,
            None => None,
        };
        let name = |t: Option<u32>| t.and_then(|t| map.tag_name(t)).unwrap_or("").to_string();
        out.push(NetgameItem {
            index,
            collection,
            collection_path: name(collection),
            item,
            item_class: item.and_then(|t| map.tag(t)).map(|t| t.class),
            item_path: name(item),
            classification: e.u8(0x14)?,
            position: e.vec3(0x40)?,
            yaw_pitch_roll: e.vec3(0x4C)?.to_array(),
            respawn_seconds: e.i16(0xE)?,
        });
    }
    Ok(out)
}

/// The weapons a player spawns with: the scenario's starting equipment
/// (+0x128, 0x9C each; six item collection references from +0x3C), first
/// entry, each collection's first item that is a `weap`. Lockout: the SMG.
pub fn starting_weapons(map: &Map) -> Result<Vec<u32>> {
    let s = scenario(map)?;
    let mut out = Vec::new();
    if let Some(e) = s.block(0x128, 0x9C)?.into_iter().next() {
        for k in 0..6 {
            let Some(c) = tag_at(map, e, 0x3C + 8 * k)? else { continue };
            if let Some(w) = collection_item(map, c)?.filter(|w| map.tag(*w).is_some_and(|t| t.is(b"weap"))) {
                out.push(w);
            }
        }
    }
    Ok(out)
}

/// The netgame weapons as placements (`ItemPlacement::tag` is the `weap`
/// datum).
pub fn weapon_placements(items: &[NetgameItem]) -> Vec<ItemPlacement> {
    items
        .iter()
        .filter(|i| i.is_weapon())
        .map(|i| {
            let [y, p, r] = i.yaw_pitch_roll;
            ItemPlacement {
                kind: ItemKind::Weapon,
                index: i.index,
                tag: i.item,
                tag_path: i.item_path.clone(),
                position: i.position,
                yaw_pitch_roll: i.yaw_pitch_roll,
                rotation: blam_geometry::scenario::rotation(y, p, r),
                scale: 1.0,
                respawn_seconds: i.respawn_seconds,
            }
        })
        .collect()
}

/// The render model (`mode`) behind an object: object +0x34 names its
/// `hlmt`, whose +0x00 names the render model.
pub fn render_model_of(map: &Map, object: u32) -> Option<u32> {
    let o = map.tag_data(object)?;
    let h = map.tag_data(o.tag_ref(0x34).ok()?)?;
    h.tag_ref(0).ok().filter(|&m| map.tag(m).is_some())
}

/// A render model's nodes (+0x48, 0x60 each: name +0x00, parent i16 +0x04,
/// translation +0x0C, rotation quaternion +0x18) as bind-pose model-space
/// frames, with their names.
fn nodes(mode: Reader<'_>) -> Result<Vec<(String, Mat4)>> {
    let mut out: Vec<(String, Mat4)> = Vec::new();
    for e in mode.block(0x48, 0x60)? {
        let parent = usize::try_from(e.i16(4)?).ok().filter(|&p| p < out.len());
        let q = Quat::from_xyzw(e.f32(0x18)?, e.f32(0x1C)?, e.f32(0x20)?, e.f32(0x24)?);
        let q = if q.length_squared() > 1e-6 { q.normalize() } else { Quat::IDENTITY };
        let local = Mat4::from_rotation_translation(q, e.vec3(0xC)?);
        let world = parent.map_or(Mat4::IDENTITY, |p| out[p].1) * local;
        out.push((mode.map.string(e.u32(0)?).to_string(), world));
    }
    Ok(out)
}

/// A render model's markers (+0x58, groups of 0x0C: name +0x00, markers
/// +0x04 of 0x24: region i8 +0x00, permutation i8 +0x01, node i16 +0x02,
/// translation +0x04, rotation quaternion +0x10, scale +0x20), and the
/// root node's frame.
pub fn markers(map: &Map, mode: u32) -> Result<(Vec<Marker>, Mat4)> {
    let r = map
        .tag_data(mode)
        .ok_or_else(|| Error::Invalid(format!("{}: no data", map.tag_name(mode).unwrap_or("?"))))?;
    let nodes = nodes(r)?;
    let mut out = Vec::new();
    for g in r.block(0x58, 0xC)? {
        let name = r.map.string(g.u32(0)?).to_string();
        for k in g.block(4, 0x24)? {
            let node = k.i16(2)?.max(0) as u16;
            let q = Quat::from_xyzw(k.f32(0x10)?, k.f32(0x14)?, k.f32(0x18)?, k.f32(0x1C)?);
            let rotation = if q.length_squared() > 1e-6 { q.normalize() } else { Quat::IDENTITY };
            let position = k.vec3(4)?;
            let (node_name, node_world) = nodes.get(node as usize).cloned().unwrap_or((String::new(), Mat4::IDENTITY));
            out.push(Marker {
                name: name.clone(),
                node,
                node_name,
                position,
                rotation,
                region: k.u8(0)?,
                permutation: k.u8(1)?,
                model: node_world * Mat4::from_rotation_translation(rotation, position),
            });
        }
    }
    Ok((out, nodes.first().map_or(Mat4::IDENTITY, |n| n.1)))
}

/// A projectile's range and speeds (`proj`: maximum range +0x16C, initial
/// and final velocity +0x17C, +0x180; world units, per second).
fn projectile(map: &Map, proj: u32) -> Result<Option<Projectile>> {
    let Some(p) = map.tag_data(proj) else { return Ok(None) };
    Ok(Some(Projectile {
        tag: map.tag_name(proj).unwrap_or("").to_string(),
        maximum_range: p.f32(0x16C)?,
        initial_velocity: p.f32(0x17C)?,
        final_velocity: p.f32(0x180)?,
        // The damage effect is not read for Halo 2.
        damage: None,
    }))
}

/// Reads one `weap` tag: its numbers (`docs/format/halo2.md` 11.3), its
/// first-person models and its render model's markers. No meshes.
pub fn read_weapon(map: &Map, weap: u32) -> Result<Weapon> {
    let tag = map.tag(weap).ok_or_else(|| Error::Invalid(format!("no tag {weap:#x}")))?;
    let path = tag.name.clone();
    let w = map.tag_data(weap).ok_or_else(|| Error::Invalid(format!("{path}: no data")))?;
    let hlmt = tag_at(map, w, 0x34)?;
    let mode = render_model_of(map, weap);
    let first_person = w
        .block(0x2A8, 0x10)?
        .into_iter()
        .map(|e| -> Result<FirstPerson> {
            let m = tag_at(map, e, 0)?;
            let a = tag_at(map, e, 8)?;
            Ok(FirstPerson {
                render_model: m,
                render_model_path: m.and_then(|t| map.tag_name(t)).unwrap_or("").to_string(),
                animations: a.and_then(|t| map.tag_name(t)).unwrap_or("").to_string(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    // A magazine with no rounds (the plasma rifle has one) is no magazine:
    // the weapon runs on its battery.
    let magazines: Vec<Magazine> = w
        .block(0x2C0, 0x5C)?
        .into_iter()
        .map(|e| -> Result<Magazine> {
            Ok(Magazine {
                rounds_total_initial: e.i16(6)?,
                rounds_total_maximum: e.i16(8)?,
                rounds_loaded_maximum: e.i16(0xA)?,
                rounds_reloaded: e.i16(0x14)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let energy = magazines.iter().all(|m| m.rounds_loaded_maximum <= 0);
    let magazines = if energy { Vec::new() } else { magazines };
    let opt = |v: i16| u16::try_from(v).ok();
    let triggers = w
        .block(0x2C8, 0x40)?
        .into_iter()
        .map(|e| -> Result<Trigger> {
            Ok(Trigger {
                behavior: e.i16(6)?,
                primary_barrel: opt(e.i16(8)?),
                secondary_barrel: opt(e.i16(0xA)?),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let barrels = w
        .block(0x2D0, 0xEC)?
        .into_iter()
        .map(|e| -> Result<Barrel> {
            let recovery = e.f32(0x20)?;
            let mut rounds_per_second = [e.f32(4)?, e.f32(8)?];
            // One round per pull (magnum, shotgun, sniper, plasma pistol)
            // stores 0 rounds per second: the fire recovery time paces it.
            if rounds_per_second.iter().all(|r| *r <= 0.0) {
                let r = if recovery > 0.0 { 1.0 / recovery } else { 5.0 };
                rounds_per_second = [r, r];
            }
            let proj = tag_at(map, e, 0x8C)?.filter(|t| map.tag(*t).is_some_and(|t| t.is(b"proj")));
            Ok(Barrel {
                rounds_per_second,
                shots_per_fire: [e.i16(0x1C)?, e.i16(0x1E)?],
                fire_recovery_time: recovery,
                magazine: if energy { None } else { opt(e.i16(0x28)?) },
                rounds_per_shot: e.i16(0x2A)?,
                minimum_error: e.f32(0x70)?,
                error_angle: [e.f32(0x74)?, e.f32(0x78)?],
                projectile: match proj {
                    Some(p) => projectile(map, p)?,
                    None => None,
                },
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let (markers, root) = match mode {
        Some(m) => markers(map, m)?,
        None => (Vec::new(), Mat4::IDENTITY),
    };
    Ok(Weapon {
        tag_index: weap,
        tag_path: path,
        class: String::new(),
        short_name: String::new(),
        model: hlmt.and_then(|t| map.tag_name(t)).unwrap_or("").to_string(),
        render_model: mode,
        first_person,
        zoom_levels: w.i16(0x1FE)?,
        zoom_range: [w.f32(0x200)?, w.f32(0x204)?],
        holster_marker: String::new(),
        magazines,
        triggers,
        barrels,
        markers,
        root,
    })
}
