//! Halo CE's effects for a mission: every effect the map's scripts name
//! (the Warthog run's explosions), read and turned into Halo 3's effect
//! library by `blam_h1::effect_library`, their bitmaps decoded into
//! atlases, so `mcc_effects` plays them as it plays Halo 3's
//! (`docs/format/halo1.md` 13).
//!
//! Sounds: an effect's `snd!` part reaches `mcc_core::script_sound` as a
//! [`mcc_core::script_sound::SoundRequest::Impulse`] with the Halo CE
//! tag index and path (`sound\sfx\impulse\impacts\large_explosion`) and
//! where the effect went off; Halo CE sound playback picks them up there.

use std::collections::HashMap;
use std::sync::Arc;

use blam_h1::effect_library;
use mcc_effects::EffectAssets;
use mcc_effects::load::Rgba;

/// Reads the effects `scripts` name, with their bitmaps.
pub fn load(map: &blam_h1::Map, scripts: &blam_script::ScriptData) -> EffectAssets {
    let started = std::time::Instant::now();
    let roots = effect_library::script_effects(scripts);
    let converted = effect_library::convert(map, &roots);
    for e in converted.library.errors.iter().take(10) {
        eprintln!("effects: {e}");
    }
    let mut textures = HashMap::new();
    for (&b, layout) in &converted.layouts {
        match effect_library::atlas_pixels(map, b, layout) {
            Ok(px) => {
                textures.insert(
                    b,
                    Rgba {
                        name: map.tags.get(b as usize).map_or_else(String::new, |t| t.name.clone()),
                        width: px.width,
                        height: px.height,
                        levels: px.levels,
                        mono: px.mono,
                    },
                );
            }
            Err(e) => eprintln!("effects: bitmap #{b}: {e}"),
        }
    }
    let lib = &converted.library;
    eprintln!(
        "halo ce effects: {} named by the scripts, {} effects, {} particles, {} lights, {} bitmaps read in {:.1} s",
        roots.len(),
        lib.effects.len(),
        lib.particles.len(),
        lib.lights.len(),
        textures.len(),
        started.elapsed().as_secs_f32()
    );
    EffectAssets {
        library: Arc::new(converted.library),
        textures,
    }
}
