# MCC Halo: Reach maps

What `crates/blam_reach` (and the Reach branches of `blam_cache` and `blam_texture`) read,
written 2026-10-31 for the 20 multiplayer maps of the user's MCC install
(`haloreach\maps`, build `Jun 21 2023 15:35:31`, MCC update 13). Every fact was checked
against the user's files with the read-only probes in `docs/format/scripts/`
(`reach_common.py`, `probe_reach_bitmaps.py`, `probe_reach_shaders.py`,
`probe_reach_geometry.py`; run them with `python -I`), and the counts are pinned by
`cargo test -p blam_reach`.

Where to look came from Reclaimer's `HaloReach` / `MccHaloReach` classes, TagTool's
collision and instanced-geometry definitions and Assembly's `ReachMCC` plugin XMLs (all
GPL-3.0, read for offsets and meanings only; nothing was copied). Where they disagreed with
the bytes, the bytes won.

Offsets are hex, little-endian. A "block" is 12 bytes (count, packed pointer, unused), a
"tag reference" 16 bytes (class, ..., datum at +0xC), as in Halo 3.

## 1. The cache file

Reach's header has **Halo 3's fields at Halo 3's offsets** (`head`, version 13, build at
0xA0, map name 0xC0, scenario path 0xE0, virtual base 0x2E0, tag index 0x2E8, partitions
0x300, section offsets 0x4CC, sections 0x4DC). Two things differ:

- **The header is 40 KiB**: the first section with bytes starts at file offset 0xA000
  (Halo 3: 0x4000). `blam_cache` (`Engine`) tells the games apart this way; it held for all
  44 Reach files and every Halo 3 file of the install (`shared.map` and `campaign.map`
  have only a resource section, which starts at 0xA000 / 0x4000).
- **Packed tag-data pointers are offset**: the 64-bit tag address is
  `(pointer << 2) + 0x50000000` (Halo 3: `pointer << 2`). Tag index entries, blocks and
  every other packed pointer use it.

String ids split 19 / 8 / 5 bits as Halo 3 U13, with the namespace table from the header.
Every Reach map has one empty tag slot.

**Resources** are Halo 3's system unchanged: the `zone` tag's entries at +0x64 (64 bytes),
fixup data at +0x148 / +0x154, the `play` tag's shared caches (+0x0C), pages (+0x18, 88
bytes) and segments (+0x3C, 16 bytes), deflate pages. A page's bytes are in the resource
section of its file; `shared.map` and `campaign.map` sit next to the map.

## 2. Scenario (`scnr`)

| offset | element | meaning |
|---|---|---|
| 0x50 | 0xAC | structure BSPs: `sbsp` reference at +0 |
| 0x88 | 0x30 | skies: a `scen` reference at +0 (the sky is a scenery object) |
| 0xAC | 0x13C | zone sets: name +0, BSP mask +0x10C |
| 0xF0 | 8 | object names: string id at +0 |
| 0xFC / 0x108 | 0xDC / 0x10 | scenery, palette |
| 0x15C / 0x168 | 0xD0 / 0x10 | weapons, palette |
| 0x180 / 0x18C | 0xE4 / 0x10 | machines, palette |
| 0x1B0 / 0x1BC | 0xDC / 0x10 | controls, palette |
| 0x274 | 0x1C | player starting locations: position +0, facing (yaw, pitch) +0xC |
| 0x280 | 0x7C | trigger volumes: name +0, forward +0x10, up +0x1C, corner +0x28, extents +0x34 |
| 0x4C8 | 4 | kill triggers: i16 trigger volume index |
| 0x600 / 0x60C | 0xD8 / 0x10 | crates, palette |
| 0x708 | ref | the scenario lightmap (`sLdT`) |

Object placements share Halo 3's head (palette index +0, name index +2, position +8,
yaw/pitch/roll +0x14, scale +0x20); the origin BSP is at +0x40. Objects name their model at
+0x64 (`hlmt`), the model its render model at +0.

**Spawns** are scenery: `objects\multi\spawning\respawn_point` (and `_invisible`,
`initial_spawn_point`), with game-type data later in the placement (team +0xA9). Every map
has 164 to 396 of them except Forge World (19). Multiplayer maps have one zone set and one
starting location (Glacier has none).

Each map's single zone set leaves out the `_hidden` BSPs; the `_shared` / second BSPs hold
the distant scenery around the play space.

## 3. Structure BSP (`sbsp`)

| offset | element | meaning |
|---|---|---|
| 0x58 | 0x18 | collision materials: shader reference at +0 |
| 0xF0 | | world bounds: x, y, z ranges |
| 0x138 | 0x8C | clusters: bounds +0, sky index i8 +0x18, **mesh index i16 +0x40** |
| 0x144 | 0x2C | materials: shader reference at +0 (the meshes' part shader indices) |
| 0x258 | 4 | instance names (string id); the instances themselves are in a resource (5) |
| 0x450 / 0x45C / 0x4E0 | | the BSP's own render geometry: meshes, compression, resource |
| 0x518 | | resource `structure_bsp_tag_resources`: collision (6) |
| 0x520 | | resource `structure_bsp_cache_file_tag_resources`: instances (5) |

The final geometry is the **lightmap BSP data's** (`Lbsp`): the scenario lightmap (`sLdT`)
lists one reference per BSP at +4 (0x20 each, in BSP order; BSPs with no geometry have an
empty entry). Cluster mesh `n` uses compression box `n`.

`Lbsp`: intensity bitmap +0x2C, direction bitmap +0x1C, per-instance lighting +0x60 (0xC
each), meshes +0x7C, compression +0x88, per-instance lightmap texcoords +0xF4 (i16 each),
geometry resource +0x10C.

## 4. Meshes

Reach's mesh (0x5C) is Halo 3's section widened:

| offset | meaning |
|---|---|
| +0x00 | parts (0x18 each): shader i16 +0, index start u32 +4, index count u32 +8, flags u16 +0x12 |
| +0x0C | subparts (0x10 each): index start u32, count u32, part u16 |
| +0x18 | eight i16 vertex buffer slots: 0 the vertices, 1 (clusters) lightmap texcoords |
| +0x28 | index buffer i16 |
| +0x2C | flags u16 (bit 4 unindexed, bit 8 positions not compressed) |
| +0x2E / +0x2F / +0x32 | rigid node, vertex type, index buffer type |

Compression boxes are 0x34 bytes, ranges at +4 (x, y, z, u, v).

## 5. Render geometry resource

The definition holds, in order: every vertex buffer description (28 bytes: count +0, buffer
type u16 +4, stride u16 +6, byte length +8), one 12-byte pointer per vertex buffer, every
index buffer description (28 bytes: format +0 (3 list, 5 strip), byte length +8), one
pointer per index buffer, then the vertex and index buffer blocks 24 and 12 bytes from the
end (count first). Fixup `i` is vertex buffer `i`'s offset in the data, fixup
`2 x vertex buffers + j` index buffer `j`'s (Halo 3's order). Indices are 16-bit unless the
buffer has more than 65535 vertices.

Vertex layouts (mesh vertex type / buffer type, stride):

| type | buffer | stride | layout |
|---|---|---|---|
| 0 world | 1 | 36 | position 3 x f32 +0, texcoord 2 x f16 +0x10, normal 4 x i16n +0x14, tangent 4 x i16n +0x1C |
| 1 rigid | 2 | 36 | position 3 x f32 **0..1 in the compression box** +0, texcoord 2 x u16n +0x10 (in the box), normal +0x14, tangent +0x1C |
| 2 skinned | 3 | 44 | rigid, then bone indices 4 x u8 +0x24, weights 4 x u8n +0x28 |

Lightmap texcoords are buffer type 4 or 9, 4 bytes (2 x u16n). Sword Base's main BSP: 557
meshes, 2001 vertex and 507 index buffers, every index in range.

## 6. Collision and instances

`structure_bsp_tag_resources` (sbsp +0x518) root: collision BSPs +0, large collision BSPs
+0xC, instanced geometry definitions +0x18 (0x144 each).

Reach's **collision geometry is 0x6C bytes**: Halo 3's eight blocks with a 3D supernodes
block (0x80 each) inserted second: nodes +0, supernodes +0xC, planes +0x18, leaves +0x24,
2D references +0x30, 2D nodes +0x3C, **surfaces +0x48, edges +0x54, vertices +0x60**.
Surfaces, edges and vertices are Halo 3's (12, 12, 16 bytes), so
`blam_geometry::collision::triangulate_geometry` walks them at the geometry + 0xC.

**Large collision BSPs** (+0xC) have the same 0x6C struct with Halo 3's 32-bit element
sizes: 3D nodes 12, supernodes 128, planes 16, leaves 8, 2D references 8, 2D nodes 20,
surfaces 16, edges 24, vertices 20 (strides measured from consecutive block addresses on
Forge World). Eight multiplayer maps keep some or all of their structure collision there:
Forge World's main BSP has no normal collision BSP at all, only one large one (60,685
surfaces). The others are Beaver Creek, Breakpoint, Highlands, Damnation, Timberland,
Prisoner and Tempest. `triangulate_large_geometry` walks them.
An instanced geometry definition has a checksum +0, flags +4 and its collision geometry
inline at +8.

`structure_bsp_cache_file_tag_resources` (sbsp +0x520) root +0x48: the **instances** (0x9C
each): scale +0, 3x3 rotation rows and translation +4, definition index i16 +0x34 (into the
collision definitions), flags u16 +0x36 (bit 1 render only, bit 3 collidable), mesh i16
+0x38, compression box i16 +0x3A, bounds +0x4C.

## 7. Render models (`mode`)

Regions +0x0C (16 bytes; permutations 24 bytes, first mesh i16 +4, count +6), instanced
mesh index +0x1C, instances +0x20 (60 bytes: name, node +4, scale +8, 3x4 matrix +0xC),
nodes +0x30 (96 bytes, as Halo 3), shaders +0x48 (0x2C each), meshes +0x68, compression
+0x74 (one box for the model), geometry resource +0xF8.

## 8. Bitmaps (`bitm`)

Bitmap data block +0x7C (0x38 each: width +0, height +2, depth +4, type u8 +6, format u16
+8, mipmaps +0x10, curve +0x11), resources +0xA8 (8 bytes, resource id first), interleaved
resources +0xB4.

The **resource definition is 72 bytes**: Halo 3's texture description with four more bytes
before the DXGI format: primary size +0, secondary size +0x14, width +0x28, height +0x2A,
depth +0x2C, levels +0x2D, type +0x2E, flags +0x2F (bit 0: mip 0 in the secondary page),
**DXGI format +0x34, Halo format +0x38, curve +0x39**. Pixel layout in the pages is Halo 3's.

### 8.2 Formats

The numbering matches Halo 3's up to 32; Reach then has five more before DXN. Seen on the
user's maps (format, DXGI): 0 A8 (65), 1 Y8 (61), 3 A8Y8 (49), 11 A8R8G8B8 (87, 91 sRGB),
14 DXT1 (71, 72), 15 DXT3 (74, 75), 16 DXT5 (77, 78), 22 U8V8 (51), 25 RGBA FP16 (10),
28 (31), 30 (11), 34 (13), **36 DXT5a / BC4 (80)**, **38 DXN / BC5 (84)**, **44 DXN mono
alpha (83)**. `blam_texture` renumbers 36 as Halo 3's 31 and 38 / 44 as 33.

## 9. Shaders

Halo 3's render method chain (`shaders.md`) at Reach's offsets: definition (`rmdf`)
reference +0, chosen options (i16 each) **+0x20**, properties **+0x38** (**0xB4** each:
template `rmt2` +0, textures +0x10 (**24** bytes each, tiling constant index byte
**+0x15**), float constants +0x1C (16 each), routing +0x50, functions +0x5C). The `rmdf`
categories (+0x10) and the `rmt2` constant and texture names (+0x48, +0x6C) are where
Halo 3 has them. Option and texture names are Halo 3's (`albedo`, `bump_mapping`,
`base_map`, `detail_map`, terrain `base_map_m_0`...), so the viewer's material code
reads Reach shaders unchanged.

## 10. Lightmaps

Reach bakes "dual vMF" lightmaps: per BSP a direction bitmap (`..._dualvmf_pixel_direction`,
BC5, one layer) and an intensity array (`..._dualvmf_pixel_intensity`, DXT5, three
layers) over one atlas (Sword Base: 1536 x 1536). Layer 0 is the colour of the main lobe
(warm in sunlight, sky-blue in shade); layer 2 a second, dim lobe; layer 1 is near-constant
mid grey. The colour layers' alphas follow the shadows while their colours stay near full
brightness, so alpha reads as each lobe's brightness. The exact lobe maths is not worked
out: the viewer takes `layer0.rgb * layer0.a + layer2.rgb * layer2.a` as a direction-free
light and scales each map so its lit texels average one level (`mcc_textures::reach`).

Clusters read their atlas texcoords from their mesh's buffer slot 1. An instance is lit
per pixel when `Lbsp + 0x60` entry `+0` is not -1; its texcoords are then the vertex buffer
named by `Lbsp + 0xF4` (one i16 per instance). Otherwise `+0x60` entry `+2` indexes a
per-vertex lighting record (`Lbsp + 0x48`, 4 bytes: vertex buffer, ...) or `+4` a light
probe colour (`Lbsp + 0x6C`, 0x24 bytes); neither is read yet.

## 11. Not read yet

Per-vertex and probe lighting, the lobe directions, cluster portals and the point-to-cluster
lookup, atmosphere / fog (`skya`, `atgf` at scnr +0x6D8 / +0x6E8), camera fx (`cfxs` at
+0x6A8), sound, weapons and the player's movement numbers, machines' and crates' collision
models, Forge World's Forge-variant objects.
