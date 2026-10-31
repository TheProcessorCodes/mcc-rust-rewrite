//! Field offsets of the MCC Reach (update 13) tags this crate reads, in one
//! place. Each is written up in `docs/format/reach.md` and checked against
//! the user's maps by the `probe_reach_*.py` scripts.

/// `scnr`.
pub mod scnr {
    pub const STRUCTURE_BSPS: usize = 0x50;
    pub const STRUCTURE_BSP_SIZE: usize = 0xAC;
    pub const SKIES: usize = 0x88;
    pub const SKY_SIZE: usize = 0x30;
    pub const ZONE_SETS: usize = 0xAC;
    pub const ZONE_SET_SIZE: usize = 0x13C;
    /// Zone set: the BSPs it loads, bit `n` for BSP `n`.
    pub const ZONE_SET_BSPS: usize = 0x10C;
    pub const OBJECT_NAMES: usize = 0xF0;
    pub const SCENERY: usize = 0xFC;
    pub const SCENERY_PALETTE: usize = 0x108;
    pub const SCENERY_SIZE: usize = 0xDC;
    pub const MACHINES: usize = 0x180;
    pub const MACHINE_PALETTE: usize = 0x18C;
    pub const MACHINE_SIZE: usize = 0xE4;
    pub const CONTROLS: usize = 0x1B0;
    pub const CONTROL_PALETTE: usize = 0x1BC;
    pub const CONTROL_SIZE: usize = 0xDC;
    pub const CRATES: usize = 0x600;
    pub const CRATE_PALETTE: usize = 0x60C;
    pub const CRATE_SIZE: usize = 0xD8;
    pub const WEAPONS: usize = 0x15C;
    pub const WEAPON_PALETTE: usize = 0x168;
    pub const WEAPON_SIZE: usize = 0xD0;
    pub const PLAYER_STARTING_LOCATIONS: usize = 0x274;
    pub const PLAYER_STARTING_LOCATION_SIZE: usize = 0x1C;
    pub const TRIGGER_VOLUMES: usize = 0x280;
    pub const TRIGGER_VOLUME_SIZE: usize = 0x7C;
    pub const KILL_TRIGGERS: usize = 0x4C8;
    pub const LIGHTMAP: usize = 0x708;
}

/// Object placements (scenery, crates, machines, controls, weapons).
pub mod placement {
    pub const POSITION: usize = 0x08;
    pub const ROTATION: usize = 0x14;
    pub const SCALE: usize = 0x20;
    pub const ORIGIN_BSP: usize = 0x40;
}

/// `sbsp`.
pub mod sbsp {
    pub const COLLISION_MATERIALS: usize = 0x58;
    pub const WORLD_BOUNDS: usize = 0xF0;
    pub const CLUSTERS: usize = 0x138;
    pub const CLUSTER_SIZE: usize = 0x8C;
    pub const CLUSTER_MESH: usize = 0x40;
    pub const MATERIALS: usize = 0x144;
    pub const INSTANCE_NAMES: usize = 0x258;
    /// The BSP's own copy of its geometry (third render geometry struct),
    /// used when it has no lightmap BSP data.
    pub const OWN_MESHES: usize = 0x450;
    pub const OWN_COMPRESSION: usize = 0x45C;
    pub const OWN_RESOURCE: usize = 0x4E0;
    /// `structure_bsp_tag_resources`: collision BSPs and instanced geometry.
    pub const COLLISION_RESOURCE: usize = 0x518;
    /// `structure_bsp_cache_file_tag_resources`: the instances.
    pub const CACHE_FILE_RESOURCE: usize = 0x520;
}

/// `Lbsp` (scenario lightmap BSP data).
pub mod lbsp {
    pub const PRIMARY_MAP: usize = 0x1C;
    pub const INTENSITY_MAP: usize = 0x2C;
    pub const MESHES: usize = 0x7C;
    pub const COMPRESSION: usize = 0x88;
    pub const RESOURCE: usize = 0x10C;
}

/// `mode` (render model).
pub mod mode {
    pub const REGIONS: usize = 0x0C;
    pub const INSTANCED_MESH: usize = 0x1C;
    pub const INSTANCES: usize = 0x20;
    pub const NODES: usize = 0x30;
    pub const SHADERS: usize = 0x48;
    pub const SHADER_SIZE: usize = 0x2C;
    pub const MESHES: usize = 0x68;
    pub const COMPRESSION: usize = 0x74;
    pub const RESOURCE: usize = 0xF8;
}

/// Objects (`scen`, `bloc`, `mach`, `ctrl`, `weap`, ...): the model.
pub const OBJECT_MODEL: usize = 0x64;
