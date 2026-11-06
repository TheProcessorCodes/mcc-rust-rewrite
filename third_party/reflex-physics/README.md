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

## Licence

No licence has been chosen for `reflex-physics` yet. It is the user's own code; pick one before a
public release (it links into `mcc-rust-rewrite.exe`, which is GPL-3.0-only because of the Skate 3 engine).
