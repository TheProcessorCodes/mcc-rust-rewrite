# What a Halo 3 runtime must expose to plug in the Skate 3 crates

Written 2026-10-27 from the `mw2-skate-3-src` sources (read-only). Every fact below names the
file it came from so a reviewer can re-check it. Paths are relative to
`D:\GAME ISO AND ROMS\PC - AI MASHUP\mw2-skate-3-src\src\mashup\` unless written in full.

The short version: the skate engine (`skate/crates/skate-host`) is a black box that takes
**a triangle soup, a list of rail polylines, a spawn point and a controller state**, and
returns **a posed skeleton, a camera and a velocity** sixty times a second. Everything else
(rendering the rider, drawing the board, deciding when to skate) is glue that IW4L wrote in
about 1,300 lines. A Halo runtime needs the same three inputs and the same glue.

---

## 1. The crates and where they come from

| piece | path | notes |
|---|---|---|
| skate engine | `skate/crates/{skate-core, skate-data, skate-net, skate-host}` | SK8-ENGINE/skate-3-rust-engine, commit `cb7968930f14dad38457e98720d1a274e469eec2` (`skate-host/src/bridge.rs` line 1, `source_revision()`) |
| skate workspace | `skate/Cargo.toml` | members `skate-core`, `skate-data`, `skate-net`; `skate-host` is **excluded** and has its own one-crate `[workspace]` |
| skate-host deps | `skate/crates/skate-host/Cargo.toml` | `bevy 0.20` with `default-features=false, features=["std","multi_threaded","bevy_log"]` (it only wants `Resource`, `Mat4/Vec3/Quat` and `info_span!`), `skate-core`, `skate-data`, `skate-net`, `serde`, `serde_json`, `half 2.7.1` |
| glue: mode + worker thread | `crates/render_anim/src/skate.rs` (543 lines) | imports `skate_host::bridge::{CollisionBuilder, InputFrame, Pose, PreparedCollision, Session}` |
| glue: collision + units | `crates/render_anim/src/skate/collision.rs` | `to_skate`, `from_skate`, `basis`, `extract` |
| glue: rails | `crates/render_anim/src/skate/rails.rs` | `find` |
| glue: retarget + board | `crates/render_anim/src/skate/rig.rs` | `pose`, `board`, bone mapping table |
| glue: board/rig export | `crates/assets/src/skate_board.rs` | writes `rig.json` and `board.json` from `skater.glb` |
| glue: toggle key | `crates/console/src/plugin/mod.rs` lines 415-425 | J, or both sticks clicked; `input_blocked` when console/menu/chat open |
| overview | `docs/SKATE.md` | |

The `mashup-halo3-valhalla` tree carries the **same** glue byte for byte (verified with
`diff --strip-trailing-cr`) except one line in `rig.rs::board`: `geom.cpu_posed = true;`
(fixes the frozen skateboard; see that tree's `CLAUDE.md`). Its `bridge.rs` is identical.

The public surface the glue uses is one file: `skate/crates/skate-host/src/physics/bridge.rs`
(321 lines), re-exported as `skate_host::bridge` by `skate-host/src/lib.rs` line 17
(`pub use physics::bridge;`). The 1-line `skate-host/src/bridge.rs` is unrelated (just the
revision string).

---

## 2. Exact Rust signatures (`skate-host/src/physics/bridge.rs`)

```rust
#[derive(Clone, Copy, Default)]
pub struct Controls {                 // raw XInput layout, used by Session::tick
    pub buttons: u16,
    pub triggers: [u8; 2],            // [left, right], 0..=255
    pub left: [i16; 2],               // left stick x, y, -32768..=32767
    pub right: [i16; 2],
}

pub struct Session { /* private: GamePhysics, SkaterRuntime, PlayerControls,
                        StockGraphs, ControllerInput, CameraRuntime, session_marker::Runtime */ }

pub struct Pose {
    pub root: Mat4,                   // animation root -> skate world (metres, Y up)
    pub bones: Vec<Mat4>,             // world matrix per bone, same order as `names`
    pub names: Vec<String>,           // bone names from the animation banks
    pub camera: Option<(Vec3, Mat3, f32)>, // position, basis (columns), vertical FOV in degrees
    pub velocity: Vec3,               // deck body linear velocity, m/s
    pub tick: u64,                    // physics ticks since Session::new
    pub state: String,                // Debug text of the player state machine
}

impl Session {
    pub fn new(root: &Path,
               triangles: Vec<[[f32; 3]; 3]>,
               rails: Vec<Vec<[f32; 3]>>,
               spawn: [f32; 3],
               heading: f32) -> Result<Self, String>;
    pub fn preload(root: &Path) -> Result<(), String>;      // decode animation banks early
    pub fn collision_builder(&self) -> CollisionBuilder;
    pub fn install_collision(&mut self, prepared: PreparedCollision) -> Result<(), String>;
    pub fn period(&self) -> f32;                             // seconds per physics tick
    pub fn set_aspect_ratio(&mut self, aspect_ratio: f32);   // for the camera runtime
    pub fn activate(&mut self, spawn: [f32; 3], heading: f32) -> Result<Pose, String>;
    pub fn collect(&mut self, frame: InputFrame, dt: f32);   // queue controller samples
    pub fn suspend_input(&mut self);
    pub fn advance(&mut self) -> Result<(), String>;         // one fixed tick
    pub fn tick(&mut self, input: Controls) -> Result<(), String>; // collect+advance in one, raw packet
    pub fn pose(&self) -> Pose;
}

pub struct PreparedCollision { /* private: BoardWorld, Arc<grind_world::StaticProvider> */ }

#[derive(Clone, Copy)]
pub struct CollisionBuilder { /* private: RetailContactMaterial */ }
impl CollisionBuilder {
    pub fn build(&self, triangles: Vec<[[f32; 3]; 3]>, rails: Vec<Vec<[f32; 3]>>)
        -> Result<PreparedCollision, String>;                 // safe to call on another thread
}

pub struct InputFrame { /* private: [Result<DevicePacket, DeviceError>; 4] */ }
impl InputFrame {
    pub fn neutral() -> Self;                                // four disconnected pads
    pub fn from_pad(buttons: u16, triggers: [u8; 2], left: [i16; 2], right: [i16; 2],
                    packet: u32) -> Self;                    // one pad in slot 0, subtype 1
    pub fn controller(&self) -> Option<usize>;               // first connected slot
    pub fn buttons(&self) -> u16;
}

#[derive(Default)]
pub struct ControllerTransport { /* XInput capability cache x4 */ }
impl ControllerTransport { pub fn poll(&mut self) -> InputFrame; } // Windows XInputGetState, unused by IW4L
```

**There is no `Session::step`.** What the glue calls a "step" (`skate.rs` lines 214-267,
`Job::Step`) is this sequence, done on the worker thread each rendered frame:

1. `session.set_aspect_ratio(aspect)`
2. `session.collect(input, dt)` with `dt = frame time clamped to 0.1 s`
3. `accumulated = min(accumulated + dt, 0.15)`
4. `while accumulated >= session.period() { accumulated -= period; session.advance()?; }`
5. if at least one tick ran: `let p = session.pose()`; reject the pose if `p.root` or any
   bone is not finite (`"Skate published a non-finite pose"` tears the session down).

`period()` is normally **1/60 s**: `skate-host/src/physics/clock.rs` sets
`timer_period(60)` and `NORMAL_STEP = f32::from_bits(0x3c888889)` = 0.016666668. The in-game
camera can request slow motion, which changes the period for a while, hence the loop reads
`period()` every iteration.

### What `Session::new` does (bridge.rs lines 33-63)

In order, all from `root` = the converted Skate 3 asset folder:

1. `skate_data::input_config::StockGameplayConfig::load(root)` (the flick-it input config)
2. `skate_data::GameAssets::load(root)`
3. `StockGraphs::load(root, &assets)` (ActionGraph + MotionGraph state graphs)
4. builds a `SkateMap` from your triangles and rails (`collision_map`, lines 216-275):
   version 14, name `"IW4L collision"`, one material `"MW2"` (friction 0.8, restitution 0,
   roughness 1), every triangle gets `surface: 0, material: 1, native_edges: None`, each rail
   becomes `Rail { name: "iw4_edge_{i}", closed: false, points, native: None }`
5. `GamePhysics::load_with_map(root, Some(&map))` at difficulty `Easy`
6. `SkaterRuntime::load(root, &graphs, &physics, "easy")`
7. `PlayerControls::load(root)`, `CameraRuntime::load(root)`, `session_marker::Runtime::load(root)`

It prints `IW4L_SKATE_LOAD begin|graphs|physics|skater <ms>` on stderr. On the user's PC the
whole thing takes a few seconds; the glue therefore builds the session **once per map, in the
background, while the map is still loading**, and keeps it alive across skate on/off.

`activate(spawn, heading)` (lines 92-110) resets the controller input, ticks once if the
session has never ticked, teleports the skater (`skater.travel_to(transform)` with
`transform = rotation_y(heading) * translation(spawn)`, with element `[3][3]` set to 0 as the
engine expects), runs 4 neutral ticks to settle, and returns the first `Pose`.

`CollisionBuilder::build` makes a `PreparedCollision` from a **new** triangle set without
touching the live session (used for Minecraft's streamed blocks); `install_collision` swaps
it in and rebinds the grind world. A static Halo map does not need it; a campaign level with
BSP switching would.

---

## 3. `InputFrame`: fields and meaning

`InputFrame` holds four `Result<DevicePacket, DeviceError>` slots (XInput's four pads).
`DevicePacket { number: u32, state: XboxState, subtype: u8 }`
(`skate-host/src/input/platform.rs` lines 5-9); `XboxState { buttons: u16, triggers: [u8; 2],
left: [i16; 2], right: [i16; 2] }` (`skate-core/src/input/xbox.rs` lines 5-10). The engine
converts that **raw XInput state** itself (`xbox.rs::convert`, 24 slots: bits 0-9 buttons,
10-11 triggers scaled by 1/255, 12-15 buttons A/B/X/Y, 16-23 the sticks after the game's own
radial deadzone: magnitude below 0.001 is zero, then `(len-0.25)*1.4286` clamped 0..1). So:
**do not apply bevy/gilrs deadzones or curves before handing it over.**

`from_pad(buttons, triggers, left, right, packet)` is what IW4L uses (`skate.rs` lines
505-543, `pad_frame`):

| field | meaning | how IW4L fills it from bevy 0.20 `Gamepad` |
|---|---|---|
| `buttons` | XInput bitmask | `0x0001` DPadUp, `0x0002` DPadDown, `0x0004` DPadLeft, `0x0008` DPadRight, `0x0010` Start, `0x0020` Select(Back), `0x0040` LeftThumb, `0x0080` RightThumb, `0x0100` LeftTrigger (= LB), `0x0200` RightTrigger (= RB), `0x1000` South (A), `0x2000` East (B), `0x4000` West (X), `0x8000` North (Y) |
| `triggers` | `[LT, RT]` 0..=255 | `LeftTrigger2`/`RightTrigger2` analog value `*255`, or 255 if only digital |
| `left`, `right` | stick `[x, y]` -32767..=32767 | `left_stick()`/`right_stick()` `*32767`, rounded |
| `packet` | XInput packet number | a counter the glue increments every frame (`host.pad_packet`) |

A frame with no pad is `InputFrame::neutral()`. `controller()` tells you which slot is live
(shown in the HUD), `buttons()` the first live pad's bits.

The eighteen gameplay actions the state graphs read are derived from those 24 slots in
`skate-core/src/input/gameplay_map.rs` (`GameplayActions::from_pad`); you never build them.

---

## 4. `Pose`: bone names, matrix convention, units

- **Units**: skate world, **metres, +Y up**, right-handed. Everything in `Pose` is in that
  frame. Gravity is -Y; the spawn height is set as `ground + wheel_radius -
  authored[0].translation.y` on the **y** component (`skate-host/src/physics.rs` lines
  273-291).
- **Matrix convention**: the engine's `NativeMatrix` is `[[f32; 4]; 4]`
  (`skate-core/src/animation/output/mod.rs` line 17) in RenderWare's row-vector layout: rows
  0..2 are the basis axes, row 3 the translation. `skate-host/src/animation.rs::native_matrix`
  turns that into a glam `Mat4` by taking row *i* as column *i* (`Mat4::from_cols(r0.xyz|0,
  r1.xyz|0, r2.xyz|0, r3.xyz|1)`). So in `Pose`, `m.x_axis/y_axis/z_axis` are the bone's
  axes and `m.w_axis.xyz` its position, standard glam column-major, apply as `m * v`.
- `root` = `skater.animated_skeleton.roots.animation_to_world`; `bones` = `skater.render_pose`
  per bone, **relative to the root, not world matrices**: a bone's world matrix is
  `root * bones[i]` (IW4L places the body entity at `root` and draws the bones inside it,
  `remote_body.rs` line 577; checked in the Halo runtime, where bones alone sit within a
  metre of the origin while the skater is hundreds of metres away); `names` =
  `evaluator.frames.bone_names` (bridge.rs lines 161-187). Look bones up **by name**; the glue
  never assumes an order.
- **35 bone names** (read from the user's `rig.json`, which the export derives from the same
  skeleton): `HIPS, SPINE, SPINE1, SPINE2, SPINE3, NECK, NECK1, HEAD, RIGHTSHOULDER, RIGHTARM,
  RIGHTFOREARM, RIGHTHAND, LEFTSHOULDER, LEFTARM, LEFTFOREARM, LEFTHAND, RIGHTUPLEG, RIGHTLEG,
  RIGHTFOOT, RIGHTTOEBASE, LEFTUPLEG, LEFTLEG, LEFTFOOT, LEFTTOEBASE, SKATEBOARD_ROOT,
  TRUCK_FRONT, RIGHT_WHEELFRONT, LEFT_WHEELFRONT, TRUCK_BACK, LEFT_WHEELBACK, RIGHT_WHEELBACK,
  RIGHTTOEBASE_REPARENTED, LEFTTOEBASE_REPARENTED, LEFTHAND_REPARENTED, RIGHTHAND_REPARENTED`.
  The seven `SKATEBOARD_ROOT .. RIGHT_WHEELBACK` bones pose the board (`rig.rs::board`).
- `camera: (position, basis, fov)`: `basis.z_axis` is the **look direction**, `basis.y_axis`
  the up vector (the glue does `Transform::looking_to(B*basis.z_axis, B*basis.y_axis)`,
  `skate.rs` lines 311-319); `fov` is the **vertical** field of view in degrees
  (`view_kick.rs` line 266 comment: "Skate publishes vertical FOV").
- `velocity`: `physics.board.bodies()[BodyId::Deck].rates.linear_velocity`, m/s.

---

## 5. Coordinate convention and the IW4 <-> skate transform

`crates/render_anim/src/skate/collision.rs` lines 4-12:

```text
to_skate(p)   = Vec3(p.x,  p.z, -p.y) * 0.0254        IW4 inches, Z up  ->  metres, Y up
from_skate(p) = Vec3(p.x, -p.z,  p.y) / 0.0254        metres, Y up      ->  IW4 inches, Z up
basis()       = Mat4::from_cols(X, Z, -Y, W)          skate-space vector -> IW4-space vector (no scale)
```

The axis swap is a proper rotation (determinant +1): IW4 `+Y` becomes skate `-Z`, IW4 `+Z`
becomes skate `+Y`. The glue applies it to triangles, rails and the spawn, and the inverse to
every pose (`present()`, `skate.rs` lines 302-324):

```text
root_iw4 = B * pose.root * B^-1,  then root_iw4.w_axis = from_skate(pose.root.w_axis.xyz)
```

(the bones are handed to `rig::pose` still in skate space; `rig.rs::convert` applies the same
`B * m * B^-1` + `from_skate` per bone).

**Heading**: `activate(spawn, heading)` rotates about skate +Y by `heading` radians. The glue
passes `yaw_degrees.to_radians() + FRAC_PI_2` where IW4 yaw 0 looks down +X and increases
counter-clockwise seen from above (`skate.rs` line 201). **Spawn**: player origin + 2 inches
up, then `to_skate` (`skate.rs` line 470); the engine then puts the wheels on the ground.

**Triangle winding**: the skate engine wants **counter-clockwise seen from the side you stand
on** (normal = `(b-a) x (c-a)` points up out of a floor). IW4 collision is stored clockwise, so
`extract` swaps vertices 1 and 2 (`collision.rs` line 51). Degenerate triangles are an error
in the engine (`"Invalid SKATE collision triangle normal"`, `skate_world.rs` line 240), so
`extract` drops anything with `|cross|^2 <= 0.001 in^4` and dedups by a 1/8-inch key (lines
24-30, 105-110). **Halo geometry is already counter-clockwise from the normal (standable) side, so a
Halo soup must not copy that swap**; see the gap-fill section at the end of this document.

What the engine does with the soup (`skate-host/src/skate_world.rs::collision_world`, lines
213-379): welds vertices at 1 mm **for adjacency only** (contact math keeps the original
points), pairs each edge with its reversed twin (most coplanar neighbour wins when several
faces share an edge, e.g. brush seams), stores per-edge cosines and the flags
`ONE_SIDED | USE_EDGE_COSINES | 0xe0` with edge bits cleared where the neighbour is convex
or coplanar, marks fully flat vertices (`0x200 << corner`), packs the surface id as
`audio | physics<<7 | pattern<<12`, groups every 64 triangles into one `QueryMesh` bounding
box for the BVH, and because the map is named `"IW4L collision"` calls
`world.enable_imported_floor_seams()`. There is no size limit on triangles; rails are counted
in a `u16` (see below).

---

## 6. Rails

**What a rail is**: `Vec<[f32; 3]>`, a polyline of two or more points, in **skate metres,
Y up**, open (not closed). The engine turns each into a grind spline
(`grind_world::StaticProvider::new`). `rails::MAX_RAILS = u16::MAX` because the engine counts
rails in a u16 (`rails.rs` line 33).

**Signature** (`rails.rs` line 43):

```rust
pub fn find(tris: &[[Vec3; 3]]) -> (Vec<Vec<Vec3>>, RailCensus)
pub struct RailCensus { pub candidates: usize, pub lips: usize, pub runs: usize, pub rails: usize }
```

Input and output are in **IW4 map units (inches), Z up**; the caller converts with
`to_skate` afterwards (`collision.rs` lines 120-123). Every constant is in inches:

| constant | value | meaning |
|---|---|---|
| `UPWARD_Z` | 0.65 | a face with normal.z above this is walkable (about 49 degrees from flat) |
| `CELL` | 64 in | XY grid cell for the probe acceleration structure |
| `PROBE_OUT` | 2 in | how far past the edge the down-probe starts |
| `MIN_DROP` | 3 in | the ground must be at least this far below the lip just past it |
| `WALL_PROBE_UP` | 3 in | height above the lip of the sideways probe that must hit nothing |
| `MIN_RAIL` | 24 in | shortest polyline kept |
| `MAX_SLOPE` | 0.7 | rise over run limit (stair handrails and ramp edges pass) |
| `MIN_TURN_COS` | 0.82 | polylines continue through joints turning less than about 35 degrees |
| keys | 1/8 in (vertex), 1/2 in (line origin), 1 in (joint node), 1.5 in (merge gap), edges under 1 in dropped, edges under 12 in get one probe sample instead of three | rounding used to match edges across seams |

**Algorithm in plain words** (`rails.rs` lines 43-308):

1. Put every triangle into an XY grid of 64-inch cells.
2. Collect every edge of every walkable face once (edges keyed by rounded endpoints).
3. For each edge that is not too steep: find the outward direction (perpendicular to the edge,
   away from the face centroid). At one or three points along the edge, probe: from 2 inches
   outward and 1 inch up, cast straight down 4 inches; if it hits anything the ground did not
   fall away. Then from 1 inch inside and 3 inches up, cast outward 4 inches; if it hits,
   a wall rises there. An edge is a **lip** if two out of three samples pass (Moller-Trumbore
   ray test against both faces, hits within `(0, len]`).
4. **Merge collinear lips**: lips on the same line (direction rounded to 1/64, origin to half
   an inch) have their parameter spans sorted and unioned when they touch or overlap within
   1.5 inches, giving maximal **runs**.
5. **Chain runs** into polylines: at a joint where exactly two runs meet, continue when the
   turn is gentler than `MIN_TURN_COS`; walk forward from one end and backward from the other.
6. Drop polylines shorter than 24 inches; if more than 65535 remain keep the longest.

On MW2 maps the census is logged as `skate rails: N walkable edges, N lips, N runs, N rails`.
The Minecraft world and the Halo stand-in maps reuse the same detector unchanged; it only
needs a triangle soup in inches, Z up.

---

## 7. `rig.json` and `board.json`

Both are **generated on the user's machine** by `crates/assets/src/skate_board.rs` from
`<assets>/private/skater.glb` (the skater model the converter extracted) and written next to
it as `<assets>/rig.json` and `<assets>/board.json` (`ensure()` writes them only if missing;
`launcher/src/first_run.rs` line 53 calls it on every start).

**`rig.json`** = JSON array, one object per joint of glTF `skins[0]` in joint order (35 on the
user's file):

| field | type | meaning |
|---|---|---|
| `name` | string | joint node name |
| `bind` | `[f64; 16]` column-major | `inverse(inverseBindMatrix) * inverse(BlenderBoneBasis)`, where the Blender basis is the matrix with columns `(1,0,0,0) (0,0,-1,0) (0,1,0,0) (0,0,0,1)`. Comment in the source: "The GLB carries Blender's bone basis; the soldier retarget wants the native bind frame" (`skate_board.rs` lines 40-58) |
| `inverse_bind` | `[f32; 16]` | the glTF inverse bind matrix as written |

`rig.rs::reference()` (lines 12-20) loads it once from `$IW4L_SKATE_ASSETS/rig.json`.

**`board.json`** (`skate_board.rs` lines 117-183):

```text
{ joints:   [ { target: <bone name>, origin: [0,0,0] } ; one per skin joint ],
  surfaces: [ { material: "iw4l_skate/<glTF material name>",   // only materials containing "Skate"
                texture: <index into textures>,
                vertices: [ { position:[f32;3], normal:[f32;3],
                              uv: u32 = (f16(u) << 16) | f16(v),
                              joints:[u32;4], weights:[f32;4] } ],
                indices: [u32]  // each triangle re-wound a,c,b } ],
  textures: [ { width, height, rgba: [u8] } ]  // PNG decoded, box-filtered to <= 512 on a side ] }
```

The user's file: 3 surfaces (`iw4l_skate/Retail_SkateBoard` 476 verts, `Retail_SkateTruck`
1085, `Retail_SkateWheel` 724), textures 512x512, 128x128, 128x128; 4.2 MB.

**Posing the board** (`rig.rs::board`, lines 155-219): for each joint `j`,
`M_j = pose.bones[index of j.target by name] * Rb * inverse_bind_j` with
`Rb = Mat4::from_cols(X, -Z, Y, W)`; skin each vertex with 4 weights; position through
`from_skate`, normal through `basis()`. Write into a CPU-posed body geometry (the IW4L
packed-vertex layout) and set `cpu_posed = true` (valhalla tree) so the GPU cache does not
freeze it.

### The retarget mapping (`rig.rs::mapping`, lines 22-48)

IW4 soldier bone -> skate bone, plus the child pair used to align the segment direction:

| IW4 bone | skate bone | aligned toward (IW4 child -> skate child) |
|---|---|---|
| `j_mainroot` / `pelvis` | `HIPS` | `j_spinelower` -> `SPINE` |
| `j_spinelower` | `SPINE` | `j_spineupper` -> `SPINE1` |
| `j_spineupper` | `SPINE1` | `j_spine4` -> `SPINE3` |
| `j_spine4` | `SPINE3` | `j_neck` -> `NECK` |
| `j_neck` | `NECK` | `j_head` -> `HEAD` |
| `j_head` | `HEAD` | terminal |
| `j_clavicle_le` / `_ri` | `LEFTSHOULDER` / `RIGHTSHOULDER` | `j_shoulder_*` -> `LEFTARM` / `RIGHTARM` |
| `j_shoulder_le` / `_ri` | `LEFTARM` / `RIGHTARM` | `j_elbow_*` -> `LEFTFOREARM` / `RIGHTFOREARM` |
| `j_elbow_le` / `_ri` | `LEFTFOREARM` / `RIGHTFOREARM` | `j_wrist_*` -> `LEFTHAND` / `RIGHTHAND` |
| `j_wrist_le` / `_ri` | `LEFTHAND` / `RIGHTHAND` | terminal |
| `j_hip_le` / `_ri` | `LEFTUPLEG` / `RIGHTUPLEG` | `j_knee_*` -> `LEFTLEG` / `RIGHTLEG` |
| `j_knee_le` / `_ri` | `LEFTLEG` / `RIGHTLEG` | `j_ankle_*` -> `LEFTFOOT` / `RIGHTFOOT` |
| `j_ankle_le` / `_ri` | `LEFTFOOT` / `RIGHTFOOT` | `j_ball_*` -> `LEFTTOEBASE` / `RIGHTTOEBASE` |
| `j_ball_le` / `_ri` | `LEFTTOEBASE` / `RIGHTTOEBASE` | terminal |

`SPINE2`, `NECK1`, the `*_REPARENTED` helpers and the board bones are not targets of the body
retarget. Note a special case before the table is consulted: `j_spine4`, `j_neck` and `j_head`
simply **copy their parent's skin matrix** (`rig.rs` lines 86-93), so the soldier's head rides
rigidly on the upper spine.

**How `pose()` works** (lines 60-150), in words: for every soldier bone, in hierarchy order:
attached models (weapons) inherit the posed attachment bone; a mapped bone gets
`skin = posed_skate_bone * inverse(skate_bind) * fit`, where `fit` is a rotation+translation
that moves the soldier's bind joint onto the skater's bind joint and turns the soldier segment
(child minus joint, pre-rotated by -90 degrees about Z, the soldier's facing) onto the
skater's segment; terminal joints reuse the alignment of their parent so wrists and head seams
stay attached; unmapped bones (fingers, gear) inherit the nearest mapped ancestor. Output per
bone is `skin * bind_world`, the usual skinning matrix.

For Halo the same idea applies to the Spartan biped (`objects\characters\masterchief`); only
the left column of the table changes to Halo node names, which the model-format reader will
list. Expect the Spartan rig to need the same "copy parent" trick for its head/neck chain and
a different facing rotation.

---

## 8. Threading model and channel protocol (`skate.rs`)

Three threads, all `std::thread` with **32 MB stacks** (the engine recurses deeply):

| thread | spawned | job |
|---|---|---|
| `skate-preload` | bevy `Startup`, if `IW4L_SKATE_ASSETS` is set | `Session::preload(root)`: decode the animation banks into a process-wide cache (`AnimationSource::load`, `skater_animation.rs` lines 41-48). Logged "Skate animation banks preloaded in N ms" |
| `iw4l-skate` | the first frame a map's collision (`DynEntPhysClip`) exists and `IW4L_SKATE_ASSETS` is set (`preload_map`, lines 123-282) | owns the `Session`; runs `collision::extract` then `Session::new(...)`, replies `Ready`, then serves jobs forever |
| `iw4l-skate-blocks` | inside `iw4l-skate` | Minecraft only: rebuilds collision around the skater with `CollisionBuilder::build` and sends it back; a Halo map does not need it |

Channels are `std::sync::mpsc`, one each way:

```rust
enum Job   { Activate(epoch: u64, spawn: Vec3 /* IW4 inches */, yaw_deg: f32, aspect: f32),
             Step(epoch: u64, dt: f32, InputFrame, aspect: f32),
             Suspend }
enum Reply { Ready,
             Activated(epoch: u64, Pose, elapsed_ms: u128),
             Pose(epoch: u64, Pose),
             Error(String) }
```

Rules the main-thread system `update` follows (registered in `ClientSet::Present`, after
`frame::PresentedPublished`, before `sync_camera_from_presented` and `render_scene::GfxSceneAdd`,
lines 36-48):

- An **epoch** counter is bumped on every activation and every stop; replies carrying an old
  epoch are ignored, so a late pose from a previous ride cannot move the player.
- Skating is allowed only when the local player is alive (`pm_type == 0`) and
  `AppScreen::InGame`; dying, leaving the match, or a map change calls `stop()`, which sends
  `Suspend` (the worker clears its input and pauses the gesture markers) but keeps the session.
- A different map (`Arc::ptr_eq` on the clip) throws the worker away and preloads a new one.
- `Reply::Error` stops skating, drops the worker and **clears `host.clip` so the next frame
  preloads a fresh session** for the same map.
- While active, every frame sends `Step(epoch, min(dt, 0.1), input, aspect)`; when the console,
  chat or script menu is open (`mode.input_blocked`) it sends one `Suspend` instead.
- On `Activated`/`Pose`: `present()` converts the pose, stores it in the shared
  `frame::SkateMode` resource (`root`, `bones`, `names`, `camera: Option<(Transform, f32)>`,
  `tick`, `status`) and tells the simulation `set_external_motion(client, true)` +
  `set_origin(client, root position)` so the sim stops moving the player itself and follows
  the skater (`sim/src/carrier.rs` lines 350-356: `external_motion` is a set of client ids the
  movement code skips).
- The render side reads `SkateMode`: `remote_body.rs` line 1044 calls `rig::pose(dobj, skate)`
  instead of the normal animation, line 1505 calls `rig::board`; `view_kick.rs` lines 253-278
  put the camera at `skate.camera` and set the lens FOV.

For a Halo runtime the same shape works: one worker owning the `Session`, main thread sends
input and reads poses, an epoch guards against stale data. The only IW4-specific parts are
the units and where the pose goes.

---

## 9. The converted Skate 3 data on this PC

Found through the installed mashup's `.env`:

```text
D:\GAME ISO AND ROMS\PC - AI MASHUP\mw2 skate 3\.env
  IW4L_GAMES="D:/Programs/SteamLibrary/steamapps/common/Call of Duty Modern Warfare 2"
  IW4L_SKATE_ASSETS="D:/GAME ISO AND ROMS/PC - AI MASHUP/mw2 skate 3/skate-data/assets"
```

`D:\GAME ISO AND ROMS\PC - AI MASHUP\mw2 skate 3\skate-data\` (188 MB, written 2026-10-14 by
`skate\iw4l-skate-convert.exe`; `conversion.log` = one line
`DEFAULT_SKATER_EXTRACTION_OK models=10 textures=33 ...`):

| path under `skate-data\assets\` | what | size / count |
|---|---|---|
| `rig.json`, `board.json` | derived by IW4L (section 7) | 35 bones; 4.2 MB |
| `private\skater.glb` | the skater model with skin and board | 4.3 MB |
| `private\game.json` | `{"version":1,"character_scene":"private/skater.glb","initial_animation":"R_IDLE_HCOM_000","action_graph":"private/stock/data/state/ActionGraph_OnBoard.stategraph","motion_graph":"private/stock/data/state/MotionGraph_OnBoard.stategraph"}` | 245 B |
| `private\character-lighting.json` | per-material shader params (`Retail_Arm` ... `character.skin`) | 4.9 KB |
| `private\native-character\*.png` | `Arm_specular`, `Rostral_specular` | 2 |
| `private\default_skater\textures\decoded\*.png` | raw textures by hash id | 33 |
| `private\default_skater\textures\materials\*.png` | `Arm/Feet/Hair/Organ/OuterTorso/Pants/Rostral/SkateBoard/SkateTruck/SkateWheel` `_base_color/_normal/_roughness` | 22 |
| `private\stock\physics-skeletons.json` | `PHYS_TPOSE` etc, bone words (f32 bits) + source offsets, `source_sha256` of `default.xex` | |
| `private\stock\skater-collections.json` | EA Reflection collections (`skatercollections.vlt`), hashed field names | |
| `private\stock\data\anim\*.abin` | animation banks: `OnBoard.abin` 10.8 MB, `OffBoard.abin` 4.0 MB, `PedestrianSkeletonPres.abin` 2.2 MB, `cac_edit_*`, `cac_viewer_*`, `Skater*`, `Marquee*`, `VehiclesDB` | 23 + `facebinds` 103, `faces` 104, `gesture` 90 |
| `private\stock\data\state\` | `ActionGraph_OnBoard.stategraph`, `MotionGraph_OnBoard.stategraph`, `ActionGraphIncludes` (23), `MotionGraphIncludes` (Grinds, Manual, Kickturn, Handplants, ...), `livingworldentities` | |
| `private\stock\data\config\input.cfg`, `joystick\*.pat` (7), `camera\*.shk` (2), `script\camera` (12), `skatepark` (12), `boundary` (561), `cacrecipes` (18), `particles` (19), `ropa2` (141), `content\aipaths` (59), `content\createacharacter\...`, `content\world\models` (22), `maps` (1), `credits`, `license`, `fe\memcard` | the rest of what the converter copies out of `data\big\{miscload,miscboot,db}.big` and `data\content\createacharacter.big` | |

`first_run.rs::skate_ready` (lines 136-145) treats the folder as usable when these four exist:
`private/skater.glb`, `private/game.json`, `private/stock/physics-skeletons.json`,
`private/stock/skater-collections.json`.

**The converter** (`skate/converter/iw4l_skate_convert.py`, packaged by `build.ps1` with
PyInstaller + a `refpack.dll` built from the engine's `tools/asset_pipeline/refpack_native.rs`):
`iw4l-skate-convert --xex <default.xex> --out <folder>`; requires `data/big/miscload.big`,
`miscboot.big`, `db.big`, `data/content/createacharacter.big` beside the xex; runs the
skate-3-rust-engine `tools/asset_pipeline/asset_exports.core` then `.character`; writes
`<folder>/assets`. ISO files are refused. The engine checkout is at `cb79689`.

A Halo runtime can point at this exact folder: nothing in it is IW4-specific except the two
derived JSON files, and even those only depend on `skater.glb`.

---

## 10. The Halo conversion

**Facts** (HANDOFF.md; `D:\reflex-tools\halo-tools\README.md` lines 60-64;
`D:\reflex-tools\halo-exports\cyberdyne\halo_export.json` `units`/`axes`):

- Halo world units are right-handed, **+Z up**, +X forward, +Y left.
- **1 world unit = 10 ft = 3.048 m** (Reclaimer `WorldUnitScale`; verified by HaloExport's
  probe: the `masterchief` render model is 0.703 wu = 7.03 ft tall).
- HaloExport's glTF transform is `gltf(x, y, z) = 3.048 * halo(x, z, -y)`, a proper rotation.

That is **the same axis swap as `to_skate`**, so for a native runtime that keeps Halo units:

```text
to_skate_halo(p)   = Vec3(p.x,  p.z, -p.y) * (3.048 * k)      Halo wu, Z up  ->  skate metres, Y up
from_skate_halo(p) = Vec3(p.x, -p.z,  p.y) / (3.048 * k)
basis()            = Mat4::from_cols(X, Z, -Y, W)              unchanged
heading            = halo_yaw_radians + PI/2                   if Halo yaw 0 = +X, CCW about Z (same as IW4)
spawn              = respawn point + ~0.05 wu up, then to_skate_halo
```

where `k` is the **scale question** below (`k = 1` is true scale).

**Rails for Halo**: `rails::find` is written for inches, Z up. The zero-risk path is to feed
it Halo triangles pretending they are inches: `p_in = p_wu * 120 * k` (1 wu = 120 in), keep
Z up, run `find` unchanged, then `to_skate` (x 0.0254) the result. The alternative, porting
`find` to metres, means dividing every constant in the table in section 6 by 39.37
(64 in = 1.6256 m, 3 in = 0.0762 m, 24 in = 0.6096 m, keys 3.175 mm / 12.7 mm / 25.4 mm) and
swapping which axis is "up" if you run it after the rotation.

**Collision source**: IW4L feeds the skate world the **clip** (collision) mesh, not render
triangles, "invisible player clips and solid props must remain solid" (`collision.rs` line
20). For Halo the stand-in maps currently use render geometry from the glTF because the real
collision BSP in `structure_bsp_tag_resources` is undecoded (HANDOFF.md "Known gap"). A native
reader should produce collision triangles with the material/surface so footstep sounds and
grind sparks can pick a surface later.

### The 7-foot Spartan vs. the skater (`k`)

`mashup-halo3-valhalla/crates/halo3_world/src/lib.rs` lines 11-13:

> `pub const UNITS_PER_METRE: f32 = 33.0;` "Map units per glTF metre. Halo is built for
> seven-foot Spartans, so the map is shrunk until one of them would stand as tall as an MW2
> soldier."

True scale would be 39.37 IW4 inches per metre, so IW4L draws and collides Halo at
`33 / 39.37 = 0.838` of its real size. Check: the IW4 player hull is 70 inches tall
(`maxs [15,15,70]`, that tree's `CLAUDE.md`), a Spartan is 84 inches; `70 / 84 = 0.833`.
And because `to_skate` is then applied to those inches, the skate engine sees the Halo glTF
multiplied by `33 * 0.0254 = 0.838`: a 2.14 m Spartan becomes 1.80 m, i.e. **exactly a
skater**. The glue works unchanged on Valhalla and The Pit.

What that implies for a native runtime, which must keep the world at true scale for Halo
gameplay later (weapons, vehicles, HaloScript all assume 1 wu = 3.048 m):

| choice | `k` | what the player experiences |
|---|---|---|
| **A. True scale** | 1.0 | a 1.8 m human skates architecture built for 2.1 m Spartans: every ledge, ramp and crate is 19 % taller than it "should" feel; a Halo 1 wu (3 m) crate is unreachable by ollie (Skate 3 ollies are about 1 m). The rendered skater is human-sized next to Spartans. |
| **B. Spartan-sized skater** (what IW4L ships) | 0.838 | the skate engine sees the map shrunk by 0.838; `from_skate_halo` divides by `3.048 * 0.838`, so the rider is drawn **2.15 m tall**, Spartan-sized, and the map proportions match what a Spartan sees. Physics constants (9.81 m/s^2, wheel radius, board length, trick heights, speeds) are untouched inside the engine; only the world is rescaled on the way in and the pose on the way out. |

Recommendation: make `k` one constant in the skate adapter (`collision in / rails in / spawn
in`, `pose out / camera out`), default to **B** (`k = 33 * 0.0254 = 0.8382`) because it is
proven on two Halo maps, and keep the renderer and future gameplay in true Halo units. The
only things that must agree on `k` are the four conversion points above; the camera FOV and
the velocity readout do not depend on it. Note that at `k = 0.838` the skater's 1/60 s step
covers 19 % more Halo distance per tick, so a speed shown to the player should be
`pose.velocity.length() / k` metres per second of Halo world.

---

## 11. Checklist: what the Halo runtime must provide

1. **Collision triangle soup** in skate metres, Y up, counter-clockwise from the standable
   side, no degenerate or duplicate triangles, one `Vec<[[f32;3];3]>` for the whole playable
   area (a static MP map) or rebuilt per BSP with `CollisionBuilder::build` +
   `install_collision` (campaign).
2. **Rails**: `Vec<Vec<[f32;3]>>` from the same soup through the lip detector (section 6), or
   later from Halo's own data if any map tag marks grindable edges.
3. **A spawn and heading** per activation: the player's position in skate metres plus a yaw.
4. **Controller** as raw XInput layout each frame (`InputFrame::from_pad`), no deadzones.
5. **A skeleton to pose**: a bone-name mapping like `rig.rs::mapping` from the Spartan model's
   nodes to the 35 skate bones, and a board mesh (reuse `board.json` and `rig.rs::board`).
6. **A worker thread** with the `Job`/`Reply` protocol and an epoch, plus a "player is being
   moved externally" switch in whatever movement code exists (noclip for now).
7. **A camera consumer** that accepts `(position, forward, up, vertical fov)` from the pose.
8. The Skate 3 asset folder path (`IW4L_SKATE_ASSETS` or our own setting) pointing at
   `...\mw2 skate 3\skate-data\assets`, or run the same converter into our project.

---

## Gap fill: Triangle winding / front-face convention

Added 2026-10-27. Full evidence and the byte-level check are in
`docs\format\sbsp-render-geometry.md`, section "Gap fill: Triangle winding / front-face convention"
(`docs\format\scripts\check_winding.py`). The part that matters to the skate adapter:

- A decoded Halo triangle `(i0, i1, i2)` is **counter-clockwise seen from the side its normals point
  to**, and for ground that is the top. Verified on riverworld: 37,260 of 37,260 cluster triangles and
  24,236 of 24,240 instance triangles have `cross(p1 - p0, p2 - p0) . (n0 + n1 + n2) > 0`; of the
  near-horizontal terrain triangles 21,241 face +Z and 1,534 face -Z. HaloExport's sidecar agrees
  (`winding_agrees_with_normals` = 1 riverworld, 0.9996 cyberdyne).
- The skate engine computes `normal = (b - a) x (c - a)` and marks every triangle one-sided
  (`skate-host\src\skate_world.rs` lines 237-246), so it wants exactly that order. **Do not swap
  vertices 1 and 2** the way `collision.rs::extract` does (lines 49-54): that swap undoes IW4's
  clockwise clip winding, which Halo does not have. `to_skate_halo` (section 10) is a proper rotation
  and preserves the order.
- The only flip a Halo soup ever needs is for a **mirrored geometry instance** (`det(R) < 0` in the
  sbsp `GeometryInstances` block): swap `i1` and `i2` for that placement. No riverworld or cyberdyne
  placement is mirrored (all 262 / 834 have det +1.0000), so this is a guard, not a hot path.
- Two-sided shaders are a separate matter: emit both orders if a surface must block from both sides,
  as `halo3_world` does for `double_sided` materials.
- The existing stand-in already proves the chain end to end: glTF (Halo order) -> `halo3_world` swaps
  to clockwise for IW4 -> `extract` swaps back -> skate engine sees the Halo order, and Valhalla and
  The Pit skate correctly.

Open: triangle strips were never seen in a BSP buffer, so the un-strip parity rule is assumed to give
the same convention; the real collision BSP's own orientation rule is not covered.
