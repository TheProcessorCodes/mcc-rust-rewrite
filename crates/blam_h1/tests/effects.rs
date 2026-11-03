//! Halo CE's effects on The Maw (`d40`): every effect the scripts name is
//! read with everything it reaches (`docs/format/halo1.md` 13) and turned
//! into Halo 3's effect types for `halo3_effects`. Skips quietly when the
//! map is not installed.

use std::path::PathBuf;

fn d40() -> Option<PathBuf> {
    let p = game_paths::halo1_maps()?.join("d40.map");
    p.exists().then_some(p)
}

/// The effects the Warthog run's scripts set off, by the globals that
/// name them.
const RUN: &[&str] = &[
    r"effects\small explosion",
    r"weapons\frag grenade\effects\explosion",
    r"effects\explosions\medium explosion",
    r"effects\explosions\medium explosion no objects",
    r"effects\explosions\large explosion",
    r"effects\explosions\large explosion no objects",
    r"effects\explosions\steam explosion",
    r"effects\explosions\steam explosion no objects",
];

#[test]
fn every_script_effect_reads() {
    let Some(path) = d40() else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let data = blam_h1::script::read(&map).unwrap();
    let roots = blam_h1::effect_library::script_effects(&data);
    let names: Vec<&str> = roots.iter().map(|&t| map.tags[t as usize].name.as_str()).collect();
    for want in RUN {
        assert!(names.iter().any(|n| n.eq_ignore_ascii_case(want)), "{want} not among the scripts' effects {names:?}");
    }
    let c = blam_h1::effect_library::convert(&map, &roots);
    assert!(c.library.errors.is_empty(), "{:?}", c.library.errors);
    for want in RUN {
        assert!(c.library.effect_named(want).is_some(), "{want} not converted");
    }
    eprintln!(
        "{} script effects: {} effects, {} particles, {} lights, {} bitmaps, {} sounds",
        roots.len(),
        c.library.effects.len(),
        c.library.particles.len(),
        c.library.lights.len(),
        c.library.bitmaps.len(),
        c.library.sound_names.len()
    );
    // Every bitmap's atlas decodes.
    for (&b, layout) in &c.layouts {
        let px = blam_h1::effect_library::atlas_pixels(&map, b, layout).unwrap_or_else(|e| panic!("{}: {e}", map.tags[b as usize].name));
        assert_eq!(px.levels[0].len(), (layout.width * layout.height * 4) as usize);
    }
}

#[test]
fn large_explosion_reads_as_in_the_tags() {
    let Some(path) = d40() else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let tag = map.find(b"effe", r"effects\explosions\large explosion").unwrap().id & 0xFFFF;
    let e = blam_h1::effect::read_effect(&map, tag).unwrap();
    assert_eq!(e.locations, ["", "normal", "gravity"]);
    assert_eq!(e.events.len(), 1);
    let ev = &e.events[0];
    let classes: Vec<&str> = ev.parts.iter().filter_map(|p| p.what.as_ref()).map(|w| w.class.as_str()).collect();
    assert_eq!(classes, ["jpt!", "snd!", "jpt!", "pctl", "deca", "garb", "garb", "garb", "ligh"]);
    // The fireball: 30 particles in 1.5 wu, fire turning to a smoke plume.
    let pctl = ev.parts[3].what.as_ref().unwrap().tag;
    let s = blam_h1::effect::read_system(&map, pctl).unwrap();
    let ty = &s.types[0];
    assert_eq!((ty.name.as_str(), ty.initial_count, ty.radius), ("fire to smoke", 30, 1.5));
    let names: Vec<&str> = ty.particle_states.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["fire", "smoke plume", "death"]);
    assert_eq!(ty.particle_states[0].shader.blend, 3, "fire is added");
    assert_eq!(ty.particle_states[1].shader.blend, 0, "smoke is alpha blended");
    assert_eq!(ty.particle_states[0].duration, [0.1, 0.3]);
    // The light: 10 wu, orange to black over 30 ticks.
    let light = ev.parts[8].what.as_ref().unwrap().tag;
    let l = blam_h1::effect::read_light(&map, light).unwrap();
    assert_eq!(l.radius, 10.0);
    assert_eq!(l.duration_seconds(), 1.0);
    assert!(l.colour_upper[1] == 1.0 && l.colour_upper[3] < 0.2 && l.colour_lower[1] == 0.0, "{l:?}");
    // The drifting smoke: a `part` with fades and smoke physics.
    let smoke = &ev.particles[0];
    assert_eq!(smoke.count, [15, 20]);
    let p = blam_h1::effect::read_particle(&map, smoke.particle.unwrap()).unwrap();
    assert_eq!((p.lifespan, p.fade_in, p.fade_out), ([2.0, 4.0], 0.3, 3.0));
    let ph = blam_h1::effect::read_physics(&map, p.physics.unwrap()).unwrap();
    assert!(ph.air_buoyancy > 0.1 && ph.air_buoyancy < 0.2, "smoke rises: {ph:?}");
}

#[test]
fn large_explosion_converts() {
    let Some(path) = d40() else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let tag = map.find(b"effe", r"effects\explosions\large explosion").unwrap().id & 0xFFFF;
    let c = blam_h1::effect_library::convert(&map, &[tag]);
    let e = c.library.effect(tag).unwrap();
    let ev = &e.events[0];
    // The pctl is two systems (fire, smoke plume); the event's particles
    // one each.
    assert_eq!(ev.systems.len(), 2 + 4, "{:?}", ev.systems.iter().map(|s| &s.emitters[0].name).collect::<Vec<_>>());
    let classes: Vec<String> = ev.parts.iter().filter_map(|p| p.what).map(|r| r.class.as_str().to_string()).collect();
    let classes: Vec<&str> = classes.iter().map(String::as_str).collect();
    assert!(classes.contains(&"ligh") && classes.contains(&"snd!") && !classes.contains(&"pctl"), "{classes:?}");
    let sound = ev.parts.iter().filter_map(|p| p.what).find(|r| r.class.as_str() == "snd!").unwrap();
    assert_eq!(c.library.sound_names[&sound.tag], r"sound\sfx\impulse\impacts\large_explosion");
    // Fire is drawn added, smoke blended, both from sprite sheets.
    let fire = &c.library.particles[&ev.systems[0].particle.unwrap()];
    assert_eq!(fire.shader.option("blend_mode"), Some("additive"));
    let base = fire.shader.map("base_map").unwrap().bitmap.unwrap();
    assert!(c.library.sequences[&base][0].sprites.len() >= 60);
    let smoke = &c.library.particles[&ev.systems[1].particle.unwrap()];
    assert_eq!(smoke.shader.option("blend_mode"), Some("alpha_blend"));
    let em = &ev.systems[0].emitters[0];
    assert_eq!(em.starting_count.function.scalar(0.0, 0.0), 30.0);
    // The fire is about 1.4 to 2.5 wu across (0.04 to 0.07 wu a pixel of
    // 36-pixel sprites) and gone by the end of its transition.
    let size = (em.size.function.scalar(0.0, 0.0), em.size.function.scalar(0.0, 1.0));
    assert!(size.0 > 1.0 && size.1 < 3.0, "{size:?}");
    assert!(em.alpha.function.scalar(0.95, 0.0) < 0.01);
    assert!(em.alpha.function.scalar(0.02, 0.0) > 0.9);
}
