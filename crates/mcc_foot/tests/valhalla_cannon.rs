//! Valhalla's man cannons on the real map's collision, headless: the
//! Spartan and the dirt bike are put on each pad and should come down
//! about where Halo's player does, which is where a point thrown at the
//! pad's speed under Halo's gravity (no drag) meets the ground: for the
//! two base cannons the foot of the middle hill, some 28 wu out. Skips
//! quietly when the map is not installed.

use bevy::math::{Vec2, Vec3};
use blam_geometry::{LiftShape, LiftVolume, WORLD_UNIT_METRES as WU};
use mcc_bike::physics::{Bike, World};
use mcc_core::coords::{to_halo, to_render, to_render_dir};
use mcc_core::lifts::{Flight, HALO_GRAVITY, Lifts};
use mcc_foot::physics::{Controls, Spartan};

const DT: f32 = 1.0 / 60.0;

fn valhalla() -> Option<(World, Lifts)> {
    let path = game_paths::halo3_map("riverworld")?;
    let cache = blam_cache::CacheFile::open(&path).expect("opens");
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(&cache, &blam_geometry::Options::default(), &mut scene).expect("decodes");
    let lifts = blam_geometry::objects::lifts(&cache, &scene);
    Some((mcc_bike::build_world(&scene.collision, u32::MAX), Lifts(lifts)))
}

/// The pad's middle, renderer metres.
fn pad(l: &LiftVolume) -> Vec3 {
    let LiftShape::Box { centre, .. } = l.shape else { panic!("{l:?}") };
    to_render(centre)
}

/// How far along the ground from `a` to `b`, wu.
fn along(a: Vec3, b: Vec3) -> f32 {
    Vec3::new(b.x - a.x, 0.0, b.z - a.z).length() / WU
}

/// Where Halo's player comes down: a point thrown from `from` at `v`,
/// falling at Halo's gravity until it meets the ground.
fn halo_landing(world: &World, from: Vec3, mut v: Vec3) -> Vec3 {
    let mut p = from;
    for _ in 0..(30.0 / DT) as usize {
        v.y -= HALO_GRAVITY * DT;
        let step = v * DT;
        if v.y < 0.0
            && let Some(hit) = world.raycast_far(p, step.normalize(), step.length())
        {
            return hit.point;
        }
        p += step;
    }
    panic!("never came down from {from}");
}

/// The pad's own throw from a standstill.
fn throw(l: &LiftVolume) -> Vec3 {
    to_render_dir(l.direction) * l.launch_speed().expect("a launch pad") * WU
}

#[test]
fn cannons_throw_the_spartan_and_the_bike_as_far_as_halo() {
    let Some((world, lifts)) = valhalla() else { return };
    let pads: Vec<&LiftVolume> = lifts.0.iter().filter(|l| l.launch_speed().is_some()).collect();
    assert_eq!(pads.len(), 4, "{:#?}", lifts.0);
    for l in pads {
        let top = l.launch_speed().unwrap();
        let dir = to_render_dir(l.direction);
        let heading = dir.x.atan2(dir.z);

        // On foot: walked onto the pad from behind it, the stick let go
        // once thrown.
        let back = Vec3::new(dir.x, 0.0, dir.z).normalize() * (1.5 * WU);
        let mut s = Spartan::dropped(&world, pad(l) - back + Vec3::Y * 2.0, heading);
        let gravity = s.tuning.gravity;
        // Where and how fast Halo's player leaves: his run-in kept, the
        // pad's speed along its direction.
        let mut thrown: Option<(Vec3, Vec3)> = None;
        let (mut peak, mut fastest, mut flown) = (0.0f32, 0.0f32, 0.0);
        for _ in 0..(30.0 / DT) as usize {
            let before = (s.position, s.velocity);
            let wish = if thrown.is_some() { Vec2::ZERO } else { Vec2::Y };
            let controls = Controls { wish, heading, ..Default::default() };
            s.step_in(&world, controls, DT, &|p, v, h, r| lifts.walk(p, v, h, r, gravity));
            if thrown.is_none() && (s.velocity - before.1).length() > 3.0 * WU {
                let v = before.1 + dir * (top * WU - before.1.dot(dir)).max(0.0);
                thrown = Some((before.0, v));
            }
            let Some((from, _)) = thrown else { continue };
            flown += DT;
            peak = peak.max(s.position.y - from.y);
            fastest = fastest.max(s.velocity.length());
            if s.grounded && along(from, s.position) > 3.0 {
                break;
            }
        }
        let (from, v) = thrown.unwrap_or_else(|| panic!("{}: walked 30 s and was never thrown, at halo {}", l.name, to_halo(s.position)));
        let halo = halo_landing(&world, from, v);
        let want = along(from, halo);
        let foot = along(from, s.position);
        eprintln!(
            "{}: {top} wu/s; Halo's flight lands {want:.1} wu out at halo {:.1}; on foot {foot:.1} wu in {flown:.1} s, peak {:.1} wu, left at {:.1} wu/s, down at halo {:.1}",
            l.name,
            to_halo(halo),
            peak / WU,
            fastest / WU,
            to_halo(s.position)
        );
        assert!(s.grounded, "{}: still in the air", l.name);
        assert!((foot - want).abs() < want * 0.15, "{}: on foot {foot:.1} wu, Halo {want:.1}", l.name);

        // On the bike (our own physics; Reflex's when the test below turns
        // it on), hands off.
        let (far, want) = bike_flight(&world, &lifts, l, heading);
        assert!((far - want).abs() < want * 0.2, "{}: the bike {far:.1} wu, Halo {want:.1}", l.name);
    }
    reflex_bike(&world, &lifts);
}

/// The bike ridden onto the pad `l` from behind it on a little gas, hands
/// off once thrown: how far out it first touches down, and how far Halo's
/// flight from where it left goes, wu.
fn bike_flight(world: &World, lifts: &Lifts, l: &LiftVolume, heading: f32) -> (f32, f32) {
    let dir = to_render_dir(l.direction);
    let back = Vec3::new(dir.x, 0.0, dir.z).normalize() * (3.0 * WU);
    let mut bike = Bike::dropped(world, pad(l) - back + Vec3::Y * 2.0, heading);
    let mut flight = Flight::default();
    let mut from = None;
    let (mut peak, mut fastest, mut flown, mut tipped) = (0.0f32, 0.0f32, 0.0, 1.0f32);
    for k in 0..(30.0 / DT) as usize {
        let controls = mcc_bike::physics::Controls {
            // Settled first, then on the gas.
            throttle: if from.is_none() && k > 60 { 0.5 } else { 0.0 },
            ..Default::default()
        };
        bike.step(world, controls, DT);
        let airborne = bike.grounded() == 0;
        if let Some(dv) = flight.ride(lifts, bike.position, bike.velocity, airborne, DT, 0.5, mcc_bike::physics::GRAVITY) {
            bike.velocity += dv;
        }
        if from.is_none() && flight.flying() {
            from = Some(bike.position);
        }
        let Some(from) = from else { continue };
        flown += DT;
        peak = peak.max(bike.position.y - from.y);
        fastest = fastest.max(bike.velocity.length());
        tipped = tipped.min(bike.up().y);
        if bike.grounded() > 0 && along(from, bike.position) > 3.0 && flown > 1.0 {
            break;
        }
    }
    let from = from.unwrap_or_else(|| panic!("{}: the bike was never thrown, at halo {}", l.name, to_halo(bike.position)));
    let far = along(from, bike.position);
    let want = along(from, halo_landing(world, from, throw(l)));
    eprintln!(
        "{}: the bike{} {far:.1} wu (Halo {want:.1}) in {flown:.1} s, peak {:.1} wu, left at {:.1} wu/s, down at halo {:.1}, most tipped up.y {tipped:.2}{}",
        l.name,
        if bike.exact().is_some() { " (Reflex physics)" } else { "" },
        peak / WU,
        fastest / WU,
        to_halo(bike.position),
        if bike.crashed.is_some() { ", wrecked" } else { "" }
    );
    (far, want)
}

/// The same on Reflex's own simulation, when the player's Reflex is set
/// up. Run after the rest: the switch is the whole program's.
fn reflex_bike(world: &World, lifts: &Lifts) {
    if mcc_bike::exact::vehicle().is_none() {
        eprintln!("no Reflex install; the bike on Reflex's physics skipped");
        return;
    }
    mcc_bike::exact::set_on(true);
    for l in lifts.0.iter().filter(|l| l.launch_speed().is_some()) {
        let dir = to_render_dir(l.direction);
        let heading = dir.x.atan2(dir.z);
        let (far, want) = bike_flight(world, lifts, l, heading);
        assert!((far - want).abs() < want * 0.2, "{}: the bike {far:.1} wu, Halo {want:.1}", l.name);
    }
    mcc_bike::exact::set_on(false);
}
