//! Halo CE's scripts on The Maw (`d40`): read into Halo 3's `ScriptData`
//! and decompiled; the Warthog run's countdown, finish and BSP switches are
//! where `docs/format/halo1.md` 12 says. Skips quietly when the map is not
//! installed.

use std::path::PathBuf;

fn d40() -> Option<PathBuf> {
    let p = game_paths::halo1_maps()?.join("d40.map");
    p.exists().then_some(p)
}

#[test]
fn the_maw_scripts_decompile() {
    let Some(path) = d40() else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let data = blam_h1::script::read(&map).unwrap();
    assert_eq!(data.scripts.len(), 298);
    assert_eq!(data.globals.len(), 66);
    let d = blam_script::Decompiler::new(&data);
    let source = |name: &str| d.script(data.script_index(name).unwrap());
    let end = source("test_for_endgame");
    assert!(end.contains("(volume_test_objects grand_finale (players))"), "{end}");
    let timer = source("timer_begin");
    assert!(timer.contains("(hud_set_timer_time timer_minutes timer_seconds)"), "{timer}");
    let g = |name: &str| d.expression(data.globals.iter().find(|g| g.name == name).unwrap().init);
    assert_eq!(g("timer_minutes"), "6");
    assert_eq!(g("timer_seconds"), "0");
    // Script calls name the script; trigger volumes their volume.
    assert!(source("enc6_6_1").contains("(trench_scene)"));
    assert!(data.trigger_volumes.iter().any(|v| v == "grand_finale"));
    assert!(data.object_names.iter().any(|o| o.name == "trench_jeep1" && o.object_type == 1));
}

#[test]
fn the_maw_switches_bsp_by_volume() {
    let Some(path) = d40() else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let options = blam_h1::Options {
        textures: false,
        objects: false,
        bsps: Some(vec![5, 6, 7]),
        ..Default::default()
    };
    let loaded = blam_h1::decode(&map, &options).unwrap();
    let scene = &loaded.scene;
    let names: Vec<&str> = scene.zone_sets.iter().map(|z| z.name.as_str()).collect();
    assert_eq!(names, ["d40f", "d40g", "d40h"]);
    // `bsp 5,6` (twice), `bsp 6,5` and `bsp 6,7` lead between the three.
    let switches: Vec<(String, u32)> = scene
        .zone_switches
        .iter()
        .map(|z| (scene.trigger_volumes[z.volume as usize].name.clone(), z.commit.unwrap()))
        .collect();
    assert!(switches.contains(&("bsp 6,7".into(), 2)), "{switches:?}");
    assert!(switches.contains(&("bsp 6,5".into(), 0)), "{switches:?}");
    assert_eq!(switches.iter().filter(|(n, _)| n == "bsp 5,6").count(), 2);
    // The jeeps the run starts from stand in the first.
    let jeep = blam_h1::object_named(&map, "trench_jeep1").unwrap();
    let b = &scene.bsps[0].bounds;
    assert!(jeep.position.cmpge(b.0).all() && jeep.position.cmple(b.1).all());
}
