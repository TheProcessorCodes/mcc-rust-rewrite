//! Halo CE's compiled HaloScript, read into the same
//! [`blam_script::ScriptData`] the Halo 3 reader makes, so the Halo 3
//! runtime (`mcc_script`) runs it and `blam_script`'s decompiler prints
//! it.
//!
//! Written from `docs/format/halo1.md` 12, checked on the bytes of `d40`
//! (The Maw) with `docs/format/scripts/probe_h1_scripts.py`. The layout is
//! Halo 3's with smaller pieces: 20-byte nodes (no source line), 0x5C-byte
//! scripts and globals, and Halo CE's own numbering of the value types,
//! which [`value_type`] turns into Halo 3's.

use blam_script::{CutsceneFlag, Global, Node, ObjectName, Script, ScriptData, ScriptType, ValueType};

use crate::{Error, Map, Result};

/// Scenario offsets (`halo1.md` 12).
mod offsets {
    /// Object names, 0x24 each: name, object type (u16 +0x20), placement
    /// index (i16 +0x22).
    pub const OBJECT_NAMES: usize = 0x204;
    /// Trigger volumes, 0x60 each, the name at +4.
    pub const TRIGGER_VOLUMES: usize = 0x360;
    /// Tag data field (20 bytes: size, flags, file offset, address,
    /// unused): the syntax data, a data array of nodes.
    pub const SYNTAX_DATA: usize = 0x474;
    /// Tag data field: the string data.
    pub const STRING_DATA: usize = 0x488;
    /// Scripts, 0x5C each: name, type (u16 +0x20), return type (u16
    /// +0x22), root node (datum +0x24).
    pub const SCRIPTS: usize = 0x49C;
    /// Globals, 0x5C each: name, type (u16 +0x20), initial value (datum
    /// +0x28).
    pub const GLOBALS: usize = 0x4A8;
    /// Tag references the scripts use, 0x28 each: the reference at +0x18.
    pub const REFERENCES: usize = 0x4B4;
    /// Cutscene flags, 0x5C each: name (+4), position (+0x24), yaw and
    /// pitch (+0x30).
    pub const CUTSCENE_FLAGS: usize = 0x4E4;
}

/// The syntax data's header: a data array named "script node" (0x38
/// bytes), then the nodes.
const ARRAY_HEADER: usize = 0x38;
const NODE_SIZE: usize = 20;
const ENTRY_SIZE: usize = 0x5C;

/// A Halo CE value type as Halo 3 numbers it (`blam_script::types`), so the
/// runtime and the decompiler read it the same way. Types Halo 3 has no
/// counterpart for keep a number of their own past Halo 3's (`200 + n`).
pub fn value_type(ce: u16) -> ValueType {
    ValueType(match ce {
        // unparsed, special form, function name, passthrough, void,
        // boolean, real, short, long, string, script: the same.
        0..=10 => ce,
        11 => 13, // trigger_volume
        12 => 14, // cutscene_flag
        14 => 16, // cutscene_title
        17 => 19, // ai
        23 => 31, // object_list
        24 => 33, // sound
        25 => 34, // effect
        26 => 35, // damage
        27 => 36, // looping_sound
        28 => 37, // animation_graph
        31 => 39, // object_definition
        32 => 50, // game_difficulty
        33 => 51, // team
        37 => 65, // object
        38 => 66, // unit
        39 => 67, // vehicle
        40 => 68, // weapon
        41 => 69, // device
        42 => 70, // scenery
        // object_name, unit_name, vehicle_name, weapon_name, device_name,
        // scenery_name
        43..=48 => 72,
        n => 200 + n,
    })
}

/// Reads the map's scripts. A map without any gives empty lists.
pub fn read(map: &Map) -> Result<ScriptData> {
    let scnr = map
        .tag_data(map.scenario)
        .ok_or_else(|| Error::Invalid("scenario has no data".into()))?;
    let mut out = ScriptData::default();

    let data = |off: usize| -> Result<Option<(usize, usize)>> {
        let size = scnr.u32(off)? as usize;
        let address = scnr.u32(off + 0xC)?;
        if size == 0 || address == 0 {
            return Ok(None);
        }
        let at = scnr
            .space
            .offset(address)
            .ok_or_else(|| Error::Invalid(format!("script data at {address:#x} is outside the tag data")))?;
        if at + size > map.bytes.len() {
            return Err(Error::Invalid("script data runs past the file".into()));
        }
        Ok(Some((at, size)))
    };

    if let Some((at, size)) = data(offsets::STRING_DATA)? {
        out.strings = map.bytes[at..at + size].to_vec();
    }
    if let Some((at, size)) = data(offsets::SYNTAX_DATA)? {
        let element = u16::from_le_bytes([map.bytes[at + 0x22], map.bytes[at + 0x23]]) as usize;
        if element != NODE_SIZE || &map.bytes[at + 0x28..at + 0x2C] != b"@t@d" {
            return Err(Error::Invalid(format!("script nodes: element size {element}, not a data array")));
        }
        let count = (size.saturating_sub(ARRAY_HEADER)) / NODE_SIZE;
        out.nodes.reserve(count);
        for k in 0..count {
            let o = at + ARRAY_HEADER + k * NODE_SIZE;
            let b = &map.bytes[o..o + NODE_SIZE];
            let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
            let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
            let next = u32_at(8);
            out.nodes.push(Node {
                salt: u16_at(0),
                opcode: u16_at(2),
                value_type: value_type(u16_at(4)),
                flags: u16_at(6),
                next: (next != u32::MAX).then_some(next as u16),
                string: u32_at(12),
                value: u32_at(16),
                line: 0,
            });
        }
    }

    let datum = |v: u32| (v != u32::MAX).then_some(v as u16);
    for e in scnr.block(offsets::SCRIPTS, ENTRY_SIZE)? {
        let Some(root) = datum(e.u32(0x24)?) else { continue };
        out.scripts.push(Script {
            name: e.str(0, 32),
            // startup, dormant, continuous, static, stub: Halo 3 has the
            // command script before the stub.
            kind: match e.u16(0x20)? {
                4 => ScriptType::Stub,
                k => ScriptType::from_raw(k as i16),
            },
            return_type: value_type(e.u16(0x22)?),
            root,
            parameters: Vec::new(),
        });
    }
    for e in scnr.block(offsets::GLOBALS, ENTRY_SIZE)? {
        out.globals.push(Global {
            name: e.str(0, 32),
            value_type: value_type(e.u16(0x20)?),
            init: datum(e.u32(0x28)?).unwrap_or(u16::MAX),
        });
    }
    for e in scnr.block(offsets::REFERENCES, 0x28)? {
        let tag = e.tag_ref(0x18)?;
        let t = map.tag(tag);
        out.references.push((
            t.map_or_else(String::new, |t| t.class_str()),
            t.map(|_| u32::from(tag as u16)),
        ));
    }
    for e in scnr.block(offsets::OBJECT_NAMES, 0x24)? {
        out.object_names.push(ObjectName {
            name: e.str(0, 32),
            object_type: e.u16(0x20)? as i16,
            placement: e.i16(0x22)?,
        });
    }
    for e in scnr.block(offsets::TRIGGER_VOLUMES, 0x60)? {
        out.trigger_volumes.push(e.str(4, 32));
    }
    for e in scnr.block(offsets::CUTSCENE_FLAGS, ENTRY_SIZE)? {
        let p = e.vec3(0x24)?;
        out.cutscene_flags.push(CutsceneFlag {
            name: e.str(4, 32),
            position: p.to_array(),
            facing: [e.f32(0x30)?, e.f32(0x34)?],
        });
    }
    Ok(out)
}
