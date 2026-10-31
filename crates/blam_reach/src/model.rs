//! Reach render models (`mode`): the meshes of scenery, crates, machines,
//! the sky and weapons. Follows `docs/format/reach.md` 7. Regions,
//! permutations, nodes and instances are laid out as Halo 3's
//! (`blam_geometry::render_model`); the meshes are Reach's.

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3};

use blam_cache::{CacheFile, ResourceId, Resources, TagEntry};
use blam_geometry::materials::MaterialTable;
use blam_geometry::render_model::{ModelParts, Piece, matrix_from_rows};
use blam_geometry::scene::{Bone, Mesh, Part, Scene, Skin};
use blam_geometry::{Error, Result};

use crate::geometry::{self, GeometryResource};
use crate::layout::mode as off;

/// Decodes the first permutation of every region (the instanced mesh's
/// instances included) into the scene's meshes.
pub fn decode(
    cache: &CacheFile,
    resources: &Resources<'_>,
    materials: &mut MaterialTable,
    mode: &TagEntry,
    scene: &mut Scene,
) -> Result<ModelParts> {
    let base = cache.tag_offset(mode)?;
    let name = cache
        .reader_at(base)
        .string_id()
        .ok()
        .and_then(|id| cache.string(id))
        .map_or_else(|| mode.name().to_string(), str::to_string);
    let regions = cache.reader_at(base + off::REGIONS).block()?;
    let instanced_mesh = cache.reader_at(base + off::INSTANCED_MESH).i32()?;
    let instances = cache.reader_at(base + off::INSTANCES).block()?;
    let nodes_block = cache.reader_at(base + off::NODES).block()?;
    let shaders = cache.reader_at(base + off::SHADERS).block()?;
    let meshes = geometry::read_meshes(cache, cache.reader_at(base + off::MESHES).block()?)?;
    let bounds = geometry::read_bounds(cache, cache.reader_at(base + off::COMPRESSION).block()?)?;
    let resource = ResourceId(cache.reader_at(base + off::RESOURCE).u32()?);
    if resource.is_null() {
        return Err(Error::Invalid(format!("{} has no geometry resource", mode.name())));
    }
    let slots: Vec<u32> = shaders
        .elements(off::SHADER_SIZE)
        .map(|o| Ok(materials.index(cache, scene, cache.reader_at(o).tag_ref()?)))
        .collect::<Result<_>>()?;
    let resource = GeometryResource::load(resources, resource)?;

    // Node frames (96 bytes: name, parent +4, position +0xC, rotation +0x18).
    let mut bones: Vec<Bone> = Vec::new();
    for (i, o) in nodes_block.elements(96).enumerate() {
        let mut r = cache.reader_at(o);
        let name = r.string_id()?;
        let parent = r.i16()?;
        let position = Vec3::from(r.at(o + 12).f32x3()?);
        let q = r.at(o + 24).f32x4()?;
        let rotation = Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize();
        let local = Mat4::from_rotation_translation(rotation, position);
        let parent = u16::try_from(parent).ok().filter(|p| (*p as usize) < bones.len());
        let world = parent.map_or(local, |p| bones[p as usize].world * local);
        bones.push(Bone {
            name: cache.string(name).map_or_else(|| format!("node {i}"), str::to_string),
            parent,
            position,
            rotation,
            world,
        });
    }

    // The meshes each region's first permutation draws.
    let mut wanted: Vec<usize> = Vec::new();
    for ro in regions.elements(16) {
        let perms = cache.reader_at(ro + 4).block()?;
        let Some(po) = perms.element(0, 24) else { continue };
        let mut r = cache.reader_at(po + 4);
        let (first, count) = (r.i16()?, r.i16()?);
        if first < 0 || count <= 0 {
            continue;
        }
        wanted.extend(first as usize..first as usize + count as usize);
    }
    wanted.sort_unstable();
    wanted.dedup();

    let mut parts = ModelParts {
        name,
        pieces: Vec::new(),
        bones,
    };
    let bounds0 = bounds.first();
    let to_mesh = |d: &blam_geometry::render_geometry::DecodedSection, name: String| Mesh {
        name,
        positions: d.vertices.positions.clone(),
        normals: d.vertices.normals.clone(),
        uvs: d.vertices.uvs.clone(),
        tangents: d.vertices.tangents.clone(),
        parts: Vec::new(),
        skin: d.vertices.is_skinned().then(|| Skin {
            joints: d.vertices.joints.clone(),
            weights: d.vertices.weights.clone(),
        }),
    };
    for &k in &wanted {
        if k as i32 == instanced_mesh {
            continue;
        }
        let Some(mesh) = meshes.get(k) else { continue };
        let Some(d) = geometry::decode_mesh(mesh, &resource, bounds0)? else {
            continue;
        };
        let mut out = to_mesh(&d, format!("{} mesh {k}", parts.name));
        for p in &mesh.parts {
            let indices = geometry::part_triangles(&d, p.index_start, p.index_count);
            if indices.is_empty() {
                continue;
            }
            let material = materials.for_slot(cache, scene, &slots, p.shader);
            match out.parts.iter_mut().find(|q| q.material == material) {
                Some(q) => q.indices.extend(indices),
                None => out.parts.push(Part { material, indices }),
            }
        }
        if out.parts.is_empty() {
            continue;
        }
        let skinned = out.skin.is_some();
        scene.meshes.push(out);
        parts.pieces.push(Piece {
            mesh: (scene.meshes.len() - 1) as u32,
            transform: Mat4::IDENTITY,
            mirrored: false,
            bone: (!skinned && mesh.node_index != u8::MAX).then_some(mesh.node_index as u16),
        });
    }

    // Instanced geometry: one mesh whose subpart `i` is instance `i`
    // (60 bytes: name, node +4, scale +8, 3x4 matrix +0xC).
    if let Ok(im) = usize::try_from(instanced_mesh)
        && let Some(mesh) = meshes.get(im)
        && let Some(d) = geometry::decode_mesh(mesh, &resource, bounds0)?
    {
        for (i, io) in instances.elements(60).enumerate() {
            let Some(sub) = mesh.subparts.get(i) else { break };
            let mut r = cache.reader_at(io + 4);
            let node = r.i32()?;
            let scale = r.f32()?;
            let rows = [r.f32x3()?, r.f32x3()?, r.f32x3()?];
            let translation = r.f32x3()?;
            let (m, mirrored) = matrix_from_rows(rows, translation, scale);
            let tris = geometry::part_triangles(&d, sub.index_start, sub.index_count);
            if tris.is_empty() {
                continue;
            }
            let mut remap: HashMap<u32, u32> = HashMap::new();
            let mut out = Mesh {
                name: format!("{} instance {i}", parts.name),
                ..Default::default()
            };
            let indices: Vec<u32> = tris
                .iter()
                .map(|&v| {
                    *remap.entry(v).or_insert_with(|| {
                        let k = v as usize;
                        out.positions.push(d.vertices.positions[k]);
                        out.normals.push(d.vertices.normals[k]);
                        out.uvs.push(d.vertices.uvs[k]);
                        if let Some(t) = d.vertices.tangents.get(k) {
                            out.tangents.push(*t);
                        }
                        (out.positions.len() - 1) as u32
                    })
                })
                .collect();
            let shader = mesh.parts.get(sub.part as usize).map_or(-1, |p| p.shader);
            let material = materials.for_slot(cache, scene, &slots, shader);
            out.parts.push(Part { material, indices });
            scene.meshes.push(out);
            let node_m = usize::try_from(node)
                .ok()
                .and_then(|n| parts.bones.get(n))
                .map_or(Mat4::IDENTITY, |b| b.world);
            parts.pieces.push(Piece {
                mesh: (scene.meshes.len() - 1) as u32,
                transform: node_m * m,
                mirrored,
                bone: u16::try_from(node).ok().filter(|n| (*n as usize) < parts.bones.len()),
            });
        }
    }
    Ok(parts)
}
