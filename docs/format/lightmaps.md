# MCC Halo 3 (U13) baked lighting: the `Lbsp` tag, lightmap texcoords and the decode

How Halo 3 lights its structure BSP: which tag fields hold the baked lighting, how each cluster
mesh and each instanced-geometry instance finds its lightmap texcoords or its per-vertex / probe
lighting, and the formula that turns two DXT5 arrays into diffuse irradiance.

- Target: MCC Halo 3 build `Dec 21 2023 22:31:37` (U13). Test maps `riverworld.map` (Valhalla) and
  `cyberdyne.map` (The Pit).
- Sources (facts only, nothing copied; both are **GPL-3.0**, not MIT):
  - TagTool (`github.com/TheGuardians-CI/TagTool`): `Tags/Definitions/ScenarioLightmapBspData.cs`,
    `Geometry/RenderGeometry/RenderGeometry.cs`, `Mesh.cs`, `VertexBufferFormat.cs`,
    `VertexStreamHalo3RetailMCC.cs`, `Common/SphericalHarmonics.cs` (`LuminanceScale`,
    `HalfRGBLightProbe`), `Lighting/LightmapCompressor.cs`, `Lighting/LightmapCache.cs`,
    `Commands/Porting/Gen2/PortTagGen2Command.Lightmaps.cs` (shows which fields link meshes to
    their lighting when TagTool builds an H3-format lightmap).
  - HaloShaderGenerator (`github.com/Beatthezombie/HaloShaderGenerator`, TagTool's submodule),
    folder `halo_online_shaders/`: Bungie's Halo 3-era HLSL as shipped with Halo Online.
    `lightmap_sampling.fx` (`decode_bpp16_luvw`, `sample_lightprobe_texture`),
    `spherical_harmonics.fx` (`pack_constants_texture_array_linear`,
    `ravi_order_2_with_dominant_light`), `entry_points.fx` (`static_per_pixel_*`,
    `static_per_vertex_*`, `static_sh_*`), `hlsl_vertex_types.fx` (`s_lightmap_per_pixel`),
    `hlsl_constant_persist.fx` (`p_lightmap_compress_constant_0/1`, samplers 13/14).
    MCC's own shaders are DX11 rebuilds of the same code. They are not in the public tree, so the
    MCC math is **inferred** from the Halo Online source. The data agrees with it (section 7).
- Verified with `blam lightmap <map> [--bsp N] [--samples N]` (`crates/blam_cli/src/lightmap.rs`,
  read-only). Every number marked **V** below was printed by it on riverworld and cyberdyne.
  **S** = from the sources only. **I** = inferred.
- Offsets are hex (decimal where useful), little-endian, relative to the start of the tag or element.

---

## 1. The chain

`scnr + 0x6AC` -> `sLdT` (`<map>_faux_lightmap`) -> `sLdT + 0x4` block of `Lbsp` tag refs -> the
`Lbsp` whose `+0x2` BSP index matches (see `sbsp-render-geometry.md`). The `Lbsp` owns:

- the final render geometry of the BSP (its `+0x130` render geometry struct, sections at `+0x134`,
  resource at `+0x1AC`);
- **two bitmap tags**: a 1024x1024 **8-layer** DXT5 array with four linear SH coefficients (two
  layers each), and a 1024x1024 **2-layer** DXT5 array with the dominant light intensity;
- five HDR scale factors, one per coefficient plus one for the intensity;
- per-cluster and per-instance lighting records that say per-pixel (lightmap texcoords),
  per-vertex (an extra vertex buffer) or a single light probe;
- light probes for instanced geometry, scenery and machines.

The 8 layers are **not pages**. There is one 1024x1024 atlas per BSP, and the layers are 4 SH
coefficients x (base, residual). Nothing picks a layer per mesh.

## 2. `Lbsp` = `scenario_lightmap_bsp_data`, 0x1E4 (484) bytes on MCC Halo 3

| offset | size | field | riverworld | cyberdyne | |
|---|---|---|---|---|---|
| `0x000` | 2 | u16 flags: 1 compressed, 2 xsynced geometry, 4 relightmapped, 8 generate fake small lightmaps, 0x10 generated from match data | `0x0007` | `0x0007` | V value, S names |
| `0x002` | 2 | i16 BSP index | 0 | 0 | V |
| `0x004` | 4 | u32 structure checksum | `0x101E0903` | `0x121B1116` | V |
| `0x008` | 9 x 24 | `LuminanceScale[9]`: 24 bytes each, f32 **scale** at +0, five more f32 (all 0 on both maps) | see below | | V |
| `0x0E0` | 16 | TagReference `bitm`: **SH coefficients** array | #11656 `levels\multi\riverworld\riverworld_lightmap_riverworld_16f_lp_array_dxt5` | #11356 `..._cyberdyne_16f_lp_array_dxt5` | V |
| `0x0F0` | 16 | TagReference `bitm`: **dominant light intensity** array | #11657 `..._16f_lp_array_intensity_dxt5` | #11357 `..._intensity_dxt5` | V |
| `0x100` | 12 | block, 16 B: static per-vertex lighting buffers | 59 | 491 | V |
| `0x10C` | 12 | block, 4 B: cluster lighting, one per sbsp cluster | 26 | 48 | V |
| `0x118` | 12 | block, 8 B: instanced geometry lighting, one per sbsp instance | 262 | 834 | V |
| `0x124` | 12 | block, 72 B: instanced geometry light probes | 18 | 255 | V |
| `0x130` | 0x84 | **render geometry** struct (TagTool `RenderGeometry`, H3 layout) | | | V |
| `0x134` | 12 | ..meshes / sections, 76 B (`sbsp-render-geometry.md`) | 102 | 359 | V |
| `0x1A0` | 12 | ..block, 16 B: **instanced geometry per-pixel lighting** (lightmap texcoord buffers for instances) | 185 | 86 | V |
| `0x1AC` | 8 | ..geometry resource reference | `0xEE3D0CDA` (entry 3290) | `0xEDD00C6D` | V |
| `0x1B4` | 12 | block, 92 B: airprobes | 0 | 0 | V count |
| `0x1C0` | 12 | block, 80 B: scenery light probes | 143 | 132 | V count |
| `0x1CC` | 12 | block, 44 B: machine light probes | 0 | 8 | V count |
| `0x1D8` | 12 | block, 4 B: unused | 0 | 0 | V count |

The render geometry struct runs from `0x130`: runtime flags u32 `+0x0` (7 on riverworld), then the
blocks meshes `+0x4`, compression `+0x10`, bounding spheres `+0x1C`, user data `+0x28`,
per-mesh raw data `+0x34`, mesh cluster visibility (MOPP) `+0x40`, per-mesh node maps `+0x4C`,
subpart visibility `+0x58`, PRT data `+0x64`, instanced geometry per-pixel lighting `+0x70`, and the
resource at `+0x7C`. That puts the sections at `0x134` and the resource at `0x1AC`, which matches
Reclaimer's offsets (`haloexport-notes.md` 3.3). Only the meshes, per-pixel lighting and resource
offsets are V. The names in between are S.

### 2.1 Scale factors (`LuminanceScale[i].scale`, f32 at `0x08 + 24 i`)

| i | meaning (S, I) | riverworld | cyberdyne |
|---|---|---|---|
| 0 | SH coefficient 0 (L0, constant) range | 4.6922183 | 31.482393 |
| 1 | SH coefficient 1 (L1, ~ -y) range | 4.5964856 | 30.415527 |
| 2 | SH coefficient 2 (L1, ~ z) range | 5.6567373 | 35.32007 |
| 3 | SH coefficient 3 (L1, ~ -x) range | 6.173758 | 31.069582 |
| 4 | dominant light intensity range | 16.105705 | 71.366516 |
| 5..8 | unused in H3 (all 0) | 0 | 0 |

The engine uploads these as pixel constants `p_lightmap_compress_constant_0.xyz = scale[0..2]`,
`p_lightmap_compress_constant_1.x = scale[3]`, `.y = scale[4]` (c210/c211 on DX9) (S). TagTool's
lightmap writer fills exactly `scale[0..4]` with the `maxL` of each compressed map (S), so the values
are "the largest luminance in that map". Valhalla is outdoors in sunlight, The Pit is indoors with
bright fixtures, which is why their ranges differ so much.

### 2.2 The two bitmaps (V)

| bitmap | size | layers | format | mips | curve |
|---|---|---|---|---|---|
| `..._16f_lp_array_dxt5` | 1024x1024 | 8 | DXT5 (BC3), DXGI 77 | 1 level | linear (3), type 3 = array |
| `..._16f_lp_array_intensity_dxt5` | 1024x1024 | 2 | DXT5 (BC3) | 1 level | linear, array |

Same on riverworld and cyberdyne. Riverworld's SH array is 8 MiB at page 1420 +0x4980000. The
intensity array is 2 MiB at page 1420 +0x5180000, the same page as the BSP geometry. Each bitmap
tag has exactly one image, and every `LightmapBitmapsImageIndex` on both maps is 0 or -1.
"16f_lp" presumably means "16-bit float light probe" source data (I).

Layer use (S from `sample_lightprobe_texture`, consistent with the data, section 7):

| SH array layer | content |
|---|---|
| 0, 1 | coefficient 0, base + residual |
| 2, 3 | coefficient 1, base + residual |
| 4, 5 | coefficient 2, base + residual |
| 6, 7 | coefficient 3, base + residual |
| intensity layer 0, 1 | dominant light intensity, base + residual |

On riverworld 1,006,528 of the 1,048,576 layer-0 texels are non-zero (96 %). On cyberdyne 988,800
are non-zero. The atlas is packed tight (V).

## 3. Lighting records

### 3.1 Cluster lighting (`Lbsp + 0x10C`, 4 bytes, index = sbsp cluster index)

| offset | field | |
|---|---|---|
| `0x0` | i16 lightmap bitmap image index (-1 = no per-pixel lightmap) | V (0 on lit clusters) |
| `0x2` | i16 static per-vertex lighting index into `Lbsp + 0x100` (-1 = none) | V (-1 on all clusters of both maps) |

The cluster's mesh is `sbsp cluster (280 B) + 216` i16 = section index (existing code). Riverworld
clusters 0, 1 and 23 have no mesh (all vertex buffer slots -1) and image -1. Every other cluster has
image 0.

### 3.2 Instanced geometry lighting (`Lbsp + 0x118`, 8 bytes, index = sbsp instance index)

| offset | field | |
|---|---|---|
| `0x0` | i16 lightmap bitmap image index | V |
| `0x2` | i16 static per-vertex lighting index into `Lbsp + 0x100` | V |
| `0x4` | i16 light probe index into `Lbsp + 0x124` | V |
| `0x6` | pad | |

It is used together with the sbsp instance (120 B at `sbsp + 0x1C0`, see `collision-bsp-research.md` 10.2):

| instance offset | field | |
|---|---|---|
| `0x34` | i16 instance definition = section index | V |
| `0x38` | i16 **lightmap texcoord block index** into `Lbsp + 0x1A0` (-1 = none) | V |
| `0x5A` | i16 lightmapping policy: **1 per-vertex, 2 single probe, 3 per-pixel** | V (matches the records exactly) |

Census, as (policy, has texcoord block, image >= 0, per-vertex >= 0, probe >= 0): count (V):

| | riverworld | cyberdyne |
|---|---|---|
| (3, yes, yes, no, no) per-pixel lightmap | 184 | 84 |
| (3, yes, no, no, no) texcoords but image -1 | 1 | 2 |
| (1, no, no, yes, no) per-vertex | 59 | 491 |
| (2, no, no, no, yes) one probe | 18 | 255 |
| (2, no, no, no, no) probe policy, no probe | 0 | 2 |

So the per-vertex buffers (59 / 491) and probes (18 / 255) are one per instance that uses them, and
the per-pixel texcoord blocks (185 / 86) are one per per-pixel instance.

### 3.3 Instanced geometry per-pixel lighting (`Lbsp + 0x1A0`, 16 bytes)

| offset | field | |
|---|---|---|
| `0x0` | block (unused vertex buffer, count 0) | S |
| `0xC` | i16 vertex buffer index into the geometry resource | V |
| `0xE` | i16 unknown | S |

### 3.4 Static per-vertex lighting buffer (`Lbsp + 0x100`, 16 bytes)

| offset | field | |
|---|---|---|
| `0x0` | block (unused vertex buffer, count 0) | S |
| `0xC` | i32 vertex buffer index into the geometry resource | V |

## 4. Vertex buffers and the lightmap texcoords

The geometry resource definition starts with one 28-byte record per vertex buffer: i32 vertex
count `+0`, **i16 declared type `+4`, i16 stride `+6`**, i32 byte length `+8`, ... (V: the types
line up with TagTool's `VertexBufferFormat` enum, and the strides match its sizes for types 6, 9
and 28. TagTool gives 8 bytes for type 4, but MCC stores 4.)

Riverworld, 394 buffers (V):

| type | TagTool name | stride | buffers | used in |
|---|---|---|---|---|
| 1 | world | 44 | 23 | section slot 0 (clusters) |
| 2 | rigid | 36 | 75 | section slot 0 (instance definitions) |
| 4 | **static per-pixel = lightmap texcoord** | **4** | 207 | section slot 1 (23 clusters) + 185 instance blocks |
| 6 | static per-vertex lighting | 20 | 59 | `Lbsp + 0x100` records |
| 9 | ambient PRT | 4 | 8 | section slot 3 |
| 27 | water (TagTool calls it tessellated water parameters) | 36 | 11 | section slot 7 |
| 28 | water (TagTool `Unknown1C`) | 128 | 11 | section slot 6 |

Cyberdyne has the same types without water: 44 world, 307 rigid, 128 per-pixel, 491 per-vertex,
117 ambient PRT.

### 4.1 Which buffer holds a mesh's lightmap texcoords

- **Cluster mesh**: its section's **vertex buffer slot 1** (`section + 0x18 + 2*1`, i16). Type 4,
  and the vertex count equals slot 0's (V: riverworld 23 / 23 lit clusters, e.g. cluster 2 = section
  78, buffers [83, 84, -, -, -, -, 86, 85], 9177 vertices in both 83 and 84).
- **Instance**: the definition mesh is shared by every placement, so its slot 1 is empty. The
  texcoords come from `instance + 0x38` -> `Lbsp + 0x1A0`[that] `+0xC` -> buffer. That buffer has the
  definition mesh's vertex count, index for index with slot 0 (V: 0 mismatches across 185 + 86 instances).
  Two placements of the same definition have their own buffers (riverworld instances 2 and 3, both
  definition 2: buffers 153 and 154, 428 vertices each).

### 4.2 Decode (V)

Each vertex is **2 x u16, normalised**: `u = x / 65535`, `v = y / 65535`. They are absolute
coordinates in the single 1024x1024 atlas. There is **no per-mesh scale, offset or layer**. Over all
buffers riverworld spans u 0.0000..0.9990, v 0.0000..0.9990, and cyberdyne spans u 0.0002..0.9989.

Orientation: v = 0 is the first row of the DXT data (top, D3D convention). Check: of 65,483
riverworld texcoords, 46 land on an empty layer-0 texel as stored. With v flipped it is 1,334, and
with u and v swapped it is 2,902. Cyberdyne gives 10 / 506 / 2,351. The texture coordinate goes
into the shader unchanged (`s_lightmap_per_pixel.texcoord : TEXCOORD1` copied to the pixel shader).
Sample all 10 layers with **bilinear filtering, no mips** (the bitmaps have one level).

## 5. Texel to irradiance

### 5.1 Unpacking one value: `decode_bpp16_luvw` (S, checked on data)

For a value stored as layers `(a, b)` (both RGBA in 0..1 after DXT5 decode, linear, no sRGB) with
range `s`:

```
L   = a.alpha * b.alpha * s
rgb = (a.rgb + b.rgb) * 2 - 2        // direction in -1..1 per channel
value = rgb * L                      // signed RGB
```

This inverts the bake: the RGB is normalised to unit length per texel (stored as `(c/|c| + 1)/2`),
the residual layer holds `error + 0.5`, and the alpha layers hold `sqrt(|c| / max)` twice, so their
product gives `|c| / max` (TagTool `LightmapCompressor`). Apply it to:

- `c0..c3 = decode(SH[2i], SH[2i+1], scale[i])`, the four linear SH coefficients (RGB each);
- `D = decode(I[0], I[1], scale[4])`, the dominant light intensity (RGB).

### 5.2 Dominant light direction (S)

```
w   = (0.212656, 0.715158, 0.0721856)           // luminance weights
dir = normalize( -dot(c3,w), -dot(c1,w), dot(c2,w) )
```

So the L1 coefficients are ordered (y, z, x) with signs (-, +, -). The Halo 3 world is Z up.

### 5.3 Diffuse irradiance for a normal `n` (S, `ravi_order_2_with_dominant_light`)

```
// pack: k0 = c0;  kR = (c3.r, c1.r, -c2.r); kG, kB likewise
// remove the dominant light from the SH:
e   = (-0.4886025*dir.y, -0.4886025*dir.z, -0.4886025*dir.x)
kR -= e.zxy * D.r;  kG -= e.zxy * D.g;  kB -= e.zxy * D.b
k0 -= 0.2820948 * D
x1  = (dot(n,kR), dot(n,kG), dot(n,kB))
ambient  = (0.886227 * k0 - 2 * 0.511664 * x1) / pi
direct   = max(dot(dir, n), 0) * D * 0.281
E = ambient + direct
```

Then `diffuse = albedo * E`. Specular uses the same SH, dominant direction and intensity
(`calc_output_color_with_explicit_light_linear_with_dominant_light`, material-dependent, not
covered here). With bump mapping `n` is the per-pixel bump normal in world space. Fog
(extinction/inscatter) and the HDR exposure are applied afterwards. The overall brightness is
therefore relative: E on riverworld is about 0.6-0.7 in direct sun, and the renderer's exposure
maps that to the screen.

In a shader it costs 10 texture fetches (8 SH layers and 2 intensity layers from two array
textures) and about 30 ALU ops. Use it as it is; there is no need for a cheaper approximation.

### 5.4 Worked example, riverworld cluster 2 vertex 0 (V, nearest texel)

Position (83.4, -145.4, 16.0) wu, normal (0.91, 0.39, 0.16), texcoord (0.6557, 0.9975) -> texel
(671, 1021):

| term | RGB |
|---|---|
| c0 | 1.067 0.998 0.984 |
| c1 | -0.448 -0.424 -0.448 |
| c2 | 0.929 0.857 0.857 |
| c3 | -0.894 -0.793 -0.760 |
| D (dominant intensity) | 2.735 2.493 2.354 |
| dir | (0.64, 0.34, 0.69) after normalising (-c3, -c1, c2) luminance |
| E | 0.728 0.667 0.653 |

Two other sunlit vertices far apart (cluster 2 v4587 at (52.4, -110.8, 5.9), v6116 at (96.3,
-164.8, 29.0)) give dominant directions (0.67, 0.44, 0.63) and (0.53, 0.47, 0.71), with intensities
about 2.7 / 2.4 / 2.3. That is one consistent sun, high in the sky. A vertex under cover (v1529,
normal -y) gets bluish sky light: c0 (0.12, 0.16, 0.26), E (0.07, 0.10, 0.15). The mean E over
157 sampled cluster vertices is (0.227, 0.212, 0.218). Cyberdyne: (0.202, 0.193, 0.203) over 297.

## 6. Per-vertex lighting and probes (lower priority)

### 6.1 Static per-vertex lighting (policy 1)

The buffer (type 6, 20 bytes per vertex, one vertex per definition-mesh vertex, V) holds five
RGBA8 values bound to TEXCOORD3..7 in this order (S, `static_per_vertex_vs`): dominant light
intensity, c0, c1, c2, c3. Decode each one (S, the `pc` branch; V by the checks below):

```
t   = 2 * (byte / 255) - 1          // all four channels, stored unsigned on PC
rgb = t.rgb * exp2(t.a * 31.75)
```

Then `dir = normalize(-c3, -c1, c2)` with luminance weights as in 5.2, and the same
`ravi_order_2_with_dominant_light` as 5.3.

- The `2x-1` is needed. The alpha bytes are around 0x63-0x77, so without it the scale would be
  2^14 or more. With it, means over riverworld's 64,922 vertices come out at intensity (0.38, 0.36,
  0.34) and c0 (0.18, 0.18, 0.17), the same magnitude as the per-pixel values. c2 (the z term) has
  the largest positive mean, so light comes from above (V, plausibility).
- **Byte order is R, G, B, A (V)**. For each per-vertex instance vertex we placed it in the world
  and compared the chroma of its c0 with the per-pixel c0 at the nearest cluster vertex within 1 wu.
  The agreement score was RGBA +603 vs BGRA -553 over 8,101 pairs on riverworld, and RGBA +8,210 vs
  BGRA -7,141 over 135,362 pairs on cyberdyne.
- Some buffers are flat `7f 7f 7f 00` (cyberdyne buffer 1, vb 597). That decodes to ~0 light,
  probably vertices the lightmapper did not reach.

### 6.2 Light probes (policy 2, scenery, machines)

`HalfRGBLightProbe`, 72 bytes, all **f16** (S layout, plausible values on data):

| offset | field |
|---|---|
| `0x00` | dominant light direction, 3 x f16, + pad |
| `0x08` | dominant light intensity, 3 x f16, + pad |
| `0x10` | SH red, 9 x f16 (order 2, 9 coefficients) |
| `0x22` | SH green, 9 x f16 |
| `0x34` | SH blue, 9 x f16 |
| `0x46` | pad |

- Instanced geometry probe (`Lbsp + 0x124`, 72 B) = one probe, chosen by `+0x4` of the instance record.
  Riverworld probe 0: dir (-0.548, -0.099, -0.831), intensity (0.090, 0.075, 0.050), SH r[0] 0.059.
  The direction from its L1 terms, (-c3, -c1, c2) = (-0.032, -0.008, -0.049), points the same way.
- Scenery probe (`Lbsp + 0x1C0`, 80 B) = 8-byte scenario object identifier + probe. Riverworld
  probe 0: object id `0xBE94000D 0x01060000`, dir (-0.507, -0.842, 0.184), intensity (0.120, 0.136, 0.187).
- Machine probes (`Lbsp + 0x1CC`, 44 B): object id 8 B, bounds 24 B (`RealRectangle3d`), block of
  84-byte probes (position 12 B + probe). Cyberdyne has 8. Not decoded.
- Shading with a probe is `static_sh` in the shader source: the 9 coefficients go through
  `pack_constants` and `ravi_order_3` (quadratic SH irradiance), plus the probe's dominant light (S).
- Dynamic objects (bipeds, weapons, vehicles) are not baked. They get their SH at runtime from the
  lighting around them (I; Halo 3 is widely described as sampling the lightmap under the object).
  Not researched here.

## 7. What is verified and what is not

Verified on both maps' bytes:
- every `Lbsp` offset in section 2 except the internal names of the render geometry blocks;
- the two bitmaps, their sizes and layers, and the image index (always 0);
- the five scales, which sit in the f32 slots TagTool's layout predicts (the other 4 are zero);
- the cluster and instance record layouts, the lightmapping policy enum, and the instance links
  `+0x38 -> Lbsp+0x1A0 -> vertex buffer` and `record+0x2 -> Lbsp+0x100 -> vertex buffer`, with
  matching vertex counts everywhere;
- the vertex buffer declaration (type at +4, stride at +6), the per-pixel texcoord format
  (2 x u16 / 65535) and that its v is not flipped;
- that the decode in 5.1-5.3 gives a single coherent sun (direction and colour) from texels far
  apart, and sky-blue shade under cover;
- that per-vertex lighting needs `2x-1`, is RGBA, and agrees in colour with the per-pixel map.

From sources only (not checked against MCC's own compiled shaders):
- the exact constants of `ravi_order_2_with_dominant_light` (0.886227, 0.511664, 0.4886025,
  0.2820948, 0.281) and the dominant-light subtraction. Halo Online is the same engine generation,
  but MCC may have retuned them. The decode's *shape* is confirmed by the data; the absolute scale
  of E is not.
- the per-vertex `exp2(a * 31.75)` constant. Magnitudes come out right, but this is not a proof.
- the probe f16 layout and the 9-coefficient order.

Open questions:
1. What does the game do for the 1 (riverworld) / 2 (cyberdyne) instances with per-pixel policy and
   texcoords but image -1, and for the 2 cyberdyne probe-policy instances with no probe? Probably
   black or a fallback. Not visible in data.
2. Section slots 2, 4 and 5 are never used. Slot 3 (ambient PRT, type 9, 4 B) holds per-vertex
   ambient occlusion for some definition meshes (S name). Its decode and use (`PrtType` at section
   `+0x2F`) were not looked at.
3. The bitmaps have flag `0x04`. Is any filtering or half-texel offset needed at atlas chart edges?
   The bake leaves gutters (4 % of texels are empty), so plain bilinear should be fine.
4. The airprobe block is empty on both maps. Campaign maps may use it, along with machine probes
   for moving parts.
5. Exposure: map E to the screen with a fixed exposure (try 1.0 to 2.0 with a filmic curve) until
   the sky/exposure tags are read.

## 8. What the renderer implements (2026-10-28)

Decoding lives in `blam_geometry::lightmap` (records, texcoords, per-vertex values, probes, the
irradiance formula of 5.3, unit-tested on the 5.4 worked example); `Scene::lightmaps` holds each
BSP's two bitmap tag indices and five scales, and every BSP placement carries a
`Placement::lighting` (`PerPixel { lightmap, uvs }`, `PerVertex`, `Probe` or `None`). Riverworld:
23 clusters and 184 instances per-pixel, 58 per-vertex, 18 probe, 1 none (the image -1 instance).
`blam lightmap` uses the same code.

The viewer (`crates/halo3/src/world/lighting.rs`, shader `world/shaders/lightmap.wesl`):

- per-pixel: both arrays go to the GPU as BC3 `D2Array` images (linear, clamp, bilinear, one
  level), the texcoord as `UV_1`; the fragment shader evaluates 5.1-5.3 with the interpolated
  vertex normal (no bump maps yet) and multiplies the albedo by it;
- per-vertex instances: 6.1 per vertex on the CPU into the vertex colour. Blank buffers
  (all `7f 7f 7f 00`) count as no data;
- probe instances: the probe's L0/L1 terms with its stored direction and intensity through the same
  formula, per vertex on the CPU. **The L2 terms are dropped** and whether the stored SH includes
  the dominant light (so that subtracting it is right) is unverified;
- scenery, crates, machines and anything else without data: the lightmap read at the lit cluster
  vertex nearest the bottom centre of the placement's bounds (the scenery/machine probe blocks are
  not matched to objects yet: their 8-byte object ids are not decoded);
- exposure: a flat `x4` (`HALO3_EXPOSURE` overrides) before bevy's default tonemapper, picked by
  comparing with in-game screenshots. Specular, fog and Halo's own exposure/tonemap are not done.
