//! Turns a scene's placements into drawable chunks, on any thread.
//!
//! A chunk is one bevy mesh: the triangles of one material and lighting
//! mode that lie in one place, every placement baked into world space.
//! "One place" is what lets the renderer skip work:
//!
//! - structure BSP clusters are split into a grid of [`CELL`] world units,
//!   triangle by triangle, and each piece remembers its cluster, so frustum
//!   culling drops the pieces off screen and portal culling (`culling.rs`)
//!   the clusters that cannot be seen;
//! - instances and objects go to the cell of their centre, split by size
//!   ([`size_class`]) so that small props far away can be dropped on their
//!   own; each chunk lists every cluster its placements touch;
//! - the sky is one chunk per material and is never culled but by the
//!   frustum.
//!
//! Work is done per [`Unit`] (one structure BSP, or everything that belongs
//! to no BSP), so a zone set's BSPs can be built in the background and
//! uploaded or dropped one by one (`stream.rs`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use blam_geometry::lightmap::{Probe, ShLight};
use blam_geometry::{Kind, Lighting, Placement, Scene, Shading, WORLD_UNIT_METRES};

use super::lighting::{self, SurfaceLights};
use super::zones::BspMember;
use super::{is_editor_only, to_render, to_render_dir};
use crate::textures::Look;

/// Grid cell size for splitting the world into chunks, world units
/// (64 wu = 195 m).
pub const CELL: f32 = 64.0;

/// What loads and unloads together: one structure BSP (its clusters,
/// instances and the objects whose origin it is), or everything else (the
/// sky, objects with no origin BSP).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Component)]
pub enum Unit {
    Global,
    Bsp(u32),
}

impl Unit {
    pub fn of(p: &Placement) -> Unit {
        if p.kind == Kind::Sky || p.bsps.count_ones() != 1 {
            Unit::Global
        } else {
            Unit::Bsp(p.bsps.trailing_zeros())
        }
    }

    /// The units a BSP mask needs, the global one first.
    pub fn of_mask(mask: u32) -> impl Iterator<Item = Unit> {
        std::iter::once(Unit::Global).chain((0..32).filter(move |b| mask & (1 << b) != 0).map(Unit::Bsp))
    }
}

/// How a batch is lit: the sky draws unlit; the world by a BSP's lightmap
/// in the shader, or by irradiance baked into its vertex colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lit {
    PerPixel(u32),
    PerVertex,
    Sky,
}

/// Everything the builder reads, shared with the background threads.
pub struct BuildInput {
    pub scene: Scene,
    pub looks: Vec<Option<Look>>,
    /// Which `Scene::lightmaps` entries can be drawn per pixel (their
    /// arrays decoded); the rest fall back to vertex light.
    pub lightmap_ok: Vec<bool>,
    pub surface: Arc<SurfaceLights>,
    /// The open map: each unit decodes the bitmaps and lightmap it needs
    /// (`None`: everything was decoded up front, `WorldSource`).
    pub cache: Option<&'static blam_cache::CacheFile>,
}

impl BuildInput {
    fn look(&self, material: u32) -> Option<&Look> {
        self.looks.get(material as usize).and_then(Option::as_ref)
    }
}

/// What the culling needs to know about a chunk.
#[derive(Component, Clone, Debug)]
pub struct Cull {
    /// The BSP whose clusters `clusters` indexes.
    pub bsp: Option<u32>,
    /// The clusters the chunk lies in (sorted); empty: not portal-culled.
    pub clusters: Vec<u16>,
    /// The radius of its largest placement, render metres: the chunk is
    /// dropped when even that is too small to see. Infinite: never.
    pub detail: f32,
    /// The instances' own fade-out size: dropped when the largest is
    /// smaller on screen than this many pixels across (sbsp instance
    /// `+0x6E`, `docs/format/lod.md`). Zero: only the size rule.
    pub fade_pixels: f32,
}

pub struct BuiltChunk {
    pub lit: Lit,
    pub material: u32,
    pub member: BspMember,
    pub mesh: Mesh,
    pub triangles: u32,
    pub cull: Cull,
}

pub struct BuiltUnit {
    pub unit: Unit,
    pub chunks: Vec<BuiltChunk>,
    pub lit_counts: Vec<(&'static str, usize)>,
    pub editor_only: usize,
    /// Render-space bounds of the non-sky geometry.
    pub extent: Option<(Vec3, Vec3)>,
    pub seconds: f32,
    /// The bitmaps and lightmaps its materials need, decoded with it (CPU
    /// copies, dropped once uploaded).
    pub bitmaps: HashMap<u32, blam_texture::Texture>,
    pub lightmaps: HashMap<u32, crate::textures::LightmapTextures>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct ChunkKey {
    lit: Lit,
    material: u32,
    member: BspMember,
    cluster: Option<u16>,
    cell: [i32; 3],
    class: u8,
    /// The instances' fade-out size in pixels (0: none).
    fade: u16,
}

#[derive(Default)]
struct Batch {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    lightmap_uvs: Vec<[f32; 2]>,
    colours: Vec<[f32; 4]>,
    tangents: Vec<[f32; 4]>,
    indices: Vec<u32>,
    has_normals: bool,
    has_tangents: bool,
    clusters: Vec<u16>,
    detail: f32,
}

/// Where one placement's per-vertex irradiance comes from.
enum VertexLight<'a> {
    PerVertex(&'a [ShLight]),
    Probe(&'a Probe),
    One(ShLight),
}

impl VertexLight<'_> {
    fn irradiance(&self, vertex: usize, n: Vec3) -> Vec3 {
        match self {
            VertexLight::PerVertex(v) => v.get(vertex).map_or(Vec3::ZERO, |l| l.irradiance(n)),
            VertexLight::Probe(p) => p.irradiance(n),
            VertexLight::One(l) => l.irradiance(n),
        }
    }
}

/// Size classes of instances and objects by bounding radius (wu): small
/// props get chunks of their own so they can be dropped at a distance
/// without taking the big rocks with them.
fn size_class(radius: f32) -> u8 {
    match radius {
        r if r < 1.5 => 0,
        r if r < 6.0 => 1,
        _ => 2,
    }
}

fn cell_of(p: Vec3) -> [i32; 3] {
    (p / CELL).floor().as_ivec3().to_array()
}

/// Whether ordinary shaders get their tangents (normal maps).
fn bumped(look: Option<&Look>) -> bool {
    look.is_some_and(|l| {
        l.surface.bump.is_some() || l.terrain.as_ref().is_some_and(|t| t.layers.iter().flatten().any(|x| x.bump.is_some()))
    })
}

/// Builds every chunk of one unit.
pub fn build_unit(input: &BuildInput, unit: Unit) -> BuiltUnit {
    let started = std::time::Instant::now();
    let scene = &input.scene;
    let mut batches: HashMap<ChunkKey, Batch> = HashMap::new();
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    let mut lit_counts: HashMap<&'static str, usize> = HashMap::new();
    let mut editor_only = 0usize;
    for placement in scene.placements.iter().filter(|p| Unit::of(p) == unit) {
        if is_editor_only(&placement.name) {
            editor_only += 1;
            continue;
        }
        let sky = placement.kind == Kind::Sky;
        let member = BspMember {
            bsps: placement.bsps,
            sky: placement.sky.filter(|_| sky),
        };
        let mesh = &scene.meshes[placement.mesh as usize];
        let m = placement.transform;
        let normal_m = Mat3::from_mat4(m).inverse().transpose();
        let has_normals = mesh.normals.len() == mesh.positions.len();
        let has_tangents = has_normals && mesh.tangents.len() == mesh.positions.len();
        let tangent_m = Mat3::from_mat4(m);
        // A mirroring transform flips the binormal's side.
        let handedness = if placement.mirrored { -1.0 } else { 1.0 };

        // World bounds: for lighting by the surface under it and for
        // placing it in a cell and its clusters.
        let mut a = Vec3::splat(f32::INFINITY);
        let mut b = Vec3::splat(f32::NEG_INFINITY);
        for p in &mesh.positions {
            let w = m.transform_point3(*p);
            a = a.min(w);
            b = b.max(w);
        }
        if !a.x.is_finite() {
            continue;
        }
        let centre = (a + b) * 0.5;
        let radius = (b - a).length() * 0.5;

        // Per-pixel when the lightmap made it; otherwise the placement's
        // own per-vertex data or probe; otherwise the lit surface nearest
        // its base.
        let (lit, lightmap_uvs, vertex_light) = match &placement.lighting {
            _ if sky => (Lit::Sky, None, None),
            Lighting::PerPixel { lightmap, uvs } if input.lightmap_ok.get(*lightmap as usize).copied().unwrap_or(false) => {
                (Lit::PerPixel(*lightmap), Some(uvs), None)
            }
            Lighting::PerVertex(v) => (Lit::PerVertex, None, Some(VertexLight::PerVertex(v))),
            Lighting::Probe(p) => (Lit::PerVertex, None, Some(VertexLight::Probe(p))),
            _ => {
                let base = Vec3::new(centre.x, centre.y, a.z + 0.1);
                let light = if input.surface.is_empty() {
                    lighting::default_light()
                } else {
                    input.surface.near_in(base, placement.bsps)
                };
                (Lit::PerVertex, None, Some(VertexLight::One(light)))
            }
        };
        *lit_counts
            .entry(match (&lit, &vertex_light) {
                (Lit::Sky, _) => "sky",
                (Lit::PerPixel(_), _) => "per-pixel",
                (_, Some(VertexLight::PerVertex(_))) => "per-vertex",
                (_, Some(VertexLight::Probe(_))) => "probe",
                _ => "nearest surface",
            })
            .or_default() += 1;

        // Where the placement's triangles go.
        let bsp = match unit {
            Unit::Bsp(b) => scene.bsps.iter().find(|i| i.index == b),
            Unit::Global => None,
        };
        enum Place {
            /// Sky: one chunk per material.
            Sky,
            /// A cluster: each triangle to its cell.
            Cluster(u16),
            /// One cell for the whole placement, these clusters.
            Whole { cell: [i32; 3], cluster: Option<u16>, clusters: Vec<u16>, class: u8 },
        }
        let place = if sky {
            Place::Sky
        } else if let (Kind::Cluster, Some(c)) = (placement.kind, placement.cluster) {
            Place::Cluster(c as u16)
        } else {
            // The cluster the centre is in; then every cluster whose box
            // the placement's box touches. A placement outside the
            // level's clusters (backdrops) is never portal-culled.
            let home = bsp.and_then(|i| i.locator.as_ref()?.cluster(centre)).map(|c| c as u16);
            let clusters = match (home, bsp) {
                (Some(h), Some(i)) => {
                    let mut c: Vec<u16> = i
                        .clusters
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| c.bounds.is_some_and(|(l, u)| l.cmple(b).all() && u.cmpge(a).all()))
                        .map(|(k, _)| k as u16)
                        .collect();
                    if !c.contains(&h) {
                        c.push(h);
                    }
                    c
                }
                _ => Vec::new(),
            };
            Place::Whole {
                cell: cell_of(centre),
                cluster: home,
                clusters,
                class: size_class(radius),
            }
        };

        let mut touched: HashSet<ChunkKey> = HashSet::new();
        for part in &mesh.parts {
            let shading = scene
                .materials
                .get(part.material as usize)
                .map_or(Shading::Opaque, |mat| mat.shading);
            let hidden = input.look(part.material).is_some_and(|l| l.hidden);
            if shading == Shading::Invisible || hidden || part.indices.is_empty() {
                continue;
            }
            let fade = placement.fade_pixels[1];
            let key_at = |cell: [i32; 3], cluster: Option<u16>, class: u8| ChunkKey {
                lit,
                material: part.material,
                member,
                cluster,
                cell,
                class,
                fade,
            };
            // Re-index this part's vertices into each chunk, winding
            // mirrored placements the other way round.
            let mut remap: HashMap<(ChunkKey, u32), u32> = HashMap::new();
            let order: [usize; 3] = if placement.mirrored { [0, 2, 1] } else { [0, 1, 2] };
            for t in part.indices.as_chunks::<3>().0 {
                let key = match &place {
                    Place::Sky => key_at([0; 3], None, 0),
                    Place::Cluster(c) => {
                        let mid = (m.transform_point3(mesh.positions[t[0] as usize])
                            + m.transform_point3(mesh.positions[t[1] as usize])
                            + m.transform_point3(mesh.positions[t[2] as usize]))
                            / 3.0;
                        key_at(cell_of(mid), Some(*c), 3)
                    }
                    Place::Whole { cell, cluster, class, .. } => key_at(*cell, *cluster, *class),
                };
                let batch = batches.entry(key).or_insert_with(|| Batch {
                    has_normals,
                    has_tangents,
                    detail: 0.0,
                    ..Default::default()
                });
                batch.has_normals &= has_normals;
                batch.has_tangents &= has_tangents;
                // Once per placement and chunk: what culls the chunk.
                if touched.insert(key) {
                    match &place {
                        Place::Sky => batch.detail = f32::INFINITY,
                        Place::Cluster(c) => {
                            batch.detail = f32::INFINITY;
                            batch.clusters.push(*c);
                        }
                        // An empty list means "always": once a placement
                        // outside the clusters joins, the chunk stays so
                        // (marked by `u16::MAX` until the end).
                        Place::Whole { clusters, .. } => {
                            batch.detail = batch.detail.max(radius * WORLD_UNIT_METRES);
                            if clusters.is_empty() {
                                batch.clusters.clear();
                                batch.clusters.push(u16::MAX);
                            } else if batch.clusters.first() != Some(&u16::MAX) {
                                batch.clusters.extend_from_slice(clusters);
                            }
                        }
                    }
                }
                for &k in &order {
                    let i = t[k];
                    let j = *remap.entry((key, i)).or_insert_with(|| {
                        let p = to_render(m.transform_point3(mesh.positions[i as usize]));
                        if !sky {
                            lo = lo.min(p);
                            hi = hi.max(p);
                        }
                        batch.positions.push(p.to_array());
                        let halo_n = mesh
                            .normals
                            .get(i as usize)
                            .map(|n| (normal_m * *n).normalize_or(Vec3::Z))
                            .unwrap_or(Vec3::Z);
                        batch.normals.push(to_render_dir(halo_n).to_array());
                        if let Some(t) = mesh.tangents.get(i as usize) {
                            let ht = (tangent_m * Vec3::new(t[0], t[1], t[2])).normalize_or(Vec3::X);
                            let rt = to_render_dir(ht);
                            batch.tangents.push([rt.x, rt.y, rt.z, t[3] * handedness]);
                        }
                        if let Some(uvs) = lightmap_uvs {
                            batch.lightmap_uvs.push(uvs.get(i as usize).copied().unwrap_or([0.0, 0.0]));
                        }
                        if let Some(l) = &vertex_light {
                            let e = l.irradiance(i as usize, halo_n);
                            batch.colours.push([e.x, e.y, e.z, 1.0]);
                        }
                        batch.uvs.push(mesh.uvs.get(i as usize).copied().unwrap_or([0.0, 0.0]));
                        (batch.positions.len() - 1) as u32
                    });
                    batch.indices.push(j);
                }
            }
        }
    }

    let mut keys: Vec<ChunkKey> = batches.keys().copied().collect();
    keys.sort_unstable();
    let mut chunks = Vec::with_capacity(keys.len());
    for key in keys {
        let mut batch = batches.remove(&key).unwrap();
        let sky = key.lit == Lit::Sky;
        let triangles = (batch.indices.len() / 3) as u32;
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        let vertices = batch.positions.len();
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, batch.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, batch.uvs);
        if !batch.lightmap_uvs.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, batch.lightmap_uvs);
        }
        if !batch.colours.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, batch.colours);
        }
        // 16-bit indices where they fit: half the index memory.
        if vertices <= u16::MAX as usize {
            mesh.insert_indices(Indices::U16(batch.indices.iter().map(|&i| i as u16).collect()));
        } else {
            mesh.insert_indices(Indices::U32(batch.indices));
        }
        // Tangents only where a normal map will use them.
        if batch.has_tangents && bumped(input.look(key.material)) && !sky && batch.tangents.len() == vertices {
            mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, batch.tangents);
        }
        if batch.has_normals {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, batch.normals);
        } else {
            mesh.compute_normals();
        }
        batch.clusters.sort_unstable();
        batch.clusters.dedup();
        let clusters = if batch.clusters.contains(&u16::MAX) { Vec::new() } else { batch.clusters };
        let bsp = match unit {
            Unit::Bsp(b) if !clusters.is_empty() => Some(b),
            _ => None,
        };
        chunks.push(BuiltChunk {
            lit: key.lit,
            material: key.material,
            member: key.member,
            mesh,
            triangles,
            cull: Cull {
                bsp,
                clusters,
                detail: batch.detail,
                fade_pixels: key.fade as f32,
            },
        });
    }
    let mut lit_counts: Vec<_> = lit_counts.into_iter().collect();
    lit_counts.sort_unstable();
    // Decode what the chunks' materials sample.
    let (bitmaps, lightmaps) = match input.cache {
        Some(cache) => {
            let mut wanted: Vec<u32> = Vec::new();
            let mut maps: Vec<u32> = Vec::new();
            let mut seen: HashSet<u32> = HashSet::new();
            for c in &chunks {
                if let Lit::PerPixel(l) = c.lit
                    && !maps.contains(&l)
                {
                    maps.push(l);
                }
                if seen.insert(c.material)
                    && let Some(look) = input.look(c.material)
                {
                    wanted.extend(crate::textures::bitmaps_of(look));
                }
            }
            wanted.sort_unstable();
            wanted.dedup();
            crate::textures::decode_bitmaps(cache, scene, &wanted, &maps)
        }
        None => Default::default(),
    };
    BuiltUnit {
        bitmaps,
        lightmaps,
        unit,
        chunks,
        lit_counts,
        editor_only,
        extent: lo.x.is_finite().then_some((lo, hi)),
        seconds: started.elapsed().as_secs_f32(),
    }
}
