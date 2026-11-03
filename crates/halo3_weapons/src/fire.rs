//! Rounds in flight and what they leave: tracers, muzzle flashes, sparks,
//! scorch marks, explosions. Renderer frame (metres, Y up).
//!
//! A shot goes where the crosshair points: a ray from the camera finds
//! the target, and the round leaves the muzzle towards it, spread by the
//! barrel's error angle. Every Halo 3 weapon fires a projectile; each one
//! here flies at its tag's speed (changing from initial to final over its
//! range) and is traced against the collision each frame, so the fast ones
//! (sniper, laser) land at once and the slow ones (plasma, spikes, grenades)
//! can be watched.
//!
//! Rounds hit actors too: each step is also traced against the AI's hit
//! shapes (`halo3_core::ai::Targets`) and, for an actor's rounds, the
//! player's capsule (`halo3_core::ai::PlayerBody`: on foot, on the bike or
//! on the board); a hit sends a `Damage` message with the projectile's
//! damage effect numbers. Explosives hurt everything near where they go off.
//! Rounds fly on the shared collision (`halo3_bike::SharedWorld`, the same
//! one the Spartan walks on).

use bevy::prelude::*;
use halo3_core::map::MapAppExt;

use blam_geometry::weapons::Barrel;

use super::assets::Kind;
use super::enemy::{Enemy, EnemyKilled};
use super::{WU, WeaponImpact};
use halo3_core::ai::{Damage, DamageSource, DamageTarget, PlayerBody, Targets, ray_capsule};
use halo3_core::zones::LoadedBsps;
use halo3_bike::SharedWorld;
use halo3_bike::physics::World;
use halo3_foot::OnFoot;

pub struct FirePlugin;

/// Rounds launched and flown; the player's body is published before.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FireSet;

/// The player's hit capsule (ends and radius), when he plays.
type Capsule = Option<(Vec3, Vec3, f32)>;

/// The collision rounds fly on: the Spartan's on foot, else the shared one
/// of the loaded BSPs once built.
fn world_of(foot: &OnFoot, shared: &SharedWorld, loaded: &LoadedBsps) -> Option<std::sync::Arc<World>> {
    let mask = if loaded.mask == 0 { u32::MAX } else { loaded.mask };
    foot.shared_world().or_else(|| shared.ready(mask))
}

impl Plugin for FirePlugin {
    fn build(&self, app: &mut App) {
        app.init_map_resource::<Shots>()
            .init_map_resource::<Targets>()
            .init_map_resource::<PlayerBody>()
            .add_message::<Damage>()
            .remove_on_map_unload::<Looks>()
            .add_systems(halo3_core::map::MapStartup, setup)
            .add_systems(
                Update,
                (launch, fly, fade)
                    .chain()
                    .in_set(FireSet)
                    .run_if(halo3_core::pause::running)
                    .after(super::trigger)
                    .before(super::place_held).run_if(halo3_core::map::ready),
            );
    }
}

/// How a round looks and behaves.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Style {
    Bullet,
    Plasma,
    Spike,
    Laser,
    Grenade,
    Missile,
}

impl Style {
    fn of(kind: &Kind) -> Style {
        let p = kind.weapon.tag_path.as_str();
        // The sentinels' and the monitor's beams draw as the laser does.
        if p.contains("laser") || p.contains("sentinel_gun") || p.contains("monitor_beam") {
            Style::Laser
        } else if p.contains("plasma") || p.contains("needler") {
            Style::Plasma
        } else if p.contains("spike") {
            Style::Spike
        } else if p.contains("brute_shot") {
            Style::Grenade
        } else if p.contains("missile") || p.contains("rocket") {
            Style::Missile
        } else {
            Style::Bullet
        }
    }

    /// Glow colour (linear, bright).
    fn colour(self, power: f32) -> LinearRgba {
        match self {
            Style::Bullet => LinearRgba::rgb(4.0, 3.2, 1.4),
            Style::Plasma if power > 0.5 => LinearRgba::rgb(1.5, 6.0, 1.5),
            Style::Plasma => LinearRgba::rgb(1.2, 4.0, 1.4),
            Style::Spike => LinearRgba::rgb(5.0, 2.0, 0.6),
            Style::Laser => LinearRgba::rgb(8.0, 0.8, 0.6),
            Style::Grenade => LinearRgba::rgb(4.0, 1.6, 0.4),
            Style::Missile => LinearRgba::rgb(5.0, 3.0, 1.0),
        }
    }

    /// Tracer length and thickness, metres.
    fn streak(self, power: f32) -> (f32, f32) {
        match self {
            Style::Bullet => (4.0, 0.03),
            Style::Plasma => (0.35, 0.07 + 0.12 * power),
            Style::Spike => (0.5, 0.03),
            Style::Laser => (0.0, 0.09),
            Style::Grenade => (0.15, 0.09),
            Style::Missile => (0.6, 0.08),
        }
    }

    fn falls(self) -> bool {
        self == Style::Grenade
    }
}

/// A shot waiting for the world: from `trigger` to `launch`.
struct Pending {
    kind_tag: u32,
    style: Style,
    /// Grip space to the renderer frame, and grip space to the muzzle.
    muzzle: Mat4,
    view: Transform,
    spread: f32,
    speed: [f32; 2],
    range: f32,
    pellets: u32,
    explosion: f32,
    power: f32,
    /// An actor's shot: where it leaves and which way (renderer frame);
    /// `None` for the player's, which go where the crosshair points.
    aimed: Option<(Vec3, Vec3)>,
    /// The actor that fired it (`None`: the player).
    owner: Option<Entity>,
    /// Damage per hit (min, max) from the projectile's damage effect.
    damage: [f32; 2],
    /// The enemy Spartan's damage per round (the projectile's; the laser
    /// kills), and the weapon's tag path for the kill feed (`enemy.rs`).
    spartan_damage: f32,
    path: String,
}

struct Round {
    weapon: u32,
    style: Style,
    position: Vec3,
    velocity: Vec3,
    speed: [f32; 2],
    range: f32,
    travelled: f32,
    explosion: f32,
    power: f32,
    spartan_damage: f32,
    path: String,
    tracer: Entity,
    owner: Option<Entity>,
    damage: [f32; 2],
}

/// What a round leaves for a moment: a flash, a spark, a mark, a beam.
#[derive(Component)]
struct Fading {
    life: f32,
    age: f32,
    /// Moving (sparks) and growing (explosions).
    velocity: Vec3,
    grow: f32,
    shrink: bool,
}

#[derive(Resource, Default)]
pub struct Shots {
    pending: Vec<Pending>,
    rounds: Vec<Round>,
    /// The pad trigger held last frame (for a pull).
    trigger_was: bool,
    rng: u32,
    /// Rounds launched and hits, for the screenshot-mode log.
    pub launched: u32,
    pub hits: u32,
    marks: std::collections::VecDeque<Entity>,
}

/// Scorch marks kept at most; older ones go.
const MARKS: usize = 80;

impl Shots {
    /// Whether the trigger was pulled this frame (it was up, now down).
    pub fn auto_pull(&mut self, down: bool) -> bool {
        let pulled = down && !self.trigger_was;
        self.trigger_was = down;
        pulled
    }

    /// Queues a shot: `weapon_world` is the weapon's grip space in the
    /// renderer frame, `view` the camera, `spread` the error angle
    /// (radians), `power` 1 for a charged shot.
    pub fn shoot(&mut self, kind: &Kind, barrel: &Barrel, weapon_world: Mat4, view: Transform, spread: f32, power: f32) {
        self.queue(kind, barrel, weapon_world, view, spread, power, None, None);
    }

    /// Queues an actor's shot from `from` towards `dir` (renderer frame),
    /// spread by `spread` radians. Its rounds hit the player (on foot or
    /// riding) and other actors, never `owner`.
    pub fn shoot_from(&mut self, kind: &Kind, barrel: &Barrel, from: Vec3, dir: Vec3, spread: f32, owner: Entity) {
        let dir = dir.normalize_or(Vec3::NEG_Z);
        let view = Transform::from_translation(from).looking_to(dir, Vec3::Y);
        self.queue(kind, barrel, Mat4::from_translation(from), view, spread, 0.0, Some((from, dir)), Some(owner));
    }

    #[allow(clippy::too_many_arguments)]
    fn queue(
        &mut self,
        kind: &Kind,
        barrel: &Barrel,
        weapon_world: Mat4,
        view: Transform,
        spread: f32,
        power: f32,
        aimed: Option<(Vec3, Vec3)>,
        owner: Option<Entity>,
    ) {
        let style = Style::of(kind);
        let projectile = barrel.projectile.as_ref();
        let speed = projectile.map_or([300.0, 300.0], |p| {
            let a = p.initial_velocity.max(1.0);
            let b = if p.final_velocity > 0.0 { p.final_velocity } else { a };
            [a, b]
        });
        let range = projectile.map_or(100.0, |p| if p.maximum_range > 1.0 { p.maximum_range } else { 100.0 });
        let explosion = projectile
            .and_then(|p| p.damage.as_ref())
            .map_or(0.0, |d| d.radius[1])
            .max(if matches!(style, Style::Grenade | Style::Missile) { 1.0 } else { 0.0 });
        // The tag's damage; beams (the Spartan laser) carry none: it kills.
        let spartan_damage = projectile
            .and_then(|p| p.damage.as_ref())
            .map_or(0.0, |d| d.upper_bound[0].max(d.upper_bound[1]));
        let spartan_damage = if spartan_damage > 0.0 { spartan_damage } else if style == Style::Laser { 500.0 } else { 10.0 };
        // The shotgun: Halo 3 throws fifteen pellets (not read from the tag).
        let pellets = if kind.weapon.tag_path.contains("shotgun") { 15 } else { 1 };
        // What a round takes off: the damage effect's upper bound (min,
        // max). Explosives without one (rockets, grenades) deal theirs by
        // area where they go off (`EXPLOSION_DAMAGE`).
        let damage = projectile.and_then(|p| p.damage.as_ref()).map_or([0.0, 0.0], |d| {
            let lo = d.upper_bound[0].max(d.lower_bound);
            [lo, d.upper_bound[1].max(lo)]
        });
        self.pending.push(Pending {
            kind_tag: kind.weapon.tag_index,
            style,
            muzzle: weapon_world * kind.muzzle,
            view,
            spread,
            speed: speed.map(|s| s * WU),
            range: range * WU,
            pellets,
            explosion: explosion * WU,
            power,
            aimed,
            owner,
            damage,
            spartan_damage,
            path: kind.weapon.tag_path.clone(),
        });
    }

    /// A damage number in a (min, max) range.
    fn roll(&mut self, range: [f32; 2]) -> f32 {
        range[0] + (range[1] - range[0]) * self.random()
    }

    fn random(&mut self) -> f32 {
        // xorshift32
        let mut x = if self.rng == 0 { 0x9E37_79B9 } else { self.rng };
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32) / (u32::MAX as f32)
    }

    /// A direction within `angle` of `dir`, even over the cone's face.
    fn spread(&mut self, dir: Vec3, angle: f32) -> Vec3 {
        if angle <= 0.0 {
            return dir;
        }
        let r = angle * self.random().sqrt();
        let a = self.random() * std::f32::consts::TAU;
        let u = dir.any_orthonormal_vector();
        let v = dir.cross(u);
        (dir + (u * a.cos() + v * a.sin()) * r.tan()).normalize()
    }
}

#[derive(Resource)]
struct Looks {
    stick: Handle<Mesh>,
    ball: Handle<Mesh>,
    disc: Handle<Mesh>,
    mark: Handle<StandardMaterial>,
    spark: Handle<StandardMaterial>,
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(Looks {
        stick: meshes.add(Cylinder::new(1.0, 1.0)),
        ball: meshes.add(Sphere::new(1.0).mesh().ico(1).expect("ico sphere")),
        disc: meshes.add(Circle::new(1.0)),
        mark: materials.add(StandardMaterial {
            base_color: Color::srgba(0.03, 0.03, 0.03, 0.85),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            depth_bias: 50.0,
            ..default()
        }),
        spark: materials.add(StandardMaterial {
            base_color: LinearRgba::rgb(6.0, 4.0, 1.5).into(),
            unlit: true,
            ..default()
        }),
    });
}

fn glow(materials: &mut Assets<StandardMaterial>, colour: LinearRgba) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: colour.into(),
        unlit: true,
        ..default()
    })
}

/// A stick from `a` to `b`, `thick` metres across.
fn stick(a: Vec3, b: Vec3, thick: f32) -> Transform {
    let d = b - a;
    let len = d.length().max(1e-3);
    Transform::from_translation((a + b) * 0.5)
        .with_rotation(Quat::from_rotation_arc(Vec3::Y, d / len))
        .with_scale(Vec3::new(thick, len, thick))
}

/// Where the crosshair points: the first thing along the camera's ray
/// beyond the Spartan (the enemy included), or far off.
fn aim_point(world: &World, enemy: &Enemy, view: &Transform, eye: Vec3) -> Vec3 {
    let f = view.forward().as_vec3();
    // Skip the stretch behind him (third person).
    let start = view.translation + f * (eye - view.translation).dot(f).max(0.0);
    let far = world.raycast_far(start, f, 600.0).map_or(600.0, |h| h.distance);
    let far = enemy.raycast(start, f, far).map_or(far, |h| h.distance);
    start + f * far
}

/// A round of `damage` from `path` (`weapon` its tag) hitting him; tells
/// the kill feed if it killed him.
fn hurt(enemy: &mut Enemy, kills: &mut MessageWriter<EnemyKilled>, weapon: u32, path: &str, damage: f32, head: bool) {
    if enemy.damage(damage, head, path) {
        kills.write(EnemyKilled { weapon, headshot: head });
    }
}

/// An explosion's share of its damage on him.
fn blast(enemy: &mut Enemy, kills: &mut MessageWriter<EnemyKilled>, weapon: u32, path: &str, damage: f32, at: Vec3, radius: f32) {
    let share = enemy.splash(at, radius);
    if share > 0.0 {
        hurt(enemy, kills, weapon, path, damage * share, false);
    }
}

/// What a round's path from `from` along `dir` (unit) meets first within
/// `max`: an actor (not `owner`), or for an actor's round the Spartan.
fn body_hit(
    targets: Option<&Targets>,
    player: Capsule,
    owner: Option<Entity>,
    from: Vec3,
    dir: Vec3,
    max: f32,
) -> Option<(DamageTarget, f32, bool)> {
    let actor = targets
        .and_then(|t| t.raycast(from, dir, max, owner))
        .map(|h| (DamageTarget::Actor(h.entity), h.distance, h.head));
    let player = owner.zip(player).and_then(|(_, (base, top, r))| {
        ray_capsule(from, dir, base, top, r)
            .filter(|d| *d <= max)
            .map(|d| (DamageTarget::Player, d, false))
    });
    match (actor, player) {
        (Some(a), Some(p)) => Some(if p.1 < a.1 { p } else { a }),
        (a, p) => a.or(p),
    }
}

/// Damage at the centre of an explosion with no damage effect of its own
/// (rockets, brute shot grenades), falling off to nothing at twice the
/// round's explosion radius. Our own number: the area damage is in the
/// detonation effect, which is not read.
const EXPLOSION_DAMAGE: f32 = 120.0;

/// Area damage round `at` (renderer frame) out to twice `radius`.
fn explode(
    damage: &mut MessageWriter<Damage>,
    targets: Option<&Targets>,
    player: Capsule,
    owner: Option<Entity>,
    at: Vec3,
    radius: f32,
) {
    let reach = radius * 2.0;
    let source = owner.map_or(DamageSource::Player, DamageSource::Actor);
    let mut hurt = |target: DamageTarget, centre: Vec3| {
        let d = centre.distance(at);
        if d < reach {
            damage.write(Damage {
                target,
                source,
                amount: EXPLOSION_DAMAGE * (1.0 - d / reach),
                at: centre,
                direction: (centre - at).normalize_or(Vec3::Y),
                head: false,
                melee: false,
            });
        }
    };
    for t in targets.map_or(&[][..], |t| &t.0[..]) {
        hurt(DamageTarget::Actor(t.entity), (t.base + t.top) * 0.5);
    }
    if let Some((base, top, _)) = player {
        hurt(DamageTarget::Player, (base + top) * 0.5);
    }
}

#[allow(clippy::too_many_arguments)]
fn launch(
    mut commands: Commands,
    mut shots: ResMut<Shots>,
    (foot, shared, loaded, body): (Res<OnFoot>, Res<SharedWorld>, Res<LoadedBsps>, Res<PlayerBody>),
    looks: Option<Res<Looks>>,
    targets: Option<Res<Targets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut impacts: MessageWriter<WeaponImpact>,
    mut damage: MessageWriter<Damage>,
    (mut enemy, mut kills): (ResMut<Enemy>, MessageWriter<EnemyKilled>),
) {
    let (Some(looks), Some(world)) = (looks, world_of(&foot, &shared, &loaded)) else {
        shots.pending.clear();
        return;
    };
    let world = &*world;
    let player = body.0.as_ref().map(|b| b.capsule());
    let eye = foot.spartan.as_ref().map(|s| s.eye());
    let pending = std::mem::take(&mut shots.pending);
    for p in pending {
        let (from, dir) = match (p.aimed, eye) {
            (Some(aimed), _) => aimed,
            // The player's own shots need him on foot.
            (None, None) => continue,
            (None, Some(eye)) => {
                let target = aim_point(world, &enemy, &p.view, eye);
                let mut from = p.muzzle.w_axis.truncate();
                // A muzzle poking through a wall fires from the eye instead.
                let to_muzzle = from - eye;
                if let Some(hit) = world.raycast_far(eye, to_muzzle.normalize_or(Vec3::Y), to_muzzle.length()) {
                    from = eye + to_muzzle.normalize_or(Vec3::Y) * (hit.distance - 0.05).max(0.0);
                }
                let ahead = (target - from).normalize_or(p.view.forward().as_vec3());
                let dir = if ahead.dot(p.view.forward().as_vec3()) > 0.2 { ahead } else { p.view.forward().as_vec3() };
                (from, dir)
            }
        };
        let colour = p.style.colour(p.power);
        let material = glow(&mut materials, colour);
        // Muzzle flash.
        let flash = if p.style == Style::Laser { 0.25 } else { 0.11 + 0.05 * p.power };
        commands.spawn((
            Mesh3d(looks.ball.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(from + dir * 0.05).with_scale(Vec3::splat(flash)),
            Fading { life: 0.05, age: 0.0, velocity: Vec3::ZERO, grow: 0.0, shrink: true },
        ));
        if p.style == Style::Laser {
            // A beam, there at once.
            let dir = shots.spread(dir, p.spread);
            let wall = world.raycast_far(from, dir, p.range);
            // The enemy Spartan stands in the way of the player's beams.
            let spartan_hit = if p.owner.is_none() { enemy.raycast(from, dir, wall.map_or(p.range, |h| h.distance)) } else { None };
            let reach = spartan_hit.as_ref().map_or(wall.map_or(p.range, |h| h.distance), |b| b.distance);
            let end = match (body_hit(targets.as_deref(), player, p.owner, from, dir, reach), wall) {
                (None, _) if let Some(b) = spartan_hit => {
                    impact(&mut commands, &looks, &mut shots, &material, b.point, b.normal, 0.0, true, false);
                    hurt(&mut enemy, &mut kills, p.kind_tag, &p.path, p.spartan_damage, b.head);
                    b.point
                }
                (Some((who, d, head)), _) => {
                    let at = from + dir * d;
                    let amount = shots.roll(p.damage);
                    damage.write(Damage {
                        target: who,
                        source: p.owner.map_or(DamageSource::Player, DamageSource::Actor),
                        amount,
                        at,
                        direction: dir,
                        head,
                        melee: false,
                    });
                    impact(&mut commands, &looks, &mut shots, &material, at, -dir, 0.0, true, false);
                    at
                }
                (None, Some(hit)) => {
                    impact(&mut commands, &looks, &mut shots, &material, hit.point, hit.normal, 0.0, true, true);
                    impacts.write(WeaponImpact { weapon: p.kind_tag, at: hit.point, normal: hit.normal });
                    hit.point
                }
                (None, None) => from + dir * p.range,
            };
            commands.spawn((
                Mesh3d(looks.stick.clone()),
                MeshMaterial3d(material),
                stick(from, end, p.style.streak(1.0).1),
                Fading { life: 0.35, age: 0.0, velocity: Vec3::ZERO, grow: 0.0, shrink: true },
            ));
            continue;
        }
        shots.launched += p.pellets;
        for _ in 0..p.pellets {
            let spread = if p.pellets > 1 { p.spread.max(0.06) } else { p.spread };
            let d = shots.spread(dir, spread);
            let (_, thick) = p.style.streak(p.power);
            let tracer = commands
                .spawn((
                    Mesh3d(if matches!(p.style, Style::Plasma | Style::Grenade) { looks.ball.clone() } else { looks.stick.clone() }),
                    MeshMaterial3d(material.clone()),
                    stick(from, from + d * 0.01, thick),
                ))
                .id();
            shots.rounds.push(Round {
                weapon: p.kind_tag,
                style: p.style,
                position: from,
                velocity: d * p.speed[0],
                speed: p.speed,
                range: p.range,
                travelled: 0.0,
                explosion: p.explosion,
                power: p.power,
                spartan_damage: p.spartan_damage,
                path: p.path.clone(),
                tracer,
                owner: p.owner,
                damage: p.damage,
            });
        }
    }
}

/// Sparks, a scorch mark and, for explosives, a fireball.
#[allow(clippy::too_many_arguments)]
fn impact(
    commands: &mut Commands,
    looks: &Looks,
    shots: &mut Shots,
    glow: &Handle<StandardMaterial>,
    at: Vec3,
    normal: Vec3,
    explosion: f32,
    big: bool,
    // A scorch mark: on walls, not on bodies (it would hang in the air
    // where the actor or the rider was).
    mark: bool,
) {
    let n = if big { 10 } else { 5 };
    for _ in 0..n {
        let d = shots.spread(normal, 1.2);
        let speed = 2.0 + 4.0 * shots.random();
        commands.spawn((
            Mesh3d(looks.ball.clone()),
            MeshMaterial3d(looks.spark.clone()),
            Transform::from_translation(at + normal * 0.03).with_scale(Vec3::splat(0.015)),
            Fading { life: 0.25 + 0.2 * shots.random(), age: 0.0, velocity: d * speed, grow: 0.0, shrink: true },
        ));
    }
    let size = if explosion > 0.0 { (explosion * 0.4).min(1.2) } else if big { 0.12 } else { 0.06 };
    if mark {
        let mark = commands
            .spawn((
                Mesh3d(looks.disc.clone()),
                MeshMaterial3d(looks.mark.clone()),
                Transform::from_translation(at + normal * 0.01)
                    .with_rotation(Quat::from_rotation_arc(Vec3::Z, normal))
                    .with_scale(Vec3::splat(size)),
                Fading { life: 20.0, age: 0.0, velocity: Vec3::ZERO, grow: 0.0, shrink: false },
            ))
            .id();
        shots.marks.push_back(mark);
        while shots.marks.len() > MARKS {
            if let Some(old) = shots.marks.pop_front() {
                commands.entity(old).try_despawn();
            }
        }
    }
    if explosion > 0.0 {
        commands.spawn((
            Mesh3d(looks.ball.clone()),
            MeshMaterial3d(glow.clone()),
            Transform::from_translation(at + normal * 0.2).with_scale(Vec3::splat(0.2)),
            Fading { life: 0.3, age: 0.0, velocity: Vec3::ZERO, grow: explosion * 3.0, shrink: false },
        ));
    } else {
        commands.spawn((
            Mesh3d(looks.ball.clone()),
            MeshMaterial3d(glow.clone()),
            Transform::from_translation(at + normal * 0.03).with_scale(Vec3::splat(if big { 0.1 } else { 0.05 })),
            Fading { life: 0.06, age: 0.0, velocity: Vec3::ZERO, grow: 0.0, shrink: true },
        ));
    }
}

/// Gravity on grenades, metres per second squared.
const GRAVITY: f32 = blam_geometry::movement::GRAVITY * WU;

#[allow(clippy::too_many_arguments)]
fn fly(
    mut commands: Commands,
    mut shots: ResMut<Shots>,
    (foot, shared, loaded, body): (Res<OnFoot>, Res<SharedWorld>, Res<LoadedBsps>, Res<PlayerBody>),
    looks: Option<Res<Looks>>,
    targets: Option<Res<Targets>>,
    time: Res<Time>,
    materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut transforms: Query<&mut Transform>,
    mut impacts: MessageWriter<WeaponImpact>,
    mut damage: MessageWriter<Damage>,
    (mut enemy, mut kills): (ResMut<Enemy>, MessageWriter<EnemyKilled>),
) {
    let (Some(looks), Some(world)) = (looks, world_of(&foot, &shared, &loaded)) else {
        return;
    };
    let world = &*world;
    let player = body.0.as_ref().map(|b| b.capsule());
    let dt = time.delta_secs();
    let mut rounds = std::mem::take(&mut shots.rounds);
    rounds.retain_mut(|r| {
        // Speed eases from initial to final over the range.
        let t = (r.travelled / r.range.max(1.0)).clamp(0.0, 1.0);
        let speed = r.speed[0] + (r.speed[1] - r.speed[0]) * t;
        let dir = r.velocity.normalize_or(Vec3::Y);
        r.velocity = if r.style.falls() { r.velocity - Vec3::Y * GRAVITY * dt } else { dir * speed };
        let step = r.velocity * dt;
        let len = step.length();
        let from = r.position;
        let unit = step / len.max(1e-6);
        let hit = world.raycast_far(from, unit, len);
        // The enemy Spartan, if he stands nearer than the wall (the
        // player's rounds only).
        let body = if r.owner.is_none() { enemy.raycast(from, unit, hit.map_or(len, |h| h.distance)) } else { None };
        // An actor before the wall (and the Spartan) takes the round.
        let reach = body.as_ref().map_or(hit.map_or(len, |h| h.distance), |b| b.distance);
        if let Some((who, d, head)) = body_hit(targets.as_deref(), player, r.owner, from, unit, reach) {
            let at = from + unit * d;
            let amount = shots.roll(r.damage);
            if amount > 0.0 {
                damage.write(Damage {
                    target: who,
                    source: r.owner.map_or(DamageSource::Player, DamageSource::Actor),
                    amount,
                    at,
                    direction: unit,
                    head,
                    melee: false,
                });
            }
            shots.hits += 1;
            let glow = materials.get(r.tracer).map(|m| m.0.clone()).unwrap_or_else(|_| looks.spark.clone());
            impact(&mut commands, &looks, &mut shots, &glow, at, -unit, r.explosion, r.power > 0.5, false);
            if r.explosion > 0.0 {
                explode(&mut damage, targets.as_deref(), player, r.owner, at, r.explosion);
            }
            commands.entity(r.tracer).try_despawn();
            return false;
        }
        let hit = match body {
            Some(b) => {
                hurt(&mut enemy, &mut kills, r.weapon, &r.path, r.spartan_damage, b.head);
                Some(halo3_bike::physics::Hit { distance: b.distance, point: b.point, normal: b.normal })
            }
            None => hit,
        };
        let end = hit.map_or(from + step, |h| h.point);
        r.travelled += from.distance(end);
        r.position = end;
        let (streak, thick) = r.style.streak(r.power);
        if let Ok(mut t) = transforms.get_mut(r.tracer) {
            *t = if matches!(r.style, Style::Plasma | Style::Grenade) {
                Transform::from_translation(end).with_scale(Vec3::splat(thick))
            } else {
                // The streak trails the round, no longer than its path.
                let back = streak.min(r.travelled).max(0.05);
                stick(end - dir * back, end, thick)
            };
        }
        let gone = hit.is_some() || r.travelled >= r.range;
        if let Some(h) = hit {
            shots.hits += 1;
            let glow = materials.get(r.tracer).map(|m| m.0.clone()).unwrap_or_else(|_| looks.spark.clone());
            impact(&mut commands, &looks, &mut shots, &glow, h.point, h.normal, r.explosion, r.power > 0.5, true);
            impacts.write(WeaponImpact { weapon: r.weapon, at: h.point, normal: h.normal });
            if r.explosion > 0.0 {
                explode(&mut damage, targets.as_deref(), player, r.owner, h.point, r.explosion);
            }
            // A direct hit on the enemy Spartan took its damage above.
            if body.is_none() && r.owner.is_none() {
                blast(&mut enemy, &mut kills, r.weapon, &r.path, r.spartan_damage, h.point, r.explosion);
            }
        } else if gone && r.explosion > 0.0 {
            // Grenades and missiles go off at the end of their range.
            let glow = materials.get(r.tracer).map(|m| m.0.clone()).unwrap_or_else(|_| looks.spark.clone());
            impact(&mut commands, &looks, &mut shots, &glow, end, Vec3::Y, r.explosion, true, true);
            explode(&mut damage, targets.as_deref(), player, r.owner, end, r.explosion);
            if r.owner.is_none() {
                blast(&mut enemy, &mut kills, r.weapon, &r.path, r.spartan_damage, end, r.explosion);
            }
        }
        if gone {
            commands.entity(r.tracer).try_despawn();
        }
        !gone
    });
    shots.rounds = rounds;
}

fn fade(
    mut commands: Commands,
    time: Res<Time>,
    mut fading: Query<(Entity, &mut Fading, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut f, mut t) in &mut fading {
        f.age += dt;
        if f.age >= f.life {
            commands.entity(e).try_despawn();
            continue;
        }
        if f.velocity != Vec3::ZERO {
            f.velocity.y -= 9.8 * dt;
            t.translation += f.velocity * dt;
        }
        if f.grow > 0.0 {
            t.scale += Vec3::splat(f.grow * dt);
        }
        if f.shrink {
            let k = 1.0 - (dt / (f.life - f.age + dt)).min(1.0) * 0.5;
            t.scale *= k;
        }
    }
}
