//! Halo CE's AI on The Maw (`d40`): every encounter and squad the scripts
//! name resolves, and the actor variants the Warthog run places read with
//! a model, a team, vitality and (where they carry one) a weapon that
//! fires something (`docs/format/halo1.md` 13). Skips quietly when the map
//! is not installed.

use std::collections::BTreeSet;
use std::path::PathBuf;

use blam_h1::ai::{self, AiName};

fn d40() -> Option<PathBuf> {
    let p = game_paths::halo1_maps()?.join("d40.map");
    p.exists().then_some(p)
}

/// Every AI constant (`encounter` or `encounter/squad`) in the map's
/// scripts.
fn script_ai_names(map: &blam_h1::Map) -> BTreeSet<String> {
    let data = blam_h1::script::read(map).unwrap();
    data.nodes
        .iter()
        .filter(|n| n.value_type == blam_script::ValueType::AI && n.flags & 2 == 0)
        .filter_map(|n| data.string(n.string))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn the_maw_script_ai_names_resolve() {
    let Some(path) = d40() else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let ai = ai::read(&map).unwrap();
    assert_eq!(ai.encounters.len(), 56);
    assert_eq!(ai.palette.len(), 35);
    let names = script_ai_names(&map);
    assert!(names.contains("enc6_1_flood/squada"), "{names:?}");
    assert!(names.contains("enc6_0_flood"), "{names:?}");
    let unresolved: Vec<&String> = names.iter().filter(|n| ai.resolve(n).is_none()).collect();
    assert!(unresolved.is_empty(), "unresolved: {unresolved:?}");
    // Case does not matter: the squad is stored as `squadA`.
    let Some(AiName::Squad(e, s)) = ai.resolve("enc6_1_flood/squada") else { panic!() };
    assert_eq!(ai.encounters[e].squads[s].name, "squadA");
    assert_eq!(ai.encounters[e].bsp, 5);
    // `ai_defend enc6_5_cov/evacs` names a platoon: its squads.
    let Some(AiName::Platoon(e, p)) = ai.resolve("enc6_5_cov/evacs") else { panic!() };
    let members: Vec<&str> = ai.encounters[e]
        .squads
        .iter()
        .filter(|s| s.platoon == Some(p as u16))
        .map(|s| s.name.as_str())
        .collect();
    assert!(members.contains(&"evacs1") && members.contains(&"evacs2"), "{members:?}");
    let Some(AiName::Encounter(e)) = ai.resolve("enc7_7_flood") else { panic!() };
    assert_eq!(ai.encounters[e].bsp, 7);
    println!("{} AI names in the scripts, all resolve", names.len());
}

#[test]
fn the_maw_run_actor_variants_read() {
    let Some(path) = d40() else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let ai = ai::read(&map).unwrap();
    // The actor variants the run's encounters place.
    let mut used = BTreeSet::new();
    for e in ai.encounters.iter().filter(|e| e.name.starts_with("enc6_") || e.name.starts_with("enc7_")) {
        for s in &e.squads {
            used.extend(s.actor);
            used.extend(s.locations.iter().filter_map(|l| l.actor));
        }
    }
    assert!(used.len() >= 10, "{used:?}");
    for i in used {
        let v = ai.palette[i as usize].as_ref().unwrap_or_else(|| panic!("palette {i} not read"));
        let u = ai::unit(&map, v.unit.unwrap()).unwrap();
        let model = u.model.unwrap_or_else(|| panic!("{}: no model", u.path));
        let nodes = blam_h1::skeleton::nodes(&map, model).unwrap();
        assert!(!nodes.is_empty(), "{}", u.path);
        assert!((1..=5).contains(&u.team), "{}: team {}", u.path, u.team);
        let body = if v.body_vitality > 0.0 { v.body_vitality } else { u.body_vitality };
        assert!(body > 0.0, "{}: no vitality", v.path);
        let weapon = v.weapon.map(|w| ai::weapon(&map, w).unwrap());
        if let Some(w) = &weapon {
            let t = w.triggers.first().unwrap_or_else(|| panic!("{}: no trigger", w.path));
            let p = t.projectile.as_ref().unwrap_or_else(|| panic!("{}: no projectile", w.path));
            assert!(p.initial_velocity > 0.0, "{}", p.path);
        }
        println!(
            "{i:2} {:52} type {:2} team {} body {:6.2} shield {:5.1} {} nodes, height {:.2}, weapon {}",
            v.path,
            v.actor_type,
            u.team,
            body,
            if v.shield_vitality > 0.0 { v.shield_vitality } else { u.shield_vitality },
            nodes.len(),
            u.height,
            weapon.map_or("none".to_string(), |w| w.path)
        );
    }
    // The Flood combat elite with an assault rifle: its numbers.
    let v = ai.palette[10].as_ref().unwrap();
    assert!(v.path.ends_with("floodcombat elite assault rifle"));
    assert_eq!(v.actor_type, 9);
    assert_eq!(v.body_vitality, 120.0);
    let w = ai::weapon(&map, v.weapon.unwrap()).unwrap();
    assert_eq!(w.triggers[0].rounds_per_second, [15.0, 15.0]);
    let p = w.triggers[0].projectile.as_ref().unwrap();
    assert!((p.initial_velocity - 324.0).abs() < 1.0, "{}", p.initial_velocity);
    assert_eq!(p.damage.as_ref().unwrap().upper[0], 10.0);
}

#[test]
fn the_maw_skeletons_stand_up() {
    let Some(path) = d40() else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let elite = map.find(b"mod2", r"characters\floodcombat elite\floodcombat elite").unwrap().id;
    let nodes = blam_h1::skeleton::nodes(&map, elite).unwrap();
    // Parents come first.
    for (i, n) in nodes.iter().enumerate() {
        assert!(n.parent.is_none_or(|p| (p as usize) < i), "{}", n.name);
    }
    let frames = blam_h1::skeleton::model_frames(&nodes);
    let at = |name: &str| frames[nodes.iter().position(|n| n.name == name).unwrap()].w_axis.truncate();
    // The head over the pelvis, the left foot on the left, near the ground.
    assert!(at("bip01 head").z > at("bip01 pelvis").z + 0.2);
    assert!(at("bip01 l foot").y > 0.0 && at("bip01 r foot").y < 0.0);
    assert!(at("bip01 l foot").z < 0.15, "{:?}", at("bip01 l foot"));
    // Skinned vertices sit near the nodes they ride on.
    let models = blam_h1::decode_models(&map, &[elite]);
    let mesh = &models.scene.meshes[models.meshes[0][0] as usize];
    let skin = mesh.skin.as_ref().unwrap();
    assert_eq!(skin.joints.len(), mesh.positions.len());
    let far = mesh
        .positions
        .iter()
        .zip(&skin.joints)
        .filter(|(p, j)| p.distance(frames[j[0] as usize].w_axis.truncate()) > 0.6)
        .count();
    assert!(far * 20 < mesh.positions.len(), "{far} of {} vertices far from their node", mesh.positions.len());
}

#[test]
fn the_maw_animation_graphs_read() {
    let Some(path) = d40() else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let ai = ai::read(&map).unwrap();
    let mut graphs = BTreeSet::new();
    for v in ai.palette.iter().flatten() {
        if let Some(u) = v.unit.and_then(|u| ai::unit(&map, u).ok()) {
            graphs.extend(u.animations.map(|a| (a, u.model)));
        }
    }
    for (antr, mode) in graphs {
        let g = blam_h1::animation::read(&map, antr).unwrap();
        let read = g.animations.iter().flatten().count();
        assert_eq!(read, g.animations.len(), "{}: {} of {} read", map.tag_name(antr).unwrap(), read, g.animations.len());
        // The graph's nodes are the model's, in order.
        let nodes = blam_h1::skeleton::nodes(&map, mode.unwrap()).unwrap();
        assert!(g.nodes.iter().zip(&nodes).all(|(a, b)| *a == b.name), "{}", map.tag_name(antr).unwrap());
        for a in g.animations.iter().flatten() {
            assert!(a.frames.iter().all(|p| (p.rotation.length() - 1.0).abs() < 1e-3), "{}", a.name);
        }
        println!("{:55} {} nodes, {} animations", map.tag_name(antr).unwrap(), g.nodes.len(), read);
    }
    // The Flood combat elite: its idle stands where its model stands, its
    // run moves it forward.
    let antr = map.find(b"antr", r"characters\floodcombat elite\floodcombat elite").unwrap().id;
    let g = blam_h1::animation::read(&map, antr).unwrap();
    let idle = g.animations[g.find("stand pistol idle").unwrap()].as_ref().unwrap();
    let pelvis = idle.pose(0)[0].translation;
    assert!(pelvis.distance(glam::Vec3::new(0.015, 0.0, 0.44)) < 0.15, "{pelvis:?}");
    let run = g.animations[g.find("stand pistol move-front").unwrap()].as_ref().unwrap();
    let speed = run.root_speed();
    assert!((0.5..5.0).contains(&speed), "{speed}");
    assert!(run.root_motion.iter().map(|m| m[0]).sum::<f32>() > 0.0);
    println!("flood elite run: {speed:.2} wu/s");
}
