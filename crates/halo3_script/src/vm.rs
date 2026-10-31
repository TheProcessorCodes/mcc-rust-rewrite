//! The HaloScript interpreter: script threads stepped at 30 ticks a
//! second, each an explicit stack of frames so a `sleep` can stop it
//! anywhere in an expression and pick up there on a later tick.
//!
//! Pure Rust: the world it changes is [`Objects`] plus the [`WorldApi`] the
//! caller passes in (the player, zone sets, effects) and the AI hooks.

use std::collections::HashSet;
use std::sync::Arc;

use bevy::log::{info, warn};
use blam_geometry::TriggerVolume;
use blam_script::{NodeKind, ScriptData, ScriptType, ValueType};
use glam::Vec3;

use halo3_core::script_sound::SoundRequest;

use crate::hooks::{AiHooks, DevicePoser, HookContext};
use crate::objects::Objects;
use crate::value::{ObjectRef, Value};

/// Script ticks per second.
pub const TICKS_PER_SECOND: f64 = 30.0;
/// Expression steps one thread may take in one tick before it is stopped
/// as runaway.
const STEP_BUDGET: u32 = 50_000;
/// Frames one thread may stack (static scripts calling static scripts).
const MAX_FRAMES: usize = 2_000;

/// What the scripts need from the game around them.
pub trait WorldApi {
    /// Where the player is, Halo world units (the skater's hips, the bike,
    /// the Spartan, or the flying camera).
    fn player_position(&self) -> Option<Vec3>;
    fn teleport_player(&mut self, position: Vec3, yaw: f32);
    fn current_zone_set(&self) -> Option<usize>;
    fn switch_zone_set(&mut self, zone_set: usize);
    /// An effect tag goes off here (drawn as a stand-in flash).
    fn effect(&mut self, tag: &str, at: Vec3);
    /// The player rides a vehicle of ours (the dirt bike). It answers for
    /// the small vehicle the map's end check accepts (see
    /// [`PLAYER_RIDE_STANDS_IN_FOR`]).
    fn player_in_vehicle(&self) -> bool {
        false
    }
}

/// AI starting locations whose vehicle the player's dirt bike stands in
/// for while he rides it (`ai_vehicle_get_from_starting_location` returns
/// the player). 120_halo's `mission_halo` ends the mission when a player
/// reaches `end_trilogy01`, at the frigate hangar's mouth, or when one of
/// the run's hidden small vehicles reaches `end_trilogy03`, in the air
/// over the gap before it (presumably because they cannot make the whole
/// jump the Warthog makes). The bike is such a vehicle: at full throttle it clears
/// `end_trilogy03` with room to spare but drops through the bottom corner
/// of `end_trilogy01`. `secret_egg/mongoose` is the Mongoose the map
/// hides on the run for one player; no other script asks where it is.
pub const PLAYER_RIDE_STANDS_IN_FOR: &[&str] = &["secret_egg/mongoose"];

/// Choices the game makes before the scripts start.
#[derive(Clone, Debug)]
pub struct Config {
    /// 0 easy, 1 normal, 2 heroic, 3 legendary.
    pub difficulty: i16,
    /// What `game_insertion_point_get` returns.
    pub insertion_point: i16,
    /// The `editor` engine global: true skips a mission's own `start`
    /// (used with a console insertion script, as the designers did).
    pub editor: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            difficulty: 1,
            insertion_point: 0,
            editor: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Special {
    Begin,
    BeginRandom,
    If,
    And,
    Or,
    Set,
    SleepUntil,
    SleepForever,
    Wake,
    None,
}

fn special(name: &str) -> Special {
    match name {
        "begin" => Special::Begin,
        "begin_random" => Special::BeginRandom,
        "if" => Special::If,
        "and" => Special::And,
        "or" => Special::Or,
        "set" => Special::Set,
        "sleep_until" => Special::SleepUntil,
        "sleep_forever" => Special::SleepForever,
        "wake" => Special::Wake,
        _ => Special::None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadState {
    /// A dormant script nobody woke yet.
    Dormant,
    Running,
    Sleeping(u64),
    Finished,
    /// `sleep_forever`, or stopped after an error.
    Stopped,
}

#[derive(Clone, Debug, Default)]
struct Frame {
    node: u16,
    args: Vec<Value>,
    scope: usize,
    phase: u8,
    order: Vec<u16>,
    started: u64,
    period: i64,
    timeout: i64,
}

impl Frame {
    fn new(node: u16, scope: usize) -> Frame {
        Frame {
            node,
            scope,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug)]
struct Thread {
    /// The script, or `None` for a console command.
    script: Option<usize>,
    root: u16,
    state: ThreadState,
    frames: Vec<Frame>,
    scopes: Vec<Vec<Value>>,
    continuous: bool,
}

enum Outcome {
    Value(Value),
    Sleep(i64),
}

enum Pending {
    Wake(usize),
    Stop(usize),
    SleepOther(usize, i64),
}

pub struct Runtime {
    pub data: Arc<ScriptData>,
    /// Per node: the interned function of a call.
    func: Vec<u16>,
    fnames: Vec<String>,
    specials: Vec<Special>,
    children: Vec<Box<[u16]>>,
    pub globals: Vec<Value>,
    threads: Vec<Thread>,
    script_thread: Vec<Option<usize>>,
    pub objects: Objects,
    pub volumes: Vec<TriggerVolume>,
    /// Folder index -> object entries.
    pub folders: Vec<Vec<u32>>,
    /// Kill volumes the scripts switched on or off, by trigger volume index.
    pub kill_volumes: Vec<(usize, bool)>,
    /// Sound function calls for the sound side, oldest first
    /// (`halo3_core::script_sound`); the game moves them on every frame.
    pub sounds: Vec<SoundRequest>,
    /// The mission dialogue's lines and the sounds' lengths
    /// (`ai_play_line*`, `sound_impulse_language_time`).
    pub dialogue: crate::dialogue::Dialogue,
    /// Trigger volumes `volume_test_players` answers true for whatever the
    /// player's position: the ones an insertion takes him to have passed
    /// through on the way (see [`Runtime::pass_volumes`]).
    pub passed_volumes: HashSet<usize>,
    pub config: Config,
    pub tick: u64,
    logged: HashSet<String>,
    rng: u32,
    pending: Vec<Pending>,
    /// Calls made so far, by function name (for the log and tests).
    pub calls: std::collections::HashMap<String, u32>,
    /// The mission is won: the script call that said so (`game_won`,
    /// `end_mission`, or `game_award_level_complete_achievements`, which
    /// 120_halo's outro calls first, once the player reaches
    /// `end_trilogy01` in the frigate's hangar mouth).
    pub mission_over: Option<String>,
}

impl Runtime {
    pub fn new(data: Arc<ScriptData>, objects: Objects, volumes: Vec<TriggerVolume>, folders: Vec<Vec<u32>>, config: Config) -> Runtime {
        let n = data.nodes.len();
        let mut func = vec![u16::MAX; n];
        let mut fnames: Vec<String> = Vec::new();
        let mut children: Vec<Box<[u16]>> = vec![Box::new([]); n];
        for i in 0..n {
            let node = &data.nodes[i];
            if node.is_free() || !matches!(node.kind(), NodeKind::Call | NodeKind::ScriptCall) {
                continue;
            }
            children[i] = data.children(i as u16).into_boxed_slice();
            if node.kind() == NodeKind::Call
                && let Some(name) = data.function_of(i as u16)
            {
                let id = match fnames.iter().position(|f| f == name) {
                    Some(p) => p,
                    None => {
                        fnames.push(name.to_string());
                        fnames.len() - 1
                    }
                };
                func[i] = id as u16;
            }
        }
        let specials = fnames.iter().map(|f| special(f)).collect();
        let mut threads = Vec::new();
        let mut script_thread = vec![None; data.scripts.len()];
        for (i, s) in data.scripts.iter().enumerate() {
            let state = match s.kind {
                ScriptType::Startup | ScriptType::Continuous => ThreadState::Running,
                ScriptType::Dormant => ThreadState::Dormant,
                _ => continue,
            };
            script_thread[i] = Some(threads.len());
            threads.push(Thread {
                script: Some(i),
                root: s.root,
                state,
                frames: Vec::new(),
                scopes: Vec::new(),
                continuous: s.kind == ScriptType::Continuous,
            });
        }
        let mut rt = Runtime {
            globals: vec![Value::Void; data.globals.len()],
            data,
            func,
            fnames,
            specials,
            children,
            threads,
            script_thread,
            objects,
            volumes,
            folders,
            kill_volumes: Vec::new(),
            sounds: Vec::new(),
            dialogue: Default::default(),
            passed_volumes: HashSet::new(),
            config,
            tick: 0,
            logged: HashSet::new(),
            rng: 0x2545_F491,
            pending: Vec::new(),
            calls: Default::default(),
            mission_over: None,
        };
        rt.init_globals();
        rt
    }

    /// Evaluates each global's initial value, in order (they may use the
    /// ones before them). No world: they are constants in practice.
    fn init_globals(&mut self) {
        struct NoWorld;
        impl WorldApi for NoWorld {
            fn player_position(&self) -> Option<Vec3> {
                None
            }
            fn teleport_player(&mut self, _: Vec3, _: f32) {}
            fn current_zone_set(&self) -> Option<usize> {
                Some(0)
            }
            fn switch_zone_set(&mut self, _: usize) {}
            fn effect(&mut self, _: &str, _: Vec3) {}
        }
        let mut ai = crate::hooks::NoAi;
        for g in 0..self.data.globals.len() {
            let init = self.data.globals[g].init;
            let ty = self.data.globals[g].value_type;
            let mut t = Thread {
                script: None,
                root: init,
                state: ThreadState::Running,
                frames: vec![Frame::new(init, 0)],
                scopes: vec![Vec::new()],
                continuous: false,
            };
            let v = self.run(&mut t, &mut NoWorld, &mut ai).unwrap_or_default();
            self.globals[g] = v.coerce(ty);
        }
    }

    pub fn function_name(&self, node: u16) -> Option<&str> {
        let f = *self.func.get(node as usize)?;
        self.fnames.get(f as usize).map(String::as_str)
    }

    pub fn script_state(&self, name: &str) -> Option<ThreadState> {
        let s = self.data.script_index(name)?;
        Some(self.threads[self.script_thread[s]?].state)
    }

    /// Runs a static script once, as if typed at the console (an
    /// insertion point such as `ins_trench_run`).
    pub fn console(&mut self, script: &str) -> bool {
        let Some(i) = self.data.script_index(script) else {
            warn!("script: no script named {script}");
            return false;
        };
        let root = self.data.scripts[i].root;
        self.threads.push(Thread {
            script: None,
            root,
            state: ThreadState::Running,
            frames: Vec::new(),
            scopes: Vec::new(),
            continuous: false,
        });
        info!("script: console runs {script}");
        true
    }

    /// Takes the player to have passed through these trigger volumes (by
    /// name): `volume_test_players` answers true for them from now on.
    /// For an insertion that starts past a volume a script still waits
    /// for (`Insertion::passed`).
    pub fn pass_volumes(&mut self, names: &[String]) {
        for n in names {
            match self.volumes.iter().position(|v| &v.name == n) {
                Some(i) => {
                    self.passed_volumes.insert(i);
                }
                None => warn!("scripts: no trigger volume {n} to pass"),
            }
        }
    }

    fn log_once(&mut self, key: &str, message: impl FnOnce() -> String) {
        if self.logged.insert(key.to_string()) {
            info!("script: {}", message());
        }
    }

    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }

    /// One script tick: every thread that is due runs until it sleeps or
    /// ends, then objects move on by a tick.
    pub fn step(&mut self, world: &mut dyn WorldApi, ai: &mut dyn AiHooks, poser: Option<&dyn DevicePoser>) {
        self.tick += 1;
        let mut i = 0;
        while i < self.threads.len() {
            let due = match self.threads[i].state {
                ThreadState::Running => true,
                ThreadState::Sleeping(t) => t <= self.tick,
                _ => false,
            };
            if due {
                let mut t = std::mem::replace(
                    &mut self.threads[i],
                    Thread {
                        script: None,
                        root: 0,
                        state: ThreadState::Stopped,
                        frames: Vec::new(),
                        scopes: Vec::new(),
                        continuous: false,
                    },
                );
                t.state = ThreadState::Running;
                if t.frames.is_empty() {
                    t.scopes = vec![Vec::new()];
                    t.frames.push(Frame::new(t.root, 0));
                }
                let done = self.run(&mut t, world, ai);
                if done.is_some() && t.state == ThreadState::Running {
                    t.state = if t.continuous {
                        ThreadState::Sleeping(self.tick + 1)
                    } else {
                        ThreadState::Finished
                    };
                }
                self.threads[i] = t;
                self.apply_pending(i);
            }
            i += 1;
        }
        let ctx = HookContext {
            player: world.player_position(),
            data: &self.data,
            time: self.tick as f64 / TICKS_PER_SECOND,
        };
        ai.tick(&ctx);
        self.objects.step((1.0 / TICKS_PER_SECOND) as f32, poser);
    }

    fn apply_pending(&mut self, current: usize) {
        for p in std::mem::take(&mut self.pending) {
            match p {
                Pending::Wake(s) => {
                    let Some(t) = self.script_thread.get(s).copied().flatten() else { continue };
                    let th = &mut self.threads[t];
                    match th.state {
                        ThreadState::Dormant => th.state = ThreadState::Running,
                        ThreadState::Sleeping(_) => th.state = ThreadState::Running,
                        _ => {}
                    }
                    let _ = current;
                }
                Pending::Stop(s) => {
                    let Some(t) = self.script_thread.get(s).copied().flatten() else { continue };
                    let th = &mut self.threads[t];
                    th.state = ThreadState::Stopped;
                    th.frames.clear();
                }
                Pending::SleepOther(s, ticks) => {
                    let Some(t) = self.script_thread.get(s).copied().flatten() else { continue };
                    let th = &mut self.threads[t];
                    if matches!(th.state, ThreadState::Running | ThreadState::Sleeping(_)) {
                        th.state = ThreadState::Sleeping(self.tick + ticks.max(1) as u64);
                    }
                }
            }
        }
    }

    fn thread_name(&self, t: &Thread) -> String {
        t.script.map_or_else(|| "console".into(), |s| self.data.scripts[s].name.clone())
    }

    /// Value of a constant, global or parameter node.
    fn leaf(&mut self, t: &Thread, frame_scope: usize, node: u16) -> Value {
        let n = self.data.nodes[node as usize];
        match n.kind() {
            NodeKind::Parameter => t
                .scopes
                .get(frame_scope)
                .and_then(|s| s.get(n.value as usize))
                .cloned()
                .unwrap_or_else(|| Value::default_of(n.value_type)),
            NodeKind::Global => {
                let index = n.value as u16;
                if index & 0x8000 == 0 {
                    self.globals.get(index as usize).cloned().unwrap_or_default()
                } else {
                    let name = self.data.string(n.string).unwrap_or("").to_string();
                    match name.as_str() {
                        "editor" => Value::Bool(self.config.editor),
                        _ => {
                            self.log_once(&format!("global {name}"), || format!("engine global {name} reads as its default"));
                            Value::default_of(n.value_type)
                        }
                    }
                }
            }
            _ => self.constant(node),
        }
    }

    fn constant(&self, node: u16) -> Value {
        let n = self.data.nodes[node as usize];
        let text = || self.data.string(n.string).map(Arc::<str>::from);
        match n.value_type {
            ValueType::BOOLEAN => Value::Bool(n.as_bool()),
            ValueType::REAL => Value::Real(n.as_f32()),
            ValueType::SHORT => Value::Short(n.as_i16()),
            ValueType::LONG => Value::Long(n.value as i32),
            ValueType::STRING => Value::Text(text().unwrap_or_else(|| Arc::from(""))),
            ValueType::VOID => Value::Void,
            ValueType::OBJECT_LIST => {
                let i = n.as_i16();
                Value::Objects(Arc::new(if i >= 0 { vec![ObjectRef(i as u32)] } else { Vec::new() }))
            }
            t if t.names_object() => {
                let i = n.as_i16();
                Value::Object((i >= 0).then_some(ObjectRef(i as u32)))
            }
            t => Value::Ref {
                ty: t,
                value: n.value,
                text: text(),
            },
        }
    }

    /// Runs a thread until it sleeps, stops or ends. `Some(value)` when it
    /// ended.
    fn run(&mut self, t: &mut Thread, world: &mut dyn WorldApi, ai: &mut dyn AiHooks) -> Option<Value> {
        let mut steps = 0u32;
        loop {
            steps += 1;
            if steps > STEP_BUDGET || t.frames.len() > MAX_FRAMES {
                let name = self.thread_name(t);
                warn!("script: {name} runs away (no sleep in {STEP_BUDGET} steps, or too deep); stopped");
                t.state = ThreadState::Stopped;
                t.frames.clear();
                return None;
            }
            let Some(top) = t.frames.last() else {
                return Some(Value::Void);
            };
            let node_index = top.node;
            let scope = top.scope;
            let Some(node) = self.data.nodes.get(node_index as usize).copied().filter(|n| !n.is_free()) else {
                warn!("script: {} reached a free node {node_index}; stopped", self.thread_name(t));
                t.state = ThreadState::Stopped;
                t.frames.clear();
                return None;
            };
            match node.kind() {
                NodeKind::Constant | NodeKind::Global | NodeKind::Parameter => {
                    let v = self.leaf(t, scope, node_index);
                    t.frames.pop();
                    if let Some(v) = deliver(t, v) {
                        return Some(v);
                    }
                }
                NodeKind::ScriptCall => {
                    let kids = self.children[node_index as usize].clone();
                    let nargs = kids.len().saturating_sub(1);
                    let script = node.opcode as usize;
                    let top = t.frames.last_mut().unwrap();
                    if top.phase == 0 {
                        if top.args.len() < nargs {
                            let k = kids[1 + top.args.len()];
                            t.frames.push(Frame::new(k, scope));
                            continue;
                        }
                        let Some(s) = self.data.scripts.get(script) else {
                            let v = Value::default_of(node.value_type);
                            t.frames.pop();
                            if let Some(v) = deliver(t, v) {
                                return Some(v);
                            }
                            continue;
                        };
                        let args: Vec<Value> = std::mem::take(&mut top.args)
                            .into_iter()
                            .zip(s.parameters.iter().map(|p| p.value_type).chain(std::iter::repeat(ValueType::VOID)))
                            .map(|(v, ty)| if ty == ValueType::VOID { v } else { v.coerce(ty) })
                            .collect();
                        top.phase = 1;
                        let root = s.root;
                        t.scopes.push(args);
                        let new_scope = t.scopes.len() - 1;
                        t.frames.push(Frame::new(root, new_scope));
                    } else {
                        let v = top.args.pop().unwrap_or_default();
                        let ret = self.data.scripts.get(script).map_or(ValueType::VOID, |s| s.return_type);
                        t.frames.pop();
                        t.scopes.pop();
                        let v = if ret == ValueType::VOID { Value::Void } else { v.coerce(ret) };
                        if let Some(v) = deliver(t, v) {
                            return Some(v);
                        }
                    }
                }
                NodeKind::Call => {
                    let kids = self.children[node_index as usize].clone();
                    let nargs = kids.len().saturating_sub(1);
                    let f = self.func[node_index as usize];
                    let sp = self.specials.get(f as usize).copied().unwrap_or(Special::None);
                    let tick = self.tick;
                    let top = t.frames.last_mut().unwrap();
                    macro_rules! push_child {
                        ($k:expr) => {{
                            let k = $k;
                            t.frames.push(Frame::new(k, scope));
                            continue;
                        }};
                    }
                    macro_rules! finish {
                        ($v:expr) => {{
                            let v = $v;
                            t.frames.pop();
                            if let Some(v) = deliver(t, v) {
                                return Some(v);
                            }
                            continue;
                        }};
                    }
                    match sp {
                        Special::Begin | Special::BeginRandom => {
                            if sp == Special::BeginRandom && top.phase == 0 {
                                let mut order: Vec<u16> = (1..=nargs as u16).collect();
                                for i in (1..order.len()).rev() {
                                    self.rng ^= self.rng << 13;
                                    self.rng ^= self.rng >> 17;
                                    self.rng ^= self.rng << 5;
                                    let j = (self.rng as usize) % (i + 1);
                                    order.swap(i, j);
                                }
                                top.order = order;
                                top.phase = 1;
                            }
                            if top.args.len() < nargs {
                                let k = if sp == Special::BeginRandom {
                                    kids[top.order[top.args.len()] as usize]
                                } else {
                                    kids[1 + top.args.len()]
                                };
                                push_child!(k);
                            }
                            let v = top.args.pop().unwrap_or_default();
                            finish!(v)
                        }
                        Special::If => match top.args.len() {
                            0 => push_child!(kids[1]),
                            1 => {
                                if top.args[0].truthy() {
                                    if nargs >= 2 {
                                        push_child!(kids[2])
                                    }
                                    finish!(Value::Void)
                                } else if nargs >= 3 {
                                    push_child!(kids[3])
                                } else {
                                    finish!(Value::default_of(node.value_type))
                                }
                            }
                            _ => {
                                let v = top.args.pop().unwrap_or_default();
                                finish!(v.coerce(node.value_type))
                            }
                        },
                        Special::And | Special::Or => {
                            let want = sp == Special::Or;
                            if let Some(last) = top.args.last()
                                && last.truthy() == want
                            {
                                finish!(Value::Bool(want))
                            }
                            if top.args.len() < nargs {
                                push_child!(kids[1 + top.args.len()])
                            }
                            finish!(Value::Bool(!want))
                        }
                        Special::Set => {
                            if nargs < 2 {
                                finish!(Value::Void)
                            }
                            if top.args.is_empty() {
                                push_child!(kids[2])
                            }
                            let v = top.args.pop().unwrap_or_default();
                            let g = self.data.nodes[kids[1] as usize];
                            let index = g.value as u16;
                            if g.kind() == NodeKind::Global
                                && index & 0x8000 == 0
                                && let Some(slot) = self.globals.get_mut(index as usize)
                            {
                                let ty = self.data.globals[index as usize].value_type;
                                *slot = v.clone().coerce(ty);
                            }
                            finish!(v)
                        }
                        Special::SleepUntil => {
                            match top.phase {
                                0 => {
                                    // The period and the timeout first.
                                    if top.args.len() < nargs.saturating_sub(1) {
                                        push_child!(kids[2 + top.args.len()])
                                    }
                                    top.period = top.args.first().map_or(30, |v| v.int() as i64).max(1);
                                    top.timeout = top.args.get(1).map_or(-1, |v| v.int() as i64);
                                    top.started = tick;
                                    top.args.clear();
                                    top.phase = 1;
                                    push_child!(kids[1])
                                }
                                1 => {
                                    let ok = top.args.pop().unwrap_or_default().truthy();
                                    let out = top.timeout >= 0 && (tick - top.started) as i64 >= top.timeout;
                                    if ok || out {
                                        finish!(Value::Void)
                                    }
                                    top.phase = 2;
                                    let period = top.period;
                                    t.state = ThreadState::Sleeping(tick + period as u64);
                                    return None;
                                }
                                _ => {
                                    top.phase = 1;
                                    push_child!(kids[1])
                                }
                            }
                        }
                        Special::SleepForever => {
                            if nargs == 0 {
                                t.state = ThreadState::Stopped;
                                t.frames.clear();
                                return None;
                            }
                            if top.args.is_empty() {
                                push_child!(kids[1])
                            }
                            let target = top.args[0].index();
                            match target {
                                Some(s) if Some(s) == t.script => {
                                    t.state = ThreadState::Stopped;
                                    t.frames.clear();
                                    return None;
                                }
                                Some(s) => self.pending.push(Pending::Stop(s)),
                                None => {}
                            }
                            finish!(Value::Void)
                        }
                        Special::Wake => {
                            if nargs == 0 {
                                finish!(Value::Void)
                            }
                            if top.args.is_empty() {
                                push_child!(kids[1])
                            }
                            if let Some(s) = top.args[0].index() {
                                self.pending.push(Pending::Wake(s));
                            }
                            finish!(Value::Void)
                        }
                        Special::None => {
                            if top.args.len() < nargs {
                                push_child!(kids[1 + top.args.len()])
                            }
                            let args = std::mem::take(&mut top.args);
                            let name = self.fnames.get(f as usize).cloned().unwrap_or_default();
                            *self.calls.entry(name.clone()).or_default() += 1;
                            match self.call(&name, &args, node.value_type, t, world, ai) {
                                Outcome::Value(v) => finish!(v.coerce(node.value_type)),
                                Outcome::Sleep(ticks) => {
                                    t.frames.pop();
                                    if let Some(v) = deliver(t, Value::Void) {
                                        return Some(v);
                                    }
                                    if ticks > 0 {
                                        t.state = ThreadState::Sleeping(tick + ticks as u64);
                                        return None;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ---- the world as scripts see it ----

    /// The object entry a script object names.
    pub fn entry(&self, o: ObjectRef) -> Option<usize> {
        let n = o.name_index()?;
        self.objects.by_name.get(n as usize).copied().flatten().map(|e| e as usize)
    }

    fn object_position(&self, o: ObjectRef, world: &dyn WorldApi, ai: &dyn AiHooks) -> Option<Vec3> {
        if o.player_index().is_some() {
            return world.player_position();
        }
        if o.0 >= ObjectRef::DYNAMIC {
            return ai.object_position(o);
        }
        let e = self.entry(o)?;
        self.objects.state[e].exists.then(|| self.objects.position(e))
    }

    fn flag(&self, v: &Value) -> Option<(Vec3, f32)> {
        let f = self.data.cutscene_flags.get(v.index()?)?;
        Some((Vec3::from(f.position), f.facing[0]))
    }

    /// A point reference: set in the high 16 bits, point in the low (all
    /// ones: any point of the set).
    fn point(&mut self, v: &Value, random: bool) -> Option<Vec3> {
        let raw = v.raw();
        let set = self.data.point_sets.get((raw >> 16) as usize)?;
        let p = raw & 0xFFFF;
        let n = set.points.len();
        if n == 0 {
            return None;
        }
        let i = if p == 0xFFFF || random && p as usize >= n {
            (self.random() * n as f32) as usize % n
        } else {
            (p as usize).min(n - 1)
        };
        Some(Vec3::from(self.data.point_sets.get((raw >> 16) as usize)?.points[i].position))
    }

    fn in_volume(&self, volume: &Value, p: Option<Vec3>) -> bool {
        match (volume.index().and_then(|i| self.volumes.get(i)), p) {
            (Some(v), Some(p)) => v.contains(p),
            _ => false,
        }
    }

    fn entries_of(&self, v: &Value) -> Vec<usize> {
        v.objects().into_iter().filter_map(|o| self.entry(o)).collect()
    }

    fn damage(&mut self, e: usize, section: &str, world: &mut dyn WorldApi) {
        let info = &self.objects.info[e];
        let known = info.sections.iter().find(|s| s.name == section).cloned();
        let destroy = section.contains("destroy") || section.contains("explode");
        let at = self.objects.position(e);
        if let Some(effect) = known.as_ref().and_then(|s| s.effect.clone()) {
            world.effect(&effect, at);
        } else if destroy {
            world.effect("explosion", at);
        }
        if known.is_none() {
            let tag = info.tag_path.clone();
            self.log_once(&format!("section {tag} {section}"), || {
                format!("{tag} has no damage section {section}; treated as {}", if destroy { "destroyed" } else { "falling" })
            });
        }
        // `HALO3_SCRIPT_TRACE=1`: every object a script damages.
        static TRACE: std::sync::LazyLock<bool> =
            std::sync::LazyLock::new(|| std::env::var("HALO3_SCRIPT_TRACE").is_ok_and(|v| v == "1"));
        if *TRACE {
            let name = self.objects.info[e].name.as_deref().unwrap_or("?");
            let exists = self.objects.state[e].exists;
            info!("script trace: tick {} {name} {section}{}", self.tick, if exists { "" } else { " (not there)" });
        }
        if destroy {
            self.objects.destroy(e);
        } else {
            // `fall` waits a moment (its response's +0x60 is 1.0),
            // `fall_immediate` does not.
            let delay = if section.contains("immediate") { 0.0 } else { 0.25 };
            self.objects.drop_object(e, delay);
        }
    }

    fn call(
        &mut self,
        name: &str,
        a: &[Value],
        ty: ValueType,
        t: &Thread,
        world: &mut dyn WorldApi,
        ai: &mut dyn AiHooks,
    ) -> Outcome {
        use Outcome::Value as V;
        let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
        let num = |i: usize| a.get(i).map_or(0.0, Value::num);
        match name {
            // ---- flow and numbers ----
            "sleep" => {
                if a.len() >= 2 {
                    if let Some(s) = a[1].index() {
                        self.pending.push(Pending::SleepOther(s, a[0].int() as i64));
                    }
                    return V(Value::Void);
                }
                Outcome::Sleep(num(0) as i64)
            }
            "not" => V(Value::Bool(!arg(0).truthy())),
            "+" => V(Value::Real(a.iter().map(Value::num).sum::<f64>() as f32)),
            "-" => V(Value::Real(if a.len() == 1 { -num(0) } else { num(0) - num(1) } as f32)),
            "*" => V(Value::Real(a.iter().map(Value::num).product::<f64>() as f32)),
            "/" => V(Value::Real(if num(1) == 0.0 { 0.0 } else { (num(0) / num(1)) as f32 })),
            "min" => V(Value::Real(a.iter().map(Value::num).fold(f64::INFINITY, f64::min) as f32)),
            "max" => V(Value::Real(a.iter().map(Value::num).fold(f64::NEG_INFINITY, f64::max) as f32)),
            "=" => V(Value::Bool(arg(0).equals(&arg(1)))),
            "!=" => V(Value::Bool(!arg(0).equals(&arg(1)))),
            "<" => V(Value::Bool(num(0) < num(1))),
            ">" => V(Value::Bool(num(0) > num(1))),
            "<=" => V(Value::Bool(num(0) <= num(1))),
            ">=" => V(Value::Bool(num(0) >= num(1))),
            "random_range" => {
                let (lo, hi) = (num(0), num(1));
                let r = self.random() as f64;
                V(Value::Long((lo + (hi - lo) * r).floor().max(lo) as i32))
            }
            "real_random_range" => {
                let (lo, hi) = (num(0), num(1));
                let r = self.random() as f64;
                V(Value::Real((lo + (hi - lo) * r) as f32))
            }
            "print" | "inspect" | "cinematic_print" => {
                let text = a.first().map(|v| v.text().map_or_else(|| format!("{v:?}"), str::to_string)).unwrap_or_default();
                info!("script {}: {text}", self.thread_name(t));
                V(Value::Void)
            }
            // ---- the end of the mission ----
            "game_won" | "end_mission" | "game_award_level_complete_achievements" => {
                if self.mission_over.is_none() {
                    info!("script {}: {name}: the mission is won", self.thread_name(t));
                    self.mission_over = Some(name.to_string());
                }
                // The thread goes no further: the rest of an outro (cutscene
                // teleports, hiding the frigate) is the game's end screen's
                // business now.
                Outcome::Sleep(1 << 40)
            }
            "script_finished" => {
                let done = arg(0)
                    .text()
                    .and_then(|n| self.script_state(n))
                    .is_some_and(|s| matches!(s, ThreadState::Finished | ThreadState::Stopped));
                V(Value::Bool(done))
            }
            // ---- the game ----
            "game_difficulty_get" | "game_difficulty_get_real" => V(Value::Ref {
                ty: ValueType::GAME_DIFFICULTY,
                value: 0xFFFF_0000 | self.config.difficulty as u16 as u32,
                text: None,
            }),
            "game_insertion_point_get" => V(Value::Short(self.config.insertion_point)),
            "game_coop_player_count" | "player_count" => V(Value::Long(1)),
            "game_is_cooperative" | "game_is_playtest" | "game_reverted" | "game_saving" => V(Value::Bool(false)),
            "players" => V(Value::Objects(Arc::new(vec![ObjectRef::player(0)]))),
            "list_get" => {
                let list = arg(0).objects();
                V(Value::Object(list.get(num(1).max(0.0) as usize).copied()))
            }
            "list_count" => V(Value::Short(arg(0).objects().len() as i16)),
            "unit" => V(Value::Object(arg(0).object())),
            "current_zone_set" | "current_zone_set_fully_active" => {
                V(Value::Long(world.current_zone_set().map_or(-1, |z| z as i32)))
            }
            "switch_zone_set" => {
                if let Some(z) = arg(0).index() {
                    info!("script {}: switch_zone_set {}", self.thread_name(t), arg(0).text().unwrap_or("?"));
                    world.switch_zone_set(z);
                }
                V(Value::Void)
            }
            // ---- trigger volumes ----
            "volume_test_players" | "volume_test_players_all" => {
                let passed = arg(0).index().is_some_and(|v| self.passed_volumes.contains(&v));
                V(Value::Bool(passed || self.in_volume(&arg(0), world.player_position())))
            }
            "volume_test_object" => {
                let (w, h) = (&*world, &*ai);
                let p = arg(1).object().and_then(|o| self.object_position(o, w, h));
                V(Value::Bool(self.in_volume(&arg(0), p)))
            }
            "volume_test_objects" | "volume_test_objects_all" => {
                let all = name.ends_with("_all");
                let objects = arg(1).objects();
                let (w, h) = (&*world, &*ai);
                let mut hits = objects.iter().map(|o| {
                    let p = self.object_position(*o, w, h);
                    self.in_volume(&arg(0), p)
                });
                V(Value::Bool(if all { !objects.is_empty() && hits.all(|h| h) } else { hits.any(|h| h) }))
            }
            "kill_volume_enable" | "kill_volume_disable" => {
                if let Some(v) = arg(0).index() {
                    self.kill_volumes.push((v, name.ends_with("enable")));
                }
                V(Value::Void)
            }
            "objects_distance_to_flag" | "objects_distance_to_object" => {
                let (w, h) = (&*world, &*ai);
                let target = if name.ends_with("flag") {
                    self.flag(&arg(1)).map(|f| f.0)
                } else {
                    arg(1).object().and_then(|o| self.object_position(o, w, h))
                };
                let d = arg(0)
                    .objects()
                    .iter()
                    .filter_map(|o| Some(self.object_position(*o, w, h)?.distance(target?)))
                    .fold(f32::INFINITY, f32::min);
                V(Value::Real(if d.is_finite() { d } else { -1.0 }))
            }
            // ---- objects ----
            "object_create" | "object_create_anew" => {
                if let Some(e) = arg(0).object().and_then(|o| self.entry(o)) {
                    self.objects.create(e, name.ends_with("anew"));
                }
                V(Value::Void)
            }
            "object_destroy" => {
                for e in self.entries_of(&arg(0)) {
                    self.objects.destroy(e);
                }
                V(Value::Void)
            }
            "object_create_folder" | "object_create_folder_anew" | "object_destroy_folder" => {
                let members = arg(0).index().and_then(|f| self.folders.get(f)).cloned().unwrap_or_default();
                for e in members {
                    if name == "object_destroy_folder" {
                        // Folders of objects placed at the start are only
                        // dropped to save the console's memory (120_halo's
                        // `object_management` drops the Warthog run's plates
                        // when its zone set loads, and `plate_management`
                        // brings them back as the player drives in). Kept
                        // here, so the floor is there wherever the player
                        // starts riding or walking.
                        if self.objects.info[e as usize].starts_absent {
                            self.objects.destroy(e as usize);
                        }
                    } else {
                        self.objects.create(e as usize, name.ends_with("anew"));
                    }
                }
                V(Value::Void)
            }
            "object_hide" => {
                let hide = arg(1).truthy();
                for e in self.entries_of(&arg(0)) {
                    self.objects.hide(e, hide);
                }
                V(Value::Void)
            }
            "object_damage_damage_section" => {
                let section = arg(1).text().unwrap_or("").to_string();
                for e in self.entries_of(&arg(0)) {
                    self.damage(e, &section, world);
                }
                V(Value::Void)
            }
            "object_teleport" | "object_teleport_to_ai_point" => {
                let target = if name == "object_teleport" {
                    self.flag(&arg(1))
                } else {
                    self.point(&arg(1), false).map(|p| (p, 0.0))
                };
                if let (Some(o), Some((p, yaw))) = (arg(0).object(), target) {
                    if o.player_index() == Some(0) {
                        info!("script {}: teleporting the player to {p}", self.thread_name(t));
                        world.teleport_player(p, yaw);
                    } else if let Some(e) = self.entry(o) {
                        let base = self.objects.info[e].base;
                        let s = &mut self.objects.state[e];
                        s.transform = glam::Mat4::from_translation(p) * glam::Mat4::from_rotation_z(yaw)
                            * glam::Mat4::from_mat3(glam::Mat3::from_mat4(base));
                        self.objects.generation += 1;
                    }
                }
                V(Value::Void)
            }
            "object_get_shield" | "object_get_health" => V(Value::Real(1.0)),
            "object_cannot_die" => {
                for e in self.entries_of(&arg(0)) {
                    self.objects.state[e].cannot_die = arg(1).truthy();
                }
                V(Value::Void)
            }
            // ---- devices ----
            "device_set_position" | "device_set_position_immediate" | "device_animate_position" => {
                let seconds = match name {
                    "device_set_position" => 2.0,
                    "device_animate_position" => num(2) as f32,
                    _ => 0.0,
                };
                for e in self.entries_of(&arg(0)) {
                    self.objects.state[e].device.animate(num(1) as f32, seconds);
                }
                V(Value::Void)
            }
            "device_get_position" => {
                V(Value::Real(arg(0).object().and_then(|o| self.entry(o)).map_or(0.0, |e| self.objects.state[e].device.position)))
            }
            "device_set_position_track" => {
                let track = arg(1).text().map(str::to_string);
                for e in self.entries_of(&arg(0)) {
                    self.objects.state[e].device.track = track.clone();
                }
                V(Value::Void)
            }
            "device_set_overlay_track" => {
                let track = arg(1).text().map(str::to_string);
                for e in self.entries_of(&arg(0)) {
                    self.objects.state[e].device.overlay_track = track.clone();
                }
                V(Value::Void)
            }
            "device_animate_overlay" => {
                for e in self.entries_of(&arg(0)) {
                    self.objects.state[e].device.animate_overlay(num(1) as f32, num(2) as f32);
                }
                V(Value::Void)
            }
            "device_set_power" => {
                for e in self.entries_of(&arg(0)) {
                    self.objects.state[e].device.power = num(1) as f32;
                }
                V(Value::Void)
            }
            "device_get_power" => {
                V(Value::Real(arg(0).object().and_then(|o| self.entry(o)).map_or(0.0, |e| self.objects.state[e].device.power)))
            }
            // ---- effects ----
            "effect_new" | "effect_new_random" | "effect_new_at_ai_point" | "effect_new_on_object_marker" => {
                let tag = arg(0).text().unwrap_or("effect").to_string();
                let at = match name {
                    "effect_new" => self.flag(&arg(1)).map(|f| f.0),
                    "effect_new_on_object_marker" => {
                        let (w, h) = (&*world, &*ai);
                        arg(1).object().and_then(|o| self.object_position(o, w, h))
                    }
                    _ => self.point(&arg(1), name == "effect_new_random"),
                };
                if let Some(at) = at {
                    world.effect(&tag, at);
                }
                V(Value::Void)
            }
            // ---- sound: handed to the sound side ----
            "sound_looping_start"
            | "sound_looping_stop"
            | "sound_looping_stop_immediately"
            | "sound_looping_set_alternate"
            | "sound_impulse_start"
            | "sound_impulse_stop"
            | "sound_class_set_gain" => {
                if let Some(r) = self.sound_request(name, a, world, ai) {
                    if !matches!(r, SoundRequest::Impulse { .. }) {
                        info!("script {}: {name} {}", self.thread_name(t), sound_args(a));
                    }
                    self.sounds.push(r);
                }
                V(Value::Void)
            }
            // ---- dialogue: a line of the mission dialogue by name ----
            // `ai_play_line_on_object object line`, `ai_play_line ai line`,
            // `vs_play_line ai interrupt line`: the line plays (at the
            // object when one is named; Cortana's `none` is in the player's
            // ear) and the call returns its length in ticks, which the
            // scripts sleep on.
            "ai_play_line_on_object" | "ai_play_line" | "vs_play_line" => {
                let line_arg = a.last().cloned().unwrap_or_default();
                let at = if name == "ai_play_line_on_object" {
                    let (w, h) = (&*world, &*ai);
                    arg(0).object().and_then(|o| self.object_position(o, w, h))
                } else {
                    None
                };
                let Some(line) = line_arg.text().and_then(|n| self.dialogue.line(n)).cloned() else {
                    let n = line_arg.text().unwrap_or("?").to_string();
                    self.log_once(&format!("line {n}"), || format!("{name}: no dialogue line {n}; plays nothing"));
                    return V(Value::Real(0.0).coerce(ty));
                };
                let seconds = self.dialogue.seconds_of(line.sound);
                info!(
                    "script {}: {name} {} ({}, {seconds:.2} s)",
                    self.thread_name(t),
                    line_arg.text().unwrap_or("?"),
                    line.speaker
                );
                self.sounds.push(SoundRequest::Impulse {
                    tag: line.sound,
                    name: line.sound_name,
                    at,
                    scale: 1.0,
                });
                V(Value::Real((seconds as f64 * TICKS_PER_SECOND).round() as f32).coerce(ty))
            }
            // `sound_impulse_language_time snd`: the sound's length in
            // ticks, in the player's language.
            "sound_impulse_language_time" => {
                let tag = arg(0).raw() & 0xFFFF;
                let seconds = self.dialogue.seconds_of(tag);
                V(Value::Real((seconds as f64 * TICKS_PER_SECOND).round() as f32).coerce(ty))
            }
            // Cinematic music bookkeeping: nothing to do without cinematics.
            "sound_impulse_mark_as_outro" => V(Value::Void),
            // ---- AI, units and vehicles: the enemies' side ----
            // The player's bike answers for the map's hidden Mongoose (see
            // `PLAYER_RIDE_STANDS_IN_FOR`).
            "ai_vehicle_get_from_starting_location"
                if world.player_in_vehicle() && arg(0).text().is_some_and(|t| PLAYER_RIDE_STANDS_IN_FOR.contains(&t)) =>
            {
                V(Value::Object(Some(ObjectRef::player(0))))
            }
            n if is_ai_function(n) => {
                let ctx = HookContext {
                    player: world.player_position(),
                    data: &self.data,
                    time: self.tick as f64 / TICKS_PER_SECOND,
                };
                match ai.call(n, a, &ctx) {
                    Some(v) => V(v),
                    None => {
                        self.log_once(n, || format!("{n} has no AI behind it yet; returns its default"));
                        V(Value::default_of(ty))
                    }
                }
            }
            n => {
                self.log_once(n, || format!("{n} is not implemented; does nothing"));
                V(Value::default_of(ty))
            }
        }
    }
}

impl Runtime {
    /// The request a sound function call makes (`None`: no tag).
    fn sound_request(&self, name: &str, a: &[Value], world: &dyn WorldApi, ai: &dyn AiHooks) -> Option<SoundRequest> {
        let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
        let tag = || {
            let raw = arg(0).raw() & 0xFFFF;
            (raw != 0xFFFF).then_some(raw)
        };
        let path = || arg(0).text().unwrap_or("").to_string();
        let at = || arg(1).object().and_then(|o| self.object_position(o, world, ai));
        let scale = || a.get(2).map_or(1.0, |v| v.num() as f32);
        Some(match name {
            "sound_looping_start" => SoundRequest::LoopStart {
                tag: tag()?,
                name: path(),
                at: at(),
                scale: scale(),
            },
            "sound_looping_stop" | "sound_looping_stop_immediately" => SoundRequest::LoopStop {
                tag: tag()?,
                immediately: name.ends_with("immediately"),
            },
            "sound_looping_set_alternate" => SoundRequest::LoopAlternate {
                tag: tag()?,
                on: arg(1).truthy(),
            },
            "sound_impulse_start" => SoundRequest::Impulse {
                tag: tag()?,
                name: path(),
                at: at(),
                scale: scale(),
            },
            "sound_impulse_stop" => SoundRequest::ImpulseStop { tag: tag()? },
            "sound_class_set_gain" => SoundRequest::ClassGain {
                class: arg(0).text().unwrap_or("").to_string(),
                gain: arg(1).num() as f32,
                seconds: (arg(2).num() / TICKS_PER_SECOND) as f32,
            },
            _ => return None,
        })
    }
}

/// A sound call's arguments for the log: tag paths by their last part.
fn sound_args(a: &[Value]) -> String {
    a.iter()
        .map(|v| match v.text() {
            Some(t) => t.rsplit('\\').next().unwrap_or(t).to_string(),
            None => match v {
                Value::Object(None) => "none".into(),
                v => format!("{}", v.num()),
            },
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Functions the AI hooks answer.
pub fn is_ai_function(name: &str) -> bool {
    name.starts_with("ai_")
        || name.starts_with("cs_")
        || name.starts_with("vs_")
        || name.starts_with("unit_")
        || name.starts_with("vehicle_")
        || matches!(name, "object_get_ai" | "objects_can_see_object" | "objects_can_see_flag" | "magic_seat_name")
}

/// Hands a finished frame's value to the frame under it; `Some` when it
/// was the last.
fn deliver(t: &mut Thread, v: Value) -> Option<Value> {
    match t.frames.last_mut() {
        Some(parent) => {
            parent.args.push(v);
            None
        }
        None => Some(v),
    }
}
