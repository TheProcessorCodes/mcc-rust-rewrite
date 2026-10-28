//! Render models (`mode` tags): the meshes of scenery, crates, machines,
//! weapons, vehicles and bipeds. Follows `docs/format/sbsp-render-geometry.md`
//! sections 3 to 7 for the shared geometry structs; the tag-level offsets
//! are unversioned in Reclaimer's definition and the same on every Halo 3
//! cache.
//!
//! A model has regions (hull, wings, ...), each with permutations (variants
//! and damage states) that name a run of sections. Static objects take the
//! first permutation of every region. Unlike a BSP, one bounds entry covers
//! the whole model.

use glam::{Mat3, Mat4, Quat, Vec3, Vec4};

use blam_cache::{CacheFile, ResourceId, Resources, TagEntry};

use crate::materials::MaterialTable;
use crate::render_geometry::{self, GeometryResource};
use crate::scene::{Bone, Mesh, Part, Scene, Skin};
use crate::{Error, Result};

/// A model's drawable pieces: scene meshes with their transform inside the
/// model's own frame.
#[derive(Clone, Debug, Default)]
pub struct ModelParts {
    pub name: String,
    pub pieces: Vec<Piece>,
    /// The skeleton, in node order; empty for models without nodes.
    pub bones: Vec<Bone>,
}

#[derive(Clone, Debug)]
pub struct Piece {
    pub mesh: u32,
    /// Local to model frame. For a rigid piece on a bone this is the bone's
    /// bind pose; for a skinned piece it is the identity.
    pub transform: Mat4,
    pub mirrored: bool,
    /// The bone a rigid piece rides on, if any. Skinned pieces have none;
    /// their mesh carries weights instead.
    pub bone: Option<u16>,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    name: blam_cache::StringId,
    parent: i16,
    position: Vec3,
    rotation: Quat,
}

/// Reads a 3x4 matrix stored as three rotation rows then a translation row
/// (row-vector convention) into a column-vector matrix with uniform scale.
pub fn matrix_from_rows(rows: [[f32; 3]; 3], translation: [f32; 3], scale: f32) -> (Mat4, bool) {
    let rot = Mat3::from_cols(rows[0].into(), rows[1].into(), rows[2].into());
    let m = Mat4::from_cols(
        (rot.x_axis * scale).extend(0.0),
        (rot.y_axis * scale).extend(0.0),
        (rot.z_axis * scale).extend(0.0),
        Vec4::from((Vec3::from(translation), 1.0)),
    );
    (m, rot.determinant() < 0.0)
}

/// Decodes a render model's first permutation of every region into the
/// scene's mesh list.
pub fn decode(
    cache: &CacheFile,
    resources: &Resources<'_>,
    materials: &mut MaterialTable,
    mode: &TagEntry,
    scene: &mut Scene,
) -> Result<ModelParts> {
    decode_with(cache, resources, materials, mode, scene, &|_, _| 0)
}

/// Picks a permutation for a region: given the region's name and its
/// permutations' names, the index to draw.
pub type PermutationPick<'a> = dyn Fn(&str, &[String]) -> usize + 'a;

/// [`decode`], choosing each region's permutation with `pick` (an index
/// out of range falls back to the first).
pub fn decode_with(
    cache: &CacheFile,
    resources: &Resources<'_>,
    materials: &mut MaterialTable,
    mode: &TagEntry,
    scene: &mut Scene,
    pick: &PermutationPick<'_>,
) -> Result<ModelParts> {
    let base = cache.tag_offset(mode)?;
    let mut r = cache.reader_at(base);
    let name = r.string_id()?;
    let name = cache
        .string(name)
        .map_or_else(|| mode.name().to_string(), str::to_string);
    let regions = cache.reader_at(base + 12).block()?;
    let instanced_section = cache.reader_at(base + 28).i32()?;
    let instances = cache.reader_at(base + 32).block()?;
    let nodes_block = cache.reader_at(base + 48).block()?;
    let shaders = cache.reader_at(base + 72).block()?;
    let sections_block = cache.reader_at(base + 104).block()?;
    let bounds_block = cache.reader_at(base + 116).block()?;
    let resource = ResourceId(cache.reader_at(base + 224).u32()?);

    let slots = materials.slots(cache, scene, shaders)?;
    let sections = render_geometry::read_sections(cache, sections_block)?;
    // Model bounds entries are 56 bytes with the same leading layout as the
    // BSP's 44-byte ones.
    let bounds = bounds_block
        .elements(56)
        .map(|o| -> Result<crate::vertex::Bounds> {
            let mut r = cache.reader_at(o + 4);
            Ok(crate::vertex::Bounds {
                x: r.f32x2()?,
                y: r.f32x2()?,
                z: r.f32x2()?,
                u: r.f32x2()?,
                v: r.f32x2()?,
            })
        })
        .next()
        .transpose()?;
    if resource.is_null() {
        return Err(Error::Invalid(format!("{} has no geometry resource", mode.name())));
    }
    let geometry = GeometryResource::load(resources, resource)?;

    // Node frames, for parts that sit on a bone.
    let nodes: Vec<Node> = nodes_block
        .elements(96)
        .map(|o| {
            let mut r = cache.reader_at(o);
            let name = r.string_id()?;
            let parent = r.i16()?;
            let position = Vec3::from(r.at(o + 12).f32x3()?);
            let q = r.at(o + 24).f32x4()?;
            Ok(Node {
                name,
                parent,
                position,
                rotation: Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let mut node_world: Vec<Mat4> = Vec::with_capacity(nodes.len());
    for n in &nodes {
        let local = Mat4::from_rotation_translation(n.rotation, n.position);
        let parent = usize::try_from(n.parent)
            .ok()
            .and_then(|p| node_world.get(p).copied())
            .unwrap_or(Mat4::IDENTITY);
        node_world.push(parent * local);
    }
    let bones: Vec<Bone> = nodes
        .iter()
        .zip(&node_world)
        .enumerate()
        .map(|(i, (n, world))| Bone {
            name: cache
                .string(n.name)
                .map_or_else(|| format!("node {i}"), str::to_string),
            parent: u16::try_from(n.parent).ok(),
            position: n.position,
            rotation: n.rotation,
            world: *world,
        })
        .collect();
    // Which sections the chosen permutation of each region draws.
    let mut wanted: Vec<usize> = Vec::new();
    let name_of = |offset: usize| -> String {
        cache
            .reader_at(offset)
            .string_id()
            .ok()
            .and_then(|id| cache.string(id))
            .unwrap_or_default()
            .to_string()
    };
    for ro in regions.elements(16) {
        let perms = cache.reader_at(ro + 4).block()?;
        let names: Vec<String> = perms.elements(24).map(name_of).collect();
        let chosen = pick(&name_of(ro), &names);
        let Some(po) = perms.element(u32::try_from(chosen).unwrap_or(0), 24).or_else(|| perms.element(0, 24)) else {
            continue;
        };
        let mut r = cache.reader_at(po + 4);
        let first = r.i16()?;
        let count = r.i16()?;
        if first < 0 || count <= 0 {
            continue;
        }
        wanted.extend((first as usize)..(first as usize + count as usize));
    }
    wanted.sort_unstable();
    wanted.dedup();

    let mut parts = ModelParts {
        name,
        pieces: Vec::new(),
        bones,
    };
    let mut decoded_cache: Vec<Option<render_geometry::DecodedSection>> = vec![None; sections.len()];
    for &s in &wanted {
        let Some(section) = sections.get(s) else {
            continue;
        };
        if s as i32 == instanced_section {
            continue;
        }
        let Some(decoded) = render_geometry::decode_section(section, &geometry, bounds.as_ref())?
        else {
            continue;
        };
        let skin = decoded.vertices.is_skinned().then(|| Skin {
            joints: decoded.vertices.joints.clone(),
            weights: decoded.vertices.weights.clone(),
        });
        let mut mesh = Mesh {
            name: format!("{} section {s}", parts.name),
            positions: decoded.vertices.positions.clone(),
            normals: decoded.vertices.normals.clone(),
            uvs: decoded.vertices.uvs.clone(),
            tangents: decoded.vertices.tangents.clone(),
            parts: Vec::new(),
            skin,
        };
        for sm in &section.submeshes {
            let indices = decoded.submesh_triangles(sm);
            if indices.is_empty() {
                continue;
            }
            let material = materials.for_slot(cache, scene, &slots, sm.shader);
            match mesh.parts.iter_mut().find(|p| p.material == material) {
                Some(p) => p.indices.extend(indices),
                None => mesh.parts.push(Part { material, indices }),
            }
        }
        decoded_cache[s] = Some(decoded);
        if mesh.parts.is_empty() {
            continue;
        }
        let skinned = mesh.skin.is_some();
        scene.meshes.push(mesh);
        parts.pieces.push(Piece {
            mesh: (scene.meshes.len() - 1) as u32,
            // Skinned vertices are already in model space, weighted by bone.
            // Vertices are stored in model space for rigid and skinned
            // sections alike; the bone only matters once the model animates.
            transform: Mat4::IDENTITY,
            mirrored: false,
            bone: (!skinned && section.node_index != u8::MAX).then_some(section.node_index as u16),
        });
    }

    // Instanced geometry: one section holds every instance's triangles as
    // a subset; each instance places its subset with its own transform.
    if let Ok(is) = usize::try_from(instanced_section)
        && let Some(section) = sections.get(is)
        && let Some(decoded) = render_geometry::decode_section(section, &geometry, bounds.as_ref())?
    {
        for (i, io) in instances.elements(60).enumerate() {
            let Some(subset) = section.subsets.get(i) else {
                break;
            };
            let mut r = cache.reader_at(io);
            let inst_name = r.string_id()?;
            let node = r.i32()?;
            let scale = r.f32()?;
            let rows = [r.f32x3()?, r.f32x3()?, r.f32x3()?];
            let translation = r.f32x3()?;
            let (m, mirrored) = matrix_from_rows(rows, translation, scale);
            let start = (subset.index_start as usize).min(decoded.indices.len());
            let end = (start + subset.index_count as usize).min(decoded.indices.len());
            let tris = decoded.layout.triangles(&decoded.indices[start..end]);
            if tris.is_empty() {
                continue;
            }
            // Re-index the subset's vertices into their own mesh.
            let mut remap = std::collections::HashMap::new();
            let mut mesh = Mesh {
                name: format!(
                    "{} instance {}",
                    parts.name,
                    cache.string(inst_name).unwrap_or("?")
                ),
                ..Default::default()
            };
            let indices: Vec<u32> = tris
                .iter()
                .map(|&v| {
                    *remap.entry(v).or_insert_with(|| {
                        let k = v as usize;
                        mesh.positions.push(decoded.vertices.positions[k]);
                        mesh.normals.push(decoded.vertices.normals[k]);
                        mesh.uvs.push(decoded.vertices.uvs[k]);
                        if let Some(t) = decoded.vertices.tangents.get(k) {
                            mesh.tangents.push(*t);
                        }
                        (mesh.positions.len() - 1) as u32
                    })
                })
                .collect();
            let shader = section
                .submeshes
                .get(subset.submesh as usize)
                .map_or(-1, |sm| sm.shader);
            let material = materials.for_slot(cache, scene, &slots, shader);
            mesh.parts.push(Part { material, indices });
            scene.meshes.push(mesh);
            let node_m = usize::try_from(node)
                .ok()
                .and_then(|n| node_world.get(n).copied())
                .unwrap_or(Mat4::IDENTITY);
            parts.pieces.push(Piece {
                mesh: (scene.meshes.len() - 1) as u32,
                transform: node_m * m,
                mirrored,
                bone: u16::try_from(node).ok().filter(|n| (*n as usize) < node_world.len()),
            });
        }
    }
    Ok(parts)
}
