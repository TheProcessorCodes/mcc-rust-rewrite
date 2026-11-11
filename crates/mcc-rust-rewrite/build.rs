//! Embeds a Windows version resource in the exe, so Windows and capture tools
//! call the game "MCC Rust Rewrite".
//!
//! NVIDIA's overlay names a capture's folder and file after the game. For a
//! game it does not know it takes, in order: the NVIDIA driver profile of the
//! exe (few games have one), the exe's FileDescription, then the folder the exe
//! is in, passing over any whose name holds `game`, `bin`, `win32`, `win64`,
//! `x86`, `x64` or `data`.
//! Without a description it can read, that last step files captures under
//! `Videos\out` (from `out\bin\`), `Videos\release` (from `target\release\`) or
//! whatever folder a player unpacked the game into (#3). Task Manager and the
//! volume mixer show the FileDescription too.
//!
//! The resource is written here and not by `winresource`, because NVIDIA's
//! reader (`GetAppFileDesc` in `_nvspcaps64.dll`, NVIDIA app 11.0.9) is broken:
//! it takes the language from the first word of `\VarFileInfo\Translation` and
//! the code page from the word at offset 4 instead of 2. With the usual single
//! translation that word lies past the value, so it asks for a string table
//! that does not exist and the description of nearly every exe is lost on it.
//! Here a second translation follows the first, and its language word is the
//! first one's code page (0x04b0): the broken reader then asks for `040904b0`,
//! the same table a correct reader asks for. The second translation has its own
//! copy of the strings, so every translation listed has a table.
//! `scripts\release.ps1` refuses an exe NVIDIA's reader would not get the name
//! from.

fn main() {
    #[cfg(windows)]
    {
        // A missing resource compiler (no Windows SDK) costs only the names, so
        // warn and keep building.
        if let Err(e) = embed_version_resource() {
            println!("cargo:warning=no Windows version resource: {e}");
        }
    }
}

#[cfg(windows)]
fn embed_version_resource() -> std::io::Result<()> {
    const NAME: &str = "MCC Rust Rewrite";
    let strings: String = [
        ("FileDescription", NAME),
        ("ProductName", NAME),
        ("FileVersion", env!("CARGO_PKG_VERSION")),
        // What players see in the menu (`game_paths::VERSION`).
        (
            "ProductVersion",
            concat!("beta-", env!("CARGO_PKG_VERSION")),
        ),
        ("InternalName", "mcc-rust-rewrite"),
        ("OriginalFilename", "mcc-rust-rewrite.exe"),
        ("LegalCopyright", "GPL-3.0-only"),
    ]
    .iter()
    .map(|(key, value)| format!("VALUE \"{key}\", \"{value}\"\n"))
    .collect();
    let version = concat!(
        env!("CARGO_PKG_VERSION_MAJOR"),
        ", ",
        env!("CARGO_PKG_VERSION_MINOR"),
        ", ",
        env!("CARGO_PKG_VERSION_PATCH"),
        ", 0"
    );
    // US English, Unicode, then the translation NVIDIA's reader needs (above).
    let rc = format!(
        "#pragma code_page(65001)\n\
         1 VERSIONINFO\n\
         FILEVERSION {version}\n\
         PRODUCTVERSION {version}\n\
         FILEOS 0x40004\n\
         FILETYPE 0x1\n\
         {{\n\
         BLOCK \"StringFileInfo\"\n\
         {{\n\
         BLOCK \"040904b0\"\n{{\n{strings}}}\n\
         BLOCK \"04b004b0\"\n{{\n{strings}}}\n\
         }}\n\
         BLOCK \"VarFileInfo\"\n\
         {{\n\
         VALUE \"Translation\", 0x0409, 0x04b0, 0x04b0, 0x04b0\n\
         }}\n\
         }}\n"
    );
    let path = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("version.rc");
    std::fs::write(&path, rc)?;
    winresource::WindowsResource::new()
        .set_resource_file(path.to_str().unwrap())
        .compile()
}
