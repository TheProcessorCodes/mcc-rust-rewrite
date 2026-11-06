//! Halo CE's Warthog run (`d40`, The Maw) sets off its explosions from its
//! own scripts, end to end with no window: the scripts, begun as the
//! `h1:warthog_run` mission begins them, are driven through the run's
//! trigger volumes; every effect they ask for is one of the map's effects
//! as `blam_h1::effect_library` converts them, at one of the map's
//! cutscene flags; the effect player then plays a large explosion from
//! its tags: the flash, the fire turning to smoke, the drifting smoke and
//! the sound handed on. Skips quietly when the map is not installed.

use std::sync::Arc;

use bevy::math::{Mat3, Vec3};
use blam_geometry::TriggerVolume;
use mcc_effects::sim::Sim;
use mcc_script::NoAi;
use mcc_script::h1::{THE_MAW, runtime};
use mcc_script::vm::WorldApi;

const LARGE: &str = r"effects\explosions\large explosion";

struct World {
    player: Vec3,
    zone_set: usize,
    effects: Vec<(String, Vec3)>,
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
        self.effects.push((tag.to_string(), at));
    }
}

fn middle(v: &TriggerVolume) -> Vec3 {
    let left = v.up.cross(v.forward);
    v.position + (v.forward * v.extents.x + left * v.extents.y + v.up * v.extents.z) * 0.5
}

#[test]
fn the_run_sets_off_its_own_explosions() {
    let Some(path) = game_paths::halo1_maps().map(|d| d.join("d40.map")).filter(|p| p.exists()) else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let options = blam_h1::Options {
        textures: false,
        objects: false,
        bsps: Some(THE_MAW.bsps.to_vec()),
        ..Default::default()
    };
    let scene = blam_h1::decode(&map, &options).unwrap().scene;
    let data = blam_h1::script::read(&map).unwrap();
    let converted = blam_h1::effect_library::convert(&map, &blam_h1::effect_library::script_effects(&data));
    let lib = Arc::new(converted.library);
    let flags: Vec<Vec3> = data.cutscene_flags.iter().map(|f| Vec3::from(f.position)).collect();
    let mut rt = runtime(data, &scene, &THE_MAW, 1);
    let start = blam_h1::object_named(&map, THE_MAW.start_at).unwrap().position;
    let mut world = World { player: start, zone_set: 0, effects: Vec::new() };
    // Along the run's volumes, as `mcc_script`'s run test drives it.
    let mut route: Vec<&TriggerVolume> = scene
        .trigger_volumes
        .iter()
        .filter(|v| (v.name.starts_with("enc6_") || v.name.starts_with("enc7_") || v.name == "section7") && middle(v).z > 20.0)
        .filter(|v| !v.name.contains("cov") && !v.name.contains("retreat"))
        .collect();
    route.sort_by(|a, b| middle(a).x.total_cmp(&middle(b).x));
    for v in route {
        world.player = middle(v);
        for _ in 0..90 {
            rt.step(&mut world, &mut NoAi, None);
        }
    }
    let mut by_name = std::collections::BTreeMap::<&str, usize>::new();
    for (name, at) in &world.effects {
        assert!(lib.effect_named(name).is_some(), "{name} is not among the converted effects");
        assert!(flags.iter().any(|f| f.distance(*at) < 1e-3), "{name} at {at} is at no cutscene flag");
        *by_name.entry(name.as_str()).or_default() += 1;
    }
    eprintln!("{} explosions: {by_name:#?}", world.effects.len());
    assert!(world.effects.len() >= 40, "{by_name:?}");
    for want in [LARGE, r"effects\small explosion", r"effects\explosions\steam explosion no objects", r"effects\explosions\medium explosion no objects"] {
        assert!(by_name.contains_key(want), "{want} never set off: {by_name:?}");
    }
}

#[test]
fn large_explosion_plays() {
    let Some(path) = game_paths::halo1_maps().map(|d| d.join("d40.map")).filter(|p| p.exists()) else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let tag = map.find(b"effe", LARGE).unwrap().id & 0xFFFF;
    let lib = Arc::new(blam_h1::effect_library::convert(&map, &[tag]).library);
    let mut sim = Sim::new(lib.clone());
    let at = Vec3::new(-106.85, 2.38, 107.67);
    sim.spawn(tag, at, Mat3::IDENTITY, None).unwrap();
    sim.step(1.0 / 60.0);
    // The flash, the sound, fire and smoke at once.
    assert_eq!(sim.flashes.len(), 1);
    let sounds: Vec<&str> = sim.sounds.iter().map(|(t, _)| lib.sound_names[t].as_str()).collect();
    assert_eq!(sounds, [r"sound\sfx\impulse\impacts\large_explosion"]);
    assert_eq!(sim.sounds[0].1, at);
    let alive = |sim: &Sim, what: &str| -> usize {
        sim.systems
            .iter()
            .filter(|s| lib.particles[&s.particle].name.contains(what))
            .map(|s| s.particles.len())
            .sum()
    };
    assert_eq!(alive(&sim, "fire to smoke fire"), 30);
    assert_eq!(alive(&sim, "smoke plume"), 30);
    assert!(alive(&sim, "drifting smoke") >= 15);
    // Every fire particle starts within the system's 1.5 wu.
    for s in sim.systems.iter().filter(|s| lib.particles[&s.particle].name.contains("fire to smoke fire")) {
        assert!(s.particles.iter().all(|p| p.position.distance(at) < 1.6));
    }
    // The light lasts its 30 ticks; the smoke rises (it is lighter than
    // air) and is gone within its states' longest life (4.8 s).
    for _ in 0..90 {
        sim.step(1.0 / 60.0);
    }
    assert!(sim.flashes.is_empty());
    let rise: Vec<f32> = sim
        .systems
        .iter()
        .filter(|s| lib.particles[&s.particle].name.contains("drifting smoke"))
        .flat_map(|s| s.particles.iter().map(|p| p.velocity.z))
        .collect();
    assert!(rise.iter().sum::<f32>() > 0.0, "{rise:?}");
    for _ in 0..60 * 5 {
        sim.step(1.0 / 60.0);
    }
    assert!(alive(&sim, "fire to smoke") == 0, "particles left after 6.5 s");
}
