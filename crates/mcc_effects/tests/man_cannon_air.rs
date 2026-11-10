//! The stream of light and smoke over Valhalla's man cannons, played from
//! the map's own effects with no window: each cannon's effect, carried by
//! a null object beside it, should blow steadily (its loop is one event a
//! frame long, played without a break) and along the cannon's push. Skips
//! quietly when the map is not installed.

use std::sync::Arc;

use bevy::math::Vec3;
use blam_effect::Library;
use mcc_effects::ambient;
use mcc_effects::sim::Sim;

#[test]
fn valhallas_cannons_blow_along_their_push() {
    let Some(path) = game_paths::halo3_map("riverworld") else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(&cache, &blam_geometry::Options::default(), &mut scene).unwrap();
    let lifts = blam_geometry::objects::lifts(&cache, &scene);
    let placed: Vec<_> = ambient::placed(&cache, &scene).into_iter().filter(|p| p.object.contains("man_cannon")).collect();
    assert_eq!(placed.len(), 4, "{placed:#?}");
    let roots: Vec<u32> = placed.iter().map(|p| p.effect).collect();
    let lib = Arc::new(Library::load(&cache, &roots));
    for p in &placed {
        let lift = lifts.iter().min_by(|a, b| a.origin.distance(p.at).total_cmp(&b.origin.distance(p.at))).unwrap();
        assert!(lift.launch_speed().is_some() && lift.origin.distance(p.at) < 1.5, "{} is by {lift:?}", p.object);
        let mut sim = Sim::new(lib.clone());
        sim.spawn_looping(p.effect, p.at, p.frame).expect("the effect was read");
        // Two seconds in, then a second watched frame by frame.
        for _ in 0..120 {
            sim.step(1.0 / 60.0);
        }
        let (mut least, mut most) = (usize::MAX, 0);
        for _ in 0..60 {
            sim.step(1.0 / 60.0);
            // One emitter each for the light, the smoke and the sparks,
            // however often the loop has come round; the heat shimmer (a
            // distortion particle) is not drawn.
            assert_eq!(sim.systems.len(), 3, "{}", p.object);
            let light = sim
                .systems
                .iter()
                .find(|s| lib.particles.get(&s.particle).is_some_and(|prt| prt.name.ends_with("antigrav_light")))
                .expect("the light beams");
            least = least.min(light.particles.len());
            most = most.max(light.particles.len());
            for q in &light.particles {
                // Each beam leaves along the cannon's push, from the pad.
                let along = q.velocity.normalize().dot(lift.direction);
                assert!(along > 0.999, "{}: a beam goes {} against the push {}", p.object, q.velocity, lift.direction);
                assert!(q.position.distance(lift.origin) < 2.5, "{}: a beam at {}", p.object, q.position);
            }
        }
        // 50 a second living 0.18 to 0.28 s: about nine to fourteen at
        // once, never none.
        eprintln!("{}: {least} to {most} light beams at once, along {}", p.object, lift.direction);
        assert!(least >= 5 && most <= 30, "{}: {least} to {most} beams", p.object);
    }
}

/// The lift effects of other maps play too: none is turned away from its
/// lift, and every one keeps one emitter per system while it loops.
#[test]
fn other_maps_lift_effects_loop_steadily() {
    for name in ["s3d_waterfall", "midship", "guardian"] {
        let Some(path) = game_paths::halo3_map(name) else { continue };
        let cache = blam_cache::CacheFile::open(&path).unwrap();
        let mut scene = blam_geometry::Scene::default();
        blam_geometry::decode_into(&cache, &blam_geometry::Options::default(), &mut scene).unwrap();
        let lifts = blam_geometry::objects::lifts(&cache, &scene);
        let placed = ambient::placed(&cache, &scene);
        let roots: Vec<u32> = placed.iter().map(|p| p.effect).collect();
        let lib = Arc::new(Library::load(&cache, &roots));
        let mut seen = 0;
        for p in &placed {
            let Some(effect) = lib.effect(p.effect) else { continue };
            let Some(lift) = lifts.iter().filter(|l| l.push.direction.0 != 0.0).find(|l| l.origin.distance(p.at) < 1.5) else { continue };
            seen += 1;
            let mut sim = Sim::new(lib.clone());
            sim.spawn_looping(p.effect, p.at, p.frame).expect("read");
            for _ in 0..90 {
                sim.step(1.0 / 60.0);
            }
            let systems = sim.systems.len();
            let mut particles = 0;
            for _ in 0..90 {
                sim.step(1.0 / 60.0);
                particles = particles.max(sim.systems.iter().map(|s| s.particles.len()).sum::<usize>());
            }
            eprintln!("{name}: {} has {systems} emitters, up to {particles} particles", effect.name);
            assert_eq!(sim.systems.len(), systems, "{name}: {} grows emitters", effect.name);
            assert!(particles > 0, "{name}: {} shows nothing", effect.name);
            // What blows some way blows with the lift, not against it.
            let flat = |v: Vec3| Vec3::new(v.x, v.y, 0.0);
            let blows = effect.events.iter().flat_map(|e| &e.systems).flat_map(|s| &s.emitters).map(|e| Vec3::from(e.direction.a)).find(|d| d.length() > 0.0);
            if let Some(d) = blows
                && flat(lift.direction).length() > 0.1
                && flat(p.frame * d).length() > 0.1
            {
                let with = flat(p.frame * d).normalize().dot(flat(lift.direction).normalize());
                assert!(with > 0.99, "{name}: {} blows {} by a lift pushing {}", effect.name, p.frame * d, lift.direction);
            }
        }
        eprintln!("{name}: {seen} lift effects");
    }
}
