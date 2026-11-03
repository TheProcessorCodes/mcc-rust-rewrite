# Halo 3 in Rust

A from-scratch Rust runtime for Halo 3, reading the map files of the user's own
Master Chief Collection install. Built the way IW4L rebuilt Modern Warfare 2:
own readers for the game's files, own renderer on bevy and wgpu, and later the
Skate 3 crates plugged in so you can skate on Halo maps.

`HANDOFF.md` has the goals, the research and the open decisions. `docs/` holds
the format descriptions the code is written from.

## Rules

- No game files live here. The code reads the MCC install read-only.
- Nothing here launches the game.
- The format descriptions in `docs/format` were written by reading the files
  on disk and public format documentation. No code from other Halo tools is
  copied (Reclaimer, TagTool, Assembly). The one outside code base in the
  tree is the Skate 3 engine in `third_party/skate-engine` (see Licence).

## Layout

| path | what |
|---|---|
| `crates/blam_anim` | Halo 3 animation graphs (`jmad`): skeleton, animation list, the mode / weapon class / weapon type / action tree, every codec of the animation data; poses sampled, blended and turned into skinning matrices (`docs/format/animation.md`) |
| `crates/blam_cache` | opens a `.map`: header, sections, tag index, tag names, string ids, a cursor for tag data, resource pages |
| `crates/blam_geometry` | decodes structure BSPs (clusters and instances) into plain meshes with materials |
| `crates/blam_h1` | MCC Halo CE maps: tags, BSP lightmap meshes, collision, gbxmodels, shaders, bitmaps (`bitmaps.map`), spawns and sky decoded into the same `Scene` (`docs/format/halo1.md`) |
| `crates/blam_h2` | MCC Halo 2 maps: inflated in memory, tags, `shared.map` and `textures.dat` read, BSP, collision, objects, spawns, shaders and bitmaps decoded into the same `Scene` (`docs/format/halo2.md`) |
| `crates/blam_script` | reads the HaloScript compiled into a scenario (scripts, globals, the expression table, the names scripts use) and prints it as source (`docs/format/halo-script.md`) |
| `crates/blam_sound` | Halo 3's sound: `snd!` / `lsnd` / sound scenery / BSP acoustics tags, and MCC's FMOD bank (FSB5 Vorbis) they play from, decoded with lewton |
| `crates/blam_effect` | Halo 3's effects: `effe` events and parts, particle systems and emitters with their function blobs, particles (`prt3`) with their render method, particle physics (`pmov`), contrails (`cntl`), lights (`ligh`) (`docs/format/effects.md`) |
| `crates/blam_texture` | decodes bitmap tags into mip chains (BCn kept as blocks) and resolves shader tags to their textures, tiling and blend options |
| `crates/blam_cli` | the `blam` tool: list tags, classes and strings, dump tag bytes, decode geometry to OBJ, inspect bitmaps and shaders, write PNGs |
| `crates/blam_reach` | MCC Halo: Reach maps: BSPs from the lightmap BSP data, instances from their resource, collision, objects, sky, spawns and lightmap texcoords into the same `Scene`; the cache file, shaders and bitmaps go through `blam_cache` / `blam_texture`, which tell Reach by its header (`docs/format/reach.md`) |
| `crates/game_paths` | where the player's games are: the saved setup and dev overrides (`HALO3_MAPS`, ...); nothing is searched for. No dependencies; the readers' tests use it |
| `crates/halo3_setup` | first-run setup: find MCC, take Skate 3 (`.iso` or `default.xex`) and convert it once, take Reflex if the player has it, save the answers |
| `crates/halo3` | the `halo3` binary: the map menu (`menu.rs`), wires the crates below into an app, `--shot` screenshots, the pause menu, Halo 2 maps into the viewer's looks (`docs/viewer.md`) |
| `crates/halo3_core` | what the runtime crates share: the map phase (`map.rs`: a map loaded, started and unloaded inside the running app), coordinate frames, the pause flag, the flying camera, zone-set state, the rider's pose, kill zones, weapon messages. Every `halo3_*` crate depends on it, so keep it small |
| `crates/halo3_textures` | resolves a scene's materials to decoded textures and lightmaps (no bevy) |
| `crates/halo3_world` | the world on the GPU: BSP streaming, culling, lightmaps, sky, water, zone keys |
| `crates/halo3_anim` | Halo's own animations on objects: `AnimModel` (one per object type) and the `Animator` component (actions with fallbacks, cross-fades, overlays, aiming, device positions, root motion) posing skinned joints |
| `crates/halo3_audio` | the map's sounds, music, the board sounds and the weapons' sounds |
| `crates/halo3_skate` | skating through the Skate 3 engine (`skate-host`) |
| `third_party/skate-engine` | the Skate 3 engine crates, vendored (GPL-3.0; its README says where from and what we changed) |
| `crates/halo3_rider` | the Spartan on the board |
| `crates/halo3_bike` | the dirt bike (MX vs ATV Unleashed's model with Reflex's numbers), drawn as Reflex's Dakumoto 450F in the Cortana kit |
| `crates/reflex_assets` | reads the user's MX vs ATV Reflex install (database/package pairs, LZX, meshes, skeletons, textures) for the bike model (no bevy) |
| `crates/halo3_foot` | the Spartan on foot |
| `crates/halo3_weapons` | the map's weapons, carried and fired on foot |
| `crates/halo3_script` | runs a map's own scripts: threads at 30 ticks a second, objects created, destroyed, dropped and moved, devices, trigger volumes, insertion points; hooks for the AI and device animation |
| `crates/halo3_effects` | plays the effects the scripts and damaged objects ask for from the map's `effe` tags: particles, contrails, light flashes, sounds |
| `crates/halo3_weapons` | the map's weapons, carried and fired on foot; rounds hit actors and the player |
| `crates/halo3_ai` | the campaign's squads as actors: placed, drawn, hurt and fighting; the player's shields and health |
| `crates/halo3_swim` | water, sinking and splashes |
| `docs/format` | the file format, field by field |
| `docs/` | notes on IW4L and the skate interface |

## First run

Nothing from any of the three games ships with this project. The first time `halo3` opens a
window it opens on its setup screen (`crates/halo3/src/menu/setup.rs`, in the map menu's own
look) and asks for the player's own copies, once, one step per game: each row's Select opens a
Windows folder picker, and problems are shown on the screen. Nothing is searched for; the
game uses only the folders the player chooses. MCC and Skate 3 are required: the game stays on
the setup screen until both are set up (B / Escape on the first step closes it). Reflex is
optional (Skip). A map named on the command line before setup is done waits for the next
launch.

1. **Halo: The Master Chief Collection.** The MCC folder (or its `halo3` / `halo3\maps`
   folder); the Halo 3 build is checked.
2. **Skate 3 for Xbox 360.** The folder of the extracted game (`default.xex` with its `data`
   folder, directly inside or one folder down) or the folder holding its disc image (`.iso`,
   the only one there). From an image only
   what the converter reads is copied out (`default.xex`, four `.big` files and `data\anim`,
   about 512 MB, deleted afterwards). The
   converter is IW4L's `iw4l-skate-convert.exe`, which this project does not ship: the
   player downloads MW2 Skate 3 (the 2010 Rust Rewrite Mashup, from the Releases page of
   https://github.com/chasmlol/2010-rust-rewrite-mashup) and setup asks for its
   folder ("Choose your MW2 Skate 3 folder") just before Skate 3's, then runs its
   `skate\iw4l-skate-convert.exe`. The conversion runs on the setup screen, which shows the
   converter's progress.
3. **MX vs ATV Reflex** (PC, optional; the dirt bike's model and engine sound). The folder
   with its `Database` folder, or Skip. Without it the
   bike is our own stand-in model (`crates/halo3_bike/src/stand_in.rs`) with no engine
   sound, and the answer is saved like a folder (`REFLEX=skip`).

The answers and the converted Skate 3 data go in `%LOCALAPPDATA%\Halo3Rewrite` (`setup.env`,
`skate-data\`), per user, so every copy of the exe shares them. `halo3 --setup` asks again
(the converted Skate 3 data is replaced only once a new conversion succeeds); Settings on the
map menu changes one game at a time. Headless `--shot` and `--menu-shot` runs never ask or
write the setup (except `--menu-shot --menu-setup [--menu-step N]`, which draws the setup
screen from whatever is saved; point `HALO3_SETUP_DIR` at an empty folder for a fresh one):
they use only what
is saved (or the dev overrides) and stop with a message if anything is missing. So do tests:
they skip when setup has not run.

Skate 3 stays required at setup (the rider's skeleton and the on-foot Spartan's rig come
from it), but skating itself can be turned off on the menu's Settings screen (Skating,
saved as `SKATING=off`). Maps then load without grind rails, the skate session or the board
(a map is ready several seconds sooner), J does nothing, and the pause menu's Skate tab says
skating is off and where to turn it on. The choice applies from the next map loaded.

Dev overrides: `HALO3_MCC`, `HALO3_MAPS`, `HALO2_MAPS`, `HALO1_MAPS`, `HALOREACH_MAPS`, `HALO3_SKATE_ASSETS` (or
`IW4L_SKATE_ASSETS`), `HALO3_SKATE_CONVERTER`, `REFLEX_GAME` (`skip` to ride without
Reflex), `HALO3_SETUP_DIR`, `HALO3_LANGUAGE` (dialogue language: `english`, `french`,
`german`, `italian`, `mexican` or `spanish`; else `LANGUAGE=` in `setup.env`, else English),
`HALO3_SKATING` (`off` / `on`, over the saved Skating setting; `--skate` shots always skate),
`HALO3_SHOT_PAUSE_TAB=N` (a `--shot` draws the pause menu open on tab N).

## Try it

```
cargo build
target\debug\blam.exe info riverworld
target\debug\blam.exe tags riverworld --class sbsp
target\debug\blam.exe dump riverworld "sbsp:levels\multi\riverworld\riverworld" --len 256
target\debug\blam.exe geometry riverworld --obj out\riverworld.obj
target\debug\blam.exe shader riverworld 11454
target\debug\blam.exe bitmap riverworld 112 --png out\mountains_snow.png
target\debug\blam.exe textures riverworld
target\debug\blam.exe sound riverworld list
target\debug\blam.exe scripts 120_halo --grep trench_timer_100_001
target\debug\blam.exe ai 120_halo --squads
target\debug\blam.exe sound 120_halo wav "lsnd:levels\solo\120_halo\music\120_music_15"
target\debug\blam.exe sound 120_halo dialogue --grep 120mg
target\debug\blam.exe anim 120_halo "bipd:objects\characters\floodcombat_human\floodcombat_human"
target\debug\blam.exe anim 120_halo "bipd:objects\characters\floodcombat_human\floodcombat_human" --anim combat:any:any:melee --frame 6
target\debug\blam.exe anim 120_halo --all
& "target\debug\halo3.exe"
& "target\debug\halo3.exe" riverworld
& "target\debug\halo3.exe" warthog_run
& "target\debug\halo3.exe" h1:warthog_run
```

`halo3.exe` with no map opens the map menu: Halo 3, Halo 2, Halo CE, Reach and
Campaign tabs, each map shown with MCC's own preview picture
(`Data\UI\texturepacks\levelstexturepack`, read at run time). MCC lets players
skip a game's content, so a map whose `.map` file is not in its game's maps
folder is grayed out, and choosing it shows "You must install these Halo maps
with MCC" instead of loading (A / B / Enter / Esc or a click closes it). Start
Game loads the map into the same window behind a loading screen; Q (or View)
in the pause menu unloads it and goes back to the menu. Arrows / WASD / d-pad /
left stick move, Q / E / LB / RB change tab, Enter / A start, Escape / B exit;
the mouse picks a tile and a double click starts it. Naming a map on the
command line skips the menu, and Q then quits. `halo3.exe --menu-shot FILE
[--menu-tab N] [--menu-pick N] [--menu-loading | --menu-missing | --menu-settings | --menu-setup | --menu-start
[--menu-back TAB,PICK]]` draws the menu, the loading screen, the picked map's
not-installed notice or the loaded map without a window (point e.g.
`HALOREACH_MAPS` at an empty folder to see a game grayed out).

A bare map name is looked up in the MCC install setup saved (`%HALO3_MAPS%`
overrides; the `blam` tool also finds MCC through Steam). `riverworld` is Valhalla, `cyberdyne` The Pit,
`deadlock` High Ground, `zanzibar` Last Resort, `bunkerworld` Standoff,
`sidewinder` Avalanche, `guardian` Guardian,
`blackout` Blackout (its file is `lockout.map`), `chill` Narrows, `armory` Rat's Nest, `shrine` Sandtrap,
`sandbox` Sandbox, `isolation` Isolation. Halo 2 maps are then looked for in MCC's
`halo2\h2_maps_win64_dx11` (`coagulation`, `lockout` Lockout; `h2:NAME` / `h3:NAME` when
both games have a map of that file name, see `docs/viewer.md` "Halo 2 maps").
Halo CE maps are only found as `h1:NAME` (`h1:bloodgulch`, ... in `%HALO1_MAPS%` or MCC's
`halo1\maps`); a map's game is told from its own header, not its file name.
Reach maps go by `hr:NAME` (`%HALOREACH_MAPS%` or MCC's `haloreach\maps`): `hr:20_sword_slayer`
Sword Base, `hr:cex_timberland` Timberland, `hr:70_boneyard` Boneyard, ... (the menu's Reach tab
lists all 20).

## Status

- [x] Milestone 1a: open a map, list its tags and strings (all 183 tag classes
      and 11,658 tags of Valhalla resolve).
- [x] Milestone 1b: decode the BSP's render geometry into triangles. Valhalla
      and The Pit come out with exactly HaloExport's counts (23 and 44 cluster
      meshes, 261 and 830 instances, 124,627 and 241,559 triangles) in 0.2 s;
      campaign maps decode all their BSPs. Checked by `cargo test`.
- [x] Milestone 2 (built, awaiting a playtest): window, BSP drawn in flat
      colours per material, flying camera with keyboard, mouse and gamepad.
- [x] Last Resort (`zanzibar`, 2026-10-28): every HaloExport count matches
      (39 cluster meshes, 1,123 instances, 624,651 triangles with objects),
      146 of 146 colour bitmaps decode, 106 spawns all on the ground and
      clear of walls; checked by `cargo test`
      (`docs/format/reference-numbers.md` 9). Fixed on the way: palm fronds
      cut by their own `alpha_test_map` in the water depth prepass, the
      sky's `from_diffuse` glow and halogram edge fade (white slabs on the
      sea), rails under deep water dropped.
- [x] High Ground (`deadlock`, 2026-10-28): every HaloExport count matches
      (43 cluster meshes, 849 instances, 564,094 triangles with objects,
      the gate machine where HaloExport put it), 101 of 101 colour bitmaps
      decode, 85 spawns all on the ground and clear of walls, no collision
      holes under the render floors; checked by `cargo test`
      (`docs/format/reference-numbers.md` 10). Fixed on the way: the sky's
      alpha-tested `water_edge_breakup` cards drawn whole round the cove,
      multiply decals (`medical_decals`) lit and exposed. Still off: a hard
      seam where the cove's water ends and the sky's far sea begins.
- [x] Isolation (`isolation`, 2026-10-29; not built yet, awaiting a build
      and a playtest): every HaloExport count matches (47 cluster meshes,
      391 instances, 380,662 triangles with objects, the Flood door where
      HaloExport put it), 49 of 49 colour bitmaps decode, 91 spawns all on
      the ground and clear of walls, no collision holes under the render
      floors, no kill volume (the walls keep everyone in), about 3,900
      rails; pinned by `cargo test` (`docs/format/reference-numbers.md`
      11). Fixed on the way: two of the three energy-wall glows store an
      intensity of 0 and pulse up to 55, and drew nothing; they now glow at
      the top of the pulse. The Flood's `organism` shader is drawn diffuse.
- [x] Milestone 1c: scenery, crates, machines, controls and the sky placed
      from the scenario, again matching HaloExport's triangle counts exactly.
- [x] Textures (built, awaiting a playtest): every material draws its real
      colour map with the shader's tiling; foliage is alpha-tested, glass and
      decals blended, glows additive, invisible shaders skipped. On Valhalla
      79 of 86 materials are textured from 58 bitmaps, 30.8 MiB uploaded
      still BC-compressed, about 1 s of extra load time. Checked by
      `cargo test` against `docs/format/bitmap.md` and `shaders.md`.
- [x] The four-layer terrain blend (blend map, base x detail per layer, lightmapped).
- [x] Water: animated waves from the shader's own slope arrays, sky
      reflection, depth-based murk and shore fade, foam, flowing waterfalls;
      the skater sinks in deep water and comes back at the last dry spot
      (`docs/format/water.md`).
- [x] Materials: detail maps, normal maps on Halo's own tangents (BC5
      kept compressed), specular from the lightmap's dominant light
      (two-lobe phong, cook-torrance, glass), self-illumination, per-map
      exposure from the camera fx; spawn markers hidden (`docs/viewer.md`).
- [x] Cube-map reflections and parallax (2026-11-02; checked by headless
      shots, awaiting a playtest): each BSP's own reflection cubes (scenario
      BSP `+0x48`, the capture points per cluster at cluster `+0x10C`) go up
      as one cube array; each chunk blends the three nearest of its
      clusters' cubes, at the level of the lightmap's light at the pixel
      (the cubes are stored normalised). `parallax` simple / interpolated
      moves the texcoords by the height map. `blam cubemaps <map>` lists
      them; `HALO3_REFLECTIONS=0` brings back the SH stand-in,
      `HALO3_PARALLAX=0` flat (`docs/format/cubemaps.md`).
- [x] Enemies and AI on 120_halo (2026-10-29; built, awaiting a playtest):
      the scenario's 262 squads, their cells and starting locations, 11 AI
      zones with 3,842 firing positions and the 15 characters read and
      pinned by `cargo test` (`docs/format/ai.md`, `blam ai`). Squads
      appear as you come within 70 m of them (`--ai nearby`, the stand-in
      until scripts place them; `--ai zone` places a zone set's all at
      once), drawn with their units' real models in the bind pose, holding
      their weapons. They see and hear you, the Flood charge and hit,
      Sentinels hover and fire their beams, gunmen keep their weapon's
      range and fire bursts from their firing patterns. Rounds hurt both
      ways; shields and health come from the tags; you come back at the
      last quiet spot when you die. No animations yet, no vehicles, no
      grenades, no pathfinding beyond firing positions and straight lines.
- [x] Milestone 3a: the game's real collision BSP decoded (the handoff's
      "known gap"): Valhalla gives 30,560 world and 39,678 instanced collision
      triangles with invisible walls, slip and two-sided surfaces flagged.
      The viewer shows it as a shell on V. Not yet used to stop the camera.
- [x] Grind rails from the collision (IW4L's lip detector, ported): 2,904 on
      Valhalla, edge-of-world rims dropped. `blam rails <map> --obj PATH`.
- [x] Spawn points (107 on Valhalla); the viewer starts on one.
- [x] Halo 3 animation (2026-10-29; checked by tests and headless shots, not
      playtested): `jmad` graphs and every codec in the maps decode (`blam
      anim`; 5881 of 120_halo's 5996 animations, the rest have no data in
      that map). `halo3_anim` plays them on the Flood, Sentinels, Master
      Chief and device machines; `halo3 120_halo --anim-test ...` shows a row
      of animated bipeds. Not yet wired into the enemies or the on-foot
      Spartan (`docs/format/animation.md`).
- [x] Spartan skeleton and skin weights decode (`blam nodes`); bone mapping
      to the skater in `docs/rider.md`.
- [x] On foot (built, awaiting a playtest): F walks the Spartan with Halo 3's
      own movement numbers (run, crouch, jump, slopes, capsule), a made-up
      gait for the skeleton, an over-shoulder or eye camera; he carries the
      map's weapons and fires them with their tags' rates, magazines and
      projectile speeds (`docs/viewer.md` "On foot").
- [x] Dirt bike (built, awaiting a playtest): B rides a motocross bike, Reflex's
      Dakumoto 450F in the Cortana kit read from the user's own MX vs ATV Reflex
      (`docs/format/reflex-bike.md`; a stick model without Reflex), with dual-stick physics (left stick bars, right stick rider, RT gas,
      LT brake), the Spartan standing on it (`docs/viewer.md` "Dirt bike").
- [x] Guardian (`guardian`; built, awaiting a playtest): 26 clusters,
      471 instances, 258 k triangles, 53 spawns (all on a floor), 14,448 world
      and 97,940 instanced collision triangles, about 3,400 rails. Falling
      into the abyss (the `kill_drop` kill volume, or anywhere far below the
      collision) puts the skater or the bike back where it last stood; the
      jump pads' and minilift's `plasma` glow is drawn. Pinned by
      `cargo test` (our own counts: there is no HaloExport reference).
- [x] Sound (built, awaiting a playtest): Halo 3's own audio from MCC's FMOD
      bank (`halo3\fmod\pc\sfx.fsb`, Vorbis; setup headers rebuilt from the
      user's `halo3.dll`), driven by the map's tags: each cluster's background
      ambience with its random details, cross-faded as you move; the sound
      scenery placed in the world, panned and attenuated by distance; music by
      hand (M; the Warthog run on 120_halo); the scripts' dialogue lines
      (Cortana, Johnson, Guilty Spark ...) from MCC's language bank
      (`english.fsb`), on the Dialogue slider; board roll, grind and landing
      sounds while skating. K mutes, `-` / `=` volume, `HALO3_AUDIO=0` off
      (`docs/format/sound.md`, `docs/viewer.md` "Sound").
- [x] Blackout (`blackout`, file `lockout.map`, DLC; built, awaiting a playtest): 21 clusters,
      447 instances, 442 k triangles, 80 spawns (all on a floor), 19,326
      world and 166,536 instanced collision triangles, about 4,150 rails.
      A night sky: the sky's lit pieces get a tenth of the daylight
      (`NIGHT_SKIES` in `world/sky.rs`), so the sea is black under white ice
      floes, with the moon, its halo and the aurora (`plasma`) drawn.
      Dropping off the rig (`kill_volume_bottom`, z -18.3) puts the skater
      or the bike back. Pinned by `cargo test` (our own counts).
- [x] Narrows (`chill`; written 2026-10-28, not yet built or playtested):
      29 cluster meshes, 201 instances, 287 k triangles with objects, 109
      materials all resolving and every bitmap decoding, 79 spawns (all on
      a floor, on the bases and the bridge), 25,770 world and 64,620
      instanced collision triangles, 4,204 rails of which about 3,115 are
      kept above the chasm. Falling off the bridge into `kill_drop` (up to
      z 48.7) puts the skater or the bike back. Fixed on the way, not yet
      built: plasma glows weighted by how much of each thin band shows
      (the energy walls blew out to white, the corridor light strips were
      solid slabs, their mask read from red where its alpha is flat), and
      a bike panic on a zero-length first frame (NaN into `clamp`). Pinned
      by `cargo test` (our own counts, no HaloExport reference).
- [x] Rat's Nest (`armory`, DLC; checked by headless shots, test written
      but not yet built): 38 clusters, 1,906 instances, 894 k triangles,
      97 spawns (all on a floor, clear of walls), 19,044 world and 319,704
      instanced collision triangles, no holes under the play space's
      floors, about 6,990 rails (47 dropped in `kill_vol_cliff`, the
      canyon under the west overlook). Interiors, the overlook, the sky's
      smoke columns, skating and the bike come out right with no
      map-specific code; the sky's scrolling smoke shaders are no longer
      mistaken for waterfalls. Not yet: crates and props have no collision
      (object `coll` models are not decoded on any map), so the hangar's
      giant crates can be ridden through. Our own counts
      (`crates/blam_geometry/tests/maps.rs`).
- [x] Sandtrap (`shrine`; checked by headless shots before the changes
      below, which are written but not yet built): 42 clusters, 327
      instances, 313 k triangles, 103 spawns, 11,496 world and 50,684
      instanced collision triangles, no holes under the render floors. The
      sand blend, the Forerunner wall and obelisks, the crashed Phantom, the
      frigate in the sky, haze and exposure come out with no map-specific
      code. New: the map's own vehicles are placed (`Kind::Vehicle`, tags
      under `objects\levels\`): the two Elephants (the team bases) and the
      six Guardian defenders on the obelisks. The map has no kill volumes;
      leaving the safe zone where a Guardian watches (`tv_turret_*`) now
      counts as a fall (`blam_geometry::scene::guarded`), so the bike can
      no longer ride out over the dunes, and rails there are cut (about
      2,940 found, 2,124 kept). Not yet: the Elephants have no collision,
      so the six spawns on their decks drop through to the sand. Our own
      counts (`crates/blam_geometry/tests/maps.rs`, `docs/viewer.md`
      "Falling out of the map").
- [x] Sandbox (`sandbox`, DLC; written, not built yet): 7 clusters,
      46 instances, 266 k triangles with its Forge pieces, 80 spawns (50 on
      the collision, 28 on Forge pieces that have no collision yet),
      12,889 world and 7,644 instanced collision triangles, about 1,095
      rails, one `kill` slab under the sky level. Fixed on the way: the
      Forge floor grid (an additive halogram with a flat grey albedo) drew
      a white sheet over the middle of the map; it now draws its overlay's
      grid lines (`docs/format/shaders.md` 8.7, and it needs the Narrows
      plasma-mask commit for its glow). Pre-multiplied sky clouds are
      multiplied by their alpha: the horizon band and cloud cards had hard
      edges (`docs/format/sky-and-atmosphere.md` 8). Pinned by `cargo test`
      (our own counts).
- [x] Halo 2: Coagulation (`coagulation`, MCC Halo 2 classic; built,
      awaiting a playtest): the zlib-chunked map inflated in memory, its
      shared tags read from `shared.map` and its pixels from `textures.dat`
      by our own `blam_h2` crate (`docs/format/halo2.md`). Every HaloExportH2
      count matches (48 clusters with 12,797 triangles, 63 instances 5,591,
      scenery 2,324, crates 2); the collision BSP gives 8,586 world and
      4,502 instanced triangles (no broken loops); 123 spawns, all on a
      floor; base and detail textures, the ground's detail blend, foliage,
      glows drawn. Lit by its own lightmap and fogged by its sky's fog
      (see Lockout). Not yet: its sky model (the fog colour stands in). Skating (610 rails), the bike
      and the Spartan rider (borrowed from Valhalla) run on it. Pinned by `cargo test`
      (`crates/blam_h2/tests/coagulation.rs`).
- [x] Halo 2: Lockout (`lockout`; Halo 3's Blackout, also `lockout.map`,
      is now `blackout`; built, awaiting a playtest): 16 clusters with
      7,049 triangles, 75 instances 7,414, 15 crates, 5 scenery; 5,570
      world and 7,312 instanced collision triangles, 66 spawns, 675 rails,
      all 33 bitmaps decoded. No HaloExportH2 export to check against; our
      own counts are pinned (`crates/blam_h2/tests/lockout.rs`). Lit by
      its lightmap (65 pages packed into one atlas, the light fixtures from
      the nearest surface) and the sky's blue fog (`docs/format/halo2.md`
      7, 10). Not yet: the sky model; a few small black panels on the boxes.
- [x] Halo CE: all 19 multiplayer maps (`h1:NAME`, the menu's Halo CE tab;
      built 2026-10-31, awaiting a playtest), read by our own `blam_h1`
      (`docs/format/halo1.md`): the BSP's lightmap meshes with their
      textures (blended ground, alpha-tested foliage, transparent
      stacks), lit by the map's lightmap pages, scenery and machines, the
      sky model with its cloud masks, outdoor fog where a cluster sees the
      sky, the collision BSP (no broken loops) and the player starting
      locations as spawns. HaloExportH1's lightmap, triangle and scenery
      counts match on Blood Gulch, Death Island, Sidewinder and Timberland
      (`crates/blam_h1/tests/maps.rs`). Headless shots of every map; the
      bike, skating and walking stand on Blood Gulch's collision. Not yet:
      weapons, sound, teleporters, water animation.
- [x] Halo CE: The Warthog Run (`h1:warthog_run`, the menu's Campaign tab; built 2026-11-02,
      awaiting a playtest): The Maw (`d40`) from the Warthogs at the top of the lift, on the
      dirt bike. Only the run's BSPs (`d40f`, `d40g`, `d40h`), one at a time, switched by
      the map's own BSP switch volumes. The map's own scripts (`blam_h1::script`, read into
      Halo 3's `ScriptData`) run on `halo3_script`: the countdown (6:00, 5:00 on Legendary)
      in the bottom right, red under a minute, paused for the Pelican scene; reaching
      `grand_finale` by the Longsword wins (MISSION COMPLETE with the time and what was left
      on the countdown), the countdown running out loses (MISSION FAILED). Checked by
      `crates/halo3_script/tests/h1_warthog_run.rs` and headless shots along the run. Not
      yet: enemies, effects (explosions), sound, dialogue, music, doors (drawn, not solid).
- [x] Halo: Reach, the 20 multiplayer maps (`hr:NAME`, the menu's Reach tab; built, awaiting
      a playtest): our own `blam_reach` crate (`docs/format/reach.md`) on the Halo 3 path:
      textured BSPs, instances, scenery, crates, machines, controls and the sky model;
      the collision BSP and instance collision (skating and walking checked headlessly on
      Sword Base and Beaver Creek); 164 to 396 spawn points per map (Forge World 19). Lit
      by Reach's lightmaps read as a direction-free light; instances lit per vertex or by
      probe in Reach use the nearest-surface fallback. Not yet: the lightmaps' directions,
      fog and atmosphere, camera fx, sound, weapons, Forge World's Forge objects. Counts
      pinned by `cargo test -p blam_reach`.
- [x] Milestone 3b: collision and rails fed to the skate world (playtested
      on Valhalla).
- [x] Milestone 4: Skate 3 crates plugged in (now vendored in
      `third_party/skate-engine`), Master Chief as the rider (`docs/rider.md`).
- [x] Milestone 5 (120_halo; built, awaiting a playtest): campaign zone sets
      (PageUp / PageDown, `--zone`, and the scenario's switch volumes), the
      player starting location, sky and haze per cluster, skating on the
      loaded zone set with kill-volume rails dropped.
- [x] Culling and streaming (built, awaiting a playtest): chunked world with
      frustum, cluster-portal and screen-size culling; only the loaded zone
      set's meshes and textures on the GPU, built in the background; the
      skate collision swapped in without stopping the ride
      (`docs/viewer.md` "Streaming and culling", `--bench N`). Other campaign maps load the same way but are not checked yet
      (`docs/viewer.md` "Zone sets", `docs/format/zone-sets.md`).
- [x] HaloScript (2026-10-29; built and tested headless, awaiting a
      playtest): `blam scripts <map>` prints a map's scripts as source
      (120_halo: 731 scripts, 113 globals, 281 functions); the game runs
      them with `--insertion NAME|N` or `--scripts` (`halo3_script`).
      120_halo's Warthog run plays its own collapse: `--insertion
      trench_run` switches to the run's zone set and puts you at its start
      flag, the floor plates appear stretch by stretch, then drop away
      behind and around you row by row (or blow up ahead), and their
      collision goes with them so the bike, the Spartan or the skater
      falls. Placed objects now collide (`coll` models; the run's plates
      are its only floor). Cinematics are stubs (`docs/format/halo-script.md` 10).
- [x] Explosion effects (2026-10-31; built and tested headless, awaiting a
      playtest): the scripts' `effect_new*` calls and the damage sections of
      the objects they damage play Halo 3's own `effe` tags: the plates
      blowing up ahead (flash, sparks, fiery smoke, fireball, ejecta with
      flaming contrails, a fiery plume, their light and sounds) and the giant
      explosions far off in the canyon, from the tags' particle systems,
      bitmaps, palettes and functions (`halo3_effects`,
      `docs/format/effects.md`). Not drawn: debris objects, camera shake,
      screen effects, decals. `HALO3_EFFECT_TRACE=1` logs every effect.

## Licence

Our own crates are Apache-2.0 (`LICENSE-APACHE`). `halo3.exe` links the Skate 3 engine in
`third_party/skate-engine`, which is GPL-3.0-only, so `halo3` and `halo3_skate`, and any
released `halo3.exe`, are distributed under the GNU General Public License version 3 only
(`LICENSE`), with this source. Apache-2.0 code may be used in a GPL-3.0 work. The Skate 3
converter `iw4l-skate-convert.exe` is not shipped; it comes from the player's MW2 Skate 3
download, with its own licence files. None of this
grants rights to any game's code, data, assets or trademarks; the player brings their own copies.
