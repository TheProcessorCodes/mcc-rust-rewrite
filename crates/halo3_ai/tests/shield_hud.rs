//! The shield meter's bitmaps and the shield sounds, read from 120_halo:
//! the Spartan HUD's meter and border (312 x 28 chud masks whose blue
//! channel ramps left to right), and the HUD globals' four shield sounds.
//! Skips quietly when the map is not installed.

use halo3_ai::hud::{self, fill_columns};
use halo3_core::ai::ShieldSounds;

#[test]
fn shield_meter_and_sounds() {
    let Some(path) = game_paths::halo3_map("120_halo") else { return };
    let cache = blam_cache::CacheFile::open(&path).expect("120_halo opens");
    let h = hud::read(&cache).expect("the shield meter's bitmaps");
    assert_eq!((h.meter.width, h.meter.height), (312, 28));
    assert_eq!((h.border.width, h.border.height), (312, 28));
    // Full fills it, empty shows nothing, half stops near the middle.
    assert!(fill_columns(&h.meter, 1.0) >= 300);
    assert_eq!(fill_columns(&h.meter, 0.0), 0);
    let half = fill_columns(&h.meter, 0.5);
    assert!((140..=172).contains(&half), "half at column {half}");
    assert!(fill_columns(&h.meter, 0.25) < half);

    // The four sounds, as the HUD globals list them, right after the
    // `chgd` tag: shield_hit, then the charge, low and depleted loops.
    let globals = cache.tags.of_class("chgd").next().expect("chud globals").index;
    let found: Vec<(String, u32)> = ShieldSounds::ALL
        .iter()
        .map(|(class, name)| {
            let t = cache.tags.find(class, name).unwrap_or_else(|| panic!("{name}"));
            (cache.tags.class_of(t).code.as_str().to_string(), t.index)
        })
        .collect();
    assert_eq!(found.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["snd!", "lsnd", "lsnd", "lsnd"]);
    assert_eq!(found[0].1, globals + 1, "shield_hit follows the chud globals");
    assert_eq!(h.sounds, [Some(found[0].1), Some(found[1].1), Some(found[2].1), Some(found[3].1)]);
}
