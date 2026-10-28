# Halo 3 water: the `rmw ` shader, animated shader arguments, and where water lives (MCC U13)

What the viewer needs to draw Halo 3's water and to let the skater fall into it: the water
shader's options, maps and parameters, how shader arguments are animated over time (waves,
foam and waterfalls scroll), the bitmaps involved, and whether the water exists anywhere
besides its render geometry.

- Target and conventions as in `shaders.md` (MCC Halo 3 U13, offsets hex, little-endian,
  32-bit tag pointers stored `>> 2`, TagBlock = 12 bytes, TagReference = 16 bytes, DataRef =
  20 bytes `{u32 size, 8 bytes, u32 shifted pointer, 4 bytes}`).
- Verified on `riverworld.map` (Valhalla) with `docs/format/scripts/probe_water.py`
  (read-only, `python -I probe_water.py <map> [--blocks]`), `blam shader`, `blam bitmap` and
  the viewer. No code from other projects was used; field names are ours unless marked as the
  usual community name. V = verified on the bytes, S = consistent with the data but the
  meaning is inferred, ? = not known.

Contents: 1 the water shader, 2 its arguments, 3 animated arguments, 4 the bitmaps,
5 what the viewer draws, 6 water and collision (physics), 7 other maps, 8 open questions.

---

## 1. The `rmw ` shader on Valhalla

`levels\multi\riverworld\shaders\riverworld_water_rough` (tag 11609), definition
`shaders\water`, template `shaders\water_templates\_0_1_1_1_1_0_1_3`. The shader tag is the
ordinary render-method layout (`shaders.md` 2); the water definition has 8 categories
(`shaders.md` 5):

| category | options | Valhalla |
|---|---|---|
| waveshape | default, none, bump | default |
| watercolor | pure, texture | **texture** (`watercolor_texture`; `pure` = the `water_diffuse` colour) |
| reflection | none, static, dynamic, static_ssr | static (the `environment_map` cube) |
| refraction | none, dynamic | dynamic (the scene behind, shifted by the waves) |
| bankalpha | none, depth, paint | depth (fade out where the water is shallow) |
| appearance | default | default |
| global_shape | none, paint, depth | paint (`global_shape_texture`) |
| foam | none, auto, paint, both | both |

Maps (usage, bitmap, xform `scale x y offset x y`):

| usage | bitmap | xform |
|---|---|---|
| `wave_displacement_array` | `rasterizer\water\wave_test1\wave_test1_displ_water` | 1 3 0 0 |
| `global_shape_texture` | `levels\multi\riverworld\bitmaps\riverworld_water_multipurpose` | none (1 1 0 0) |
| `wave_slope_array` | `rasterizer\water\wave_test1\wave_test1_slope_water` | 2 2 0 0 |
| `watercolor_texture` | `levels\multi\riverworld\bitmaps\riverworld_water_diffuse` | none |
| `environment_map` | `levels\multi\riverworld\bitmaps\z_cubemap_temp` | none |
| `foam_texture` | `levels\multi\riverworld\bitmaps\riverworld_water_foam_dif` | 2 1 0 0 |
| `foam_texture_detail` | `levels\multi\riverworld\bitmaps\riverworld_water_foam_dif` | 3 1.5 0 0 |

## 2. Arguments (all 37, in template order)

Texture usages also appear in the argument list (their xform). The scalar arguments are float4
with the value replicated (V):

| argument | value | what it does (S unless noted) |
|---|---|---|
| `slope_range_x`, `slope_range_y` | 0.5, 0.4 | range the U8V8 slope texels decode to |
| `time_warp`, `time_warp_aux` | 0, 0 | animated (3): which slice of the wave arrays, main and detail sample |
| `detail_slope_scale_x/y/z` | 10, 3, 3 | detail wave sample: tiling and strength |
| `detail_slope_steepness` | 0.5 | detail wave strength |
| `watercolor_coefficient` | 1 | multiplies the water colour |
| `reflection_coefficient` | 600 | multiplies the reflection; the cube's texels carry a small alpha (0.01) that looks like an HDR scale, so 600 x alpha is about 7 |
| `sunspot_cut` | 0.01 | sun glint threshold |
| `shadow_intensity_mark` | 0.5 | |
| `refraction_texcoord_shift` | 0.12 | how far the waves shift the refracted image |
| `water_murkiness` | 3 | how fast the bed fades with depth |
| `refraction_extinct_distance` | 1000 | |
| `minimal_wave_disturbance` | 1 | |
| `refraction_depth_dominant_ratio` | 0.95 | |
| `bankalpha_infuence_depth` | 0.22 | depth (wu) over which the banks fade in (spelling as stored) |
| `fresnel_coefficient` | 0.1 | reflectance looking straight down |
| `water_diffuse` | (0.0667, 0.1137, 0.1373, 1) | water colour (the watercolor texture's mean, V) |
| `foam_height`, `foam_pow` | 0.5, 2 | wave height where automatic foam starts, and its curve |
| `displacement_range_x/y/z` | 0.3, 0.2, 0.3 | displacement array decode range (vertex waves) |
| `wave_height`, `wave_height_aux` | 0.5, 0.6 | wave amplitude, main and aux |
| `choppiness_forward/backward/side` | 1, 0.5, 0.4 | |
| `wave_visual_damping_distance` | 8 | wu |
| `waveshape`, `global_shape` | 0, 1 | the option indices again |

## 3. Animated arguments (any shader)

The 132-byte shader property block (`shaders.md` 2.2) has four blocks after TilingData that
`shaders.md` left unread. Water, waterfall and other scrolling shaders fill them:

| offset | entry | what (S) |
|---|---|---|
| `+0x38` | 2 bytes | per entry point (16 on water, 18 on the waterfall): packed start/count into `+0x44` |
| `+0x44` | 6 bytes? | per pass: packed ranges into `+0x50` |
| **`+0x50`** | **4 bytes** | **routing**: `u8 register`, `u8 type` (0x70 on vertex-shader routes, 0x71 on pixel-shader routes, S), **`u8 function index`** into `+0x5C`, **`u8 argument index`** into the template's argument list (V: on Valhalla's water the indices 22 and 23 are exactly `foam_texture` and `foam_texture_detail`, 2 and 4 the wave arrays; on the waterfall 1, 2, 3 are `detail_map`, `detail_map2`, `bump_map`) |
| **`+0x5C`** | **36 bytes** | **functions**, below |

Function entry (36 bytes):

| offset | type | field |
|---|---|---|
| `0x00` | i32 | kind: **0 value** (V: drives `time_warp`), **5 translation x**, **6 translation y** (V: drive xform offsets of texture usages). By the order of the usual community enum, 1 colour, 2 uniform scale, 3 scale x, 4 scale y, 7 frame index, 8 alpha (not seen here, unverified) |
| `0x04` | StringId | input name (empty on every function seen: the input is time) |
| `0x08` | StringId | range name (empty) |
| `0x0C` | f32 | **period**, seconds |
| `0x10` | DataRef | the function bytes: 32 bytes on every Valhalla function, `00 34 00 00`, then f32 `from` at `+4` and f32 `to` at `+8` (0 and +1 or -1), zero after |

Read as a sawtooth: `value = from + (to - from) * fract(t / period)` (S: scrolling an xform
offset from 0 to 1 over the period moves the texture one tile, so the wrap is invisible). The
same entry is routed several times (once per pass and shader stage); dedupe by
`(function, argument)`. `blam_texture::shader` reads these as `Shader::animations`, and
`blam shader` prints them.

Valhalla, `riverworld_water_rough` (V):

| argument | kind | period | from -> to |
|---|---|---|---|
| `time_warp` | value | 0.8 s | 0 -> 1 |
| `wave_slope_array` | translation x / y | 30 s / 20 s | 0 -> -1 / 0 -> 1 |
| `time_warp_aux` | value | 1.2 s | 0 -> 1 |
| `foam_texture` | translation x / y | 3 s / 22 s | 0 -> -1 / 0 -> -1 |
| `foam_texture_detail` | translation x / y | 3 s / 22 s | 0 -> -1 / 0 -> 1 |

`vahalla_waterfall` (rmsh, albedo `two_detail`, `alpha_blend`): `detail_map` translation y,
4 s, 0 -> -1; `detail_map2` translation y, 5 s, 0 -> -1; `bump_map` translation y, 4 s,
0 -> -1. Its template has only the four map xforms as arguments.

## 4. The bitmaps

From `blam bitmap riverworld <tag> --png` (which now also writes all layers side by side):

| bitmap | shape | format | notes |
|---|---|---|---|
| `wave_test1_slope_water` (11636) | 128 x 128 x **16-slice array**, 8 levels | U8V8 (DXGI 51, R8G8_SNORM) | two signed slopes per texel; the 16 slices are frames of the animation (`time_warp` runs through them) |
| `wave_test1_displ_water` (11635) | 128 x 128 x 16 array, 8 levels | A8R8G8B8, linear | displacement for the vertex waves |
| `riverworld_water_diffuse` (11637) | 512 x 1024 | DXT5, xRGB | mean RGB = `water_diffuse` |
| `riverworld_water_multipurpose` (11639) | 512 x 1024 | DXT1 | a painted top-down mask of the whole river and lake: green = water (waves), red = the lake, blue = foam streaks in the rapids |
| `riverworld_water_foam_dif` (11640) | 512 x 512 | DXT5, linear | grey-white foam |
| `z_cubemap_temp` (11638) | 64 x 64 cube, 3 levels | A8R8G8B8, xRGB | dark blue generic scene, alpha about 0.01 everywhere (an HDR scale?); "temp" in the name |

### 4.1 The water mesh's texcoords

The masks (`global_shape_texture`, `watercolor_texture`) are sampled with no xform, so they
expect a texcoord that spans the river layout 0..1. **The water mesh's decoded UV does not**:
on Valhalla it runs u -28.9..94.7, v -49.8..9.1 and is not an affine function of world XY
(a least-squares fit leaves a median error of 2.6 UV units). Sampled at that UV the masks come
out as triangle-shaped patches. The layout coordinate is probably in the water-only vertex
buffers (types 27 and 28, `lightmaps.md` 4), which nothing decodes yet (?). The wave arrays are
sampled in world space instead (5).

## 5. What the viewer draws

`crates/halo3/src/world/water.rs` and `world/shaders/water.wesl`; `docs/viewer.md` "Water".

- Waves: the slope array, at Halo world XY x 0.25 (`WAVE_UV_PER_WU`, by eye) x the
  `wave_slope_array` xform, scrolled by its functions; the slice from `time_warp`
  (interpolated between neighbours); a second detail sample at 3x the tiling stepped by
  `time_warp_aux` and weighted by `detail_slope_steepness`. Slopes x `slope_range` x
  `wave_height` x 0.6 (`WAVE_SLOPE_SCALE`: at face value the river turns milky).
- Reflection: Schlick fresnel with `fresnel_coefficient` as F0; the sky's own scattering (the
  `skya` setting used for the sky, `sky-and-atmosphere.md`) along the reflected ray, plus a sun
  glint. The cube map is not used (above).
- Refraction / murk: no scene-colour copy; the water is blended over what is behind it. The
  depth prepass gives the path length through the water; `T = exp(-0.5 x murkiness x path)`
  of the bed shows through, the rest is the water colour (`water_diffuse`) lit by the
  lightmap. Banks fade over `bankalpha_infuence_depth`.
- Foam (`auto`): where the waves are steeper than `foam_height`, shaped by `foam_pow`, the foam
  texture and its detail scrolled by their functions. Painted foam needs the mask texcoord.
- Flowing shaders: an alpha-blended `two_detail` `rmsh` whose detail maps scroll is drawn by
  the same material in flow mode: `base x detail x detail2 x 4.595^2`, alpha the product,
  every xform offset moved by its function, lit by the lightmap.

## 6. Water and collision (physics)

- **The water surface is not collision.** The structure BSP's collision materials
  (sbsp `+0x28`, 24-byte entries: shader TagReference, then `i16` global material at `+0x10`)
  on Valhalla are 37 shaders, **none of class `rmw `** (V). The river bed is
  `riverworld_ground_water` (rmtr, entry 31, global material 0x30) and the lake bed
  `ground_lake_deep` (rmsh, entry 24, 0x67): both are solid collision. So the skate engine,
  fed the collision BSP, already rides on the river bed and drops to the lake floor.
- A scan of every tag block in the sbsp's first 0x3A0 bytes for a reference to the water
  shader finds it only in the render `Shaders` block (`+0xD0`, entry 19) (V,
  `probe_water.py`). No water-volume block is visible in the sbsp tag on Valhalla; the render
  submesh flag bit 3 (`IsWaterSurface`, `sbsp-render-geometry.md`) and the water vertex buffers
  are the only water-specific data found. The `skya` atmosphere has underwater settings
  (`water_physics00..`, `sky-and-atmosphere.md`) but no geometry.
- So the viewer takes the water volume from the render triangles drawn with an `rmw ` shader:
  a column through a point gives the water surface above it, and the collision below that the
  bed. Valhalla's numbers (from the skate test, 6): the river is ankle deep along most of its
  run (0.03 wu where the board rolled in), the lake at (70, 20) has its surface at z -0.37 and
  its bed 6.18 wu (19 m) below. The water surface spans z -0.4..0.8 over x -40.9..157.5,
  y -150.4..92.5 (2,607 triangles).

What the skater does (`crates/halo3/src/swim.rs`):

| case | rule |
|---|---|
| over dry land, wheels within 0.25 wu of the collision below | remember the spot and heading as the last safe one |
| into water | a splash; the depth is logged |
| shallow (surface minus bed under 0.45 wu, about 1.4 m) | rides the bed (the engine already does) |
| deep, board under the surface for 0.6 s | sinks: splash, then `Session::activate` at the last safe spot (or the start spawn if there is none) |

No drag in shallow water: the skate engine takes no outside forces through its API.

## 7. Other maps

`probe_water.py` lists the `rmw ` tags (and the sbsp Shaders entry that uses them):

| map | water shaders |
|---|---|
| riverworld (Valhalla) | `riverworld_water_rough` |
| docks (Longshore) | `levels\dlc\docks\shaders\water\dock_water_1`, `dock_water_2` |
| zanzibar (Last Resort) | `zanzibar_water_awesome` |
| deadlock (High Ground) | `deadlock_water_awesome` |
| 030_outskirts | `levels\solo\030_outskirts\shaders\natural\water_030` |
| 050_floodvoi | `voi_flood_water`, `voi_flood_small_water` |
| s3d_waterfall (Waterfall), 120_halo | none (s3d_waterfall's falls are ordinary shaders) |

## 8. Open questions

- The texcoord the painted masks use (4.1), so `global_shape`, `watercolor = texture` and
  painted foam can be drawn.
- The wave texcoord scale in world units (picked by eye) and how `detail_slope_scale_x/y/z`
  and the displacement array combine; the vertex displacement is not drawn.
- The function bytes beyond `from`/`to` (the `0x34` flag byte and the zero tail): other
  function shapes (sine, curves) would read differently.
- `+0x38` / `+0x44` packing.
- Where Halo 3 keeps water volumes for physics, if anywhere other than the render geometry.
