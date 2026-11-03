//! Compiles the vendored CELT 0.11.0 library (`third_party/celt-0.11.0`,
//! BSD-3-Clause; its README says what was kept and how it is built).

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/celt-0.11.0/libcelt");
    let files = [
        "bands.c", "celt.c", "cwrs.c", "entcode.c", "entdec.c", "entenc.c", "kiss_fft.c", "laplace.c",
        "mathops.c", "mdct.c", "modes.c", "pitch.c", "plc.c", "quant_bands.c", "rate.c", "vq.c",
    ];
    let mut b = cc::Build::new();
    b.include(&dir)
        .define("CUSTOM_MODES", None)
        .define("ENABLE_POSTFILTER", None)
        .define("USE_ALLOCA", None)
        .warnings(false)
        .opt_level(2);
    if b.get_compiler().is_like_msvc() {
        // `alloca` comes from malloc.h there (stack_alloc.h only includes
        // it when WIN32 is defined, which would also make every function
        // dllexport).
        b.flag("/FImalloc.h");
    } else {
        b.define("HAVE_ALLOCA_H", None);
    }
    for f in files {
        b.file(dir.join(f));
        println!("cargo:rerun-if-changed={}", dir.join(f).display());
    }
    b.compile("celt0110");
}
