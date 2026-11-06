//! The Warthog run's first plate blowing up, end to end with no window:
//! 120_halo's own scripts (`ins_trench_run`, the player driven into
//! `trench_run_100_trig00`) damage plate `100_001c`'s `destroy` section,
//! which asks for its chunks and the trench's `plate_explosion`; the
//! effect player then plays that effect from its tags: the flash, the
//! fireball, the ejecta with their contrails and the sounds. Skips quietly
//! when the map is not installed.

use std::sync::Arc;

use bevy::math::{Mat3, Vec3};
use blam_effect::Library;
use mcc_effects::sim::Sim;
use mcc_script::vm::{Config, WorldApi};
use mcc_script::{NoAi, setup};

const PLATE_EXPLOSION: &str = r"levels\solo\120_halo\fx\trench_run\plate_explosion\plate_explosion";

#[derive(Debug, Clone)]
struct Request {
    name: String,
    at: Vec3,
    frame: Mat3,
    source: Option<u32>,
}

struct World {
    player: Vec3,
    zone_set: usize,
    effects: Vec<Request>,
}

impl WorldApi for World {
    fn player_position(&self) -> Option<Vec3> {
        Some(self.player)
    }
    fn teleport_player(&mut self, position: Vec3, _yaw: f32) {
        self.player = position;
    }
    fn current_zone_set(&self) -> Option<usize> {
        Some(self.zone_set)
    }
    fn switch_zone_set(&mut self, zone_set: usize) {
        self.zone_set = zone_set;
    }
    fn effect(&mut self, tag: &str, at: Vec3) {
        self.effects.push(Request { name: tag.to_string(), at, frame: Mat3::IDENTITY, source: None });
    }
    fn effect_placed(&mut self, tag: &str, at: Vec3, frame: Mat3, source: Option<u32>) {
        self.effects.push(Request { name: tag.to_string(), at, frame, source });
    }
}

#[test]
fn first_plate_blows_up_with_its_own_effect() {
    let Some(path) = game_paths::halo3_maps().map(|d| d.join("120_halo.map")).filter(|p| p.exists()) else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    let options = blam_geometry::Options {
        bsps: Some(vec![7, 8, 9]),
        collision: false,
        ..Default::default()
    };
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(&cache, &options, &mut scene).unwrap();
    let data = Arc::new(blam_script::ScriptData::read(&cache).unwrap());
    setup::mark_dynamic(&mut scene, &data);
    let config = Config { editor: true, ..Default::default() };
    let mut rt = setup::runtime(&cache, &scene, Arc::clone(&data), config);
    assert!(rt.console("ins_trench_run"));
    let mut world = World { player: Vec3::new(-263.64, -433.77, -5.05), zone_set: 0, effects: Vec::new() };
    let mut ai = NoAi;
    for _ in 0..90 {
        rt.step(&mut world, &mut ai, None);
    }
    let centre = |name: &str| {
        let v = scene.trigger_volumes.iter().find(|v| v.name == name).unwrap();
        let left = v.up.cross(v.forward);
        v.position + v.forward * v.extents.x * 0.5 + left * v.extents.y * 0.5 + v.up * v.extents.z * 0.5
    };
    world.effects.clear();
    world.player = centre("trench_run_100_trig00");
    for _ in 0..30 {
        rt.step(&mut world, &mut ai, None);
    }
    // `blow_away 100_001c`: the plate's `destroy` section plays its chunks
    // and the plate explosion, both at the plate, upright, from the plate.
    let plate = rt.objects.by_name[data.object_index("100_001c").unwrap()].unwrap() as usize;
    let plate_tag = rt.objects.info[plate].tag;
    let at = rt.objects.position(plate);
    let from_plate: Vec<&Request> = world.effects.iter().filter(|r| r.source == plate_tag && r.at == at).collect();
    let names: Vec<&str> = from_plate.iter().map(|r| r.name.rsplit('\\').next().unwrap()).collect();
    assert!(names.contains(&"plate_explosion"), "{names:?} of {:?}", world.effects);
    assert!(names.iter().any(|n| n.ends_with("_explosion_chunks") || n.ends_with("_explosion01")), "{names:?}");
    let req = from_plate.iter().find(|r| r.name.eq_ignore_ascii_case(PLATE_EXPLOSION)).unwrap();
    assert!(req.frame.z_axis.z > 0.9, "the plate lies flat: {:?}", req.frame);

    // Play it.
    let roots: Vec<u32> = world.effects.iter().filter_map(|r| cache.tags.find("effe", &r.name)).map(|t| t.index).collect();
    let lib = Arc::new(Library::load(&cache, &roots));
    assert!(lib.errors.is_empty(), "{:?}", lib.errors);
    let mut sim = Sim::new(lib.clone());
    let tag = lib.by_name[&PLATE_EXPLOSION.to_ascii_lowercase()];
    sim.spawn(tag, req.at, req.frame, req.source).unwrap();
    let particle = |sim: &Sim, short: &str| -> usize {
        sim.systems
            .iter()
            .filter(|s| cache.tags.get(s.particle).unwrap().name().ends_with(short))
            .map(|s| s.particles.len())
            .sum()
    };
    sim.step(1.0 / 30.0);
    sim.step(1.0 / 30.0);
    // The flash: one quad; the fireball: 50 particles leaving upward.
    assert_eq!(particle(&sim, "explosive_flash"), 1);
    assert_eq!(particle(&sim, "fireball_human_large"), 50);
    let sounds: Vec<&str> = sim.sounds.iter().map(|(t, _)| cache.tags.get(*t).unwrap().name().rsplit('\\').next().unwrap()).collect();
    assert_eq!(sounds, ["plate_expl", "plate_expl_verb"]);
    let fire = sim.systems.iter().find(|s| cache.tags.get(s.particle).unwrap().name().ends_with("fireball_human_large")).unwrap();
    let rising = fire.particles.iter().filter(|p| p.velocity.z > 0.0).count();
    assert!(rising >= 45, "{rising} of 50 fireball particles rise");
    // Five to eight ejecta, each carrying the ejecta effect and its contrail.
    sim.step(1.0 / 30.0);
    let ejecta = particle(&sim, r"plate_explosion\ejecta");
    assert!((5..=8).contains(&ejecta), "{ejecta} ejecta");
    for _ in 0..10 {
        sim.step(1.0 / 30.0);
    }
    assert!(sim.contrails.len() >= ejecta, "{} contrails for {ejecta} ejecta", sim.contrails.len());
    assert!(sim.contrails.iter().all(|c| c.points.len() >= 2));
    let first = sim.contrails[0].points.first().unwrap().position;
    let head = sim.contrails[0].points.last().unwrap().position;
    assert!(first.distance(head) > 2.0, "the contrail follows its ejecta");
    // After 2 s the fireball has burnt out (lifespans 0.5 to 1.5 s); the
    // effect lasts its 5 s hold and then ends.
    for _ in 0..60 {
        sim.step(1.0 / 30.0);
    }
    assert_eq!(particle(&sim, "fireball_human_large"), 0);
    for _ in 0..(30 * 8) {
        sim.step(1.0 / 30.0);
    }
    assert!(sim.instances.iter().all(|i| i.effect != tag), "the plate explosion ended");
}

/// Prints where the run's first triggers and the plates they blow up are,
/// for placing headless `--shot` views (`cargo test -- --ignored
/// --nocapture where_things_are`).
#[test]
#[ignore]
fn where_things_are() {
    let Some(path) = game_paths::halo3_maps().map(|d| d.join("120_halo.map")).filter(|p| p.exists()) else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    let options = blam_geometry::Options { bsps: Some(vec![7, 8, 9]), collision: false, ..Default::default() };
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(&cache, &options, &mut scene).unwrap();
    for name in ["120_08_trench_encounter_100", "trench_run_100_trig00", "trench_run_100_trig01", "trench_run_100_trig03", "trench_run_100_trig04", "trench_run_110_trig01"] {
        let v = scene.trigger_volumes.iter().find(|v| v.name == name).unwrap();
        let left = v.up.cross(v.forward);
        let c = v.position + v.forward * v.extents.x * 0.5 + left * v.extents.y * 0.5 + v.up * v.extents.z * 0.5;
        println!("volume {name}: centre {c:.2} extents {:.2}", v.extents);
    }
    for name in ["100_001c", "100_003a", "100_004d", "100_003e", "100_008a", "100_010a", "100_012b", "110_012f"] {
        if let Some(o) = scene.objects.iter().find(|o| o.name.as_deref() == Some(name)) {
            println!("object {name}: {:.2} {}", o.transform.w_axis.truncate(), o.tag_path);
        }
    }
    let data = blam_script::ScriptData::read(&cache).unwrap();
    for set in data.point_sets.iter().filter(|s| s.name.starts_with("big_explosions") || s.name.starts_with("blow_out")) {
        for p in &set.points {
            println!("point {}/{}: {:.2?}", set.name, p.name, p.position);
        }
    }
}

/// Prints what the plate explosion's emitters draw a moment in (debugging
/// aid: `cargo test -- --ignored --nocapture plate_explosion_quads`).
#[test]
#[ignore]
fn plate_explosion_quads() {
    let Some(path) = game_paths::halo3_maps().map(|d| d.join("120_halo.map")).filter(|p| p.exists()) else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    let tag = cache.tags.find("effe", PLATE_EXPLOSION).unwrap().index;
    let lib = Arc::new(Library::load(&cache, &[tag]));
    let mut sim = Sim::new(lib.clone());
    sim.spawn(tag, Vec3::ZERO, Mat3::IDENTITY, None).unwrap();
    let eye = mcc_effects::draw::Eye {
        position: Vec3::new(0.0, 5.0, 100.0),
        right: Vec3::X,
        up: Vec3::Y,
        forward: Vec3::NEG_Z,
    };
    for step in 0..12 {
        sim.step(0.05);
        if step % 3 != 1 {
            continue;
        }
        println!("t = {:.2}", sim.time);
        for s in &sim.systems {
            let frames = mcc_effects::draw::frames(&lib, &lib.sequences, s.particle);
            let q = mcc_effects::draw::system_quads(&lib, s, &frames, &eye, Vec3::ZERO);
            let n = q.colours.len() / 4;
            let name = cache.tags.get(s.particle).unwrap().name().rsplit('\\').next().unwrap().to_string();
            let first = q.colours.first().copied();
            let size = q.positions.get(0..4).map(|c| Vec3::from(c[0]).distance(Vec3::from(c[1])));
            println!("  {name}: {} particles, {n} quads, colour {first:?}, width {size:?} m, uv1 {:?}", s.particles.len(), q.uv1.first());
        }
    }
}

/// Prints the fireball's palette rows and its base map's spread (debugging
/// aid: `cargo test -- --ignored --nocapture fireball_palette`).
#[test]
#[ignore]
fn fireball_palette() {
    let Some(path) = game_paths::halo3_maps().map(|d| d.join("120_halo.map")).filter(|p| p.exists()) else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    let tag = cache.tags.find("effe", PLATE_EXPLOSION).unwrap().index;
    let assets = mcc_effects::load::load(&cache, &[tag]);
    for (b, t) in &assets.textures {
        let px = &t.levels[0];
        let n = (t.width * t.height) as usize;
        let mean = |c: usize| px.chunks_exact(4).map(|p| p[c] as f32).sum::<f32>() / n as f32;
        println!("{b} {} {}x{} mono {} mean rgba {:.0} {:.0} {:.0} {:.0}", t.name, t.width, t.height, t.mono, mean(0), mean(1), mean(2), mean(3));
        if t.name.ends_with("explosion_animated") {
            for row in [0u32, 8, 16, 24, 31] {
                let line: Vec<String> = [0u32, 32, 64, 128, 192, 255]
                    .iter()
                    .map(|u| {
                        let o = ((row * t.width + u) * 4) as usize;
                        format!("{:02x}{:02x}{:02x}/{:02x}", px[o], px[o + 1], px[o + 2], px[o + 3])
                    })
                    .collect();
                println!("   row {row}: {}", line.join(" "));
            }
        }
    }
}

/// The run's giant explosions far off in the canyon: `big_explosions_ambient`
/// wakes at the run's first trigger and plays `giant_explosion` at a random
/// point of `g_location` every 15 to 20 s.
#[test]
fn giant_explosions_go_off_in_the_canyon() {
    let Some(path) = game_paths::halo3_maps().map(|d| d.join("120_halo.map")).filter(|p| p.exists()) else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    let options = blam_geometry::Options { bsps: Some(vec![7, 8, 9]), collision: false, ..Default::default() };
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(&cache, &options, &mut scene).unwrap();
    let data = Arc::new(blam_script::ScriptData::read(&cache).unwrap());
    setup::mark_dynamic(&mut scene, &data);
    let config = Config { editor: true, ..Default::default() };
    let mut rt = setup::runtime(&cache, &scene, Arc::clone(&data), config);
    assert!(rt.console("ins_trench_run"));
    let mut world = World { player: Vec3::new(-263.64, -433.77, -5.05), zone_set: 0, effects: Vec::new() };
    let mut ai = NoAi;
    for _ in 0..90 {
        rt.step(&mut world, &mut ai, None);
    }
    let centre = |name: &str| {
        let v = scene.trigger_volumes.iter().find(|v| v.name == name).unwrap();
        let left = v.up.cross(v.forward);
        v.position + v.forward * v.extents.x * 0.5 + left * v.extents.y * 0.5 + v.up * v.extents.z * 0.5
    };
    world.player = centre("120_08_trench_encounter_100");
    for _ in 0..30 {
        rt.step(&mut world, &mut ai, None);
    }
    world.player = centre("trench_run_100_trig00");
    for _ in 0..(30 * 45) {
        rt.step(&mut world, &mut ai, None);
    }
    let giant: Vec<&Request> = world.effects.iter().filter(|r| r.name.ends_with("giant_explosion")).collect();
    assert!(giant.len() >= 2, "{:?}", world.effects.iter().map(|r| &r.name).collect::<Vec<_>>());
    let in_set = |name: &str, at: Vec3| {
        data.point_sets.iter().find(|s| s.name == name).unwrap().points.iter().any(|p| Vec3::from(p.position) == at)
    };
    // The first at `g_location`'s first value, the rest where
    // `trench_run_global` moved it as the player reached the run.
    assert!(in_set("big_explosions_100_01", giant[0].at), "{:?}", giant[0].at);
    for g in &giant[1..] {
        assert!(in_set("big_explosions_100_02", g.at), "{:?}", g.at);
    }
}
