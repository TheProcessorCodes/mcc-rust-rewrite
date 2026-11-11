# Skate 3 engine (vendored)

The skating engine `crates/mcc_skate` runs: `skate-core`, `skate-data`, `skate-net` and
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

- `skate-host/Cargo.toml`: bevy 0.20 (was 0.19), to match the mcc-rust-rewrite workspace.
- `skate-net/Cargo.toml`: blake3 pinned to 1.8.5 (was 1.8.7).
- `skate-host/src/physics/bridge.rs`: `Session::session_marker()`, and
  `skate-host/src/session_marker/mod.rs`: `Runtime::position()`. Both report where the
  session marker is, for the session flag `mcc_skate` draws (2026-10-30).
- `skate-host/src/physics/bridge.rs`: `Session::session_marker()` also gives the marker's
  heading, and `Session::set_session_marker(position, heading)` sets it from outside;
  `skate-host/src/session_marker/mod.rs`: `Runtime::position()` became `Runtime::get()`,
  `Runtime::set()` is new, and a marker's stance is optional (a marker set from outside keeps
  the skater's stance). The dirt bike and the board share one session marker in mcc-rust-rewrite
  (2026-11-09).
- `skate-host/src/physics/bridge.rs`: `Session::launch(velocity)`, which gives the board, its
  hook and the skater's body a velocity right after `activate`, so mcc-rust-rewrite can put the rider on
  the board moving (off the bike or from a run) (2026-11-03).
- `skate-host/src/physics/bridge.rs`: `Session::push(change)`, which adds a velocity change to the
  board, its hook and the skater's body, so Halo's man cannons and gravity lifts can throw the
  board (2026-11-10).
- Comments in `skate-host/src/physics/bridge.rs` and `skate-host/src/session_marker/mod.rs`
  name the host project by its new name, mcc-rust-rewrite (was halo3) (2026-11-06).
- `skate-host/src/physics/bridge.rs`: `Session::score()` and `Pose::score`, a `Score` with
  the ScoreModule's publication (the trick last called out, its stance and points, the
  sequence in progress, the combo multiplier, the line and the lines banked), and
  `skate-host/src/scoring_runtime.rs`: `Runtime` counts the tricks it calls out (with the
  called trick's reward, also on a conversion) and the sequences it publishes (whether the
  last was a bail), and reads out its trick name, stance and sequence. mcc-rust-rewrite shows
  the trick names and the score on screen (2026-11-10).
- `license` fields in the four `Cargo.toml`s, and this folder's `Cargo.toml` (the mashup's
  `skate/Cargo.toml` with a new header).

## Licence

Everything in this folder is distributed under the GNU General Public License version 3 only
(`LICENSE`). Because `mcc-rust-rewrite.exe` links these crates, a released `mcc-rust-rewrite.exe` is distributed
under GPL-3.0-only too, with its source; see the top-level README. This does not grant any
rights to Electronic Arts' game code, data, assets or trademarks.
