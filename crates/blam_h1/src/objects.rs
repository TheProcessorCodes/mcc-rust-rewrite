//! Halo CE's placed objects as the game treats them: which structure BSPs
//! each stands in, its collision model (`coll`) placed on its model's
//! nodes, and what a device (`mach`, `ctrl`) says about how it moves.
//! Follows `docs/format/halo1.md` sections 13 to 15; every fact there was
//! checked on The Maw (`d40`) with `probe_h1_objects.py`.

use glam::{Mat4, Quat, Vec3};

use blam_geometry::{CollisionSource, CollisionTriangle, Kind, Scene};

use crate::cache::{Map, Reader};
use crate::Result;

/// Object tag (`obje`, the start of every object tag).
pub mod obje {
    /// Bounding radius (f32) and bounding offset (vec3), object frame.
    pub const BOUNDING_RADIUS: usize = 0x04;
    pub const BOUNDING_OFFSET: usize = 0x08;
    /// The render model (`mod2`), animations (`antr`), collision (`coll`).
    pub const MODEL: usize = 0x28;
    pub const ANIMATIONS: usize = 0x38;
    pub const COLLISION: usize = 0x70;
}

/// Collision model (`coll`).
pub mod coll {
    /// Regions, 0x54 each: name, permutations (block `+0x48`, 0x40 each).
    pub const REGIONS: usize = 0x240;
    /// Nodes, 0x40 each: name (char[32]), region (i16 `+0x20`), parent
    /// (i16 `+0x22`), next sibling (`+0x24`), first child (`+0x26`), and
    /// its collision BSPs (block `+0x34`, 0x60 each, laid out as the
    /// structure BSP's, section 7), one per permutation of its region.
    pub const NODES: usize = 0x28C;
    pub const NODE: usize = 0x40;
    pub const NODE_BSPS: usize = 0x34;
}

/// The render model's nodes (`mod2 +0xB8`, 0x9C each): name, parent (i16
/// `+0x24`), default translation (`+0x28`) and rotation (`+0x34`, i j k w)
/// relative to the parent. The rotation is stored inverted: collision put
/// on the nodes with it conjugated lands on the drawn model (mean distance
/// 0.00 against 0.02 wu on d40's `signal light`; 0.01 against 0.76 on
/// a10's `cryotube`).
const MODEL_NODES: usize = 0xB8;
const MODEL_NODE: usize = 0x9C;

/// One structure BSP's collision tree, for telling which BSPs a point
/// stands in.
pub struct BspTree {
    /// 3D nodes: plane, back child, front child. A child with the top bit
    /// set is a leaf (inside the BSP), -1 is outside.
    nodes: Vec<[i32; 3]>,
    planes: Vec<[f32; 4]>,
}

impl BspTree {
    /// Whether `p` is in the BSP's open space (a leaf, not solid or past
    /// its walls). A BSP with no tree holds everything.
    pub fn contains(&self, p: Vec3) -> bool {
        if self.nodes.is_empty() {
            return true;
        }
        let mut n = 0usize;
        for _ in 0..self.nodes.len() + 1 {
            let Some(&[plane, back, front]) = self.nodes.get(n) else { return false };
            let Some(&[a, b, c, d]) = usize::try_from(plane).ok().and_then(|k| self.planes.get(k)) else { return false };
            let child = if a * p.x + b * p.y + c * p.z - d >= 0.0 { front } else { back };
            match child {
                -1 => return false,
                c if c < 0 => return true,
                c => n = c as usize,
            }
        }
        false
    }
}

/// Every structure BSP's collision tree, by scenario index.
pub fn bsp_trees(map: &Map) -> Result<Vec<BspTree>> {
    let mut out = Vec::new();
    for chunk in map.bsps()? {
        let sbsp = map.sbsp(&chunk);
        let mut tree = BspTree { nodes: Vec::new(), planes: Vec::new() };
        if let Some(c) = sbsp.block(0xB0, 0x60)?.into_iter().next() {
            for n in c.block(0, 12)? {
                tree.nodes.push([n.i32(0)?, n.i32(4)?, n.i32(8)?]);
            }
            for p in c.block(0xC, 16)? {
                tree.planes.push([p.f32(0)?, p.f32(4)?, p.f32(8)?, p.f32(12)?]);
            }
        }
        out.push(tree);
    }
    Ok(out)
}

/// The BSPs (bit `k` for scenario BSP `k`) whose open space holds `p`. For
/// scenery the map stores this itself (placement `+0x20`, u16), and it is
/// exactly this on every one of d40's 293 scenery placements.
pub fn bsp_mask(trees: &[BspTree], p: Vec3) -> u32 {
    trees.iter().enumerate().take(32).filter(|(_, t)| t.contains(p)).fold(0, |m, (k, _)| m | 1 << k)
}

/// The model's nodes in model space at the default pose, with their names.
pub fn node_frames(map: &Map, mode: u32) -> Result<Vec<(String, Mat4)>> {
    let Some(r) = map.tag_data(mode) else { return Ok(Vec::new()) };
    let nodes = r.block(MODEL_NODES, MODEL_NODE)?;
    let mut local = Vec::with_capacity(nodes.len());
    for n in &nodes {
        let t = n.vec3(0x28)?;
        let q = Quat::from_xyzw(n.f32(0x34)?, n.f32(0x38)?, n.f32(0x3C)?, n.f32(0x40)?);
        let q = if q.length_squared() > 1e-6 { q.normalize().conjugate() } else { Quat::IDENTITY };
        local.push((n.str(0, 32), n.i16(0x24)?, Mat4::from_rotation_translation(q, t)));
    }
    let mut world: Vec<Option<Mat4>> = vec![None; local.len()];
    // Parents come before children in Halo's node lists; walk up anyway.
    for k in 0..local.len() {
        let mut chain = vec![k];
        let mut p = local[k].1;
        while let Some(pi) = usize::try_from(p).ok().filter(|&pi| pi < local.len() && world[pi].is_none() && !chain.contains(&pi)) {
            chain.push(pi);
            p = local[pi].1;
        }
        for &c in chain.iter().rev() {
            let parent = usize::try_from(local[c].1).ok().and_then(|pi| world.get(pi).copied().flatten());
            world[c] = Some(parent.unwrap_or(Mat4::IDENTITY) * local[c].2);
        }
    }
    Ok(local.into_iter().zip(world).map(|((name, _, _), w)| (name, w.unwrap_or(Mat4::IDENTITY))).collect())
}

/// The collision model of an object tag in the object's own frame: each
/// node's collision BSP for `permutation` (the first when the region has
/// fewer), put on the render model's node of the same name at the default
/// pose. Empty when the object has none (Halo CE's effects scenery, the
/// Longsword in d40's hangar).
pub fn collision_model(map: &Map, object: u32, permutation: usize) -> Result<Vec<CollisionTriangle>> {
    let Some(o) = map.tag_data(object) else { return Ok(Vec::new()) };
    let coll = o.tag_ref(obje::COLLISION)?;
    if !map.tag(coll).is_some_and(|t| t.is(b"coll")) {
        return Ok(Vec::new());
    }
    let Some(c) = map.tag_data(coll) else { return Ok(Vec::new()) };
    let mode = o.tag_ref(obje::MODEL)?;
    let frames = node_frames(map, mode)?;
    let mut out = Vec::new();
    for (k, node) in c.block(coll::NODES, coll::NODE)?.into_iter().enumerate() {
        let bsps = node.block(coll::NODE_BSPS, 0x60)?;
        let Some(bsp) = bsps.get(permutation).or(bsps.first()) else { continue };
        let name = node.str(0, 32);
        let frame = frames
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(&name))
            .or(frames.get(k))
            .map_or(Mat4::IDENTITY, |(_, m)| *m);
        let start = out.len();
        crate::collision::triangulate(*bsp, CollisionSource::Object { object: 0, bsps: 0 }, 0, &mut out)?;
        for t in &mut out[start..] {
            t.points = t.points.map(|p| frame.transform_point3(p));
            t.material = -1;
        }
    }
    Ok(out)
}

/// A machine's kind (`mach +0x290`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineType {
    Door,
    Platform,
    Gear,
}

/// What a device placement and its tag say about how it moves.
#[derive(Clone, Debug)]
pub struct Device {
    /// `None` for a control.
    pub machine: Option<MachineType>,
    /// Seconds from closed to open (`devi +0x188`, position transition
    /// time); 0 when the tag gives none.
    pub position_seconds: f32,
    /// A door's time held open after nothing is near (`mach +0x294`).
    pub door_open_seconds: f32,
    /// Placement device flags (`+0x2C`): 1 initially open, 2 initially
    /// off, 4 can change only once, 8 position reversed, 16 not usable
    /// from any side.
    pub device_flags: u32,
    /// Placement machine flags (`+0x30`): 1 does not operate
    /// automatically, 2 one-sided, 4 never appears locked, 8 opened by
    /// melee attack.
    pub machine_flags: u32,
    /// The object's bounding sphere (`obje +0x4`, `+0x8`), object frame.
    pub radius: f32,
    pub centre: Vec3,
}

impl Device {
    pub fn initially_open(&self) -> bool {
        self.device_flags & 1 != 0
    }
    pub fn initially_off(&self) -> bool {
        self.device_flags & 2 != 0
    }
    /// A door the game opens by itself when someone comes near: a door
    /// machine without "does not operate automatically".
    pub fn automatic_door(&self) -> bool {
        self.machine == Some(MachineType::Door) && self.machine_flags & 1 == 0
    }
}

/// Placement blocks of devices: machines and controls (`+0x294`, `+0x2AC`,
/// 0x40 each, palettes at `+0x2A0`, `+0x2B8`).
const MACHINES: (usize, usize) = (0x294, 0x2A0);
const CONTROLS: (usize, usize) = (0x2AC, 0x2B8);

/// The device behind a scene object, when it is a machine or a control.
pub fn device(map: &Map, scene_object: &blam_geometry::SceneObject) -> Option<Device> {
    let scnr = map.tag_data(map.scenario)?;
    let tag = map.tags.get(scene_object.tag as usize)?;
    let (at, _) = if tag.is(b"mach") {
        MACHINES
    } else if tag.is(b"ctrl") {
        CONTROLS
    } else {
        return None;
    };
    let e = scnr.block(at, 0x40).ok()?.into_iter().nth(scene_object.placement as usize)?;
    let o = map.tag_data(tag.id)?;
    device_of(tag.is(b"mach"), o, e).ok()
}

fn device_of(machine: bool, o: Reader<'_>, placement: Reader<'_>) -> Result<Device> {
    let kind = if machine {
        Some(match o.u16(0x290)? {
            0 => MachineType::Door,
            1 => MachineType::Platform,
            _ => MachineType::Gear,
        })
    } else {
        None
    };
    let seconds = o.f32(0x188)?;
    Ok(Device {
        machine: kind,
        position_seconds: if seconds.is_finite() && seconds > 0.0 { seconds } else { 0.0 },
        door_open_seconds: if machine { o.f32(0x294)?.max(0.0) } else { 0.0 },
        device_flags: placement.u32(0x2C)?,
        machine_flags: if machine { placement.u32(0x30)? } else { 0 },
        radius: o.f32(obje::BOUNDING_RADIUS)?,
        centre: o.vec3(obje::BOUNDING_OFFSET)?,
    })
}

/// Every scene object's device, by `Scene::objects` index.
pub fn devices(map: &Map, scene: &Scene) -> Vec<Option<Device>> {
    scene
        .objects
        .iter()
        .map(|o| if matches!(o.kind, Kind::Machine | Kind::Control) { device(map, o) } else { None })
        .collect()
}
