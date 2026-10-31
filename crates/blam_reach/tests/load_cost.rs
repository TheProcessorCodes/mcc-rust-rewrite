//! Times what riding a Reach map costs at load: the bike's column grid
//! and the rail finder. Diagnostic; run with
//! `cargo test --release -p blam_reach --test load_cost -- --ignored --nocapture`
//! (`REACH_MAP=forge_halo` for one map).

use std::path::PathBuf;
use std::time::Instant;

fn maps_dir() -> PathBuf {
    std::env::var_os("HALOREACH_MAPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\haloreach\maps")
    })
}

#[test]
#[ignore]
fn load_cost() {
    let name = std::env::var("REACH_MAP").unwrap_or_else(|_| "forge_halo".into());
    let path = maps_dir().join(format!("{name}.map"));
    if !path.exists() {
        return;
    }
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    let mut scene = blam_geometry::Scene::default();
    let t = Instant::now();
    blam_reach::decode_into(&cache, &mut scene).unwrap();
    println!("{name}: decoded in {:.1} s, {} collision triangles", t.elapsed().as_secs_f32(), scene.collision.triangles.len());

    // The bike's grid: 4 m columns over Halo X / Y.
    let t = Instant::now();
    let mut grid = blam_geometry::column_grid::ColumnGrid::new(4.0 / 3.048);
    for (i, tri) in scene.collision.triangles.iter().enumerate() {
        let lo = tri.points[0].min(tri.points[1]).min(tri.points[2]);
        let hi = tri.points[0].max(tri.points[1]).max(tri.points[2]);
        grid.insert(i as u32, [lo.x, lo.y], [hi.x, hi.y]);
    }
    println!("bike grid: {} entries in {:.2} s", grid.entries(), t.elapsed().as_secs_f32());

    let t = Instant::now();
    let (rails, _) = blam_geometry::rails::find_rails(&scene.collision);
    println!("rails: {} in {:.1} s", rails.len(), t.elapsed().as_secs_f32());
}
