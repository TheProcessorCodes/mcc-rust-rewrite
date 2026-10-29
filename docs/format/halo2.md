# MCC Halo 2 (classic) maps

What `crates/blam_h2` reads, written 2026-10-28 for the multiplayer map Coagulation
(`halo2\h2_maps_win64_dx11\coagulation.map`, the Halo 2 remake of Blood Gulch). Every fact was
checked against the user's files with the read-only probes in `docs/format/scripts/`
(`h2_common.py`, `h2_decompress.py`, `probe_h2_collision.py`, `probe_h2_scenario.py`,
`probe_h2_shaders.py`, `probe_h2_templates.py`, `probe_h2_bitmaps.py`; run them with `python -I`).

Sources for where to look: Reclaimer's Halo 2 classes and HaloExportH2's `README-H2.md` (both
GPL-3.0, `D:\reflex-tools\halo-tools`), and Assembly's `Halo2MCC` / `Halo2` plugin XMLs
(GPL-3.0, fetched to `.research-cache/assembly-h2`). Only offsets, sizes and meanings were
taken; no code was copied. Where they disagreed with the bytes, the bytes won (noted below).

All numbers little-endian. Offsets hex. "Block" = 8 bytes: element count (u32), then a tag-data
pointer (u32, section 2). "Tag reference" = 8 bytes: class fourcc, then the tag's datum. A
"short reference" is the datum alone (4 bytes). Units are world units (1 wu = 10 ft = 3.048 m,
the same as Halo 3; HaloExportH2's probe measures Master Chief at 0.704 wu), +Z up.

## 1. Header and compression

The first `0x380` bytes are the header, always stored as is.

| offset | type | meaning |
|---|---|---|
| 0x000 | u32 | `head` (0x68656164) |
| 0x004 | u32 | version, 13 (Halo 3 MCC maps are 13 too) |
| 0x008 | u32 | size of the file once inflated (coagulation 0xAA6E80) |
| 0x010 | u32 | tag index address (file offset in the inflated file) |
| 0x014 | u32 | tag index size |
| 0x01C | u16 | flags; bit 0: compressed |
| 0x020 / 0x024 / 0x028 / 0x02C | u32 | tag name count, name table, its size, name index table |
| 0x030 / 0x034 / 0x038 / 0x03C | u32 | string count, string table, its size, string index table |
| 0x0B0 | char[32] | map name (`coagulation`) |
| 0x0D0 | char[256] | scenario path (`scenarios\multi\halo\coagulation\coagulation`) |
| 0x2D0 | u32 | tag-data address mask (0x03200000) |
| 0x2D4 / 0x2D8 | u32 | tag data offset (from the index) and size |
| 0x308 | u32 | chunk size once inflated (0x40000) |
| 0x30C | u32 | first chunk's file offset (0x380) |
| 0x310 / 0x314 | u32 | chunk table offset, chunk count (coagulation: 43) |

**Telling Halo 2 from Halo 3:** both say `head`, 13. Halo 2 has its scenario path at 0x0D0,
always starting `scenarios\`; Halo 3 has its map name and build there (`riverworld`,
`levels\...` further on). `blam_h2::is_halo2` uses that.

**Compression:** the chunk table holds `count` pairs (i32 size, u32 file offset). A positive
size is a zlib stream at that offset; a negative size is `-size` bytes stored after a two-byte
prefix. Inflated one after the other behind the header they give the whole file; the total
must equal 0x008 (coagulation: 43 zlib chunks, exactly 0xAA6E80; `shared.map`: 173 chunks,
0x2B04800). Every offset below is in the inflated file. The reader inflates in memory
(0.02 s); `h2_decompress.py` can write an inflated copy under `.scratch/h2/` for probing.

## 2. Tag index

At the index address:

| offset | meaning |
|---|---|
| +0x00 | class table, relative to the index address (0x20: right after this header) |
| +0x04 | class count (120) |
| +0x08 | tag table, relative to the index address |
| +0x18 | tag count (23,131 slots) |

Class entries are 12 bytes (fourcc, parent, grandparent; each fourcc stored byte-reversed).
Tag entries are 16 bytes: fourcc (reversed), datum (salt << 16 | index), tag-data pointer, size.

- A datum of 0xFFFFFFFF is an empty slot (coagulation: 17,255 of them).
- Pointer and size both 0: the tag lives in **`shared.map`** beside the map, under the same
  datum and slot (5,608 such tags on coagulation; all match by class and name). That is where
  most shaders, templates, multiplayer objects (`flag_base`, `2way_teleporter`) and their
  render models are.
- **Tag-data pointers** (in the index and inside tags) become file offsets as
  `pointer - mask (0x2D0) + index address`. The first tag (the scenario) lands at
  index + tag data offset (0x2D4).

Tag names: the name index table holds one i32 offset per slot into the name table, each a
NUL-terminated path. **String ids**: the low 24 bits index the string index table (one i32
offset into the string table per string); the high byte is the length. Checked on instance
names (`boulder_narrow`, `boulder_tall`).

## 3. Raw data pointers

Geometry raw data is referenced by a u32 whose top two bits say which file holds it
(0 this map, 1 `mainmenu.map`, 2 `shared.map`, 3 `single_player_shared.map`) and whose low 30
bits are the offset in that file's inflated bytes. Render models from `shared.map` point with
location 2 (`0x80000380`); local models and BSP clusters with 0. **Bitmap pixels** always live in
`textures.dat` (section 9), whatever the top bits say.

## 4. Scenario (`scnr`) fields used

| offset | block element | meaning |
|---|---|---|
| 0x008 | 8 | skies: tag reference to a `sky ` |
| 0x050 | 0x5C | scenery placements |
| 0x058 | 0x28 | scenery palette: tag reference at +0 |
| 0x0A8 / 0x0B0 | 0x48 / 0x28 | machines, their palette |
| 0x0B8 / 0x0C0 | 0x44 / 0x28 | controls, their palette |
| 0x100 | 0x34 | player starting locations: **the multiplayer respawn points** |
| 0x108 | 0x44 | trigger volumes |
| 0x118 | 0x20 | netgame flags (flag, hill, teleporter points; not read) |
| 0x210 | 0x44 | structure BSPs: `sbsp` reference at +0x10, lightmap `ltmp` at +0x18 (the first three u32s are zero on MCC) |
| 0x230 | 2 | kill triggers: i16 indices into the trigger volumes |
| 0x328 / 0x330 | 0x4C / 0x28 | crates, their palette |

Object placements share a head: palette index i16 +0x00, name index +0x02, position +0x08,
rotation (yaw, pitch, roll, radians) +0x14, scale +0x20 (0 means 1), origin BSP i16 +0x2C.
They are composed like Halo 3's (`blam_geometry::scenario::rotation`).

Object tags (`scen`, `bloc`, ...) name their model (`hlmt`) at +0x34; the model names its
render model (`mode`) at +0x00.

Player starting location: position +0x00, facing (radians) +0x0C, team +0x10, BSP +0x12, game
types +0x14 (four u16). Coagulation has **123**, all game type 12, z from -2.4 to 6.0 wu.

Trigger volume: name string id +0x00, forward +0x0C, up +0x18, corner position +0x24, extents
+0x30 (the Halo 3 meaning). Coagulation has none, and no kill triggers.

## 5. Geometry sections

A **section** is the unit of render geometry: each structure BSP cluster, each instanced
geometry definition, each render model section. The section block holds counts and a
reference to the section's raw data:

| field | BSP cluster / instance definition | render model section (0x5C) |
|---|---|---|
| vertex count u16 | +0x00 | +0x04 |
| triangle count u16 | +0x02 | +0x06 |
| raw data pointer (section 3) | +0x28 | +0x38 |
| raw data size | +0x2C | +0x3C |
| header size | +0x30 | +0x40 |
| resources block (0x10 each) | +0x38 | +0x48 |

Reclaimer reads the render model's header size at +0x44; on these maps +0x44 is the resources'
total size and the header size is at +0x40 (raw size = header + resources + 12 in both
layouts: flag_base 0x78 + 15,664 + 12 = 15,796).

The raw data starts with `blkh` and a size; the section's resources start at `header + 8`,
with a `rsrc` marker. Each resource entry:

| offset | meaning |
|---|---|
| +0x0 | u8 kind: 0 a tag block, 1 a data reference, 2 a vertex buffer |
| +0x4 | i16 the field it fills in the section's render data: 0x00 parts, 0x08 subparts, 0x20 strip indices, 0x28 visibility MOPP, 0x30 MOPP reorder table, 0x38 vertex buffers |
| +0x6 | i16 element size for blocks; buffer number for vertex buffers |
| +0x8 | u32 byte size |
| +0xC | u32 offset from `header + 8` |

- **Parts** (0x48 each): material index i16 +0x04 (into the BSP's materials or the model's
  shaders), first index u16 +0x06, index count u16 +0x08.
- **Indices**: u16. On coagulation every section is a **triangle list** (index count = 3 x
  triangle count: 738 = 3 x 246). A section where that does not hold, or whose first triangles
  repeat a vertex, is a strip (read with degenerate triangles skipped and the winding
  alternating).
- **Vertex buffers** (kind 2, field 0x38): buffer 0 positions, three floats (stride 12 in BSPs;
  12, 16 or 20 in models, the rest being node indices and weights); buffer 1 texcoords, two
  floats; buffer 2 normal, binormal, tangent, nine floats (36 bytes); buffer 3 the lightmap
  texcoord, two floats, 0..1 over the section's own lightmap page (section 10); buffer 4 three
  floats per vertex, not read (a direction, presumably the incident light's for bump mapping).
  Light fixtures' instanced definitions have no buffer 3. Positions are not compressed on PC.
- Render model regions (+0x1C, 16 bytes: name sid, permutations block +0x08 of 16 bytes: name
  sid, then six i16 section indices from the lowest level of detail to the highest). We take the
  first permutation's highest present level. Shaders block at +0x60 (0x20 each, shader
  reference at +0x08).

## 6. Structure BSP (`sbsp`)

| offset | meaning |
|---|---|
| 0x00C | collision materials (0x14): old shader reference +0x00, global material i16 +0x08, shader reference +0x0C |
| 0x014 | collision BSP (0x40 each; one) |
| 0x02C | leaves (0x08: cluster i16) |
| 0x034 | world bounds: x min, max, y min, max, z min, max |
| 0x05C | cluster portals (0x24) |
| 0x09C | clusters (0xB0): section fields above; bounds +0x54 (six floats); sky index i8 +0x6C |
| 0x0A4 | materials (0x20): shader reference +0x08 |
| 0x138 | instanced geometry definitions (0xC8): section fields above; its collision BSP blocks at +0x70 |
| 0x140 | instanced geometry instances (0x58) |
| 0x214 | water definitions (0xAC; not read: the water surface is also a cluster part) |

**Instances**: scale +0x00, forward +0x04, left +0x10, up +0x1C, position +0x28, definition i16
+0x34, flags +0x36, name string id +0x50. The transform's columns are forward, left, up (times
the scale), then the position.

**Collision BSP** (the nine blocks at the BSP's collision element, or at +0x70 of an
instanced definition): 3D nodes +0x00 (8 bytes), planes +0x08 (16), leaves +0x10 (4), 2D
references +0x18 (4), 2D nodes +0x20 (16), **surfaces +0x28 (8 bytes: plane i16, first edge u16
+0x02, flags u8 +0x04, breakable u8 +0x05, material i16 +0x06)**, **edges +0x30 (12 bytes: start
vertex, end vertex, forward edge, reverse edge, left surface, right surface, all u16)**,
**vertices +0x38 (16 bytes: point, first edge u16)**. It is Halo 3's normal collision BSP with a
smaller surface. A surface's polygon is walked as in Halo 3: from its first edge, take the
start vertex and the forward edge when the edge's left surface is this one, else the end vertex
and the reverse edge, until back at the first edge. The loops come out counter-clockwise seen
from outside: fanned into triangles, 6,035 of coagulation's 8,586 face up.

Surface flags are Halo 3's bits: 1 two-sided, 2 invisible, 4 climbable, 8 breakable, 16 invalid.
Coagulation: 6,386 surfaces with no flag, 16 two-sided, 264 two-sided and invisible (the
invisible walls round the canyon).

| Coagulation | value |
|---|---|
| clusters | 48 (12,797 triangles) |
| instanced definitions / instances | 22 / 63 (5,591 triangles placed) |
| BSP collision | 15,117 nodes, 5,362 planes, 6,666 surfaces, 10,959 edges, 4,102 vertices: **8,586 triangles**, 0 broken loops |
| instanced collision | **4,502 triangles** placed |
| bounds | x -25.6..179.2, y -230.4..0.0, z -4.4..115.2 wu |

The render counts are exactly HaloExportH2's (`.scratch/h2-coagulation/export/halo_export.json`:
world 12,797, instance 5,591, scenery 2,324, crate 2). HaloExportH2 reads no collision, so the
collision numbers are ours alone.

## 7. Sky (`sky `)

Render model reference at +0x00. Coagulation's sky model has 8 pieces, 349 triangles: stars,
two moons, the ring, a sun glow, the heavenly glow, a fog band and cloud cover.

The root is 0xAC bytes, with three blocks of one element each on Lockout and Coagulation:

| offset | element | meaning (read from the values) |
|---|---|---|
| +0x1C | 0x0C | a tag reference and a float (null and 0.8 on Lockout); not read |
| +0x48 | 0x18 | **fog**: colour (three floats, gamma space) +0x00, maximum density +0x0C, start distance +0x10, opaque distance +0x14 (world units) |
| +0x78 | 0x5C | a light: zero colour, then two angles (0.698, -0.436 rad on Lockout), a null reference, a colour (0.59, 0.61, 0.70) and power 0.25; not read |

| map | fog colour | density | start..opaque |
|---|---|---|---|
| Lockout | 0.510, 0.588, 0.800 | 0.5 | 0..40 wu |
| Coagulation | 0.64, 0.51, 0.51 | 0.4 | 10..150 wu |

The viewer draws the fog as a linear distance fog whose colour also fills the background, in
place of the sky model (`docs/viewer.md` "Halo 2 maps").

## 8. Shaders (`shad`) and templates (`stem`)

A Halo 2 shader names its template (`shaders\shader_templates\<opaque|transparent|water>\...`) at
+0x00. The MCC cache keeps no parameter names: the shader's parameter block (+0x18) is empty
and so are the template's categories. What is left:

| shader offset | meaning |
|---|---|
| 0x000 | template reference |
| 0x00C | runtime properties (0x50): diffuse map reference at +0x00 |
| 0x020 | runtime definition (first element used): bitmaps block +0x04 (0x0C: short reference +0x00), pixel constants +0x0C (4), vertex constants +0x14 (16 bytes: four floats) |

The template's runtime definition is at +0x58 (0x28): passes +0x10 (0x0A: pass reference +0x00,
implementation range u16 +0x08), implementations +0x18 (6 bytes: bitmap range +0x00, pixel
constant range +0x02, vertex constant range +0x04), remappings +0x20 (4 bytes: source index u8
+0x03). A **range** u16 is a 9-bit start and a 7-bit count: `start = v & 0x1FF`, `count = v >> 9`
(checked against HaloExportH2's dump: pass 0x0E08 is "implementations 8, count 7").

For a pass: its first implementation's bitmap range picks remappings; each remapping's source
index is the shader's runtime bitmap for that pass slot. Same for vertex constants.

### 8.1 Pass slots

What a slot holds depends on the pass. Our table (`blam_h2::shader::pass_slots`), built from
coagulation's shaders with HaloExportH2's `--info --shaders` dump as the check:

| pass | slots |
|---|---|
| `lightmap_tex` | bump, base |
| `lightmap_tex_detail`, `..._alpha_test` | bump, base, detail |
| `lightmap_tex_detail_alpha_test_no_bump` | base, detail |
| `lightmap_tex_detail_overlay` | bump, base, detail, overlay detail |
| `lightmap_tex_detail_blend_detail` | bump, base, detail, secondary detail, overlay detail |
| `lightmap_tex_env` | bump, base, environment |
| `lightmap_tex_env_detail` | bump, base, environment, detail |
| `default_transparent_one_sided` | environment, specular, self-illumination, alpha blend |
| `plasma_alpha` | noise, noise, alpha |

The base colour is read from the template's first `lightmap*` pass. For the other templates
(sky, `one_add_illum`, `default_transparent_detail`) the shader's runtime diffuse map is used,
else its first bitmap that is not a bump, reflection or cube map.

### 8.2 Tiling

In the `lightmap*` passes the vertex constants tile the slots in order, **the environment map
left out** (it takes no constant): `lightmap_tex_env_detail` reads bitmaps [bump, base, env,
detail] but only three constants, for bump, base and detail (`coag_panel_i`: 1 1 | 1 1 | 16 16).
A constant is scale x, y and offset z, w; the overlay detail's (`0 0 1 1`, `0 0 10 10`) keeps
its scale in z, w. Detail tilings are relative to the base map's (multiplied by its scale).

### 8.3 Blending

From the template path: `transparent\*` with `add`, `illum` or `plasma` in the name is additive
(`coag_light_bands_*`, the teleporter glows, the grav lift), `multiply` multiplies, other
transparent templates alpha-blend; `water\*` is water; `alpha_test` in the name cuts out
(`multi_banner`, `redcedarbough`). Shaders named `invisible`, `portal`, ... and `shaders\invalid`
are not drawn.

### 8.4 Detail-blend ground

`tex_bump_detail_blend*` (coagulation's `coag_ground`): colour = base.rgb x lerp(detail,
secondary detail, base.alpha) x 2 (HaloExportH2's README). The viewer draws it through the Halo 3
terrain path with a two-layer blend map made from the base map's alpha (red = 1 - a, green = a),
each layer the base map times one detail map.

## 9. Bitmaps (`bitm`)

Images block at +0x44, 0xA8 each:

| offset | meaning |
|---|---|
| +0x04 / +0x06 | width, height (u16) |
| +0x08 | depth (u8) |
| +0x0A | type: 0 2D, 1 3D, 2 cube |
| +0x0C | format (below) |
| +0x0E | flags; 0x08 swizzled (none seen on PC) |
| +0x14 | mipmap count: levels after the first |
| +0x38 | pixel pointer into `textures.dat` (top two bits ignored) |
| +0x50 | its stored (compressed) size |

Formats: 0 A8, 1 Y8, 2 AY8, 3 A8Y8, 6 R5G6B5, 8 A1R5G5B5, 9 A4R4G4B4, 10 X8R8G8B8, 11
A8R8G8B8, 14 DXT1, 15 DXT3, 16 DXT5, 17 P8 bump, 18 P8, 22 U8V8 (Halo 3's numbers where both
have the format).

### 9.1 `textures.dat`

At the pointer: an i32 chunk count `n`, `n` i32 chunk sizes (negative: stored, that many
bytes), then the chunks back to back, each a zlib stream. Inflated they are the whole mip chain:
**plain rows (or BCn blocks), level 0 first**, cube faces one after the other. On coagulation
every one of the 1,833 images listed inflates to at least its full chain
(`probe_h2_bitmaps.py`). Uncompressed rows are tight: `width x bytes per pixel`, no padding
(Reclaimer's A8R8G8B8 path pads rows by the registration point, which is wrong for MCC; see
HaloExportH2's README).

## 10. Lightmaps (`ltmp`)

The scenario's BSP entry names the lightmap at +0x18 (section 4). Checked on Lockout and
Coagulation with `scripts/probe_h2_lightmap.py`.

| offset | meaning |
|---|---|
| root +0x80 | lightmap groups (0x84 each; one on both maps) |
| group +0x08 | 31 elements of 0x400 bytes (on Lockout; palettes of 256 colours by their size; not read) |
| group +0x18 | the lightmap bitmap (`bitm` reference) |
| group +0x20 | clusters (0x54 each, one per BSP cluster): geometry section fields with a null raw pointer; not read |
| group +0x28 | **cluster render info** (4 bytes): page (bitmap image) index i16 +0, palette index i8 +2 |
| group +0x30 | instanced definitions (0x54 each, one per definition); not read |
| group +0x48 | **instance render info** (4 bytes), one per instance: page index i16 +0 (-1: none) |
| group +0x50 | one per instance; not read |

The pages are the bitmap's images: plain **A8R8G8B8** light colours on MCC (Lockout: 65 pages
from 32x64 to 512x1024, the unused texels a flat lilac 116, 114, 216), so the palette indices
have nothing to index. Lockout's clusters use pages 0-15 in order, its instances 16-64; the 26
light fixtures have none and no lightmap texcoords. A vertex's light is the page's texel at its
buffer 3 texcoord (section 5); an instance reads its own page through its definition's
texcoords.

How the game combines the light with the surface is not in the map (MCC's PC shaders are not
in the `.map`). The viewer treats the texel as a gamma-space colour that multiplies the
surface, doubled (`halo3::halo2::LIGHTMAP_SCALE`, picked by eye on Lockout): dark blue snow
outside, warm lamp pools inside.
