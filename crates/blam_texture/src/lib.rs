//! Halo 3 textures: `bitm` tags decoded into mip chains of raw BCn or pixel
//! data, and shader tags resolved to the bitmaps they draw with.
//!
//! No renderer types here; the viewer turns a [`Texture`] into a GPU image.
//! Written from `docs/format/bitmap.md` and `docs/format/shaders.md`; no
//! code from other projects is copied.
//!
//! ```no_run
//! let cache = blam_cache::CacheFile::open("riverworld.map")?;
//! let resources = cache.resources()?;
//! let shader = blam_texture::shader::resolve(&cache, 11454)?;
//! if let Some(base) = &shader.base {
//!     let tex = blam_texture::bitmap::decode(&cache, &resources, base.bitmap, 0)?;
//!     println!("{} {}x{} {:?}, {} levels", tex.name, tex.width, tex.height, tex.format, tex.mips.len());
//! }
//! # Ok::<(), blam_texture::Error>(())
//! ```
//!
//! Every read is bounds-checked: bad or unknown data gives an [`Error`],
//! never a panic, and no buffer is allocated larger than the bytes that
//! were actually found.

pub mod bitmap;
pub mod format;
pub mod rgba;
pub mod shader;

pub use bitmap::{Kind, Texture};
pub use format::Format;
pub use shader::{Blend, Shader, TextureSlot, Xform};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Cache(#[from] blam_cache::Error),
    /// The bitmap has no pixels in this map (a wiped or null resource).
    #[error("no data: {0}")]
    NoData(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("invalid data: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;
