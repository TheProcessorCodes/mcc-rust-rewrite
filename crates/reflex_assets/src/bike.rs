//! The Dakumoto 450F (`NeptuneF`) put together from Reflex's files: the
//! bike (`HR_MXs`, else `MXs`), the THQ tyres (`MX_Tires`), the stock
//! four-stroke pipe (`MX_Exhausts`) and the THQ / Green graphics kit
//! (`HR_MX_GraphicKits`, else `MX_GraphicKits`), painted Cortana
//! (`cortana.rs`) unless the user's kit already is.
//!
//! Everything comes out in `halo3_bike`'s bike frame: metres, +Y up, +Z
//! forward, +X the rider's left (Reflex has +X on the right, left-handed),
//! triangles wound for a right-handed renderer. The origin is Reflex's
//! model origin, which sits on the bike's centre of mass.

use std::collections::HashMap;
use std::path::Path;

use crate::package::Package;
use crate::texture::Rgba;
use crate::{Error, FT, bxml, model, surface, texture};

pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    /// Column-major 4x4 (`Mat4::from_cols_array_2d`), bike frame, metres.
    pub local: [[f32; 4]; 4],
}

pub struct Part {
    pub bone: usize,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// A key into `BikeModel::textures`.
    pub texture: Option<String>,
    /// The texture's alpha cuts holes (spokes).
    pub cutout: bool,
    /// Reflex's material slot, for logs.
    pub slot: String,
}

pub struct BikeModel {
    pub bones: Vec<Bone>,
    pub parts: Vec<Part>,
    pub textures: HashMap<String, Rgba>,
    /// What the kit sheet is: "the user's Cortana kit" or "painted Cortana".
    pub kit: &'static str,
}

impl BikeModel {
    pub fn bone(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }

    /// A bone's rest transform from the bike frame (column-major).
    pub fn rest(&self, bone: usize) -> [[f32; 4]; 4] {
        let b = &self.bones[bone];
        match b.parent {
            Some(p) => mul_cols(self.rest(p), b.local),
            None => b.local,
        }
    }
}

/// The key the kit sheet goes under in `textures`.
pub const KIT: &str = "kit";

type Rows = [[f32; 4]; 4];

/// Row-vector product: `p · a · b`.
fn mul_rows(a: &Rows, b: &Rows) -> Rows {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..4).map(|k| a[r][k] * b[k][c]).sum()))
}

/// Column-major product `a · b` (apply `b`, then `a`).
fn mul_cols(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    std::array::from_fn(|c| std::array::from_fn(|r| (0..4).map(|k| a[k][r] * b[c][k]).sum()))
}

/// The inverse of an affine row-vector matrix.
fn inverse_rows(m: &Rows) -> Rows {
    let a = |r: usize, c: usize| m[r][c];
    let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1)) - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0))
        + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0));
    let d = if det.abs() > 1e-12 { 1.0 / det } else { 0.0 };
    let mut inv = [[0.0; 4]; 4];
    inv[0][0] = (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1)) * d;
    inv[0][1] = (a(0, 2) * a(2, 1) - a(0, 1) * a(2, 2)) * d;
    inv[0][2] = (a(0, 1) * a(1, 2) - a(0, 2) * a(1, 1)) * d;
    inv[1][0] = (a(1, 2) * a(2, 0) - a(1, 0) * a(2, 2)) * d;
    inv[1][1] = (a(0, 0) * a(2, 2) - a(0, 2) * a(2, 0)) * d;
    inv[1][2] = (a(0, 2) * a(1, 0) - a(0, 0) * a(1, 2)) * d;
    inv[2][0] = (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0)) * d;
    inv[2][1] = (a(0, 1) * a(2, 0) - a(0, 0) * a(2, 1)) * d;
    inv[2][2] = (a(0, 0) * a(1, 1) - a(0, 1) * a(1, 0)) * d;
    for c in 0..3 {
        inv[3][c] = -(0..3).map(|k| m[3][k] * inv[k][c]).sum::<f32>();
    }
    inv[3][3] = 1.0;
    inv
}

fn transform_rows(p: [f32; 3], m: &Rows) -> [f32; 3] {
    std::array::from_fn(|c| p[0] * m[0][c] + p[1] * m[1][c] + p[2] * m[2][c] + m[3][c])
}

/// Reflex (feet, +X right) to the bike frame (metres, +X left).
fn point(p: [f32; 3]) -> [f32; 3] {
    [-p[0] * FT, p[1] * FT, p[2] * FT]
}

fn direction(n: [f32; 3]) -> [f32; 3] {
    [-n[0], n[1], n[2]]
}

/// A row-vector bone matrix in feet as a column-major bike-frame matrix.
fn bone_matrix(m: &Rows) -> [[f32; 4]; 4] {
    let s = [-1.0, 1.0, 1.0, 1.0];
    let mut cols: [[f32; 4]; 4] = std::array::from_fn(|c| std::array::from_fn(|r| m[c][r] * s[r] * s[c]));
    for r in 0..3 {
        cols[3][r] *= FT;
    }
    cols
}

/// The diffuse texture a material draws with: the last texture it lists
/// that is not a mask, a normal map or mud.
fn diffuse(package: &Package, subpackage: &str, material: &str) -> Option<String> {
    let data = package.get(subpackage, material, "material").ok()?;
    let x = bxml::Bxml::parse(&data).ok()?;
    let names: Vec<String> = x
        .all_named("Texture")
        .filter_map(|n| x.attr_str(n, "textureName"))
        .map(str::to_string)
        .collect();
    names
        .into_iter()
        .rev()
        .find(|n| {
            let l = n.to_lowercase();
            !["mud_noise", "sfx", "combo", "nrml", "normals", "rust_alpha", "numbers"]
                .iter()
                .any(|k| l.contains(k))
        })
}

struct Loader {
    bones: Vec<Bone>,
    parts: Vec<Part>,
    textures: HashMap<String, Rgba>,
}

impl Loader {
    /// A texture from `subpackage`, decoded once; returns its key and
    /// whether it has holes.
    fn texture(&mut self, package: &Package, subpackage: &str, name: &str) -> Option<(String, bool)> {
        let key = format!("{subpackage}/{name}");
        if !self.textures.contains_key(&key) {
            let t = texture::read(&package.get(subpackage, name, "texture").ok()?).ok()?;
            self.textures.insert(key.clone(), t);
        }
        let holes = self.textures[&key].pixels.iter().any(|p| p[3] < 128);
        Some((key, holes))
    }

    /// The groups of `surface` (LOD 0), hung from `bone` unless the model
    /// says otherwise. With `world` (the bones in model space) skinned groups
    /// are moved from model space into their bone's; `texture_of` picks each slot's texture.
    fn add(
        &mut self,
        package: &Package,
        subpackage: &str,
        name: &str,
        bone: usize,
        world: Option<&[Rows]>,
        mut texture_of: impl FnMut(&mut Self, &str) -> Option<(String, bool)>,
    ) -> Result<(), Error> {
        let groups = surface::read(&package.get(subpackage, name, "surface")?)?;
        let model = model::read(&package.get(subpackage, name, "model")?, groups.len())?;
        for (k, g) in groups.iter().enumerate().take(model.lod0) {
            let slot = model.materials[k].clone();
            if slot.contains("Stand") || g.triangles.is_empty() {
                continue;
            }
            let own = model.group_bones[k];
            let (bone, to_local) = match world {
                Some(world) if own < world.len() => (own, g.skinned.then(|| inverse_rows(&world[own]))),
                _ => (bone, None),
            };
            let positions = g
                .positions
                .iter()
                .map(|&p| point(to_local.as_ref().map_or(p, |m| transform_rows(p, m))))
                .collect();
            let normals = if g.normals.len() == g.positions.len() {
                g.normals
                    .iter()
                    .map(|&n| {
                        let n = match &to_local {
                            Some(m) => std::array::from_fn(|c| n[0] * m[0][c] + n[1] * m[1][c] + n[2] * m[2][c]),
                            None => n,
                        };
                        direction(n)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let (texture, cutout) = match texture_of(self, &slot) {
                Some((t, holes)) => (Some(t), holes),
                None => (None, false),
            };
            self.parts.push(Part {
                bone,
                positions,
                normals,
                uvs: g.uvs.clone(),
                // Mirroring X turns the winding over.
                indices: g.triangles.iter().flat_map(|&[a, b, c]| [a, c, b]).collect(),
                texture,
                cutout,
                slot,
            });
        }
        Ok(())
    }
}

/// Opens `HR_<stem>` when the install has it, else `<stem>`; the bool says HR.
fn open(dir: &Path, stem: &str) -> Result<(Package, bool), Error> {
    match Package::open(dir, &format!("HR_{stem}")) {
        Ok(p) => Ok((p, true)),
        Err(_) => Ok((Package::open(dir, stem)?, false)),
    }
}

/// The Cortana 450F from the Reflex install at `root`.
pub fn load(root: &Path) -> Result<BikeModel, Error> {
    let dir = root.join("Database");
    let mut l = Loader {
        bones: Vec::new(),
        parts: Vec::new(),
        textures: HashMap::new(),
    };

    // The graphics kit: THQ / Green, the sheet the plastics and seat use.
    let (kits, hr) = open(&dir, "MX_GraphicKits")?;
    let kit_sub = if hr { "HR_MX_NeptuneF_THQ_Green" } else { "MX_NeptuneF_THQ_Green" };
    let sheet = kits
        .assets
        .iter()
        .filter(|a| a.subpackage == kit_sub && a.kind == "material" && a.name.contains("Plstc"))
        .find_map(|a| diffuse(&kits, kit_sub, &a.name))
        .ok_or_else(|| Error::Missing(format!("{kit_sub} plastics")))?;
    let kit = texture::read(&kits.get(kit_sub, &sheet, "texture")?)?;

    // The bike and its skeleton.
    let (mxs, hr) = open(&dir, "MXs")?;
    let (sub, name) = if hr { ("HR_NeptuneF", "HR_MX_NeptuneF") } else { ("NeptuneF", "MX_NeptuneF") };
    let groups = surface::read(&mxs.get(sub, name, "surface")?)?.len();
    let skeleton = model::read(&mxs.get(sub, name, "model")?, groups)?;
    let mut world: Vec<Rows> = Vec::with_capacity(skeleton.bones.len());
    for b in &skeleton.bones {
        let w = match b.parent {
            Some(p) if p < world.len() => mul_rows(&b.local, &world[p]),
            _ => b.local,
        };
        world.push(w);
        l.bones.push(Bone {
            name: b.name.clone(),
            parent: b.parent,
            local: bone_matrix(&b.local),
        });
    }
    if l.bones.is_empty() {
        return Err(Error::Format("the 450F has no skeleton".into()));
    }
    l.add(&mxs, sub, name, 0, Some(&world), |l, slot| {
        if slot.contains("Plstc") || slot.contains("Seat") {
            return Some((KIT.to_string(), false));
        }
        let t = diffuse(&mxs, sub, slot)?;
        l.texture(&mxs, sub, &t)
    })?;

    // Tyres and rims on the wheel bones.
    let (tyres, hr) = open(&dir, "MX_Tires")?;
    let tyre_sub = if hr { "HR_MX_Tire_THQ" } else { "MX_Tire_THQ" };
    for (end, bone) in [("FRT", "OBJ_FRT_Tire"), ("BCK", "OBJ_BCK_Tire")] {
        let Some(at) = l.bones.iter().position(|b| b.name == bone) else {
            continue;
        };
        let Some(surface) = tyres
            .assets
            .iter()
            .find(|a| a.subpackage == tyre_sub && a.kind == "surface" && a.name.contains(end))
            .map(|a| a.name.clone())
        else {
            continue;
        };
        l.add(&tyres, tyre_sub, &surface, at, None, |l, slot| {
            let t = diffuse(&tyres, tyre_sub, slot)?;
            l.texture(&tyres, tyre_sub, &t)
        })?;
    }

    // The stock four-stroke pipe, in the bike's model space, aluminium.
    if let Ok(pipes) = Package::open(&dir, "MX_Exhausts") {
        let finish = diffuse(&pipes, "MX_Exh_StockF_Alm", "MX_Exh_Stock4_Alm");
        let _ = l.add(&pipes, "MX_Exh_StockF", "MX_Exh_Stock4", 0, None, |l, _| {
            l.texture(&pipes, "MX_Exh_StockF_Alm", finish.as_deref()?)
        });
    }

    let (kit, painted) = if crate::cortana::is_cortana(&kit) {
        (kit, "the Cortana kit installed in Reflex")
    } else {
        let pieces: Vec<&Part> = l.parts.iter().filter(|p| p.texture.as_deref() == Some(KIT)).collect();
        let world_of = |bone: usize| rest_of(&l.bones, bone);
        (crate::cortana::paint(&kit, &pieces, world_of), "painted Cortana")
    };
    l.textures.insert(KIT.to_string(), kit);

    Ok(BikeModel {
        bones: l.bones,
        parts: l.parts,
        textures: l.textures,
        kit: painted,
    })
}

fn rest_of(bones: &[Bone], bone: usize) -> [[f32; 4]; 4] {
    match bones[bone].parent {
        Some(p) => mul_cols(rest_of(bones, p), bones[bone].local),
        None => bones[bone].local,
    }
}

/// A point through a column-major matrix.
pub fn apply(m: &[[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r])
}
