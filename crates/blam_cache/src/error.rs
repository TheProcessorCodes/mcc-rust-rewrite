use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("not a Halo cache file: magic {0:#010x} is not 'head'")]
    BadMagic(u32),
    #[error("cache version {0} is not supported (want 13: MCC Halo 3 update 6 or later)")]
    UnsupportedVersion(u32),
    #[error("cache build {0:?} is not supported")]
    UnsupportedBuild(String),
    #[error("{what} at file offset {offset:#x} ({len} bytes) runs past the end of the file")]
    OutOfBounds {
        what: &'static str,
        offset: u64,
        len: u64,
    },
    #[error("tag-data address {0:#x} is outside the tag section")]
    BadTagAddress(u64),
    #[error("invalid data: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;
