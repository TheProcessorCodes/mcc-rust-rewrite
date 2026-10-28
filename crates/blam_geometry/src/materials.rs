//! One scene material per shader tag, shared by every BSP and model that
//! uses it.

use std::collections::HashMap;

use blam_cache::{CacheFile, TagRef};

use crate::scene::{Material, Scene, Shading};

/// How a shader class is drawn, until shader options are read.
pub fn classify_shader(class: &str) -> (Shading, bool) {
    match class {
        "rmtr" => (Shading::Terrain, false),
        "rmfl" => (Shading::AlphaTest, true),
        "rmd " => (Shading::Translucent, true),
        "rmw " | "rmct" => (Shading::Translucent, false),
        _ => (Shading::Opaque, false),
    }
}

#[derive(Default)]
pub struct MaterialTable {
    /// Scene material by shader tag index; `None` is the null shader.
    by_tag: HashMap<Option<u32>, u32>,
}

impl MaterialTable {
    /// The scene material for a shader reference, added on first sight.
    pub fn index(&mut self, cache: &CacheFile, scene: &mut Scene, shader: TagRef) -> u32 {
        let tag = shader.index.and_then(|i| cache.tags.get(i));
        let key = tag.map(|t| t.index);
        if let Some(&i) = self.by_tag.get(&key) {
            return i;
        }
        let class = tag
            .map(|t| cache.tags.class_of(t).code.as_str().to_string())
            .unwrap_or_else(|| shader.class.as_str().to_string());
        let (mut shading, two_sided) = classify_shader(&class);
        // Shaders the game never draws (invisible walls' render side).
        if tag.is_some_and(|t| {
            let name = t.name().to_ascii_lowercase();
            name.ends_with("\\invisible") || name.ends_with("z_invis_lightquad")
        }) {
            shading = Shading::Invisible;
        }
        scene.materials.push(Material {
            tag_path: tag.map_or_else(|| "(none)".to_string(), |t| t.name().to_string()),
            shader_class: class,
            shading,
            two_sided,
            shader_tag: key,
        });
        let i = (scene.materials.len() - 1) as u32;
        self.by_tag.insert(key, i);
        i
    }

    /// The materials of a 36-byte-per-entry shader block, in slot order.
    pub fn slots(
        &mut self,
        cache: &CacheFile,
        scene: &mut Scene,
        shaders: blam_cache::Block,
    ) -> crate::Result<Vec<u32>> {
        shaders
            .elements(36)
            .map(|o| {
                let shader = cache.reader_at(o).tag_ref()?;
                Ok(self.index(cache, scene, shader))
            })
            .collect()
    }

    /// Material for a submesh's shader slot, with a placeholder for slots
    /// outside the block.
    pub fn for_slot(
        &mut self,
        cache: &CacheFile,
        scene: &mut Scene,
        slots: &[u32],
        shader: i16,
    ) -> u32 {
        match usize::try_from(shader).ok().and_then(|s| slots.get(s)) {
            Some(&m) => m,
            None => self.index(cache, scene, TagRef {
                class: blam_cache::FourCc::from_code("????"),
                index: None,
            }),
        }
    }
}
