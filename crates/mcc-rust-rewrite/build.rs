//! Embeds a Windows version resource in the exe. Without one, NVIDIA's overlay
//! (and other capture tools) name the capture folder after the exe's directory
//! (`Videos\out` for `out\bin\mcc-rust-rewrite.exe`) instead of the game, and
//! Task Manager and the volume mixer show the bare exe name.

fn main() {
    #[cfg(windows)]
    {
        const NAME: &str = "MCC Rust Rewrite";
        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", NAME)
            .set("FileDescription", NAME)
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
