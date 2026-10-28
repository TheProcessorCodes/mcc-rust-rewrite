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

One bevy mesh per material, every placement baked into world space; `Sky`
placements are skipped.

Before the window opens, `textures.rs` resolves every material's shader tag
(`blam_texture::shader::resolve`) and decodes the bitmaps it needs, each
bitmap once; `world.rs` turns them into one shared bevy `Image` per bitmap.

| shader | drawn as |
|---|---|
| `rmsh`, `rmhg`, `rmd ` with a `base_map` | the base map, tinted by `albedo_color`, tiled by the base map's xform through `uv_transform` (the mesh keeps the raw UVs) |
| foliage (`rmfl`) and any `alpha_test simple` shader | `AlphaMode::Mask(0.5)` on the base map's alpha, both sides drawn |
| `blend_mode` alpha_blend / pre_multiplied_alpha / additive / multiply | `Blend` / `Premultiplied` / `Add` / `Multiply`, no shadows |
| opaque shaders | `Opaque`: the base map's alpha is a specular mask, not transparency, and is ignored |
| terrain (`rmtr`) | **one layer only**: the layer the blend map weights most over the whole map (Valhalla's ground: layer 0, the grass overlay at 40 x 80), as HaloExport does. The four-layer blend needs a custom material |
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

Not yet: detail maps (they multiply over the base and tile 3-10x tighter, so
surfaces look smoother and lighter than in the game), normal maps (need
tangents), the terrain blend, self-illumination, reflections, cube maps,
animated water. Translucent materials are still one mesh per material, so
two overlapping translucent materials can sort wrongly.
