# Halo 3 Rust rewrite: handoff

First written 2026-10-27 at the end of a research thread in `mw2-skate-3-src`; rewritten the
same day after the first build session in this folder. `README.md` has the layout and the
status list; `docs/` has the format notes the code is written from.

## The goal

Do for Halo 3 what `mw2-skate-3-src` did for MW2: get our own code running with Halo 3
so the user can **skate (Skate 3 physics) on real Halo 3 maps**.

| step | goal | state |
|---|---|---|
| 1 | get custom code into Halo 3, via a rewrite or a custom launcher | done as a rewrite: our code reads the real maps |
| 2 | noclip: fly around the maps | built (`halo3.exe`), awaiting the user's first playtest |
| 3 | skate on multiplayer maps | next: collision and rails are ready, the skate crates are not linked yet |
| 4 | skate on campaign maps | 120_halo built (zone sets, start location, per-cluster sky and haze, skating per zone set), awaiting a playtest; other campaign maps not checked |
| later | dirt bike game (MX vs ATV Reflex) in the mix | not before skating works |

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

## What exists now

A Cargo workspace (`cargo build`, `cargo test`, all green, clippy clean):

| crate | what it does |
|---|---|
| `blam_cache` | opens a `.map` read-only (memory-mapped): header, four sections and their address rules, tag classes, tag index, tag names, string ids with the namespace table, a cursor for tag data (blocks, tag references, data references), and the resource system (`zone` gestalt, `play` layout table, deflate pages, shared caches, fixed-up definitions) |
| `blam_geometry` | decodes structure BSPs (clusters and instances via the lightmap BSP data tag's geometry), render models (first permutation per region, nodes, instanced geometry), the scenario's scenery / crates / machines / controls / sky, and the **real collision BSP** (`structure_bsp_tag_resources`: winged-edge surfaces fanned into triangles, with material and surface flags, instanced geometry placed) into one plain `Scene` |
| `blam_texture` | bitmap tags -> mip chains (primary/secondary pages, cube maps, arrays, the 11 formats MCC uses plus BC4), own CPU decoders to RGBA8, and the shader resolver (options, template usages, bitmaps with xforms, blend / alpha test / two-sided, terrain layers) |
| `blam_cli` | `blam info|classes|tags|strings|string-id|dump|geometry|rails|nodes|bitmap|shader|textures` ; `geometry --obj` writes the render geometry and the collision as OBJ (metres, Y up); `bitmap --png` writes mip 0 |
| `halo3` | the viewer: bevy 0.20, one mesh per material with its real colour texture (BC kept compressed), tiling, alpha test and blending; terrain draws its four-layer blend; directional and ambient light, a flying camera (keyboard, mouse, gamepad), collision shell on V, P prints the camera position in Halo units |

Verified against HaloExport's sidecars for Valhalla (`riverworld`) and The Pit (`cyberdyne`):
every triangle count matches exactly (clusters, instances, scenery, crates, machines), bounds
match to the metre, and the collision BSP comes out with the counts the research predicted
(30,560 world triangles on Valhalla). Campaign maps decode all their BSPs (120_halo 11, 100_citadel
15, 030_outskirts 6 with 3.25 M triangles) in under a second each. `cargo test` pins these.

Run it:

```
cargo build
target\debug\blam.exe geometry riverworld --obj out\riverworld.obj
& "target\debug\halo3.exe" riverworld
& "target\debug\halo3.exe" --test-scene
```

`docs/viewer.md` has the controls. Nothing in this folder launches the game.

## What the next session should do, in order

1. **Hear how the viewer playtest went.** Nobody has run `halo3.exe` yet (the rule is the user
   playtests). Likely issues: camera direction signs, mouse grab on Windows, performance on
   campaign maps (all BSPs are drawn at once; `--bsp` selection is only in the CLI so far).
2. **Textures: base colour done (2026-10-28), real shaders next.** `docs/viewer.md` lists what
   is drawn. Next steps, roughly in order of visible gain: detail maps and the four-layer terrain
   blend (an `ExtendedMaterial` with its own fragment shader, `docs/bevy-textures.md` section 10),
   normal maps (Halo's own tangents or `bevy_mikktspace`; BC5 is signed, `flip_normal_map_y`
   unverified), splitting translucent batches per placement for sorting. Ask the user how the
   textured viewer looks first: brightness (all colour maps sampled as sRGB), the dominant-layer
   terrain, and whether anything that should be invisible is drawn.
3. **Skate glue (goal 3).** `docs/skate-interface.md` is the contract. Collision triangles and
   the lip detector input are ready in `Scene::collision` (Halo wu, Z up). Open choices there:
   - link the skate crates as a **path dependency** on `mw2-skate-3-src\src\mashup\skate\crates`
     or **copy** them in: ask the user (the handoff before this one asked the same);
   - the scale constant `k` (true scale vs the Spartan-sized skater IW4L ships, see section 10
     of that doc);
   - the rider: the Spartan biped (`objects\characters\masterchief`) needs skinned vertex
     decoding (`s_skinned_vertex`), its node names mapped onto the 35 skate bones (`rig.rs`
     table), and skinned drawing in bevy.
   The user's converted Skate 3 data is at `D:\GAME ISO AND ROMS\PC - AI MASHUP\mw2 skate 3\skate-data\assets`.
4. **Spawns** are done for multiplayer: every `objects\multi\spawning\respawn_point*` and
   `initial_spawn_point` placement is a `Scene::spawns` entry (position, yaw, kind); Valhalla
   has 107, The Pit a similar number. The viewer starts at the first one. Campaign maps
   start at the scenario's player starting location, found by our own probes
   (`docs/format/zone-sets.md`, which also has zone sets, per-cluster sky and atmosphere,
   trigger volumes and zone-set switch volumes).
5. **Campaign (goal 4), 2026-10-28: 120_halo.** Zone sets switch with PageUp / PageDown,
   `--zone`, or the scenario's switch volumes as you move; only the loaded zone set is on the GPU
   (streamed per BSP in the background, textures decoded with it); the world is chunked and
   culled by frustum, cluster portals (`docs/format/sbsp-portals.md`) and screen size;
   skating swaps the new zone set's collision in without stopping the ride
   (`docs/viewer.md` "Streaming and culling"). Fixed in review: the run's haze (cluster
   atmosphere goes through the BSP's palette: `trench`, not `interior`), the black rider (light
   samples inside triangles), the dark start (auto exposure). Next: playtest the streaming on the
   run; the other campaign maps (010_jungle's BSP 11 still fails with "segment has no page"); a
   real look at the sky from far above the play space (now the trench haze colour, no black).

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
