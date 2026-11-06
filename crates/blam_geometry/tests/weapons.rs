//! Weapon tags, their models and markers, and the scenario's weapon
//! placements on the user's own maps (`docs/format/weapons.md`). Skips
//! quietly when a map is not installed.

use std::collections::BTreeMap;
use std::path::PathBuf;

use blam_cache::CacheFile;
use blam_geometry::weapons::{self, ItemKind};

fn map(name: &str) -> Option<PathBuf> {
    game_paths::halo3_map(name)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn valhalla_weapons() {
    let Some(path) = map("riverworld") else { return };
    let cache = CacheFile::open(&path).expect("riverworld opens");
    let all = weapons::weapons(&cache).expect("weapons read");
    assert_eq!(all.len(), 47, "weap tags");
    let by_name = |n: &str| {
        all.iter()
            .find(|w| w.tag_path.rsplit('\\').next() == Some(n))
            .unwrap_or_else(|| panic!("{n} missing"))
    };

    let ar = by_name("assault_rifle");
    assert_eq!((ar.class.as_str(), ar.short_name.as_str()), ("rifle", "ar"));
    assert_eq!(ar.magazines[0].rounds_loaded_maximum, 32);
    assert_eq!(ar.magazines[0].rounds_total_initial, 96);
    assert_eq!(ar.magazines[0].rounds_total_maximum, 384);
    assert!(close(ar.rate_of_fire(), 10.0));
    assert_eq!(ar.zoom_levels, 0);
    assert_eq!(ar.holster_marker, "weapon_back");
    assert_eq!(ar.first_person.len(), 2);
    assert!(ar.first_person[0].animations.contains(r"masterchief\fp"));
    let p = ar.barrels[0].projectile.as_ref().expect("ar projectile");
    assert!(close(p.initial_velocity, 80.0));
    assert!(close(p.damage.as_ref().expect("ar damage").upper_bound[0], 7.5));
    let muzzle = ar.muzzle().expect("ar muzzle");
    assert_eq!(muzzle.name, "primary_trigger");
    assert!(close(muzzle.position.x, 0.2334));
    let left = ar.marker("left_hand").expect("ar left_hand");
    assert!(close(left.position.x, 0.1264));
    assert!(ar.root.abs_diff_eq(glam::Mat4::IDENTITY, 1e-4));

    let br = by_name("battle_rifle");
    assert_eq!(br.magazines[0].rounds_loaded_maximum, 36);
    assert_eq!(br.barrels[0].shots_per_fire, [3, 3]);
    assert_eq!(br.triggers[0].behavior, 4);
    assert_eq!(br.zoom_levels, 1);

    let sniper = by_name("sniper_rifle");
    assert_eq!(sniper.magazines[0].rounds_loaded_maximum, 4);
    assert_eq!(sniper.zoom_levels, 2);
    assert_eq!(sniper.zoom_range, [4.0, 9.0]);
    let sp = sniper.barrels[0].projectile.as_ref().expect("sniper projectile");
    assert!(sp.is_near_hitscan());
    assert!(close(sp.damage.as_ref().expect("sniper damage").upper_bound[0], 80.0));

    let laser = by_name("spartan_laser");
    assert_eq!(laser.barrels.len(), 2);
    assert_eq!(laser.barrels[0].magazine, None);

    // Every placed weapon has a model with a muzzle and the same root.
    let placements = weapons::item_placements(&cache).expect("placements read");
    let weapons_placed: Vec<_> = placements.iter().filter(|p| p.kind == ItemKind::Weapon).collect();
    let equipment = placements.len() - weapons_placed.len();
    assert_eq!(weapons_placed.len(), 42, "weapon placements");
    assert_eq!(equipment, 32, "equipment placements");
    let mut per_tag: BTreeMap<&str, usize> = BTreeMap::new();
    for p in &weapons_placed {
        *per_tag.entry(p.tag_path.rsplit('\\').next().unwrap()).or_default() += 1;
    }
    let expected: BTreeMap<&str, usize> = [
        ("battle_rifle", 19),
        ("smg", 6),
        ("spike_rifle", 4),
        ("plasma_pistol", 4),
        ("sniper_rifle", 2),
        ("missile_pod", 2),
        ("shotgun", 2),
        ("brute_shot", 2),
        ("spartan_laser", 1),
    ]
    .into_iter()
    .collect();
    assert_eq!(per_tag, expected);
    let laser_at = weapons_placed.iter().find(|p| p.tag_path.ends_with("spartan_laser")).unwrap();
    assert!(close(laser_at.position.x, 82.80) || (laser_at.position.x - 82.80).abs() < 0.01);
    assert_eq!(laser_at.respawn_seconds, 180);
    for p in &weapons_placed {
        let w = all.iter().find(|w| Some(w.tag_index) == p.tag).expect("placed weapon read");
        assert!(w.render_model.is_some(), "{} has a model", w.tag_path);
        assert!(w.muzzle().is_some(), "{} has a muzzle", w.tag_path);
        // The model origin is the grip: a weapon's own right_hand marker
        // sits on it (sniper 1.1 cm, shotgun 0.7 cm, brute shot / missile
        // pod about 3 cm off), wherever its root node is.
        if let Some(grip) = w.grip() {
            let d = grip.model.w_axis.truncate().length();
            assert!(d < 0.04, "{} grip {d} wu from origin", w.tag_path);
        }
    }

    // The Spartan's hand: right_hand rides on r_hand.
    let spartan = weapons::spartan_markers(&cache).unwrap().expect("mp_masterchief present");
    let hand = weapons::find_marker(&spartan, weapons::RIGHT_HAND_MARKER).expect("right_hand");
    assert_eq!(hand.node_name, "r_hand");
    assert!(close(hand.position.x, 0.0465));
    let back = weapons::find_marker(&spartan, "weapon_back").expect("weapon_back");
    assert_eq!(back.node_name, "spine1");
}

#[test]
fn valhalla_weapon_meshes_decode() {
    let Some(path) = map("riverworld") else { return };
    let cache = CacheFile::open(&path).expect("riverworld opens");
    let resources = cache.resources().expect("resources");
    let all = weapons::weapons(&cache).expect("weapons read");
    let placed: Vec<u32> = weapons::weapon_placements(&cache)
        .unwrap()
        .iter()
        .filter_map(|p| p.tag)
        .collect();
    let mut counts = BTreeMap::new();
    for w in all.iter().filter(|w| placed.contains(&w.tag_index)) {
        let name = w.tag_path.rsplit('\\').next().unwrap().to_string();
        let mut scene = blam_geometry::Scene::default();
        let mut materials = blam_geometry::materials::MaterialTable::default();
        let third = weapons::decode_model(&cache, &resources, &mut materials, w.render_model.unwrap(), &mut scene)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!third.pieces.is_empty(), "{name} third person");
        let t3 = scene.triangle_count_of_meshes();
        let fp = w.first_person.first().and_then(|f| f.render_model).map(|m| {
            let mut scene = blam_geometry::Scene::default();
            let mut materials = blam_geometry::materials::MaterialTable::default();
            weapons::decode_model(&cache, &resources, &mut materials, m, &mut scene)
                .unwrap_or_else(|e| panic!("{name} first person: {e}"));
            scene.triangle_count_of_meshes()
        });
        eprintln!("{name}: third person {t3} triangles, first person {fp:?}");
        counts.insert(name, (t3, fp));
    }
    // Triangles after strip decoding, first permutation of every region.
    let expected: BTreeMap<String, (usize, Option<usize>)> = [
        ("battle_rifle", 2949, Some(2949)),
        ("spartan_laser", 2478, Some(2511)),
        ("shotgun", 2043, Some(2249)),
        ("smg", 1840, Some(2086)),
        ("sniper_rifle", 2358, Some(3090)),
        ("spike_rifle", 1872, Some(2352)),
        ("plasma_pistol", 1991, Some(1951)),
        ("missile_pod", 2220, None),
        ("brute_shot", 2865, Some(3553)),
    ]
    .into_iter()
    .map(|(n, t, f)| (n.to_string(), (t, f)))
    .collect();
    assert_eq!(counts, expected);
}

/// The Mongoose's splatter numbers on 120_halo: the global collision damage
/// effect deals 100, its `cddf` (`vehicle_small`) scales by 1.2 over 3..8
/// wu/s. The dirt bike uses these (`mcc_ai`).
#[test]
fn mongoose_collision_damage() {
    let Some(path) = map("120_halo") else { return };
    let cache = CacheFile::open(&path).expect("120_halo opens");
    let c = weapons::read_vehicle_collision(&cache, r"objects\vehicles\mongoose\mongoose")
        .expect("read")
        .expect("the Mongoose and the collision effect are on 120_halo");
    assert_eq!(c.effect_tag, r"globals\collision_damage\collision");
    assert_eq!(c.vehicle_tag, r"globals\collision_damage\vehicle_small");
    assert!(close(c.damage[0], 100.0) && close(c.damage[1], 100.0), "{:?}", c.damage);
    assert!(close(c.scale, 1.2), "{}", c.scale);
    assert!(close(c.speeds[0], 3.0) && close(c.speeds[1], 8.0), "{:?}", c.speeds);
}
