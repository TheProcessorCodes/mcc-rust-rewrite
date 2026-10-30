//! What a decoded map looks like to the rest of the program: plain meshes in
//! Halo's own coordinates, grouped by material, plus the placements that put
//! them in the world. No file-format detail survives past this point.
//!
//! Units and axes are Halo's: one world unit is ten feet (3.048 m), +Z is up,
//! the frame is right-handed. Converting to a renderer's or the skate
//! engine's frame is the consumer's job (see `WORLD_UNIT_METRES`).

use glam::{Mat4, Vec3};

pub use crate::lightmap::{Lighting, Lightmap};

/// Metres per Halo world unit: one unit is ten feet.
pub const WORLD_UNIT_METRES: f32 = 3.048;

/// How a material should be drawn, as far as a first renderer cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shading {
    Opaque,
    /// Alpha-tested cut-outs: fences, leaves, grates.
    AlphaTest,
    /// Four ground layers mixed by a blend map.
    Terrain,
    /// Glass, water, decals, glows: drawn after the opaque world, no shadows.
    Translucent,
    /// Sky blockers, portals and other things a player never sees.
    Invisible,
}

#[derive(Clone, Debug)]
pub struct Material {
    /// The shader tag's path, e.g. `levels\multi\riverworld\shaders\rock_cliff`.
    pub tag_path: String,
    /// The shader tag's class, e.g. `rmsh`, `rmtr`, `rmfl`, `rmd `, `rmw `.
    pub shader_class: String,
    pub shading: Shading,
    /// Whether both sides of a triangle are drawn.
    pub two_sided: bool,
    /// The shader tag's index, for looking up its textures
    /// (`blam_texture::shader::resolve`); `None` for a null or unknown shader.
    pub shader_tag: Option<u32>,
}

/// One triangle list that uses one material. Indices are into the owning
/// mesh's vertex arrays.
#[derive(Clone, Debug, Default)]
pub struct Part {
    pub material: u32,
    pub indices: Vec<u32>,
}

/// Per-vertex bone weights of a skinned mesh. Joint indices name bones of
/// the owning model's skeleton.
#[derive(Clone, Debug, Default)]
pub struct Skin {
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
}

/// Geometry in its own local frame, shared by every placement of it.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub name: String,
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<[f32; 2]>,
    /// Halo's own tangents (xyz, handedness in w), one per vertex, or empty.
    pub tangents: Vec<[f32; 4]>,
    pub parts: Vec<Part>,
    /// Bone weights when the mesh is skinned; its positions are then the
    /// bind pose in model space.
    pub skin: Option<Skin>,
}

/// One bone of a model's skeleton.
#[derive(Clone, Debug)]
pub struct Bone {
    pub name: String,
    pub parent: Option<u16>,
    /// Local frame relative to the parent, bind pose.
    pub position: Vec3,
    pub rotation: glam::Quat,
    /// Bind pose in model space.
    pub world: Mat4,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.parts.iter().map(|p| p.indices.len() / 3).sum()
    }

    /// Every triangle of the mesh in its local frame.
    pub fn triangles(&self) -> impl Iterator<Item = [Vec3; 3]> + '_ {
        self.parts.iter().flat_map(move |p| {
            p.indices.as_chunks::<3>().0.iter().map(move |t| {
                [
                    self.positions[t[0] as usize],
                    self.positions[t[1] as usize],
                    self.positions[t[2] as usize],
                ]
            })
        })
    }
}

/// What kind of thing a placement is, for filtering and for naming.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Fixed world geometry: one placement per BSP cluster, identity transform.
    Cluster,
    /// Rocks, trees, girders and the like, placed many times from one mesh.
    Instance,
    Scenery,
    Crate,
    Machine,
    Control,
    Sky,
    /// Vehicles that are the map's own set pieces (tags under
    /// `objects\levels\`: Sandtrap's Elephants and Guardian defenders),
    /// parked where the scenario puts them. The game type's Warthogs and
    /// Mongooses are not placed.
    Vehicle,
}

#[derive(Clone, Debug)]
pub struct Placement {
    pub name: String,
    pub kind: Kind,
    pub mesh: u32,
    /// Local frame to world frame, scale included.
    pub transform: Mat4,
    /// The transform mirrors the mesh, so triangles must be wound the other
    /// way round to keep facing out.
    pub mirrored: bool,
    /// The baked lighting the game uses for it (structure BSP clusters and
    /// instances); [`Lighting::None`] for objects.
    pub lighting: Lighting,
    /// The structure BSPs it belongs to, bit `n` for BSP `n`: its own BSP
    /// for clusters and instances, the origin BSP for objects, the active
    /// BSPs for skies. Zero: shown whatever is loaded.
    pub bsps: u32,
    /// For [`Kind::Sky`]: which entry of the scenario's sky block it is.
    pub sky: Option<u32>,
    /// For [`Kind::Cluster`]: the cluster's index in its BSP.
    pub cluster: Option<u32>,
    /// For [`Kind::Instance`]: the instance fades out as it shrinks on
    /// screen from the first to the second size, in pixels (sbsp instance
    /// `+0x6C`; `docs/format/lod.md`). Zero: never.
    pub fade_pixels: [u16; 2],
}

/// Where a collision triangle came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CollisionSource {
    /// The structure BSP's own collision, by BSP index.
    Bsp(u32),
    /// An instanced geometry definition placed in that BSP.
    Instance(u32),
    /// A placed object's own collision model (`objects::add_collision`):
    /// its index in [`Scene::objects`] and the BSPs it belongs to (zero:
    /// every BSP).
    Object { object: u32, bsps: u32 },
}

/// One triangle the game collides with, in world space, wound
/// counter-clockwise seen from the side you stand on.
#[derive(Clone, Copy, Debug)]
pub struct CollisionTriangle {
    pub points: [Vec3; 3],
    /// Index into [`Collision::materials`], or -1.
    pub material: i16,
    /// Surface flags: see `collision::surface_flags`.
    pub flags: u8,
    pub source: CollisionSource,
}

impl CollisionTriangle {
    pub fn normal(&self) -> Vec3 {
        let [a, b, c] = self.points;
        (b - a).cross(c - a).normalize_or_zero()
    }
}

#[derive(Clone, Debug)]
pub struct CollisionMaterial {
    /// The shader tag's path.
    pub shader: String,
    pub shader_class: String,
}

/// Where a player can start: a respawn point's place and facing.
#[derive(Clone, Debug)]
pub struct Spawn {
    pub name: String,
    /// Feet position, Halo world units.
    pub position: Vec3,
    /// Facing about +Z in radians; zero looks along +X.
    pub yaw: f32,
    /// Whether the marker is drawn in the real game (plain respawn points
    /// are not; the "invisible" variant never is either).
    pub kind: SpawnKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnKind {
    /// `objects\multi\spawning\respawn_point`
    Respawn,
    /// `objects\multi\spawning\respawn_point_invisible`
    RespawnInvisible,
    /// `objects\multi\spawning\initial_spawn_point`
    Initial,
}

impl Spawn {
    /// Unit vector the spawn faces, Halo frame.
    pub fn facing(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), self.yaw.sin(), 0.0)
    }
}

/// The game's own collision for the whole map.
#[derive(Clone, Debug, Default)]
pub struct Collision {
    pub triangles: Vec<CollisionTriangle>,
    pub materials: Vec<CollisionMaterial>,
}

impl CollisionSource {
    /// The structure BSP the triangle belongs to.
    /// For objects, the lowest BSP of their mask, or 32 for objects that
    /// belong to every BSP.
    pub fn bsp(self) -> u32 {
        match self {
            CollisionSource::Bsp(b) | CollisionSource::Instance(b) => b,
            CollisionSource::Object { bsps, .. } => bsps.trailing_zeros(),
        }
    }

    /// Whether the triangle is there when the BSPs in `mask` are loaded.
    pub fn in_mask(self, mask: u32) -> bool {
        match self {
            CollisionSource::Object { bsps, .. } => bsps == 0 || bsps & mask != 0,
            _ => self.bsp() < 32 && mask & (1 << self.bsp()) != 0,
        }
    }

    /// The scene object the triangle belongs to, if any.
    pub fn object(self) -> Option<u32> {
        match self {
            CollisionSource::Object { object, .. } => Some(object),
            _ => None,
        }
    }
}

impl Collision {
    /// Only the triangles of the BSPs in `mask` (bit `n` for BSP `n`).
    pub fn of_bsps(&self, mask: u32) -> Collision {
        Collision {
            triangles: self
                .triangles
                .iter()
                .filter(|t| t.source.in_mask(mask))
                .copied()
                .collect(),
            materials: self.materials.clone(),
        }
    }
}

/// A whole map as decoded: every mesh once, every placement of them, and the
/// materials they share.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub name: String,
    pub materials: Vec<Material>,
    pub meshes: Vec<Mesh>,
    pub placements: Vec<Placement>,
    /// The BSP's stated world bounds, min and max.
    pub bounds: Option<(Vec3, Vec3)>,
    /// The real collision BSP, when it decoded.
    pub collision: Collision,
    /// Respawn points from the scenario, in placement order.
    pub spawns: Vec<Spawn>,
    /// One per structure BSP that has a lightmap; `Lighting::PerPixel`
    /// indexes this.
    pub lightmaps: Vec<Lightmap>,
    /// The structure BSPs that decoded, by scenario index.
    pub bsps: Vec<BspInfo>,
    /// Which BSPs the game loads together (campaign maps have several).
    pub zone_sets: Vec<ZoneSet>,
    /// The scenario's player starting locations (campaign: four co-op
    /// players; multiplayer maps have one, spawns are in `spawns`).
    pub starts: Vec<StartingLocation>,
    /// Named boxes scripts test the player against.
    pub trigger_volumes: Vec<TriggerVolume>,
    /// Trigger volumes that begin or commit a zone-set switch.
    pub zone_switches: Vec<ZoneSwitch>,
    /// Every object the scenario places (scenery, machines, controls,
    /// crates, the map's own vehicles), in placement order; their pieces
    /// are in `placements`.
    pub objects: Vec<SceneObject>,
    /// Volumes that push whoever is in them: gravity lifts, man cannons
    /// (`lift.rs`).
    pub lifts: Vec<crate::LiftVolume>,
}

/// One object the scenario places.
#[derive(Clone, Debug)]
pub struct SceneObject {
    pub kind: Kind,
    /// Its index in the scenario's object names, when it has a name.
    pub name_index: Option<u16>,
    pub name: Option<String>,
    /// Index into its kind's placement block.
    pub placement: u32,
    /// The object tag (`scen`, `mach`, `ctrl`, `bloc`, `vehi`).
    pub tag: u32,
    pub tag_path: String,
    /// Local frame to world frame, scale included.
    pub transform: Mat4,
    /// Its origin BSP as a mask; zero for none (shown whatever is loaded).
    pub bsps: u32,
    /// The editor folder scripts create it with (`object_create_folder`).
    pub folder: Option<u16>,
    /// Placement flags (`+0x04`); bit 0: not created when the map starts.
    pub flags: u32,
    /// Its pieces in [`Scene::placements`].
    pub placements: std::ops::Range<u32>,
    /// Drawn on its own so scripts can move and hide it (the game sets
    /// this; the world builder reads it).
    pub dynamic: bool,
}

impl SceneObject {
    /// Placement flag bit 0: the object waits for a script to create it.
    pub fn starts_absent(&self) -> bool {
        self.flags & 1 != 0
    }
}

/// A structure BSP that decoded.
#[derive(Clone, Debug)]
pub struct BspInfo {
    /// Position in the scenario's structure BSP block; bit `index` of a
    /// zone set's mask.
    pub index: u32,
    /// The `sbsp` tag path.
    pub name: String,
    /// The BSP's stated world bounds, min and max.
    pub bounds: (Vec3, Vec3),
    /// Every cluster of the BSP, by index.
    pub clusters: Vec<ClusterInfo>,
    /// The openings between clusters (`docs/format/sbsp-portals.md`).
    pub portals: Vec<Portal>,
    /// Finds the cluster a point is in, when the collision BSP decoded.
    pub locator: Option<ClusterLocator>,
}

/// A convex opening joining two clusters of one BSP: whatever is seen of
/// one cluster from the other is seen through its portals.
#[derive(Clone, Debug)]
pub struct Portal {
    pub back: u32,
    pub front: u32,
    /// The polygon, world units.
    pub vertices: Vec<Vec3>,
    pub centroid: Vec3,
    /// Distance from the centroid to the farthest vertex.
    pub radius: f32,
}

impl Portal {
    /// The cluster on the other side from `cluster`.
    pub fn other(&self, cluster: u32) -> u32 {
        if self.back == cluster { self.front } else { self.back }
    }
}

/// Point to cluster: the BSP's collision tree down to a leaf, then the
/// structure BSP's leaf table (`docs/format/sbsp-portals.md` 3).
#[derive(Clone, Debug, Default)]
pub struct ClusterLocator {
    /// Collision BSP 3D nodes as stored: plane in bits 0..15, back child
    /// 16..39, front child 40..63.
    pub nodes: Vec<u64>,
    /// Plane normal and distance.
    pub planes: Vec<[f32; 4]>,
    /// The cluster of each collision leaf, -1 for none.
    pub leaf_clusters: Vec<i8>,
}

impl ClusterLocator {
    /// The cluster holding `p`, or `None` when `p` is in solid space
    /// (inside a wall, outside the level) or the tree is malformed.
    pub fn cluster(&self, p: Vec3) -> Option<u32> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut node = 0usize;
        // A malformed tree must not loop forever.
        for _ in 0..self.nodes.len().min(4096) {
            let n = *self.nodes.get(node)?;
            let plane = self.planes.get((n & 0xFFFF) as usize)?;
            let d = plane[0] * p.x + plane[1] * p.y + plane[2] * p.z - plane[3];
            let child = if d >= 0.0 { (n >> 40) & 0xFF_FFFF } else { (n >> 16) & 0xFF_FFFF } as u32;
            // 23-bit index, sign-extended; bit 23 marks a leaf.
            let index = ((child << 9) as i32) >> 9;
            if index < 0 {
                return None;
            }
            if child & 0x80_0000 != 0 {
                return u32::try_from(*self.leaf_clusters.get(index as usize)?).ok();
            }
            node = index as usize;
        }
        None
    }
}

/// What a cluster says about its surroundings.
#[derive(Clone, Copy, Debug)]
pub struct ClusterInfo {
    /// Bounds, min and max; `None` when the cluster stores none.
    pub bounds: Option<(Vec3, Vec3)>,
    /// The scenario sky seen from inside it (index into the scenario's
    /// sky block); `None` indoors.
    pub sky: Option<u32>,
    /// The atmosphere setting of the air in it (index into the `skya`
    /// tag's settings, `atmosphere.rs`, through the BSP's atmosphere
    /// palette); `None`: the map's default.
    pub atmosphere: Option<u32>,
    /// Index into the BSP's acoustics palette (background sound and sound
    /// environment, `docs/format/sound.md` 5); `None`: none.
    pub acoustics: Option<u32>,
}

/// A set of BSPs the game loads together.
#[derive(Clone, Debug)]
pub struct ZoneSet {
    pub name: String,
    /// Bit `n` set: structure BSP `n` is loaded.
    pub bsps: u32,
}

/// A player starting location.
#[derive(Clone, Copy, Debug)]
pub struct StartingLocation {
    /// Feet position, Halo world units.
    pub position: Vec3,
    /// Facing about +Z in radians from +X, and pitch up.
    pub yaw: f32,
    pub pitch: f32,
}

/// An oriented box, Halo world units.
#[derive(Clone, Debug)]
pub struct TriggerVolume {
    pub name: String,
    pub forward: Vec3,
    pub up: Vec3,
    /// One corner of the box; it extends `extents` along forward, left
    /// (up x forward) and up from here.
    pub position: Vec3,
    pub extents: Vec3,
}

impl TriggerVolume {
    /// Whether `p` is inside the box.
    pub fn contains(&self, p: Vec3) -> bool {
        let d = p - self.position;
        let left = self.up.cross(self.forward);
        let local = Vec3::new(d.dot(self.forward), d.dot(left), d.dot(self.up));
        local.cmpge(Vec3::ZERO).all() && local.cmple(self.extents).all()
    }

    /// A volume that kills the player (`kill`, `kill_110_01`,
    /// `ziggurat_kill_player01`...): nobody stands or grinds in there.
    pub fn kills(&self) -> bool {
        let n = self.name.to_ascii_lowercase();
        n.starts_with("kill") || n.contains("_kill")
    }

    /// Whether `p` is inside the box seen from above: its height is not
    /// looked at. For upright boxes (`up` along +Z).
    pub fn covers(&self, p: Vec3) -> bool {
        let d = p - self.position;
        let left = self.up.cross(self.forward);
        let (along, across) = (d.dot(self.forward), d.dot(left));
        (0.0..=self.extents.x).contains(&along) && (0.0..=self.extents.y).contains(&across)
    }

    /// A Guardian's watch volume (`tv_turret_north0` ... on Sandtrap and
    /// Sandbox): the defender on the obelisk over it shoots anyone in it who
    /// has left the safe zone. See [`guarded`].
    pub fn guards(&self) -> bool {
        self.name.to_ascii_lowercase().starts_with("tv_turret")
    }

    /// The scenario's `safe_zone`: on Sandtrap and Sandbox, the play space
    /// the Guardians leave alone.
    pub fn is_safe_zone(&self) -> bool {
        self.name.to_ascii_lowercase().starts_with("safe_zone")
    }
}

/// The Guardian volume whose defender shoots a player at `p`, if any.
/// Sandtrap (`shrine`) has no kill volumes: its edge is kept by six
/// Guardian defenders on the obelisks round the desert, each watching one
/// `tv_turret_*` volume, which reaches from the defender in towards the
/// middle; the play space is the `safe_zone`. A point is guarded when it is
/// inside a watch volume and outside every safe zone, both seen from above:
/// the dunes in the west rise over the watch volumes' tops, and a jump
/// inside the play space can rise over the safe zone's. Maps without
/// `tv_turret` volumes are never guarded.
pub fn guarded(volumes: &[TriggerVolume], p: Vec3) -> Option<&TriggerVolume> {
    let guard = volumes.iter().find(|v| v.guards() && v.covers(p))?;
    (!volumes.iter().any(|v| v.is_safe_zone() && v.covers(p))).then_some(guard)
}

/// Entering `volume` begins loading zone set `begin` (prepare) or commits
/// the switch to zone set `commit`; see `docs/format/zone-sets.md` 4.
#[derive(Clone, Copy, Debug)]
pub struct ZoneSwitch {
    pub flags: u16,
    pub begin: Option<u32>,
    pub volume: u32,
    pub commit: Option<u32>,
}

impl Scene {
    /// The triangles drawn with water shaders (`rmw `), in world space.
    pub fn water_triangles(&self) -> Vec<[Vec3; 3]> {
        self.world_triangles()
            .filter(|(m, _)| self.materials.get(*m as usize).is_some_and(|m| m.shader_class == "rmw "))
            .map(|(_, t)| t)
            .collect()
    }

    /// Every triangle of every placement in world space, with the material
    /// it is drawn with. The order is placements, then parts, then triangles.
    pub fn world_triangles(&self) -> impl Iterator<Item = (u32, [Vec3; 3])> + '_ {
        self.placements.iter().flat_map(move |p| {
            let mesh = &self.meshes[p.mesh as usize];
            let m = p.transform;
            let (b, c) = if p.mirrored { (2, 1) } else { (1, 2) };
            mesh.parts.iter().flat_map(move |part| {
                part.indices.as_chunks::<3>().0.iter().map(move |t| {
                    (
                        part.material,
                        [
                            m.transform_point3(mesh.positions[t[0] as usize]),
                            m.transform_point3(mesh.positions[t[b] as usize]),
                            m.transform_point3(mesh.positions[t[c] as usize]),
                        ],
                    )
                })
            })
        })
    }

    pub fn triangle_count(&self) -> usize {
        self.placements
            .iter()
            .map(|p| self.meshes[p.mesh as usize].triangle_count())
            .sum()
    }

    /// Triangles of every mesh once, whether or not it is placed.
    pub fn triangle_count_of_meshes(&self) -> usize {
        self.meshes.iter().map(Mesh::triangle_count).sum()
    }
}
