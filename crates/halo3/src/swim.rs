//! What water does to the skater (`docs/format/water.md` 6).
//!
//! Halo's water surface is not in the collision BSP (Valhalla's collision
//! materials hold the river bed, `riverworld_ground_water`, and the lake bed,
//! `ground_lake_deep`, but no `rmw ` shader), so the skate engine already
//! rides on the bed under the water. Here the water surfaces come from the
//! render geometry drawn with water shaders, and every frame:
//!
//! - over land, on the ground: the spot is remembered as the last safe one;
//! - into water: a splash, and the depth (surface minus the bed under it) is
//!   logged. Shallow water (the river) is ridden on its bed;
//! - deep water (deeper than [`DEEP_WU`]) with the board under the surface
//!   for [`SINK_SECONDS`]: the skater sinks and is put back at the last safe
//!   spot through `Session::activate` (`skate::Respawn`), with a splash.
//!
//! The engine takes no outside forces, so shallow water cannot slow the
//! board down (no drag); see the doc.
//!
//! The same last safe spot is where the skater comes back after falling into
//! a kill volume or out of the world (`falls.rs`; Guardian's abyss), so this
//! runs on maps without water too.

use std::collections::HashMap;

use bevy::prelude::*;

use blam_geometry::Scene;

use crate::falls::KillZones;
use crate::skate::{Respawn, RiderPose};
use crate::world::{to_halo, to_render};

/// Water deeper than this (world units, 1 wu = 3.048 m) cannot be ridden:
/// about 1.4 m, chest-deep on the skater.
pub const DEEP_WU: f32 = blam_geometry::rails::DEEP_WATER;
/// How long the board must stay under deep water before the skater is
/// taken back.
pub const SINK_SECONDS: f32 = 0.6;
/// Below this height over the ground the board counts as on it.
const GROUNDED_WU: f32 = 0.25;

/// Triangles bucketed by their XY footprint, for "what is above or below
/// this point" in Halo world units.
pub struct Columns {
    cell: f32,
    triangles: Vec<[Vec3; 3]>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

impl Columns {
    pub fn new(triangles: Vec<[Vec3; 3]>, cell: f32) -> Columns {
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, t) in triangles.iter().enumerate() {
            let lo = t[0].min(t[1]).min(t[2]);
            let hi = t[0].max(t[1]).max(t[2]);
            for x in (lo.x / cell).floor() as i32..=(hi.x / cell).floor() as i32 {
                for y in (lo.y / cell).floor() as i32..=(hi.y / cell).floor() as i32 {
                    grid.entry((x, y)).or_default().push(i as u32);
                }
            }
        }
        Columns { cell, triangles, grid }
    }

    /// Heights where the vertical line through `(x, y)` crosses a triangle
    /// that faces up (steep walls are skipped).
    pub fn heights(&self, x: f32, y: f32) -> impl Iterator<Item = f32> + '_ {
        let key = ((x / self.cell).floor() as i32, (y / self.cell).floor() as i32);
        self.grid
            .get(&key)
            .into_iter()
            .flatten()
            .filter_map(move |&i| height_at(&self.triangles[i as usize], x, y))
    }

    /// The highest surface at or below `z`.
    pub fn below(&self, x: f32, y: f32, z: f32) -> Option<f32> {
        self.heights(x, y).filter(|h| *h <= z).max_by(f32::total_cmp)
    }

    /// The lowest surface at or above `z`.
    pub fn above(&self, x: f32, y: f32, z: f32) -> Option<f32> {
        self.heights(x, y).filter(|h| *h >= z).min_by(f32::total_cmp)
    }
}

/// The height of a triangle over `(x, y)`, when the point is inside its
/// footprint and the triangle is not a wall.
fn height_at(t: &[Vec3; 3], x: f32, y: f32) -> Option<f32> {
    let [a, b, c] = *t;
    let n = (b - a).cross(c - a);
    if n.z.abs() < 0.2 * n.length() {
        return None;
    }
    let d = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
    if d.abs() < 1e-9 {
        return None;
    }
    let u = ((x - a.x) * (c.y - a.y) - (c.x - a.x) * (y - a.y)) / d;
    let v = ((b.x - a.x) * (y - a.y) - (x - a.x) * (b.y - a.y)) / d;
    let e = -1e-4;
    (u >= e && v >= e && u + v <= 1.0 - e).then_some(a.z + u * (b.z - a.z) + v * (c.z - a.z))
}

/// The map's water surfaces and the ground (the collision), as columns.
#[derive(Resource)]
pub struct WaterWorld {
    pub water: Columns,
    pub ground: Columns,
}

impl WaterWorld {
    /// From the scene's triangles drawn with the given water materials and
    /// its collision.
    pub fn build(scene: &Scene, water_materials: &[u32]) -> WaterWorld {
        let water: Vec<[Vec3; 3]> = if water_materials.is_empty() {
            Vec::new()
        } else {
            scene
                .world_triangles()
                .filter(|(m, _)| water_materials.contains(m))
                .map(|(_, t)| t)
                .collect()
        };
        let ground = scene.collision.triangles.iter().map(|t| t.points).collect();
        WaterWorld {
            water: Columns::new(water, 4.0),
            ground: Columns::new(ground, 4.0),
        }
    }

    /// What is at a point: the water surface over it (if the point is under
    /// one, or within a little above), and the ground under that.
    pub fn probe(&self, p: Vec3) -> Probe {
        let surface = self.water.above(p.x, p.y, p.z - 0.15);
        let top = surface.unwrap_or(p.z) + 0.01;
        let bed = self.ground.below(p.x, p.y, top);
        Probe { surface, bed }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Probe {
    pub surface: Option<f32>,
    pub bed: Option<f32>,
}

impl Probe {
    /// Water depth over the bed here (0 without water).
    pub fn depth(&self) -> f32 {
        match (self.surface, self.bed) {
            (Some(s), Some(b)) => (s - b).max(0.0),
            (Some(_), None) => f32::INFINITY,
            _ => 0.0,
        }
    }
}

#[derive(Resource, Default)]
struct SwimState {
    /// Last place the board stood on dry ground, and its heading (Halo yaw).
    safe: Option<(Vec3, f32)>,
    in_water: bool,
    deep_logged: bool,
    under: f32,
    /// The skater was just put back: wait for a pose from the new spot.
    settling: f32,
    /// Seconds the board has been on dry ground (for the test throw).
    dry: f32,
    /// `HALO3_SWIM_TEST=x,y,z`: once the skater has stood on dry ground for
    /// a second, throw it to that Halo point (into water) to exercise the
    /// sinking and the way back. Taken once.
    throw: Option<Vec3>,
    thrown: bool,
}

impl SwimState {
    fn new() -> SwimState {
        let throw = std::env::var("HALO3_SWIM_TEST").ok().and_then(|s| {
            let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            (v.len() == 3).then(|| Vec3::new(v[0], v[1], v[2]))
        });
        SwimState {
            throw,
            ..default()
        }
    }
}

/// Where to put the skater when it sinks before ever touching dry ground:
/// the map's start spawn (Halo position, yaw).
#[derive(Resource, Clone, Copy)]
pub struct FallbackSpot(pub Vec3, pub f32);

/// One splash droplet, in the renderer frame.
#[derive(Component)]
struct Drop {
    p: Vec3,
    v: Vec3,
    life: f32,
}

/// Droplets waiting to be spawned.
#[derive(Resource, Default)]
struct Splashes(Vec<Drop>);

/// The droplets' shared mesh and material.
#[derive(Resource)]
struct DropLook(Handle<Mesh>, Handle<StandardMaterial>);

pub struct SwimPlugin;

impl Plugin for SwimPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SwimState::new())
            .init_resource::<Splashes>()
            .add_systems(Update, (water_rules.run_if(crate::pause::running), draw_splashes).chain());
    }
}

fn splash(splashes: &mut Splashes, at: Vec3, strength: f32) {
    // A ring of droplets thrown up and out, deterministic (no rng).
    let n = (24.0 * strength) as usize + 8;
    for i in 0..n {
        let a = i as f32 * 2.399_963; // golden angle
        let r = 0.6 + (i % 5) as f32 * 0.35;
        let up = 2.5 + (i % 7) as f32 * 0.6 * strength;
        splashes.0.push(Drop {
            p: at,
            v: Vec3::new(a.cos() * r, up, a.sin() * r) * strength.max(0.6),
            life: 0.9 + (i % 3) as f32 * 0.2,
        });
    }
}

fn board_of(pose: &RiderPose) -> Option<(Vec3, f32)> {
    let at = |name: &str| {
        let i = pose.names.iter().position(|n| n == name)?;
        Some(pose.bones.get(i)?.w_axis.truncate() / pose.scale)
    };
    let (front, back) = (at("TRUCK_FRONT")?, at("TRUCK_BACK")?);
    let (f, b) = (to_halo(front), to_halo(back));
    let d = f - b;
    Some(((f + b) * 0.5, d.y.atan2(d.x)))
}

#[allow(clippy::too_many_arguments)]
fn water_rules(
    world: Option<Res<WaterWorld>>,
    kill: Option<Res<KillZones>>,
    fallback: Option<Res<FallbackSpot>>,
    pose: Option<Res<RiderPose>>,
    time: Res<Time>,
    mut state: ResMut<SwimState>,
    mut respawn: ResMut<Respawn>,
    mut splashes: ResMut<Splashes>,
) {
    let (Some(world), Some(pose)) = (world, pose) else {
        return;
    };
    if !pose.active || pose.bones.is_empty() || pose.bike {
        state.in_water = false;
        state.under = 0.0;
        return;
    }
    let dt = time.delta_secs().min(0.1);
    if state.settling > 0.0 {
        state.settling -= dt;
        return;
    }
    let Some((board, heading)) = board_of(&pose) else {
        return;
    };
    if let Some(volume) = kill.as_deref().and_then(|k| k.deadly(board)) {
        let back = state
            .safe
            .map(|s| (s, "the last safe spot"))
            .or_else(|| fallback.as_deref().map(|f| ((f.0, f.1), "the start spawn (no safe spot yet)")));
        match back {
            Some(((p, yaw), what)) => {
                info!(
                    "fall: into {volume} at halo [{:.2}, {:.2}, {:.2}]; back to {what} halo [{:.2}, {:.2}, {:.2}]",
                    board.x, board.y, board.z, p.x, p.y, p.z
                );
                respawn.0 = Some((p, yaw));
            }
            None => warn!("fall: into {volume}, but no safe spot is known; left where it is"),
        }
        state.under = 0.0;
        state.in_water = false;
        state.settling = 0.5;
        return;
    }
    let probe = world.probe(board);
    let under = probe.surface.is_some_and(|s| board.z < s);
    let mut on_dry_ground = false;
    if !under && probe.surface.is_none() {
        // Dry: remember the spot when the board is on the ground.
        let ground = world.ground.below(board.x, board.y, board.z + 0.1);
        if ground.is_some_and(|g| board.z - g < GROUNDED_WU) {
            if state.safe.is_none() {
                info!("water: first safe spot halo [{:.2}, {:.2}, {:.2}]", board.x, board.y, board.z);
            }
            state.safe = Some((board, heading));
            on_dry_ground = true;
        }
    }
    state.dry = if on_dry_ground { state.dry + dt } else { 0.0 };
    if !state.thrown
        && state.dry > 1.0
        && let Some(target) = state.throw
    {
        state.thrown = true;
        info!(
            "water: test throw from halo [{:.2}, {:.2}, {:.2}] to [{:.2}, {:.2}, {:.2}]",
            board.x, board.y, board.z, target.x, target.y, target.z
        );
        respawn.0 = Some((target, heading));
        state.settling = 0.3;
        return;
    }
    let depth = probe.depth();
    if under && !state.in_water {
        let s = probe.surface.unwrap_or(board.z);
        info!(
            "water: in at halo [{:.2}, {:.2}, {:.2}], surface z {s:.2}, depth {depth:.2} wu ({})",
            board.x,
            board.y,
            board.z,
            if depth > DEEP_WU { "deep" } else { "shallow: riding the bed" }
        );
        splash(&mut splashes, to_render(Vec3::new(board.x, board.y, s)), 1.0);
        state.deep_logged = false;
    } else if !under && state.in_water {
        info!("water: out at halo [{:.2}, {:.2}, {:.2}]", board.x, board.y, board.z);
    }
    state.in_water = under;
    if under && depth > DEEP_WU {
        state.under += dt;
        if !state.deep_logged {
            state.deep_logged = true;
            info!("water: deep ({depth:.2} wu over the bed), sinking");
        }
        if state.under >= SINK_SECONDS {
            let s = probe.surface.unwrap_or(board.z);
            splash(&mut splashes, to_render(Vec3::new(board.x, board.y, s)), 1.6);
            let back = state
                .safe
                .map(|s| (s, "the last safe spot"))
                .or_else(|| fallback.as_deref().map(|f| ((f.0, f.1), "the start spawn (no safe spot yet)")));
            match back {
                Some(((p, yaw), what)) => {
                    info!(
                        "water: sank after {:.1} s; back to {what} halo [{:.2}, {:.2}, {:.2}] yaw {:.0} deg",
                        state.under,
                        p.x,
                        p.y,
                        p.z,
                        yaw.to_degrees()
                    );
                    respawn.0 = Some((p, yaw));
                }
                None => warn!("water: sank, but no safe spot is known; left where it is"),
            }
            state.under = 0.0;
            state.in_water = false;
            state.settling = 0.5;
        }
    } else {
        state.under = 0.0;
    }
}

/// Spawns waiting droplets as small white spheres and moves them under
/// gravity, shrinking as they fade, until they expire.
#[allow(clippy::too_many_arguments)]
fn draw_splashes(
    mut commands: Commands,
    time: Res<Time>,
    mut splashes: ResMut<Splashes>,
    mut look: Local<Option<DropLook>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut drops: Query<(Entity, &mut Drop, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.1);
    for (entity, mut d, mut t) in &mut drops {
        if !d.life.is_finite() {
            continue;
        }
        d.life -= dt;
        d.v.y -= 9.8 * dt;
        let v = d.v;
        d.p += v * dt;
        // Stretched along the motion, like a streak.
        let s = d.life.clamp(0.0, 1.0);
        let along = v.normalize_or(Vec3::Y);
        *t = Transform::from_translation(d.p)
            .with_rotation(Quat::from_rotation_arc(Vec3::Y, along))
            .with_scale(Vec3::new(s, s * (1.0 + v.length() * 0.15), s));
        if d.life <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
    let first = look.is_none();
    let look = look.get_or_insert_with(|| {
        DropLook(
            meshes.add(Sphere::new(0.07).mesh().ico(1).unwrap()),
            materials.add(StandardMaterial {
                base_color: Color::srgba(0.9, 0.95, 1.0, 0.85),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                ..default()
            }),
        )
    });
    if first {
        // One droplet far below the world for good, so the pipeline is
        // compiled before the first real splash (else that splash is gone
        // before it can be drawn).
        splashes.0.push(Drop {
            p: Vec3::new(0.0, -50_000.0, 0.0),
            v: Vec3::ZERO,
            life: f32::INFINITY,
        });
    }
    for d in splashes.0.drain(..) {
        commands.spawn((
            Mesh3d(look.0.clone()),
            MeshMaterial3d(look.1.clone()),
            Transform::from_translation(d.p),
            bevy::light::NotShadowCaster,
            d,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns() {
        // A floor at z 0 and a water plane at z 1 over x, y in 0..10.
        let quad = |z: f32| {
            [
                [Vec3::new(0.0, 0.0, z), Vec3::new(10.0, 0.0, z), Vec3::new(10.0, 10.0, z)],
                [Vec3::new(0.0, 0.0, z), Vec3::new(10.0, 10.0, z), Vec3::new(0.0, 10.0, z)],
            ]
        };
        let water = Columns::new(quad(1.0).to_vec(), 4.0);
        let ground = Columns::new(quad(0.0).to_vec(), 4.0);
        let w = WaterWorld { water, ground };
        let p = w.probe(Vec3::new(3.0, 7.0, 0.2));
        assert_eq!(p.surface, Some(1.0));
        assert_eq!(p.bed, Some(0.0));
        assert!((p.depth() - 1.0).abs() < 1e-6);
        // Above the water, close: still sees it; far above: dry.
        assert_eq!(w.probe(Vec3::new(3.0, 7.0, 1.1)).surface, Some(1.0));
        assert_eq!(w.probe(Vec3::new(3.0, 7.0, 3.0)).surface, None);
        // Outside the footprint.
        assert_eq!(w.probe(Vec3::new(13.0, 7.0, 0.2)).surface, None);
    }
}
