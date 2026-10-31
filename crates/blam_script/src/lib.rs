//! The HaloScript compiled into an MCC Halo 3 (U13) scenario: scripts,
//! globals, the expression ("syntax datum") table and its string data,
//! plus the scenario tables scripts name things in (object names, trigger
//! volumes, cutscene flags, point sets, editor folders, zone sets).
//!
//! Follows `docs/format/halo-script.md`. Everything here was worked out from
//! the bytes of the user's own `120_halo.map` and checked on other maps
//! (`docs/format/scripts/probe_scripts.py`); no code from other projects is
//! copied.
//!
//! The scenario keeps no function table: the engine's function list lives
//! in the game's code. Each call's first child is a "function name" node
//! whose string is the function's name and whose opcode is the function's
//! index, so [`ScriptData::function_names`] rebuilds the part of the table
//! a map uses from the map itself.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use blam_cache::{Block, CacheFile};

mod source;
pub mod types;

pub use source::Decompiler;
pub use types::{ValueType, type_name};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Cache(#[from] blam_cache::Error),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Offsets in the scenario tag (MCC U13), `halo-script.md` 1.
pub mod offsets {
    /// Data reference (20 bytes): the script string data.
    pub const STRING_DATA: usize = 0x3D8;
    /// Block of [`super::Script`], 52 bytes each.
    pub const SCRIPTS: usize = 0x3EC;
    pub const SCRIPT_SIZE: usize = 52;
    /// Block of [`super::Global`], 40 bytes each.
    pub const GLOBALS: usize = 0x3F8;
    pub const GLOBAL_SIZE: usize = 40;
    /// Block of 16-byte tag references the scripts use.
    pub const REFERENCES: usize = 0x404;
    /// Block of one "scripting data" element; its first block is the point sets.
    pub const SCRIPTING_DATA: usize = 0x41C;
    pub const POINT_SET_SIZE: usize = 52;
    pub const POINT_SIZE: usize = 60;
    /// Block of cutscene flags, 28 bytes each.
    pub const CUTSCENE_FLAGS: usize = 0x428;
    pub const CUTSCENE_FLAG_SIZE: usize = 28;
    /// Block of syntax datums (expression nodes), 24 bytes each.
    pub const NODES: usize = 0x49C;
    pub const NODE_SIZE: usize = 24;
    /// Block of editor folders, 260 bytes each.
    pub const FOLDERS: usize = 0x614;
    pub const FOLDER_SIZE: usize = 260;
    /// Object names, 36 bytes each (`scenario-and-tag-reading.md` 2.2).
    pub const OBJECT_NAMES: usize = 0xA8;
    /// Trigger volumes, 68 bytes each (`zone-sets.md` 5).
    pub const TRIGGER_VOLUMES: usize = 0x258;
    /// Zone sets, 40 bytes each (`zone-sets.md` 2).
    pub const ZONE_SETS: usize = 0x58;
}

/// When a script runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScriptType {
    /// Once, when the map starts.
    Startup,
    /// When another script `wake`s it.
    Dormant,
    /// Every tick, from the start.
    Continuous,
    /// Called by other scripts like a function.
    Static,
    /// AI command scripts (`cs_*`), run by an actor.
    CommandScript,
    /// A declaration whose body another file supplies.
    Stub,
    Other(i16),
}

impl ScriptType {
    pub fn from_raw(v: i16) -> ScriptType {
        match v {
            0 => ScriptType::Startup,
            1 => ScriptType::Dormant,
            2 => ScriptType::Continuous,
            3 => ScriptType::Static,
            4 => ScriptType::CommandScript,
            5 => ScriptType::Stub,
            v => ScriptType::Other(v),
        }
    }
    pub fn name(self) -> String {
        match self {
            ScriptType::Startup => "startup".into(),
            ScriptType::Dormant => "dormant".into(),
            ScriptType::Continuous => "continuous".into(),
            ScriptType::Static => "static".into(),
            ScriptType::CommandScript => "command_script".into(),
            ScriptType::Stub => "stub".into(),
            ScriptType::Other(v) => format!("script_type_{v}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Parameter {
    pub name: String,
    pub value_type: ValueType,
}

#[derive(Clone, Debug)]
pub struct Script {
    pub name: String,
    pub kind: ScriptType,
    pub return_type: ValueType,
    /// The body: index of its root node (always a `begin`).
    pub root: u16,
    pub parameters: Vec<Parameter>,
}

#[derive(Clone, Debug)]
pub struct Global {
    pub name: String,
    pub value_type: ValueType,
    /// The initial value's expression.
    pub init: u16,
}

/// What a node is, from its flags (`halo-script.md` 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// A call of a built-in function: `value` is the first child, the
    /// function-name node.
    Call,
    /// A call of a script: `opcode` is the script's index, `value` the
    /// first child (the name node).
    ScriptCall,
    /// A literal; for type 2 (function name) the head of a call.
    Constant,
    /// A global variable: `value` is its index in the scenario's globals,
    /// or an engine global when the low 16 bits have bit 15 set.
    Global,
    /// A parameter of the static script it is in: `value` is its index.
    Parameter,
}

/// One syntax datum, 24 bytes.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    /// Zero for a free slot.
    pub salt: u16,
    /// The function's index for calls and function names, the script's
    /// index for script calls, else the value type again.
    pub opcode: u16,
    pub value_type: ValueType,
    pub flags: u16,
    /// The next sibling's index.
    pub next: Option<u16>,
    /// Offset of the token's text in the string data. Only meaningful for
    /// function names and for constants whose type names something.
    pub string: u32,
    /// Four bytes whose meaning depends on the kind and type.
    pub value: u32,
    /// Source line.
    pub line: u16,
}

impl Node {
    pub fn is_free(&self) -> bool {
        self.salt == 0
    }
    pub fn kind(&self) -> NodeKind {
        if self.flags & 1 == 0 {
            if self.flags & 2 != 0 { NodeKind::ScriptCall } else { NodeKind::Call }
        } else if self.flags & 0x10 != 0 {
            NodeKind::Parameter
        } else if self.flags & 4 != 0 {
            NodeKind::Global
        } else {
            NodeKind::Constant
        }
    }
    /// For calls: the first child (the head).
    pub fn first_child(&self) -> Option<u16> {
        (self.value != u32::MAX).then_some(self.value as u16)
    }
    pub fn as_i16(&self) -> i16 {
        self.value as u16 as i16
    }
    pub fn as_f32(&self) -> f32 {
        f32::from_bits(self.value)
    }
    pub fn as_bool(&self) -> bool {
        self.value & 0xFF != 0
    }
}

/// A name a script object constant can point at.
#[derive(Clone, Debug)]
pub struct ObjectName {
    pub name: String,
    /// The object type (0 biped, 1 vehicle, 2 weapon, 3 equipment, 6
    /// scenery, 7 machine, 8 control, 9 sound scenery, 10 crate, ...).
    pub object_type: i16,
    /// Index into that type's placement block.
    pub placement: i16,
}

#[derive(Clone, Debug)]
pub struct CutsceneFlag {
    pub name: String,
    pub position: [f32; 3],
    /// Yaw and pitch, radians.
    pub facing: [f32; 2],
}

#[derive(Clone, Debug)]
pub struct Point {
    pub name: String,
    pub position: [f32; 3],
    pub bsp: i16,
    pub facing: [f32; 2],
}

#[derive(Clone, Debug)]
pub struct PointSet {
    pub name: String,
    pub points: Vec<Point>,
    pub bsp: i16,
}

#[derive(Clone, Debug)]
pub struct Folder {
    pub parent: i32,
    pub name: String,
}

/// Everything script-related a scenario holds.
#[derive(Clone, Debug, Default)]
pub struct ScriptData {
    pub scripts: Vec<Script>,
    pub globals: Vec<Global>,
    /// Every slot of the expression table, free ones included, by index.
    pub nodes: Vec<Node>,
    pub strings: Vec<u8>,
    /// The tags scripts use (class, tag index), for preloading.
    pub references: Vec<(String, Option<u32>)>,
    pub object_names: Vec<ObjectName>,
    pub trigger_volumes: Vec<String>,
    pub cutscene_flags: Vec<CutsceneFlag>,
    pub point_sets: Vec<PointSet>,
    pub folders: Vec<Folder>,
    pub zone_sets: Vec<String>,
}

fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

impl ScriptData {
    /// Reads the scenario's scripts and the tables they name things in.
    pub fn read(cache: &CacheFile) -> Result<ScriptData> {
        let scnr = cache
            .scenario()
            .ok_or_else(|| Error::Invalid(format!("no scenario tag named {}", cache.header.scenario_path)))?;
        let s = cache.tag_offset(scnr)?;
        let bytes = cache.bytes();
        let fixed = |o: usize, n: usize| cstr(&bytes[o..(o + n).min(bytes.len())]);
        let sid = |v: u32| cache.string(blam_cache::StringId(v)).unwrap_or("").to_string();
        let mut out = ScriptData::default();

        let data = cache.reader_at(s + offsets::STRING_DATA).data_ref()?;
        if let Some(o) = data.offset {
            let end = o + data.size as usize;
            if end > bytes.len() {
                return Err(Error::Invalid("script string data runs past the file".into()));
            }
            out.strings = bytes[o..end].to_vec();
        }

        let block: Block = cache.reader_at(s + offsets::NODES).block()?;
        for o in block.elements(offsets::NODE_SIZE) {
            let mut r = cache.reader_at(o);
            let salt = r.u16()?;
            let opcode = r.u16()?;
            let value_type = ValueType(r.u16()?);
            let flags = r.u16()?;
            let next = r.u32()?;
            let string = r.u32()?;
            let value = r.u32()?;
            let line = r.u16()?;
            out.nodes.push(Node {
                salt,
                opcode,
                value_type,
                flags,
                next: (next != u32::MAX).then_some(next as u16),
                string,
                value,
                line,
            });
        }

        let block: Block = cache.reader_at(s + offsets::SCRIPTS).block()?;
        for o in block.elements(offsets::SCRIPT_SIZE) {
            let mut r = cache.reader_at(o + 32);
            let kind = ScriptType::from_raw(r.i16()?);
            let return_type = ValueType(r.u16()?);
            let root = r.u32()? as u16;
            let params: Block = r.block()?;
            let mut parameters = Vec::new();
            for p in params.elements(36) {
                parameters.push(Parameter {
                    name: fixed(p, 32),
                    value_type: ValueType(cache.reader_at(p + 32).u16()?),
                });
            }
            out.scripts.push(Script {
                name: fixed(o, 32),
                kind,
                return_type,
                root,
                parameters,
            });
        }

        let block: Block = cache.reader_at(s + offsets::GLOBALS).block()?;
        for o in block.elements(offsets::GLOBAL_SIZE) {
            out.globals.push(Global {
                name: fixed(o, 32),
                value_type: ValueType(cache.reader_at(o + 32).u16()?),
                init: cache.reader_at(o + 36).u32()? as u16,
            });
        }

        let block: Block = cache.reader_at(s + offsets::REFERENCES).block()?;
        for o in block.elements(16) {
            let t = cache.reader_at(o).tag_ref()?;
            out.references.push((t.class.to_string(), t.index));
        }

        let block: Block = cache.reader_at(s + offsets::OBJECT_NAMES).block()?;
        for o in block.elements(36) {
            out.object_names.push(ObjectName {
                name: fixed(o, 32),
                object_type: cache.reader_at(o + 32).i16()?,
                placement: cache.reader_at(o + 34).i16()?,
            });
        }

        let block: Block = cache.reader_at(s + offsets::TRIGGER_VOLUMES).block()?;
        for o in block.elements(68) {
            out.trigger_volumes.push(sid(cache.reader_at(o).u32()?));
        }

        let block: Block = cache.reader_at(s + offsets::ZONE_SETS).block()?;
        for o in block.elements(40) {
            out.zone_sets.push(sid(cache.reader_at(o).u32()?));
        }

        let block: Block = cache.reader_at(s + offsets::CUTSCENE_FLAGS).block()?;
        for o in block.elements(offsets::CUTSCENE_FLAG_SIZE) {
            let mut r = cache.reader_at(o);
            let name = sid(r.u32()?);
            let position = r.f32x3()?;
            let facing = r.f32x2()?;
            out.cutscene_flags.push(CutsceneFlag { name, position, facing });
        }

        let block: Block = cache.reader_at(s + offsets::SCRIPTING_DATA).block()?;
        if let Some(o) = block.element(0, 1) {
            let sets: Block = cache.reader_at(o).block()?;
            for e in sets.elements(offsets::POINT_SET_SIZE) {
                let pts: Block = cache.reader_at(e + 32).block()?;
                let mut points = Vec::new();
                for p in pts.elements(offsets::POINT_SIZE) {
                    points.push(Point {
                        name: fixed(p, 32),
                        position: cache.reader_at(p + 0x20).f32x3()?,
                        bsp: cache.reader_at(p + 0x2E).i16()?,
                        facing: cache.reader_at(p + 0x34).f32x2()?,
                    });
                }
                out.point_sets.push(PointSet {
                    name: fixed(e, 32),
                    points,
                    bsp: cache.reader_at(e + 44).i16()?,
                });
            }
        }

        let block: Block = cache.reader_at(s + offsets::FOLDERS).block()?;
        for o in block.elements(offsets::FOLDER_SIZE) {
            out.folders.push(Folder {
                parent: cache.reader_at(o).i32()?,
                name: fixed(o + 4, 256),
            });
        }
        Ok(out)
    }

    /// The text at `offset` in the string data.
    pub fn string(&self, offset: u32) -> Option<&str> {
        let s = self.strings.get(offset as usize..)?;
        let end = s.iter().position(|&b| b == 0)?;
        std::str::from_utf8(&s[..end]).ok()
    }

    pub fn node(&self, index: u16) -> Option<&Node> {
        self.nodes.get(index as usize).filter(|n| !n.is_free())
    }

    /// The children of a call (head included), in order.
    pub fn children(&self, call: u16) -> Vec<u16> {
        let mut out = Vec::new();
        let Some(n) = self.node(call) else { return out };
        let mut c = n.first_child();
        while let Some(i) = c {
            if out.len() > 4096 {
                break;
            }
            out.push(i);
            c = self.node(i).and_then(|n| n.next);
        }
        out
    }

    /// The function a call node calls, by name.
    pub fn function_of(&self, call: u16) -> Option<&str> {
        let n = self.node(call)?;
        if n.kind() != NodeKind::Call {
            return None;
        }
        let head = self.node(n.first_child()?)?;
        self.string(head.string)
    }

    /// Every function opcode the map's scripts call, with its name.
    pub fn function_names(&self) -> BTreeMap<u16, String> {
        let mut out = BTreeMap::new();
        for (i, n) in self.nodes.iter().enumerate() {
            if !n.is_free() && n.kind() == NodeKind::Call
                && let Some(name) = self.function_of(i as u16)
            {
                out.insert(n.opcode, name.to_string());
            }
        }
        out
    }

    /// How many times each function is called.
    pub fn function_counts(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for (i, n) in self.nodes.iter().enumerate() {
            if !n.is_free() && n.kind() == NodeKind::Call
                && let Some(name) = self.function_of(i as u16)
            {
                *out.entry(name.to_string()).or_default() += 1;
            }
        }
        out
    }

    pub fn script_index(&self, name: &str) -> Option<usize> {
        self.scripts.iter().position(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn object_index(&self, name: &str) -> Option<usize> {
        self.object_names.iter().position(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// The scripts as readable source, one per form.
    pub fn source(&self) -> String {
        let d = Decompiler::new(self);
        let mut out = String::new();
        for (i, g) in self.globals.iter().enumerate() {
            let _ = writeln!(out, "(global {} {} {})", type_name(g.value_type), g.name, d.expression(g.init));
            let _ = i;
        }
        if !self.globals.is_empty() {
            out.push('\n');
        }
        for i in 0..self.scripts.len() {
            out.push_str(&d.script(i));
            out.push_str("\n\n");
        }
        out
    }
}
