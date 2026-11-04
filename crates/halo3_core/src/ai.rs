//! What the AI shares with the rest of the game: commands for scripts
//! (`ai_place`, `ai_erase`, `ai_kill`, allegiances), the living census
//! scripts read, damage, the hit shapes rounds are traced against, the
//! player's shields and health, and the hooks the animation code poses
//! actors through. `halo3_ai` does the work; these types live here so the
//! script and animation crates need only `halo3_core`.
//!
//! Positions are in the renderer frame (metres, Y up) unless a field says
//! Halo world units.

use std::collections::HashMap;

use bevy::prelude::*;

/// Teams as Halo 3's unit tags store them (`docs/format/ai.md`).
pub use blam_geometry::ai::team;

/// Who an AI command is about, as HaloScript names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiRef {
    /// Every squad.
    All,
    /// A squad or a squad group by name (squads win when both match), as
    /// `ai_place sentinel_left00` or `ai_place sentinels`.
    Named(String),
    /// A squad by its index in the scenario.
    Squad(usize),
}

impl AiRef {
    pub fn named(name: impl Into<String>) -> AiRef {
        AiRef::Named(name.into())
    }
}

/// What a script asks of the AI. Send with `MessageWriter<AiCommand>`.
#[derive(Message, Clone, Debug)]
pub enum AiCommand {
    /// `ai_place`: place the squad(s) at their starting locations; `count`
    /// limits the actors per squad (`ai_place squad 3`).
    Place { who: AiRef, count: Option<u32> },
    /// `ai_erase`: remove them at once, no death.
    Erase { who: AiRef },
    /// `ai_kill`: kill them (they fall and vanish).
    Kill { who: AiRef },
    /// `ai_allegiance` / `ai_allegiance_remove`: teams `a` and `b` (see
    /// [`team`]) become friends or enemies.
    Allegiance { a: i16, b: i16, allied: bool },
}

/// Living actors per squad and per squad group, rebuilt every frame
/// (`ai_living_count`, `ai_spawn_count`).
#[derive(Resource, Default, Debug)]
pub struct AiCensus {
    pub living: HashMap<String, u32>,
    /// Actors ever placed per squad and group (`ai_spawn_count`).
    pub placed: HashMap<String, u32>,
    pub total_living: u32,
}

impl AiCensus {
    /// Living actors of a squad or group; 0 for an unknown name.
    pub fn living_count(&self, name: &str) -> u32 {
        self.living.get(name).copied().unwrap_or(0)
    }

    pub fn spawn_count(&self, name: &str) -> u32 {
        self.placed.get(name).copied().unwrap_or(0)
    }
}

/// How squads get placed while scripts do not place them.
///
/// `Nearby` (the default): a squad is placed the first time the player
/// comes within `radius` metres of one of its starting locations in a
/// loaded BSP, at most `max_living` actors at once. `ZoneSet`: every squad
/// of the loaded zone set at once (`--ai zone`). `Off`: only [`AiCommand`]s
/// place squads; the script runtime sets this when it starts.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub enum AutoPlace {
    Off,
    Nearby { radius: f32, max_living: u32 },
    ZoneSet,
}

impl Default for AutoPlace {
    fn default() -> Self {
        AutoPlace::Nearby {
            radius: 70.0,
            max_living: 40,
        }
    }
}

/// Which teams are friends. Symmetric; a team is always its own friend.
#[derive(Resource, Clone, Debug)]
pub struct Allegiances {
    pairs: Vec<(i16, i16)>,
}

impl Default for Allegiances {
    /// Halo 3's sides: the player with humans and the Covenant separatists
    /// (the Arbiter); 343 Guilty Spark on the player's side until a script
    /// says otherwise. The Flood and the Sentinels are everyone's enemies
    /// and each other's.
    fn default() -> Self {
        let mut a = Allegiances { pairs: Vec::new() };
        a.set(team::PLAYER, team::HUMAN, true);
        a.set(team::PLAYER, team::COVENANT, true);
        a.set(team::HUMAN, team::COVENANT, true);
        a.set(team::PLAYER, team::GUILTY, true);
        a.set(team::HUMAN, team::GUILTY, true);
        a.set(team::COVENANT, team::GUILTY, true);
        a.set(team::SENTINEL, team::GUILTY, true);
        a
    }
}

impl Allegiances {
    fn key(a: i16, b: i16) -> (i16, i16) {
        (a.min(b), a.max(b))
    }

    pub fn allied(&self, a: i16, b: i16) -> bool {
        a == b || self.pairs.contains(&Self::key(a, b))
    }

    pub fn set(&mut self, a: i16, b: i16, allied: bool) {
        let k = Self::key(a, b);
        self.pairs.retain(|p| *p != k);
        if allied && a != b {
            self.pairs.push(k);
        }
    }
}

/// What took the damage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageTarget {
    Player,
    Actor(Entity),
}

/// What dealt it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageSource {
    Player,
    Actor(Entity),
    World,
}

/// A hit. `amount` is the damage effect's number before shields
/// (`docs/format/weapons.md` 6).
#[derive(Message, Clone, Copy, Debug)]
pub struct Damage {
    pub target: DamageTarget,
    pub source: DamageSource,
    pub amount: f32,
    pub at: Vec3,
    /// Which way the blow travelled.
    pub direction: Vec3,
    /// It hit the head sphere.
    pub head: bool,
    pub melee: bool,
}

/// One body rounds can hit: a capsule from `base` to `top` (renderer
/// frame), and a head sphere.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub entity: Entity,
    pub team: i16,
    pub base: Vec3,
    pub top: Vec3,
    pub radius: f32,
    pub head: Option<(Vec3, f32)>,
}

/// Every actor's hit shape this frame, written by `halo3_ai`, read by the
/// weapons' rounds.
#[derive(Resource, Default)]
pub struct Targets(pub Vec<Target>);

/// A ray's hit on a target.
#[derive(Clone, Copy, Debug)]
pub struct TargetHit {
    pub entity: Entity,
    pub distance: f32,
    pub head: bool,
}

impl Targets {
    /// The nearest target along a ray (`dir` unit length) within `max`,
    /// skipping `skip` (the shooter).
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max: f32, skip: Option<Entity>) -> Option<TargetHit> {
        let mut best: Option<TargetHit> = None;
        for t in &self.0 {
            if Some(t.entity) == skip {
                continue;
            }
            let head = t.head.and_then(|(c, r)| ray_sphere(origin, dir, c, r)).filter(|d| *d <= max);
            let body = ray_capsule(origin, dir, t.base, t.top, t.radius).filter(|d| *d <= max);
            let hit = match (head, body) {
                (Some(h), Some(b)) if h <= b + t.radius => Some((h, true)),
                (Some(h), None) => Some((h, true)),
                (_, Some(b)) => Some((b, false)),
                (None, None) => None,
            };
            if let Some((d, head)) = hit
                && best.is_none_or(|b| d < b.distance)
            {
                best = Some(TargetHit {
                    entity: t.entity,
                    distance: d,
                    head,
                });
            }
        }
        best
    }
}

/// Distance along a ray (unit `dir`) to a sphere, or `None`.
pub fn ray_sphere(origin: Vec3, dir: Vec3, centre: Vec3, radius: f32) -> Option<f32> {
    let m = origin - centre;
    let b = m.dot(dir);
    let c = m.length_squared() - radius * radius;
    if c > 0.0 && b > 0.0 {
        return None;
    }
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    Some((-b - disc.sqrt()).max(0.0))
}

/// Distance along a ray (unit `dir`) to a capsule from `a` to `b`.
pub fn ray_capsule(origin: Vec3, dir: Vec3, a: Vec3, b: Vec3, radius: f32) -> Option<f32> {
    let axis = b - a;
    let len2 = axis.length_squared();
    if len2 < 1e-8 {
        return ray_sphere(origin, dir, a, radius);
    }
    // The infinite cylinder first, then the end spheres.
    let ao = origin - a;
    let d_par = dir.dot(axis) / len2;
    let o_par = ao.dot(axis) / len2;
    let d_perp = dir - axis * d_par;
    let o_perp = ao - axis * o_par;
    let qa = d_perp.length_squared();
    let qb = 2.0 * d_perp.dot(o_perp);
    let qc = o_perp.length_squared() - radius * radius;
    let mut best: Option<f32> = None;
    if qa > 1e-10 {
        let disc = qb * qb - 4.0 * qa * qc;
        if disc >= 0.0 {
            let t = (-qb - disc.sqrt()) / (2.0 * qa);
            let t = if t < 0.0 && qc < 0.0 { 0.0 } else { t };
            let s = o_par + d_par * t;
            if t >= 0.0 && (0.0..=1.0).contains(&s) {
                best = Some(t);
            }
        }
    }
    for end in [a, b] {
        if let Some(t) = ray_sphere(origin, dir, end, radius)
            && best.is_none_or(|x| t < x)
        {
            best = Some(t);
        }
    }
    best
}

/// The player's shields and health (hit points as the player model's
/// damage info gives them: 70 shield, 45 body for the Chief).
#[derive(Resource, Clone, Debug)]
pub struct PlayerVitality {
    pub shield: f32,
    pub body: f32,
    pub max_shield: f32,
    pub max_body: f32,
    /// Seconds since the last hit.
    pub since_hit: f32,
    /// Dead: seconds since death; he comes back after a few.
    pub dead: Option<f32>,
    /// Takes no damage (`HALO3_GOD=1`).
    pub god: bool,
    /// Seconds of the red flash after a hit to the body.
    pub flash: f32,
}

impl Default for PlayerVitality {
    fn default() -> Self {
        PlayerVitality {
            shield: 70.0,
            body: 45.0,
            max_shield: 70.0,
            max_body: 45.0,
            since_hit: 100.0,
            dead: None,
            god: false,
            flash: 0.0,
        }
    }
}

/// The player's shield sounds: the first four entries of the HUD globals'
/// (`chgd ui\chud\globals`) sound list, as (tag class, tag path). Halo 3
/// has no low-health sound there (its next entries are the rocket and
/// missile launchers' lock tones). `halo3_audio` loads them with the map,
/// `halo3_ai` starts and stops them through [`crate::script_sound::ScriptSounds`].
pub struct ShieldSounds;

impl ShieldSounds {
    /// A one-shot as the shields take a hit.
    pub const HIT: (&str, &str) = ("snd!", r"sound\game_sfx\ui\shield_hit");
    /// Looped while the shields recharge.
    pub const CHARGE: (&str, &str) = ("lsnd", r"sound\game_sfx\ui\shield_charge\shield_charge");
    /// The low-shield beeping, looped while they are low.
    pub const LOW: (&str, &str) = ("lsnd", r"sound\game_sfx\ui\shield_low\shield_low");
    /// The alarm, looped while they are empty.
    pub const DEPLETED: (&str, &str) = ("lsnd", r"sound\game_sfx\ui\shield_depleted\shield_depleted");
    pub const ALL: [(&str, &str); 4] = [Self::HIT, Self::CHARGE, Self::LOW, Self::DEPLETED];
}

/// The game's difficulty: 0 easy, 1 normal (the default), 2 heroic, 3
/// legendary (`--difficulty`). Scales the damage actors deal the player.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Difficulty(pub usize);

impl Default for Difficulty {
    fn default() -> Self {
        Difficulty(1)
    }
}

/// How the player gets about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerMode {
    Foot,
    Bike,
    Board,
}

/// The player's body this frame, however he gets about: what the actors
/// see, aim at and swing at, and what their rounds hit. Written by
/// `halo3_ai` from the Spartan on foot, the dirt bike
/// ([`crate::bike::BikeMotion`]) or the skater
/// ([`crate::skate::SkateMotion`]); `None` while nobody plays (the free
/// camera). Renderer frame.
#[derive(Resource, Default, Clone, Debug)]
pub struct PlayerBody(pub Option<Body>);

#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub mode: PlayerMode,
    /// Where he touches the ground (on the bike: under the wheels), metres.
    pub feet: Vec3,
    /// The hit capsule's height above `feet` and its radius, metres.
    pub height: f32,
    pub radius: f32,
    /// m/s.
    pub velocity: Vec3,
    /// Facing about +Y (0 looks down +Z).
    pub heading: f32,
    /// Standing (or riding) steadily on the ground: a checkpoint may be
    /// taken here.
    pub steady: bool,
}

impl Body {
    /// The point actors aim at.
    pub fn chest(&self) -> Vec3 {
        self.feet + Vec3::Y * self.height * 0.7
    }

    pub fn centre(&self) -> Vec3 {
        self.feet + Vec3::Y * self.height * 0.5
    }

    /// The hit capsule: its two end centres and radius.
    pub fn capsule(&self) -> (Vec3, Vec3, f32) {
        let r = self.radius;
        (self.feet + Vec3::Y * r, self.feet + Vec3::Y * (self.height - r).max(r), r)
    }
}

/// Where the player comes back after dying, for the ride he is on:
/// `halo3_bike` takes `bike` (renderer frame feet and heading) and puts the
/// bike down there; `dead` holds the bike's controls at rest meanwhile.
/// (On foot `halo3_ai` moves the Spartan itself; the skater comes back
/// through [`crate::skate::Respawn`].)
#[derive(Resource, Default, Clone, Debug)]
pub struct PlayerRespawn {
    pub bike: Option<(Vec3, f32)>,
    pub dead: bool,
}

/// The scripts asked for a checkpoint (`game_save`, true) or called the
/// asked one off (`game_save_cancel`, false); `halo3_ai` takes it at the
/// player's next steady moment and clears it.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct SaveRequest(pub bool);

/// What an actor is doing, for the animation code to pick an animation
/// from. Written by `halo3_ai` every frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ActorState {
    #[default]
    Idle,
    /// Has a target, standing.
    Alert,
    Moving,
    Firing,
    Melee,
    /// Falling over; `Dead` once down.
    Dying,
    Dead,
}

/// An actor's activity (Halo frame: world units, +Z up).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ActorActivity {
    pub state: ActorState,
    /// Velocity, wu/s.
    pub velocity: Vec3,
    /// Facing: yaw about +Z from +X, radians.
    pub yaw: f32,
    /// Where it aims, unit vector.
    pub aim: Vec3,
    /// Seconds into the current melee swing or death.
    pub time_in_state: f32,
    /// It flies (sentinels, the monitor).
    pub flying: bool,
}

/// An actor's skeleton, for posing. `joints[i]` is an entity whose local
/// `Transform` is node `i` relative to its parent node (Halo model units,
/// wu); the root node's parent is the actor's root entity, which carries
/// the placement. Set at spawn to the bind pose (`bind`); the animation
/// code may overwrite the joint transforms every frame and the skinned
/// meshes follow. While `posed_externally` is false the AI leaves the
/// joints alone (and, for a dead actor, tips the root entity over).
#[derive(Component, Clone, Debug)]
pub struct ActorSkeleton {
    pub joints: Vec<Entity>,
    pub names: Vec<String>,
    pub bind: Vec<Transform>,
    /// The unit's animation graph (`jmad` tag index) and render model.
    pub animations: Option<u32>,
    pub render_model: Option<u32>,
    pub posed_externally: bool,
}

/// Marks an actor's root entity; `squad` is the scenario squad index and
/// `character` the palette index.
#[derive(Component, Clone, Debug)]
pub struct ActorTag {
    pub squad: usize,
    pub squad_name: String,
    pub character: usize,
    pub team: i16,
}

/// A handle scripts can hold outside the ECS (HaloScript's `AiHooks` is
/// called synchronously, with no world access): commands queue up here and
/// `halo3_ai` sends them as [`AiCommand`]s each frame, and copies the
/// [`AiCensus`] back so `ai_living_count` answers at once. Clone it freely;
/// insert one as a resource and give a clone to the script adapter.
#[derive(Resource, Clone, Default)]
pub struct AiBridge(pub std::sync::Arc<std::sync::Mutex<BridgeState>>);

#[derive(Default)]
pub struct BridgeState {
    pub queue: Vec<AiCommand>,
    pub living: HashMap<String, u32>,
    pub placed: HashMap<String, u32>,
    pub total_living: u32,
}

impl AiBridge {
    /// Queues a command (sent next frame).
    pub fn send(&self, cmd: AiCommand) {
        if let Ok(mut s) = self.0.lock() {
            s.queue.push(cmd);
        }
    }

    /// `ai_living_count` for a squad or group name (`squad/cell` counts
    /// the squad); `None` for every actor.
    pub fn living_count(&self, name: Option<&str>) -> u32 {
        let Ok(s) = self.0.lock() else { return 0 };
        match name {
            None => s.total_living,
            Some(n) => s.living.get(n.split(['/', '.']).next().unwrap_or(n)).copied().unwrap_or(0),
        }
    }

    pub fn spawn_count(&self, name: &str) -> u32 {
        self.0
            .lock()
            .map(|s| s.placed.get(name.split(['/', '.']).next().unwrap_or(name)).copied().unwrap_or(0))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rays_hit_capsules_and_heads() {
        // A capsule standing at x = 5 from y 0.3 to 1.7, radius 0.3.
        let (a, b) = (Vec3::new(5.0, 0.3, 0.0), Vec3::new(5.0, 1.7, 0.0));
        let d = ray_capsule(Vec3::new(0.0, 1.0, 0.0), Vec3::X, a, b, 0.3).unwrap();
        assert!((d - 4.7).abs() < 1e-4, "{d}");
        assert!(ray_capsule(Vec3::new(0.0, 2.5, 0.0), Vec3::X, a, b, 0.3).is_none());
        // Over the top cap.
        let d = ray_capsule(Vec3::new(5.0, 5.0, 0.0), Vec3::NEG_Y, a, b, 0.3).unwrap();
        assert!((d - 3.0).abs() < 1e-4, "{d}");
        let e = Entity::from_bits(1 << 32 | 7);
        let targets = Targets(vec![Target {
            entity: e,
            team: team::FLOOD,
            base: a,
            top: b,
            radius: 0.3,
            head: Some((Vec3::new(5.0, 1.8, 0.0), 0.15)),
        }]);
        let hit = targets.raycast(Vec3::new(0.0, 1.8, 0.0), Vec3::X, 100.0, None).unwrap();
        assert!(hit.head && hit.entity == e);
        assert!(targets.raycast(Vec3::new(0.0, 1.0, 0.0), Vec3::X, 100.0, Some(e)).is_none());
    }

    #[test]
    fn allegiances() {
        let mut a = Allegiances::default();
        assert!(a.allied(team::PLAYER, team::HUMAN));
        assert!(!a.allied(team::PLAYER, team::FLOOD));
        assert!(!a.allied(team::SENTINEL, team::FLOOD));
        a.set(team::SENTINEL, team::PLAYER, true);
        assert!(a.allied(team::PLAYER, team::SENTINEL));
        a.set(team::PLAYER, team::GUILTY, false);
        assert!(!a.allied(team::GUILTY, team::PLAYER));
    }
}
