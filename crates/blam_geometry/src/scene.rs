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
}

/// Where a collision triangle came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CollisionSource {
    /// The structure BSP's own collision, by BSP index.
    Bsp(u32),
    /// An instanced geometry definition placed in that BSP.
    Instance(u32),
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
    pub fn bsp(self) -> u32 {
        match self {
            CollisionSource::Bsp(b) | CollisionSource::Instance(b) => b,
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
                .filter(|t| t.source.bsp() < 32 && mask & (1 << t.source.bsp()) != 0)
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
    /// tag's settings, `atmosphere.rs`); `None`: the map's default.
    pub atmosphere: Option<u32>,
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
