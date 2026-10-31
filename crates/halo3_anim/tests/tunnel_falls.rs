//! The girders that fall in the Warthog run's two tunnels land on the road
//! and stay there, solid. 120_halo's own scripts run headless
//! (`ins_trench_run`), the player is walked through the run's triggers and
//! both tunnels, and every `tunnel_girder` crate the scripts drop
//! (`fall_off 100_hw_ceiling02` ...) must end up resting on the road (not
//! under it), with the bike's collision world holding it where it lies.
//! Skips quietly when the map is not installed.
//!
//! One test only: the moved and removed objects are process-wide
//! (`halo3_core::objects`).

use std::path::PathBuf;
use std::sync::Arc;

use glam::{Vec2, Vec3};
use halo3_anim::MachinePoser;
use halo3_bike::physics::World;
use halo3_core::coords::{to_halo, to_render};
use halo3_script::objects::Motion;
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
    let dir = game_paths::halo3_maps()?;
    let p = dir.join("120_halo.map");
    p.exists().then_some(p)
}

/// The girders the tunnels' scripts drop (`trench_tunnel01` / `02`); the
/// scripts also send `100_hw_ceiling10` away with `tunnel_fall_away`,
/// which destroys it.
const DROPPED: [&str; 12] = [
    "100_hw_ceiling02",
    "100_hw_ceiling04",
    "100_hw_ceiling08",
    "100_hw_ceiling09",
    "100_hw_ceiling13",
    "100_hw_ceiling14",
    "110_hw_ceiling02",
    "110_hw_ceiling05",
    "110_hw_ceiling08",
    "110_hw_ceiling10",
    "110_hw_ceiling11",
    "110_hw_ceiling14",
];

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


/// Checks a girder the moment it lands: resting on the road, not under it,
/// solid, and in the bike's collision world where it lies. Returns a line
/// for the log.
fn check_landed(name: &str, e: usize, rt: &Runtime, scene: &blam_geometry::Scene, world: &World) -> String {
    let (info, s) = (&rt.objects.info[e], &rt.objects.state[e]);
    assert!(s.exists && s.solid, "{name} landed but is not solid");
    let motion = s.transform * info.base.inverse();
    let fell = info.base.w_axis.z - s.transform.w_axis.z;
    assert!(fell > 5.0, "{name} fell only {fell:.2} wu");
    // It may tip over its width as it settles, but never turns.
    let (along0, along1) = (info.base.x_axis.truncate(), s.transform.x_axis.truncate());
    assert!(along0.truncate().perp_dot(along1.truncate()).abs() < 1e-3 * along0.length(), "{name} turned as it fell");
    let tilt = info.base.z_axis.truncate().angle_between(s.transform.z_axis.truncate()).to_degrees();
    // Where it lies, Halo frame: the corners and triangles of its collision.
    let corners: Vec<Vec3> = info.contacts.iter().map(|p| motion.transform_point3(*p)).collect();
    let low = corners.iter().map(|p| p.z).fold(f32::INFINITY, f32::min);
    let tris: Vec<[Vec3; 3]> = scene
        .collision
        .triangles
        .iter()
        .filter(|t| t.source.object() == Some(e as u32))
        .map(|t| t.points.map(|p| motion.transform_point3(p)))
        .collect();

    // Resting on the road: with the girder itself taken out of the bike's
    // world for a moment, the road straight under each corner (looked for
    // from 3 wu up) is never above it, and is right at it under some.
    halo3_core::objects::set_collision_removed(e as u32, true);
    let mut on = Vec::new();
    for p in &corners {
        let Some(road) = down(world, *p + Vec3::Z * 3.0, 50.0) else { continue };
        assert!(road < p.z + 0.03, "{name}: a corner at {p} is {:.2} wu into the road", road - p.z);
        if p.z - road < 0.03 {
            on.push(*p);
        }
    }
    halo3_core::objects::set_collision_removed(e as u32, false);
    let touching = on.len();
    assert!(touching >= 1, "{name}: no corner on the road (lowest at {low:.2})");
    // Held up under its middle, not balanced on one end: it touches on
    // both sides of its middle or right under it; tipped over, at two
    // places well apart.
    let c = s.transform.w_axis.truncate();
    let along = along1.normalize();
    let span = on
        .iter()
        .map(|p| (*p - c).dot(along))
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), d| (a.min(d), b.max(d)));
    assert!(
        span.0 < 0.5 && span.1 > -0.5 && (tilt < 1.0 || span.1 - span.0 > 2.0),
        "{name}: tipped {tilt:.1} degrees, rests only between {:.1} and {:.1} along its length",
        span.0,
        span.1
    );
    // Not under the road just beside its middle (where no girder is; the
    // plates either side can sit a little higher or lower).
    let side = s.transform.transform_vector3(Vec3::Y).normalize();
    let beside: Vec<f32> = [-1.5f32, 1.5].iter().filter_map(|d| down(world, c + side * *d + Vec3::Z * 6.0, 30.0)).collect();
    assert!(!beside.is_empty(), "{name}: no road beside it");
    let road = beside.iter().copied().fold(f32::INFINITY, f32::min);
    let middle = corners
        .iter()
        .filter(|p| (p.truncate() - c.truncate()).length() < 1.0)
        .map(|p| p.z)
        .fold(f32::INFINITY, f32::min);
    assert!(middle > road - 0.3, "{name}: its middle is at {middle:.2}, under the road beside it at {road:.2}");

    // The bike's world has it where it lies: straight down onto its middle
    // the bike meets its top, as drawn at rest ...
    let top = height(&tris, c.x, c.y, c.z + 10.0).unwrap_or_else(|| panic!("{name}: nothing at its middle"));
    let hit = down(world, Vec3::new(c.x, c.y, top + 3.0), 6.0).unwrap_or_else(|| panic!("{name}: the bike's world is empty over it"));
    assert!((hit - top).abs() < 0.05, "{name}: lies with its top at {top:.2}, the bike meets {hit:.2}");
    // ... and nothing is left where it hung.
    let hung = info.base.w_axis.z;
    let up = world.raycast_far(to_render(Vec3::new(c.x, c.y, top + 0.5)), Vec3::Y, 40.0 * 3.048).map(|h| to_halo(h.point).z);
    assert!(up.is_none_or(|z| z > hung + 1.4), "{name}: still collides where it hung (hit at {up:?})");
    format!("{name}: fell {fell:.1} wu, tipped {tilt:.1} degrees; middle at {middle:.2} on the road at {road:.2}; lowest corner {low:.2}, {touching} corners touching between {:.1} and {:.1} along it; top {top:.2}", span.0, span.1)
}

#[test]
fn tunnel_girders_land_on_the_road() {
    let Some(path) = map() else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    let options = blam_geometry::Options {
        bsps: Some(vec![7, 8, 9]),
        ..Default::default()
    };
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(&cache, &options, &mut scene).unwrap();
    halo3_anim::devices::place_machines(&cache, &mut scene);
    blam_geometry::objects::add_collision(&cache, &mut scene);
    let data = Arc::new(blam_script::ScriptData::read(&cache).unwrap());
    setup::mark_dynamic(&mut scene, &data);
    let mut rt = setup::runtime(&cache, &scene, Arc::clone(&data), Config { editor: true, ..Default::default() });
    let poser = MachinePoser::load(&cache, rt.objects.info.iter().filter(|o| o.scene.is_some()).filter_map(|o| o.tag));
    let n = rt.objects.info.iter().filter(|o| o.scene.is_some()).count();
    let (mut solid, mut posed) = (vec![true; n], vec![false; n]);
    // BSPs 040, 100, 110 and 120: the run's zone sets together (the second
    // tunnel runs on into 120).
    let world = halo3_bike::build_world(&scene.collision, 0x390);
    let ground = rt.objects.ground.as_ref().expect("ground under the girders");
    eprintln!("ground: {} triangles under the crates that can land", ground.len());

    let girders: Vec<(&str, usize)> = DROPPED
        .iter()
        .map(|&name| (name, rt.objects.by_name[data.object_index(name).unwrap()].unwrap() as usize))
        .collect();
    for &(name, e) in &girders {
        let info = &rt.objects.info[e];
        assert!(info.tag_path.ends_with("tunnel_girder"), "{name} is {}", info.tag_path);
        assert!(!info.contacts.is_empty(), "{name} cannot land");
    }
    // Floor plates still tumble away into the chasm.
    let plate = rt.objects.by_name[data.object_index("100_001f").unwrap()].unwrap() as usize;
    assert!(rt.objects.info[plate].contacts.is_empty(), "a floor plate must not land");

    assert!(rt.console("ins_trench_run"));
    let mut player = Player {
        at: Vec3::new(-263.64, -433.77, -5.05),
        zone_set: 0,
    };
    let mut ai = NoAi;
    // Per girder: its check when it landed; whether it ever tumbled.
    let mut landed: Vec<Option<String>> = vec![None; girders.len()];
    let mut tumbled: Vec<bool> = vec![false; girders.len()];
    let mut step = |rt: &mut Runtime, player: &mut Player, ticks: usize| {
        for _ in 0..ticks {
            rt.step(player, &mut ai, Some(&poser as &dyn DevicePoser));
            halo3_script::publish_collision(&rt.objects, &mut solid, &mut posed);
            for (k, &(name, e)) in girders.iter().enumerate() {
                match rt.objects.state[e].motion {
                    Motion::Landed if landed[k].is_none() => landed[k] = Some(check_landed(name, e, rt, &scene, &world)),
                    Motion::Falling { lands: false, .. } => tumbled[k] = true,
                    Motion::Falling { .. } | Motion::Settling { .. } => assert!(!rt.objects.state[e].solid, "{name}: a falling girder is not solid"),
                    _ => {}
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
    // On to the second tunnel: the plate folders that bring its girders in,
    // its triggers in order, and the road between them.
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
    // Long enough for the last one to come down (25 wu: 4 s).
    step(&mut rt, &mut player, 30 * 6);

    for (k, &(name, e)) in girders.iter().enumerate() {
        assert!(!tumbled[k], "{name} tumbled away instead of dropping onto the road");
        let line = landed[k].as_deref().unwrap_or_else(|| panic!("{name} never landed: {:?}", rt.objects.state[e].motion));
        // The run's timers drop the first tunnel's floor plates behind the
        // player later on; a girder lying on them goes down with them.
        let now = match rt.objects.state[e].motion {
            Motion::Landed => "still there",
            _ if !rt.objects.state[e].exists => "since gone down with its floor",
            _ => "since falling with its floor",
        };
        eprintln!("{line}; {now}");
    }
    // The second tunnel's girders lie where they landed.
    for &(name, e) in &girders[6..] {
        assert!(matches!(rt.objects.state[e].motion, Motion::Landed), "{name} no longer lies on the road");
    }
}
