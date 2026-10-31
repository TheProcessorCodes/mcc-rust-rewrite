//! HaloScript value types, by the number the expression table stores.
//!
//! The scenario does not name them. The names below are the ones whose
//! meaning the 120_halo data pins down (`halo-script.md` 4): the functions
//! that take or return them, and what their constants' values index. Types
//! not seen in a checked map print as `type_N`.

/// A value type as stored (`u16`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ValueType(pub u16);

impl ValueType {
    pub const FUNCTION_NAME: ValueType = ValueType(2);
    pub const VOID: ValueType = ValueType(4);
    pub const BOOLEAN: ValueType = ValueType(5);
    pub const REAL: ValueType = ValueType(6);
    pub const SHORT: ValueType = ValueType(7);
    pub const LONG: ValueType = ValueType(8);
    pub const STRING: ValueType = ValueType(9);
    pub const SCRIPT: ValueType = ValueType(10);
    pub const STRING_ID: ValueType = ValueType(11);
    pub const UNIT_SEAT_MAPPING: ValueType = ValueType(12);
    pub const TRIGGER_VOLUME: ValueType = ValueType(13);
    pub const CUTSCENE_FLAG: ValueType = ValueType(14);
    pub const CUTSCENE_TITLE: ValueType = ValueType(16);
    pub const AI: ValueType = ValueType(19);
    pub const AI_COMMAND_SCRIPT: ValueType = ValueType(21);
    pub const AI_LINE: ValueType = ValueType(24);
    pub const STARTING_PROFILE: ValueType = ValueType(25);
    pub const ZONE_SET: ValueType = ValueType(27);
    pub const POINT_REFERENCE: ValueType = ValueType(29);
    pub const OBJECT_LIST: ValueType = ValueType(31);
    pub const FOLDER: ValueType = ValueType(32);
    pub const SOUND: ValueType = ValueType(33);
    pub const EFFECT: ValueType = ValueType(34);
    pub const DAMAGE: ValueType = ValueType(35);
    pub const LOOPING_SOUND: ValueType = ValueType(36);
    pub const ANIMATION_GRAPH: ValueType = ValueType(37);
    pub const OBJECT_DEFINITION: ValueType = ValueType(39);
    pub const CINEMATIC: ValueType = ValueType(45);
    pub const CINEMATIC_SCENE: ValueType = ValueType(46);
    pub const GAME_DIFFICULTY: ValueType = ValueType(50);
    pub const TEAM: ValueType = ValueType(51);
    pub const MODEL_STATE: ValueType = ValueType(62);
    pub const OBJECT: ValueType = ValueType(65);
    pub const UNIT: ValueType = ValueType(66);
    pub const VEHICLE: ValueType = ValueType(67);
    pub const DEVICE: ValueType = ValueType(69);
    pub const SCENERY: ValueType = ValueType(70);
    pub const OBJECT_NAME: ValueType = ValueType(72);

    /// Types whose constants hold an object name index in the low 16 bits.
    pub fn names_object(self) -> bool {
        (65..=72).contains(&self.0) || self == ValueType::OBJECT_LIST
    }

    /// Types whose constants are tag references (the value is a full tag id).
    pub fn is_tag(self) -> bool {
        matches!(self.0, 33..=49)
    }
}

/// The name of a value type, or `type_N`.
pub fn type_name(t: ValueType) -> String {
    let s = match t.0 {
        2 => "function_name",
        4 => "void",
        5 => "boolean",
        6 => "real",
        7 => "short",
        8 => "long",
        9 => "string",
        10 => "script",
        11 => "string_id",
        12 => "unit_seat_mapping",
        13 => "trigger_volume",
        14 => "cutscene_flag",
        16 => "cutscene_title",
        19 => "ai",
        21 => "ai_command_script",
        24 => "ai_line",
        25 => "starting_profile",
        27 => "zone_set",
        29 => "point_reference",
        31 => "object_list",
        32 => "folder",
        33 => "sound",
        34 => "effect",
        35 => "damage",
        36 => "looping_sound",
        37 => "animation_graph",
        39 => "object_definition",
        45 => "cinematic_definition",
        46 => "cinematic_scene_definition",
        50 => "game_difficulty",
        51 => "team",
        62 => "model_state",
        65 => "object",
        66 => "unit",
        67 => "vehicle",
        69 => "device",
        70 => "scenery",
        72 => "object_name",
        n => return format!("type_{n}"),
    };
    s.to_string()
}
