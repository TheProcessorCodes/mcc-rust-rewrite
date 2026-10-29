//! Decodes the user's own Coagulation (MCC Halo 2) and pins its numbers:
//! the render counts against HaloExportH2's export of the same file
//! (`docs/format/halo2.md` 6), the collision, spawns, rails and textures
//! against our own probes. Skips quietly when the map is not installed.

use std::path::PathBuf;

use blam_geometry::{CollisionSource, Kind};

fn map(name: &str) -> Option<PathBuf> {
    game_paths::halo2_map(name)
}

#[test]
fn halo2_maps_are_told_from_halo3_maps() {
    if let Some(p) = map("coagulation") {
        assert!(blam_h2::is_halo2(&p));
    }
    if let Some(h3) = game_paths::halo3_map("riverworld") {
        assert!(!blam_h2::is_halo2(&h3));
    }
}

#[test]
fn coagulation_index_and_strings() {
    let Some(path) = map("coagulation") else { return };
    let m = blam_h2::Map::open(&path).expect("opens");
    assert!(m.compressed);
    assert_eq!(m.bytes.len(), 0xAA6E80);
    assert_eq!(m.map_name, "coagulation");
    assert_eq!(m.scenario_path, "scenarios\\multi\\halo\\coagulation\\coagulation");
    assert_eq!(m.tags.len(), 23131);
    assert_eq!(m.classes.len(), 120);
    assert_eq!(m.strings.len(), 13044);
    let live = m.tags.iter().filter(|t| t.id != u32::MAX).count();
    let local = m.tags.iter().filter(|t| t.offset.is_some()).count();
    assert_eq!((live, local), (5876, 268));
    // Shared tags resolve in shared.map under the same datum.
    let shared = m.shared().expect("shared.map opens");
    for t in m.tags.iter().filter(|t| t.id != u32::MAX && t.offset.is_none()) {
        let s = shared.tag(t.id).expect("in shared.map");
        assert_eq!((&s.class, &s.name), (&t.class, &t.name));
    }
}

#[test]
fn coagulation_matches_haloexport_h2() {
    let Some(path) = map("coagulation") else { return };
    let m = blam_h2::Map::open(&path).expect("opens");
    let loaded = blam_h2::decode(&m, &blam_h2::Options::default()).expect("decodes");
    let scene = &loaded.scene;
    let tris = |kind: Kind| -> (usize, usize) {
        let ps: Vec<_> = scene.placements.iter().filter(|p| p.kind == kind).collect();
        (ps.len(), ps.iter().map(|p| scene.meshes[p.mesh as usize].triangle_count()).sum())
    };
    // HaloExportH2's halo_export.json: world 12,797, instance 5,591 (63
    // nodes), scenery 2,324 (11 placed), crate 2 (2 placed).
    assert_eq!(tris(Kind::Cluster), (48, 12797));
    assert_eq!(tris(Kind::Instance), (63, 5591));
    assert_eq!(tris(Kind::Scenery).1, 2324);
    assert_eq!(tris(Kind::Crate), (2, 2));
    assert_eq!(loaded.stats.objects.get(&Kind::Scenery), Some(&11));
    assert_eq!(tris(Kind::Sky), (8, 349));
    assert!(loaded.stats.models_failed.is_empty(), "{:?}", loaded.stats.models_failed);
    let (lo, hi) = scene.bounds.expect("bounds");
    assert!((lo.x + 25.6).abs() < 0.01 && (hi.x - 179.2).abs() < 0.01);
    assert!((lo.y + 230.4).abs() < 0.01 && hi.y.abs() < 0.01);

    // Every mesh has a normal and a texcoord per vertex.
    for mesh in &scene.meshes {
        assert_eq!(mesh.normals.len(), mesh.positions.len(), "{}", mesh.name);
        assert_eq!(mesh.uvs.len(), mesh.positions.len(), "{}", mesh.name);
    }
}

#[test]
fn coagulation_collision_spawns_and_rails() {
    let Some(path) = map("coagulation") else { return };
    let m = blam_h2::Map::open(&path).expect("opens");
    let loaded = blam_h2::decode(&m, &blam_h2::Options { textures: false, ..Default::default() }).expect("decodes");
    let scene = &loaded.scene;
    let col = &scene.collision;
    let bsp = col.triangles.iter().filter(|t| matches!(t.source, CollisionSource::Bsp(_))).count();
    assert_eq!(bsp, 8586);
    assert_eq!(col.triangles.len() - bsp, 4502);
    assert_eq!(loaded.stats.corrupt_loops, 0);
    assert_eq!(col.materials.len(), 20);
    // The invisible walls round the canyon: 264 surfaces, two-sided too.
    let invisible = col.triangles.iter().filter(|t| t.flags & 2 != 0).count();
    assert_eq!(invisible, 288);
    // Loops face out: most of a canyon's collision faces up.
    let up = col.triangles.iter().filter(|t| t.source == CollisionSource::Bsp(0) && t.normal().z > 0.0).count();
    assert_eq!(up, 6035);

    // 123 respawn points, every one standing on a collision floor.
    assert_eq!(scene.spawns.len(), 123);
    for sp in &scene.spawns {
        let p = sp.position;
        let floor = col
            .triangles
            .iter()
            .filter(|t| t.normal().z > 0.3)
            .filter_map(|t| {
                let [a, b, c] = t.points;
                let d = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
                if d.abs() < 1e-9 {
                    return None;
                }
                let u = ((p.x - a.x) * (c.y - a.y) - (c.x - a.x) * (p.y - a.y)) / d;
                let v = ((b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y)) / d;
                (u >= -1e-4 && v >= -1e-4 && u + v <= 1.0001).then_some(a.z + u * (b.z - a.z) + v * (c.z - a.z))
            })
            .filter(|z| *z <= p.z + 0.3)
            .fold(f32::NEG_INFINITY, f32::max);
        // Starting location 62 hangs 0.51 wu over the creek bank (the game
        // drops the player); the rest stand within 0.3 wu of a floor.
        let limit = if sp.name.ends_with(" 62") { 0.6 } else { 0.3 };
        assert!((p.z - floor).abs() < limit, "{sp:?} is {:.2} wu over its floor", p.z - floor);
    }
    // No kill volumes on Coagulation: falling off is caught by the floor
    // under the collision (`falls.rs`).
    assert!(scene.trigger_volumes.is_empty());

    // Grind rails from the collision (`rails.rs`).
    let (rails, _) = blam_geometry::rails::find_rails(col);
    assert!((RAILS_LOW..=RAILS_HIGH).contains(&rails.len()), "{} rails", rails.len());
}

/// 610 rails found on 2026-10-28; the finder's count can move by a few.
const RAILS_LOW: usize = 600;
const RAILS_HIGH: usize = 620;

#[test]
fn coagulation_textures() {
    let Some(path) = map("coagulation") else { return };
    let m = blam_h2::Map::open(&path).expect("opens");
    let loaded = blam_h2::decode(&m, &blam_h2::Options::default()).expect("decodes");
    let look = |name: &str| {
        let i = loaded.scene.materials.iter().position(|mm| mm.tag_path.ends_with(name)).expect(name);
        loaded.looks[i].clone().expect("resolves")
    };
    // The ground: base, grass detail x60, pebble secondary x60, blended by
    // the base alpha.
    let g = look("\\coag_ground");
    assert!(g.detail_blend);
    let name = |b: Option<(u32, blam_texture::Xform)>| m.tags[b.unwrap().0 as usize].short_name().to_string();
    assert_eq!(name(g.base), "coag_ground");
    assert_eq!(name(g.detail), "coag_grass");
    assert_eq!(name(g.secondary_detail), "dirt_pebble");
    assert_eq!(g.detail.unwrap().1.scale, [60.0, 60.0]);
    // An environment-mapped metal: the detail takes the third constant.
    let p = look("\\coag_panel_i");
    assert_eq!(name(p.detail), "scratches");
    assert_eq!(p.detail.unwrap().1.scale, [16.0, 16.0]);
    assert!(look("\\redcedarbough").alpha_test);
    assert_eq!(look("\\coag_light_bands_red").blend, blam_texture::Blend::Additive);
    assert!(look("shaders\\invalid").hidden);
    // Every base map decodes (the glass's cube map is not taken as one).
    for (mm, l) in loaded.scene.materials.iter().zip(&loaded.looks) {
        if let Some((b, _)) = l.as_ref().and_then(|l| l.base) {
            assert!(loaded.bitmaps.contains_key(&b), "{}: base not decoded", mm.tag_path);
        }
    }
    assert_eq!(loaded.stats.bitmaps_failed.len(), 0, "{:?}", loaded.stats.bitmaps_failed);
    let t = &loaded.bitmaps[&g.base.unwrap().0];
    assert_eq!(t.mips[0].len() as u64, t.format.level_size(t.width, t.height, 0));
}
