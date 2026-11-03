# CELT 0.11.0 (decoder build), vendored

The Xiph.Org CELT codec, release 0.11.0 (February 2011), the version FMOD's FSB5
"CELT" codec (codec 12) is built on. MCC's Halo: Combat Evolved keeps its music,
dialogue, streamed and most effect sounds in FSB5 banks of this codec
(`halo1\sound\pc\*.fsb`, `docs/format/halo1.md` 19); `crates/fsb_celt` compiles these
files with the `cc` crate and wraps the decoder.

- Source: `https://downloads.xiph.org/releases/celt/celt-0.11.0.tar.gz`
  (sha256 `c94d4d34f5a2caa1574b1a94869202cacd959b55f643a8bafe0660008acad9c3`).
- Licence: BSD 3-clause (`COPYING`, authors in `AUTHORS`), compatible with this
  project's licences.
- Kept: the `libcelt` sources and headers the library is made of (the encoder
  parts stay because `celt.c` holds both sides). Left out: `testcelt.c`,
  `dump_modes.c`, `header.c`, `static_modes_fixed.c`, the tools, tests and
  autotools files.
- Built with `CUSTOM_MODES` (FMOD's mode is 44.1 kHz, 512-sample frames, not one
  of the static modes), `ENABLE_POSTFILTER` (decodes frames that use the pitch
  post-filter instead of rejecting them; frames that do not are decoded the same),
  floating point, `USE_ALLOCA`.

**Changes made here:** none. The files are as released.
