//! Reading a map's effects before it starts (on the loading thread): the
//! effect tags, everything they reach, and their bitmaps decoded to RGBA.

use std::collections::HashMap;
use std::sync::Arc;

use blam_cache::CacheFile;
use blam_effect::Library;
use blam_texture::Format;

/// A bitmap decoded for the particles: every level, RGBA8, largest first.
#[derive(Clone, Debug)]
pub struct Rgba {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub levels: Vec<Vec<u8>>,
    /// One channel stored (A8, Y8, DXT5a): its value fills every channel,
    /// alpha too, so it works as a colour map and as an alpha map.
    pub mono: bool,
}

/// What the effects of a map need.
#[derive(Clone, Debug)]
pub struct EffectAssets {
    pub library: Arc<Library>,
    pub textures: HashMap<u32, Rgba>,
}

/// Reads effects `roots` (tag indices) and decodes their bitmaps.
pub fn load(cache: &CacheFile, roots: &[u32]) -> EffectAssets {
    let started = std::time::Instant::now();
    let library = Library::load(cache, roots);
    for e in library.errors.iter().take(10) {
        eprintln!("effects: {e}");
    }
    let mut textures = HashMap::new();
    if let Ok(resources) = cache.resources() {
        for &b in &library.bitmaps {
            match blam_texture::bitmap::decode(cache, &resources, b, 0).map_err(|e| e.to_string()).and_then(|t| rgba(&t)) {
                Ok(t) => {
                    textures.insert(b, t);
                }
                Err(e) => eprintln!("effects: bitmap {b}: {e}"),
            }
        }
        resources.clear_page_cache();
    }
    eprintln!(
        "effects: {} effects, {} particles, {} contrails, {} lights, {} bitmaps read in {:.1} s",
        library.effects.len(),
        library.particles.len(),
        library.contrails.len(),
        library.lights.len(),
        textures.len(),
        started.elapsed().as_secs_f32()
    );
    EffectAssets {
        library: Arc::new(library),
        textures,
    }
}

/// Every level of layer 0 as RGBA8.
fn rgba(t: &blam_texture::Texture) -> Result<Rgba, String> {
    let mono = matches!(t.format, Format::A8 | Format::Y8 | Format::Bc4);
    let mut levels = Vec::new();
    for l in 0..t.mips.len() as u32 {
        let (w, h) = t.level_dims(l);
        let Some(data) = t.level(l, 0) else { break };
        let mut px = blam_texture::rgba::decode_level(t.format, w, h, data).map_err(|e| e.to_string())?;
        if mono {
            for p in px.chunks_exact_mut(4) {
                let v = if t.format == Format::A8 { p[3] } else { p[0] };
                p.copy_from_slice(&[v, v, v, v]);
            }
        }
        levels.push(px);
        // The GPU wants whole levels down to 1x1 or a prefix; stop at the
        // first level smaller than a block if the chain is odd.
        if w == 1 && h == 1 {
            break;
        }
    }
    if levels.is_empty() {
        return Err("no levels".into());
    }
    Ok(Rgba {
        name: t.name.clone(),
        width: t.width,
        height: t.height,
        levels,
        mono,
    })
}
