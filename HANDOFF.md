# Halo 3 Rust rewrite: handoff

First written 2026-10-27 at the end of a research thread in `mw2-skate-3-src`; last
rewritten 2026-10-28 after the lighting, rider, campaign, streaming, water, sound and
multiplayer-map work. `README.md` has the layout and the per-feature status list; `docs/` has
the format notes the code is written from; `docs/viewer.md` has the controls.

## The goal

Do for Halo 3 what `mw2-skate-3-src` did for MW2: get our own code running with Halo 3
so the user can **skate (Skate 3 physics) on real Halo 3 maps**.

| step | goal | state |
|---|---|---|
| 1 | get custom code into Halo 3, via a rewrite or a custom launcher | done as a rewrite: our code reads the real maps from the MCC install |
| 2 | noclip: fly around the maps | done and playtested (keyboard, mouse after Tab, gamepad, Shift for speed) |
| 3 | skate on multiplayer maps | done and playtested on Valhalla: the Skate 3 crates are linked and Master Chief rides. Guardian, Last Resort, High Ground and Blackout are built and pass tests but are not playtested |
| 4 | skate on campaign maps | narrowed to the final mission, `120_halo`: zone sets, streaming, culling and skating across zone sets are built, awaiting a playtest. Other missions not checked |
| later | dirt bike game (MX vs ATV Reflex) in the mix | started: B rides a dirt bike on MX vs ATV Unleashed's Pro 250 model (`docs/bike-unleashed.md`); the user called it the best base so far (2026-10-29) |

## The decision (made 2026-10-27)

The user chose **A, a from-scratch Rust rewrite built the way IW4L was built**: own readers for
the MCC `.map` files, bevy 0.20 and wgpu for the window and drawing, the Skate 3 crates plugged
in later with the same glue pattern as the MW2 mashup. Options B (own launcher for `halo3.dll`)
and C (halo-3-recomp) are off the table unless the user reopens them.

The licence question is handled by **not porting code**: Reclaimer, TagTool and Assembly are all
GPL-3.0, so the readers were written from format facts (offsets, sizes, meanings) recorded in our
own words under `docs/format/`, each verified against the bytes of the user's maps with small
Python probes (`docs/format/scripts/`). This project stays Apache-2.0 like IW4L. Nobody has
copied a line of C# or XML; keep it that way, or raise it with the user first.

## What is done

A Cargo workspace (`cargo build`, `cargo test`, all green). Crates:

| crate | what it does |
|---|---|
| `blam_cache` | opens a `.map` read-only (memory-mapped): header, sections, tag index and names, string ids, a tag data cursor, the resource system (zone gestalt, play layout, deflate pages, shared caches) |
| `blam_geometry` | structure BSPs, render models, scenario objects, sky, the real collision BSP, lightmaps, atmosphere, camera fx (exposure), zone sets, trigger volumes and cluster portals, into one plain `Scene` |
| `blam_texture` | bitmap tags to mip chains (BC kept compressed, own CPU decoders) and the shader resolver |
| `blam_sound` | sound tags and MCC's FMOD bank (`halo3\fmod\pc\sfx.fsb`, Vorbis, setup headers rebuilt from the user's `halo3.dll`), decoded with lewton |
| `blam_cli` | the `blam` tool: tags, dumps, geometry and rails to OBJ, bitmaps to PNG, shaders, lightmaps, sound |
| `blam_script` | the scenario's HaloScript: scripts, globals, expression nodes, the names they use; decompiled to source |
| `halo3_script` | runs a map's scripts in the game (threads, objects, devices, trigger volumes, insertions), with AI and animation hooks |
| `halo3` | the game: bevy 0.20, flying, skating, the dirt bike, sound, the pause menu, headless screenshots |

What the game does now:

- **Look.**
  - Lighting: Halo 3's baked lightmaps (SH coefficients and intensity; per-vertex, probe and per-pixel instances).
  - Surfaces: the four-layer terrain blend; detail and normal maps on Halo's own tangents; specular from the lightmap's dominant light; self-illumination and plasma glow; alpha-tested cut-outs; multiply decals.
  - Sky: the real sky (`skya`) with scattering, haze per cluster through the BSP's atmosphere palette, night skies (Blackout).
  - Exposure: per map, plus auto exposure, from the camera fx.
  - Spawn markers and editor-only objects are hidden.
  - Docs: `docs/format/lightmaps.md`, `sky-and-atmosphere.md`, `shaders.md`.
- **Water.** Animated waves from the shader's slope arrays, sky reflection, depth murk, shore fade, foam and waterfalls. Deep water sinks the skater, who respawns at the last dry spot (`docs/format/water.md`, `swim.rs`).
- **Skating.**
  - The Skate 3 crates come from `mw2-skate-3-src` as a path dependency and run on a worker thread (`skate.rs`).
  - Skate collision is the real collision BSP; grind rails come from IW4L's lip detector.
  - Kill volumes and the bottom of the world respawn you (`falls.rs`).
  - J or both sticks toggles skating.
- **Rider.** Master Chief (Mark VI helmet) is retargeted onto the 35 skate bones, with his head attached and feet flat, and is lit from the lightmap under him (`rider.rs`, `docs/rider.md`). Playtested: "looks great".
- **Dirt bike.** B rides it, with Master Chief on it (`halo3_bike`). Its physics are MX vs ATV Unleashed's model pulled towards stock Reflex ("Reflex v1", `reflex.rs`); 0 while riding opens the physics tuning menu (`tune.rs`), which also switches back to plain Unleashed (see "Dirt bike" below).
- **On foot.** F walks the Spartan (`foot/`): the map's own movement numbers (`blam movement`,
  `docs/format/movement.md`), a capsule on the collision, a made-up gait on the skate skeleton,
  shoulder and eye cameras. He carries the map's weapons (`weapons/`, `blam weapons`,
  `docs/format/weapons.md`): placed where the scenario puts them, picked up, fired with the tags'
  rates, magazines, bursts, charge and zoom, projectiles traced against the collision. The user
  has walked, jumped, fired and reloaded in it. Open: the tag's jump looks high (4.5 m); no damage,
  no Halo animations, no first-person arms.
- **Campaign, 120_halo.** Its 8 zone sets switch by PageUp / PageDown, `--zone`, or the scenario's own switch volumes as you move. You start at the player starting location (`docs/format/zone-sets.md`).
- **Culling and streaming.**
  - The world is chunked into 64 wu cells.
  - Culling is by frustum, cluster portals (`docs/format/sbsp-portals.md`) and screen size; instance fade sizes are honoured.
  - Only the loaded zone set's meshes and textures are on the GPU, built in the background. Skate collision is swapped in without stopping the ride.
  - On section 6 this takes 206 MiB of textures instead of 383. Frame time is now limited by pixel shading, not draw count (`docs/viewer.md`, `docs/format/lod.md`).
- **Sound.**
  - Cluster ambience with its random details, cross-faded as you move.
  - Sound scenery placed in the world and attenuated by distance.
  - Music by hand: M, or `--music` (the Warthog run on 120_halo).
  - Board roll, grind and landing sounds.
  - K mutes; `-` / `=` set the volume (`docs/format/sound.md`).
- **Pause menu.** Escape or Start. It has tabs for free roam, dirt bike and skate controls; Q quits.
- **Headless screenshots.** `halo3.exe <map> --shot <dir> [--view x,y,z,yaw,pitch|spawn|overview] [--size WxH] [--skate|--bike] [--bench N] [--zone N]` renders without a window and exits. Claude may run this on its own to check visuals; it never opens a window.

Maps checked (counts pinned by `cargo test`, matched to HaloExport where it has numbers):

| map | file | state |
|---|---|---|
| Valhalla | `riverworld` | playtested: flying, skating, Master Chief |
| The Pit | `cyberdyne` | playtested: flying |
| Guardian | `guardian` | built, tests pass, not playtested |
| Last Resort | `zanzibar` | built, tests pass, not playtested |
| High Ground | `deadlock` | built, tests pass, not playtested; seam where the cove's water meets the sky's sea |
| Blackout | `lockout` | built, tests pass, not playtested |
| Sandbox | `sandbox` | written and pinned in tests, not built yet; Forge pieces have no collision |
| Halo (final mission) | `120_halo` | built with streaming, not playtested |
| Standoff, Avalanche, the rest | `bunkerworld`, `sidewinder`, ... | decode, not checked |

Run it:

```
cargo build
& "D:\GAME ISO AND ROMS\PC - AI MASHUP\halo-3-rust-rewrite\target\debug\halo3.exe" riverworld
& "D:\GAME ISO AND ROMS\PC - AI MASHUP\halo-3-rust-rewrite\target\debug\halo3.exe" 120_halo --zone 6 --music 120_music_15
```

## What is left

**Waiting on the user's playtest.** These are built but the user has not run them yet:

1. 120_halo: streaming and culling on the Warthog run, and skating across zone sets (`--zone 4`).
2. Sound on Valhalla and 120_halo: anything too loud, too quiet or broken.
3. The material, water and exposure changes made since the last look at Valhalla.
4. Guardian, Last Resort, High Ground and Blackout.

**Look, still different from real Halo 3:**

- Real cube-map reflections (now an SH stand-in) and parallax.
- Halo's exact specular and tonemap curve.
- Haze that varies with height; cliffs come out a little too warm.
- Water's painted masks; High Ground's seam between the cove and the sea.
- The rider is lit at one level for his whole body (no per-part lighting).

**Sound, still missing:** reverb from the BSP acoustics, dialogue, and the game's real mix levels.

**Campaign:**

- The other missions are untested.
- `010_jungle` BSP 11 fails with "segment has no page".
- Seen from far above the play space, the sky is only the trench haze colour.

**Not started (the big pieces):**

- Halo's own biped animations (`jmad`) for the on-foot Spartan, first-person arms, grenades,
  melee, damage and something to shoot at.
- Vehicles (the Warthog), and objects that move (machines, doors).
- AI, so campaign missions play rather than just load (HaloScript runs: see below; its AI calls are hooks).
- Reflex's real physics data read from the game files. The bike now uses numbers from the
  `D:\reflex-tools\physics` docs.

Suggested order: playtest 120_halo first, then either the on-foot Spartan (to make it feel like
Halo) or the look fixes above.

## Working rules for agents

- New work goes in a new worktree under `.claude/worktrees/`, on its own branch from master
  (CLAUDE.md). Each worktree needs a junction `.claude\worktrees\mw2-skate-3-src` pointing at the
  real `mw2-skate-3-src` folder so the skate crates' relative path resolves.
- Agents commit their own files by path once verified; never `git add -A` in a shared tree.
- The `bike-*` and `pause-menu` worktrees, and any locked agent worktree, belong to the user's
  other sessions; leave them alone.
- `.gitattributes` forces LF.

## Dirt bike (2026-10-28)

The bike now rides on MX vs ATV Unleashed's stock Pro 250 model (`docs/bike-unleashed.md`,
`crates/halo3_bike/src/unleashed.rs`), read from a decompile of the fan client's `mxacore.dll` and
the user's own Unleashed `.phy` files. The earlier physics are on `backup/bike-physics-master-2026-10-28`
and `backup/bike-physics-reflex-2026-10-28`. Awaiting the user's playtest; the doc lists what is
not ported (clutch boost, surfaces, tricks, Unleashed's wreck rules).

2026-10-29: the user called the Unleashed bike the best base so far and asked to tweak it back
towards Reflex. "Reflex v1" (`crates/halo3_bike/src/reflex.rs`) keeps Unleashed's simulation with
Reflex's numbers and steering, lean and grip rules, and is the default; the tuning menu (0) is
back, with Unleashed as a switch. Awaiting the user's playtest; the doc's "Reflex v1" section has
the details.

## HaloScript and the Warthog run (2026-10-29, branch `halo-script`)

The scenario's compiled scripts are read (`blam_script`, `docs/format/halo-script.md`):
`blam scripts 120_halo` prints all 731 scripts as Lisp-like source, `--functions` the 281
functions they call, `--grep TEXT` one topic. The game runs them (`halo3_script`) when asked:

```
& "<worktree>\out\bin\halo3.exe" 120_halo --insertion trench_run
```

runs `ins_trench_run` the way Bungie's designers did at the console: zone set
`120_04_100_110`, the player put at the run's start flag, `mission_halo` wakes the Warthog run
chapter. Driving into the run brings its floor plates in stretch by stretch; then rows of
plates drop away behind and beside you every `trench_speed` ticks and others blow up ahead,
exactly as the map's `trench_timer_*` / `trench_run_*_event_*` scripts say. Each dropped plate
loses its collision at once (bike and Spartan at once, the skater a second later when its
collision is rebuilt) and tumbles away. **Placed objects now collide** (their `coll` models):
the run's plates are its only floor, so before this the whole run had no collision.

Stubs and hooks: AI (`ai_place warthog01` etc.) goes to `AiHooks` (enemies agent), device
animation (the run's two sinking islands) to `DevicePoser` (animation agent) with a made-up
sink as the fallback, effects are stand-in flashes, cinematics / sound / HUD are no-ops that log
once. Without `--insertion` / `--scripts` nothing changes from before except object collision.
Tested headless (`halo3_script/tests/warthog_run.rs` drives the player through the run's
volumes and checks plates drop); awaiting the user's playtest.

## Open questions and known gaps

- **Object rotation order.** Yaw/pitch/roll are composed as Blam's intrinsic yaw, then pitch,
  then roll (`scenario::rotation`). Reclaimer and HaloExport compose the other way round; the two
  agree whenever only one angle is non-zero, which is almost every placement. Check one tilted
  crate in game before trusting either (`docs/format/scenario-and-tag-reading.md` 2.3).
- **Instance scale.** Applied as a uniform scale on top of the 3x4 matrix, as Reclaimer does.
  Every instance checked so far has scale 1.
- **Shader classification** in `blam_geometry` (`Material::shading`) is still by class code
  only; the viewer refines it from the shader options through `blam_texture` (alpha test,
  blend mode, HaloExport's "special" path rule). `Material::shader_tag` links the two.
- **Large collision BSPs** (32-bit indices) are implemented from the documented layout but no
  installed map uses them.
- **Spawn-point discs** and the other game-type markers are no longer drawn (2026-10-28,
  `world.rs` `is_editor_only`, `textures.rs` `is_game_type_marker`); `Scene::spawns` still has
  them all.
- The stated world bounds of some maps reach far above the geometry (Valhalla's Z goes to 1892 wu
  while the ground tops out at 91); the camera start uses the placed geometry instead.
- Review notes from the first session's adversarial review are in the final message of that
  session; anything not fixed there is listed in `README.md`.

## What already exists elsewhere on this PC

- **Halo 3 maps already play and skate inside IW4L** as stand-in maps
  (`mw2-skate-3-src\src\mashup-halo3-valhalla\crates\assets\src\halo3_map.rs`): Valhalla and The
  Pit via HaloExport glTFs. That is Halo maps inside the MW2 game, the model this project copies.
- **MCC install**: `C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection`,
  Halo 3 maps in `halo3\maps` (`riverworld` = Valhalla, `deadlock` = High Ground, `zanzibar` =
  Last Resort, `bunkerworld` = Standoff, `sidewinder` = Avalanche, `cyberdyne` = The Pit; campaign
  `005_intro` ... `130_epilogue`; `shared.map`, `campaign.map` hold shared resources). Build
  `Dec 21 2023 22:31:37`, Reclaimer's `MccHalo3U13`; the readers refuse other header layouts.
- **HaloExport** (`D:\reflex-tools\halo-tools\`): the C# exporter on Reclaimer that produced the
  reference numbers (`D:\reflex-tools\halo-exports\<map>\halo_export.json`).
- **Reclaimer clone** at `D:\reflex-tools\halo-tools\Reclaimer` (GPL-3.0, read-only reference).
- **H3EK** is not installed (free on Steam); its standalone `tag_test` may have a developer camera.

## Research results (2026-10-27)

| project | what it is | use to us |
|---|---|---|
| Project Reclaimer (projectreclaimer.dev) | fan launcher running MCC's `halo3.dll` with its own menus, 128 players, split screen | source is private; proves `halo3.dll` runs outside MCC |
| george0815/halo-3-recomp | static recompilation of the Xbox 360 `default.xex` into C++ | boots to menu; needs a 360 dump; no licence stated |
| ElDewrito + TagTool | Halo Online mod; TagTool (GPL-3.0) documents Gen3 tag layouts | TagTool's definitions were the source of the collision BSP facts |
| Assembly | tag editor with plugin XMLs (GPL-3.0) | MCC Halo 3 sbsp/zone/play plugin offsets cross-checked |
| Rust Halo 3 rewrite | none existed before this folder | this is it |

## User preferences

- Plain-terms explanations; not interested in the Minecraft part of the mashup.
- Plays and tests themselves. Do not launch the game or the viewer; hand over a PowerShell line
  with the path quoted and `&` in front.
