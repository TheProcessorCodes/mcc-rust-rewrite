//! The weapons' models: decoded from the map at start-up (CPU side), then
//! turned into bevy meshes and materials once, shared by every copy.
//!
//! Model space is Halo's: world units, X along the barrel, Y left, Z up.
//! Each kind's meshes are kept in "grip space", that model space moved so
//! the weapon's `right_hand` marker is at the origin, which is what the
//! hands hold (`foot/pose.rs`).

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::math::{Affine2, Mat2};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use blam_cache::CacheFile;
use blam_geometry::Scene;
use blam_geometry::weapons::{ItemPlacement, Weapon};

use crate::textures::MaterialTextures;

/// Weapons every Spartan may start with, besides those placed on the map.
const LOADOUT: [&str; 2] = [
    r"objects\weapons\rifle\assault_rifle\assault_rifle",
    r"objects\weapons\pistol\magnum\magnum",
];

/// One weapon kind's mesh piece: grip-space triangles of one material.
pub struct Piece {
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub material: u32,
}

/// One weapon kind: its tag's numbers and its model.
pub struct Kind {
    pub weapon: Weapon,
    pub pieces: Vec<Piece>,
    /// Model space to grip space.
    pub to_grip: Mat4,
    /// Grip space: the muzzle (`primary_trigger`), and the off hand.
    pub muzzle: Mat4,
    pub left_hand: Vec3,
}

impl Kind {
    pub fn name(&self) -> String {
        display_name(&self.weapon.tag_path)
    }
}

/// "objects\weapons\rifle\battle_rifle\battle_rifle" -> "Battle Rifle".
pub fn display_name(path: &str) -> String {
    let last = path.rsplit('\\').next().unwrap_or(path);
    let special = match last {
        "smg" => Some("SMG"),
        "spartan_laser" => Some("Spartan Laser"),
        "rocket_launcher" => Some("Rocket Launcher"),
        "missile_pod" => Some("Missile Pod"),
        _ => None,
    };
    if let Some(s) = special {
        return s.to_string();
    }
    last.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The map's weapons, CPU side, handed from `main` to the plugin.
pub struct Arsenal {
    pub kinds: Vec<Kind>,
    /// Where the scenario puts weapons: kind (index into `kinds`) and the
    /// placement (Halo world units).
    pub placed: Vec<(usize, ItemPlacement)>,
    /// The kinds a Spartan starts with (`LOADOUT`, where the map has them).
    pub loadout: Vec<usize>,
    textures: MaterialTextures,
}

/// Reads the map's weapons: every kind placed on the map, plus the
/// start-up loadout. `None` (logged) when the map has none.
pub fn load(cache: &CacheFile) -> Option<Arsenal> {
    let started = std::time::Instant::now();
    let all = match blam_geometry::weapons::weapons(cache) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("weapons: not read: {e}");
            return None;
        }
    };
    let placements = blam_geometry::weapons::weapon_placements(cache).unwrap_or_else(|e| {
        eprintln!("weapons: placements not read: {e}");
        Vec::new()
    });
    let resources = cache.resources().ok()?;
    let mut materials = blam_geometry::materials::MaterialTable::default();
    let mut scene = Scene::default();
    let mut kinds: Vec<Kind> = Vec::new();
    let mut by_tag: HashMap<u32, usize> = HashMap::new();
    let wanted = |w: &Weapon| {
        LOADOUT.contains(&w.tag_path.as_str()) || placements.iter().any(|p| p.tag == Some(w.tag_index))
    };
    for weapon in all.into_iter().filter(|w| wanted(w)) {
        let Some(mode) = weapon.render_model else {
            continue;
        };
        let first = scene.meshes.len();
        let parts = match blam_geometry::weapons::decode_model(cache, &resources, &mut materials, mode, &mut scene) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("weapons: {} not decoded: {e}", weapon.tag_path);
                continue;
            }
        };
        let grip = weapon.grip().map_or(Vec3::ZERO, |m| m.model.w_axis.truncate());
        let to_grip = Mat4::from_translation(-grip);
        let mut pieces = Vec::new();
        for piece in &parts.pieces {
            let Some(mesh) = scene.meshes.get(piece.mesh as usize) else {
                continue;
            };
            if (piece.mesh as usize) < first {
                continue;
            }
            let m = to_grip * piece.transform;
            let n = mesh.positions.len();
            let positions: Vec<[f32; 3]> = mesh.positions.iter().map(|p| m.transform_point3(*p).to_array()).collect();
            let normals = (mesh.normals.len() == n).then(|| {
                mesh.normals.iter().map(|v| m.transform_vector3(*v).normalize_or_zero().to_array()).collect::<Vec<_>>()
            });
            let uvs = if mesh.uvs.len() == n { mesh.uvs.clone() } else { vec![[0.0; 2]; n] };
            for part in &mesh.parts {
                let mut indices = part.indices.clone();
                if piece.mirrored {
                    for t in indices.chunks_exact_mut(3) {
                        t.swap(1, 2);
                    }
                }
                pieces.push(Piece {
                    positions: positions.clone(),
                    normals: normals.clone(),
                    uvs: uvs.clone(),
                    indices,
                    material: part.material,
                });
            }
        }
        let muzzle = weapon
            .muzzle()
            .map_or(Mat4::from_translation(Vec3::X * 0.3), |m| to_grip * m.model);
        let left_hand = weapon
            .marker(blam_geometry::weapons::LEFT_HAND_MARKER)
            .or_else(|| weapon.marker("left_hand_cyborg"))
            .map_or(Vec3::new(0.12, 0.0, 0.0), |m| to_grip.transform_point3(m.model.w_axis.truncate()));
        by_tag.insert(weapon.tag_index, kinds.len());
        kinds.push(Kind {
            weapon,
            pieces,
            to_grip,
            muzzle,
            left_hand,
        });
    }
    drop(resources);
    if kinds.is_empty() {
        eprintln!("weapons: none on this map");
        return None;
    }
    let textures = crate::textures::load(cache, &scene);
    if let Ok(r) = cache.resources() {
        r.clear_page_cache();
    }
    let placed: Vec<(usize, ItemPlacement)> = placements
        .into_iter()
        .filter_map(|p| Some((*by_tag.get(&p.tag?)?, p)))
        .collect();
    let loadout = LOADOUT
        .iter()
        .filter_map(|path| kinds.iter().position(|k| k.weapon.tag_path == *path))
        .collect();
    eprintln!(
        "weapons: {} kinds ({}), {} placed, in {:.1} s",
        kinds.len(),
        kinds.iter().map(Kind::name).collect::<Vec<_>>().join(", "),
        placed.len(),
        started.elapsed().as_secs_f32()
    );
    Some(Arsenal {
        kinds,
        placed,
        loadout,
        textures,
    })
}

/// One kind's meshes on the GPU: a mesh and the material's unlit colour
/// per piece (each copy makes its own material to light it where it is).
pub struct Look {
    pub pieces: Vec<(Handle<Mesh>, LookMaterial)>,
}

#[derive(Clone)]
pub struct LookMaterial {
    pub texture: Option<Handle<Image>>,
    pub colour: LinearRgba,
    pub uv_transform: Affine2,
    pub alpha_mode: AlphaMode,
}

impl LookMaterial {
    /// A material for one copy, lit by `light` (`rider::object_light`).
    pub fn lit(&self, light: Vec3) -> StandardMaterial {
        let c = self.colour;
        StandardMaterial {
            base_color: LinearRgba::rgb(c.red * light.x, c.green * light.y, c.blue * light.z).into(),
            base_color_texture: self.texture.clone(),
            uv_transform: self.uv_transform,
            alpha_mode: self.alpha_mode,
            unlit: true,
            ..default()
        }
    }
}

/// Makes each kind's meshes and textures (index as `Arsenal::kinds`).
pub fn upload(arsenal: &Arsenal, meshes: &mut Assets<Mesh>, images: &mut Assets<Image>) -> Vec<Look> {
    let mut image_handles: HashMap<u32, Option<Handle<Image>>> = HashMap::new();
    arsenal
        .kinds
        .iter()
        .map(|kind| {
            let pieces = kind
                .pieces
                .iter()
                .filter_map(|p| {
                    let look = arsenal.textures.looks.get(p.material as usize).and_then(Option::as_ref);
                    if look.is_some_and(|l| l.hidden) {
                        return None;
                    }
                    let texture = look.and_then(|l| l.texture).and_then(|b| {
                        image_handles
                            .entry(b)
                            .or_insert_with(|| {
                                arsenal
                                    .textures
                                    .bitmaps
                                    .get(&b)
                                    .and_then(crate::rider::halo_image)
                                    .map(|i| images.add(i))
                            })
                            .clone()
                    });
                    let colour = match (texture.is_some(), look.and_then(|l| l.colour)) {
                        (_, Some(c)) => Color::srgb(c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0)),
                        (true, None) => Color::WHITE,
                        (false, None) => Color::srgb(0.35, 0.36, 0.38),
                    };
                    let uv_transform = look.map_or(Affine2::IDENTITY, |l| Affine2 {
                        matrix2: Mat2::from_diagonal(Vec2::from(l.xform.scale)),
                        translation: Vec2::from(l.xform.offset),
                    });
                    let alpha_mode = if look.is_some_and(|l| l.alpha_test) {
                        AlphaMode::Mask(0.5)
                    } else {
                        AlphaMode::Opaque
                    };
                    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
                    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, p.positions.clone());
                    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, p.uvs.clone());
                    mesh.insert_indices(Indices::U32(p.indices.clone()));
                    match &p.normals {
                        Some(n) => mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, n.clone()),
                        None => mesh.compute_normals(),
                    }
                    Some((
                        meshes.add(mesh),
                        LookMaterial {
                            texture,
                            colour: colour.to_linear(),
                            uv_transform,
                            alpha_mode,
                        },
                    ))
                })
                .collect();
            Look { pieces }
        })
        .collect()
}
