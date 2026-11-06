//! Ground from Bevy meshes (feature `mesh`). Add [`ReflexMeshGroundPlugin`] and put [`ReflexCollider`] on
//! entities with a `Mesh3d`: their triangles (in world space, from the `GlobalTransform`) become the
//! [`ReflexGround`], rebuilt whenever a collider is added, removed, moved or its mesh changes. Only triangle-list
//! meshes count; a mesh still loading is picked up when it arrives.
//!
//! Winding: the riding side is the mesh's front face (counter-clockwise), as Bevy renders it. Set
//! [`ReflexCollider::double_sided`] for meshes seen from both sides (thin ramps, bridges).

use bevy::asset::AssetEvent;
use bevy::mesh::{Indices, Mesh, Mesh3d, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

use crate::ground::{ReflexGround, TriangleGround};
use crate::ReflexSet;

/// Rebuilds [`ReflexGround`] from the [`ReflexCollider`] meshes before the bikes step. It owns the ground: do not
/// also set `ReflexGround` yourself.
pub struct ReflexMeshGroundPlugin;

impl Plugin for ReflexMeshGroundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MeshGroundState>().add_systems(Update, rebuild.before(ReflexSet::Step));
    }
}

/// Marks a mesh entity as ground the bikes ride on.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ReflexCollider {
    /// Add every triangle facing both ways.
    pub double_sided: bool,
}

#[derive(Resource, Default)]
struct MeshGroundState {
    /// A collider's mesh was not loaded at the last rebuild.
    waiting: bool,
}

#[allow(clippy::type_complexity)]
fn rebuild(
    mut state: ResMut<MeshGroundState>,
    mut ground: ResMut<ReflexGround>,
    meshes: Res<Assets<Mesh>>,
    mut mesh_events: MessageReader<AssetEvent<Mesh>>,
    colliders: Query<(&Mesh3d, &GlobalTransform, &ReflexCollider)>,
    changed: Query<(), (With<ReflexCollider>, Or<(Changed<GlobalTransform>, Changed<Mesh3d>, Added<ReflexCollider>)>)>,
    mut removed: RemovedComponents<ReflexCollider>,
) {
    let ids: Vec<AssetId<Mesh>> = colliders.iter().map(|(m, _, _)| m.id()).collect();
    let mesh_changed = mesh_events.read().any(|e| match e {
        AssetEvent::Added { id } | AssetEvent::Modified { id } | AssetEvent::LoadedWithDependencies { id } => ids.contains(id),
        _ => false,
    });
    let dirty = state.waiting || mesh_changed || !changed.is_empty() || removed.read().count() > 0;
    if !dirty {
        return;
    }
    let mut g = TriangleGround::new();
    let mut waiting = false;
    for (mesh, xf, collider) in &colliders {
        let Some(mesh) = meshes.get(&mesh.0) else {
            waiting = true;
            continue;
        };
        add_mesh(&mut g, mesh, xf, collider.double_sided);
    }
    state.waiting = waiting;
    *ground = ReflexGround::from_triangles(g);
}

/// Adds a triangle-list mesh's triangles in world space.
pub fn add_mesh(g: &mut TriangleGround, mesh: &Mesh, xf: &GlobalTransform, double_sided: bool) {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return;
    }
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return };
    let world: Vec<Vec3> = pos.iter().map(|p| xf.transform_point(Vec3::from_array(*p))).collect();
    let tris: Vec<[u32; 3]> = match mesh.indices() {
        Some(Indices::U16(i)) => i.as_chunks::<3>().0.iter().map(|c| c.map(u32::from)).collect(),
        Some(Indices::U32(i)) => i.as_chunks::<3>().0.to_vec(),
        None => (0..world.len() as u32 / 3).map(|t| [3 * t, 3 * t + 1, 3 * t + 2]).collect(),
    };
    // a mirroring transform (negative determinant) reverses the winding
    let flip = xf.affine().matrix3.determinant() < 0.0;
    for [a, b, c] in tris {
        let (b, c) = if flip { (c, b) } else { (b, c) };
        g.add_indexed(&world, [[a, b, c]]);
        if double_sided {
            g.add_indexed(&world, [[a, c, b]]);
        }
    }
}
