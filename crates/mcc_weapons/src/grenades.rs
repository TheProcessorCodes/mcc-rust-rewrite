//! Hand grenades: frag and plasma, thrown with G (LT on a pad), the type
//! chosen with X (LB). The Spartan starts with two frags and carries at
//! most two of each, as in Halo 3; walking over a grenade the map places
//! (its scenario equipment) picks it up, and it comes back after the
//! placement's respawn time.
//!
//! A frag bounces and goes off a moment after it first lands; a plasma
//! grenade sticks to the first thing it hits, an actor included, and goes
//! off a little later. Either goes off in the end if it never lands. The
//! blast is the weapons' (`fire::explode`): it hurts actors, the enemy
//! Spartan and the player alike.
//!
//! Halo keeps the throw speed, the fuses and the blast in the grenade's
//! projectile and the globals, which are not read: the numbers here are
//! ours. The models are the map's own grenade equipment
//! (`assets::GRENADES`); without them a small glowing ball stands in.

use bevy::input::gamepad::GamepadButton as B;
use bevy::prelude::*;

use blam_geometry::weapons::ItemPlacement;
use mcc_core::ai::{Damage, PlayerBody, Targets};
use mcc_core::coords::to_render;
use mcc_core::zones::LoadedBsps;
use mcc_bike::SharedWorld;
use mcc_foot::OnFoot;

use super::assets::{LookMaterial, Look};
use super::enemy::{Enemy, EnemyKilled};
use super::fire::{self, Looks, Shots};
use super::{Inventory, REACH, halo_to_render, spawn_copy};

/// Frag, plasma.
pub const NAMES: [&str; 2] = ["Frag", "Plasma"];
/// What the Spartan starts with: two frags, as Halo 3's default start.
pub const START: [u8; 2] = [2, 0];
/// The most he carries of each, as in Halo 3.
pub const CARRY: u8 = 2;
/// Kill feed weapon numbers for the grenades (no weapon tag of their own
/// in the arsenal).
pub const KILL: [u32; 2] = [u32::MAX - 1, u32::MAX - 2];

/// Throw speed (m/s) and the lift added to it.
const THROW_SPEED: f32 = 17.0;
const THROW_LIFT: f32 = 3.0;
/// Seconds from a frag's first landing, and from a plasma grenade sticking,
/// to the blast; and the longest any flies.
const FRAG_FUSE: f32 = 1.3;
const PLASMA_FUSE: f32 = 2.5;
const LONGEST: f32 = 6.0;
/// A frag's bounce: the share of speed kept off a surface, and along it.
const BOUNCE: f32 = 0.35;
const SLIDE: f32 = 0.7;
/// The blast's radius (metres; it reaches twice as far, fading).
const BLAST: f32 = 2.2;
/// Seconds the throw keeps the weapon from firing.
const THROW_TIME: f32 = 0.5;

/// The grenades' models and the map's grenade placements, from `setup`.
#[derive(Resource)]
pub(crate) struct Kit {
    pub looks: [Option<Look>; 2],
    /// Model space to grip space, per type.
    pub to_grip: [Mat4; 2],
    /// Type, placement and when a taken one comes back (game seconds; 0:
    /// lying there, or never).
    pub placed: Vec<(usize, ItemPlacement, f64)>,
}

/// A grenade lying where the map put it.
struct Lying {
    kind: usize,
    entity: Entity,
    placement: usize,
    lit: bool,
    materials: Vec<(Handle<StandardMaterial>, LookMaterial)>,
}

/// What a plasma grenade stuck to.
#[derive(Clone, Copy)]
enum Stuck {
    World,
    /// An actor, and where on it (from its capsule's base).
    Actor(Entity, Vec3),
}

/// A grenade in flight or about to go off.
struct Flying {
    kind: usize,
    entity: Entity,
    position: Vec3,
    velocity: Vec3,
    spin: Quat,
    age: f32,
    /// Seconds left once the fuse is lit.
    fuse: Option<f32>,
    stuck: Option<Stuck>,
}

#[derive(Resource, Default)]
pub(crate) struct Grenades {
    lying: Vec<Lying>,
    flying: Vec<Flying>,
    /// The stand-in look when a model is missing.
    ball: Option<[Handle<StandardMaterial>; 2]>,
    /// The map's grenades have been put out.
    placed: bool,
}

/// Where a grenade in flight is drawn: its model space (Halo's) to the
/// renderer frame, at `at` turned by `spin`.
fn flying_model(at: Vec3, spin: Quat) -> Mat4 {
    Mat4::from_rotation_translation(spin, at) * halo_to_render()
}

/// The model of grenade type `kind` placed by `model` (its model space to
/// the renderer frame), or the stand-in ball at its origin.
fn spawn_grenade(
    commands: &mut Commands,
    kit: &Kit,
    grenades: &mut Grenades,
    looks: &Looks,
    materials: &mut Assets<StandardMaterial>,
    kind: usize,
    model: Mat4,
) -> (Entity, Vec<(Handle<StandardMaterial>, LookMaterial)>) {
    let at = model.w_axis.truncate();
    match &kit.looks[kind] {
        Some(look) => spawn_copy(commands, look, materials, model * kit.to_grip[kind], Vec3::ONE),
        None => {
            let glow = grenades.ball.get_or_insert_with(|| {
                [
                    fire::glow(materials, LinearRgba::rgb(0.6, 0.9, 0.3)),
                    fire::glow(materials, LinearRgba::rgb(0.6, 1.6, 4.0)),
                ]
            });
            let e = commands
                .spawn((
                    Mesh3d(looks.ball.clone()),
                    MeshMaterial3d(glow[kind].clone()),
                    Transform::from_translation(at).with_scale(Vec3::splat(0.06)),
                ))
                .id();
            (e, Vec::new())
        }
    }
}

/// Puts the map's grenades out, and back once their time comes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn place(
    mut commands: Commands,
    inv: Res<Inventory>,
    kit: Option<ResMut<Kit>>,
    mut grenades: ResMut<Grenades>,
    looks: Option<Res<Looks>>,
    lights: Option<Res<mcc_world::lighting::SceneLights>>,
    loaded: Option<Res<LoadedBsps>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (Some(mut kit), Some(looks)) = (kit, looks) else {
        return;
    };
    let first = !grenades.placed;
    grenades.placed = true;
    for n in 0..kit.placed.len() {
        let (kind, model, when) = {
            let (kind, placement, when) = &kit.placed[n];
            (*kind, halo_to_render() * placement.transform(), *when)
        };
        let back = when > 0.0 && inv.clock >= when;
        if !first && !back {
            continue;
        }
        kit.placed[n].2 = 0.0;
        let (entity, mats) = spawn_grenade(&mut commands, &kit, &mut grenades, &looks, &mut materials, kind, model);
        grenades.lying.push(Lying {
            kind,
            entity,
            placement: n,
            lit: false,
            materials: mats,
        });
    }
    // Lit where they lie once the map's light is known.
    if let Some(lights) = lights {
        let shown = loaded.as_deref().map_or(0, |b| b.shown);
        for g in grenades.lying.iter_mut().filter(|g| !g.lit) {
            g.lit = true;
            let at = kit.placed[g.placement].1.position;
            let e = mcc_rider::object_light(Some(&lights), at + Vec3::Z * 0.1, shown);
            for (h, look) in &g.materials {
                if let Some(mut m) = materials.get_mut(h) {
                    *m = look.lit(e);
                }
            }
        }
    }
}

/// Walking over a grenade takes it, while he has room for that type.
pub(crate) fn pick_up(
    mut commands: Commands,
    mut inv: ResMut<Inventory>,
    kit: Option<ResMut<Kit>>,
    mut grenades: ResMut<Grenades>,
    foot: Res<OnFoot>,
) {
    let (Some(mut kit), Some(spartan)) = (kit, foot.spartan.as_ref()) else {
        return;
    };
    let feet = spartan.position + Vec3::Y * 0.3;
    let clock = inv.clock;
    let mut i = 0;
    while i < grenades.lying.len() {
        let g = &grenades.lying[i];
        let (kind, placement) = (g.kind, g.placement);
        let near = to_render(kit.placed[placement].1.position).distance(feet) < REACH;
        if !near || inv.grenades[kind] >= CARRY {
            i += 1;
            continue;
        }
        inv.grenades[kind] += 1;
        let g = grenades.lying.swap_remove(i);
        commands.entity(g.entity).despawn();
        let respawn = kit.placed[placement].1.respawn_seconds;
        kit.placed[placement].2 = if respawn > 0 { clock + respawn as f64 } else { 0.0 };
        info!("grenades: picked up a {} grenade ({} now)", NAMES[kind].to_lowercase(), inv.grenades[kind]);
    }
}

/// G / LT throws the chosen type; X / LB chooses the other.
#[allow(clippy::too_many_arguments)]
pub(crate) fn throw(
    mut commands: Commands,
    mut inv: ResMut<Inventory>,
    kit: Option<Res<Kit>>,
    mut grenades: ResMut<Grenades>,
    looks: Option<Res<Looks>>,
    foot: Res<OnFoot>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut trigger_was: Local<bool>,
    mut auto_throw: Local<Option<f32>>,
    time: Res<Time>,
) {
    let (Some(kit), Some(looks), Some(spartan)) = (kit, looks, foot.spartan.as_ref()) else {
        return;
    };
    let pad = gamepads.iter().next();
    if keys.just_pressed(KeyCode::KeyX) || pad.is_some_and(|p| p.just_pressed(B::LeftTrigger)) {
        inv.grenade = 1 - inv.grenade;
    }
    let down = pad.is_some_and(|p| p.get(B::LeftTrigger2).unwrap_or(0.0) > 0.5);
    let pulled = down && !*trigger_was;
    *trigger_was = down;
    // `MCC_SHOT_GRENADE=1` (dev): a `--foot` shot throws one a second in.
    let auto = auto_throw.get_or_insert_with(|| {
        if std::env::var("MCC_SHOT_GRENADE").is_ok_and(|v| v == "1") { 1.0 } else { f32::INFINITY }
    });
    *auto -= time.delta_secs();
    let shot = *auto <= 0.0;
    if shot {
        *auto = f32::INFINITY;
    }
    if !(keys.just_pressed(KeyCode::KeyG) || pulled || shot) || inv.busy > 0.0 {
        return;
    }
    let kind = inv.grenade;
    if inv.grenades[kind] == 0 {
        return;
    }
    inv.grenades[kind] -= 1;
    inv.busy = THROW_TIME;
    let active = inv.active;
    if let Some(h) = inv.slots[active].as_mut() {
        h.reload = 0.0;
        h.zoom = 0;
    }
    let aim = foot.aim_rotation();
    let forward = aim * Vec3::NEG_Z;
    let right = aim * Vec3::X;
    let from = spartan.eye() + forward * 0.4 + right * 0.15 - Vec3::Y * 0.1;
    let velocity = forward * THROW_SPEED + Vec3::Y * THROW_LIFT + spartan.velocity;
    let model = flying_model(from, aim);
    let (entity, _) = spawn_grenade(&mut commands, &kit, &mut grenades, &looks, &mut materials, kind, model);
    grenades.flying.push(Flying {
        kind,
        entity,
        position: from,
        velocity,
        spin: aim,
        age: 0.0,
        fuse: None,
        stuck: None,
    });
    info!("grenades: threw a {} grenade ({} left)", NAMES[kind].to_lowercase(), inv.grenades[kind]);
}

/// Grenades in flight: falling, bouncing or sticking, and going off.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fly(
    mut commands: Commands,
    mut grenades: ResMut<Grenades>,
    kit: Option<Res<Kit>>,
    (foot, shared, loaded, body): (Res<OnFoot>, Res<SharedWorld>, Res<LoadedBsps>, Res<PlayerBody>),
    looks: Option<Res<Looks>>,
    targets: Option<Res<Targets>>,
    time: Res<Time>,
    mut transforms: Query<&mut Transform>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shots: ResMut<Shots>,
    mut damage: MessageWriter<Damage>,
    (mut enemy, mut kills): (ResMut<Enemy>, MessageWriter<EnemyKilled>),
) {
    let (Some(kit), Some(looks)) = (kit, looks) else {
        return;
    };
    if grenades.flying.is_empty() {
        return;
    }
    let Some(world) = fire::world_of(&foot, &shared, &loaded) else {
        return;
    };
    let dt = time.delta_secs();
    let player = body.0.as_ref().map(|b| b.capsule());
    let targets = targets.as_deref();
    let mut flying = std::mem::take(&mut grenades.flying);
    flying.retain_mut(|g| {
        g.age += dt;
        match g.stuck {
            Some(Stuck::Actor(e, offset)) => {
                if let Some(t) = targets.and_then(|t| t.0.iter().find(|t| t.entity == e)) {
                    g.position = t.base + offset;
                }
            }
            Some(Stuck::World) => {}
            None => step(g, &world, targets, dt),
        }
        if let Some(f) = g.fuse.as_mut() {
            *f -= dt;
        }
        let at = g.position;
        if let Ok(mut t) = transforms.get_mut(g.entity) {
            let scale = t.scale;
            *t = match &kit.looks[g.kind] {
                Some(_) => Transform::from_matrix(flying_model(at, g.spin) * kit.to_grip[g.kind]),
                None => Transform::from_translation(at).with_scale(scale),
            };
        }
        if !(g.fuse.is_some_and(|f| f <= 0.0) || g.age > LONGEST) {
            return true;
        }
        // The blast.
        let glow = fire::glow(&mut materials, if g.kind == 0 { LinearRgba::rgb(5.0, 2.4, 0.6) } else { LinearRgba::rgb(1.2, 2.6, 7.0) });
        fire::impact(&mut commands, &looks, &mut shots, &glow, at, Vec3::Y, BLAST, true, g.stuck.is_some_and(|s| matches!(s, Stuck::World)));
        fire::explode(&mut damage, targets, player, None, at, BLAST);
        fire::blast(&mut enemy, &mut kills, KILL[g.kind], "grenade", fire::EXPLOSION_DAMAGE, at, BLAST * 2.0);
        commands.entity(g.entity).try_despawn();
        false
    });
    grenades.flying = flying;
}

/// One frame of flight: gravity, then the collision and the actors along
/// the way.
fn step(g: &mut Flying, world: &mcc_bike::physics::World, targets: Option<&Targets>, dt: f32) {
    g.velocity.y -= fire::GRAVITY * dt;
    let travel = g.velocity * dt;
    let len = travel.length();
    if len < 1e-6 {
        return;
    }
    let dir = travel / len;
    let wall = world.raycast_far(g.position, dir, len);
    let actor = targets
        .and_then(|t| t.raycast(g.position, dir, wall.map_or(len, |h| h.distance), None))
        .and_then(|h| Some((h, targets?.0.iter().find(|t| t.entity == h.entity)?)));
    let plasma = g.kind == 1;
    if let Some((hit, t)) = actor {
        g.position += dir * hit.distance;
        if plasma {
            g.stuck = Some(Stuck::Actor(t.entity, g.position - t.base));
            g.fuse = Some(PLASMA_FUSE);
        } else {
            // A frag glances off a body.
            g.velocity = -dir * g.velocity.length() * BOUNCE * 0.5;
            g.fuse.get_or_insert(FRAG_FUSE);
        }
        return;
    }
    let Some(hit) = wall else {
        g.position += travel;
        // A frag spins as it flies.
        if !plasma {
            g.spin = Quat::from_axis_angle(dir.cross(Vec3::Y).normalize_or(Vec3::X), -len * 8.0) * g.spin;
        }
        return;
    };
    g.position += dir * (hit.distance - 0.03).max(0.0);
    if plasma {
        g.stuck = Some(Stuck::World);
        g.velocity = Vec3::ZERO;
        g.fuse = Some(PLASMA_FUSE);
        return;
    }
    // A frag bounces, sliding along the surface, and settles.
    let n = hit.normal;
    let into = g.velocity.dot(n);
    let along = g.velocity - n * into;
    g.velocity = along * SLIDE - n * into * BOUNCE;
    if g.velocity.length() < 0.8 && n.y > 0.5 {
        g.velocity = Vec3::ZERO;
    }
    g.fuse.get_or_insert(FRAG_FUSE);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcc_bike::physics::World;

    fn floor() -> World {
        let s = 50.0;
        World::new([
            ([Vec3::new(-s, 0.0, -s), Vec3::new(-s, 0.0, s), Vec3::new(s, 0.0, s)], false),
            ([Vec3::new(-s, 0.0, -s), Vec3::new(s, 0.0, s), Vec3::new(s, 0.0, -s)], false),
        ])
    }

    fn thrown(kind: usize) -> Flying {
        Flying {
            kind,
            entity: Entity::PLACEHOLDER,
            position: Vec3::new(0.0, 1.6, 0.0),
            velocity: Vec3::new(0.0, THROW_LIFT, -THROW_SPEED),
            spin: Quat::IDENTITY,
            age: 0.0,
            fuse: None,
            stuck: None,
        }
    }

    /// A frag lands some way off, bounces on and comes to rest on the
    /// floor with its fuse lit; a plasma grenade stops where it lands.
    #[test]
    fn frags_bounce_and_plasma_sticks() {
        let world = floor();
        let mut frag = thrown(0);
        let mut first = None;
        for i in 0..600 {
            step(&mut frag, &world, None, 1.0 / 60.0);
            if first.is_none() && frag.fuse.is_some() {
                first = Some((i, frag.position));
            }
        }
        let (_, landed) = first.expect("the frag landed");
        assert!(landed.z < -8.0, "lands well ahead: {landed}");
        assert!(frag.position.z < landed.z, "bounces on: {} then {}", landed, frag.position);
        assert!(frag.position.y.abs() < 0.1, "rests on the floor: {}", frag.position);
        assert_eq!(frag.velocity, Vec3::ZERO);

        let mut plasma = thrown(1);
        for _ in 0..600 {
            step(&mut plasma, &world, None, 1.0 / 60.0);
            if plasma.stuck.is_some() {
                break;
            }
        }
        assert!(matches!(plasma.stuck, Some(Stuck::World)));
        assert_eq!(plasma.fuse, Some(PLASMA_FUSE));
        assert!(plasma.position.y.abs() < 0.1, "{}", plasma.position);
    }
}
