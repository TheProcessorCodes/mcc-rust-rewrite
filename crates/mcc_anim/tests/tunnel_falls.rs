//! The girders that come down in the Warthog run's two tunnels. 120_halo's
//! own scripts run headless (`ins_trench_run`), the player is walked
//! through the run's triggers and both tunnels, and:
//!
//! - exactly the `tunnel_girder` crates the scripts name move (`fall_off`
//!   in `trench_tunnel01` / `02`, and `tunnel_fall_away 100_hw_ceiling10`);
//!   the rest stay up;
//! - a girder whose hinge `fall` loosens swings down on it, turning as it
//!   goes; `100_hw_ceiling10`, whose hinge `destroy` also breaks, falls
//!   free and tumbles;
//! - each comes to rest within a bounded time, solid, with no corner sunk
//!   into the road, touching it or hanging at its hinge's limit, and the
//!   bike's collision world has it where it lies.
//!
//! Skips quietly when the map is not installed. One test only: the moved
//! and removed objects are process-wide (`mcc_core::objects`).

use std::path::PathBuf;
use std::sync::Arc;

use glam::{Mat4, Vec2, Vec3};
use mcc_anim::MachinePoser;
use mcc_bike::physics::World;
use mcc_core::coords::{to_halo, to_render};
use mcc_script::objects::Motion;
use mcc_script::vm::{Config, WorldApi};
use mcc_script::{DevicePoser, NoAi, Runtime, setup};

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
    let dir = game_paths::halo3_maps()?;
    let p = dir.join("120_halo.map");
    p.exists().then_some(p)
}

/// What the tunnels' scripts do to girders, in order: `trench_tunnel01`
/// (`fall_off` on reaching `trench_run_game_save01`, `tunnel01_trig01`,
/// `tunnel01_trig03` twice a second apart, `tunnel01_trig04`,
/// `tunnel01_trig05`; `tunnel_fall_away` on `120_08_trench_encounter_110`)
/// and `trench_tunnel02` (`fall_off` on `trench_run_game_save03`,
/// `tunnel02_trig02`, `tunnel02_trig04`, `tunnel02_trig05` twice,
/// `tunnel02_trig07`). `true`: the hinge is broken too (falls free).
const SCRIPTED: [(&str, bool); 13] = [
    ("100_hw_ceiling02", false),
    ("100_hw_ceiling04", false),
    ("100_hw_ceiling08", false),
    ("100_hw_ceiling09", false),
    ("100_hw_ceiling10", true),
    ("100_hw_ceiling13", false),
    ("100_hw_ceiling14", false),
    ("110_hw_ceiling02", false),
    ("110_hw_ceiling05", false),
    ("110_hw_ceiling08", false),
    ("110_hw_ceiling10", false),
    ("110_hw_ceiling11", false),
    ("110_hw_ceiling14", false),
];

/// A girder at rest must be at rest this soon after it let go, seconds.
const REST_WITHIN: f32 = 15.0;

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

/// The first thing the bike's world has straight down from a Halo point, Halo z.
fn down(world: &World, p: Vec3, max: f32) -> Option<f32> {
    world.raycast_far(to_render(p), -Vec3::Y, max * 3.048).map(|h| to_halo(h.point).z)
}

/// How far an object is turned from where the scenario placed it, degrees.
fn turned(base: Mat4, now: Mat4) -> f32 {
    let (_, a, _) = base.to_scale_rotation_translation();
    let (_, b, _) = now.to_scale_rotation_translation();
    a.angle_between(b).to_degrees()
}

/// Checks a girder the moment it comes to rest; returns a line for the log.
fn check_rest(name: &str, e: usize, rt: &Runtime, scene: &blam_geometry::Scene, world: &World, seconds: f32) -> String {
    let (info, s) = (&rt.objects.info[e], &rt.objects.state[e]);
    assert!(s.exists && s.solid, "{name} came to rest but is not solid");
    assert!(seconds <= REST_WITHIN, "{name} took {seconds:.1} s to come to rest");
    let motion = s.transform * info.base.inverse();
    let corners: Vec<Vec3> = info.contacts.iter().map(|p| motion.transform_point3(*p)).collect();
    let tris: Vec<[Vec3; 3]> = scene
        .collision
        .triangles
        .iter()
        .filter(|t| t.source.object() == Some(e as u32))
        .map(|t| t.points.map(|p| motion.transform_point3(p)))
        .collect();

    // No corner in the road: with the girder taken out of the bike's world
    // for a moment, the floor straight under each corner (looked for from
    // where that corner hung, or is, whichever is higher) is never above
    // it. The nearest gap says whether it touches.
    mcc_core::objects::set_collision_removed(e as u32, true);
    let mut nearest = f32::INFINITY;
    for (p, hung) in corners.iter().zip(&info.contacts) {
        let from = Vec3::new(p.x, p.y, p.z.max(hung.z) + 0.05);
        let Some(floor) = down(world, from, 60.0) else { continue };
        assert!(floor < p.z + 0.05, "{name}: a corner at {p} is {:.2} wu into the floor", floor - p.z);
        nearest = nearest.min(p.z - floor);
    }
    mcc_core::objects::set_collision_removed(e as u32, false);
    let (how, touching) = match &s.motion {
        Motion::Swinging(w) => {
            assert!(w.resting);
            let at_limit = (w.angle.abs() - w.limit).abs() < 0.01;
            assert!(at_limit || nearest < 0.1, "{name}: hangs at {:.1} of {:.1} degrees, {nearest:.2} wu off the floor", w.angle.to_degrees(), w.limit.to_degrees());
            (format!("hangs on its hinge at {:.1} degrees (limit {:.1})", w.angle.to_degrees(), w.limit.to_degrees()), nearest < 0.1)
        }
        Motion::Tumbling(b) => {
            assert!(b.asleep);
            assert!(nearest < 0.1, "{name}: asleep {nearest:.2} wu off the floor");
            ("fell free and lies".to_string(), true)
        }
        m => panic!("{name}: at rest as {m:?}"),
    };

    // The bike's world has it where it lies: straight down onto it, at
    // every unit along it where it has a top, the bike meets that top.
    let mut met = 0;
    let c = s.transform.w_axis.truncate();
    let along = s.transform.x_axis.truncate().normalize();
    for k in -10..=10 {
        let p = c + along * k as f32;
        let Some(top) = height(&tris, p.x, p.y, p.z + 30.0) else { continue };
        let hit = down(world, Vec3::new(p.x, p.y, top + 0.3), 1.0).unwrap_or_else(|| panic!("{name}: the bike's world is empty over it"));
        assert!((hit - top).abs() < 0.05, "{name}: its top is at {top:.2} there, the bike meets {hit:.2}");
        met += 1;
    }
    assert!(met >= 3, "{name}: the bike met it at only {met} points");
    format!(
        "{name}: {how}, turned {:.1} degrees, {} the floor (nearest corner {nearest:.2} wu), at rest {seconds:.1} s after it let go",
        turned(info.base, s.transform),
        if touching { "on" } else { "above" }
    )
}

#[test]
fn tunnel_girders_come_down() {
    let Some(path) = map() else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    let options = blam_geometry::Options {
        bsps: Some(vec![7, 8, 9]),
        ..Default::default()
    };
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(&cache, &options, &mut scene).unwrap();
    mcc_anim::devices::place_machines(&cache, &mut scene);
    blam_geometry::objects::add_collision(&cache, &mut scene);
    let data = Arc::new(blam_script::ScriptData::read(&cache).unwrap());
    setup::mark_dynamic(&mut scene, &data);
    let mut rt = setup::runtime(&cache, &scene, Arc::clone(&data), Config { editor: true, ..Default::default() });
    let poser = MachinePoser::load(&cache, rt.objects.info.iter().filter(|o| o.scene.is_some()).filter_map(|o| o.tag));
    let n = rt.objects.info.iter().filter(|o| o.scene.is_some()).count();
    let (mut solid, mut posed) = (vec![true; n], vec![false; n]);
    // BSPs 040, 100, 110 and 120: the run's zone sets together.
    let world = mcc_bike::build_world(&scene.collision, 0x390);

    // Every girder on the map, and the physics model's hinge.
    let girders: Vec<(String, usize)> = (0..n)
        .filter(|&e| rt.objects.info[e].tag_path.ends_with("tunnel_girder"))
        .map(|e| (rt.objects.info[e].name.clone().unwrap_or_default(), e))
        .collect();
    assert!(girders.len() >= SCRIPTED.len(), "{} girders", girders.len());
    for (name, e) in &girders {
        let info = &rt.objects.info[*e];
        let physics = info.physics.as_ref().unwrap_or_else(|| panic!("{name}: no physics model"));
        let hinge = physics.hinge.as_ref().unwrap_or_else(|| panic!("{name}: no hinge"));
        assert!((hinge.pivot - Vec3::new(-10.75, 0.0, 2.513)).length() < 0.01, "{name}: pivot {}", hinge.pivot);
        assert!((physics.friction - 0.8).abs() < 1e-3 && (physics.restitution - 0.35).abs() < 1e-3);
    }
    let scripted = |name: &str| SCRIPTED.iter().find(|(n, _)| *n == name).map(|(_, free)| *free);
    for (name, _) in SCRIPTED {
        assert!(girders.iter().any(|(n, _)| n == name), "no girder {name}");
    }

    assert!(rt.console("ins_trench_run"));
    let mut player = Player {
        at: Vec3::new(-263.64, -433.77, -5.05),
        zone_set: 0,
    };
    let mut ai = NoAi;
    // Per girder: the tick it let go, the most it has turned while
    // moving, the check when it came to rest.
    let mut let_go: Vec<Option<u64>> = vec![None; girders.len()];
    let mut most: Vec<f32> = vec![0.0; girders.len()];
    let mut rested: Vec<Option<String>> = vec![None; girders.len()];
    let mut freed: Vec<bool> = vec![false; girders.len()];
    let mut step = |rt: &mut Runtime, player: &mut Player, ticks: usize| {
        for _ in 0..ticks {
            rt.step(player, &mut ai, Some(&poser as &dyn DevicePoser));
            mcc_script::publish_collision(&rt.objects, &mut solid, &mut posed);
            for (k, (name, e)) in girders.iter().enumerate() {
                let s = &rt.objects.state[*e];
                let resting = match &s.motion {
                    Motion::Still => continue,
                    Motion::Swinging(w) => w.resting,
                    Motion::Tumbling(b) => {
                        freed[k] = true;
                        b.asleep
                    }
                    m => panic!("{name}: moves as {m:?}"),
                };
                let start = *let_go[k].get_or_insert(rt.tick);
                assert!(s.solid, "{name}: not solid while it moves");
                most[k] = most[k].max(turned(rt.objects.info[*e].base, s.transform));
                if resting && rested[k].is_none() {
                    let seconds = (rt.tick - start) as f32 / 30.0;
                    rested[k] = Some(check_rest(name, *e, rt, &scene, &world, seconds));
                }
            }
        }
    };
    let centre = |name: &str| {
        let v = scene.trigger_volumes.iter().find(|v| v.name == name).unwrap_or_else(|| panic!("{name}"));
        let left = v.up.cross(v.forward);
        v.position + v.forward * v.extents.x * 0.5 + left * v.extents.y * 0.5 + v.up * v.extents.z * 0.5
    };
    step(&mut rt, &mut player, 90);
    for v in [
        "trench_run_plate_100_trig",
        "trench_run_100_trig00",
        "trench_run_100_trig01",
        "trench_run_plate_100_hw_trig",
        "trench_run_100_trig04",
        "100_game_save_trig",
        "trench_run_100_trig06",
        "trench_run_100_trig07",
        "trench_run_100_trig08",
        "trench_run_game_save01",
    ] {
        player.at = centre(v);
        step(&mut rt, &mut player, 20);
    }
    // Through the first tunnel, a unit every two ticks (about 45 km/h).
    let mut y = -60.0f32;
    while y < 175.0 {
        player.at = Vec3::new(58.7, y, -34.0);
        step(&mut rt, &mut player, 2);
        y += 1.0;
    }
    // On to the second tunnel.
    for v in [
        "120_08_trench_encounter_110",
        "trench_run_plate_110_1_trig",
        "trench_run_plate_110_2_trig",
        "trench_run_plate_110_3_trig",
        "trench_run_plate_110_4_trig",
        "trench_run_plate_110_hw_trig",
        "trench_run_110_trig15",
        "trench_run_game_save03",
    ] {
        player.at = centre(v);
        step(&mut rt, &mut player, 20);
    }
    let mut from = player.at;
    for k in 1..=7 {
        let to = centre(&format!("tunnel02_trig0{k}"));
        let steps = (to - from).length().ceil().max(1.0) as usize;
        for s in 1..=steps {
            player.at = from.lerp(to, s as f32 / steps as f32);
            step(&mut rt, &mut player, 2);
        }
        from = to;
    }
    step(&mut rt, &mut player, (30.0 * (REST_WITHIN + 1.0)) as usize);

    for (k, (name, e)) in girders.iter().enumerate() {
        match scripted(name) {
            None => {
                // The scripts never touch it: it stays where it hangs.
                assert!(let_go[k].is_none(), "{name} came down but no script drops it");
                let s = &rt.objects.state[*e];
                assert!(s.transform == rt.objects.info[*e].base, "{name} moved");
                eprintln!("{name}: stays up (no script drops it)");
            }
            Some(free) => {
                assert!(let_go[k].is_some(), "{name} never came down");
                assert_eq!(freed[k], free, "{name}: fell free {}, the scripts say {free}", freed[k]);
                assert!(most[k] > 5.0, "{name} turned only {:.1} degrees as it came down", most[k]);
                let line = rested[k].as_deref().unwrap_or_else(|| panic!("{name} never came to rest: {:?}", rt.objects.state[*e].motion));
                let now = if rt.objects.state[*e].exists { "" } else { "; since gone down with its floor" };
                eprintln!("{line}{now}");
            }
        }
    }
}
