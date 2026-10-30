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
  on disk and public format documentation. No code from other projects is
  copied: Reclaimer is GPL-3.0 and this project is Apache-2.0.

## Layout

| path | what |
|---|---|
| `crates/blam_cache` | opens a `.map`: header, sections, tag index, tag names, string ids, a cursor for tag data, resource pages |
| `crates/blam_geometry` | decodes structure BSPs (clusters and instances) into plain meshes with materials |
| `crates/blam_h2` | MCC Halo 2 maps: inflated in memory, tags, `shared.map` and `textures.dat` read, BSP, collision, objects, spawns, shaders and bitmaps decoded into the same `Scene` (`docs/format/halo2.md`) |
| `crates/blam_script` | reads the HaloScript compiled into a scenario (scripts, globals, the expression table, the names scripts use) and prints it as source (`docs/format/halo-script.md`) |
| `crates/blam_sound` | Halo 3's sound: `snd!` / `lsnd` / sound scenery / BSP acoustics tags, and MCC's FMOD bank (FSB5 Vorbis) they play from, decoded with lewton |
| `crates/blam_texture` | decodes bitmap tags into mip chains (BCn kept as blocks) and resolves shader tags to their textures, tiling and blend options |
| `crates/blam_cli` | the `blam` tool: list tags, classes and strings, dump tag bytes, decode geometry to OBJ, inspect bitmaps and shaders, write PNGs |
| `crates/halo3` | the `halo3` binary: wires the crates below into an app, `--shot` screenshots, the pause menu, Halo 2 maps into the viewer's looks (`docs/viewer.md`) |
| `crates/halo3_core` | what the runtime crates share: coordinate frames, the pause flag, the flying camera, zone-set state, the rider's pose, kill zones, weapon messages. Every `halo3_*` crate depends on it, so keep it small |
| `crates/halo3_textures` | resolves a scene's materials to decoded textures and lightmaps (no bevy) |
| `crates/halo3_world` | the world on the GPU: BSP streaming, culling, lightmaps, sky, water, zone keys |
| `crates/halo3_audio` | the map's sounds, music, the board sounds and the weapons' sounds |
| `crates/halo3_skate` | skating through the Skate 3 engine (`skate-host`) |
| `crates/halo3_rider` | the Spartan on the board |
| `crates/halo3_bike` | the dirt bike (MX vs ATV Unleashed's model with Reflex's numbers), drawn as Reflex's Dakumoto 450F in the Cortana kit |
| `crates/reflex_assets` | reads the user's MX vs ATV Reflex install (database/package pairs, LZX, meshes, skeletons, textures) for the bike model (no bevy) |
| `crates/halo3_foot` | the Spartan on foot |
| `crates/halo3_weapons` | the map's weapons, carried and fired on foot |
| `crates/halo3_script` | runs a map's own scripts: threads at 30 ticks a second, objects created, destroyed, dropped and moved, devices, trigger volumes, insertion points; hooks for the AI and device animation |
| `crates/halo3_swim` | water, sinking and splashes |
| `docs/format` | the file format, field by field |
| `docs/` | notes on IW4L and the skate interface |

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
target\debug\blam.exe sound 120_halo wav "lsnd:levels\solo\120_halo\music\120_music_15"
& "target\debug\halo3.exe" riverworld
```

A bare map name is looked up in `%HALO3_MAPS%`, or in the default Steam
install when that is not set. `riverworld` is Valhalla, `cyberdyne` The Pit,
`deadlock` High Ground, `zanzibar` Last Resort, `bunkerworld` Standoff,
`sidewinder` Avalanche, `guardian` Guardian,
`blackout` Blackout (its file is `lockout.map`), `chill` Narrows, `armory` Rat's Nest, `shrine` Sandtrap,
`sandbox` Sandbox, `isolation` Isolation. Halo 2 maps are then looked for in `%HALO2_MAPS%` or MCC's
`halo2\h2_maps_win64_dx11` (`coagulation`, `lockout` Lockout; `h2:NAME` / `h3:NAME` when
both games have a map of that file name, see `docs/viewer.md` "Halo 2 maps").

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
      (two-lobe phong, cook-torrance, glass), an SH stand-in for
      environment maps, self-illumination, per-map exposure from the
      camera fx; spawn markers hidden (`docs/viewer.md`).
- [ ] Real cube-map reflections, parallax.
- [x] Milestone 3a: the game's real collision BSP decoded (the handoff's
      "known gap"): Valhalla gives 30,560 world and 39,678 instanced collision
      triangles with invisible walls, slip and two-sided surfaces flagged.
      The viewer shows it as a shell on V. Not yet used to stop the camera.
- [x] Grind rails from the collision (IW4L's lip detector, ported): 2,904 on
      Valhalla, edge-of-world rims dropped. `blam rails <map> --obj PATH`.
- [x] Spawn points (107 on Valhalla); the viewer starts on one.
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
      hand (M; the Warthog run on 120_halo); board roll, grind and landing
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
- [x] Milestone 3b: collision and rails fed to the skate world (playtested
      on Valhalla).
- [x] Milestone 4: Skate 3 crates plugged in (path dependency on
      `mw2-skate-3-src`), Master Chief as the rider (`docs/rider.md`).
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
      are its only floor). AI, cinematics, sound and real effects are
      stubs or hooks (`docs/format/halo-script.md` 10).
