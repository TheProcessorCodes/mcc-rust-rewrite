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

use bevy::prelude::*;

use blam_geometry::weapons::Barrel;

use super::assets::Kind;
use super::{WU, WeaponImpact};
use halo3_bike::physics::World;
use halo3_foot::OnFoot;

pub struct FirePlugin;

impl Plugin for FirePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Shots>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (launch, fly, fade)
                    .chain()
                    .run_if(halo3_core::pause::running)
                    .after(super::trigger)
                    .before(super::place_held),
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
        if p.contains("laser") {
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
    tracer: Entity,
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
        // The shotgun: Halo 3 throws fifteen pellets (not read from the tag).
        let pellets = if kind.weapon.tag_path.contains("shotgun") { 15 } else { 1 };
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
        });
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
/// beyond the Spartan, or far off.
fn aim_point(world: &World, view: &Transform, eye: Vec3) -> Vec3 {
    let f = view.forward().as_vec3();
    // Skip the stretch behind him (third person).
    let start = view.translation + f * (eye - view.translation).dot(f).max(0.0);
    match world.raycast_far(start, f, 600.0) {
        Some(hit) => hit.point,
        None => start + f * 600.0,
    }
}

#[allow(clippy::too_many_arguments)]
fn launch(
    mut commands: Commands,
    mut shots: ResMut<Shots>,
    foot: Res<OnFoot>,
    looks: Option<Res<Looks>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut impacts: MessageWriter<WeaponImpact>,
) {
    let (Some(looks), Some(world), Some(spartan)) = (looks, foot.world(), foot.spartan.as_ref()) else {
        shots.pending.clear();
        return;
    };
    let eye = spartan.eye();
    let pending = std::mem::take(&mut shots.pending);
    for p in pending {
        let target = aim_point(world, &p.view, eye);
        let mut from = p.muzzle.w_axis.truncate();
        // A muzzle poking through a wall fires from the eye instead.
        let to_muzzle = from - eye;
        if let Some(hit) = world.raycast_far(eye, to_muzzle.normalize_or(Vec3::Y), to_muzzle.length()) {
            from = eye + to_muzzle.normalize_or(Vec3::Y) * (hit.distance - 0.05).max(0.0);
        }
        let ahead = (target - from).normalize_or(p.view.forward().as_vec3());
        let dir = if ahead.dot(p.view.forward().as_vec3()) > 0.2 { ahead } else { p.view.forward().as_vec3() };
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
            let end = match world.raycast_far(from, dir, p.range) {
                Some(hit) => {
                    impact(&mut commands, &looks, &mut shots, &material, hit.point, hit.normal, 0.0, true);
                    impacts.write(WeaponImpact { weapon: p.kind_tag, at: hit.point, normal: hit.normal });
                    hit.point
                }
                None => from + dir * p.range,
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
                tracer,
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

fn fly(
    mut commands: Commands,
    mut shots: ResMut<Shots>,
    foot: Res<OnFoot>,
    looks: Option<Res<Looks>>,
    time: Res<Time>,
    materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut transforms: Query<&mut Transform>,
    mut impacts: MessageWriter<WeaponImpact>,
) {
    let (Some(looks), Some(world)) = (looks, foot.world()) else {
        return;
    };
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
        let hit = world.raycast_far(from, step / len.max(1e-6), len);
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
            impact(&mut commands, &looks, &mut shots, &glow, h.point, h.normal, r.explosion, r.power > 0.5);
            impacts.write(WeaponImpact { weapon: r.weapon, at: h.point, normal: h.normal });
        } else if gone && r.explosion > 0.0 {
            // Grenades and missiles go off at the end of their range.
            let glow = materials.get(r.tracer).map(|m| m.0.clone()).unwrap_or_else(|_| looks.spark.clone());
            impact(&mut commands, &looks, &mut shots, &glow, end, Vec3::Y, r.explosion, true);
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
