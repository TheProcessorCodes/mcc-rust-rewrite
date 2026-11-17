# Reflex bike physics (vendored)

MX vs ATV Reflex's dirt-bike simulation as a library: `reflex_core` (engine-independent, Reflex units)
and `reflex_bevy` (the Bevy 0.20 adapter). `crates/mcc_bike` links `reflex_bevy` for the "Reflex"
bike model (`crates/mcc_bike/src/exact.rs`). They are copied here so the project builds from one
download. They contain no Reflex code or data: the vehicle values are read at run time from the
player's own Reflex install (`reflex_assets::vehicle`).

## Where it comes from

The user's own `reflex-physics` workspace (`D:\reflex-physics`, its own git repository), commit
c9636ee (2026-11-05, "reflex_bevy: general Bevy 0.20 adapter"): `crates/reflex_core` and
`crates/reflex_bevy` (sources, tests, the `ride` example, the adapter's README). Its `HANDOFF.md`
and `README.md` there describe how the model was rebuilt and checked frame by frame against
recordings of the game. Make changes there first where possible, then copy them here.

## What this project changed

- `Cargo.toml` (this folder): a two-crate workspace (the original also has `reflex_validate` and
  `reflex_godot`).
- `reflex_core/tests/stock_data.rs` left out: it reads the developer's Reflex dumps from a fixed
  `D:\` path.
- `reflex_core/src/contact.rs` `disc_contact`: no contact when the triangle normals under a wheel
  add up to nothing (two faces back to back), instead of normalising a zero vector into NaN, which
  made the whole bike NaN and panicked in the tyre code (2026-11-06).
- `reflex_core/src/wreck.rs` and `sim.rs`: taken from upstream commit 15a977d (2026-11-06, "wreck:
  drop the stick-flick recovery window; soft-limit excursions never wreck"): only chassis contacts
  and rate / load readings past the hard limits wreck.
- `reflex_bevy/src/lib.rs`: `ReflexBike::step(dt, input, ground, state)`, public, so a game can
  step a bike it keeps outside the ECS on its own `reflex_core::Ground`; the plugin's system calls
  it. `ReflexBike` derives `Clone` (2026-11-06).
- `reflex_bevy/src/ground.rs`: `ReflexGround::as_ground()` (was the crate-private `get()`) and
  `reflex_triangle` public (2026-11-06).

- `Cargo.toml`, `crates/*/Cargo.toml`: `license = "GPL-3.0-only"` (2026-11-10).
- `reflex_core` checked against OPEN REFLEX (2026-11-17, #46): every module compared with the
  matching functions of ItZFerret/OpenReflex (GPL-3.0, commit b8d170f; `core/game/bike_*.c`,
  `phys_*.c`, `world_wreck_checker.c`), a public hand transliteration of the game verified bit for
  bit against it, and corrected where ours differed. Each fix cites its C function and address in
  a comment. In short: the tyre's rear wheelspin bound, grip flags and airborne branch; the rider-off
  controls, backwards wobble, rough-ground head shake, lean hold, the up vector and the front brake
  easing with steering; the stunt latches and stick gestures (the trick hold now runs); per-triangle
  chassis probes, the rest pin and rolling orientation; rest entry / exit, the new bike's brake hold,
  the speed filter time constant and the garage steering factor; the wreck cooldown surviving a
  respawn and the four-button bail. Kept as before: no stick-flick recovery window (upstream 15a977d,
  described in `wreck.rs`), the zero-normal guard in `disc_contact`, f64 throughout. These changes
  are only here so far, not in the `reflex-physics` workspace.

## Licence

`reflex-physics` is GPL-3.0-only (its author's choice, 2026-11-10), the same as the rest of this
project; the text is in `LICENSE` at the top of the repository. It links into
`mcc-rust-rewrite.exe`, which is GPL-3.0-only as a whole.
