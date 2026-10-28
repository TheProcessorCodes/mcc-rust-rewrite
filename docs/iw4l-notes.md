# IW4L notes: the bootstrap, render and camera pattern worth copying

Written 2026-10-27 from `D:\GAME ISO AND ROMS\PC - AI MASHUP\mw2-skate-3-src\src\mashup\`
(the newest tree; Apache-2.0, upstream github.com/vladtrc/iw4L) and, for the Halo and noclip
parts, `...\src\mashup-halo3-valhalla\`. Paths below are relative to the `mashup` tree unless
written in full. Nothing here was run; it is what the code says.

---

## 1. Versions and workspace shape

| item | value | source |
|---|---|---|
| toolchain | `stable`, components rustfmt + clippy | `rust-toolchain.toml` |
| edition | 2024, workspace `resolver = "3"` | `Cargo.toml` |
| bevy | 0.20.0 (locked) | `Cargo.lock` line 780 |
| wgpu | 30.0.1 (locked, through bevy) | `Cargo.lock` line 7962 |
| glam | bevy's 0.29.3 **and** the workspace's own `glam = "0.33.2"` for non-bevy crates (two copies in the lock) | `Cargo.toml`, `Cargo.lock` |
| other deps worth noting | `dotenvy 0.15` (`.env`), `serde/serde_json`, `flate2` with `zlib-rs`, `miniz_oxide`, `lzokay-native`, `bcdec_rs 0.2` (BC texture decode), `png 0.18`, `sha2`, `half`, `naga 29` with `wgsl-in` (render_gpu), `rfd 0.15` (Windows dialogs, launcher only) | `Cargo.toml`, crate manifests |
| crates | about 70 members; the skate crates and `third_party/minecraftoss` are **excluded** from the workspace and pulled in as path dependencies | `Cargo.toml` `[workspace]` |
| binary | `iw4l` from `crates/launcher` (`[[bin]] name = "iw4l"`), MSVC link arg `/STACK:8388608` (8 MB main thread) | `crates/launcher/Cargo.toml`, `build.rs` |

Lints: `clippy::excessive_precision = allow` and `approx_constant = allow` (retail constants
are transcribed bit-exact), `assertions_on_constants = deny`; `clippy.toml` forbids
`unwrap_or_default`. `rustfmt.toml`: `style_edition = "2024"`, `newline_style = "Unix"`,
field-init and `?` shorthand.

---

## 2. Bevy features actually enabled, and which a 3D viewer with a gamepad needs

`Cargo.toml` `[workspace.dependencies] bevy = { version = "0.20", default-features = false,
features = [...] }`:

| feature | why IW4L has it | needed for a bare Halo viewer with gamepad? |
|---|---|---|
| `bevy_window`, `bevy_winit` | the window and event loop | **yes** |
| `bevy_render`, `bevy_core_pipeline` | wgpu device/surface/present, `Camera3d`, the `Core3d` render graph that custom passes hook into | **yes** |
| `bevy_asset` | shaders and UI images as assets | **yes** (shaders go through `PipelineCache`) |
| `bevy_log` | logging | **yes** |
| `bevy_gilrs` | gamepad input (`Gamepad` components) | **yes** |
| `multi_threaded` | task pools; IW4L's loaders run on `bevy::tasks` | **yes** |
| `x11`, `wayland` | Linux windowing | Linux only |
| `png` | PNG decode through bevy's image loader (UI) | only if bevy loads images for you; IW4L decodes map textures itself |
| `bevy_state` | app states | optional |
| `bevy_ui`, `bevy_ui_render`, `bevy_text`, `bevy_ui_widgets`, `ui_picking` | menus, HUD, console | not for a viewer; useful once there is a console |
| `bevy_sprite`, `bevy_sprite_render` | 2D pieces of the UI | no |
| `system_clipboard` | console paste | no |
| `debug` | `Name`-style debug info | optional |

**Not enabled**: `bevy_pbr`, `bevy_gltf`, `bevy_scene`, `bevy_audio`, `default_font`. IW4L
draws every world triangle with its own wgpu pipelines (section 4), has its own audio crate,
and ships OFL fonts (Oxanium for UI, Fira Mono for the console). For a from-scratch Halo
viewer the honest minimum is `bevy_window, bevy_winit, bevy_render, bevy_core_pipeline,
bevy_asset, bevy_log, bevy_gilrs, multi_threaded` (+ `x11`/`wayland` on Linux). The skate host
itself only asks for `std, multi_threaded, bevy_log` (its `Cargo.toml`).

Optional bootstrap features: `bevy-debug = ["bevy/debug", "bevy/track_location"]` and
`bevy-trace = ["bevy/trace", "bevy/trace_chrome"]` (`crates/bootstrap/Cargo.toml`; the
comment explains the Chrome trace is the only way to time bevy's own `queue_submit`).

---

## 3. App bootstrap order

### Process entry: `crates/launcher/src/main.rs` (84 lines)

1. `#[global_allocator]` = `diag::ProcessCountingAllocator` (memory counters for the bench).
2. `iw4l licenses` prints the five embedded license texts and exits.
3. `prepare_process_root()`: on Windows `set_current_dir(exe dir)` so relative paths (`.env`,
   `skate\`, `iw4l-artifacts\`) resolve beside the exe (double-click friendly).
4. Windows only: `first_run::prepare()` (`first_run.rs`, 274 lines): read `.env`; if
   `IW4L_GAMES` does not point at an MW2 with `mp_rust`, search Steam libraries
   (`libraryfolders.vdf`, `C:\Program Files (x86)\Steam`, `X:\SteamLibrary` for every drive)
   and confirm with an `rfd` Yes/No dialog, else a folder picker; then unless
   `IW4L_SKATE=off`, check `IW4L_SKATE_ASSETS` with `skate_ready()`, else ask "Do you have
   Skate 3?", pick `default.xex`, run `skate\iw4l-skate-convert.exe --xex .. --out skate-data`
   echoing its stdout and catching `ERROR:` lines, write `IW4L_SKATE_ASSETS=<out>/assets`;
   finally `assets::skate_board::ensure(assets)` writes `rig.json`/`board.json`. `.env` lines
   it does not own are preserved. Any failure shows a dialog and exits with code 2.
5. `ensure_artifacts_dir()` -> `iw4l-artifacts\` (logs, cache, clips, settings.cfg,
   account.dat), then `diag::init_log` and a stdout line naming the log file.
6. `updater::startup()` may rewrite argv or return `None` (exit after update).
7. Empty args mean `menu`. `bootstrap::bench::arm()`.
8. `bootstrap::parse_cli(args)` -> `(LaunchMode, Option<AcceptanceLaunch>, sim::HostCheats)`.
   `LaunchMode = Menu | Map(zone) | Serve(zone) | ExportGltf(zone) | Play { name,
   zone_override }` (`args.rs`); `--no-cheats` turns `HostCheats` off; `--cmds '<script>'`
   is stripped by `console::strip_cmds_flag`; `--render-acceptance <dir>` for screenshot tests.
9. `games_root_from_env()` (reads `.env` via `asset_transport::load_dotenv` ->
   `dotenvy::from_path`, `asset_transport/src/discover.rs` lines 23-49) then
   `bootstrap::launch(games, artifacts, mode, acceptance, cheats)`.

### Composition root: `crates/bootstrap/src/launch.rs` + `plugins.rs`

`launch()` (lines 251-305): `set_game_folders` (extra game trees from `settings.cfg`),
`link_steam_games`, log the games-root report, on Linux pin to performance cores
(`frame_owner.rs`), then dispatch: `run_menu`, `run_map(zone, role Listen)`,
`run_map(role Dedicated)` for `serve`, `run_export_gltf`, `run_play` (replay).

`run_map()` (lines 598-820), the path a Halo viewer would copy:

1. Resolve the zone file and `common_mp`, content families, master-server intent, decide the
   role (`Listen | Dedicated | Client | Replay`), start `perf`, load the UI menu catalog.
2. `App::new()`; optional `ui::PresentModeOverride` from `IW4L_PRESENT_MODE`
   (`AutoNoVsync | Immediate | Mailbox | Fifo`, default Fifo = vsync).
3. Dedicated: `WinitSettings::continuous()`, `frame::Headless`, `ExitCondition::DontExit`,
   no primary window.
4. `app.add_plugins(default_plugins_with_quiet_log(WindowPlugin { primary_window: Window {
   title, resolution: (ACCEPTANCE_WIDTH, ACCEPTANCE_HEIGHT), present_mode, .. } }))`.
   That helper (`plugins.rs` lines 97-124) is `DefaultPlugins`
   - `.set(window)` with `desired_maximum_frame_latency = 1` (2 on macOS; env
     `IW4L_FRAME_LATENCY`),
   - `.set(LogPlugin { filter: "warn,iw4l=info", level: WARN })`,
   - `.set(bevy::render::RenderPlugin { render_creation: RenderCreation::Automatic(WgpuSettings
     with features TEXTURE_FORMAT_16BIT_NORM | TEXTURE_COMPRESSION_BC | POLYGON_MODE_LINE |
     TEXTURE_BINDING_ARRAY | SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING |
     PARTIALLY_BOUND_BINDING_ARRAY) })`,
   - `.disable::<PipelinedRenderingPlugin>()` when `IW4L_PIPELINED_RENDERING=0` (default on:
     render thread one frame behind extraction).
5. Insert resources: `LoadingPreviewSource`, `LaunchIdentity { role_label, games_root,
   artifacts, zone }`, `HostCheats`, `LaunchReport`, `MenuCatalog`, `MatchLoadRequest { zone,
   zone_ff, common_mp, progress, families }`, `LoadingScreen`, `UiAssetRoot`,
   `StartupCommands { lines: console::startup_commands() }`, `ClearColor(BLACK)`,
   `AppScreen::Loading`, `UiLayers` showing `Loading + Overlay`.
6. `add_runtime_plugins(_with_role)` (`plugins.rs` lines 22-85): `AudioRuntime`, then
   `AssetPlugin, UiPlugin, ConsolePlugin, NetPlugin::{listen|dedicated|client|replay}(),
   BotsPlugin, HudPlugin, AudioPlugin, ReplayPlugin, theater::TheaterPlugin,
   render::RenderPlugin, SessionPlugin`. Then, in the `RenderApp` sub-app, the `RenderGraph`,
   `Core3d`, `Core2d`, `Render` and `ExtractSchedule` schedules are switched to
   `SingleThreadedExecutor` ("multithreaded render schedules compete with camera prepare's
   own ComputeTaskPool lanes for the same workers and lose ~5% fps"); macOS keeps the
   multi-threaded `Render` executor because `create_surfaces` must run on the main thread.
7. `register_shell_common_systems` (menu content in the background), `ScopeControl.begin_load()`,
   optional `CaptureRequest::from_env()` screenshot, bench manifest, **`app.run()`**,
   `perf::flush()`.

`render::RenderPlugin` (`crates/render/src/plugin.rs`) is itself a group:
`render_anim::RenderAnimPlugin, render_fx::RenderFxPlugin, render_scene::RenderScenePlugin,
render_frontend::{RenderPreparePlugin, RenderAdaptersPlugin, RenderAssemblePlugin},
render_gpu::RenderGpuPlugin`, plus extract systems added to the `RenderApp`'s
`ExtractSchedule` (`extract_exact_colour`, `extract_image_handles`, `extract_postfx`,
`extract_model_lighting_tiles`, `extract_minecraft_world`).

### Schedules (`crates/frame/src/schedule.rs`)

- `ClientSet` in `Update`, chained: `Load, Receive, Reconcile, Input, Predict, Send, Present,
  Ui, Effects, Diag`. Inside `Present`: `RenderSet::{FrontendPrepare, Anim, Fx,
  FrontendAssemble, Gpu}` chained, and the 21 `WorkerCmdSet`s named after IW4's worker
  commands (`cell_dyn_brush` ... `physics`).
- `AuthoritySet` in `FixedUpdate`, chained: `Advance, Ingress, Gather, Step, Snapshot, Fanout,
  Bookkeeping` (the server-side simulation; a listen host runs both).
- Marker sets used as ordering anchors: `PresentedPublished`, `SessionSwapApplied`,
  `ModelLightingSeated`, `FxSoundPublished`, `OwnerEventsPublished`.
- `AppScreen = MainMenu | Loading | ClassSelect | InGame` and `RuntimeRole = Listen |
  Dedicated | Client | Replay` (`frame/src/session.rs`).
- Match-scoped entities carry `DespawnOnExit<frame::MatchScope>` (`frame::scope`), which is
  how the camera and world entities vanish on map change.

What to copy for Halo: the launcher/bootstrap split (process entry vs. an `App` assembler),
`.env` + `iw4l-artifacts` beside the exe, a `LaunchMode` enum parsed before bevy starts, one
`default_plugins_with_quiet_log(WindowPlugin)` helper, and the chained client sets with
`Present` holding render prep. The net/authority split is not needed until Halo gameplay.

---

## 4. Rendering: bevy's renderer for the window, custom wgpu for the pixels

- **bevy owns**: the wgpu instance, adapter, device, queue, surface, swapchain and present
  (through `bevy_render` + `bevy_winit`), the `Camera3d` entity, the `Core3d` render graph,
  the `PipelineCache`, `RenderDevice`/`RenderQueue` resources, the extract/prepare/queue/
  render schedules and pipelined rendering.
- **IW4L owns**: everything drawn. `render_backend` is the CPU port of IW4's `RB_*`
  (draw lists, tess lists, material execution); `render_frontend` the `R_*` prepare/assemble;
  `render_gpu` ("wgpu resources, pipelines, upload, render graph, and submit") builds raw
  pipelines (`RawRenderPipelineDescriptor`, WGSL via `naga 29`; MW2's D3D9 shader model 3
  bytecode is translated to WGSL by the `d3d9_sm3` crate) and records its own passes into
  `Core3d` using `RenderContext`, `ViewTarget`, `RenderDevice`, `RenderQueue` (see
  `render_gpu/src/drawsurf/colour_submit/`, `iw_tess.rs`, `postfx.rs`, `floatz.rs`).
- The camera is a plain `Camera3d` child entity (`FpvLens`) with `Projection::Perspective`
  (fov = vertical from a 65 degree horizontal, `near` in inches, `far` from the scene radius),
  `Tonemapping::None`, `DebandDither::Disabled`, `Msaa::Off`, depth texture usages
  `RENDER_ATTACHMENT | TEXTURE_BINDING`, and the marker `frame::DisplayEncodedCamera`
  (`render_frontend/src/prepare/scene/world_occupancy.rs` lines 330-355). `render_gpu`'s
  plugin forces that camera's target to `Rgba8Unorm` and its compositing space to sRGB
  ("the custom scene and HUD passes write sRGB values into unorm targets",
  `render_gpu/src/plugin.rs`).
- The cleanest self-contained example of this pattern, about 600 lines, is the Halo stand-in
  world pass: `mashup-halo3-valhalla/crates/render_gpu/src/drawsurf/halo_world.rs`. It owns a
  `HaloWorldFrame { active, scene: Option<Arc<Scene>> }` resource extracted from the main
  world, uploads once per scene (`upload_scene`: one `Buffer` pair per batch, one
  `MaterialParams` uniform + 9 texture views + sampler bind group per material, textures via
  `create_texture` with mips), a per-frame `HaloView` uniform (`clip_from_rel`,
  `rel_from_clip`, `view`, `sun`, `sun_colour`, `sky`, `ground`, `horizon`, `zenith`, `fog`),
  draws opaque then translucent before the exact IW4 material passes with the same reverse-Z
  depth range, and fills the sky with a gradient. Start the Halo renderer from this file's
  shape, not from the IW4 material system.

---

## 5. Camera, flying and noclip

### The world camera (`crates/render_scene/src/camera.rs`)

```rust
#[derive(Component)]
#[require(DespawnOnExit<frame::MatchScope> = DespawnOnExit(MatchScope::Live))]
pub struct FlyCamera { pub yaw: f32, pub pitch: f32, pub speed: f32 }   // host entity, Transform in IW4 inches, Z up

#[derive(Resource, Default)]
pub struct SimCamera { pub enabled: bool, pub freeze_fly: bool, pub view_override: Option<CameraView> }
pub struct CameraView { pub transform: Transform, pub horizontal_fov: f32 }
pub struct WorldCameraPose { pub origin: [f32; 3], pub angles: [f32; 3] }  // IW4 pitch, yaw, roll in degrees
pub fn transform_from_iw_view(view: WorldCameraPose) -> Transform
```

`transform_from_iw_view` builds `forward = (cos p cos y, cos p sin y, -sin p)` plus right/up
with roll and makes the rotation from `Mat3(right, up, -forward)`: the Z-up world and bevy's
-Z-forward camera reconciled in one place. The `FlyCamera` host is spawned by
`world_occupancy.rs` when a world is ready, placed at the map's intermission view if it has
one, else at an eye computed from the scene centre and radius, with the `Camera3d` lens as its
child; `speed` is 320 in/s with an intermission view, else `radius * 0.25`.

### Free flight: `fly_camera` (`crates/render_frontend/src/prepare/scene/camera.rs`)

Runs every frame unless `SimCamera.enabled || freeze_fly`. Arrow keys look (1.6 rad/s, pitch
clamped to +-1.54 rad, yaw via `rotate_z`, pitch via `rotate_local_x`), `W/S` along the
flattened forward, `A/D` strafe, `E` up, `Q` down (world Z), left Shift x6, moving
`normalize(dir) * speed * sprint * dt`. This is the "developer camera" goal 2 asks for: a
transform plus a speed, driven by keys, with no collision at all. Nothing in it is IW4
specific except the unit.

`SimCamera.enabled` is false by default (`render_scene/src/plugin.rs` `init_resource`), so a
viewer that never enables it gets the fly camera for free. In a match
`sync_camera_from_presented` (`crates/render_anim/src/occupancy/view_kick.rs` line 209)
overrides the `FlyCamera` transform each frame from, in priority: `SimCamera.view_override`
(theater), then only if `enabled`: the skate camera (`SkateMode.camera`, vertical FOV -> lens
and converted to IW4's 4:3 horizontal `refdef` FOV), the killcam, or the first-person view of
the presented player. Theater's free camera (`crates/theater/src/editor.rs` lines 959-985,
`CameraMode::Free`) is a second implementation: right-mouse drag looks (0.003 rad/px), `WASD`
+ `Q/E`, 200 in/s or 1200 with Shift, written into `view_override`. Either is fine to copy;
the first is 90 lines.

### Noclip (only in `mashup-halo3-valhalla`)

The newer `mashup` tree has no noclip; the valhalla tree does, and the user's install keeps
those builds as `...\mw2 skate 3\iw4l-old-noclip.exe` and `noclip exe backup\iw4l.exe`
(Oct 21). Pieces:

- Console command `noclip` (alias `ufo`), `crates/console/src/debug_move.rs` lines 69-75 and
  172-206: needs the local player Alive, a listen host (the authority), cheats on, and not
  skating; calls `authority.toggle_noclip(local)`.
- `sim/src/carrier.rs` lines 350-365: `toggle_noclip` flips `pm_type` between `0` and
  `movement_iw4::PM_TYPE_NOCLIP` (= 2, IW4's `PM_NOCLIP`), returns `None` in any other state.
  `set_external_motion` next to it is the switch the skate glue uses.
- Movement: `crates/movement_iw4/src/single.rs` lines 920-955 `noclip_move`: wish =
  `forward * forwardmove/127 + right * rightmove/127`, `+Z` while jump is held, `-Z` while
  crouch or prone, normalised if longer than 1, speed `NOCLIP_SPEED = 190 * 3 = 570 in/s`
  (14.5 m/s), `x 2.5` with sprint (1425 in/s); origin += velocity * frametime; clears the
  ground entity and the ladder/mantle/jumping/sprinting/prone/crouch flags; `pmove` returns
  early after it (line 1002).
- Hotkey: `GameSettings.noclip_key` default `"O"` (`crates/frame/src/settings.rs` lines 92-128),
  `console/src/plugin/mod.rs` `noclip_hotkey` pushes the line `noclip` into the console queue.

For Halo, goal 2 ("noclip") is the fly camera (section above) first; the player-attached
noclip only matters once there is a player.

---

## 6. Input: gamepad and keyboard

- Gamepads come from `bevy_gilrs`: systems query `Query<&bevy::input::gamepad::Gamepad>` and
  pick the active one through `frame::ActivePad(Option<Entity>)`; `frame::InputDevices {
  pad_prompts, focused, aiming_with_pad }` tells the UI which glyphs to show
  (`crates/frame/src/pad.rs`, `ControllerIcon` maps A/B/X/Y/D-pad to private-use glyphs and
  `iw4l_controller_*` materials).
- `frame::GameSettings` (`settings.rs`) holds the pad options worth copying: layout, stick
  layout, sensitivity presets `[0.6, 1.0, 1.4, 1.8, 2.0, 2.2, 2.6, 3.0, 3.5, 4.0]`, custom
  and ADS sensitivity, invert, curve, acceleration, aim assist, prompts, vibration,
  deadzones 0.12 left/right, plus display: 1280x720, fullscreen off, vsync on, FOV 65
  (min 65, max 120), shadows/DoF/bloom on, mouse sensitivity 5.0.
- `crates/input_iw4` is a `no_std`, `forbid(unsafe_code)` port of IW4's client input: `Kbutton`
  with millisecond-fraction key timing (`key_state`), `KbuttonSet` of the 28 game buttons,
  the 83 `+attack`/`-attack`... command names (`names.rs`), `UserCmd` assembly
  (`forwardmove/rightmove: i8` from `axis_to_move`, `angles: [i32; 3]` with
  `ANGLE2SHORT = 65536/360`, mouse sensitivity and acceleration). That is MW2 behaviour; a
  Halo runtime wants Halo's, so only the structure (bind name -> command id -> button bit ->
  per-tick command) is reusable.
- For skating the pad is read **raw** in XInput layout with no deadzone
  (`render_anim/src/skate.rs::pad_frame`; see `docs/skate-interface.md` section 3).

---

## 7. Profiles and build settings worth copying (`Cargo.toml`)

```toml
[profile.release]            # the shipped binary: wall p50 -5.7 %, 183 MB -> 114 MB
lto = true
codegen-units = 1
strip = "symbols"

[profile.play]               # the dev loop (make map / play / bench): release speed without the fat-LTO link
inherits = "release"
opt-level = 3
lto = false                  # crate-local ThinLTO; the fat link was timed at 13m03s and ~6.6 GB
codegen-units = 16
incremental = true
strip = "symbols"

[profile.perf]               # same code as play, plus what a sampler needs
inherits = "play"
debug = 1                    # DWARF line tables
strip = "none"               # recipe adds RUSTFLAGS=-Cforce-frame-pointers=yes
```

`Makefile`: `PROFILE ?= play`, `cargo run --profile $(PROFILE) -p launcher -- map <zone>`;
`PROFILE=release` only for published numbers. Bench runs set `IW4L_BENCH=1 IW4L_PERF=1
IW4L_PRESENT_MODE=AutoNoVsync`. Copy the three-profile idea as is: bevy is unusable at
opt-level 0, and a fat-LTO link on every edit kills the loop.

Environment switches that exist and are cheap to keep: `IW4L_PRESENT_MODE`,
`IW4L_PIPELINED_RENDERING`, `IW4L_FRAME_LATENCY`, `IW4L_SKATE_ASSETS`, `IW4L_GAMES`,
`IW4L_PERF_FOCUS`.

---

## 8. What IW4L already does with Halo 3 data (valhalla tree)

Everything Halo in IW4L goes through **HaloExport's glTF** (`scene.gltf` + `scene.bin` +
`textures\*.png` + `halo_export.json`), never the `.map` file. Two pieces:

### `crates/halo3_world/src/lib.rs` (728 lines; deps `bytemuck`, `image` (png only), `serde_json`)

Reads one export into what the mashup needs, "in IW4 map units, Z up":

| output | type | how it is computed |
|---|---|---|
| placement | `Layout { anchor: [f32;3] glTF metres, spawns: &[([f32;3], yaw_deg)], intermission: ([f32;3], [pitch,yaw,roll]) }`; `to_map(anchor, p) = ((p-a).x*33 + 40000, -(p-a).z*33, (p-a).y*33)`; `UNITS_PER_METRE = 33`, `PLACE = [40000, 0, 0]` | glTF metres Y-up -> inches Z-up, scaled 0.838 (see skate-interface.md section 10), moved 40000 in away from the proxy map |
| `Material { name, shading, textures: [Option<u32>; 9], xforms: [[f32;4]; 9], layers: [bool;4], double_sided }` | `Shading = Opaque | AlphaTest | Terrain | Translucent` | from glTF `materials[].extras.halo.{category, textures[{parameter, image, xform}], options{material_k}}` and `pbrMetallicRoughness.baseColorTexture`, `alphaMode == "MASK"`, `doubleSided`. Slot 0 = base map on `uv0` (terrain: the blend map), slots 1..=4 terrain layer base maps, 5..=8 detail maps on `uv1` (non-terrain: slot 5 = `detail_map`); `xform = [scale x, scale y, offset x, offset y]` applied to `uv1`; `layers[k] = options.material_k != "off" && base map k present` |
| material use | `material_use(name, category)` | `invisible*` / `special` -> not drawn but **collides**; `decal*`, `additive`, `z_*`, `spawn_point*`, `multiplayer_*`, `koth_*`, `*decals*`, `*nutblocker*`, `*plasma*` -> dropped; `foliage` -> AlphaTest, no collision; `alpha_test` -> AlphaTest, collides unless `leaf`/`netting`; `alpha_blend` -> Translucent, collides unless `waterfall`; `water` -> Translucent, no collision; `terrain` -> Terrain, collides; else Opaque, collides |
| surface type | `surface_of(name, category) << 20` | IW4 surface ids for footsteps: terrain `snow`/`concrete|tarmac|outer`/`cliff|rock`/`water`->gravel/`distant`->dirt/else grass; others `glass`, `bark|tree|stool`->bark, `rock|cliff|lake`, a metal word list (`metal, panel, military, pelican, forerunner, generator, mongoose, radio, longsword, grill, resupply, crate`), else concrete |
| `Batch { material: u32, vertices: Vec<Vertex>, indices: Vec<u32> }`, `Vertex { position, normal, uv0, uv1 }` (`repr(C)`, `Pod`) | **one batch per material**, all nodes and instances merged | each node's `matrix` (column-major 16) applied to positions and normals, negative determinant flips winding; node `mode != 4` (not triangles) skipped; nodes whose `extras.iw4l.model` is one of 34 `GAMETYPE_MODELS` (`respawn_point`, `respawn_point_invisible`, `ctf_*`, `koth_*`, `oddball_*`, `assault_*`, `territories_*`, `vip_*`, `juggernaut_*`, `infection_*`, `slayer_*`, Forge `box_*`/`wall_*` blockers) are skipped and counted |
| `Texture { name, width, height, levels: Vec<Vec<u8>> }` | RGBA8 sRGB, full mip chain | PNG decoded with `image`, only textures a kept material references, mips box-filtered **in linear light** (gamma 2.2) down to 1x1 |
| `CollisionTriangle { verts: [[f32;3];3], surface: u32 }` | clockwise seen from the blocking side (IW4's convention) | glTF is counter-clockwise, so `b,c` swapped unless the node matrix flipped; `double_sided` materials emit both windings |
| `Spawn { origin, yaw }`, `intermission`, `mins/maxs`, `skipped_nodes`, `summary()` | | spawns are the hand-placed `Layout.spawns` through `to_map`; bounds from every placed vertex |

### `crates/assets/src/halo3_map.rs` (499 lines)

The table `MAPS: [&HaloMap; 2] = [&VALHALLA, &THE_PIT]`, each
`HaloMap { zone: "halo3:valhalla", label, export_env: "IW4L_HALO3_VALHALLA", default_export:
r"D:\reflex-tools\halo-3-maps\halo-3-valhalla", layout, team_axis, pictures: "valhalla" }`
(The Pit: `IW4L_HALO3_THE_PIT`, `D:\reflex-tools\halo-exports\cyberdyne`, anchor
`[0, -0.9, 0]`; Valhalla anchor `[236, 0, 302]`, both 16 spawns on the maps' own respawn
markers). `find_scene(dir)` accepts `dir\scene.gltf` or the newest
`dir\build\*\halo_prep\scene.gltf`. Pictures `{stem}_loading.png` (1920x1080) and
`{stem}_preview.png` (668x292) from `D:\reflex-tools\halo-exports\previews` (env
`IW4L_HALO3_PICTURES`), pulled from MCC's `Data\UI\texturepacks`.

`stand_in(zone, prepared)` turns a walk of the proxy map `iw4:mp_rust` into the Halo map:
collision = `clipmap_iw4::mesh_tables_from_triangles(scene.collision)` with
`CONTENTS_SOLID = 1` and materials named `halo3/surface_{id}`; the proxy's `cmodels` keep
their slots with zero brushes and cmodel 0 is widened to the Halo bounds +-256; the entity
string is rewritten (`mp_*_spawn` and `mp_global_intermission` removed, then per spawn
`mp_dm_spawn`, `mp_tdm_spawn` and `mp_tdm_spawn_{allies|axis}_start` by which side of
`PLACE[team_axis]` it lies, then the intermission); `PreparedMap::spawns` replaced;
`intermission_view` set; `exp_fog = None`; world bounds extended.

Integration points in that tree (for the record): `assets/src/match_load.rs` lines 167-178
(post-pass after the resident-map cache), `session/src/match_apply.rs` line 1164 (level script
of `PROXY_MAP`), `render/src/extract.rs` lines 949-953 (`HaloWorldFrame`),
`render_gpu/src/drawsurf/halo_world.rs` (the draw pass, section 4), `ui/src/frontend/maps.rs`
and `ui/src/loading.rs` (pictures), `render_frontend/.../model_lighting_cache.rs` line 219
(no light grid while a Halo map is live, same as the voxel world).

### What the native `.map` reader must therefore produce

To replace the glTF step and feed the same consumers (and the skate glue), per BSP:

1. **Draw batches per material**: positions, normals, `uv0`, `uv1`, u32 indices, already
   placed in world space (instances flattened or kept as instance transforms), with a material
   table giving a shading class (opaque / alpha-test / terrain blend / translucent), up to 9
   texture slots with uv scale+offset, the terrain layer mask, double-sidedness, and the
   shader category the material came from (the name-based heuristics above need it).
2. **Textures** as RGBA8 (or the BC formats directly, since `TEXTURE_COMPRESSION_BC` is on)
   with mip chains, only for referenced materials.
3. **Collision triangles** with a per-triangle surface/material id and a known winding, ideally
   from the real collision BSP (`structure_bsp_tag_resources`, undecoded today) rather than
   render geometry; the skate glue then needs no per-map code (its rail detector runs on any
   soup, see `skate-interface.md`).
4. **Spawn points** (the `respawn_point` scenery placements with yaw) and the map's
   intermission camera (today hand-written per map), plus bounds.
5. The gametype/Forge marker objects to **hide** (the 34 names), the sky (today a gradient),
   and the loading/preview pictures, which already exist for Valhalla and The Pit under
   `D:\reflex-tools\halo-exports\previews`.
