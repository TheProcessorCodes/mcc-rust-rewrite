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
| `crates/blam_texture` | decodes bitmap tags into mip chains (BCn kept as blocks) and resolves shader tags to their textures, tiling and blend options |
| `crates/blam_cli` | the `blam` tool: list tags, classes and strings, dump tag bytes, decode geometry to OBJ, inspect bitmaps and shaders, write PNGs |
| `crates/halo3` | the viewer: a window, the BSP drawn, a flying camera (`docs/viewer.md`) |
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
& "target\debug\halo3.exe" riverworld
```

A bare map name is looked up in `%HALO3_MAPS%`, or in the default Steam
install when that is not set. `riverworld` is Valhalla, `cyberdyne` The Pit,
`deadlock` High Ground, `zanzibar` Last Resort, `bunkerworld` Standoff,
`sidewinder` Avalanche, `guardian` Guardian.

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
- [x] Dirt bike (built, awaiting a playtest): B rides a stick-model motocross
      bike with dual-stick physics (left stick bars, right stick rider, RT gas,
      LT brake), the Spartan standing on it (`docs/viewer.md` "Dirt bike").
- [x] Guardian (`guardian`; built, awaiting a playtest): 26 clusters,
      471 instances, 258 k triangles, 53 spawns (all on a floor), 14,448 world
      and 97,940 instanced collision triangles, about 3,400 rails. Falling
      into the abyss (the `kill_drop` kill volume, or anywhere far below the
      collision) puts the skater or the bike back where it last stood; the
      jump pads' and minilift's `plasma` glow is drawn. Pinned by
      `cargo test` (our own counts: there is no HaloExport reference).
- [ ] Milestone 3b: feed collision and rails to the skate world.
- [ ] Milestone 4: Skate 3 crates plugged in.
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
