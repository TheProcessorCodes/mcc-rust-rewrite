# The viewer (`halo3`)

Opens a map and lets you fly through it: goal 2 of the handoff, the noclip.

```
cargo build -p halo3
& "target\debug\halo3.exe" riverworld
& "target\debug\halo3.exe" --test-scene
```

A bare map name is looked up in `%HALO3_MAPS%`, or in the default Steam MCC
install. `--test-scene` shows a made-up room (floor, ramp, boxes) for checking
the camera without any map decoding.

Headless pictures (no window; `shot.rs`):

```
target\debug\halo3.exe riverworld --shot out\shots --view spawn --view overview --view spawn12 --view 10,-20,5,90,-10
```

A view is `x,y,z,yaw,pitch` in Halo units and degrees, `spawn` / `spawnN` the start (or Nth)
spawn point at eye height, `overview` high over a corner of the spawn points looking at their
middle.

## Controls

| input | effect |
|---|---|
| W A S D | move |
| Space / C, or E / Q | up / down |
| Shift | fast (6x) |
| Alt | slow |
| mouse | look around (whenever the window has focus) |
| Escape | give the cursor back; click the window to take it again |
| P | print the camera's position in Halo units to the console |
| V | show or hide the game's real collision: green shell for surfaces, red for invisible walls |
| J | skate / fly (`crates/halo3/src/skate.rs`) |
| B, or gamepad View / Back | dirt bike / fly (see "Dirt bike" below) |
| PageDown / PageUp | next / previous zone set (campaign maps) |
| Home | load every BSP at once |
| M | music on / off (the map's showpiece: the Warthog run on 120_halo); Shift+M next track |
| N | music: switch to the alternate loop and back (at the next loop boundary) |
| K | mute / unmute |
| `-` / `=` | sound volume down / up (10 % steps) |
| gamepad left stick | move |
| gamepad right stick | look |
| gamepad right / left trigger | up / down |
| gamepad A | fast |

Normal speed is 25 m/s. The camera starts at the map's first respawn point, at
Spartan eye height, facing the way the marker faces. Campaign maps have no
respawn points: they start at the scenario's first player starting location
(`docs/format/zone-sets.md` 4). A map with neither starts above the middle of
its world.

## Zone sets (campaign maps)

A campaign map is several structure BSPs, and the game loads a few at a time:
a **zone set** (`docs/format/zone-sets.md`). 120_halo has 11 BSPs and 8 zone
sets; the Warthog run is zone sets 4-7 (`120_04_100_110`, `120_100_110`,
`120_110_120`, `120_120_130`). Some BSPs are two versions of one place
(120_halo's `010` and `070`) and overlap if loaded together.

The viewer decodes every BSP's geometry at start (120_halo: about 1 s,
2.9 M triangles) but puts only the loaded zone set on the GPU, with the
textures and lightmaps it uses; see "Streaming and culling" below. What is
loaded first:

```
& "target\debug\halo3.exe" 120_halo                 # the zone set the start is in (zone set 0)
& "target\debug\halo3.exe" 120_halo --zone 6        # by index ...
& "target\debug\halo3.exe" 120_halo --zone 120_110_120   # ... or by name
& "target\debug\halo3.exe" 120_halo --bsps 7,8,9    # any BSPs (indices in the scenario)
& "target\debug\halo3.exe" 120_halo --all-bsps      # everything (overlaps)
```

The console prints the zone sets and BSPs at start. As you fly or skate
through the scenario's zone-set switch volumes the zone set follows, as in the
game: entering a `begin_zone_set:` volume builds the next zone set's BSPs in
the background, entering a `zone_set:` volume switches to it
(`HALO3_ZONE_TRIGGERS=0` turns this off; it is off for `--shot` unless
`HALO3_ZONE_TRIGGERS=1`, and whenever `--bsps`, `--all-bsps` or Home chose
BSPs by hand). PageDown / PageUp still step by hand.

Objects follow their origin BSP (objects with none always show); a sky shows
only while it is the sky of the camera's cluster, so 120_halo switches from the
snowfield sky to the Warthog run's ring sky (`sky_03`) as you cross over. The
haze follows the camera's cluster too (found through the BSP's collision tree,
`docs/format/sbsp-portals.md`): each cluster names an entry of its BSP's
atmosphere palette, which names one of the atmosphere tag's settings (snow at
the crash site, `interior` inside, the dim red `trench` air of the Warthog
run), used for the world fog, the sky's scattering and the clear colour;
outside every cluster the setting most loaded clusters name.

Skating uses the collision of the loaded BSPs only. Changing zone set while
riding does not stop the ride: the new zone set's collision and rails are built
on a helper thread and swapped into the running session (title bar: "skate:
building the new zone set's collision", then "skate: new zone set"; about 3.6 s
for the Warthog run). Before the first ride the world is built as before. Rails inside the
scenario's kill volumes are dropped. 120_halo's Warthog run (zone set 6): the
skate world is ready about 15 s after start (520 k triangles, 6,300 rails found
in 4.6 s, the skater's own data 6 s). Rails over all 11 BSPs would take 16 s
and give 58,600, most of them ledges under the run the player never reaches.

`world/zones.rs` is the API: `LoadedBsps` (wanted mask, shown mask, zone set,
sky), `load_zone_set`, `load_bsps`. `HALO3_SKY=0` hides the sky, for telling
sky pieces from the world. `HALO3_ZONE_SWITCH_TEST=N@S` switches to zone set N
after S seconds of skating (or of running), for testing without a keyboard.

## Streaming and culling

`world/build.rs`, `world/stream.rs`, `world/culling.rs`.

- **Chunks.** The world is not one mesh per material any more: each
  structure BSP is cut into chunks per material and lighting mode, cluster
  triangles by 64 wu cells, instances and objects by cell, home cluster and
  size class (bounding radius under 1.5 wu, under 6, larger). Every chunk
  has its own bounds, so bevy's frustum culling skips what is off screen.
  Chunks of one material share one material (and its textures).
- **Units and streaming.** A unit is one BSP with its instances and the
  objects whose origin it is, or the global unit (sky, objects of no BSP).
  Only the loaded zone set's units are on the GPU. A unit that is needed is
  built on a background thread, decoding the bitmaps and lightmap its
  materials use from the map file, and uploaded about 400 k triangles a frame.
  The old zone set stays on screen until the new one is complete; then its
  entities are despawned, which frees their meshes, materials and (when
  nothing else uses them) textures. 120_halo: building BSP 120 takes 1.1-1.4 s
  in the background, the switch from zone set 5 to 6 drops BSP 100, and GPU
  textures go from 190 to 206 MiB (all 11 BSPs: 472 MiB). The CPU no longer
  keeps every decoded bitmap (396 MiB on 120_halo): only the lightmaps
  (110 MiB) are decoded at start, for the surface lights, and dropped after
  the first upload.
- **Portals.** Each frame the camera's cluster is found in every shown BSP
  and a flood through the cluster portals, keeping the screen rectangle each
  portal covers, marks the clusters that can be seen; chunks of the others
  are hidden (`docs/format/sbsp-portals.md` 4). A camera in no cluster (in a
  wall, outside the level) sees the whole BSP. `HALO3_PORTALS=0` turns it off.
- **Size.** Chunks of props whose largest member covers less than 1.5 pixels
  (bounding radius, 720-line view) are hidden, as are BSP instances smaller
  than their own fade size (`docs/format/lod.md`; Halo 3 has no other LOD or
  draw-distance data). The sky is never culled but by the frustum.
  `HALO3_MIN_PIXELS=0` turns the size rule off.

`--bench N` times N frames before each `--shot` picture (waiting for the GPU
every frame) and logs what was drawn and the portal state. Measured
2026-10-28 at 1280x720 (frame ms mean; meshes drawn / entities; triangles
drawn):

| view | before (one mesh per material) | after |
|---|---|---|
| 120_halo `--zone 6`, run tunnel (410, 590, -45) | 3.01 ms; 83 / 567; 944 k | 3.21 ms; 1,134 / 3,171; 861 k (23 of 48 clusters) |
| 120_halo `--zone 6`, over the run (300, 400, 150) | 3.85 ms; 118 / 567; 1.31 M | 3.89 ms; 1,684 / 3,171; 998 k (4 of 29 clusters) |
| 120_halo `--all-bsps`, start | 8.47 ms; 392 / 567; 2.73 M | 7.53 ms; 3,393 / 6,530; 1.35 M |
| riverworld, first spawn | 4.61 ms; 79 / 89; 148 k | 4.57 ms; 320 / 522; 129 k (13 of 26 clusters) |
| riverworld, overview | 4.54 ms; 82 / 89; 148 k | 4.44 ms; 448 / 522; 144 k (all clusters) |

On this GPU the frame is bound by pixel shading (water, terrain blends,
haze), not triangles, so halving the triangles barely moves the frame time
except with every BSP loaded; the wins are in GPU memory (zone set 6: 227 MiB
of textures uploaded instead of 383) and on slower GPUs. Pictures with and
without the culling match (`HALO3_PORTALS=0 HALO3_MIN_PIXELS=0`; under 0.1 % of
pixels differ, a few far props).

## Frame and units

Halo is Z up and measures in world units of ten feet (3.048 m). The renderer
is Y up in metres: `render = 3.048 * (hx, hz, -hy)`. That is a rotation, not a
mirror, and the same mapping HaloExport uses for its glTF files, so numbers
from the Reflex pipeline sidecars carry over.

## What is drawn

Chunks of one material and lighting mode per place (see "Streaming and culling"), every placement baked into
world space; `Sky` placements get their own shader (see "Sky and haze" below).

Before the window opens, `textures.rs` resolves every material's shader tag
(`blam_texture::shader::resolve`) and decodes the bitmaps it needs, each
bitmap once; `world.rs` turns them into one shared bevy `Image` per bitmap.

| shader | drawn as |
|---|---|
| `rmsh`, `rmhg`, `rmd ` with a `base_map` | the base map, tinted by `albedo_color`, tiled by the base map's xform through `uv_transform` (the mesh keeps the raw UVs) |
| the rest of an ordinary shader's material | detail map, bump map (and bump detail), specular, environment reflection and self-illumination, in the lighting shader: see "Materials" below |
| foliage (`rmfl`) and any `alpha_test simple` shader | `AlphaMode::Mask(0.5)` on the base map's alpha, or on `alpha_test_map` when the shader has its own (grates; Last Resort's palm fronds cut by their bump map's alpha), both sides drawn. The lighting shader is also the depth prepass's fragment shader, so maps with water (which get a depth prepass) cut the same way there; with bevy's own prepass the whole frond cards hid what was behind them and showed the haze colour |
| `blend_mode` alpha_blend / pre_multiplied_alpha / additive / multiply | `Blend` / `Premultiplied` / `Add` / `Multiply`, no shadows. Multiply decals (High Ground's `medical_decals` on the medical crates) are drawn unlit, unexposed and unfogged: they scale the lit surface under them by their own colour. Lit and exposed, their white background times the exposure brightened what lay under them and washed the red crosses out |
| opaque shaders | `Opaque`: the base map's alpha is a specular mask, not transparency, and is ignored |
| terrain (`rmtr`) | the **four-layer blend**, in the lighting shader (see "Terrain" below). If the blend map or every layer fails to decode, or with `HALO3_TERRAIN_BLEND=0`, the single layer the blend map weights most over the whole map, as HaloExport does |
| water (`rmw `) | its own water material: animated waves, sky reflection, murk and foam (see "Water" below) |
| alpha-blended `two_detail` shaders whose detail maps scroll (Valhalla's waterfall) | the water material's flow mode: the maps scroll by the shader's own functions (see "Water") |
| spawn markers: placements of `respawn_point`, `*_initial_spawn_point`, `*_respawn_zone` objects, and the `objects\multi\shaders\spawn_point*` and `shaders\invalid` shaders | not drawn: Forge shows them, a game never does (they were the black discs and white plasma columns on every map) |
| constant-colour shaders (no base map) | flat `albedo_color` |
| additive glows without a base map | `self_illum_map` tinted by `self_illum_color`, added |
| `self_illumination plasma` (Guardian's jump pads and minilift) | `alpha_mask_map`'s **alpha** (its colour channels are black) times the mean of `color_medium`, `color_wide` and `color_sharp` times `self_illum_intensity`, added; the crawling noise bands are not drawn. Before, these had no `self_illum_map` and drew nothing |
| "special" shaders (path segment `invisible`, `portal`, `sky_portal`, `z_only`, `black`; effect classes) and alpha-tested shaders without a colour map (light quads) | not drawn |
| anything that fails to resolve or decode | the old steady hashed colour, with a message on the console |

Textures: BC1/BC2/BC3 go to the GPU still compressed when the device has
BC support (desktop DX12 / Vulkan does; the log line `textures: ...` says
which), A8R8G8B8 as BGRA, every other format expanded to RGBA8 on the CPU.
Colour maps are always sampled as sRGB, whatever the bitmap's curve byte
says: the "linear"-curve ones (most of Valhalla) only look right that way.
Repeat addressing, trilinear filtering, 8x anisotropy. UV V runs down the
image as in D3D and wgpu, so there is no flip (pinned by the
`riverworld_v_runs_down` test in `blam_texture`).

Valhalla: 86 materials, 79 with a texture, 58 images, 30.8 MiB on the GPU
(114.8 MiB if everything were expanded to RGBA8); loading the textures adds
about one second.

### Terrain

Terrain shaders (`rmtr`) mix up to four material layers using a blend map
(`docs/format/shaders.md` 7.5). The StandardMaterial gets no texture, only
the `global_albedo_tint`; the lightmap shader (`world/shaders/lightmap.wesl`,
`terrain_albedo`) builds the albedo per pixel and then lights it as usual:

- the blend map is sampled at the mesh UV through its own xform (1, 1 on
  every terrain seen). It is uploaded **linear** (its texels are weights),
  not sRGB;
- weight of layer N = blend map channel N (red, green, blue, alpha). A layer
  whose `material_N` is `off` gets weight 0. The weights are then divided by
  their sum. The blend maps are already authored to sum to about 1, so this
  changes little; `HALO3_TERRAIN_RAW=1` skips it so the two can be compared;
- layer N = `base_map_m_N(uv * xform) * detail_map_m_N(uv * xform) *
  4.595`, both sampled as sRGB like every colour map. 4.595 = 2^2.2
  (`DETAIL_MULTIPLIER` in `world/lighting.rs`) makes a detail map's mid grey
  neutral: `default_detail` is exactly 0x80 grey, and 0.502^2.2 * 4.595 = 1.0.

Valhalla has 6 terrain materials, The Pit 3. Valhalla's ground now shows
green grass, dirt edges, rock and snow where it was flat grey before (the
grass colour lives in `detail_map_m_1`; the base is a grey overlay). Uploading
the extra layers raises Valhalla's GPU texture memory from 30.6 to 45.9 MiB.

A terrain whose template has no blend map at all (Sandbox's one-layer
`sand_tiling02`: `blend_map` is then a parameter, not a texture) weighs its
layers alike instead of falling back to the bare overlay, which drew white.
Each layer's `bump_map_m_N` is blended by the same weights, and the
`diffuse_plus_specular` layers get a phong lobe (coefficient, power, tint and
analytical contribution per layer, masked by the layer's base alpha). Not
done for terrain: `detail_bump_m_N`, area and environment specular, and
`dynamic_morph` blending (no map uses it here).

### Materials

Ordinary shaders (`rmsh` and kin) get the rest of their material in the same
lighting shader (`docs/format/shaders.md` section 11 has the parameters and the
formulas):

- **detail map**: `base * detail * 4.595`, like terrain;
- **bump map**: Halo's own tangent and binormal per vertex (decoded in
  `blam_geometry::vertex`), the normal map's x and y (BC5 signed, uploaded
  still compressed as `Bc5RgSnorm`; U8V8 and others expanded to RGBA8), z
  rebuilt; `bump_detail_map` adds its x and y times `bump_detail_coefficient`.
  The bumped normal feeds the lightmap's diffuse and the specular;
- **specular**: the lightmap's dominant light (direction and intensity) as
  the one light: two-lobe phong (normal and glancing powers and tints blended
  by `fresnel_curve_steepness`), cook-torrance (power from `roughness`, Schlick
  fresnel from `fresnel_color`), glass; times `specular_coefficient`,
  `analytical_specular_contribution` and the specular mask (base alpha or
  `specular_mask_texture`). The SH towards the reflection gives the area
  specular;
- **environment map**: `dynamic` (the usual, a runtime cube map) and
  `per_pixel` are both stood in for by the lightmap's SH (without the
  dominant light) towards the reflection, times `env_tint_color` and
  `environment_map_specular_contribution`; the cube bitmaps are not sampled;
- **self-illumination**: `simple` and kin (`self_illum_map * self_illum_color *
  self_illum_intensity`), `illum_detail`, `from_diffuse`,
  `self_illum_times_diffuse`, `3_channel_self_illum`; exposed like the rest.

Bump and specular need the per-pixel lightmap (BSP clusters and most
instances); objects lit per vertex keep flat normals and get only the
reflection, with their vertex irradiance standing in for the surroundings
(that is what turned The Construct's and The Citadel's black Forerunner
blocks into the reflective panels they are: a near-black albedo under a
strong specular mask).
`HALO3_MATERIALS=0` turns all of this off (base colour and lightmap only, as
before), `HALO3_SPECULAR` scales the specular (0 turns it off) and
`HALO3_SELF_ILLUM` the self-illumination, for comparisons.

Not yet: real cube-map reflections, parallax, `two_detail`'s second detail map, change colours, decal
bump. Translucent materials are still one mesh per material, so two
overlapping translucent materials can sort wrongly.

### Water

`world/water.rs` and `world/shaders/water.wesl`; the tag layout and the evidence are in
`docs/format/water.md`. `world.rs` tags every world batch with its material
(`BatchMaterial`); the water module swaps the material of `rmw ` batches and of flowing
shaders, keeping their lightmap, and gives the camera a depth prepass when the map has water.

- **Waves**: the shader's wave slope array (16 slices of signed slopes) sampled in world
  space, the slice stepped by the shader's `time_warp` function (0.8 s a cycle on Valhalla),
  scrolled by its own translation functions, plus a finer detail sample on `time_warp_aux`.
- **Reflection**: Schlick fresnel with the shader's `fresnel_coefficient`; the sky's own
  scattering (the map's atmosphere, as the sky dome uses) along the reflected ray, and a sun
  glint. Looking down you see into the water, at grazing angles the sky.
- **Murk and shores**: the depth prepass gives how much water lies between the surface and
  the ground behind it; the bed shows through shallow water and fades with
  `water_murkiness`, the water colour (`water_diffuse`, lit by the lightmap) takes its place;
  the banks fade out over `bankalpha_infuence_depth`.
- **Foam**: on steep waves (`foam auto`), the foam texture scrolled by its functions.
- **Waterfalls**: an alpha-blended `two_detail` shader whose detail maps scroll (Valhalla's
  `vahalla_waterfall`) is drawn as base x detail x detail2 with the xforms moving, lit by the
  lightmap: the water runs down the fall.
- `HALO3_WATER_OFF=<bits>` turns parts off for comparison (1 waves, 2 foam, 64 lightmap,
  128 sky reflection; see `WaterShading::params3`). `HALO3_WATER_MASKS=1` samples the painted
  masks (shape, watercolor, painted foam) at the mesh UV, which is the wrong texcoord: they
  come out in triangle patches (water.md 4.1), so they are off by default.

**Skating and water** (`swim.rs`, `docs/format/water.md` 6). Halo's water surface is not in
the collision, so the board rides the river bed and drops to the lake floor on its own. On top
of that, while skating: the last spot where the board stood on dry ground is remembered; going
into water throws a splash (small white droplets) and logs the depth; in water deeper than
0.45 wu (about 1.4 m) the skater sinks for 0.6 s and is put back at the last dry spot (the start
spawn if there was none) through `Session::activate`. Rails more than that deep under a water
surface are dropped (`rails::out_of_deep_water`; Last Resort's sea bed had a 108 wu one). Shallow water is ridden on its bed, with
no drag (the engine takes no outside forces). The fly camera passes through water. Log lines
start with `water:`.

For testing without a pad, `HALO3_SWIM_TEST=x,y,z` throws the skater to that Halo point once it
has stood on dry ground for a second, and `HALO3_SKATE_FOR=<s>` rides that long before the
`--skate` pictures:

```
$env:HALO3_SWIM_TEST="70,20,1.0"; $env:HALO3_SKATE_FOR="8"
& "target\debug\halo3.exe" riverworld --shot out\shots\lake --skate --view "79.76,-56.73,1.5,-88,0"
```

**Falling out of the map** (`falls.rs`). Guardian floats over a misty abyss. The scenario
puts a kill volume, `kill_drop`, under the walkways (z -0.9 to 13.7; the lowest floor is at
15.8). Any `kill*` / `*_kill*` trigger volume counts, and so does anything more than 20 wu
under the lowest collision triangle, so nothing falls for ever on maps without one. The
skater who falls in is put back at the same last safe spot as after sinking (the start spawn
if there is none yet; log lines start with `fall:`). The bike is put back where it last rode
well, as after a crash. The start prints `kill volumes: N; out of the world below z ...`.

High Ground's cove (`deadlock_water_awesome`, a flat sheet at z 4.41 over x -37.0..15.1,
y -67.8..-25.5) ends in a straight hard edge out at sea, where the sky model's far sea (`lake`,
an opaque `rmsh` at z about 2.9-3.7, lit like the rest of the sky) takes over about 1 wu lower and
greener. The game probably hides that seam with the shader's painted bank alpha (`bankalpha paint`),
which needs the mask texcoord (`docs/format/water.md` 4.1); the depth fade only fades the beach
side. Skating: the beach spawns stand in ankle-deep water (0.2 wu); out in the cove the bed is
1.5 wu down and the skater sinks; the `kill_water2` kill volume lies under the sea bed.

Not done: refraction (no copy of the scene behind, so no wave distortion of the bed), the
static cube reflection, vertex wave displacement, painted foam, `global_shape` and the
watercolor texture (they need the mask texcoord), water seen from below.

## Lighting

There are no bevy lights. The world is lit by Halo 3's own baked lighting
(`docs/format/lightmaps.md`, section 8 has the details), through
`ExtendedMaterial<StandardMaterial, HaloLightmap>` (`world/lighting.rs`,
fragment shader `world/shaders/lightmap.wesl`, embedded in the binary). The
StandardMaterial part still gives the albedo (texture, tint, tiling, alpha
mask / blend); the shader multiplies it by Halo's diffuse irradiance, then
by the exposure, then bevy's fog and tonemapping. Batches are split by how
they are lit as well as by material.

| what | lit by |
|---|---|
| BSP clusters, most instanced geometry | the BSP's per-pixel lightmap: two BC3 texture arrays sampled in the shader at the lightmap texcoord (`UV_1`) with the surface normal. Cliff, tree and building shadows on the ground come from here |
| instances with static per-vertex lighting | their own coefficients, turned into irradiance per vertex on the CPU (vertex colour) |
| instances with a light probe | the probe (L0 and L1 terms plus its dominant light), per vertex on the CPU |
| scenery, crates, machines, controls | the lightmap at the lit surface nearest the bottom centre of the object (samples inside every per-pixel lit cluster and instance triangle, about 3 wu apart; up-facing ground under the point counts as nearer than walls and ceilings), per vertex with each vertex's normal (an approximation: the scenario's own scenery/machine probes are not matched up yet). The rider is lit the same way |
| the sky | its own shader and the map's atmosphere ("Sky and haze" below) |
| maps with no lightmap, `--test-scene` | a fixed sunny Valhalla texel (`lighting::default_light`) |

Exposure comes from the map's camera fx (`cfxs +0x18`, an exposure in stops;
`docs/format/sky-and-atmosphere.md` section 5): `4 * 2^(0.6 * (stops +
4.8365))`, so Valhalla keeps the x4 picked against in-game screenshots and
the other maps move from it by 60 % of their difference in stops (Standoff
x10.5, Avalanche x8.1, The Pit x7.5, Narrows x3.65). Then, standing in for
Halo's auto exposure, the result is held so that the map's mean lit-surface
irradiance (every 25th BSP cluster vertex, luminance) comes out at most 3.0
(`MAX_EXPOSED_MEAN`; Valhalla is at 1.2): only Sandbox (mean 1.23, x2.45)
and Sandtrap (1.15, x2.6) are held back. The log prints it (`camera fx ...:
exposure -2.51 stops, mean irradiance 0.101 -> x10.51`). `HALO3_EXPOSURE` sets a fixed
multiplier, `HALO3_EXPOSURE_SLOPE` the 0.6. Not done: Halo's own auto

**Auto exposure** (a stand-in for Halo's): in a place more than 4 stops darker than the map's
mean lit surface the picture is brightened, by the darkness beyond those 4 stops and at most the
width of the camera fx's auto range (`+0x10..+0x14`; Valhalla 0..3, 120_halo 0..4, capped at 3), through bevy's
colour grading so world, sky, haze and rider move together. The place's light is the lit surface
nearest the camera averaged over six directions; it adapts over about a second, snaps when the
camera jumps, and never darkens. 120_halo's start, inside `120_bsp_010` (whose lightmap is about a
quarter as bright as the outdoor BSPs'), gets +2.7 stops; Valhalla's shaded first spawn (3.6 stops under) nothing. `HALO3_AUTO_EXPOSURE=0` turns it off.
exposure and tonemap curve, the probes' L2 terms.

The log says how each placement was lit, e.g. Valhalla:
`lit placements: [("nearest surface", 401), ("per-pixel", 207),
("per-vertex", 58), ("probe", 18), ("sky", 10)]`.

## Sky and haze

`world/sky.rs` and `world/shaders/sky.wesl`, from the map's `skya` atmosphere tag
(`docs/format/sky-and-atmosphere.md` has the layout and the evidence).

- **Sky placements** keep their StandardMaterial for blending and culling but are drawn by
  `SkyMaterial`: the shader's `albedo` option combines base and detail maps the way Halo does
  (`default`, `detail_blend`, `three_detail_blend`, `two_detail`, `two_detail_overlay`,
  `two_detail_black_point`, `constant_color`), adds self-illumination (stars, the sun disk),
  multiplies lit surfaces by a fixed sunlit irradiance, and then runs Halo 3's light scattering
  along the ray from the camera: `colour * extinction + inscatter`, Rayleigh and Mie with
  exponential height falloff, using the setting flagged for the sky (`haze_skydome` on Valhalla).
  The nearly black sky dome comes out blue from that, as in the game. Additive layers only get
  the extinction. Sky bitmaps are sampled linear or sRGB as their DXGI format says.
  `self_illumination from_diffuse` makes the albedo itself glow (there is no illumination map to
  sample), and a halogram's `edge_fade simple` runs the colour from `edge_fade_edge_tint` where the
  surface is seen edge-on to `edge_fade_center_tint` face-on, by `|n.v|^edge_fade_power`. Last
  Resort's foam rings round the distant islands (`island_water_edges`, additive, x2.75) drew as
  solid white slabs on the sea without these two.
- Alpha-tested sky shaders are cut by the albedo's alpha in the sky shader, and keep their base map
  on the StandardMaterial so bevy's depth prepass cuts them too. High Ground's `water_edge_breakup`
  cards (alpha mean 0.14) round the cove drew as pale slabs along the edge of the sea.
- **The world** gets bevy's `DistanceFog` (atmospheric falloff) from the level setting
  (`haze_level`): per-channel extinction and the in-scattered colour, recomputed every frame for
  the camera's height, since the haze thins with height (Mie scale height 6 wu over a sea level at
  z 0 on Valhalla). It is applied in the lightmap shader's post-processing call, so bluish
  distance haze needs no change there.
- **Clear colour**: a long level ray through the sky's air at the camera's height, for the
  places below the dome's rim with no sky geometry (only seen out of bounds).
- **Campaign clusters** name their own setting (see "Zone sets" above); when the camera is in
  one, that setting replaces both the level and the sky setting. Lit sky surfaces are scaled by
  their shader's `diffuse_coefficient` (120_halo's ring plates: 0.3).
- `HALO3_HAZE` scales the world haze's density (`0` turns it off) for comparisons.
- Below a setting's sea level the density stays at the sea level's (world haze and sky alike):
  Sandtrap plays about 20 wu under its sea level, where the exponential falloff made the haze
  e^10 times thicker and every ground-level view black.

The setting's distance bias is applied to the world haze too (no haze in the first 15 wu on
Valhalla, 100 wu on the Warthog run): bevy's fog has no start, so the start is passed to the world
and water shaders in the fog's unused directional-light exponent.

Not done: the world haze uses one density for the whole ray (the eye's), so looking down from a
height onto the lake under-hazes it; the sun-angle dependence of the haze colour is not applied
to the world; `desaturation` (used by The Pit, 0.2) is ignored; Halo's real sky lighting and
exposure curve is not decoded (the `cfxs` exposure is, see "Lighting"); no sun glare or bloom; the sky does not move with the camera
(it is at true size, about 7,500 wu out, which is what the haze distances need).

## Dirt bike

B (or the controller's View / Back button) drops a dirt bike under the camera
with the Spartan standing on it; B again flies. Code: `crates/halo3/src/bike/`
(`physics.rs` the bike, `pose.rs` the rider, `mod.rs` controls, camera and the
stick model). Dual-stick riding in the style of MX vs ATV Reflex:

| controller | keyboard | effect |
|---|---|---|
| left stick X | A / D | handlebars (less lock at speed); the bike leans into the turn itself |
| right stick | arrow keys | rider: forward / back moves his weight (front down, wheelies); left / right tightens the turn; in the air it pitches, leans and whips the bike |
| RT | W | throttle: one long gear, 7 m/s² linear acceleration to 90 km/h |
| LT | S | brake (65 % front) |
| A (hold) | Space (hold) | back up, up to 3 m/s |
| Y | R | stand the bike back up |

Hitting the bars or the rider's head, or landing upside down, is a crash: two
seconds later the bike comes back where it last rode well. Falling into a kill
volume or out of the world brings it back there at once (see "Falling out of
the map" above). The rider is one
stiff stance (attack position) kept on the pegs and grips by IK. The bike is a
stick model of unlit cylinders for now. It rides on the loaded zone set's
collision. Tuning constants are at the top of `physics.rs`.

Headless check: `halo3 riverworld --shot DIR --bike --view spawn50` rides from
spawn 50 holding `HALO3_BIKE_THROTTLE` (default 0.4) for `HALO3_SKATE_FOR`
seconds, then takes the rider pictures.

## Sound

`crates/halo3/src/audio/` plays Halo 3's own sound from the user's install: the
`snd!` / `lsnd` / `ssce` tags of the map (`crates/blam_sound`, `docs/format/sound.md`) and
the samples of MCC's FMOD bank `halo3\fmod\pc\sfx.fsb` (Vorbis, decoded with lewton;
the Vorbis setup headers are rebuilt from the user's `halo3.dll`). Nothing is copied into
this repo. `HALO3_AUDIO=0` turns sound off; `--shot` never opens the sound device.

- **Mixer** (`mixer.rs`): one endless 48 kHz stereo source handed to bevy_audio, into which
  every voice is mixed, with its own gains, pitch (playback rate) and a soft limiter.
  A voice plays a queue of clips, then random permutations of its loop forever, the way
  Halo loops a looping sound's permutations; loop permutations are cut at the bank's loop
  end. Decoding runs on a worker thread as clips are needed (a loop starts once all its
  permutations are in); multichannel samples (120_halo's quad wind) are folded to stereo.
- **Background per cluster**: the camera's cluster (through the collision tree of the
  loaded BSPs) names an entry of its BSP's acoustics palette, whose background `lsnd`
  plays non-positional at its track gain, cross-fading over about a second when the entry
  changes (Valhalla: `halo_ext` outside, `riverworld_interior` in the bases). Outside every
  cluster the last background stays.
- **Details**: each background's detail sounds (birds, gusts, cicadas) fire at random
  within their period, at a random yaw, pitch and distance around the listener; mono ones
  are panned there, stereo ones play as they are.
- **Sound scenery**: every placement of the scenario's sound scenery whose origin BSP is
  loaded plays its `lsnd` loop at its position when the camera comes within its far
  distance (the placement's own bounds, else the sound's playback distances): full volume
  inside the near distance, falling as the square of the remaining fraction to silence at
  the far one, equal-power panned by the camera's right vector. Valhalla has 77 (the
  waterfall, the creek and brook along the river, the lake shore). Two stand-ins for
  the game's undecoded sound classes and mix: only the 2 loudest placements of one
  looping sound play at a time, and sound scenery is mixed 6 dB down (120_halo's eight
  falling-plate loops along the Warthog run otherwise summed to -5 dBFS).
- **Music** is started by HaloScript in the game; here M plays the map's showpiece (on
  120_halo `120_music_15`, the Warthog run's `halo3_trench`: 92 s intro, then its four loop
  permutations), Shift+M the next `lsnd` under a `music` folder, `--music NAME|N` one at
  start (`--music 120_music_15` or an index into the list the log prints). N moves to the
  alternate loop through its transition at the end of the current loop, and back. M again
  plays the outro (or fades out over the track's fade time).
- **Board sounds while skating** (Skate 3's data has no audio): Halo's own samples, picked
  from the bank by folder: the Brute chopper's hard-wheel loop for rolling (gain and pitch
  with speed), a vehicle scrape while grinding, the Chief's hard concrete landing after
  more than 0.25 s in the air (louder the longer the air). The skate engine's state names
  (`PhysicsGround`, `PhysicsAir`, `GrindFiftyFifty`, ...) come through `skate::SkateMotion`.
- **Volume**: `HALO3_VOLUME` (default 0.8) sets the start level, `-` / `=` change it, K mutes.

Headless check: `HALO3_AUDIO_TRACE=1` with `--shot` runs all of this without a sound device
and logs, 0.3 s after each view, the cluster, background layers and their fade, the sound
scenery in range with distance and gain, and the decoded clips:

```
$env:HALO3_AUDIO_TRACE="1"
target\debug\halo3.exe riverworld --shot .scratch\shots --view "96,-140,2,90,0" --view spawn
```

`blam sound <map> list | show TAG | wav TAG... | check` lists, inspects and decodes the same
sounds to WAV (`.scratch/sound` by default) with duration, rate, RMS and peak.

Not done: the sound environments (`snde` reverb) and obstruction; pitch ranges past the
first; dialogue (the per-language banks); weapon, vehicle, footstep and impact sounds of the
game world; HaloScript-driven music cues (music is by hand); the dirt bike's engine
(the Mongoose engine loops are in the bank). Halo's real distance curve and the meaning of
the playback parameters' first two distances are not checked against the game.
