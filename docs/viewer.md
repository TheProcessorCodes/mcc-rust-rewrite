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
| right mouse button (hold), or Tab | look around with the mouse |
| Escape | give the cursor back |
| P | print the camera's position in Halo units to the console |
| V | show or hide the game's real collision: green shell for surfaces, red for invisible walls |
| gamepad left stick | move |
| gamepad right stick | look |
| gamepad right / left trigger | up / down |
| gamepad A | fast |

Normal speed is 25 m/s. The camera starts at the map's first respawn point, at
Spartan eye height, facing the way the marker faces; a map without respawn
points starts above the middle of its world.

## Frame and units

Halo is Z up and measures in world units of ten feet (3.048 m). The renderer
is Y up in metres: `render = 3.048 * (hx, hz, -hy)`. That is a rotation, not a
mirror, and the same mapping HaloExport uses for its glTF files, so numbers
from the Reflex pipeline sidecars carry over.

## What is drawn

One bevy mesh per material and lighting mode, every placement baked into
world space; `Sky` placements get their own shader (see "Sky and haze" below).

Before the window opens, `textures.rs` resolves every material's shader tag
(`blam_texture::shader::resolve`) and decodes the bitmaps it needs, each
bitmap once; `world.rs` turns them into one shared bevy `Image` per bitmap.

| shader | drawn as |
|---|---|
| `rmsh`, `rmhg`, `rmd ` with a `base_map` | the base map, tinted by `albedo_color`, tiled by the base map's xform through `uv_transform` (the mesh keeps the raw UVs) |
| the rest of an ordinary shader's material | detail map, bump map (and bump detail), specular, environment reflection and self-illumination, in the lighting shader: see "Materials" below |
| foliage (`rmfl`) and any `alpha_test simple` shader | `AlphaMode::Mask(0.5)` on the base map's alpha, or on `alpha_test_map` when the shader has its own (grates), both sides drawn |
| `blend_mode` alpha_blend / pre_multiplied_alpha / additive / multiply | `Blend` / `Premultiplied` / `Add` / `Multiply`, no shadows |
| opaque shaders | `Opaque`: the base map's alpha is a specular mask, not transparency, and is ignored |
| terrain (`rmtr`) | the **four-layer blend**, in the lighting shader (see "Terrain" below). If the blend map or every layer fails to decode, or with `HALO3_TERRAIN_BLEND=0`, the single layer the blend map weights most over the whole map, as HaloExport does |
| water (`rmw `) | flat `water_diffuse` colour at 75 % opacity (the watercolor texture's alpha is not opacity and its colour is the same), plus a fresnel-weighted reflection of the surrounding light and a sun glint (`fresnel_coefficient`) |
| spawn markers: placements of `respawn_point`, `*_initial_spawn_point`, `*_respawn_zone` objects, and the `objects\multi\shaders\spawn_point*` and `shaders\invalid` shaders | not drawn: Forge shows them, a game never does (they were the black discs and white plasma columns on every map) |
| constant-colour shaders (no base map) | flat `albedo_color` |
| additive glows without a base map | `self_illum_map` tinted by `self_illum_color`, added |
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

Not yet: real cube-map reflections, parallax, animated water (waves,
refraction, foam), `two_detail`'s second detail map, change colours, decal
bump. Translucent materials are still one mesh per material, so two
overlapping translucent materials can sort wrongly.

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
| scenery, crates, machines, controls | the lightmap at the lit BSP vertex nearest the bottom centre of the object, per vertex with each vertex's normal (an approximation: the scenario's own scenery/machine probes are not matched up yet) |
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
- **The world** gets bevy's `DistanceFog` (atmospheric falloff) from the level setting
  (`haze_level`): per-channel extinction and the in-scattered colour, recomputed every frame for
  the camera's height, since the haze thins with height (Mie scale height 6 wu over a sea level at
  z 0 on Valhalla). It is applied in the lightmap shader's post-processing call, so bluish
  distance haze needs no change there.
- **Clear colour**: a long level ray through the sky's air at the camera's height, for the
  places below the dome's rim with no sky geometry (only seen out of bounds).
- `HALO3_HAZE` scales the world haze's density (`0` turns it off) for comparisons.
- Below a setting's sea level the density stays at the sea level's (world haze and sky alike):
  Sandtrap plays about 20 wu under its sea level, where the exponential falloff made the haze
  e^10 times thicker and every ground-level view black.

Not done: the world haze uses one density for the whole ray (the eye's), so looking down from a
height onto the lake under-hazes it; the level setting's distance bias (-15 wu, no haze in the
first 15 wu) and the sun-angle dependence of the haze colour are not applied to the world (bevy's
fog has neither); `desaturation` (used by The Pit, 0.2) is ignored; Halo's real sky lighting and
exposure curve is not decoded (the `cfxs` exposure is, see "Lighting"); no sun glare or bloom; the sky does not move with the camera
(it is at true size, about 7,500 wu out, which is what the haze distances need).
