//! Getting a map's scripts ready: which objects they move (drawn on their
//! own), the object table, the folders.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use blam_cache::CacheFile;
use blam_geometry::{Kind, Scene};
use blam_script::{NodeKind, ScriptData, ValueType};

use crate::objects::{ObjectInfo, Objects, Section};
use crate::vm::{Config, Runtime};

/// The object names and folders any script constant names.
pub fn named_by_scripts(data: &ScriptData) -> (HashSet<u16>, HashSet<u16>) {
    let mut names = HashSet::new();
    let mut folders = HashSet::new();
    for n in &data.nodes {
        if n.is_free() || n.kind() != NodeKind::Constant {
            continue;
        }
        let i = n.as_i16();
        if i < 0 {
            continue;
        }
        if n.value_type.names_object() {
            names.insert(i as u16);
        } else if n.value_type == ValueType::FOLDER {
            folders.insert(i as u16);
        }
    }
    (names, folders)
}

/// Marks the scene objects scripts can move or hide as dynamic, so the
/// world draws each on its own. Returns how many.
pub fn mark_dynamic(scene: &mut Scene, data: &ScriptData) -> usize {
    let (names, folders) = named_by_scripts(data);
    let mut n = 0;
    for o in &mut scene.objects {
        o.dynamic = o.name_index.is_some_and(|i| names.contains(&i))
            || o.folder.is_some_and(|f| folders.contains(&f))
            || o.starts_absent();
        n += o.dynamic as usize;
    }
    n
}

/// Builds the runtime for a decoded map.
pub fn runtime(cache: &CacheFile, scene: &Scene, data: Arc<ScriptData>, config: Config) -> Runtime {
    let mut sections: HashMap<u32, Vec<Section>> = HashMap::new();
    let mut info = Vec::with_capacity(scene.objects.len());
    let mut by_name: Vec<Option<u32>> = vec![None; data.object_names.len()];
    for (i, o) in scene.objects.iter().enumerate() {
        let secs = if o.dynamic {
            sections
                .entry(o.tag)
                .or_insert_with(|| {
                    let Some(tag) = cache.tags.get(o.tag) else {
                        return Vec::new();
                    };
                    blam_geometry::objects::damage_sections(cache, tag)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|s| Section {
                            effects: s
                                .responses
                                .iter()
                                .flat_map(|r| [r.effect, r.effect2])
                                .flatten()
                                .filter_map(|t| cache.tags.get(t))
                                .map(|t| t.name().to_string())
                                .collect(),
                            name: s.name,
                        })
                        .collect()
                })
                .clone()
        } else {
            Vec::new()
        };
        if let Some(n) = o.name_index
            && let Some(slot) = by_name.get_mut(n as usize)
        {
            *slot = Some(i as u32);
        }
        info.push(ObjectInfo {
            name: o.name.clone(),
            scene: Some(i as u32),
            object_type: o
                .name_index
                .and_then(|n| data.object_names.get(n as usize))
                .map_or(kind_type(o.kind), |n| n.object_type),
            base: o.transform,
            folder: o.folder,
            starts_absent: o.starts_absent(),
            sections: secs,
            tag_path: o.tag_path.clone(),
            tag: Some(o.tag),
            contacts: Vec::new(),
        });
    }
    // Named objects nothing draws (sound scenery, bipeds, vehicles).
    for (n, name) in data.object_names.iter().enumerate() {
        if by_name[n].is_none() {
            by_name[n] = Some(info.len() as u32);
            info.push(ObjectInfo {
                name: Some(name.name.clone()),
                scene: None,
                object_type: name.object_type,
                base: glam::Mat4::IDENTITY,
                folder: None,
                starts_absent: false,
                sections: Vec::new(),
                tag_path: String::new(),
                tag: None,
                contacts: Vec::new(),
            });
        }
    }
    let mut folders: Vec<Vec<u32>> = vec![Vec::new(); data.folders.len()];
    for (i, o) in info.iter().enumerate() {
        if let Some(f) = o.folder
            && let Some(list) = folders.get_mut(f as usize)
        {
            list.push(i as u32);
        }
    }
    // Crates scripts drop land on the floor under them (the tunnels'
    // girders); only when the scene has collision.
    let (contacts, ground) = crate::ground::build(&scene.collision, &info);
    for (o, c) in info.iter_mut().zip(contacts) {
        o.contacts = c;
    }
    let mut objects = Objects::new(info, by_name);
    objects.ground = ground;
    let mut rt = Runtime::new(data, objects, scene.trigger_volumes.clone(), folders, config);
    rt.dialogue = crate::dialogue::Dialogue::read(cache);
    rt
}

fn kind_type(kind: Kind) -> i16 {
    match kind {
        Kind::Vehicle => 1,
        Kind::Machine => 7,
        Kind::Control => 8,
        Kind::Crate => 10,
        _ => 6,
    }
}

/// A start the map's own insertion scripts do not have (`--insertion
/// trench_end`): once the insertion script has run its start-up ticks, the
/// zone set switches and the player is put here.
#[derive(Clone, Debug)]
pub struct StartOverride {
    /// Scenario zone set index.
    pub zone_set: Option<usize>,
    /// Feet position, Halo world units, and facing (radians from +X).
    pub at: glam::Vec3,
    pub yaw: f32,
}

/// Switches to the start's zone set and puts the player there, then runs
/// `ticks` more so the scripts catch up with both (120_halo's
/// `object_management` switches its kill volumes off and builds the
/// frigate's lifts as it sees the zone sets come in).
pub fn apply_start(
    rt: &mut Runtime,
    world: &mut dyn crate::vm::WorldApi,
    ai: &mut dyn crate::hooks::AiHooks,
    poser: Option<&dyn crate::hooks::DevicePoser>,
    start: &StartOverride,
    ticks: usize,
) {
    if let Some(z) = start.zone_set {
        world.switch_zone_set(z);
    }
    world.teleport_player(start.at, start.yaw);
    for _ in 0..ticks {
        rt.step(world, ai, poser);
    }
    // The last word: no script moved the player elsewhere meanwhile.
    world.teleport_player(start.at, start.yaw);
}

/// What `--insertion NAME` runs and where it puts the player.
#[derive(Clone, Debug)]
pub struct Insertion {
    /// The insertion script, run as at the console (`ins_trench_run`).
    pub script: String,
    /// Where the player starts (feet, Halo frame) and which way he faces,
    /// when known before the scripts run: the bike is dropped there.
    pub start: Option<(glam::Vec3, f32)>,
    /// Applied after the script's start-up ticks.
    pub start_override: Option<StartOverride>,
    /// Trigger volumes the player is taken to have passed through on the
    /// way to this start (`Runtime::pass_volumes`, [`PASSED_ON_THE_WAY`]).
    pub passed: Vec<String>,
}

/// The insertion `--insertion NAME` names: the map's own `ins_NAME`, which
/// puts player 0 on its `NAME_player0` flag when it has one
/// (`trench_run`: `trench_run_player0`, the Warthog run's start); or on
/// 120_halo, `trench_end`, ours: `ins_trench_run`, then the zone set of the
/// run's last BSP (`120_110_120`) and the last stretch before the jump
/// into the frigate ([`trench_end_start`]). `None` when the map has
/// neither.
pub fn insertion(data: &ScriptData, scene: &Scene, name: &str) -> Option<Insertion> {
    let bare = name.strip_prefix("ins_").unwrap_or(name);
    if bare == TRENCH_END && data.script_index("ins_trench_run").is_some() {
        let (at, yaw) = trench_end_start(data, scene)?;
        let zone_set = scene.zone_sets.iter().position(|z| z.name == "120_110_120");
        return Some(Insertion {
            script: "ins_trench_run".into(),
            start: Some((at, yaw)),
            start_override: Some(StartOverride { zone_set, at, yaw }),
            passed: Vec::new(),
        });
    }
    let script = format!("ins_{bare}");
    data.script_index(&script)?;
    let flag = format!("{bare}_player0");
    let start = data.cutscene_flags.iter().find(|f| f.name == flag).map(|f| {
        let at = glam::Vec3::from(f.position);
        // Facing: the flag's, unless the map's own first trigger of that
        // stretch says which way it goes (`FACE_TOWARDS`).
        let yaw = FACE_TOWARDS
            .iter()
            .find(|(ins, _)| *ins == bare)
            .and_then(|(_, volume)| scene.trigger_volumes.iter().find(|v| v.name == *volume))
            .map(|v| {
                let left = v.up.cross(v.forward);
                let d = (v.position + v.forward * v.extents.x * 0.5 + left * v.extents.y * 0.5 - at).truncate();
                d.y.atan2(d.x)
            })
            .unwrap_or(f.facing[0]);
        (at, yaw)
    });
    Some(Insertion {
        script,
        start,
        // A turned start: the scripts' own teleport (and the spot a fall
        // before riding anywhere comes back to) faces the same way.
        start_override: start
            .filter(|_| FACE_TOWARDS.iter().any(|(ins, _)| *ins == bare))
            .map(|(at, yaw)| StartOverride { zone_set: None, at, yaw }),
        passed: PASSED_ON_THE_WAY
            .iter()
            .filter(|(ins, _)| *ins == bare)
            .map(|(_, volume)| volume.to_string())
            .collect(),
    })
}

/// Our insertion near the end of 120_halo's Warthog run.
pub const TRENCH_END: &str = "trench_end";

/// The start of `trench_end`, from the map's own navigation flags: on
/// `trench_run_nav08`, the last waypoint of the Warthog run (about 75 wu
/// before the ramp the run ends on), facing `trench_run_nav09`, the
/// waypoint in the frigate's hangar mouth; just above the floor under the
/// flag (the bike is dropped onto it).
pub fn trench_end_start(data: &ScriptData, scene: &Scene) -> Option<(glam::Vec3, f32)> {
    let flag = |name: &str| data.cutscene_flags.iter().find(|f| f.name == name).map(|f| glam::Vec3::from(f.position));
    let from = flag(TRENCH_END_FLAG)?;
    let to = flag("trench_run_nav09")?;
    let d = (to - from).truncate();
    let yaw = d.y.atan2(d.x);
    let floor = floor_under(scene, from)?;
    Some((glam::Vec3::new(from.x, from.y, floor + 0.3), yaw))
}

/// The flag `trench_end` starts on.
pub const TRENCH_END_FLAG: &str = "trench_run_nav08";

/// The highest upward-facing collision under `p` (no more than 1 wu above
/// it, within 40 wu below), Halo z.
pub fn floor_under(scene: &Scene, p: glam::Vec3) -> Option<f32> {
    scene
        .collision
        .triangles
        .iter()
        .filter_map(|t| height_on(t.points, p.truncate()))
        .filter(|z| *z <= p.z + 1.0 && *z > p.z - 40.0)
        .reduce(f32::max)
}

/// The height of triangle `t` straight above or below `p`, if it covers it
/// and faces up.
pub fn height_on(t: [glam::Vec3; 3], p: glam::Vec2) -> Option<f32> {
    let (a, b, c) = (t[0].truncate(), t[1].truncate(), t[2].truncate());
    let d = (b - a).perp_dot(c - a);
    if d.abs() < 1e-9 || (t[1] - t[0]).cross(t[2] - t[0]).z <= 0.0 {
        return None;
    }
    let w1 = (p - a).perp_dot(c - a) / d;
    let w2 = (b - a).perp_dot(p - a) / d;
    let w0 = 1.0 - w1 - w2;
    (w0 >= -1e-4 && w1 >= -1e-4 && w2 >= -1e-4).then(|| t[0].z * w0 + t[1].z * w1 + t[2].z * w2)
}

/// Insertions whose start flag faces the wrong way for us, and the trigger
/// volume to face instead. 120_halo's `trench_run_player0` has no facing
/// set (0: +X, like every flag of the run), which looks into the side of
/// the canyon the run starts in; the canyon runs north to the run's first
/// trigger, `120_08_trench_encounter_100`.
pub const FACE_TOWARDS: &[(&str, &str)] = &[("trench_run", "120_08_trench_encounter_100")];

/// Insertions that start past a trigger volume a script they wake still
/// waits for, and that volume. 120_halo's `120_08_trench_run` wakes
/// `md_09_cortana_hints`, whose first wait is for the end of the hallway
/// before the run (`120_07_c_hall_end`): played through, the player has
/// just passed it ("There! Johnson's Warthog!"), but `ins_trench_run` puts
/// him beyond it, so without this the run's first six Cortana lines never
/// play (`ins_trench_run` itself does nothing about it).
pub const PASSED_ON_THE_WAY: &[(&str, &str)] = &[("trench_run", "120_07_c_hall_end")];
