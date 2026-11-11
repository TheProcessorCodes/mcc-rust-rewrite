//! Embeds a Windows version resource in the exe, so Windows and capture tools
//! call the game "MCC Rust Rewrite".
//!
//! NVIDIA's overlay names a capture's folder and file after the game. For a
//! game it does not know it takes, in order: the NVIDIA driver profile of the
//! exe (few games have one), the exe's FileDescription, then the folder the exe
//! is in, passing over `game`, `bin`, `win32`, `win64`, `x86`, `x64` and `data`.
//! Without the resource that last step filed captures under `Videos\out` (from
//! `out\bin\`), `Videos\release` (from `target\release\`) or whatever folder a
//! player unpacked the game into (#3). Task Manager and the volume mixer show
//! the FileDescription too. `scripts\release.ps1` refuses an exe without it.

fn main() {
    #[cfg(windows)]
    {
        const NAME: &str = "MCC Rust Rewrite";
        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", NAME)
            .set("FileDescription", NAME)
            // What players see in the menu (`game_paths::VERSION`).
            .set("ProductVersion", concat!("beta-", env!("CARGO_PKG_VERSION")))
            .set("InternalName", "mcc-rust-rewrite")
            .set("OriginalFilename", "mcc-rust-rewrite.exe")
            .set("LegalCopyright", "GPL-3.0-only");
        // A missing resource compiler (no Windows SDK) costs only the names
        // above, so warn and keep building.
        if let Err(e) = res.compile() {
            println!("cargo:warning=no Windows version resource: {e}");
        }
    }
}
