//! `.model`: the skeleton and the material slot of each `.surface` group.
//!
//! Header u32s: 0x00 `00 BB AA 88`, 0x08 skinned, 0x0C bone count, 0x14
//! LOD count, 0x1C group count. With bones: their local matrices from 0xAC
//! (16 f32 each, row vectors, row 3 the translation), then 44-byte records
//! `{name offset, parent, 9 f32}`, then the bone names. The slot names are
//! the last NUL-separated strings before the final `5 x groups` bytes. The
//! per-group `{slot name offset, group, bone, 0, 0}` records (20 bytes) sit
//! somewhere after the names, found by their group numbers counting up.

use crate::Error;

pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    /// Row-vector matrix (`p' = p · m`), Reflex model units (feet).
    pub local: [[f32; 4]; 4],
}

pub struct Model {
    pub bones: Vec<Bone>,
    /// The material slot of each group.
    pub materials: Vec<String>,
    /// The bone each group hangs from (all 0 when it has no bones).
    pub group_bones: Vec<usize>,
    /// How many groups the most detailed LOD uses (the first ones).
    pub lod0: usize,
}

fn u32_at(b: &[u8], p: usize) -> Result<usize, Error> {
    b.get(p..p + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()) as usize)
        .ok_or_else(|| Error::Format(format!("model ends at {p}")))
}

fn f32_at(b: &[u8], p: usize) -> Result<f32, Error> {
    Ok(f32::from_bits(u32_at(b, p)? as u32))
}

pub fn read(d: &[u8], groups: usize) -> Result<Model, Error> {
    if u32_at(d, 0)? != 0x88AA_BB00 {
        return Err(Error::Format("not a .model".into()));
    }
    let count = u32_at(d, 0x0C)?;
    let lods = u32_at(d, 0x14)?;
    let mut bones = Vec::new();
    if count > 1 {
        let records = 0xAC + 64 * count;
        let names = records + 44 * count;
        for i in 0..count {
            let mut local = [[0.0; 4]; 4];
            for (r, row) in local.iter_mut().enumerate() {
                for (c, x) in row.iter_mut().enumerate() {
                    *x = f32_at(d, 0xAC + 64 * i + (r * 4 + c) * 4)?;
                }
            }
            let off = names + u32_at(d, records + 44 * i)?;
            let parent = u32_at(d, records + 44 * i + 4)? as i32;
            let end = d[off..].iter().position(|&b| b == 0).map_or(d.len(), |e| off + e);
            bones.push(Bone {
                name: String::from_utf8_lossy(&d[off..end]).into_owned(),
                parent: (parent >= 0).then_some(parent as usize),
                local,
            });
        }
    }
    // Slot names.
    let end = d.len().checked_sub(5 * groups).ok_or_else(|| Error::Format("model slots".into()))?;
    let blob = &d[..end];
    let blob = blob.strip_suffix(&[0]).ok_or_else(|| Error::Format("model slot names".into()))?;
    let parts: Vec<&[u8]> = blob.split(|&b| b == 0).collect();
    let materials: Vec<String> = parts[parts.len().saturating_sub(groups)..]
        .iter()
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    if materials.len() != groups {
        return Err(Error::Format("model slot count".into()));
    }
    // Per-group bones: the records with group numbers 0, 1, 2, ... at a
    // 20-byte stride, the first with a zero name offset.
    let records = (groups >= 2)
        .then(|| {
            (0..d.len().saturating_sub(20 * groups)).step_by(4).find(|&s| {
                u32_at(d, s).ok() == Some(0)
                    && [0, 1, groups - 1]
                        .iter()
                        .all(|&g| u32_at(d, s + 20 * g + 4).ok() == Some(g))
                    && (groups < 3 || u32_at(d, s + 20 * 2 + 4).ok() == Some(2))
            })
        })
        .flatten();
    let group_bones = match (records, count > 1) {
        (Some(s), true) => (0..groups)
            .map(|g| u32_at(d, s + 20 * g + 8))
            .collect::<Result<Vec<_>, _>>()?,
        _ => vec![0; groups],
    };
    // LOD 0: `{count, first, distance}` entries, just before the group records
    // (skinned) or after the name at 0x118 (static).
    let lod_at = if count <= 1 {
        Some(0x118 + u32_at(d, 0x10)?)
    } else {
        records.and_then(|s| s.checked_sub(12 * lods))
    };
    let lod0 = lod_at
        .filter(|_| lods > 0)
        .and_then(|at| Some((u32_at(d, at).ok()?, u32_at(d, at + 4).ok()?)))
        .filter(|&(c, first)| first == 0 && c > 0 && c <= groups)
        .map_or(groups, |(c, _)| c);
    Ok(Model {
        bones,
        materials,
        group_bones,
        lod0,
    })
}
