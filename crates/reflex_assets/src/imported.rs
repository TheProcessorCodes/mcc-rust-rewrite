//! What the game needs from Reflex, copied once out of the player's install
//! into [`game_paths::reflex_import_dir`] (`%LOCALAPPDATA%\MccRustRewrite\reflex-1`),
//! so the install is read only when it is chosen in setup. After that the
//! Reflex folder may move or go; the bike keeps its model, sound and physics.
//!
//! - `bike.bin`: the Cortana 450F as [`bike::load`] puts it together
//!   (bones, parts and textures; deflated, layout in [`write_bike`]);
//! - `engine.fev`, `engine.fsb`: the 450F engine's event file and sample
//!   bank as Reflex has them, decoded at load as before;
//! - `vehicle.xml`: `mx450f_vehicle` as [`vehicle::xml`] gives it;
//! - `done`: written last, so a half-finished import never counts.
//!
//! An install set up before this (the Reflex folder saved, nothing
//! imported) is imported the first time something asks for it.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::bike::{self, BikeModel, Bone, Part};
use crate::engine_audio::{self, Engine};
use crate::package::Package;
use crate::texture::Rgba;
use crate::{Error, vehicle};

const BIKE: &str = "bike.bin";
const FEV: &str = "engine.fev";
const FSB: &str = "engine.fsb";
const VEHICLE: &str = "vehicle.xml";
const DONE: &str = "done";
/// The vehicle class whose values are imported.
pub const CLASS: &str = "mx450f";
/// The first bytes of `bike.bin`.
const MAGIC: &[u8; 8] = b"MCCBIKE1";

/// Whether the import is there.
pub fn done() -> bool {
    game_paths::reflex_imported()
}

/// Whether there is Reflex to ride with: imported, or still to import
/// (`game_paths::reflex_ready`).
pub fn available() -> bool {
    game_paths::reflex_ready()
}

/// Copies what the game needs out of the Reflex install at `root`. Written
/// to a fresh folder and swapped in at the end, so a failure keeps the old
/// import.
pub fn import(root: &Path) -> Result<(), Error> {
    let dir = game_paths::reflex_import_dir();
    let fresh = dir.with_extension("new");
    let _ = std::fs::remove_dir_all(&fresh);
    std::fs::create_dir_all(&fresh)?;
    let started = std::time::Instant::now();

    let model = bike::load(root)?;
    write_bike(&model, &fresh.join(BIKE))?;
    let pkg = Package::open(&root.join("Database"), "VehicleEngineAudio")?;
    let (fev, fsb) = engine_audio::raw_450f(&pkg)?;
    // Checked here, so a bank this cannot read is found at setup.
    engine_audio::sound_defs(&fev)?;
    std::fs::write(fresh.join(FEV), fev)?;
    std::fs::write(fresh.join(FSB), fsb)?;
    std::fs::write(fresh.join(VEHICLE), vehicle::xml(root, CLASS)?)?;
    std::fs::write(fresh.join(DONE), root.display().to_string())?;

    let _ = std::fs::remove_dir_all(&dir);
    std::fs::rename(&fresh, &dir)?;
    eprintln!("reflex: imported from {} in {:.1} s", root.display(), started.elapsed().as_secs_f32());
    Ok(())
}

/// The import folder, importing first from the saved Reflex install when
/// there is no import yet (a setup from before imports existed, or
/// `REFLEX_GAME`). Tried once per run.
fn ready() -> Result<PathBuf, Error> {
    static TRIED: Mutex<Option<Result<(), String>>> = Mutex::new(None);
    if done() {
        return Ok(game_paths::reflex_import_dir());
    }
    let root = game_paths::reflex_root().ok_or_else(|| Error::Missing("no Reflex imported or set up".into()))?;
    let mut tried = TRIED.lock().unwrap_or_else(|e| e.into_inner());
    let outcome = tried.get_or_insert_with(|| import(&root).map_err(|e| e.to_string()));
    match outcome {
        Ok(()) => Ok(game_paths::reflex_import_dir()),
        Err(e) => Err(Error::Format(format!("import from {}: {e}", root.display()))),
    }
}

/// The Cortana 450F.
pub fn bike() -> Result<BikeModel, Error> {
    read_bike(&ready()?.join(BIKE))
}

/// The 450F's engine recordings and sound definitions.
pub fn engine() -> Result<Engine, Error> {
    let dir = ready()?;
    Ok(Engine {
        clips: engine_audio::read_bank(&std::fs::read(dir.join(FSB))?)?,
        defs: engine_audio::sound_defs(&std::fs::read(dir.join(FEV))?)?,
    })
}

/// `mx450f_vehicle` as XML text. Without an import yet it is read straight
/// from the saved install rather than importing: this one is asked for on
/// the game's main thread.
pub fn vehicle_xml() -> Result<String, Error> {
    if done() {
        return Ok(std::fs::read_to_string(game_paths::reflex_import_dir().join(VEHICLE))?);
    }
    let root = game_paths::reflex_root().ok_or_else(|| Error::Missing("no Reflex imported or set up".into()))?;
    vehicle::xml(&root, CLASS)
}

// `bike.bin`: MAGIC, then deflated: the kit note; the bones (name, parent
// or u32::MAX, 16 f32); the parts (bone, positions, normals, uvs, indices,
// texture or "", cutout, slot); the textures (key, width, height, RGBA).
// Counts and lengths are u32, everything little-endian.

fn write_bike(m: &BikeModel, path: &Path) -> Result<(), Error> {
    let mut w = Writer(Vec::new());
    w.str(m.kit);
    w.u32(m.bones.len() as u32);
    for b in &m.bones {
        w.str(&b.name);
        w.u32(b.parent.map_or(u32::MAX, |p| p as u32));
        w.f32s(b.local.as_flattened());
    }
    w.u32(m.parts.len() as u32);
    for p in &m.parts {
        w.u32(p.bone as u32);
        w.u32(p.positions.len() as u32);
        w.f32s(p.positions.as_flattened());
        w.u32(p.normals.len() as u32);
        w.f32s(p.normals.as_flattened());
        w.u32(p.uvs.len() as u32);
        w.f32s(p.uvs.as_flattened());
        w.u32(p.indices.len() as u32);
        for &i in &p.indices {
            w.u32(i);
        }
        w.str(p.texture.as_deref().unwrap_or(""));
        w.0.push(u8::from(p.cutout));
        w.str(&p.slot);
    }
    let mut keys: Vec<&String> = m.textures.keys().collect();
    keys.sort();
    w.u32(keys.len() as u32);
    for k in keys {
        let t = &m.textures[k];
        w.str(k);
        w.u32(t.width);
        w.u32(t.height);
        w.0.extend(t.pixels.as_flattened());
    }
    let mut file = std::fs::File::create(path)?;
    file.write_all(MAGIC)?;
    let mut z = flate2::write::DeflateEncoder::new(file, flate2::Compression::fast());
    z.write_all(&w.0)?;
    z.finish()?;
    Ok(())
}

fn read_bike(path: &Path) -> Result<BikeModel, Error> {
    let file = std::fs::read(path)?;
    let body = file
        .strip_prefix(MAGIC.as_slice())
        .ok_or_else(|| Error::Format(format!("{} is not an imported bike", path.display())))?;
    let mut data = Vec::new();
    flate2::read::DeflateDecoder::new(body).read_to_end(&mut data)?;
    let mut r = Reader { d: &data, p: 0 };
    let kit = r.str()?;
    // The notes `bike::load` gives, kept as the same `&'static str`.
    let kit = ["the Cortana kit installed in Reflex", "painted Cortana"]
        .into_iter()
        .find(|k| *k == kit)
        .unwrap_or("imported Cortana");
    let mut bones = Vec::new();
    for _ in 0..r.u32()? {
        let name = r.str()?;
        let parent = Some(r.u32()?).filter(|&p| p != u32::MAX).map(|p| p as usize);
        let f = r.f32s(16)?;
        let local = std::array::from_fn(|c| std::array::from_fn(|i| f[c * 4 + i]));
        bones.push(Bone { name, parent, local });
    }
    let mut parts = Vec::new();
    for _ in 0..r.u32()? {
        let bone = r.u32()? as usize;
        let n = r.u32()? as usize;
        let positions = r.f32s(n * 3)?.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
        let n = r.u32()? as usize;
        let normals = r.f32s(n * 3)?.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
        let n = r.u32()? as usize;
        let uvs = r.f32s(n * 2)?.chunks(2).map(|c| [c[0], c[1]]).collect();
        let n = r.u32()? as usize;
        let indices = (0..n).map(|_| r.u32()).collect::<Result<_, _>>()?;
        let texture = Some(r.str()?).filter(|t| !t.is_empty());
        let cutout = r.bytes(1)?[0] != 0;
        let slot = r.str()?;
        parts.push(Part { bone, positions, normals, uvs, indices, texture, cutout, slot });
    }
    let mut textures = HashMap::new();
    for _ in 0..r.u32()? {
        let key = r.str()?;
        let (width, height) = (r.u32()?, r.u32()?);
        let raw = r.bytes(width as usize * height as usize * 4)?;
        let pixels = raw.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
        textures.insert(key, Rgba { width, height, pixels });
    }
    Ok(BikeModel { bones, parts, textures, kit })
}

struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend(v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend(s.as_bytes());
    }
    fn f32s(&mut self, v: &[f32]) {
        for f in v {
            self.0.extend(f.to_le_bytes());
        }
    }
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8], Error> {
        let b = self
            .p
            .checked_add(n)
            .and_then(|end| self.d.get(self.p..end))
            .ok_or_else(|| Error::Format("imported bike cut short".into()))?;
        self.p += n;
        Ok(b)
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn str(&mut self) -> Result<String, Error> {
        let n = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.bytes(n)?).into_owned())
    }
    fn f32s(&mut self, n: usize) -> Result<Vec<f32>, Error> {
        let b = self.bytes(n.checked_mul(4).ok_or_else(|| Error::Format("imported bike: bad count".into()))?)?;
        Ok(b.chunks(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bike_reads_back_as_written() {
        let mut textures = HashMap::new();
        textures.insert(bike::KIT.to_string(), Rgba { width: 2, height: 1, pixels: vec![[1, 2, 3, 4], [5, 6, 7, 8]] });
        let m = BikeModel {
            bones: vec![
                Bone { name: "root".into(), parent: None, local: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.5, 1.5, -2.0, 1.0]] },
                Bone { name: "OBJ_FRT_Tire".into(), parent: Some(0), local: [[2.0; 4]; 4] },
            ],
            parts: vec![Part {
                bone: 1,
                positions: vec![[0.0, 1.0, 2.0], [3.0, 4.0, 5.0], [6.0, 7.0, 8.0]],
                normals: vec![[0.0, 1.0, 0.0]; 3],
                uvs: vec![[0.0, 0.5], [1.0, 0.25], [0.5, 1.0]],
                indices: vec![0, 1, 2],
                texture: Some(bike::KIT.into()),
                cutout: true,
                slot: "Plstc".into(),
            }],
            textures,
            kit: "painted Cortana",
        };
        let path = std::env::temp_dir().join(format!("mcc-imported-bike-{}.bin", std::process::id()));
        write_bike(&m, &path).unwrap();
        let back = read_bike(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(back.kit, "painted Cortana");
        assert_eq!(back.bones.len(), 2);
        assert_eq!(back.bones[0].parent, None);
        assert_eq!(back.bones[1].parent, Some(0));
        assert_eq!(back.bones[0].local, m.bones[0].local);
        assert_eq!(back.bones[1].name, "OBJ_FRT_Tire");
        let p = &back.parts[0];
        assert_eq!((p.bone, p.cutout, p.slot.as_str()), (1, true, "Plstc"));
        assert_eq!(p.positions, m.parts[0].positions);
        assert_eq!(p.uvs, m.parts[0].uvs);
        assert_eq!(p.indices, vec![0, 1, 2]);
        assert_eq!(p.texture.as_deref(), Some(bike::KIT));
        let t = &back.textures[bike::KIT];
        assert_eq!((t.width, t.height), (2, 1));
        assert_eq!(t.pixels, vec![[1, 2, 3, 4], [5, 6, 7, 8]]);
    }
}
