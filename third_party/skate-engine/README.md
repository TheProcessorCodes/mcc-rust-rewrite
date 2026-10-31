# Skate 3 engine (vendored)

The skating engine `crates/halo3_skate` runs: `skate-core`, `skate-data`, `skate-net` and
`skate-host`. They are copied here so the project builds from one download. They contain no
Skate 3 code or data; they read the player's own Skate 3 copy, converted once by first-run
setup.

## Where it comes from

- **Skate 3 Rust Engine** (github.com/SK8-ENGINE/skate-3-rust-engine), Copyright (c) 2026
  dumbad and the Skate 3 Rust Engine contributors, GPL-3.0-only (`LICENSE` here, added
  upstream in commit 8280fc4, 2026-10-04). `skate-core`, `skate-data` and `skate-net` are its
  crates as of commit cb79689 (2026-09-10). `skate-host` is cut down from its `skate-game`
  crate.
- **2010 Rust Rewrite Mashup** (github.com/chasmlol/2010-rust-rewrite-mashup, Apache-2.0),
  which took those crates into its `skate/` folder, wrote `skate-host` as a library around
  them, and changed `skate-core`'s board world. These files are the mashup's `skate/crates` at
  commit f608f85 (2026-10-02). Apache-2.0 code may be distributed as part of a GPL-3.0 work.

## What this project changed

- `skate-host/Cargo.toml`: bevy 0.20 (was 0.19), to match the halo3 workspace.
- `skate-net/Cargo.toml`: blake3 pinned to 1.8.5 (was 1.8.7).
- `skate-host/src/physics/bridge.rs`: `Session::session_marker()`, and
  `skate-host/src/session_marker/mod.rs`: `Runtime::position()`. Both report where the
  session marker is, for the session flag `halo3_skate` draws (2026-10-30).
- `license` fields in the four `Cargo.toml`s, and this folder's `Cargo.toml` (the mashup's
  `skate/Cargo.toml` with a new header).

## Licence

Everything in this folder is distributed under the GNU General Public License version 3 only
(`LICENSE`). Because `halo3.exe` links these crates, a released `halo3.exe` is distributed
under GPL-3.0-only too, with its source; see the top-level README. This does not grant any
rights to Electronic Arts' game code, data, assets or trademarks.
