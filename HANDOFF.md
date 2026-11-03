# Halo 3 Rust rewrite: handoff

First written 2026-10-27 at the end of a research thread in `mw2-skate-3-src`; last
rewritten 2026-10-28 after the lighting, rider, campaign, streaming, water, sound and
multiplayer-map work; audited against master on 2026-10-30 (crate split, Halo 2 maps, the
Cortana bike, analog throttle, the unmerged branches). `README.md` has the layout and the per-feature status list; `docs/` has
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
| later | dirt bike game (MX vs ATV Reflex) in the mix | playable and being ridden by the user: B rides Reflex's Dakumoto 450F in the Cortana kit on "Reflex v1" physics (Unleashed's simulation with Reflex's numbers) with analog throttle (see "Dirt bike" below) |

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

A Cargo workspace (`cargo build`, `cargo test`). Crates (`README.md` "Layout" has one line each):

| crate | what it does |
|---|---|
| `blam_cache` | opens a `.map` read-only (memory-mapped): header, sections, tag index and names, string ids, a tag data cursor, the resource system (zone gestalt, play layout, deflate pages, shared caches) |
| `blam_geometry` | structure BSPs, render models, scenario objects, sky, the real collision BSP, lightmaps, atmosphere, camera fx (exposure), zone sets, trigger volumes and cluster portals, into one plain `Scene` |
| `blam_h1` | MCC Halo CE maps into the same `Scene`: BSP lightmap meshes, collision, gbxmodels, shaders, `bitmaps.map`, spawns, sky (`docs/format/halo1.md`) |
| `blam_h2` | MCC Halo 2 maps into the same `Scene`: inflated in memory, `shared.map` and `textures.dat`, BSP, collision, lightmaps, lifts, weapons, sound (`docs/format/halo2.md`) |
| `blam_reach` | MCC Halo: Reach maps into the same `Scene`: BSPs, instances, collision, objects, sky, spawns, lightmap texcoords (`docs/format/reach.md`); `blam_cache` and `blam_texture` read Reach's cache file, shaders and bitmaps |
| `blam_texture` | bitmap tags to mip chains (BC kept compressed, own CPU decoders) and the shader resolver |
| `blam_sound` | sound tags and MCC's FMOD bank (`halo3\fmod\pc\sfx.fsb`, Vorbis, setup headers rebuilt from the user's `halo3.dll`), decoded with lewton |
| `blam_cli` | the `blam` tool: tags, dumps, geometry and rails to OBJ, bitmaps to PNG, shaders, lightmaps, sound, scripts, animations |
| `blam_script` | the scenario's HaloScript: scripts, globals, expression nodes, the names they use; decompiled to source |
| `halo3_script` | runs a map's scripts in the game (threads, objects, devices, trigger volumes, insertions), with AI and animation hooks |
| `blam_anim` | animation graphs (`jmad`) and their codec data; poses sampled and blended for skinning |
| `reflex_assets` | reads the user's MX vs ATV Reflex install for the bike model (no bevy) |
| `halo3` | the game binary (bevy 0.20): the map menu, wires the runtime crates into an app, the pause menu, headless screenshots |
| `halo3_core`, `halo3_world`, `halo3_textures`, `halo3_audio`, `halo3_skate`, `halo3_rider`, `halo3_bike`, `halo3_foot`, `halo3_weapons`, `halo3_swim` | the runtime, split out of `halo3` on 2026-10-28 for faster rebuilds |

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
  - The Skate 3 crates are vendored in `third_party/skate-engine` (GPL-3.0, 2026-10-31; its README has their origin and our changes) and run on a worker thread (`skate.rs`).
  - Skate collision is the real collision BSP; grind rails come from IW4L's lip detector.
  - Kill volumes and the bottom of the world respawn you (`falls.rs`).
  - J or both sticks toggles skating.
- **Rider.** Master Chief (Mark VI helmet) is retargeted onto the 35 skate bones, with his head attached and feet flat, and is lit from the lightmap under him (`rider.rs`, `docs/rider.md`). Playtested: "looks great".
- **Dirt bike.** B rides it, with Master Chief on it (`halo3_bike`). It is drawn as Reflex's Dakumoto 450F in the user's Cortana graphics kit, read from their own MX vs ATV Reflex install (`reflex_assets`, `docs/format/reflex-bike.md`); without Reflex it is the old stick model. Its physics are MX vs ATV Unleashed's model pulled towards stock Reflex ("Reflex v1", `reflex.rs`); 0 while riding opens the physics tuning menu (`tune.rs`), which also switches back to plain Unleashed (see "Dirt bike" below).
- **On foot.** F walks the Spartan (`foot/`): the map's own movement numbers (`blam movement`,
  `docs/format/movement.md`), a capsule on the collision, a made-up gait on the skate skeleton,
  shoulder and eye cameras. He carries the map's weapons (`weapons/`, `blam weapons`,
  `docs/format/weapons.md`): placed where the scenario puts them, picked up, fired with the tags'
  rates, magazines, bursts, charge and zoom, projectiles traced against the collision. He starts
  with the sniper rifle on every map (`halo3_foot::STARTING_WEAPONS`). The user has walked,
  jumped, fired and reloaded in it. Open: the tag's jump looks high (4.5 m); no damage,
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
  - Music played by Halo 3's rules (intro, looping bars, alternate loop, outro, fades), started
    by the mission's scripts when they run, or by hand: M, or `--music` (the Warthog run on
    120_halo).
  - Board roll, grind and landing sounds.
  - K mutes; `-` / `=` set the volume (`docs/format/sound.md`).
- **Switching modes.** Jumping off the bike or the board lands you on foot with the ride's speed;
  getting back on from walking keeps it too.
- **Map menu (2026-10-31).** `halo3.exe` with no map opens it (`halo3/src/menu.rs`): Halo 3, Halo 2
  and Campaign tabs listing every map in the user's MCC install, with MCC's own preview pictures
  read from `Data\UI\texturepacks\levelstexturepack` at run time. Start Game decodes the map on a
  thread behind a Halo-ring loading screen and puts it into the same window. The screen stays
  up ("Preparing bike collision and skate") until the map's `halo3_core::map::Warmup` work is
  done: the bike / on-foot / enemy collision world (`halo3_bike::SharedWorld`, built once on a
  thread) and the first skate session; at most 180 s. Until then game time stands still and
  nothing plays (`halo3_core::map::Playing`, 2026-11-01): no scripts or dialogue, no controls, no AI,
  no run clock; the same on a restart. Every map is offered:
  a headless load of all 47 maps not checked by hand (Halo 3 multiplayer, campaign and Halo 2) drew
  each one with textures and lighting; they have not been walked for spawns, collision or water.
  Crow's Nest has no preview picture in MCC. A map named on the command line skips the menu.
- **Skating optional (2026-11-02).** Settings on the map menu has a Skating row (on by default,
  `SKATING=off` in `setup.env`, `halo3_core::skate::SkateEnabled`). Off, a map loads no grind
  rails, skate session or board (Valhalla ready in 0.8 s instead of 8.1 s), J does nothing on
  foot or flying, and the pause menu's Skate tab says it is off and to turn it on in Settings.
  Skate 3 is still required at first-run setup (the rider's and the Spartan's skeleton).
- **Pause menu.** Escape or Start. It has tabs for free roam, on foot, dirt bike (with the engine
  volume slider) and skate controls, and a Sound tab with the Master / Music / Effects / Ambience /
  Dialogue sliders. Q goes back to the map menu (unloading the map in place), or quits when the game
  was started on a map.
- **Window.** Opens borderless fullscreen.
- **Headless screenshots.** `halo3.exe <map> --shot <dir> [--view x,y,z,yaw,pitch|spawn|overview] [--size WxH] [--skate|--bike] [--bench N] [--zone N]` renders without a window and exits. Claude may run this on its own to check visuals; it never opens a window.

Maps checked (counts pinned by `cargo test`, matched to HaloExport where it has numbers):

| map | file | state |
|---|---|---|
| Valhalla | `riverworld` | playtested: flying, skating, Master Chief |
| The Pit | `cyberdyne` | playtested: flying |
| Guardian | `guardian` | built, tests pass, not playtested |
| Last Resort | `zanzibar` | built, tests pass, not playtested |
| High Ground | `deadlock` | built, tests pass, not playtested; seam where the cove's water meets the sky's sea |
| Blackout | `lockout` (run as `blackout`) | built, tests pass, not playtested |
| Narrows | `chill` | on master, counts pinned in tests, not playtested |
| Rat's Nest | `armory` | checked by headless shots, counts pinned, not playtested; crates and props have no collision |
| Sandtrap | `shrine` | checked by headless shots, counts pinned, not playtested; vehicles placed, but the Elephants have no collision |
| Isolation | `isolation` | on master, counts match HaloExport in tests, not playtested |
| Sandbox | `sandbox` | on master, counts pinned in tests, not playtested; Forge pieces have no collision |
| Halo (final mission) | `120_halo` | built with streaming, not playtested |
| Halo 2: Coagulation | `coagulation` (MCC Halo 2) | built, HaloExportH2 counts match, not playtested; no sky model yet |
| Halo 2: Lockout | `lockout` (MCC Halo 2) | built with lightmaps, fog, gravity lift, weapons, ambient sound; own counts pinned; not playtested |
| Halo CE: all 19 multiplayer maps | `h1:bloodgulch` ... (MCC `halo1\maps`) | built 2026-10-31, every map drawn in headless shots (textures, lightmaps, sky, spawns); HaloExportH1 counts match on 4 maps; bike, skate and foot stand on Blood Gulch; no weapons or sound yet; not playtested |
| Standoff, Avalanche, the rest | `bunkerworld`, `sidewinder`, ... | decode, not checked |
| Reach: all 20 multiplayer maps | `hr:20_sword_slayer`, `hr:cex_timberland`, ... | built 2026-10-31, counts pinned, each drawn headlessly; not playtested. Lightmaps read without their directions (per-vertex and probe-lit instances use the fallback), no fog, sound or weapons |

Narrows, Isolation and Sandbox were "written, not built yet" when their README entries were
made; they are on master now and build with the rest. This audit did not re-run their tests.

Run it (build from the checkout through `.dev\build-shared.ps1`, see CLAUDE.md, then play that
checkout's `out\bin` copy; `target\debug\halo3.exe` belongs to whichever checkout built last):

```
& "$(git rev-parse --path-format=absolute --git-common-dir)/../.dev/build-shared.ps1"
& ".\out\bin\halo3.exe" riverworld
& ".\out\bin\halo3.exe" 120_halo --zone 6 --music 120_music_15
```

**First-run setup (2026-10-29; in the game's own screen since 2026-11-02).** The first windowed
launch opens the map menu on its setup screen (`crates/halo3/src/menu/setup.rs`): Halo MCC, then
Skate 3 (the MW2 Skate 3 folder for its converter, then `.iso` or `default.xex`, converted there
with its progress shown), then MX vs ATV Reflex or Skip, each row a folder picker; no native
message boxes any more (`halo3_setup` returns the problems as text). It covers MCC, Skate 3
and MX vs ATV Reflex (MCC and Skate 3 required, Reflex optional since
2026-10-31: without it the bike is `stand_in.rs` and silent), converts Skate 3 once and saves everything in
`%LOCALAPPDATA%\Halo3Rewrite` (`crates/halo3_setup`, README "First run"). All game paths go
through `crates/game_paths`; no code names a folder on this PC any more. The Skate 3
converter is not shipped (2026-10-31): setup asks for the player's MW2 Skate 3 folder and runs
its `skate\iw4l-skate-convert.exe`. Still open for a public release: only MCC's
Halo 3 builds in `blam_cache::KNOWN_BUILDS` are read (setup says so when the build is newer).
The full-dump disc offsets (XGD2/XGD3 video partition) in `halo3_setup/src/xiso.rs` are from
the format, not checked on a real full dump; a trimmed image was.

## What is left

**The Warthog Run mission (user's list, 2026-10-31).** The user played `warthog_run` from start
to the hangar and it all worked. What is left to call it finished:

To do:

1. ~~**Falling pieces in the tunnels fall through the road.**~~ Done 2026-10-31; reworked
   on branch `girder-tumble` after the user's playtest (they plopped straight down): the
   girders now swing down on their hinges as the map's physics model says (see "Tunnel girders
   land on the road" below). Awaiting the user's playtest.
2. ~~**Dialogue.**~~ Done 2026-10-31 on branch `mission-dialogue`, awaiting the user's
   playtest: the scripts' lines play from MCC's language bank on the Dialogue bus (see
   "Dialogue" under the Warthog Run below). Not done: the radio / Cortana voice effects, ducking
   under lines, AI combat chatter.
3. ~~**Accurate explosion effects.**~~ Done 2026-10-31 on branch `explosion-effects`, awaiting
   the user's playtest: the scripts' effects and the plates' damage sections play the map's own
   `effe` tags (particles, contrails, light flashes, sounds; see "Explosion effects" under the
   Warthog Run below). Not done: debris objects (the plate chunks), camera shake, screen
   effects, decals.
4. ~~**Enemies must attack.**~~ Done 2026-11-03 on branch `enemies-attack-rider`, awaiting the
   user's playtest: the Flood and Sentinels see, chase, shoot and hit the rider on the bike and
   the skater as on foot, he dies and comes back on the bike, and the bike splatters them (see
   "Enemies attack the rider" under the Warthog Run below).

Skipped by the user's choice (do not do unless asked):

- The Warthog and the Mongoose (vehicles, and the `warthog01..04`, `chopper*`, `secret_egg*`
  crews). The dirt bike stays the ride.
- The final cutscene (`cin_halo_outro`): the mission still ends on MISSION COMPLETE at the hangar.
- The Arbiter driving or riding with the player (`cs_*` command scripts, `ai_get_object`); he is
  placed and nothing more.

Order: the user approved 1, 2, 3, 4 as the priority order (2026-10-31).

**Waiting on the user's playtest.** These are built but the user has not run them yet:

1. 120_halo: skating across zone sets (`--zone 4`). (Streaming on the Warthog run: played
   through 2026-10-31, fine.)
2. Sound on Valhalla and 120_halo: anything too loud, too quiet or broken.
3. The material, water and exposure changes made since the last look at Valhalla.
4. Guardian, Last Resort, High Ground, Blackout, Narrows, Rat's Nest, Sandtrap, Isolation,
   Sandbox, and Halo 2's Coagulation and Lockout.

**Look, still different from real Halo 3:**

- Real cube-map reflections (now an SH stand-in) and parallax.
- Halo's exact specular and tonemap curve.
- Haze that varies with height; cliffs come out a little too warm.
- Water's painted masks; High Ground's seam between the cove and the sea.
- The rider is lit at one level for his whole body (no per-part lighting).

**Sound, still missing:** reverb from the BSP acoustics, the dialogue's voice effects and
ducking, AI combat chatter, and the game's real mix levels.

**Dirt bike engine sound** (merged 2026-10-30, user-approved): Reflex's own 450F recordings
read from the user's install (`reflex_assets::engine_audio`, `docs/format/reflex-engine-audio.md`),
played as Reflex plays them (whole takes from the event file's pools at their own pitch, a new
one at each shift), H / left stick click turns it off; silent without Reflex (a synth was
tried and dropped 2026-10-31); engine volume slider on the pause
menu's Dirt bike tab (`bike-sound.json`). Still a guess: which pool plays in which gear.

**Campaign:**

- The other missions are untested.
- `010_jungle` BSP 11 fails with "segment has no page".
- Seen from far above the play space, the sky is only the trench haze colour.

**Merged to master 2026-10-31** (was on branches; kept here for where things live):

- `halo-script` also merges `biped-anim`, `enemies-ai`, `mission-music` and `music-volume`
  into one build for 120_halo.
- `biped-anim`: reads `jmad` animation graphs (`blam_anim`, `blam anim`) and plays them
  (`halo3_anim`, `halo3 --anim-test`). Not yet driving the on-foot Spartan.
- `enemies-ai`: reads the scenario's AI (squads, cells, starting locations, characters) and puts
  120_halo's squads in as actors that see, move, fight, hurt and die.
- `halo-script`: reads HaloScript (`blam_script`, `blam scripts`) and runs the map's own scripts
  (`halo3_script`); object collision models and damage sections; the Warthog run's floor
  collapses.
- `first-run-setup`: the one-time MCC / Skate 3 / Reflex setup (`game_paths`, `halo3_setup`);
  Reflex optional, with the stand-in bike.

**Not started:**

- First-person arms, thrown grenades, melee.
- Vehicles (the Warthog), and machines and doors that move.
- Reflex's real physics data read from the game files. The bike now uses numbers from the
  `reflex-tools\physics` docs.

Suggested order: playtest 120_halo first, then either the on-foot Spartan (to make it feel like
Halo) or the look fixes above.

## Working rules for agents

- New work goes in a new worktree under `.claude/worktrees/`, on its own branch from master
  (CLAUDE.md). The skate crates are in the tree (`third_party/skate-engine`), so a worktree
  needs nothing outside it.
- Agents commit their own files by path once verified; never `git add -A` in a shared tree.
- Worktrees you did not create in this thread, and any locked agent worktree, belong to the
  user's other sessions; leave them alone.
- `.gitattributes` forces LF.
- **The map is a phase of the app** (`halo3_core/src/map.rs`), so the menu can load and unload maps
  in one window. In a map crate: start-up systems go in `halo3_core::map::MapStartup` (not
  `Startup`); every `Update` / `PreUpdate` / `PostUpdate` system gets `.run_if(halo3_core::map::ready)`;
  per-map resources use `MapAppExt`: `init_map_resource` (reset on unload) instead of
  `init_resource`, and `remove_on_map_unload` for resources a system inserts later. `main.rs` is
  `load` (decode, no bevy), `install` (resources into the world) and `run`; whatever `install`
  inserts must be listed for removal in `MenuPlugin::build`. Anything left out carries over into the
  next map.

## Dirt bike (2026-10-28)

The bike now rides on MX vs ATV Unleashed's stock Pro 250 model (`docs/bike-unleashed.md`,
`crates/halo3_bike/src/unleashed.rs`), read from a decompile of the fan client's `mxacore.dll` and
the user's own Unleashed `.phy` files. The earlier physics are on `backup/bike-physics-master-2026-10-28`
and `backup/bike-physics-reflex-2026-10-28`. Playtested (see below); the doc lists what is
not ported (clutch boost, surfaces, tricks, Unleashed's wreck rules).

2026-10-29: the user called the Unleashed bike the best base so far and asked to tweak it back
towards Reflex. "Reflex v1" (`crates/halo3_bike/src/reflex.rs`) keeps Unleashed's simulation with
Reflex's numbers and steering, lean and grip rules, and is the default; the tuning menu (0) is
back, with Unleashed as a switch. The user has ridden it since (the throttle work below); the
doc's "Reflex v1" section has the details.

2026-10-29 (later): the stick bike is replaced by Reflex's Dakumoto 450F with the Cortana kit
(the user's own design, in their `reflex-tools` folder), read at start-up
from the user's Reflex install; its forks, swingarm and wheels follow the physics. Reflex v1 took
the 450F's shape: 1.46 m wheelbase (was 1.57), 0.345 / 0.335 m tyres (0.366), 29.2° rake (26.9°),
the model's grips, pegs and probe spheres (`docs/format/reflex-bike.md`). Reflex becomes a third,
optional game for the public release: the first-run setup should find and save it like MCC and
Skate 3 (`reflex_assets::find_reflex` is the stand-in until that lands). Merged 2026-10-30.

2026-10-29/30, also on master: five saved setup slots in the tuning menu; Skate 3's session
marker on the bike, with a small red flag on a pole where it is set; analog throttle (the amount
held reaches the clutch, gearbox, wheelie hold and air brake, on a squared curve; pads that send
RT as on/off are read from the Z axis). The user has been riding it: the throttle work came from
their playtests, and RT reading 0 or 1 turned out to be their Elite Series 2's trigger locks, not
the game.

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
sink as the fallback, effects go to `halo3_effects` (see "Explosion effects"), cinematics / HUD are no-ops that log
once (sound is played: see "Mission music"). Without `--insertion` / `--scripts` nothing changes from before except object collision.
Tested headless (`halo3_script/tests/warthog_run.rs` drives the player through the run's
volumes and checks plates drop); awaiting the user's playtest.

First playtest (2026-10-29): B, J and F started back at the crash site and walking was stuck.
Cause: the run's start flag lies inside the kill volume `kill_100_01`, which the game's scripts
switch off when the zone set loads, but our kill volumes were always on, so every rider "fell"
at once and went back to the map's old start (outside the loaded collision: a fall loop).
Fixed: kill volumes follow `kill_volume_enable` / `_disable` (`halo3_core::falls`), the
insertion runs 15 ticks before the first frame, a script teleport becomes the fallback spot,
and the run's plate folders are no longer dropped for memory (`object_destroy_folder` only
removes script-created objects), so the floor is there wherever you start. Checked headless
with `HALO3_SCRIPT_TRACE=1` (player position once a second) for the bike, on foot
(`HALO3_FOOT_WALK=0,1`) and skating, at the start flag and dropped straight onto the plates.
## Animation (2026-10-29, branch `biped-anim`)

Halo 3's own animations read from the maps: `blam_anim` (reader), `halo3_anim` (playback),
`blam anim` (CLI), `docs/format/animation.md` (layout, byte checks), probe
`docs/format/scripts/probe_animation.py`. Every codec found in the installed maps decodes
(static, every-frame with i16 or f32 rotations, byte and word keyframes); 120_halo decodes 5881
animations, the other 115 have no data shipped in that map. Checked by `cargo test -p blam_anim`
and by headless shots (`--anim-test`): the flood combat form walking and swinging, the flood
elite, Master Chief running with a rifle and a sentinel aiming all hold together. Not playtested;
nothing in the game uses it yet except `--anim-test`.

The API for the enemies, script and on-foot code (`crates/halo3_anim/src/lib.rs`):

```rust
// Once per object type, from the map (bipd, mach, vehi, hlmt or jmad tag):
let model: Arc<AnimModel> = AnimModel::load(cache, tag)?;          // or load_path(cache, path)
// The skinned mesh: joints in render model node order, inverse binds from the model.
let joints = halo3_anim::spawn_joints(&mut commands, &model);      // AnimJoints
let binds = model.inverse_bindposes();                             // SkinnedMeshInverseBindposes
// The actor entity (Transform = where it stands, renderer frame) gets:
let mut a = Animator::new(model.clone()).with_seed(n);
a.set_stance("combat", "rifle", "ar");     // Halo's mode / weapon class / weapon type
a.play("move_front");                      // loops, cross-fades 0.2 s, falls back to any/any
a.play_once("melee"); a.finished();        // one-shots
a.overlay("fire_1", false);                // overlays on top
a.aim(yaw, pitch, moving);                 // aim grid, radians, left / up positive
a.take_root_motion();                      // dx, dy, dz (wu, own frame), dyaw since last call
a.set_position(0.0..=1.0);                 // device machines: device:position
a.play_named("fall", false);               // any animation by full name
commands.entity(actor).insert((a, joints));
app.add_plugins(halo3_anim::AnimPlugin);   // advances and poses joints in PostUpdate
```

Without bevy, `Animator::model_matrices()` gives one model-space matrix per node (Halo model
frame; multiply by `halo3_anim::halo_to_render()` and the actor's transform). 120_halo's
machines with graphs all use `device:position` (doors, the `plates\island_test` and
`plates\pillar_fall` pieces, terminals, the monitor); the island plates also carry `fall`,
`tilt`, `tilt_up`, `other_fall`, `other_tilt` and `device:power`.

Master Chief instead of the made-up gait (not done): `halo3_foot` builds a skate-skeleton pose
that `halo3_rider` retargets. The jmad route would skip both: load
`objects\characters\masterchief\masterchief` (campaign maps; on Valhalla `mp_masterchief` uses
the same graph, 952 animations, and all 51 nodes match), draw him skinned the way `crates/halo3/src/anim_test.rs` does,
and drive an `Animator` from the foot state: stance `combat:rifle:<weapon type>` (or
`crouch:...`), `play("move_front" | "move_back" | "move_left" | "move_right" | "idle" |
"turn_left" | "airborne" | "land_soft")`, `aim(0, pitch, moving)`, `overlay("fire_1")`. Its
run is 2.3 wu/s of root motion, so `set_speed(ground speed / 2.3)` keeps the feet from sliding.

Open: overlay rotation order and the aim grid's up/down are inferred, not proven; frame events
(footstep sounds), IK and transitions are not read; Halo's full fallback rules are approximated.
## Enemies and AI (2026-10-29, worktree `enemies-ai`)

`crates/halo3_ai`, reader `blam_geometry::ai`, format `docs/format/ai.md`, CLI `blam ai <map>
[--squads] [--zones] [--characters]`, tests `crates/blam_geometry/tests/ai.rs`. Built and checked
with headless shots; not playtested.

- **What 120_halo has**: 262 squads in 19 groups, 15 characters: the Flood (elite, human and brute
  combat forms, infection forms, carriers, stalker and tank pure forms, the ranged pure form),
  sentinel aggressors, 343 Guilty Spark (`monitor`, `monitor_combat`), Johnson, the Arbiter. Zone
  sets 0-1 the ziggurat, 2 the control room, 3 the way out, 4-7 the Warthog run (`blam ai 120_halo`).
- **Placement until scripts run** (`AutoPlace`, `--ai nearby|zone|off|none`): default `nearby`, a
  squad appears the first time you come within 70 m of it, at most 40 living. Squads go when their
  BSPs unload.
- **Works**: real models (skinned, bind pose), weapons in hand, sight/hearing from the character
  tags, LOS on the collision, walkers on the Spartan's capsule code, flyers hover, Flood charge and
  melee with the biped's melee damage, ranged actors keep the character's combat range (zone firing
  positions) and fire bursts from its firing patterns, rounds hurt actors and the player both ways
  (damage effect numbers), shields before body, death (fall, sink, vanish; infection forms pop), the
  player's shields (70) and body (45) from his model, recharge, red flash, death and respawn at the
  last quiet checkpoint, HUD bars. Teams from the unit tags; Flood and Sentinels hostile to all,
  Guilty Spark allied until a script says otherwise.
- **Stubbed / own numbers**: run speeds (Halo uses animation root motion), head-shot double damage,
  explosion damage 120 for rockets / grenades, no pathfinding graph (straight lines + firing
  positions + a hop when stuck), no vehicles (Warthog / chopper crews skipped), no grenades, no
  carrier spawns, no infection reanimating corpses. (Until 2026-11-03 the AI only fought while you
  were on foot; it now fights you on the bike and the board too, see "Enemies attack the rider".)

**For the script agent** (all in `halo3_core::ai`, no dependency on `halo3_ai` needed):
`MessageWriter<AiCommand>` with `Place { who: AiRef::named("sentinels"), count: None }` (`ai_place`;
squad or group names, `squad/cell` takes the squad), `Erase` (`ai_erase`), `Kill` (`ai_kill`),
`Allegiance { a, b, allied }` (`ai_allegiance`, teams in `halo3_core::ai::team`). Read
`Res<AiCensus>` for `ai_living_count` / `ai_spawn_count` (squads and groups by name). When scripts
start, set `ResMut<AutoPlace>` to `AutoPlace::Off`. `Res<PlayerVitality>` has the player's state.

Adapter for `halo3_script` (branch `halo-script`, whose `AiHooks::call` is synchronous with no ECS
access): take a clone of the `halo3_core::ai::AiBridge` resource (the AI plugin inits it; it is an
`Arc<Mutex<..>>`) into a struct implementing `AiHooks`, and map `ai_place` ->
`bridge.send(AiCommand::Place { who: AiRef::named(name), count })`, `ai_erase` / `ai_kill` likewise,
`ai_allegiance` / `ai_allegiance_remove` -> `Allegiance`, `ai_living_count` ->
`bridge.living_count(Some(name))`, `ai_spawn_count` -> `spawn_count`; return `None` for the rest.
Commands go out the next frame; the census comes back every frame. `squad/location`
(`arbiter_trench/trench`) places one actor at that named starting location; any other suffix
(`warthog01/driver` when no location matches) places the whole squad. Known gap for the Warthog run:
`warthog01..04`, `chopper*` and `secret_egg*` (a Mongoose) are vehicle crews with no character, so
they place nobody until vehicles exist; `arbiter_trench/trench` places the Arbiter.

**For the animation agent**: each actor root has `ActorSkeleton` (joint entities in node order, local
`Transform` = node relative to parent in Halo model units, bind pose in `bind`, `animations` = the
unit's `jmad` tag index) and `ActorActivity` (state Idle / Alert / Moving / Firing / Melee / Dying /
Dead, Halo-frame velocity, yaw, aim, time in state). Write joint transforms and the skinned meshes
follow; set `posed_externally` once you do. The dead tip over by their root transform.

Adapter for `halo3_anim` (branch `biped-anim`) once both are on master: its `pose_joints` writes
joint `GlobalTransform`s as `owner * halo_to_render() * node` after propagation, but an actor's root
already carries the Halo axes and world-unit scale (`place::root_transform`). So at spawn, give the
root a child with `Transform::from_matrix(halo_to_render().inverse())` holding
`(Animator::new(AnimModel::load(cache, unit_tag)).with_seed(n), AnimJoints(skeleton.joints.clone()))`
(the joints are already in render-model node order), set `posed_externally = true`, and each frame
map `ActorActivity` to `set_stance` / `play` / `play_once("melee")` / `aim`; `take_root_motion()`
can replace `ActorKind::speed`. Weapons hang off the joint entities' propagated (bind) frames, so
they would need re-parenting to the posed hand (one `GlobalTransform` write after `AnimSystems`).

## Scripts, AI and animation connected (2026-10-30, branch `halo-script`)

- **Scripts -> AI**: `halo3_script::BridgeAi` (`ai_bridge.rs`) implements `AiHooks` over
  `halo3_core::ai::AiBridge`: `ai_place` (with count), `ai_erase`, `ai_kill`, `ai_allegiance(_remove)`
  (teams by name or enum index), `ai_living_count`, `ai_spawn_count`; `None` for the rest. `main`
  installs it whenever scripts and the AI both run, and sets `AutoPlace::Off` while scripts run.
  `--insertion trench_run` places `arbiter_trench/trench` (the Arbiter) at once, then the run's
  Flood and Sentinels as the player passes their triggers (`trench_infection_100*`,
  `trench_sentinel_100_*`, `trench_carrier_100_01`, `trench_combat_forms01..04`, ...). Vehicle crews
  (`warthog01`, `chopper*`, `secret_egg*`) have no character and place nobody (`ai: placed warthog01
  (0 actors)`). Without `--insertion` / `--scripts` the free-roam `AutoPlace` works as before.
- **Animation -> actors** (`halo3_ai/src/animate.rs`): every actor whose unit has a graph with the
  drawn skeleton (all 15 characters on 120_halo) gets an `Animator`. Instead of the child entity +
  `AnimJoints` sketched above, the pose's local node transforms are written into the
  `ActorSkeleton` joints' own `Transform`s (same frame as a `jmad` frame), so the skinned meshes,
  the gun on the right hand and the head hit sphere follow through normal propagation; nothing is
  re-parented. Stance: `combat` with the class/type the graph has for the actor's weapon (`rifle/ar`
  becomes `pistol/ar` for Flood combat forms, `any` for the Flood elite, `unarmed` without a gun);
  `idle`, `move_front/back/left/right` by velocity, played at actor speed / root-motion speed so the
  feet do not slide; `play_once("melee")` on a swing; `h_kill` / `s_kill` / `landing_dead` deaths
  (the AI then no longer tips the body over); the aim overlay towards `ActorActivity::aim`;
  `fire_1` every 0.25 s while firing. Walkers' run speed (`ActorKind::speed`) is now their
  `move_front` root-motion speed. Sentinels and the monitor hover with `idle` + aim.
- **Animation -> devices** (`halo3_anim/src/devices.rs`): `MachinePoser` implements `DevicePoser`
  for every scripted `mach`: the root node posed by the scripts' position track (`other_fall`,
  `fall`, else `device:position`) with the overlay track (`tilt_up`, `tilt`, `other_tilt`) on top,
  both relative to their first frames, so position 0 leaves the object where the scenario put it.
  A falling device keeps its collision until its root node has dropped 1.5 wu (`DEVICE_DROP`); until
  then its collision moves with it (see "Warthog run collision fixes" below). The made-up sink is now only
  the fallback for machines without a graph.
- Checked: `cargo test -p halo3_script` (bridge), `-p halo3_anim` (`tests/islands.rs`: 100_island01
  tilts while solid, falls 50+ wu and stops being solid), headless `--insertion trench_run` shots
  walking the camera through the run's triggers (squads placed, posed Flood and Sentinels, the
  island tipping mid-fall) and a free-roam `--foot` shot at the crash site. Not playtested.
- Not done: vehicles (so the Warthog crews), `ai_get_object` / `object_get_ai` (scripts get no
  actor objects), command scripts (`cs_*`), turning and evade animations, frame events.

## Warthog run collision fixes (2026-10-30, branch `halo-script`)

Two bugs from the first bike playtest of `--insertion trench_run`:

- **Dropped through the first island while riding round it.** Its collision was static at the
  scenario's pose while the drawn island moved: on `100_island_trig01` the scripts play the
  `tilt_up` overlay, which lifts `100_island01` about 2.7 wu (8 m) and tips it 2 degrees in the
  first second, so the bike ended up inside the drawn island. Nothing removed its collision early
  (it falls only on `trench_run_100_trig07`, north of the island, or after 40 s, as in the game),
  its collision covers the whole top, and the kill volume over it (`kill_100_02a`) only comes on
  at `trench_run_100_trig08`. Fix: solid objects a script moves publish their motion
  (`halo3_core::objects::set_object_pose`, from `halo3_script::publish_collision`); the bike's
  collision world (also the Spartan's, the AI's and the guns') looks their triangles up where
  they are now, and the skater's collision is rebuilt with them a second after they settle. The
  lurch is too fast for the bike's springs (about 2.5 wu/s up), so the bike and the Spartan are
  also carried by what they stand on when it moves (`World::moving_ground`). All four islands
  (`tilt_up`, `tilt`, `other_tilt`) get this. The lift itself is Bungie's animation as read
  (frames 0-2 of `tilt_up` rise 0.96 -> 3.70 wu, then hold); not compared against the real game.
- **Invisible wall halfway through the first tunnel.** Not a missed trigger: BSPs 100 and 110
  meet at y = 37.5 in the tunnel and each closes its collision there with a flat cap (one facing
  each way). The game walks through BSP seams; we collided with BSP 100's cap. Fix:
  `Collision::of_bsps` drops pairs of BSP triangles with the same corners facing opposite ways
  from two loaded BSPs (`drop_seams`); with only one of the two loaded its cap stays. This opens
  every seam between loaded BSPs, on every map. Also in that tunnel: a `guard_rail` crate
  (folder `trench_run_100_crates`) lies across the right half at y = 21; it is drawn and solid
  here, where the game's Warthog would push the crate aside.
- Checked: `halo3_anim/tests/warthog_run_collision.rs` (scripts headless, the bike's own world:
  95 points round island 1 within 0.07 wu of the drawn island before, during and after the tilt
  and 20 s riding round; a bike standing on it stays 0.22 wu above it through the lurch; floor
  and no wall at 1 m steps through the first tunnel at x 52 and 58.7; it fails on the seam
  without the fix), `blam_geometry/tests/objects.rs` (`seam_between_loaded_bsps_is_open`),
  headless `--bike` runs with `HALO3_SCRIPT_TRACE=1`: through the tunnel the bike crosses
  y = 37.5 at 100 km/h; onto island 1 it is lifted with the island (z -28.2 -> -23.5) where
  before the carrying it fell through. Not playtested.

## Tunnel girders land on the road (2026-10-31, branches `tunnel-fall-collision`, `girder-tumble`)

User playtest: the giant pieces falling in the tunnels went straight through the road. They are
the run's `tunnel_girder` crates (`objects\levels\solo\120_halo\plates\tunnel_girder`, a `bloc`
with a `phmo`, 22.5 wu long, hung across the ceiling). The first fix (`tunnel-fall-collision`)
dropped them straight onto the road. Second playtest: they "plop right on the drivable area",
no rotation, no bounce, and maybe more fall than should.

**What the map says** (`docs/format/halo-script.md` 8, `probe_girder.py`): the scripts never push
or spin them (no velocity or impulse calls); `fall_off` damages section `fall`, whose response
*loosens* the physics model's one constraint, a hinge at the girder's end against the ceiling
(pivot model (-10.75, 0, 2.513), cone 45, plane 30 degrees); `destroy` *breaks* it and does not
remove the object (we used to remove it). Friction 0.8, restitution 0.35, mass 71213, all from
the phmo.

**What the scripts drop, and when** (`trench_tunnel01` / `02`, all `fall_off` unless noted):

| trigger | girder |
|---|---|
| `trench_run_game_save01` (after `trench_run_100_trig08`) | `100_hw_ceiling02` |
| `tunnel01_trig01` | `100_hw_ceiling04` |
| `tunnel01_trig03` | `100_hw_ceiling08`, a second later `100_hw_ceiling09` |
| `120_08_trench_encounter_110` | `100_hw_ceiling10` (`tunnel_fall_away`: `fall` then `destroy`) |
| `tunnel01_trig04`, `tunnel01_trig05` | `100_hw_ceiling13`, `100_hw_ceiling14` |
| `trench_run_game_save03` (after `trench_run_110_trig15`) | `110_hw_ceiling02` |
| `tunnel02_trig02` / `04` | `110_hw_ceiling05` / `08` |
| `tunnel02_trig05` | `110_hw_ceiling10`, `110_hw_ceiling11` |
| `tunnel02_trig07` | `110_hw_ceiling14` |

`110_hw_ceiling01`, `06` and `12` are never dropped and stay up. Master dropped the same twelve
(each tumbled away and vanished after 6 s; `100_hw_ceiling10` vanished at once); the first fix
dropped the same twelve but kept them, so more were in sight at once. Nothing drops extra:
damage goes only to the objects a call names, and none comes down early.

**Now** (`halo3_script/src/crate_physics.rs`, `objects.rs`): a loosened girder swings down about
its hinge under gravity (sub-stepped pendulum, inertia from its collision's corners), bouncing
off the hinge's 30 degree limit or the road with the phmo's restitution, and comes to rest
hanging (about 6 s). `100_hw_ceiling10`, its hinge broken, falls free as a rigid body (gravity,
corner contacts against the BSP and solid objects with the phmo's friction and restitution,
sleeps when slow, wakes if its floor drops) and rolls onto its side. Crates without a
constraint fall free the same way, with a small sideways push and spin of our own. Solid
throughout; while moving their pose is published every tick, so the bike's collision follows
them. The plates keep their old tumble. Ours, not the map's: the corners as contact points, the
damping, the sleep thresholds, the free fall's push, and the reading of "loosen" (turns within
its limits); not compared with the real game.

What it looks like: in the first tunnel the girders hang from one wall down to 1 to 2 wu over
the road (`100_hw_ceiling14` reaches it); in the second, where the road lies lower, they hang 8
to 16 wu up. `HALO3_SCRIPT_TRACE=1` logs every object a script damages.

Checked: `halo3_anim/tests/tunnel_falls.rs` (scripts headless through both tunnels: exactly the
scripts' girders move, `100_hw_ceiling10` falls free, every one turns more than 5 degrees, rests
within 15 s, solid, no corner in the floor, at its limit or on the road, the bike's world meets
its top where it lies), the `crate_physics` unit tests (a beam swings to its limit; a dropped
beam bounces and sleeps), all `halo3_anim`, `halo3_script`, `halo3_effects` tests. Shots:
`out\shots\girder_swing\` in the `girder-tumble` worktree. Awaiting the user's playtest.

## Mission music (2026-10-30, branch `mission-music`)

The `lsnd` tags are now read in full (`docs/format/sound.md` 5 and 8, probe
`probe_lsnd.py`, CLI `blam sound 120_halo music`): music time, "not a loop" and "stops music"
flags, the tracks' fade-in / fade-out / cross-fade-alternate flags and cross-fade time. The
scripts' `sound_looping_start` / `_stop` / `_stop_immediately` / `_set_alternate`,
`sound_impulse_start` / `_stop` and `sound_class_set_gain` (music and amb) reach the sound
through a queue in `halo3_core::script_sound` (the VM's only change: those functions, which used
to log "not implemented"). `halo3_audio/src/music.rs` plays the pieces: intro, random loop bars
back to back, the alternate on a bar boundary through its transition or by cross-fade, the
outro / alternate outro / fade on stop, music time, stops-music. On the Warthog run
(`--insertion trench_run`) `120_music_15` starts on the first stretch, goes to its alternate on
leaving the second island and plays its 77 s finale at the last stretch
(`halo3_script/tests/trench_music.rs`). M / Shift+M / N / `--music` still work on top. Without
scripts nothing changes. Not done: dialogue (language banks), `lsnd` layers, the music class's
mix level, cinematic music. Awaiting the user's playtest.

## Music volume and the Sound tab (2026-10-30, branch `music-volume`)

User playtest: the run's music was buried under the effects. Cause: Halo 3 mixes by sound class
(`sncl + 0x44/0x48`, a gain range in dB, `docs/format/sound.md` 9, probe
`probe_sound_classes.py`): music 0 dB, dialogue -3, weapons, ambience, sound scenery and most
effects -12 to -15. We were not reading it, so effects and ambience played 9 to 12 dB too loud
(the falling-plate loops by the run about 8 dB over the music). Now every sound gets its class
gain (`SoundInfo::gain_db`); sound scenery a further -9 dB, weapons +6 dB (our choices,
commented). Headless on the run: music -18 dBFS, ambience next to two plates -25 (was about
-11). The mixer has buses (music, ambience, effects, dialogue) with per-bus volumes and level
meters; `HALO3_AUDIO_TRACE=1` logs `audio levels` every 5 s. The pause menu has a fifth tab,
**Sound**: Master, Music, Effects, Ambience, Dialogue sliders, 0-200 % (100 % = the game's
mix), keyboard / pad / mouse, saved to `sound-levels.json` next to the exe
(`halo3_audio/src/levels.rs`); `-` / `=` / K drive the same Master value. A slider multiplies
on top of the scripts' `sound_class_set_gain` (music, amb). Awaiting the user's playtest.

## The frigate, the end of the run, and `trench_end` (2026-10-30, branch `halo-script`)

User playtest: the Forward Unto Dawn hung broadside across the end of the run. It is
`trench_frigate`, a device machine (`objects\cinematics\human\frigate\frigate`), placed by the
scenario (yaw 180, pitch 2.45 degrees); no script moves it. Two causes, both fixed from the map
data:

- **Machines stand in their animation.** The frigate's model lies nose along +Y; frame 0 of its
  `device:position` track turns the root a quarter turn. The game never shows a machine
  unanimated, so every machine with a graph now stands in its `device:position` pose at
  position 0, laid over the rest pose as the clip's kind says (an overlay adds; `halo3_anim::devices::place_machines`, called in `main` before the
  collision is built; `docs/format/animation.md` 10.3). That turns the open hangar (under the
  stern, 13 wu wide, 8 high, 34 deep) to face the run. The islands move by about 0.3 wu with it. (Read as whole values instead, the islands sank 56 wu out of sight; the user saw them gone. `tests/machines_at_rest.rs` now pins every machine within 1 wu of its placement.)
- **Rotation order.** Composed about the world's axes as Reclaimer does (`scenario::rotation`);
  the other order sank the stern into the run's last ramp (`docs/format/scenario-and-tag-reading.md` 2.3).

Now: the run's last ramp ends at x 370.7, z -71.3; the hangar mouth is 29.7 wu on at z -76.5,
the stern 4.9 wu over the lip; the hangar's collision matches the drawn bay (floor, walls; open
mouth). Its back wall is drawn but not in the collision (open into the hull).

**The end.** `mission_halo` waits for a player in `end_trilogy01` (the hangar mouth, floor to
ceiling) or one of the map's hidden small vehicles in `end_trilogy03` (over the gap), then wakes
`cin_halo_outro`, which calls `game_award_level_complete_achievements`. The VM marks the mission
won on that (or `game_won` / `end_mission`) and stops the thread there (no cutscene teleports, the
frigate stays); the scripts stop. The dirt bike counts as the Mongoose (`secret_egg/mongoose`,
`vm::PLAYER_RIDE_STANDS_IN_FOR`): headless at full throttle from `trench_end` it reaches 125 km/h,
flies through `end_trilogy03` and only grazes `end_trilogy01`'s bottom corner, then drops just
under the hangar's floor. `halo3/src/finish.rs` stops play and shows MISSION COMPLETE with the
time (game time from the first frame the player is in control after loading, so pauses do not
count and deaths do not reset it), Restart run or Quit; styled like the pause menu. Since the
master merge (2026-10-31) Restart unloads the map and loads it again in the same window (the map
menu's loading screen, or a plain RESTARTING screen when started from the command line); Quit
goes back to the map menu, or closes the game when it was started on a map.

**The Warthog Run mission (2026-10-31).** `main.rs` `MISSIONS`: `warthog_run` is 120_halo with
`--insertion trench_run --zone 120_04_100_110`, and only zone sets `120_04_100_110`,
`120_100_110`, `120_110_120`: only their BSPs (040, 100, 110, 120: indices 4, 7, 8, 9) are
decoded, and `Zones::allowed` refuses any other zone set (trigger volumes, scripts, Page keys).
It is a tile on the menu's Campaign tab ("The Warthog Run", MCC's Halo picture) and starts with
`halo3.exe warthog_run`. Objects of the other BSPs are still decoded (never uploaded).

**Starts.** A named insertion now starts on the dirt bike (`--foot`: on foot) where it puts the
player. `trench_run`: the flag `trench_run_player0`, on the floor; the flag has no facing set
(+X, into the side of the canyon the run starts in), so the start faces up the canyon to the run's
first trigger, `120_08_trench_encounter_100` (`setup::FACE_TOWARDS`). `trench_end` (ours,
`halo3_script::setup::insertion`): `ins_trench_run`, then zone set `120_110_120` and the run's
last waypoint `trench_run_nav08` (75 wu before the ramp), facing `trench_run_nav09` in the
hangar mouth; `object_management` then switches the run's kill volumes off and makes the frigate's
lift shafts. Tests: `halo3_anim/tests/frigate.rs`.

```
& "<worktree>\out\bin\halo3.exe" 120_halo --insertion trench_end
```

**Dialogue (2026-10-31, branch `mission-dialogue`, awaiting the user's playtest).** The
scripts' lines play: MCC keeps them in `halo3\fmod\pc\<language>.fsb`, one bank per language,
laid out like `sfx.fsb` (same Vorbis setups), and the dialogue `snd!` tags store their bank ids,
so `blam_sound::Library::add_dialog` opens the bank beside `sfx.fsb` and every dialogue tag
decodes. The scripts name lines, not tags: the map's mission dialogue tag (`mdlg`, read
`SoundTags::mission_dialogue`) maps `120mg_030` to its speaker (`cor`) and `snd!`.
`ai_play_line_on_object`, `ai_play_line` and `vs_play_line` now queue the line as a one-shot and
return its length in ticks, and `sound_impulse_language_time` returns a sound's length, from the
language bank's `.info` (`halo3_script::dialogue`), so the scripts' `(sleep (ai_play_line...))`
wait as long as the take. Lines play on the dialogue bus at their class and tag gains (-6 dB on
most of Cortana's), and the other buses duck under them by the classes' dialogue ducking (music
-3 dB, effects -6 dB). The language is English unless `setup.env` has `LANGUAGE=french` (or
german, italian, mexican, spanish; setup does not ask) or `HALO3_LANGUAGE` overrides.
`docs/format/sound.md` 11 has the format; `blam sound 120_halo dialogue --grep 120mg` lists the
lines.

On the run, Cortana: "There! Johnson's Warthog!" (`120mg_010`) at the start, `120mg_030` on the
first stretch, then a line every 30 s (`120mg_040` to `090`) until the countdown takes over at
`trench_run_110_trig09` (`120mg_100` "charging sequence at 30 percent" to `120mg_160` "floor it!
right into the hangar!"). `md_09_cortana_hints` first waits for the hallway before the run, which
the insertion starts past, so `trench_run` counts `120_07_c_hall_end` as passed
(`setup::PASSED_ON_THE_WAY`). As in the map's scripts, if the player takes over 20 s to reach the
first stretch and then crosses it within a second, `md_09`'s once-a-second check misses it and the
hint lines stay silent until the countdown.

Checked: `halo3_script/tests/trench_dialogue.rs` (the six lines at their volumes, `120mg_040`
915 ticks plus `120mg_030`'s 64 ticks after it, `120mg_050` right after `040`),
`blam_sound/tests/install.rs` (`english_dialogue_bank_and_mission_dialogue`), and a headless
`warthog_run --bike --view -178,-300,-4,90` with `HALO3_AUDIO_TRACE=1`: `120mg_010`, `030`,
`040` (32.6 s after `030`) and `050` played, Cortana about -22 to -25 dBFS against the music at
-18.6. Not done: the voice effects (Cortana's `cortana_mission`, the radio filters), AI combat
chatter (`udlg`), placing `ai_play_line` lines at the actor. For the playtest: is Cortana loud
enough over the music (the Dialogue slider goes to 200 %)?

**Explosion effects (2026-10-31, branch `explosion-effects`, awaiting the user's playtest).**
The stand-in flashes are gone. The scripts' `effect_new`, `effect_new_random`,
`effect_new_at_ai_point`, `effect_new_on_object_marker` and the effects of the damage sections
they hit (both of a response's effects: a plate's `destroy` plays its chunks and the trench's
`plate_explosion`) go into `halo3_core::effects::EffectQueue`; `halo3_effects` plays them from
the map's tags, read with their bitmaps while the map loads (`main.rs`: the scripts'
referenced `effe` tags plus their objects' damage effects; 64 effects, 48 particles, 7
contrails, 8 lights, 35 bitmaps on the run, 0.1 s). Reader `blam_effect`, format
`docs/format/effects.md`, probe `docs/format/scripts/probe_effects.py`.

- **Played from the data**: events one after another with their delays and durations; each
  emitter's starting count, rate, lifespan, emission radius and angle, speed, size and scale,
  tint, alpha and black point by the tags' own functions and random inputs; particle physics
  (`pmov`: gravity scale, drag); the particles' bitmaps, sprite frames, palettes (the
  fireball's base map picks colours along `explosion_animated`), billboard and sprite alpha
  maps, billboards (camera facing, stretched along the velocity for sparks), blend modes;
  effects a particle carries (the ejecta each carry a flaming contrail); contrails (`cntl`:
  points a second, point life, width, colour, alpha, black point); light flashes (`ligh`) lit
  into the world by a term added to `lightmap.wesl`; sounds (`snd!`) through `halo3_audio`.
- **Our choices (commented in the code)**: particles at 3x their authored colour
  (`HALO3_EFFECT_EXPOSURE`; the world's x8 exposure turned fireballs white); particles leave
  along the effect's up axis; light falloff 4x the light's size, its strength scaled by 0.05;
  every particle material is drawn once invisibly at the start so the first explosion is not
  lost to shader compiles.
- **Not done**: `bloc` debris (the plate chunks flying up, the tunnel half girders), `jpt!`
  (shock wave, camera shake), `sefc` screen effects, `decs` decals, markers (effects play at
  the object's origin), soft particle edges, mesh particles. The plates' `fall` effects only
  hold a `bloc` and a `jpt!`, so a dropping plate has no effect to show (its fall is still
  ours). The weapons' and AI's impact effects are unchanged (their stand-ins in
  `halo3_weapons/src/fire.rs`; switching them over needs their tags read at load).
- Checked: `blam_effect/tests/run_effects.rs` (the plate explosion's and the giant explosion's
  layouts, every effect in 120_halo reads), `halo3_effects/tests/trench_effects.rs` (the run's
  scripts blow up `100_001c` and the player makes the flash, 50 rising fireball particles, the
  sounds and five to eight ejecta with contrails; the giant explosions go off at the
  scripts' points), all `halo3_script` and `halo3_anim` tests. Headless shots:
  `out\shots\plate_explosion\120_halo_2.png`, `_3.png` (plate `100_001c` blowing up, from the
  run's first trigger), `out\shots\giant4\120_halo_6.png`, `_9.png` (the first giant
  explosion in the canyon, from `120_08_trench_encounter_100`). `HALO3_EFFECT_TRACE=1` logs
  every effect started; `HALO3_SHOT_NEXT_WAIT` spaces later `--shot` views for time series.

**Enemies attack the rider (2026-11-03, branch `enemies-attack-rider`, awaiting the user's
playtest).** The AI fought only the Spartan on foot; on the bike the run's Flood and Sentinels
ignored you. Now:

- **One body for the player** (`halo3_core::ai::PlayerBody`, written by `halo3_ai/src/player.rs`
  every frame from the Spartan, the bike's `BikeMotion` or the skater's `SkateMotion`): feet, a
  hit capsule, velocity. The brain sees, hears, chases, leads and swings at it; the actors' rounds
  (`halo3_weapons/src/fire.rs`) hit it and fly on the shared collision (`SharedWorld`, the one
  the bike rides), not only the on-foot one; the damage goes to the same shields and health.
  On the bike the capsule stands round the rider (2.5 m, radius 0.63; ours): hits on the bike
  count as hits on him, the bike has no health of its own.
- **Firing at a fast rider**, from the tags: the lead is the firing pattern's target leading
  times his velocity times the round's flight time (beams lead 0; they are instant), and the
  aim now follows a moving target only as well as the pattern's **target tracking** (0.5 for
  every 120_halo character): the aimed point trails him by `(1 - tracking) x 0.3 s` (the 0.3 s
  and this reading of the field are ours). It applies on foot too. `blam ai --characters` now
  prints tracking and leading.
- **Difficulty**: what actors deal the player is scaled by the globals' first difficulty row
  (`matg` +0x410: 0.1 / 0.775 / 1.0 / 1.6 for easy / normal / heroic / legendary; our reading,
  `docs/format/ai.md` 8), by `--difficulty` (`halo3_core::ai::Difficulty`, normal by default).
  On foot too.
- **The engine is heard**: a moving bike is heard like gunfire within half the character's
  hearing distance (our share), so a squad you pass behind turns and fights. Without it a
  squad facing away never saw a rider going by (sight is the character's 50 deg cone).
- **Death and respawn on the bike**: health decides; a hit does not knock him off (Halo 3's
  vehicles do not throw their drivers). While he is dead the bike's controls go dead
  (`halo3_core::ai::PlayerRespawn::dead`) and it rolls on; after 3 s it is put down at his last
  quiet checkpoint (`PlayerRespawn::bike`, taken by `halo3_bike`) with full shields and health.
  Checkpoints are taken on the bike while it is upright on both wheels and no enemy has seen him
  for 3 s. On the board he comes back through the skate engine's respawn
  (`halo3_core::skate::Respawn`). The HUD bars, the red flash and "You died" show in all three.
- **Splatter**: the bike hurts what it runs into (`player.rs` `ram`, a capsule along the bike,
  swept from last frame so a fast bike cannot skip a small actor). From the map: the global
  `jpt! globals\collision_damage\collision` (100) times the Mongoose's `cddf`
  (`globals\collision_damage\vehicle_small`) scale 1.2, ramped by closing speed over its 3 to 8
  wu/s band (33 to 88 km/h): 120 at full speed, a Flood combat form (40 hp) dies from about
  50 km/h. The scale and the band are our reading of the `cddf` floats
  (`docs/format/weapons.md` 6a, `blam_geometry::weapons::read_vehicle_collision`). Slower,
  the bike only shoves them aside. The body is thrown along the bike's way. The bike does not
  slow down, the rider takes nothing, and actors are not solid to the bike's physics.
- Checked: `halo3_ai/tests/rider.rs` (120_halo's AI and weapons in a bare app on the run's
  collision, the rider stood in by `BikeMotion`): `trench_sentinel_100_1` hears the bike going
  past at 8 m/s, fires its beams and hurts him within the first 30 s, kills him, the bike's
  controls go dead and he is sent back to his checkpoint with full shields and health; riding
  at 20 m/s into a `trench_combat_forms01` elite kills it. `blam_geometry/tests/weapons.rs`
  `mongoose_collision_damage` and `ai.rs` `enemy_damage_by_difficulty` pin the numbers. Unit tests for the segment distance and the
  ramp. All `halo3_ai`, `halo3_script`, `halo3_anim` (but `frigate`, see below) and
  `halo3_effects` tests pass.
- Rounds that hit a body (an actor or the rider) no longer leave a scorch mark hanging in the air.
- Headless shots (`HALO3_AI_PLACE=squad,...` places squads as play begins whatever the scripts do,
  a dev hook for this; `HALO3_AI_TRACE=1` logs every hit on the player and every splatter):
  `out\shots\sentinel_beams\120_halo_0.png`, `_1.png` (`trench_sentinel_100_1`'s beams on a
  stopped rider, from `warthog_run --bike --view 92,-147,-29,180,0`, `HALO3_GOD=1
  HALO3_BIKE_THROTTLE=0 HALO3_SKATE_FOR=4`), `out\shots\flood_charge_4\120_halo_0.png`
  (`trench_combat_forms01` charging and swinging at him in the first tunnel, `--view
  55,-2,-33,90,0`), in the worktree. A ride at throttle 0.4 into all eight combat forms of
  `01` and `02` killed him within two seconds at normal before the difficulty scale (they fire
  head-on at a capsule they cannot miss); riding past the sentinels at 80 km/h drew no hits
  (most were beyond the sentinel gun's 8 wu range).
- **Still not right**: enemies on foot cannot keep up with the bike (combat forms run at their
  run animation's speed, about 2 wu/s), so the Flood mostly gets one swing or one burst as you
  pass; they have no pathfinding to cut you off. Sentinel beams are the tag's 4 damage at 30 a
  second, so a few sentinels together drain a standing rider in two or three seconds (the
  targets' material responses and the difficulty scaling are not read; the same on foot).
  Carriers do not spawn infection forms; no grenades; the Flood do not leap onto the bike. The
  rider's death has no animation; the bike just rolls on. `halo3_anim/tests/frigate.rs` does
  not build on master (it uses `setup::START_AHEAD`, which no longer exists); not touched here.

## Open questions and known gaps

- **Object rotation order.** Settled 2026-10-30 from 120_halo's frigate (yaw 180, pitch 2.45
  degrees): yaw, pitch, roll are composed about the world's axes as Reclaimer and HaloExport do
  (`scenario::rotation`); the other order sank the frigate's stern into the run's last ramp
  (`docs/format/scenario-and-tag-reading.md` 2.3). Not yet compared in the real game.
- **Instance scale.** Applied as a uniform scale on top of the 3x4 matrix, as Reclaimer does.
  Every instance checked so far has scale 1.
- **Shader classification** in `blam_geometry` (`Material::shading`) is still by class code
  only; the viewer refines it from the shader options through `blam_texture` (alpha test,
  blend mode, HaloExport's "special" path rule). `Material::shader_tag` links the two.
- **Large collision BSPs** (32-bit indices): no installed Halo 3 map uses them; eight Reach
  maps do (Forge World's main BSP only has a large one), read since 2026-10-31
  (`docs/format/reach.md` 6). Reach instances that are not flagged collidable have empty
  collision definitions and get no collision, as in the game; `blam_reach`'s
  `collision_coverage` test (ignored) lists drawn geometry with no collision near spawns.
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
- **HaloExport** (`reflex-tools\halo-tools\`): the C# exporter on Reclaimer that produced the
  reference numbers (`reflex-tools\halo-exports\<map>\halo_export.json`).
- **Reclaimer clone** at `reflex-tools\halo-tools\Reclaimer` (GPL-3.0, read-only reference).
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
