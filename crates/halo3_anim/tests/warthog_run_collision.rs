//! What the bike rides on along the Warthog run, with 120_halo's own
//! scripts running headless (`ins_trench_run`) and the islands posed by
//! their animations: the first round island (`100_island01`) is solid all
//! the way round while it tilts (its collision goes where it is drawn), and
//! the first tunnel has a floor and no wall from end to end (the seam
//! between BSPs 100 and 110 used to be one). Uses the bike's own collision
//! world, as the game builds it. Skips quietly when the map is not
//! installed.
//!
//! One test only: the moved and removed objects are process-wide
//! (`halo3_core::objects`).

use std::path::PathBuf;
use std::sync::Arc;

use glam::{Mat4, Vec2, Vec3};
use halo3_anim::MachinePoser;
use halo3_bike::physics::{Bike, Controls, World};
use halo3_core::coords::{to_halo, to_render, to_render_dir};
use halo3_script::vm::{Config, WorldApi};
use halo3_script::{DevicePoser, NoAi, Runtime, setup};

struct Player {
    at: Vec3,
    zone_set: usize,
}

impl WorldApi for Player {
    fn player_position(&self) -> Option<Vec3> {
        Some(self.at)
    }
    fn teleport_player(&mut self, position: Vec3, _yaw: f32) {
        self.at = position;
    }
    fn current_zone_set(&self) -> Option<usize> {
        Some(self.zone_set)
    }
    fn switch_zone_set(&mut self, zone_set: usize) {
        self.zone_set = zone_set;
    }
    fn effect(&mut self, _tag: &str, _at: Vec3) {}
}

fn map() -> Option<PathBuf> {
    let dir = std::env::var_os("HALO3_MAPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps")
    });
    let p = dir.join("120_halo.map");
    p.exists().then_some(p)
}

/// The highest point of `tris` straight under (x, y) and below `top`, Halo frame.
fn height(tris: &[[Vec3; 3]], x: f32, y: f32, top: f32) -> Option<f32> {
    let p = Vec2::new(x, y);
    let mut best: Option<f32> = None;
    for t in tris {
        let (a, b, c) = (t[0].truncate(), t[1].truncate(), t[2].truncate());
        let d = (b - a).perp_dot(c - a);
        if d.abs() < 1e-9 {
            continue;
        }
        let w1 = (p - a).perp_dot(c - a) / d;
        let w2 = (b - a).perp_dot(p - a) / d;
        let w0 = 1.0 - w1 - w2;
        if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
            continue;
        }
        let z = t[0].z * w0 + t[1].z * w1 + t[2].z * w2;
        if z <= top && best.is_none_or(|q| z > q) {
            best = Some(z);
        }
    }
    best
}

/// The floor the bike's world has under a Halo point, Halo z.
fn floor(world: &World, x: f32, y: f32, from: f32) -> Option<(f32, Vec3)> {
    let hit = world.raycast_far(to_render(Vec3::new(x, y, from)), -Vec3::Y, 40.0 * 3.048)?;
    Some((to_halo(hit.point).z, hit.normal))
}

#[test]
fn island_and_tunnel_are_ridable() {
    let Some(path) = map() else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    let options = blam_geometry::Options {
        bsps: Some(vec![7, 8, 9]),
        ..Default::default()
    };
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(&cache, &options, &mut scene).unwrap();
    blam_geometry::objects::add_collision(&cache, &mut scene);
    let data = Arc::new(blam_script::ScriptData::read(&cache).unwrap());
    setup::mark_dynamic(&mut scene, &data);
    let mut rt = setup::runtime(&cache, &scene, Arc::clone(&data), Config { editor: true, ..Default::default() });
    let poser = MachinePoser::load(&cache, rt.objects.info.iter().filter(|o| o.scene.is_some()).filter_map(|o| o.tag));
    let n = rt.objects.info.iter().filter(|o| o.scene.is_some()).count();
    let (mut solid, mut posed) = (vec![true; n], vec![false; n]);
    // The zone set the run starts in: BSPs 100 and 110 (and the hallway).
    let world = halo3_bike::build_world(&scene.collision, 0x190);

    assert!(rt.console("ins_trench_run"));
    let mut player = Player {
        at: Vec3::new(-263.64, -433.77, -5.05),
        zone_set: 0,
    };
    let mut ai = NoAi;
    let mut step = |rt: &mut Runtime, player: &mut Player, ticks: usize| {
        for _ in 0..ticks {
            rt.step(player, &mut ai, Some(&poser as &dyn DevicePoser));
        }
    };
    let centre = |name: &str| {
        let v = scene.trigger_volumes.iter().find(|v| v.name == name).unwrap_or_else(|| panic!("{name}"));
        let left = v.up.cross(v.forward);
        v.position + v.forward * v.extents.x * 0.5 + left * v.extents.y * 0.5 + v.up * v.extents.z * 0.5
    };
    step(&mut rt, &mut player, 90);
    for v in ["trench_run_plate_100_trig", "trench_run_100_trig00", "trench_run_100_trig01", "trench_run_plate_100_hw_trig", "trench_run_100_trig04", "100_game_save_trig"] {
        player.at = centre(v);
        step(&mut rt, &mut player, 20);
    }

    // ---- The first island, tilting under the player. ----
    let island = rt.objects.by_name[data.object_index("100_island01").unwrap()].unwrap() as usize;
    let o = &scene.objects[island];
    let mesh: Vec<[Vec3; 3]> = scene.placements[o.placements.start as usize..o.placements.end as usize]
        .iter()
        .flat_map(|pl| {
            let m = &scene.meshes[pl.mesh as usize];
            m.parts.iter().flat_map(move |part| {
                part.indices
                    .chunks_exact(3)
                    .map(move |t| [0, 1, 2].map(|k| pl.transform.transform_point3(m.positions[t[k] as usize])))
            })
        })
        .collect();
    let at_rest: Vec<[Vec3; 3]> = scene
        .collision
        .triangles
        .iter()
        .filter(|t| t.source.object() == Some(island as u32))
        .map(|t| t.points)
        .collect();
    let base = rt.objects.info[island].base;
    let middle = Vec3::new(72.6, -212.7, 0.0);
    // Points round the ring where the drawn island and its collision agree
    // when it is still (they differ on a few edges of the model).
    let mut ring = Vec::new();
    for r in [12.0f32, 18.0, 24.0, 30.0] {
        for k in 0..24 {
            let a = k as f32 / 24.0 * std::f32::consts::TAU;
            let (x, y) = (middle.x + r * a.cos(), middle.y + r * a.sin());
            if let (Some(d), Some(c)) = (height(&mesh, x, y, 0.0), height(&at_rest, x, y, 0.0))
                && (d - c).abs() < 0.1
            {
                ring.push((x, y));
            }
        }
    }
    assert!(ring.len() > 60, "{} ring points", ring.len());
    let check = |rt: &Runtime, what: &str| {
        let s = &rt.objects.state[island];
        assert!(s.exists && s.solid, "{what}: the island must be solid");
        let motion: Mat4 = s.transform * base.inverse();
        let drawn: Vec<[Vec3; 3]> = mesh.iter().map(|t| t.map(|p| motion.transform_point3(p))).collect();
        let mut worst = 0.0f32;
        for &(x, y) in &ring {
            let Some(d) = height(&drawn, x, y, 0.0) else { continue };
            let (z, normal) = floor(&world, x, y, d + 3.0).unwrap_or_else(|| panic!("{what}: no floor at ({x}, {y})"));
            worst = worst.max((z - d).abs());
            assert!((z - d).abs() < 0.25, "{what}: at ({x:.1}, {y:.1}) the island is drawn at {d:.2} but collides at {z:.2}");
            assert!(normal.dot(to_render_dir(Vec3::Z)) > 0.0, "{what}: floor faces down at ({x}, {y})");
        }
        eprintln!("{what}: {} ring points, collision within {worst:.3} wu of the drawn island", ring.len());
    };
    check(&rt, "before");
    // A bike standing on the island where the player comes on, as it
    // lurches up: carried with it, not left inside it.
    let (bx, by) = *ring
        .iter()
        .min_by(|a, b| Vec2::new(a.0 - 54.0, a.1 + 225.0).length().total_cmp(&Vec2::new(b.0 - 54.0, b.1 + 225.0).length()))
        .unwrap();
    let rest = height(&mesh, bx, by, 0.0).unwrap();
    let mut bike = Bike::dropped(&world, to_render(Vec3::new(bx, by, rest + 1.0)), 0.0);
    for _ in 0..60 {
        bike.step(&world, Controls::default(), 1.0 / 60.0);
    }
    let bike_above = |rt: &Runtime, bike: &Bike| {
        let p = to_halo(bike.position);
        let motion = rt.objects.state[island].transform * base.inverse();
        let drawn: Vec<[Vec3; 3]> = mesh.iter().map(|t| t.map(|q| motion.transform_point3(q))).collect();
        p.z - height(&drawn, p.x, p.y, p.z + 1.0).unwrap_or(f32::NAN)
    };
    let sitting = bike_above(&rt, &bike);
    player.at = centre("100_island_trig01");
    for tick in 0..150 {
        step(&mut rt, &mut player, 1);
        halo3_script::publish_collision(&rt.objects, &mut solid, &mut posed);
        bike.step(&world, Controls::default(), 1.0 / 60.0);
        bike.step(&world, Controls::default(), 1.0 / 60.0);
        let above = bike_above(&rt, &bike);
        assert!((above - sitting).abs() < 0.3, "tick {tick}: the bike sat {sitting:.2} wu above the island, now {above:.2}");
        if tick == 30 {
            check(&rt, "tilting");
        }
    }
    eprintln!("bike carried: {sitting:.2} wu above the island before, {:.2} after the lurch", bike_above(&rt, &bike));
    let lifted = rt.objects.state[island].transform.w_axis.z - base.w_axis.z;
    let tilt = rt.objects.state[island].transform * base.inverse();
    assert!(!tilt.abs_diff_eq(Mat4::IDENTITY, 1e-3), "tilt_up moved nothing (lifted {lifted})");
    check(&rt, "tilted");
    // Round the far side, as the user rode it, for twenty seconds.
    for k in 0..20 {
        let a = std::f32::consts::PI * (1.0 + k as f32 / 10.0);
        player.at = middle + Vec3::new(24.0 * a.cos(), 24.0 * a.sin(), -20.0);
        step(&mut rt, &mut player, 30);
    }
    halo3_script::publish_collision(&rt.objects, &mut solid, &mut posed);
    check(&rt, "ridden round");

    // ---- Off the island and through the first tunnel. ----
    for v in ["trench_run_100_trig06", "trench_run_100_trig07", "trench_run_100_trig08", "trench_run_game_save01"] {
        player.at = centre(v);
        step(&mut rt, &mut player, 20);
    }
    let mut y = -40.0f32;
    let mut walls = Vec::new();
    while y < 175.0 {
        player.at = Vec3::new(58.7, y, -34.0);
        step(&mut rt, &mut player, 2);
        halo3_script::publish_collision(&rt.objects, &mut solid, &mut posed);
        for x in [52.0f32, 58.7] {
            let (z, normal) = floor(&world, x, y, -26.0).unwrap_or_else(|| panic!("no tunnel floor at ({x}, {y})"));
            assert!(z > -45.0, "tunnel floor at ({x}, {y}) is {z}");
            assert!(normal.dot(to_render_dir(Vec3::Z)) > 0.7, "tunnel floor at ({x}, {y}) is steep");
            // Nothing steep straight ahead at wheel or rider height.
            for h in [0.4f32, 1.0, 1.6] {
                let from = to_render(Vec3::new(x, y, z + h));
                if let Some(hit) = world.raycast_far(from, to_render_dir(Vec3::Y), 1.0 * 3.048)
                    && hit.normal.dot(to_render_dir(Vec3::Z)) < 0.5
                {
                    walls.push((x, y, h, to_halo(hit.point)));
                }
            }
        }
        y += 1.0;
    }
    assert!(walls.is_empty(), "walls in the first tunnel: {walls:?}");
}
