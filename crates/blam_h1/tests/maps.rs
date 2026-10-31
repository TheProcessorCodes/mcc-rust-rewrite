//! Decodes the user's own Halo CE multiplayer maps and pins their numbers:
//! the render triangles and lightmaps against HaloExportH1's exports of the
//! same files (`D:\reflex-tools\halo-exports\h1-*`), the collision, spawns
//! and objects against our own probes (`docs/format/scripts/probe_h1_*.py`).
//! Skips quietly when the maps are not installed.

use std::path::PathBuf;

use blam_geometry::Kind;

fn maps_dir() -> PathBuf {
    std::env::var_os("HALO1_MAPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo1\maps")
    })
}

fn map(name: &str) -> Option<PathBuf> {
    let p = maps_dir().join(format!("{name}.map"));
    p.exists().then_some(p)
}

/// The 19 multiplayer maps the menu lists.
const MULTIPLAYER: [&str; 19] = [
    "beavercreek",
    "bloodgulch",
    "boardingaction",
    "chillout",
    "putput",
    "damnation",
    "dangercanyon",
    "deathisland",
    "carousel",
    "gephyrophobia",
    "hangemhigh",
    "icefields",
    "infinity",
    "longest",
    "prisoner",
    "ratrace",
    "sidewinder",
    "timberland",
    "wizard",
];

#[test]
fn halo1_maps_are_told_from_the_others() {
    if let Some(p) = map("sidewinder") {
        assert!(blam_h1::is_halo1(&p));
        assert!(!blam_h2::is_halo2(&p));
    }
    let mcc = maps_dir().join("..").join("..");
    for other in [r"halo3\maps\sidewinder.map", r"halo2\h2_maps_win64_dx11\coagulation.map", r"halo3\maps\riverworld.map"] {
        let p = mcc.join(other);
        if p.exists() {
            assert!(!blam_h1::is_halo1(&p), "{other}");
        }
    }
    if let Some(p) = map("bitmaps") {
        assert!(!blam_h1::is_halo1(&p));
    }
}

/// HaloExportH1's numbers: (map, lightmap meshes, render triangles, scenery
/// placed). Ours: (collision triangles, player starting locations).
const PINNED: [(&str, usize, usize, usize, usize, usize); 4] = [
    ("bloodgulch", 16, 5503, 47, 5940, 72),
    ("deathisland", 22, 19581, 90, 0, 0),
    ("sidewinder", 27, 10228, 42, 0, 0),
    ("timberland", 46, 6995, 168, 7160, 0),
];

#[test]
fn counts_match_haloexport() {
    for (name, lightmaps, triangles, scenery, collision, starts) in PINNED {
        let Some(p) = map(name) else { continue };
        let m = blam_h1::Map::open(&p).unwrap();
        let options = blam_h1::Options {
            textures: false,
            ..Default::default()
        };
        let l = blam_h1::decode(&m, &options).unwrap();
        let s = &l.stats;
        assert_eq!(s.lightmaps, lightmaps, "{name} lightmaps");
        assert_eq!(s.bsp_triangles, triangles, "{name} triangles");
        assert_eq!(s.objects.get(&Kind::Scenery).copied().unwrap_or(0), scenery, "{name} scenery");
        assert_eq!(s.corrupt_loops, 0, "{name} corrupt loops");
        if collision > 0 {
            assert_eq!(s.collision_triangles, collision, "{name} collision");
        }
        if starts > 0 {
            assert_eq!(l.scene.spawns.len(), starts, "{name} spawns");
        }
    }
}

#[test]
fn every_multiplayer_map_decodes() {
    for name in MULTIPLAYER {
        let Some(p) = map(name) else { continue };
        let m = blam_h1::Map::open(&p).unwrap_or_else(|e| panic!("{name}: {e}"));
        let l = blam_h1::decode(&m, &blam_h1::Options::default()).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(l.scene.triangle_count() > 1000, "{name}");
        assert!(!l.scene.collision.triangles.is_empty(), "{name} collision");
        assert!(!l.scene.spawns.is_empty(), "{name} spawns");
        assert_eq!(l.lightmaps.len(), 1, "{name} lightmap atlas");
        assert!(l.stats.lightmapped > 0, "{name} lightmapped");
        assert!(l.bitmaps.len() > 10, "{name} bitmaps: {}", l.bitmaps.len());
    }
}
