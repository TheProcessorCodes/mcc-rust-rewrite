//! The Warthog run's effects on 120_halo, read from the user's MCC install
//! (skipped when it is not there). Counts and values are the ones
//! `docs/format/scripts/probe_effects.py` prints (`docs/format/effects.md`).

use blam_cache::CacheFile;
use blam_effect::{Billboard, Library};

fn map() -> Option<CacheFile> {
    CacheFile::open(game_paths::halo3_map("120_halo")?).ok()
}

const PLATE_EXPLOSION: &str = r"levels\solo\120_halo\fx\trench_run\plate_explosion\plate_explosion";
const GIANT_EXPLOSION: &str = r"levels\solo\120_halo\fx\trench_run\giant_explosion\giant_explosion";

fn short(c: &CacheFile, tag: u32) -> String {
    c.tags.get(tag).unwrap().name().rsplit('\\').next().unwrap().to_string()
}

#[test]
fn plate_explosion_layout() {
    let Some(c) = map() else { return };
    let tag = c.tags.find("effe", PLATE_EXPLOSION).unwrap().index;
    let lib = Library::load(&c, &[tag]);
    assert!(lib.errors.is_empty(), "{:?}", lib.errors);
    let e = lib.effect(tag).unwrap();
    let names: Vec<&str> = e.events.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["flash", "fireball", "fiery_plume", "rumble", "event_4"]);
    assert_eq!(e.locations.len(), 1);
    // The last event only holds the effect for 5 s.
    assert_eq!(e.events[4].duration, [5.0, 5.0]);
    assert!(e.events[4].parts.is_empty() && e.events[4].systems.is_empty());

    // flash: two sounds and three particle systems.
    let flash = &e.events[0];
    let parts: Vec<(String, String)> = flash
        .parts
        .iter()
        .map(|p| (p.what.unwrap().class.as_str().to_string(), short(&c, p.what.unwrap().tag)))
        .collect();
    assert_eq!(parts, [("snd!".into(), "plate_expl".into()), ("snd!".into(), "plate_expl_verb".into())]);
    let systems: Vec<String> = flash.systems.iter().map(|s| short(&c, s.particle.unwrap())).collect();
    assert_eq!(systems, ["explosive_flash", "burst_spray_additive", "smoke_fiery_small"]);
    // One flash quad, 8 wu across.
    let flash_emitter = &flash.systems[0].emitters[0];
    assert_eq!(flash_emitter.starting_count.function.scalar(0.0, 0.0), 1.0);
    assert_eq!(flash_emitter.size.function.scalar(0.0, 0.0), 8.0);

    // fireball: a light and the fireball particles.
    let fireball = &e.events[1];
    assert_eq!(fireball.parts[0].what.unwrap().class.as_str(), "ligh");
    let sys = &fireball.systems[0];
    assert_eq!(short(&c, sys.particle.unwrap()), "fireball_human_large");
    let em = &sys.emitters[0];
    assert_eq!(em.name, "emitter_0");
    assert_eq!(em.starting_count.function.scalar(0.0, 0.0), 50.0);
    // Lifespan 0.5 to 1.5 s and speed 0 to 20 wu/s, both on the same
    // random number (input 4).
    assert_eq!((em.lifespan.input, em.lifespan.range), (4, 4));
    assert!((em.lifespan.function.scalar(0.0, 0.0) - 0.5).abs() < 1e-5);
    assert!((em.lifespan.function.scalar(1.0, 0.0) - 1.5).abs() < 1e-3);
    assert!((em.velocity.function.scalar(1.0, 0.0) - 20.0).abs() < 1e-2);
    // 1.5 to 1 wu across, growing to twice that over its life.
    assert_eq!(em.size.function.scalar(0.0, 0.0), 1.5);
    assert_eq!(em.size.function.scalar(0.0, 1.0), 1.0);
    assert!((em.scale.function.scalar(1.0, 0.0) - 2.0).abs() < 0.03);
    // It rises and slows: gravity -0.2, drag 3 (pmov explosion_fireball).
    assert_eq!(em.movement.physics(0).unwrap().function.scalar(0.0, 0.0), -0.2);
    assert_eq!(em.movement.physics(1).unwrap().function.scalar(0.0, 0.0), 3.0);
    // White to orange-brown over its life.
    assert_eq!(em.tint.function.colour(0.0, 0.0), [1.0, 1.0, 1.0]);
    let end = em.tint.function.colour(1.0, 0.0);
    assert!(end[0] > end[1] && end[1] > end[2], "{end:?}");

    let p = &lib.particles[&sys.particle.unwrap()];
    assert_eq!(p.billboard, Billboard::Camera);
    assert_eq!(p.shader.option("albedo"), Some("palettized_plus_billboard_alpha"));
    assert_eq!(p.shader.option("blend_mode"), Some("add_src_times_srcalpha"));
    let base = p.shader.map("base_map").unwrap();
    assert!(base.bitmap_name.ends_with("rolling_smoke_plate"));
    assert!(p.shader.map("palette").unwrap().bitmap_name.ends_with("explosion_animated"));
    // Alpha holds at 1 to half its life, then fades.
    assert!((p.alpha.function.scalar(0.4, 0.0) - 1.0).abs() < 1e-3);
    assert!(p.alpha.function.scalar(1.0, 0.0).abs() < 1e-2);

    // The ejecta particles carry the ejecta effect, which leaves a contrail.
    let ejecta = &lib.particles[&fireball.systems[1].particle.unwrap()];
    assert_eq!(ejecta.attachments.len(), 1);
    assert_eq!(ejecta.attachments[0].what.class.as_str(), "effe");
    let carried = lib.effect(ejecta.attachments[0].what.tag).unwrap();
    assert_eq!(carried.events[0].parts[0].what.unwrap().class.as_str(), "cntl");
    assert_eq!(lib.contrails.len(), 2);

    // The light: orange, up to 20, for 2 s.
    let light = &lib.lights[&fireball.parts[0].what.unwrap().tag];
    assert_eq!(light.duration, 2.0);
    assert_eq!(light.size, 6.0);
    assert!((light.intensity.function.scalar(0.0, 0.0) - 20.0).abs() < 1e-3);
    assert!(light.intensity.function.scalar(1.0, 0.0).abs() < 0.05);
}

#[test]
fn giant_explosion_layout() {
    let Some(c) = map() else { return };
    let tag = c.tags.find("effe", GIANT_EXPLOSION).unwrap().index;
    let lib = Library::load(&c, &[tag]);
    assert!(lib.errors.is_empty(), "{:?}", lib.errors);
    let e = lib.effect(tag).unwrap();
    let names: Vec<&str> = e.events.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["explosion", "audio", "delay"]);
    assert_eq!(e.events[0].systems.len(), 5);
    assert_eq!(e.events[1].delay, [1.5, 1.5]);
    let sparks = &e.events[0].systems[0];
    assert_eq!(short(&c, sparks.particle.unwrap()), "spark_long");
    assert_eq!(lib.particles[&sparks.particle.unwrap()].billboard, Billboard::Velocity);
    let em = &sparks.emitters[0];
    assert_eq!(em.starting_count.function.scalar(0.0, 0.0), 60.0);
    assert_eq!(em.size.function.scalar(0.0, 0.0), 0.35);
    // Every bitmap the effect draws with was found.
    assert_eq!(lib.bitmaps.len(), 7);
}

/// Every effect in the map reads without error (their functions use only
/// the kinds and segment shapes `function.rs` knows).
#[test]
fn every_effect_in_120_halo_reads() {
    let Some(c) = map() else { return };
    let roots: Vec<u32> = c.tags.of_class("effe").map(|t| t.index).collect();
    let lib = Library::load(&c, &roots);
    assert!(lib.effects.len() >= 900, "{}", lib.effects.len());
    assert!(lib.errors.len() <= 5, "{} errors: {:#?}", lib.errors.len(), &lib.errors[..lib.errors.len().min(20)]);
}
