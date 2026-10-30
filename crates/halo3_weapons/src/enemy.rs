//! A target to shoot: one enemy Spartan in red, with no AI. He stands
//! still where he was put, takes the rounds' damage (shields, then
//! health), falls over when killed and goes a few seconds later.
//!
//! Free roam and on foot only (not on the bike or the board):
//!
//! | controller | keyboard | |
//! |---|---|---|
//! | d-pad left | Insert | spawn him where you look (moves the one there is) |
//! | d-pad right | Delete | remove him |
//!
//! Damage is the projectile tag's (`Damage::upper_bound`) against Halo 3's
//! multiplayer Spartan: 70 shield, 45 health, both back after 5 seconds
//! untouched. A headshot from a precision weapon kills once the shield is
//! down; the sniper's kills outright. Renderer frame (metres, Y up).

use bevy::input::gamepad::GamepadButton as B;
use bevy::prelude::*;

use halo3_bike::physics::World;
use halo3_core::coords::to_halo;
use halo3_core::fly_camera::FlyCamera;
use halo3_core::skate::Skating;
use halo3_core::zones::LoadedBsps;
use halo3_foot::OnFoot;
use halo3_rider::{SpartanStatue, object_light};
use halo3_world::lighting::SceneLights;

use super::WU;

pub struct EnemyPlugin;

impl Plugin for EnemyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Enemy>()
            .add_message::<EnemyKilled>()
            .add_systems(halo3_core::map::MapStartup, setup)
            .add_systems(Update, (control, tick).chain().run_if(halo3_core::pause::running).run_if(halo3_core::map::ready));
    }
}

/// The enemy died: `weapon` is the `weap` tag that killed him.
#[derive(Message, Clone, Copy, Debug)]
pub struct EnemyKilled {
    pub weapon: u32,
    pub headshot: bool,
}

const SHIELD: f32 = 70.0;
const HEALTH: f32 = 45.0;
/// Seconds untouched before the shield and health come back, and how long
/// they take to fill.
const RECHARGE_DELAY: f32 = 5.0;
const RECHARGE_TIME: f32 = 2.0;
/// Seconds he takes to fall over, and lies there before going.
const FALL: f32 = 0.6;
const LIE: f32 = 4.0;
/// His hit capsule's radius (Halo's biped radius), metres.
const RADIUS: f32 = 0.175 * WU;
/// The part of his height above which a hit is a headshot.
const HEAD: f32 = 0.84;

/// The enemy Spartan, if there is one.
#[derive(Resource, Default)]
pub struct Enemy {
    body: Option<Body>,
    /// Collision to stand him on while flying (on foot, the Spartan's is
    /// used), and the BSPs it was built from.
    world: Option<(u32, World)>,
}

struct Body {
    root: Entity,
    /// Where his soles are, and the way he faces (flat).
    feet: Vec3,
    facing: Vec3,
    height: f32,
    shield: f32,
    health: f32,
    since_hit: f32,
    /// Seconds since he died.
    dead: Option<f32>,
}

/// A round reaching him.
#[derive(Clone, Copy, Debug)]
pub struct BodyHit {
    pub distance: f32,
    pub point: Vec3,
    pub normal: Vec3,
    pub head: bool,
}

impl Enemy {
    /// The first point along a ray (unit `dir`) where it meets him alive,
    /// within `max`.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<BodyHit> {
        let b = self.body.as_ref().filter(|b| b.dead.is_none())?;
        let (pa, pb) = (b.feet + Vec3::Y * RADIUS, b.feet + Vec3::Y * (b.height - RADIUS).max(RADIUS));
        let t = capsule(origin, dir, pa, pb, RADIUS)?;
        if t > max {
            return None;
        }
        let point = origin + dir * t;
        let axis = pa + Vec3::Y * (point.y - pa.y).clamp(0.0, pb.y - pa.y);
        Some(BodyHit {
            distance: t,
            point,
            normal: (point - axis).normalize_or(-dir),
            head: point.y - b.feet.y > b.height * HEAD,
        })
    }

    /// Where an explosion at `at` with `radius` reaches him: the share of
    /// its damage, 1 at the centre to 0 at the edge.
    pub fn splash(&self, at: Vec3, radius: f32) -> f32 {
        let Some(b) = self.body.as_ref().filter(|b| b.dead.is_none() && radius > 0.0) else {
            return 0.0;
        };
        let pa = b.feet + Vec3::Y * RADIUS;
        let pb = b.feet + Vec3::Y * (b.height - RADIUS).max(RADIUS);
        let nearest = pa + Vec3::Y * (at.y - pa.y).clamp(0.0, pb.y - pa.y);
        let d = (at.distance(nearest) - RADIUS).max(0.0);
        (1.0 - d / radius).clamp(0.0, 1.0)
    }

    /// Takes `amount` from his shield, then his health. Returns whether
    /// this killed him.
    pub fn damage(&mut self, amount: f32, head: bool, weapon_path: &str) -> bool {
        let Some(b) = self.body.as_mut().filter(|b| b.dead.is_none()) else {
            return false;
        };
        b.since_hit = 0.0;
        let sniper = weapon_path.contains("sniper_rifle") || weapon_path.contains("beam_rifle");
        let precision = sniper || ["battle_rifle", "covenant_carbine", "magnum"].iter().any(|w| weapon_path.contains(w));
        if head && (sniper || (precision && b.shield <= 0.0)) {
            b.health = 0.0;
        } else {
            let s = amount.min(b.shield);
            b.shield -= s;
            b.health -= amount - s;
        }
        if b.health <= 0.0 {
            b.health = 0.0;
            b.dead = Some(0.0);
            return true;
        }
        false
    }
}

/// Ray (unit `rd`) against the capsule `pa`..`pb` of radius `r`: the
/// distance to the first hit in front of the origin.
fn capsule(ro: Vec3, rd: Vec3, pa: Vec3, pb: Vec3, r: f32) -> Option<f32> {
    let ba = pb - pa;
    let oa = ro - pa;
    let baba = ba.dot(ba);
    let bard = ba.dot(rd);
    let baoa = ba.dot(oa);
    let rdoa = rd.dot(oa);
    let oaoa = oa.dot(oa);
    let a = baba - bard * bard;
    // The side of the cylinder.
    let mut cap_end = None;
    if a > 1e-8 {
        let b = baba * rdoa - baoa * bard;
        let c = baba * oaoa - baoa * baoa - r * r * baba;
        let h = b * b - a * c;
        if h < 0.0 {
            return None;
        }
        let t = (-b - h.sqrt()) / a;
        let y = baoa + t * bard;
        if y > 0.0 && y < baba {
            return (t >= 0.0).then_some(t);
        }
        cap_end = Some(if y <= 0.0 { pa } else { pb });
    }
    // The end spheres (both when the ray runs along the axis).
    [cap_end.unwrap_or(pa), cap_end.unwrap_or(pb)]
        .iter()
        .filter_map(|&centre| {
            let oc = ro - centre;
            let b = rd.dot(oc);
            let h = b * b - (oc.dot(oc) - r * r);
            let t = -b - h.max(0.0).sqrt();
            (h > 0.0 && t >= 0.0).then_some(t)
        })
        .reduce(f32::min)
}

/// A plain red capsule for maps without the Spartan model.
#[derive(Resource)]
struct Stand(Handle<Mesh>, Handle<StandardMaterial>);

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let h = 0.70 * WU;
    commands.insert_resource(Stand(
        meshes.add(Capsule3d::new(RADIUS, h - 2.0 * RADIUS)),
        materials.add(StandardMaterial {
            base_color: Color::srgb(0.6, 0.08, 0.06),
            unlit: true,
            ..default()
        }),
    ));
}

/// Insert / d-pad left spawns him, Delete / d-pad right removes him.
#[allow(clippy::too_many_arguments)]
fn control(
    mut commands: Commands,
    mut enemy: ResMut<Enemy>,
    foot: Res<OnFoot>,
    input: Option<Res<halo3_bike::BikeInput>>,
    loaded: Res<LoadedBsps>,
    statue: Option<Res<SpartanStatue>>,
    stand: Option<Res<Stand>>,
    (lights, mut materials): (Option<Res<SceneLights>>, ResMut<Assets<StandardMaterial>>),
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    cameras: Query<(&Transform, Has<Skating>), With<FlyCamera>>,
) {
    let pad = gamepads.iter().next();
    let spawn = keys.just_pressed(KeyCode::Insert) || pad.is_some_and(|p| p.just_pressed(B::DPadLeft));
    let remove = keys.just_pressed(KeyCode::Delete) || pad.is_some_and(|p| p.just_pressed(B::DPadRight));
    if !spawn && !remove {
        return;
    }
    let Ok((view, skating)) = cameras.single() else {
        return;
    };
    // Free roam (the camera flies) or on foot; not on the bike or board.
    if skating && foot.spartan.is_none() {
        return;
    }
    if let Some(old) = enemy.body.take() {
        commands.entity(old.root).try_despawn();
    }
    if !spawn {
        info!("enemy Spartan removed");
        return;
    }

    // The collision: the Spartan's on foot, else our own for these BSPs.
    let mask = if loaded.mask == 0 { u32::MAX } else { loaded.mask };
    if foot.world().is_none() && enemy.world.as_ref().is_none_or(|(m, _)| *m != mask) {
        let Some(input) = input else {
            return;
        };
        enemy.world = Some((mask, halo3_bike::build_world(&input.0, mask)));
    }
    let Some(world) = foot.world().or(enemy.world.as_ref().map(|(_, w)| w)) else {
        return;
    };

    // Where the crosshair points, within reach, else a few metres ahead;
    // stood on the ground under it.
    let look = view.forward().as_vec3();
    let flat = Vec3::new(look.x, 0.0, look.z).normalize_or(Vec3::NEG_Z);
    let start = foot.spartan.as_ref().map_or(view.translation, |s| {
        // Past the Spartan's own body in third person.
        view.translation + look * (s.eye() - view.translation).dot(look).max(0.0)
    });
    let spot = match world.raycast_far(start, look, 60.0) {
        // Back off a wall so he stands in front of it.
        Some(hit) => hit.point - flat * (RADIUS + 0.2),
        None => start + flat * 8.0,
    };
    let feet = world.raycast_far(spot + Vec3::Y * 1.0, Vec3::NEG_Y, 200.0).map_or(spot, |h| h.point);
    let to_viewer = Vec3::new(view.translation.x - feet.x, 0.0, view.translation.z - feet.z);
    let facing = to_viewer.normalize_or(-flat);

    let at = to_halo(feet) + Vec3::Z * 0.3;
    let light = object_light(lights.as_deref(), at, loaded.shown);
    let root = commands
        .spawn((
            Transform::from_translation(feet).with_rotation(Quat::from_rotation_arc(Vec3::X, facing)),
            Visibility::Inherited,
            Name::new("enemy spartan"),
        ))
        .id();
    let height = match (statue.as_deref(), stand.as_deref()) {
        (Some(statue), _) if !statue.parts.is_empty() => {
            for (mesh, material, colour) in &statue.parts {
                if let Some(mut m) = materials.get_mut(material) {
                    m.base_color = LinearRgba::rgb(colour.red * light.x, colour.green * light.y, colour.blue * light.z).into();
                }
                commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::IDENTITY, ChildOf(root)));
            }
            statue.height
        }
        (_, Some(stand)) => {
            let h = 0.70 * WU;
            commands.spawn((
                Mesh3d(stand.0.clone()),
                MeshMaterial3d(stand.1.clone()),
                Transform::from_translation(Vec3::Y * h * 0.5),
                ChildOf(root),
            ));
            h
        }
        _ => 0.70 * WU,
    };
    enemy.body = Some(Body {
        root,
        feet,
        facing,
        height,
        shield: SHIELD,
        health: HEALTH,
        since_hit: 0.0,
        dead: None,
    });
    info!("enemy Spartan at {feet:.1}");
}

/// Recharges him while alive; tips him over and clears him away once dead.
fn tick(mut commands: Commands, mut enemy: ResMut<Enemy>, time: Res<Time>, mut transforms: Query<&mut Transform>) {
    let dt = time.delta_secs();
    let Some(b) = enemy.body.as_mut() else {
        return;
    };
    match b.dead.as_mut() {
        None => {
            b.since_hit += dt;
            if b.since_hit > RECHARGE_DELAY {
                b.shield = (b.shield + SHIELD * dt / RECHARGE_TIME).min(SHIELD);
                b.health = (b.health + HEALTH * dt / RECHARGE_TIME).min(HEALTH);
            }
        }
        Some(t) => {
            *t += dt;
            let t = *t;
            if t > FALL + LIE {
                let root = b.root;
                enemy.body = None;
                commands.entity(root).try_despawn();
                return;
            }
            // Over backwards about his feet, quickening like a fall.
            let k = (t / FALL).min(1.0);
            let angle = std::f32::consts::FRAC_PI_2 * 0.97 * k * k;
            let axis = Vec3::Y.cross(-b.facing).normalize_or(Vec3::X);
            if let Ok(mut tf) = transforms.get_mut(b.root) {
                tf.rotation = Quat::from_axis_angle(axis, angle) * Quat::from_rotation_arc(Vec3::X, b.facing);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_meets_capsule_side_and_top() {
        let (pa, pb, r) = (Vec3::new(0.0, 0.5, 0.0), Vec3::new(0.0, 1.5, 0.0), 0.5);
        // From the side at mid height: the surface, 0.5 short of the axis.
        let t = capsule(Vec3::new(-5.0, 1.0, 0.0), Vec3::X, pa, pb, r).unwrap();
        assert!((t - 4.5).abs() < 1e-4, "{t}");
        // Straight down onto the top: the top of the sphere at 2.0.
        let t = capsule(Vec3::new(0.0, 5.0, 0.0), Vec3::NEG_Y, pa, pb, r).unwrap();
        assert!((t - 3.0).abs() < 1e-4, "{t}");
        // Past him, and behind the origin.
        assert!(capsule(Vec3::new(-5.0, 1.0, 2.0), Vec3::X, pa, pb, r).is_none());
        assert!(capsule(Vec3::new(5.0, 1.0, 0.0), Vec3::X, pa, pb, r).is_none());
        // Low, into the bottom sphere.
        let t = capsule(Vec3::new(-5.0, 0.2, 0.0), Vec3::X, pa, pb, r).unwrap();
        assert!(t > 4.5 && t < 5.0, "{t}");
    }
}
