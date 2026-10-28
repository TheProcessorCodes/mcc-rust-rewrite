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
| J | skate / fly (`crates/halo3/src/skate.rs`) |
| PageDown / PageUp | next / previous zone set (campaign maps) |
| Home | load every BSP at once |
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

The viewer decodes and uploads every BSP at start (120_halo: about 1 s to
decode, 2.9 M triangles, 240 MiB of textures) and shows only the loaded zone
set; switching is instant. Each BSP keeps its own lightmap. What is loaded:

```
& "target\debug\halo3.exe" 120_halo                 # the zone set the start is in (zone set 0)
& "target\debug\halo3.exe" 120_halo --zone 6        # by index ...
& "target\debug\halo3.exe" 120_halo --zone 120_110_120   # ... or by name
& "target\debug\halo3.exe" 120_halo --bsps 7,8,9    # any BSPs (indices in the scenario)
& "target\debug\halo3.exe" 120_halo --all-bsps      # everything (overlaps)
```

The console prints the zone sets and BSPs at start. Objects follow their
origin BSP (objects with none always show); a sky shows only while it is the
sky of the camera's cluster, so 120_halo switches from the snowfield sky to the
Warthog run's ring sky (`sky_03`) as you cross over. The haze follows the
camera's cluster too: each cluster names one of the atmosphere tag's settings
(thick snow, the thin air of the run, none indoors), used for the world fog,
the sky's scattering and the clear colour; outside every cluster the setting
most loaded clusters name.

Skating uses the collision of the loaded BSPs only. Changing zone set stops the
ride and rebuilds the skate world in the background (title bar: "skate:
loading the new zone set"); J works again when it is ready. Rails inside the
scenario's kill volumes are dropped. 120_halo's Warthog run (zone set 6): the
skate world is ready about 15 s after start (520 k triangles, 6,300 rails found
in 4.6 s, the skater's own data 6 s). Rails over all 11 BSPs would take 16 s
and give 58,600, most of them ledges under the run the player never reaches.

`world/zones.rs` is the API a streaming step can drive: `LoadedBsps` (mask,
zone set, sky), `load_zone_set`, `load_bsps`; the scene keeps the trigger
volumes and zone-set switch volumes (`Scene::zone_switches`) for that.
`HALO3_SKY=0` hides the sky, for telling sky pieces from the world.

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
| foliage (`rmfl`) and any `alpha_test simple` shader | `AlphaMode::Mask(0.5)` on the base map's alpha, both sides drawn |
| `blend_mode` alpha_blend / pre_multiplied_alpha / additive / multiply | `Blend` / `Premultiplied` / `Add` / `Multiply`, no shadows |
| opaque shaders | `Opaque`: the base map's alpha is a specular mask, not transparency, and is ignored |
| terrain (`rmtr`) | the **four-layer blend**, in the lighting shader (see "Terrain" below). If the blend map or every layer fails to decode, or with `HALO3_TERRAIN_BLEND=0`, the single layer the blend map weights most over the whole map, as HaloExport does |
| water (`rmw `) | flat `water_diffuse` colour at 75 % opacity (the watercolor texture's alpha is not opacity and its colour is the same) |
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

Not yet done for terrain: the bump maps, specular on the `diffuse_plus_specular`
layers (their base alpha is a specular mask), and `dynamic_morph` blending
(no map uses it here).

Not yet: detail maps on ordinary shaders (they multiply over the base and
tile 3-10x tighter, so surfaces look smoother and lighter than in the game),
normal maps (need tangents), self-illumination, reflections, cube maps,
animated water. Translucent materials are still one mesh per material, so
two overlapping translucent materials can sort wrongly.

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

Exposure is a flat x4 (`EXPOSURE` in `world/lighting.rs`; set
`HALO3_EXPOSURE` to try others), picked against in-game Valhalla
screenshots. Not done: specular, bump-mapped normals, Halo's own
exposure/tonemap curve, the probes' L2 terms.

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
- **Campaign clusters** name their own setting (see "Zone sets" above); when the camera is in
  one, that setting replaces both the level and the sky setting. Lit sky surfaces are scaled by
  their shader's `diffuse_coefficient` (120_halo's ring plates: 0.3).
- `HALO3_HAZE` scales the world haze's density (`0` turns it off) for comparisons.

Not done: the world haze uses one density for the whole ray (the eye's), so looking down from a
height onto the lake under-hazes it; the level setting's distance bias (-15 wu, no haze in the
first 15 wu) and the sun-angle dependence of the haze colour are not applied to the world (bevy's
fog has neither); `desaturation` (used by The Pit, 0.2) is ignored; Halo's real sky lighting and
exposure (`cfxs`) are not decoded; no sun glare or bloom; the sky does not move with the camera
(it is at true size, about 7,500 wu out, which is what the haze distances need).
