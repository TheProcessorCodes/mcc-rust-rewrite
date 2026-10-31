//! Weapon sounds on Valhalla (`riverworld`), checked against the user's MCC
//! install (skipped when it is not there). The names are the ones in
//! `docs/format/weapon-sounds.md` 8, found by
//! `docs/format/scripts/probe_weapon_sounds.py`.

use blam_cache::CacheFile;
use blam_sound::tags::{SoundTags, distance_gain};
use blam_sound::weapons::{WeaponSoundReader, WeaponSounds};

fn map() -> Option<CacheFile> {
    CacheFile::open(game_paths::halo3_map("riverworld")?).ok()
}

/// Tag names with the common `sound\` prefix cut off.
fn names(c: &CacheFile, tags: &[u32]) -> Vec<String> {
    tags.iter()
        .map(|t| {
            let n = c.tags.get(*t).unwrap().name();
            n.strip_prefix("sound\\").unwrap_or(n).to_string()
        })
        .collect()
}

fn name(c: &CacheFile, tag: Option<u32>) -> Option<String> {
    tag.map(|t| names(c, &[t]).remove(0))
}

fn weapon<'a>(all: &'a [WeaponSounds], short: &str) -> &'a WeaponSounds {
    all.iter()
        .find(|w| w.name.rsplit('\\').next() == Some(short))
        .unwrap_or_else(|| panic!("no weapon {short}"))
}

#[test]
fn riverworld_weapon_sounds() {
    let Some(c) = map() else { return };
    let mut all: Vec<WeaponSounds> = WeaponSoundReader::new(&c)
        .all()
        .unwrap()
        .into_values()
        .collect();
    all.sort_by_key(|w| w.weapon);
    assert_eq!(all.len(), 47);
    let n = |v: &Vec<u32>| names(&c, v);

    let br = weapon(&all, "battle_rifle");
    assert_eq!(br.weapon, 990);
    assert_eq!(
        n(&br.fire),
        [
            r"weapons\battle_rifle\br_lod_far",
            r"weapons\battle_rifle\eject_br",
            r"weapons\battle_rifle\fire_burst_h3"
        ]
    );
    assert_eq!(n(&br.empty), [r"weapons\battle_rifle\dryfire"]);
    assert_eq!(
        n(&br.reload),
        [r"weapons\battle_rifle\battle_rifle_fp\br_reload_full"]
    );
    assert_eq!(
        n(&br.ready),
        [r"weapons\battle_rifle\battle_rifle_fp\br_ready"]
    );
    assert_eq!(n(&br.impact), [r"weapons\impacts_riccs\bullet_impact_dirt"]);
    assert!(br.fire_charged.is_empty() && br.charge.is_none() && br.fire_loops.is_empty());
    assert_eq!(
        name(&c, br.ammo_pickup).as_deref(),
        Some(r"weapons\battle_rifle\battle_rifle_ammo")
    );

    let ar = weapon(&all, "assault_rifle");
    assert_eq!(
        n(&ar.fire),
        [
            r"weapons\assault_rifle\ar_fire",
            r"weapons\assault_rifle\assault_rifle_lod_far"
        ]
    );
    assert_eq!(n(&ar.reload), [r"weapons\assault_rifle\ar_reload"]);
    assert_eq!(n(&ar.ready), [r"weapons\assault_rifle\ar_ready"]);

    // The SMG's shot is a loop held while firing, not a per-round sound.
    let smg = weapon(&all, "smg");
    assert!(smg.fire.is_empty());
    assert_eq!(
        names(&c, &smg.fire_loops),
        [
            r"weapons\smg\smg_loop\smg_loop",
            r"weapons\smg\smg_lod_far\smg_lod_far"
        ]
    );
    assert_eq!(n(&smg.empty), [r"weapons\smg\dryfire"]);
    assert_eq!(n(&smg.reload), [r"weapons\smg\smg_fp\smg_reload"]);
    assert_eq!(n(&smg.ready), [r"weapons\smg\smg_fp\smg_ready"]);

    let sniper = weapon(&all, "sniper_rifle");
    assert_eq!(
        n(&sniper.fire),
        [
            r"weapons\sniper_rifle\sniper_fire_h3",
            r"weapons\sniper_rifle\sniper_eject",
            r"weapons\sniper_rifle\fire_lod_far"
        ]
    );
    assert_eq!(
        n(&sniper.reload),
        [r"weapons\sniper_rifle\sniper_rifle_fp\sr_reload"]
    );

    let shotgun = weapon(&all, "shotgun");
    assert_eq!(
        n(&shotgun.fire),
        [r"weapons\shotgun\fire", r"weapons\shotgun\shotgun_lod_far"]
    );
    assert_eq!(
        n(&shotgun.fire_first_person),
        [r"weapons\shotgun\shotgun_fp\sg_fire"]
    );
    assert_eq!(
        n(&shotgun.reload),
        [r"weapons\shotgun\shotgun_fp\sg_reload_loop"]
    );
    assert_eq!(n(&shotgun.ready), [r"weapons\shotgun\shotgun_fp\sg_ready"]);

    let pp = weapon(&all, "plasma_pistol");
    assert_eq!(
        n(&pp.fire),
        [
            r"weapons\plasma_pistol\fire",
            r"weapons\plasma_pistol\plasma_pistol_lod_far"
        ]
    );
    assert_eq!(
        n(&pp.fire_charged),
        [
            r"weapons\plasma_pistol\chargefire",
            r"weapons\plasma_pistol\plasma_charge_lod_far"
        ]
    );
    assert_eq!(n(&pp.empty), [r"weapons\plasma_pistol\plasma_dryfire"]);
    assert!(pp.reload.is_empty());
    assert_eq!(
        name(&c, pp.charge).as_deref(),
        Some(r"weapons\plasma_pistol\plasmapistol_charging\plasmapistol_charging")
    );
    assert_eq!(
        n(&pp.impact),
        [r"weapons\impacts_riccs\plasma_impacts_h3_up"]
    );
    assert_eq!(
        n(&pp.impact_charged),
        [r"weapons\plasma_pistol\plasma_charge_hit"]
    );

    let spiker = weapon(&all, "spike_rifle");
    assert_eq!(
        n(&spiker.fire),
        [
            r"weapons\spike_rifle\spike_rifle_fire",
            r"weapons\spike_rifle\spike_lod_far"
        ]
    );
    assert_eq!(
        n(&spiker.reload),
        [r"weapons\spike_rifle\fp_spike_rifle\fp_spike_rifle_reload_full"]
    );
    assert_eq!(
        n(&spiker.impact),
        [r"weapons\spike_rifle\spike_impacts_soft"]
    );

    let bs = weapon(&all, "brute_shot");
    assert_eq!(
        n(&bs.fire),
        [
            r"weapons\brute_shot\brute_shot_fire_new",
            r"weapons\brute_shot\brute_shot_lod_far"
        ]
    );
    assert_eq!(
        n(&bs.reload),
        [r"weapons\brute_shot\fp_brute_shot\fp_brute_shot_reload"]
    );
    assert_eq!(
        n(&bs.impact),
        [
            r"weapons\brute_shot\brute_round_lod_far",
            r"weapons\brute_shot\brute_round_explode",
            r"materials\tough\terrain_dirt\dirt_burst"
        ]
    );

    // No first-person graph: no reload or ready sound.
    let pod = weapon(&all, "missile_pod");
    assert_eq!(
        n(&pod.fire),
        [
            r"weapons\missile_launcher\missile_launch",
            r"weapons\rocket_launcher\rocket_launcher_lod_far"
        ]
    );
    assert!(pod.reload.is_empty() && pod.ready.is_empty());
    assert_eq!(
        n(&pod.impact),
        [
            r"visual_fx\h3_dist_explosion",
            r"visual_fx\h3_lrg_explosion",
            r"materials\tough\terrain_dirt\dirt_burst"
        ]
    );

    // The laser's primary barrel is the silent aiming tracer; its shot is
    // the secondary barrel, after charging.
    let laser = weapon(&all, "spartan_laser");
    assert!(laser.fire.is_empty());
    assert_eq!(
        n(&laser.fire_charged),
        [
            r"weapons\spartan_laser\spartan_laser_overheat",
            r"weapons\spartan_laser\fire",
            r"weapons\spartan_laser\spartan_laser_lod_far"
        ]
    );
    assert_eq!(n(&laser.empty), [r"weapons\battle_rifle\dryfire"]);
    assert_eq!(
        name(&c, laser.charge).as_deref(),
        Some(r"weapons\spartan_laser\charging\charging")
    );
    assert_eq!(
        n(&laser.impact_charged),
        [
            r"weapons\spartan_laser\spartan_laser_impacts_lod_far",
            r"weapons\spartan_laser\spartan_laser_impacts"
        ]
    );
    assert_eq!(
        n(&laser.ready),
        [r"weapons\spartan_laser\fp_spartan_laser\fp_spartan_laser_ready"]
    );
}

#[test]
fn riverworld_sound_classes_and_distances() {
    let Some(c) = map() else { return };
    let tags = SoundTags::new(&c).unwrap();
    let classes = tags.classes().unwrap();
    assert_eq!(classes.len(), 54);
    // Weapon fire, and its far-away version.
    assert_eq!((classes[4].max_per_tag, classes[4].max_per_object), (8, 2));
    assert_eq!(classes[4].distances, [0.0, 0.0, 3.0, 55.0]);
    assert_eq!(classes[15].distances, [3.0, 20.0, 35.0, 70.0]);

    let reader = WeaponSoundReader::new(&c);
    let ar = reader.weapon(280).unwrap();
    let near = tags.sound(ar.fire[0]).unwrap();
    let far = tags.sound(ar.fire[1]).unwrap();
    assert_eq!((near.class, far.class), (4, 15));
    // Both use their class's distances (flags 0x1F).
    let dn = near
        .playback
        .distances_with(classes.get(near.class as usize));
    let df = far.playback.distances_with(classes.get(far.class as usize));
    assert_eq!(dn, [0.0, 0.0, 3.0, 55.0]);
    assert_eq!(df, [3.0, 20.0, 35.0, 70.0]);
    // In the hand: the near shot at full volume, the far one silent.
    assert_eq!(distance_gain(0.0, dn), 1.0);
    assert_eq!(distance_gain(0.0, df), 0.0);
    assert_eq!(distance_gain(30.0, df), 1.0);

    // The sniper's far shot sets all four of its own distances (flags 0x10).
    let sniper = reader.weapon(1945).unwrap();
    let far = tags.sound(sniper.fire[2]).unwrap();
    assert_eq!(
        far.playback.distances_with(classes.get(far.class as usize)),
        [6.0, 45.0, 50.0, 100.0]
    );
    // Its near shot: class zeros, then its own 6 and 65.
    let near = tags.sound(sniper.fire[0]).unwrap();
    assert_eq!(
        near.playback
            .distances_with(classes.get(near.class as usize)),
        [0.0, 0.0, 6.0, 65.0]
    );
}
