//! The scenario's AI and the characters and units behind it, on the user's
//! own maps (`docs/format/ai.md`). Skips quietly when a map is not
//! installed.

use std::path::PathBuf;

use blam_cache::CacheFile;
use blam_geometry::ai::{self, team};

fn map(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("HALO3_MAPS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(
                r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps",
            )
        });
    let p = dir.join(format!("{name}.map"));
    p.exists().then_some(p)
}

fn short(path: &str) -> &str {
    path.rsplit('\\').next().unwrap_or(path)
}

#[test]
fn halo_squads_and_zones() {
    let Some(path) = map("120_halo") else { return };
    let cache = CacheFile::open(&path).expect("120_halo opens");
    let data = ai::read(&cache).expect("AI read");

    assert_eq!(data.squad_groups.len(), 19, "squad groups");
    assert_eq!(data.squads.len(), 262, "squads");
    let cells: usize = data.squads.iter().map(|s| s.cells.len()).sum();
    let locations: usize = data.squads.iter().flat_map(|s| &s.cells).map(|c| c.locations.len()).sum();
    assert_eq!(cells, 267, "cells");
    assert_eq!(locations, 872, "starting locations");
    assert_eq!(data.zones.len(), 11, "AI zones");
    let fps: usize = data.zones.iter().map(|z| z.firing_positions.len()).sum();
    assert_eq!(fps, 3842, "firing positions");
    assert_eq!(data.characters.iter().flatten().count(), 15, "characters");

    // Groups nest.
    let sentinels = data.group("sentinels").expect("sentinels group");
    let left = data.group("sentinel_left").expect("sentinel_left group");
    assert_eq!(data.squad_groups[left].parent, Some(sentinels as u16));
    assert_eq!(data.squads_in_group(data.group("all").unwrap()).len(), 216);

    // 343 Guilty Spark: the monitor with his beam, in the ziggurat's BSP.
    let spark = data.squad("343_spark").expect("343_spark");
    let cell = &spark.cells[0];
    assert_eq!(cell.count, 1);
    let ch = data.character_of(cell, None).expect("character");
    assert_eq!(short(&ch.tag_path), "monitor");
    let weapon = data.weapon_of(cell, None).and_then(|t| cache.tags.get(t)).expect("weapon");
    assert_eq!(short(weapon.name()), "monitor_beam");
    let loc = &cell.locations[0];
    assert!((loc.position.x + 118.99).abs() < 0.01 && (loc.position.y + 441.12).abs() < 0.01, "{:?}", loc.position);
    assert_eq!(loc.bsp, Some(2));

    // A combat form at the ziggurat: a brute form with a spike rifle.
    let c1 = data.squad("combat_01").expect("combat_01");
    assert_eq!(data.squad_groups[c1.group.unwrap() as usize].name, "exterior_meteorites");
    assert_eq!(data.zones[c1.zone.unwrap() as usize].name, "ziggurat");
    let cell = &c1.cells[0];
    assert_eq!(short(&data.character_of(cell, None).unwrap().tag_path), "floodcombat_brute");
    assert_eq!(short(cache.tags.get(data.weapon_of(cell, None).unwrap()).unwrap().name()), "spike_rifle");
    let l = &cell.locations[0];
    assert!((l.position.x + 210.70).abs() < 0.01 && (l.position.y + 425.83).abs() < 0.01, "{:?}", l.position);
    assert_eq!(l.bsp, Some(2));

    // Trench sentinels: eight, in BSP 110 (index 8), zone trench_run_110.
    let t = data.squad("trench_sentinel_110_1").expect("trench sentinels");
    assert_eq!(t.cells[0].count, 8);
    assert_eq!(t.bsp_mask(), 1 << 8);
    assert_eq!(data.zones[t.zone.unwrap() as usize].name, "trench_run_110");
    // Trench flood store no BSP on their locations: their zone's counts.
    let f = data.squad("trench_combat_forms01").expect("trench combat forms");
    assert_eq!(f.bsp_mask(), 0);
    assert_eq!(data.squad_bsps(f), 1 << 7);

    // The finale's boss forms override character and weapon per location.
    let boss = data.squad("ziggurat_finale_center_boss").expect("boss squad");
    let over: Vec<_> = boss.cells[0]
        .locations
        .iter()
        .map(|l| {
            (
                short(&data.character_of(&boss.cells[0], Some(l)).unwrap().tag_path).to_string(),
                short(cache.tags.get(data.weapon_of(&boss.cells[0], Some(l)).unwrap()).unwrap().name()).to_string(),
            )
        })
        .collect();
    assert_eq!(
        over,
        [
            ("floodcombat_elite".to_string(), "flak_cannon".to_string()),
            ("flood_combat_human".to_string(), "rocket_launcher".to_string())
        ]
    );

    // Zones: BSPs and areas.
    let z = &data.zones[1];
    assert_eq!((z.name.as_str(), z.bsp), ("ziggurat", Some(2)));
    assert_eq!((z.firing_positions.len(), z.areas.len()), (1004, 81));
    assert_eq!((z.areas[1].first_firing_position, z.areas[1].firing_position_count), (13, 15));
    let trench = data.zones.iter().find(|z| z.name == "trench_run_100").unwrap();
    assert_eq!(trench.bsp, Some(7));
}

#[test]
fn halo_characters_and_units() {
    let Some(path) = map("120_halo") else { return };
    let cache = CacheFile::open(&path).expect("120_halo opens");
    let data = ai::read(&cache).expect("AI read");
    let by = |n: &str| {
        data.characters
            .iter()
            .flatten()
            .find(|c| short(&c.tag_path) == n)
            .unwrap_or_else(|| panic!("{n} missing"))
    };
    let unit = |c: &ai::Character| ai::read_unit(&cache, cache.tags.get(c.unit.unwrap()).unwrap()).unwrap();

    // Combat forms inherit from floodcombat_base.
    let human = by("flood_combat_human");
    assert_eq!(short(&human.parents[0]), "floodcombat_base");
    assert_eq!(human.actor_type, 9);
    assert_eq!((human.vitality.body, human.vitality.legendary_body), (40.0, 80.0));
    assert_eq!(human.perception.max_vision_distance, 30.0);
    assert_eq!(human.perception.hearing_distance, 50.0);
    assert_eq!(human.charge.melee_consider_range, 14.0);
    assert!((human.charge.melee_attack_range - 1.2).abs() < 1e-5);
    let spike = human.weapons.iter().find(|w| short(&w.weapon_path) == "spike_rifle").unwrap();
    assert_eq!(spike.normal_combat_range, [2.5, 7.0]);
    assert_eq!(spike.maximum_firing_range, 14.0);
    let pattern = human.firing_pattern(spike.weapon).unwrap();
    assert_eq!(pattern.rate_of_fire, 4.0);
    assert_eq!(pattern.burst_separation, [1.4, 2.8]);
    let u = unit(human);
    assert_eq!(u.team, team::FLOOD);
    assert_eq!((u.height_standing, u.radius), (0.6, 0.18));
    assert!(!u.flies);
    let melee = blam_geometry::weapons::read_damage(&cache, cache.tags.get(u.melee_damage.unwrap()).unwrap()).unwrap();
    assert_eq!(melee.upper_bound, [25.0, 25.0]);

    // Sentinels fly and carry their beam.
    let sentinel = by("sentinel_aggressor");
    assert_eq!(sentinel.actor_type, 13);
    assert_eq!(sentinel.vitality.body, 25.0);
    let u = unit(sentinel);
    assert_eq!(u.team, team::SENTINEL);
    assert!(u.flies);
    assert_eq!(short(cache.tags.get(u.weapons[0]).unwrap().name()), "sentinel_gun");

    // Infection forms are creatures: a model's single hit point.
    let infection = by("flood_infection");
    let u = unit(infection);
    assert_eq!(u.class, "crea");
    assert_eq!(u.team, team::FLOOD);
    assert_eq!(u.body_vitality, 1.0);
    assert!(u.melee_damage.is_some());

    // 343 Guilty Spark and the Arbiter.
    assert_eq!(unit(by("monitor")).team, team::GUILTY);
    let arbiter = by("dervish");
    assert_eq!((arbiter.vitality.body, arbiter.vitality.shield), (200.0, 160.0));
    assert_eq!(unit(arbiter).team, team::COVENANT);

    // The player: the Chief's model gives 45 body, 70 shield.
    let chief = cache.tags.find("hlmt", r"objects\characters\masterchief\masterchief").unwrap();
    assert_eq!(ai::model_vitality(&cache, chief).unwrap(), (45.0, 70.0));
}

#[test]
fn outskirts_reads_too() {
    let Some(path) = map("030_outskirts") else { return };
    let cache = CacheFile::open(&path).expect("030_outskirts opens");
    let data = ai::read(&cache).expect("AI read");
    assert_eq!(data.squad_groups.len(), 60);
    assert_eq!(data.squads.len(), 171);
    let locations: usize = data.squads.iter().flat_map(|s| &s.cells).map(|c| c.locations.len()).sum();
    assert_eq!(locations, 500);
    assert_eq!(data.zones.len(), 18);
    let marines = data.squad("sq_elev_mar").expect("sq_elev_mar");
    assert_eq!(short(&data.character_of(&marines.cells[0], None).unwrap().tag_path), "marine");
}

#[test]
fn multiplayer_maps_have_no_squads() {
    let Some(path) = map("riverworld") else { return };
    let cache = CacheFile::open(&path).expect("riverworld opens");
    let data = ai::read(&cache).expect("AI read");
    assert!(data.squads.is_empty());
}
