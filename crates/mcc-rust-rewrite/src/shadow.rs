//! A blob shadow under the player: a soft dark patch on the collision
//! straight below him, on foot, on the board or on the dirt bike, so a
//! landing can be judged. Nothing in the world casts a real shadow (the
//! map's chunks are `NotShadowCaster` and there is no shadow light; Halo
//! 3's shadows are baked into its lightmaps), so this is the whole of it.
//!
//! Every frame a ray goes down from the player's body
//! (`mcc_core::ai::PlayerBody`) onto the shared collision
//! (`mcc_bike::SharedWorld`, the one the Spartan walks on); the patch lies
//! on the face it meets, round on foot, longer along the board and longer
//! still along the bike. It shrinks and fades as he rises, gone by
//! [`FADE_HEIGHT`]. Our own sizes and numbers.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use mcc_core::ai::{PlayerBody, PlayerMode};
use mcc_core::zones::LoadedBsps;

use crate::bike::SharedWorld;
use crate::foot::OnFoot;

/// Height above the ground (m) at which the shadow is gone.
const FADE_HEIGHT: f32 = 7.0;
/// How far down the ray looks past that (m): a shadow far below would only
/// be a dark spot nobody can tie to the player.
const REACH: f32 = FADE_HEIGHT + 1.0;
/// How dark the middle is on the ground.
const DARKNESS: f32 = 0.7;
/// Segments round the disc.
const SEGMENTS: usize = 32;
/// Rings from the middle out to the rim.
const RINGS: usize = 6;

pub struct ShadowPlugin;

impl Plugin for ShadowPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(mcc_core::map::MapStartup, spawn).add_systems(
            PostUpdate,
            place.before(TransformSystems::Propagate).run_if(mcc_core::map::ready),
        );
    }
}

/// The patch and the darkness its colours were last made for.
#[derive(Component)]
struct Blob(Handle<Mesh>, f32);

/// How much of the light gets through at `r` (0 the middle, 1 the rim) of
/// a patch `darkness` dark: all of it at the rim, smoothly less inwards,
/// flat over the middle half.
fn light_through(r: f32, darkness: f32) -> f32 {
    let t = ((r - 0.45) / 0.55).clamp(0.0, 1.0);
    let shade = 1.0 - t * t * (3.0 - 2.0 * t);
    1.0 - darkness * shade
}

/// The vertices' colours for `darkness`: the light let through, which the
/// multiply blend darkens the world by. (Shading through alpha does not
/// work here: blended pixels under half opaque never show on the world.)
fn colours(darkness: f32) -> Vec<[f32; 4]> {
    let mut out = vec![[1.0; 4]; 1 + RINGS * SEGMENTS];
    let v = light_through(0.0, darkness);
    out[0] = [v, v, v, 1.0];
    for ring in 0..RINGS {
        let v = light_through((ring + 1) as f32 / RINGS as f32, darkness);
        for k in 0..SEGMENTS {
            out[1 + ring * SEGMENTS + k] = [v, v, v, 1.0];
        }
    }
    out
}

/// A disc 1 m across in the XY plane: the middle, then [`RINGS`] rings out
/// to the rim.
fn soft_disc(darkness: f32) -> Mesh {
    use bevy::mesh::{Indices, PrimitiveTopology};
    let mut positions = vec![[0.0, 0.0, 0.0]];
    for ring in 0..RINGS {
        let r = 0.5 * (ring + 1) as f32 / RINGS as f32;
        for k in 0..SEGMENTS {
            let t = k as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            positions.push([r * t.cos(), r * t.sin(), 0.0]);
        }
    }
    let n = SEGMENTS as u32;
    let mut indices = Vec::new();
    for k in 0..n {
        indices.extend([0, 1 + k, 1 + (k + 1) % n]);
    }
    for ring in 0..RINGS as u32 - 1 {
        let (inner, outer) = (1 + ring * n, 1 + (ring + 1) * n);
        for k in 0..n {
            let j = (k + 1) % n;
            indices.extend([inner + k, outer + k, outer + j, inner + k, outer + j, inner + j]);
        }
    }
    let count = positions.len();
    let uvs: Vec<[f32; 2]> = positions.iter().map(|p| [p[0] + 0.5, 0.5 - p[1]]).collect();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; count])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours(darkness))
        .with_inserted_indices(Indices::U32(indices))
}

fn spawn(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let material = materials.add(StandardMaterial {
        // The world under it times the vertices' colours.
        base_color: Color::WHITE,
        alpha_mode: AlphaMode::Multiply,
        unlit: true,
        fog_enabled: false,
        double_sided: true,
        cull_mode: None,
        // Kept in front of the face it lies on.
        depth_bias: 50.0,
        ..default()
    });
    let mesh = meshes.add(soft_disc(DARKNESS));
    commands.spawn((
        Name::new("player shadow"),
        Blob(mesh.clone(), DARKNESS),
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::default(),
        Visibility::Hidden,
        bevy::light::NotShadowCaster,
        bevy::light::NotShadowReceiver,
    ));
}

/// The patch's width and length (m) for how he gets about.
fn size(mode: PlayerMode) -> Vec2 {
    match mode {
        PlayerMode::Foot => Vec2::new(1.0, 1.0),
        PlayerMode::Board => Vec2::new(0.9, 1.4),
        PlayerMode::Bike => Vec2::new(1.0, 2.4),
    }
}

/// How large and how dark the shadow is `height` m above the ground: full
/// on the ground, half the size and nothing at [`FADE_HEIGHT`].
fn falloff(height: f32) -> (f32, f32) {
    let t = (height / FADE_HEIGHT).clamp(0.0, 1.0);
    (1.0 - 0.5 * t, (1.0 - t) * (1.0 - t))
}

fn place(
    player: Option<Res<PlayerBody>>,
    foot: Option<Res<OnFoot>>,
    shared: Option<Res<SharedWorld>>,
    loaded: Option<Res<LoadedBsps>>,
    mut blobs: Query<(&mut Blob, &mut Transform, &mut Visibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let mask = loaded.map_or(u32::MAX, |l| if l.mask == 0 { u32::MAX } else { l.mask });
    let world = foot
        .as_deref()
        .and_then(|f| f.shared_world())
        .or_else(|| shared.as_deref().and_then(|s| s.ready(mask)));
    for (mut blob, mut transform, mut visibility) in &mut blobs {
        let body = player.as_deref().and_then(|p| p.0);
        // From a little above the feet, so a foot sunk into a slope still
        // finds the ground under it.
        let hit = body.zip(world.as_deref()).and_then(|(b, w)| {
            let from = b.feet + Vec3::Y * 0.5;
            w.raycast_far(from, Vec3::NEG_Y, REACH + 0.5).map(|h| (b, h))
        });
        let Some((body, hit)) = hit else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let height = (body.feet.y - hit.point.y).max(0.0);
        let (grow, dark) = falloff(height);
        if dark <= 0.0 {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        }
        visibility.set_if_neq(Visibility::Inherited);
        let normal = if hit.normal.y > 0.2 { hit.normal } else { Vec3::Y };
        // The disc lies flat with its length along the heading, then tilts
        // onto the face.
        let rotation = Quat::from_rotation_arc(Vec3::Y, normal)
            * Quat::from_rotation_y(body.heading)
            * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        let s = size(body.mode) * grow;
        *transform = Transform {
            translation: hit.point + normal * 0.03,
            rotation,
            scale: Vec3::new(s.x, s.y, 1.0),
        };
        let darkness = DARKNESS * dark;
        if (blob.1 - darkness).abs() > 0.01
            && let Some(mut mesh) = meshes.get_mut(&blob.0)
        {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours(darkness));
            blob.1 = darkness;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinks_and_fades_with_height() {
        assert_eq!(falloff(0.0), (1.0, 1.0));
        let (g1, d1) = falloff(2.0);
        let (g2, d2) = falloff(4.0);
        assert!(g2 < g1 && g1 < 1.0 && d2 < d1 && d1 < 1.0);
        assert_eq!(falloff(FADE_HEIGHT).1, 0.0);
        assert_eq!(falloff(FADE_HEIGHT * 3.0), (0.5, 0.0));
    }

    #[test]
    fn dark_in_the_middle_and_clear_at_the_rim() {
        let c = colours(DARKNESS);
        assert!((c[0][0] - (1.0 - DARKNESS)).abs() < 1e-6);
        assert_eq!(c[c.len() - 1][0], 1.0);
        assert!(colours(0.0).iter().all(|v| v[0] == 1.0));
        let mesh = soft_disc(DARKNESS);
        assert_eq!(mesh.count_vertices(), c.len());
        assert_eq!(mesh.indices().unwrap().len(), SEGMENTS * 3 + (RINGS - 1) * SEGMENTS * 6);
    }
}
