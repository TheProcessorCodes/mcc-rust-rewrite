//! The import from the user's own Reflex install reads back as the install
//! does; skipped when there is none. Its own test binary, as it points the
//! settings folder at a temporary one.

use reflex_assets::{bike, engine_audio, find_reflex, imported, vehicle};

#[test]
fn the_import_matches_the_install() {
    let Some(root) = find_reflex() else {
        eprintln!("no Reflex install; skipped");
        return;
    };
    let dir = std::env::temp_dir().join(format!("mcc-reflex-import-{}", std::process::id()));
    // SAFETY: the only test in this binary, and nothing else runs yet.
    unsafe { std::env::set_var("MCC_SETUP_DIR", &dir) };
    assert!(!imported::done());
    imported::import(&root).expect("import");
    assert!(imported::done());

    let (a, b) = (bike::load(&root).unwrap(), imported::bike().unwrap());
    assert_eq!(a.kit, b.kit);
    assert_eq!(a.bones.len(), b.bones.len());
    assert_eq!(a.parts.len(), b.parts.len());
    for (x, y) in a.parts.iter().zip(&b.parts) {
        assert_eq!((x.bone, &x.slot, &x.texture, x.cutout), (y.bone, &y.slot, &y.texture, y.cutout));
        assert_eq!(x.positions, y.positions);
        assert_eq!(x.indices, y.indices);
    }
    assert_eq!(a.textures.len(), b.textures.len());
    for (k, t) in &a.textures {
        assert_eq!(t.pixels, b.textures[k].pixels, "{k}");
    }

    let (a, b) = (engine_audio::load_450f(&root).unwrap(), imported::engine().unwrap());
    assert_eq!(a.clips.len(), b.clips.len());
    assert_eq!(a.defs.len(), b.defs.len());
    assert_eq!(a.clips[0].samples, b.clips[0].samples);

    assert_eq!(vehicle::xml(&root, imported::CLASS).unwrap(), imported::vehicle_xml().unwrap());
    let size: u64 = std::fs::read_dir(game_paths::reflex_import_dir())
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    eprintln!("imported {:.1} MB", size as f64 / 1e6);
    std::fs::remove_dir_all(&dir).unwrap();
}
