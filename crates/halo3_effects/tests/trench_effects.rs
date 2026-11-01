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
use halo3_effects::sim::Sim;
use halo3_script::vm::{Config, WorldApi};
use halo3_script::{NoAi, setup};

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
