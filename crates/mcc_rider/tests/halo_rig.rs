//! Master Chief's own skeleton as the rider's rig, from the player's MCC:
//! the multiplayer Spartan (Valhalla, `riverworld`) and the campaign's
//! Master Chief (Halo 3's last mission, `120_halo`). Checks only that he
//! comes out a standing man about the skater's size, facing +Z with his
//! left at +X, that the bike's and the on-foot rig take him, and that on the
//! bike his head is by the bike's head crash probe; no numbers of his own
//! are written here. Skips quietly when Halo 3 is not installed
//! (`HALO3_MAPS` overrides the folder).

use bevy::math::{Vec2, Vec3};
use mcc_bike::physics::{BAR_HALF_WIDTH, PROBES, bar_centre};
use mcc_bike::pose::{Rig, Stance};
use mcc_core::skate::{DEFAULT_SCALE, HaloRig};

/// The farthest the head bone may be from the bike's head probe, metres.
/// The probe is the helmet; Halo's `head` node is at the base of the skull,
/// lower in the helmet than the skater's HEAD, so it sits further out than
/// the skater's does while the helmet drawn round it lands a few
/// centimetres from where it is on the skater (compare the `--shot --bike`
/// pictures with and without `MCC_SKATE_ASSETS=skip`). A rig turned or
/// scaled wrong lands far further out.
const HEAD_NEAR: f32 = 0.4;

fn at(rig: &HaloRig, name: &str) -> Vec3 {
    let i = rig.names.iter().position(|n| n == name).unwrap_or_else(|| panic!("no {name}"));
    rig.binds[i].w_axis.truncate()
}

/// How far the posed head bone is from the bike's head probe (the last of
/// `PROBES`, render metres in the bike frame), sitting still, hands on the
/// bars.
fn head_off_the_probe(rig: &Rig) -> f32 {
    let side = Vec3::X * BAR_HALF_WIDTH;
    let stance = Stance {
        grips: [bar_centre() + side, bar_centre() - side],
        lean: Vec2::ZERO,
        contacts: [Vec3::ZERO; 2],
    };
    let pose = rig.pose(DEFAULT_SCALE, &stance);
    let i = rig.names.iter().position(|n| n == "HEAD").expect("a head");
    let head = pose[i].w_axis.truncate() / DEFAULT_SCALE;
    let (probe, _, _) = PROBES[PROBES.len() - 1];
    head.distance(probe)
}

fn check(map: &str) {
    let Some(path) = game_paths::halo3_map(map) else { return };
    let cache = blam_cache::CacheFile::open(&path).expect("opens");
    let chief = mcc_rider::load_chief(&cache).expect("a Spartan");
    let rig = mcc_rider::chief_rig(&chief, DEFAULT_SCALE).unwrap_or_else(|e| panic!("{map}: {e}"));
    let hips = at(&rig, "HIPS");
    let head = at(&rig, "HEAD");
    let toe = at(&rig, "LEFTTOEBASE").y.min(at(&rig, "RIGHTTOEBASE").y);
    let leg = at(&rig, "LEFTUPLEG").distance(at(&rig, "LEFTLEG")) + at(&rig, "LEFTLEG").distance(at(&rig, "LEFTFOOT"));
    eprintln!("{map}: hips {:.3}, head {:.3}, lowest toe {toe:.3}, leg {leg:.3}", hips.y, head.y);
    assert!((0.85..1.15).contains(&hips.y), "{map}: hips at {}", hips.y);
    assert!((1.4..1.75).contains(&head.y), "{map}: head at {}", head.y);
    assert!((-0.05..0.1).contains(&toe), "{map}: lowest toe at {toe}");
    assert!(leg > 0.75, "{map}: leg {leg}");
    assert!(at(&rig, "LEFTUPLEG").x > 0.0 && at(&rig, "RIGHTUPLEG").x < 0.0, "{map}: left is not +X");
    assert!(at(&rig, "LEFTTOEBASE").z > at(&rig, "LEFTFOOT").z, "{map}: not facing +Z");

    let rig = Rig::from_binds(rig.names, rig.binds, map).unwrap_or_else(|e| panic!("{e}"));
    let off = head_off_the_probe(&rig);
    eprintln!("{map}: head {off:.3} m from the bike's head probe");
    assert!(off < HEAD_NEAR, "{map}: head {off} m from the bike's head probe");
}

#[test]
fn valhallas_spartan_makes_a_rig() {
    check("riverworld");
}

#[test]
fn the_campaigns_master_chief_makes_a_rig() {
    check("120_halo");
}

/// The same head check on Skate 3's skater, for comparison, when Skate 3 is
/// set up.
#[test]
fn the_skaters_head_on_the_bike() {
    let Some(assets) = game_paths::skate_assets().filter(|_| game_paths::skate_ready()) else { return };
    let rig = Rig::load(&assets.join("rig.json")).unwrap_or_else(|e| panic!("{e}"));
    let off = head_off_the_probe(&rig);
    eprintln!("skater: head {off:.3} m from the bike's head probe");
    assert!(off < HEAD_NEAR, "skater: head {off} m from the bike's head probe");
}
