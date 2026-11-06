# reflex_bevy

MX vs ATV Reflex's dirt bike in Bevy **0.20**: the `reflex_core` simulation (validated frame by frame against
recordings of the original game) behind a small set of components. No physics engine is used or needed; the bike
integrates its own four bodies and only asks the game about the ground.

This crate defines the contract. A game conforms to it as follows.

## Add it

```toml
[dependencies]
reflex_bevy = { path = "D:/reflex-physics/crates/reflex_bevy", features = ["mesh", "gizmos"] }
```

| feature | default | what |
|---|---|---|
| `controls` | yes | `ReflexDefaultControls`: keyboard + gamepad mapping with Reflex's deadzones (Bevy `keyboard`, `gamepad`) |
| `mesh` | no | `ReflexMeshGroundPlugin` + `ReflexCollider`: ground from tagged `Mesh3d` entities |
| `gizmos` | no | `ReflexDebugDrawPlugin`: stick-model bike and contact normals |
| `example` | no | everything the `ride` example needs (window, renderer) |

The game's own Bevy must be 0.20 (one Bevy per build).

## The contract

1. **Plugin**: `app.add_plugins(ReflexPlugin)`. Systems run in `Update` in the sets
   `ReflexSet::Input -> Step -> Sync`.
2. **Ground**: one `ReflexGround` resource for all bikes, in metres, Bevy's frame. Pick one:
   - `ReflexGround::from_triangles(TriangleGround)`: hand over the level's collision once
     (`add_triangle`, `add_indexed`, `add_box`, `add_convex`, `add_cylinder`).
   - `ReflexGround::from_query(impl GroundQuery)`: answer two queries from the game's own collision:
     `triangles(center, half_extent, out)` (every triangle touching that box; the bike asks about ~0.37 m around
     each wheel several times a frame) and `raycast(from, to) -> Option<(point, normal)>` (first hit of a segment).
   - `ReflexMeshGroundPlugin` (feature `mesh`): add `ReflexCollider` to `Mesh3d` entities; the ground is rebuilt
     when they change. Do not also set `ReflexGround` yourself.

   **Winding**: a triangle is solid on the side it is counter-clockwise from (Bevy's front face). Wheels only
   touch that side; `ReflexCollider { double_sided: true }` adds both.
3. **Bike**: spawn `ReflexBike::from_file(path)` with a `Transform` whose origin is **on the riding surface**,
   facing its **-Z**. It is placed at rest there on the first `Update` (0.6 m chassis height, `spawn_height`).
   The vehicle file is the player's own Reflex data (the stock JSON or XML vehicle dump, or an in-memory tuning
   dump); nothing from Reflex ships with this crate. Options: `with_garage(suspension, gearing, brakes)`
   (1 = stock), `with_auto_respawn(false)`, `respawn_point`, `respawn_at(transform)`.
4. **Input**: write the bike's `ReflexInput` every frame before `ReflexSet::Step`: `throttle` 0..1, `brake`
   0..1 (both levers), `steer` -1..1, `lean` (x: left/right, y: back/forward) -1..1, `reverse`, `clutch`,
   `respawn` (one frame). Values past the deadzone, as Reflex reads them (sticks: XInput 0.24 rescaled,
   triggers: under 30/255 is 0; `input::stick_deadzone` / `trigger_deadzone`). Or add `ReflexDefaultControls`.
5. **Drawing**: the bike entity's `Transform` is the chassis pose. For the rest, spawn entities with
   `ReflexPart { bike, part }`; as children of the bike entity they get chassis-local transforms, otherwise
   world transforms. Parts: `Chassis`, `Rider`, `FrontWheel` / `RearWheel` (with spin about local X; the front
   one steers), `Steering` (bars and crowns, origin at the steering head), `FrontHub` (front axle, steered, no
   spin: fork lowers), `SwingArm` (origin at the pivot, local +Z toward the rear axle), `RearHub`. Model a part
   with its pivot at the origin, the bike facing -Z, +Y up. Poses are interpolated between substeps.
6. **State**: `ReflexBikeState` (after `ReflexSet::Step`): `forward_speed`, `speed`, `velocity`,
   `angular_velocity`, `rpm`, `gear`, `airborne`, `crashed`, `at_rest`, `steer_angle`, `lean_command`,
   `teleported` (snap cameras), and per wheel `contact`, `point`, `normal`, `load` (N), `side_grip`, `roll_grip`,
   slips, `spin`, suspension length and rest length.

Units: metres, seconds, radians, newtons. Timing: the frame's `Time` delta (pause by pausing virtual time),
fixed 7.5 ms substeps inside, frames over 0.1 s clamped. Reflex units and the raw simulation are available through
`ReflexBike::sim()` for tools.

## Try it

```
cargo run --release -p reflex_bevy --example ride --features example -- <your vehicle file>
```

Flat yard with kickers, a table-top and rollers; W / RT throttle, S / LT brake, A D / left stick steer,
arrows / right stick lean, X / A reverse, E / RB clutch, Backspace / Back respawn, F1 gizmos on / off.

## Tests

`cargo test --release -p reflex_bevy --features mesh,gizmos` runs headless apps: the bike settles at the
right height, drives along a rotated spawn heading and turns the right way, parts follow it (child and world),
the `GroundQuery` path rides exactly like triangles, respawn works, and it rides on a tagged Bevy mesh. They
skip without a vehicle file (`REFLEX_VEHICLE`).
