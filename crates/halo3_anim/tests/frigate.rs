//! The Forward Unto Dawn at the end of 120_halo's Warthog run
//! (`trench_frigate`, a device machine): standing in its animation's pose
//! at rest (`devices::place_machines`), its open hangar faces the run's
//! last stretch, the map's own end trigger (`end_trilogy01`) sits at the
//! hangar's mouth, and the hangar has a floor and walls to land on.
//! Also the two starts on the run (`--insertion trench_run` and our
//! `trench_end`), the end of the mission at the hangar, and the bike's jump
//! into it. Skips quietly when the map is not installed.
use std::path::PathBuf;
use std::sync::Arc;

use glam::Vec3;
use halo3_bike::physics::{Bike, Controls};
use halo3_core::coords::{to_halo, to_render};
use halo3_script::vm::{Config, WorldApi};
use halo3_script::{NoAi, setup};

fn map() -> Option<PathBuf> {
    let dir = std::env::var_os("HALO3_MAPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps")
    });
    let p = dir.join("120_halo.map");
    p.exists().then_some(p)
}

/// The nearest hit along a ray, distance in world units.
fn ray(tris: &[[Vec3; 3]], o: Vec3, d: Vec3) -> Option<f32> {
    let mut best: Option<f32> = None;
    for t in tris {
        let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
        let p = d.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-9 {
            continue;
        }
        let s = o - t[0];
        let u = s.dot(p) / det;
        let q = s.cross(e1);
        let v = d.dot(q) / det;
        let t = e2.dot(q) / det;
        if u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t > 0.0 && best.is_none_or(|b| t < b) {
            best = Some(t);
        }
    }
    best
}

#[test]
fn frigate_hangar_faces_the_run() {
    let Some(path) = map() else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    // Zone set 120_110_120: BSPs 110 and 120, the end of the run.
    let options = blam_geometry::Options {
        bsps: Some(vec![8, 9]),
        ..Default::default()
    };
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(&cache, &options, &mut scene).unwrap();
    let frigate = scene.objects.iter().position(|o| o.name.as_deref() == Some("trench_frigate")).expect("trench_frigate");
    let placed = scene.objects[frigate].transform;
    assert!(halo3_anim::devices::place_machines(&cache, &mut scene) > 0);
    blam_geometry::objects::add_collision(&cache, &mut scene);
    let o = &scene.objects[frigate];
    assert_eq!(o.kind, blam_geometry::Kind::Machine);
    // Its animation turns it a quarter turn about its own up axis.
    let turn = (placed.inverse() * o.transform).transform_vector3(Vec3::Y);
    assert!(turn.dot(Vec3::X) > 0.99, "frame 0 of device:position turns +Y to {turn}");

    // The hangar (model space): a bay 13 wu wide and 8 high under the
    // stern, floor at z -20, open towards -Y at y -36.
    let to_model = o.transform.inverse();
    let out = o.transform.transform_vector3(Vec3::NEG_Y).normalize();
    // The run's last stretch heads -X (trench_run_120_trig14 .. 20, x 412
    // down to 366), so the bay must open towards +X.
    assert!(out.x > 0.95, "the hangar opens towards {out}");

    let volume = |name: &str| {
        let v = scene.trigger_volumes.iter().find(|v| v.name == name).unwrap_or_else(|| panic!("{name}"));
        let left = v.up.cross(v.forward);
        v.position + v.forward * v.extents.x * 0.5 + left * v.extents.y * 0.5 + v.up * v.extents.z * 0.5
    };
    // The map's end trigger: the bay's mouth, on the centre line.
    let end = to_model.transform_point3(volume("end_trilogy01"));
    assert!(end.x.abs() < 3.0 && (-46.0..-36.0).contains(&end.y) && (-20.0..-10.0).contains(&end.z), "end_trilogy01 at {end} (model)");

    // Collision: the bay as drawn.
    let tris: Vec<[Vec3; 3]> = scene.collision.of_bsps(0x300).triangles.iter().map(|t| t.points).collect();
    let world = |p: Vec3| o.transform.transform_point3(p);
    let dir = |d: Vec3| o.transform.transform_vector3(d).normalize();
    for y in [-34.0f32, -25.0, -15.0, -5.0] {
        let at = world(Vec3::new(0.0, y, -17.0));
        let floor = ray(&tris, at, dir(Vec3::NEG_Z)).unwrap_or_else(|| panic!("no hangar floor at y {y}"));
        assert!((2.5..3.5).contains(&floor), "hangar floor {floor} wu under y {y}");
        for side in [Vec3::X, Vec3::NEG_X] {
            let wall = ray(&tris, at, dir(side)).unwrap_or_else(|| panic!("no hangar wall at y {y}"));
            assert!((6.0..7.5).contains(&wall), "hangar wall {wall} wu at y {y}");
        }
    }
    // Open mouth: from outside, nothing in the way until deep inside.
    let from = world(Vec3::new(0.0, -60.0, -17.0));
    let clear = ray(&tris, from, dir(Vec3::Y)).unwrap_or(f32::INFINITY);
    assert!(clear > 40.0, "something {clear} wu into the hangar mouth");

    // The end trigger fills the mouth from floor to ceiling (with the
    // placement's 2.45 degree pitch composed about the world's axes; the
    // other order puts it half inside the stern).
    let v = scene.trigger_volumes.iter().find(|v| v.name == "end_trilogy01").unwrap();
    let (lo, hi) = (to_model.transform_point3(v.position).z, to_model.transform_point3(v.position + v.up * v.extents.z).z);
    assert!((lo - -20.0).abs() < 1.0 && (hi - -12.0).abs() < 1.0, "end_trilogy01 spans model z {lo:.2} .. {hi:.2}, the mouth -20 .. -12");

    // The run's last stretch: a ramp up to a lip, then the jump over a
    // drop to the mouth, the stern high over the lip.
    let mouth = world(Vec3::new(0.0, -36.0, -20.0));
    let ground: Vec<[Vec3; 3]> = scene
        .collision
        .of_bsps(0x300)
        .triangles
        .iter()
        .filter(|t| t.source.object() != Some(frigate as u32))
        .map(|t| t.points)
        .collect();
    let hull: Vec<[Vec3; 3]> = scene
        .collision
        .triangles
        .iter()
        .filter(|t| t.source.object() == Some(frigate as u32))
        .map(|t| t.points)
        .collect();
    let start = volume("trench_run_120_trig14");
    let mut lip = None;
    for k in 0..=80 {
        let p = start.lerp(mouth, k as f32 / 80.0);
        let top = mouth.z + 20.0;
        if let Some(d) = ray(&ground, Vec3::new(p.x, p.y, top), Vec3::NEG_Z)
            && top - d > mouth.z - 3.0
        {
            lip = Some(Vec3::new(p.x, p.y, top - d));
        }
    }
    let lip = lip.expect("no ground at the end of the run");
    let gap = lip.x - mouth.x;
    assert!((20.0..40.0).contains(&gap) && (2.0..10.0).contains(&(lip.z - mouth.z)), "lip {lip}, mouth {mouth}");
    let over = ray(&hull, lip + Vec3::Z * 0.1, Vec3::Z).map(|d| d + 0.1);
    assert!(over.is_none_or(|h| h > 3.0), "the stern is {over:?} wu over the lip at {lip}");
    eprintln!(
        "frigate hangar opens towards {out}; end_trilogy01 at {end} (model z {lo:.2} .. {hi:.2}); lip {lip}, mouth {mouth}: {gap:.1} wu across, {:.1} down; stern {over:.1?} wu over the lip",
        lip.z - mouth.z
    );
}

/// The player as the scripts see him.
struct Player {
    at: Vec3,
    zone_set: usize,
    teleports: Vec<Vec3>,
    riding: bool,
}

impl WorldApi for Player {
    fn player_position(&self) -> Option<Vec3> {
        Some(self.at)
    }
    fn teleport_player(&mut self, position: Vec3, _yaw: f32) {
        self.at = position;
        self.teleports.push(position);
    }
    fn current_zone_set(&self) -> Option<usize> {
        Some(self.zone_set)
    }
    fn switch_zone_set(&mut self, zone_set: usize) {
        self.zone_set = zone_set;
    }
    fn effect(&mut self, _tag: &str, _at: Vec3) {}
    fn player_in_vehicle(&self) -> bool {
        self.riding
    }
}

/// The scene of the Warthog run's BSPs (and the hallway it starts from),
/// machines posed, object collision in.
fn run_scene(cache: &blam_cache::CacheFile, bsps: Vec<u32>) -> blam_geometry::Scene {
    let options = blam_geometry::Options {
        bsps: Some(bsps),
        ..Default::default()
    };
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(cache, &options, &mut scene).unwrap();
    halo3_anim::devices::place_machines(cache, &mut scene);
    blam_geometry::objects::add_collision(cache, &mut scene);
    scene
}

/// `--insertion trench_end`: the last stretch, in the last zone set, on
/// the ground facing the frigate; riding into the hangar's mouth ends the
/// mission (the map's own `mission_halo`), and the outro's teleports never
/// happen.
#[test]
fn trench_end_starts_on_the_last_stretch_and_the_hangar_ends_the_mission() {
    let Some(path) = map() else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    let mut scene = run_scene(&cache, vec![8, 9]);
    let data = Arc::new(blam_script::ScriptData::read(&cache).unwrap());
    setup::mark_dynamic(&mut scene, &data);

    let ins = setup::insertion(&data, &scene, "trench_end").expect("trench_end");
    assert_eq!(ins.script, "ins_trench_run");
    let (at, yaw) = ins.start.unwrap();
    let start = ins.start_override.clone().unwrap();
    let last = scene.zone_sets.iter().position(|z| z.name == "120_110_120").unwrap();
    assert_eq!(start.zone_set, Some(last));
    // On the floor, facing the hangar (the run's last straight heads -X).
    let floor = setup::floor_under(&scene, at).expect("floor under trench_end");
    assert!((0.0..1.0).contains(&(at.z - floor)), "trench_end at {at}, floor {floor}");
    assert!(yaw.cos() < -0.99, "trench_end faces {} degrees", yaw.to_degrees());
    let lip_distance = at.x - 370.6;
    assert!((40.0..120.0).contains(&lip_distance), "trench_end is {lip_distance} wu before the ramp");

    let mut rt = setup::runtime(&cache, &scene, Arc::clone(&data), Config { editor: true, ..Default::default() });
    assert!(rt.console(&ins.script));
    let mut player = Player {
        at: Vec3::ZERO,
        zone_set: 0,
        teleports: Vec::new(),
        riding: false,
    };
    let mut ai = NoAi;
    for _ in 0..15 {
        rt.step(&mut player, &mut ai, None);
    }
    setup::apply_start(&mut rt, &mut player, &mut ai, None, &start, 15);
    assert_eq!(player.zone_set, last);
    assert_eq!(player.at, at);
    // The run's kill volumes are off (`object_management`), the frigate's
    // lift shafts are made (zone set 6), the mission is not over.
    let volume_index = |name: &str| rt.volumes.iter().position(|v| v.name == name).unwrap();
    for kill in ["kill_120_04", "kill_120_05"] {
        let i = volume_index(kill);
        assert_eq!(rt.kill_volumes.iter().rev().find(|(v, _)| *v == i).map(|(_, on)| *on), Some(false), "{kill}");
    }
    for _ in 0..90 {
        rt.step(&mut player, &mut ai, None);
    }
    assert_eq!(rt.mission_over, None);
    let shaft = rt.objects.by_name[data.object_index("trench_f_back").unwrap()].unwrap() as usize;
    assert!(rt.objects.state[shaft].exists, "trench_f_back not created");

    // Down the last straight: nothing ends it until the hangar's mouth.
    for x in [430.0f32, 400.0, 375.0, 360.0] {
        player.at = Vec3::new(x, 963.5, floor_or(&scene, x) + 1.0);
        for _ in 0..10 {
            rt.step(&mut player, &mut ai, None);
        }
        assert_eq!(rt.mission_over, None, "ended at x {x}");
    }
    let v = scene.trigger_volumes.iter().find(|v| v.name == "end_trilogy01").unwrap();
    let left = v.up.cross(v.forward);
    player.at = v.position + v.forward * v.extents.x * 0.5 + left * v.extents.y * 0.5 + v.up * 1.5;
    let teleports = player.teleports.len();
    for _ in 0..10 {
        rt.step(&mut player, &mut ai, None);
    }
    assert_eq!(rt.mission_over.as_deref(), Some("game_award_level_complete_achievements"));
    // The outro stops there: no cutscene teleports, the frigate stays.
    for _ in 0..60 {
        rt.step(&mut player, &mut ai, None);
    }
    assert_eq!(player.teleports.len(), teleports, "teleported after the end");
    let frigate = rt.objects.by_name[data.object_index("trench_frigate").unwrap()].unwrap() as usize;
    assert!(rt.objects.state[frigate].exists && !rt.objects.state[frigate].hidden);

    // On the bike, the air over the gap (`end_trilogy03`) ends it as it
    // does for the map's hidden Mongoose; on foot the same spot does not.
    let v = scene.trigger_volumes.iter().find(|v| v.name == "end_trilogy03").unwrap();
    let left = v.up.cross(v.forward);
    let gap = v.position + v.forward * v.extents.x * 0.5 + left * v.extents.y * 0.5 + v.up * v.extents.z * 0.5;
    for riding in [false, true] {
        let mut rt = setup::runtime(&cache, &scene, Arc::clone(&data), Config { editor: true, ..Default::default() });
        assert!(rt.console(&ins.script));
        let mut player = Player {
            at: Vec3::ZERO,
            zone_set: 0,
            teleports: Vec::new(),
            riding,
        };
        for _ in 0..15 {
            rt.step(&mut player, &mut ai, None);
        }
        setup::apply_start(&mut rt, &mut player, &mut ai, None, &start, 15);
        player.at = gap;
        for _ in 0..10 {
            rt.step(&mut player, &mut ai, None);
        }
        assert_eq!(rt.mission_over.is_some(), riding, "riding {riding} over the gap");
    }
}

/// The floor of the last straight at `x` (y 963.5).
fn floor_or(scene: &blam_geometry::Scene, x: f32) -> f32 {
    setup::floor_under(scene, Vec3::new(x, 963.5, -50.0)).unwrap_or(-70.0)
}

/// `--insertion trench_run` starts on the run's start flag, on the floor,
/// facing up the canyon the run starts in (towards its first trigger), with
/// ground ahead (the flag itself faces +X, into the canyon's side).
#[test]
fn trench_run_starts_on_the_floor_facing_the_run() {
    let Some(path) = map() else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    // Zone set 120_04_100_110.
    let scene = run_scene(&cache, vec![4, 7, 8]);
    let data = blam_script::ScriptData::read(&cache).unwrap();
    let ins = setup::insertion(&data, &scene, "trench_run").expect("trench_run");
    assert_eq!(ins.script, "ins_trench_run");
    // Turned by an override with no zone set of its own.
    assert!(ins.start_override.as_ref().is_some_and(|s| s.zone_set.is_none() && s.at == ins.start.unwrap().0));
    let (at, yaw) = ins.start.unwrap();
    let flag = data.cutscene_flags.iter().find(|f| f.name == "trench_run_player0").unwrap();
    assert_eq!(at, Vec3::from(flag.position));
    let floor = setup::floor_under(&scene, at).expect("floor under trench_run");
    assert!((-0.5..1.5).contains(&(at.z - floor)), "trench_run at {at}, floor {floor}");
    // Up the canyon: ground all the way to the first trigger, 60 wu on.
    let facing = glam::Vec2::new(yaw.cos(), yaw.sin());
    assert!(facing.y > 0.99, "trench_run faces {} degrees", yaw.to_degrees());
    let mut last = floor;
    for r in (5..=60).step_by(5) {
        let p = at + (facing * r as f32).extend(0.0);
        let z = setup::floor_under(&scene, p + Vec3::Z * 6.0).unwrap_or_else(|| panic!("no ground {r} wu ahead of trench_run"));
        assert!((z - last).abs() < 4.0, "a {:.1} wu step {r} wu ahead of trench_run", z - last);
        last = z;
    }
    // The flag's own facing looks at the canyon's side: no ground 20 wu on.
    let side = at + Vec3::X * 20.0;
    assert!(setup::floor_under(&scene, side + Vec3::Z * 6.0).is_none_or(|z| z < floor - 20.0));
}

/// From `trench_end`, full throttle straight down the last straight: the
/// bike leaves the ramp and flies through `end_trilogy03`, over the gap,
/// which ends the mission for a vehicle (`PLAYER_RIDE_STANDS_IN_FOR`).
/// Where it goes after that is printed: at the bike's top speed it reaches
/// the hangar's mouth just under its floor.
#[test]
fn bike_jumps_from_trench_end_towards_the_hangar() {
    let Some(path) = map() else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    let scene = run_scene(&cache, vec![8, 9]);
    let data = blam_script::ScriptData::read(&cache).unwrap();
    let (at, yaw) = setup::insertion(&data, &scene, "trench_end").unwrap().start.unwrap();
    let world = halo3_bike::build_world(&scene.collision, 0x300);
    let mut bike = Bike::dropped(&world, to_render(at) + glam::Vec3::Y, halo3_bike::heading_of_halo_yaw(yaw));
    let volume = |name: &str| scene.trigger_volumes.iter().find(|v| v.name == name).unwrap();
    let (gap, mouth) = (volume("end_trilogy03"), volume("end_trilogy01"));
    let frigate = scene.objects.iter().find(|o| o.name.as_deref() == Some("trench_frigate")).unwrap();
    let to_model = frigate.transform.inverse();
    let (mut over_gap, mut at_mouth, mut top_speed, mut path) = (None, None, 0.0f32, Vec::new());
    let dt = 1.0 / 60.0;
    for step in 0..(60 * 25) {
        bike.step(&world, Controls { throttle: 1.0, ..Default::default() }, dt);
        let p = to_halo(bike.position);
        top_speed = top_speed.max(bike.speed());
        if step % 30 == 0 {
            path.push(format!("({:.0} {:.0} {:.1})", p.x, p.y, p.z));
        }
        // The rider's hips, a little over the bike.
        let hips = p + Vec3::Z * 0.5;
        if over_gap.is_none() && gap.contains(hips) {
            over_gap = Some((step as f32 * dt, p, bike.speed() * 3.6));
        }
        if at_mouth.is_none() && mouth.contains(hips) {
            at_mouth = Some((step as f32 * dt, p));
        }
        if p.z < -120.0 {
            break;
        }
    }
    let p = to_halo(bike.position);
    eprintln!(
        "bike from trench_end: top speed {:.0} km/h; over the gap (end_trilogy03) {over_gap:?}; at the mouth (end_trilogy01) {at_mouth:?}; ends at {p} (frigate model {}); path {}",
        top_speed * 3.6,
        to_model.transform_point3(p),
        path.join(" ")
    );
    let (t, _, _) = over_gap.expect("the bike never flew over the gap (end_trilogy03)");
    assert!(t < 20.0, "{t} s to the gap");
}
