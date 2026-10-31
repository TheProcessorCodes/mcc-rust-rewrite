//! The scripts of the user's own maps: counts pinned from
//! `docs/format/halo-script.md`. Skips quietly when a map is not installed.

use std::path::PathBuf;

use blam_script::{NodeKind, ScriptData, ScriptType};

fn open(name: &str) -> Option<blam_cache::CacheFile> {
    let dir = std::env::var_os("HALO3_MAPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps")
    });
    let p = dir.join(format!("{name}.map"));
    p.exists().then(|| blam_cache::CacheFile::open(p).expect("map opens"))
}

#[test]
fn halo_scripts_counts() {
    let Some(cache) = open("120_halo") else { return };
    let d = ScriptData::read(&cache).expect("scripts read");
    assert_eq!(d.scripts.len(), 731);
    assert_eq!(d.globals.len(), 113);
    assert_eq!(d.nodes.len(), 54_952);
    assert_eq!(d.nodes.iter().filter(|n| !n.is_free()).count(), 50_390);
    assert_eq!(d.strings.len(), 80_186);
    assert_eq!(d.references.len(), 69);
    assert_eq!(d.cutscene_flags.len(), 86);
    assert_eq!(d.point_sets.len(), 97);
    assert_eq!(d.folders.len(), 54);
    assert_eq!(d.object_names.len(), 1362);
    assert_eq!(d.function_counts().len(), 281);

    // Kinds of node.
    let kind = |k: NodeKind| d.nodes.iter().filter(|n| !n.is_free() && n.kind() == k).count();
    assert_eq!(kind(NodeKind::Call), 13_310);
    assert_eq!(kind(NodeKind::ScriptCall), 2_377);
    assert_eq!(kind(NodeKind::Global), 1_230);
    assert_eq!(kind(NodeKind::Parameter), 107);

    // A few scripts by name, type and shape.
    let s = &d.scripts[d.script_index("mission_halo").unwrap()];
    assert_eq!(s.kind, ScriptType::Startup);
    let s = &d.scripts[d.script_index("fall_off").unwrap()];
    assert_eq!(s.kind, ScriptType::Static);
    assert_eq!(s.parameters.len(), 1);
    assert_eq!(s.parameters[0].name, "obj_name");
    let src = blam_script::Decompiler::new(&d).script(d.script_index("fall_off").unwrap());
    assert_eq!(
        src,
        "(script static void fall_off ((object obj_name))\n  (sleep (random_range 0 5))\n  (object_damage_damage_section obj_name fall 1.0))"
    );
    let src = blam_script::Decompiler::new(&d).script(d.script_index("trench_timer_100_001").unwrap());
    assert!(src.contains("(fall_off 100_001f)"), "{src}");
    assert!(src.contains("(sleep trench_speed)"), "{src}");

    // Function names come from the map: opcodes are the engine's indices.
    let f = d.function_names();
    assert_eq!(f.get(&0).map(String::as_str), Some("begin"));
    assert_eq!(f.get(&19).map(String::as_str), Some("sleep"));
    assert_eq!(f.get(&71).map(String::as_str), Some("object_create"));
    assert_eq!(f.get(&116).map(String::as_str), Some("object_damage_damage_section"));

    // The names scripts point at.
    assert_eq!(d.cutscene_flags[35].name, "cin_120lb_anchor");
    assert_eq!(d.folders[1].name, "trench_run_100");
    assert_eq!(d.folders[12].name, "trench_run_100_crates");
    let p = d.point_sets.iter().find(|p| p.name == "big_explosions_100_02").unwrap();
    assert_eq!(p.points.len(), 9);
    assert_eq!(p.bsp, 7);
    let o = &d.object_names[d.object_index("100_001a").unwrap()];
    assert_eq!((o.object_type, o.placement), (6, 94));

    // Every script decompiles without a dangling node.
    let src = d.source();
    assert!(!src.contains("<free node"), "dangling node in the source");
    assert!(!src.contains("<function>"));
}

#[test]
fn other_maps_read() {
    for name in ["010_jungle", "riverworld"] {
        let Some(cache) = open(name) else { continue };
        let d = ScriptData::read(&cache).expect("scripts read");
        let src = d.source();
        assert!(!src.contains("<free node"), "{name}: dangling node");
        assert!(!src.contains("<function>"), "{name}: call without a name");
    }
}
