//! The scenario's camera effects (`cfxs`, camera_fx_settings): the
//! level's exposure. Field meanings are our reading of the bytes over the
//! 27 multiplayer maps; `docs/format/sky-and-atmosphere.md` section 5.

use blam_cache::CacheFile;

use crate::{Error, Result};

/// Where the scenario keeps its `cfxs` reference (MCC U13).
pub const SCNR_CAMERA_FX: usize = 0x660;

/// The level's camera effects, as far as they are decoded.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraFx {
    pub tag: String,
    /// Flags word at `+0x00` (0x14 on 25 of 27 maps).
    pub flags: u32,
    /// `+0x18`: the exposure in stops, `2^stops` scales the scene's
    /// light. -4.84 on Valhalla; more negative on brighter-lit maps.
    pub exposure: f32,
    /// `+0x10`, `+0x14`: probably the auto-exposure range in stops around
    /// it (Valhalla 0 and 3). The viewer brightens dark places by up to its
    /// width.
    pub auto_range: [f32; 2],
}

/// Reads the scenario's camera effects; `None` when it has none.
pub fn read(cache: &CacheFile) -> Result<Option<CameraFx>> {
    // Reach's scenario keeps this elsewhere and its tag differs: none read.
    if cache.engine() == blam_cache::Engine::Reach {
        return Ok(None);
    }
    let scnr = cache
        .scenario()
        .ok_or_else(|| Error::Invalid("no scenario tag".into()))?;
    let reference = cache.reader_at(cache.tag_offset(scnr)? + SCNR_CAMERA_FX).tag_ref()?;
    if reference.class.as_str() != "cfxs" {
        return Ok(None);
    }
    let Some(tag) = reference.index.and_then(|i| cache.tags.get(i)) else {
        return Ok(None);
    };
    let base = cache.tag_offset(tag)?;
    let mut r = cache.reader_at(base);
    let flags = r.u32()?;
    let mut a = r.at(base + 0x10);
    let auto_range = [a.f32()?, a.f32()?];
    let exposure = r.at(base + 0x18).f32()?;
    if !exposure.is_finite() || exposure.abs() > 30.0 {
        return Err(Error::Invalid(format!("{}: exposure {exposure}", tag.name())));
    }
    Ok(Some(CameraFx {
        tag: tag.name().to_string(),
        flags,
        exposure,
        auto_range,
    }))
}
