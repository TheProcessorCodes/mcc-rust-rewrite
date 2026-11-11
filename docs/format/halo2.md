# MCC Halo 2 (classic) maps

What `crates/blam_h2` reads, written 2026-10-28 for the multiplayer map Coagulation
(`halo2\h2_maps_win64_dx11\coagulation.map`, the Halo 2 remake of Blood Gulch). Every fact was
checked against the user's files with the read-only probes in `docs/format/scripts/`
(`h2_common.py`, `h2_decompress.py`, `probe_h2_collision.py`, `probe_h2_scenario.py`,
`probe_h2_shaders.py`, `probe_h2_templates.py`, `probe_h2_bitmaps.py`; run them with `python -I`).

Sources for where to look: Reclaimer's Halo 2 classes and HaloExportH2's `README-H2.md` (both
GPL-3.0, <https://github.com/Gravemind2401/Reclaimer>), and Assembly's `Halo2MCC` / `Halo2` plugin XMLs
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
  Light fixtures' instanced definitions have no buffer 3. BSP cluster and instance positions
  are plain world floats on PC; **render model** positions and texcoords are not (below).
- **Render model compression** (+0x14, one 0x38-byte element): ten floats as min/max pairs,
  x, y, z, u, v (then four more, a second texcoord's, zero on Lockout). Every render model
  section (scenery, crates, skies) stores its positions and texcoords -1..1; the real value is
  `min + (v + 1) / 2 * (max - min)`. Found on Lockout (2026-10-30), where reading them raw drew
  every prop as a 2 wu blob: the five `flag_base` plates (really 0.44 wu across, 0.05 tall)
  as large round plates, the fourteen `power_module` canisters (0.26 x 0.26 x 0.82 wu) as
  giant dark rocks, and the lift's `lockout_lift_tube` crate (a 0.2 wu `generic_grey`
  octahedron marker under the pad) as a pyramid on the gravity lift. Same on Coagulation (its
  teleporter, flag bases and sky); the triangle counts never showed it. The skies decode to
  about 150 wu in radius, the size `SKY_SCALE` assumes.
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
surface, doubled (`mcc_rust_rewrite::halo2::LIGHTMAP_SCALE`, picked by eye on Lockout): dark blue snow
outside, warm lamp pools inside.

## 11. Weapons

Read by `blam_h2::weapons`, checked on Lockout with `scripts/probe_h2_weapons.py`. All weapon
tags live in `shared.map`.

**Where they come from.** A Halo 2 multiplayer scenario places no weapons through its weapon
block (+0x90, palette +0x98; empty on Lockout). Weapons and grenades are **netgame equipment**
(scenario +0x120), each naming an item collection; what a player spawns with is the **starting
equipment** (+0x128). Before this reader the viewer read neither, so walking on a Halo 2 map gave
no weapon at all: the Halo 3 weapon loader (`mcc_weapons::assets::load`) needs a Halo 3 cache
file, and the Halo 2 path never called anything in its place.

### 11.1 Netgame equipment (+0x120, 0x90 each)

| offset | meaning |
|---|---|
| 0x00 | u32 flags (1 on two of Lockout's entries; not read) |
| 0x04 | four u16 game type values (12, 0, 0, 0 on Lockout; not read) |
| 0x0E | i16 respawn seconds (30 most, 45 battle rifle, 25 sword, 15 shotgun, 39 sniper) |
| 0x12 | i16, 1 on the power weapons (sword, shotgun, sniper) and the two empty entries; not read |
| 0x14 | u8, 0 weapons, 8 grenades, 6 and 7 the two entries with no collection (meaning inferred) |
| 0x40 | position |
| 0x4C | yaw, pitch, roll (radians; the sword lies on its side: pitch 1.47, roll -1.55) |
| 0x58 | item collection (`itmc`) reference |

**Item collection** (`itmc`): entries at +0x00, 0x10 each: weight f32 +0x00, item reference
+0x04 (`weap` or `eqip`), variant string id +0x0C. Lockout's have one entry each; we take the
first.

**Starting equipment** (+0x128, 0x9C each): six item collection references from +0x3C. Lockout:
one entry, the SMG.

Lockout: 32 entries, **14 weapons** of 9 kinds (battle rifle x2, sword, magnum x3, needler x2,
plasma pistol, plasma rifle x2, shotgun, SMG, sniper rifle), 16 grenades (`eqip`, not placed by
the viewer) and 2 with no collection.

### 11.2 Weapon render model

Object +0x34 names the `hlmt`, whose +0x00 names the render model, as for scenery. The `mode`
also has:

| offset | meaning |
|---|---|
| 0x48 | nodes (0x60): name sid +0x00, parent i16 +0x04, first child +0x06, next sibling +0x08, translation +0x0C, rotation quaternion +0x18 (x, y, z, w; identity stored as 0, 0, 0, -1) |
| 0x58 | marker groups (0x0C): name sid +0x00, markers +0x04 (0x24: region i8 +0x00, permutation i8 +0x01, node i16 +0x02, translation +0x04, quaternion +0x10, scale +0x20, 0 on weapons) |

The same node layout as Halo 3's `mode` (there at +0x30), and the marker element is Halo 3's
too, except the node index is an i16 at +0x02 (region and permutation -1 on the Master Chief:
`ff ff 14 00` is `right_hand` on node 20, `r_hand`).

Halo 2 weapons have **no `right_hand` marker**: the model's origin (the root node) is the grip.
`primary_trigger` is the muzzle (battle rifle: 0.192, 0, 0.046 wu), `left_hand` the off hand.

### 11.3 `weap` fields

The root is 0x31C bytes. The blocks keep Halo 3's element layouts where both games have the
field; only the root offsets differ.

| offset | meaning |
|---|---|
| 0x1FE | zoom levels i16 (battle rifle 1, sniper 2) |
| 0x200 | zoom magnification min, max (battle rifle 2, 2; sniper 3.5, 9.5) |
| 0x2A8 | first person (0x10): render model reference +0x00, animations (`jmad`) +0x08 |
| 0x2C0 | magazines (0x5C): total initial i16 +0x06, total maximum +0x08, loaded maximum +0x0A, rounds reloaded +0x14 (battle rifle 108, 144, 36, 36; shotgun 36, 48, 12, 1) |
| 0x2C8 | triggers (0x40): behaviour i16 +0x06 (0 automatic, 1 one per pull, 2 charge, 4 burst: Halo 3's values), primary barrel +0x08, secondary +0x0A |
| 0x2D0 | barrels (0xEC): rounds per second min, max +0x04; shots per fire i16 x2 +0x1C; fire recovery time +0x20; magazine i16 +0x28; rounds per shot +0x2A; minimum error +0x70; error angle min, max +0x74; projectile reference +0x8C |

- One-shot-per-pull weapons (magnum, shotgun, sniper, plasma pistol) store **0 rounds per
  second**; the fire recovery time paces them. The reader puts 1 / recovery there (the viewer's
  fire loop takes the rate as given).
- The plasma rifle has one magazine of all zeros: read as no magazine (a battery weapon).
- The sword has no triggers or barrels (it is held but does not fire).

**Projectile** (`proj`): maximum range +0x16C, initial and final velocity +0x17C, +0x180 (wu, wu/s;
battle rifle 40, 400, 400; sniper 120, 1200, 1200; needler 60, 6, 6). The same 0x10 gap as Halo 3's
range and velocities. The damage effect is not read.

### 11.4 In the viewer

`halo2.rs` reads the above, decodes the weapons' third-person models through the same render
model path as scenery (`blam_h2::decode_models`, so the compression bounds of section 5 apply:
the battle rifle is 0.30 wu long) and hands them to `mcc_weapons::assets::from_scene`. Walking
then works as on Halo 3 maps: the SMG in hand, the 14 netgame weapons lying where the scenario
puts them, picked up, swapped and fired with the numbers above. Not done: grenades and other
`eqip` items are not placed; items rest at the netgame point as stored (the game drops them to
the floor with physics, so some hang a little above it); no first-person model or arms (the
viewer draws the third-person model in front of the camera, as for Halo 3); the Halo 2 Master
Chief is not decoded (the rider is still Halo 3's Spartan from Valhalla); no weapon sounds.

## 12. Gravity lifts (phantoms)

A Halo 2 lift is not a trigger volume and not a script: it is a **phantom**, a shape in a placed
object's physics model that collides with nothing but pushes whatever overlaps it. Checked on
every MCC multiplayer map with `scripts/probe_h2_lifts.py` (`python -I probe_h2_lifts.py all`);
offsets from Assembly's `Halo2MCC\phmo.xml` (fetched to `.research-cache/assembly-h2`), meanings
confirmed by the values. `blam_h2::lift` reads them into `Scene::lifts`
(`blam_geometry::LiftVolume`).

The chain: a scenery, machine, control or crate placement (section 4) -> its object tag
(+0x34) -> the model `hlmt`, whose **+0x20** names the physics model `phmo` (+0x00 is the render
model, +0x08 collision, +0x10 animation, +0x18 physics, all empty on Lockout's lift).

`phmo` blocks used:

| offset | element | meaning |
|---|---|---|
| +0x28 | 0x68 | **phantom types** (below) |
| +0x48 | 0xA0 | spheres: radius +0x38, translation +0x90 |
| +0x58 | 0x60 | pills (capsules): radius +0x38, ends +0x40 and +0x50 |
| +0x60 | 0xB0 | boxes: half sizes +0x40, rotation rows +0x70 / +0x80 / +0x90 (identity on every box seen), translation +0xA0 |
| +0x70 | 0x120 | polyhedra: bounding box half sizes +0x40, centre +0x50; vertices inline as groups of four (x x x x, y y y y, z z z z) from +0x70, count +0x100 |

Every shape starts with name string id +0x00, material i16 +0x04 and **phantom type i8 +0x1E**
(-1: an ordinary collision shape). Shapes are in the physics model's node space; every lift
seen uses node 0, which sits at the object's origin.

Phantom type (0x68):

| offset | meaning |
|---|---|
| +0x00 | flags: bit 2 negates gravity, bit 3 ignores players, bit 6 ignores vehicles (others: ignores weapons, crates, ...) |
| +0x04 / +0x05 | minimum / maximum object size (u8) |
| +0x08 | marker name (string id) that aims the push; empty: the object's origin and up |
| +0x1C | dead radius |
| +0x20 / +0x24 | centre acceleration / top speed: towards the marker |
| +0x28 / +0x2C | axis acceleration / top speed: towards the line through the marker along its direction |
| +0x30 / +0x34 | direction acceleration / top speed: along the marker's direction |
| +0x18, +0x38..+0x5C | Hooke's law, orbit and alignment terms; zero on Lockout, not read |

Accelerations in wu/s^2, speeds in wu/s. The marker comes from the render model's marker groups
(`mode` +0x58, 12 bytes: name string id, markers block +0x04 of 36 bytes: node i8 +0x02,
translation +0x04, rotation i, j, k, w +0x10). The push direction is the marker's **+X turned by
that quaternion** taken as written: the `direction_up` markers on Coagulation, Cyclotron and the
rest all come out (0, 0, 1) that way.

### 12.1 Lockout

The lift is the crate `scenarios\objects\multi\lockout\lockout_lift_tube` (crate 0) at
**(-1.5, 12.53, -12.2)**, angles (yaw -1.571, pitch 0, roll 0.122): its column leans 7 degrees
towards -Y, the way it throws you (towards -X before 2026-10-30, when the angles were composed about
the object's own axes; `scenario-and-tag-reading.md` 2.3). The render model is the pale blue light column (8 triangles, shader `grav_lift`). The
physics model has two polyhedra, both boxes 1.21 x 1.21 wu across:

| shape | local z | phantom type | marker | push |
|---|---|---|---|---|
| `lower_lift` | 0 .. 4.5 | 0, flags 4 (negates gravity) | `lower_lift_direction` at (0.01, 0, 0), +X turned to (0, 0, 1) | direction 10 / 4, axis 10 / 10 |
| `upper_lift` | 4.5 .. 6.5 | 1, no flags | `upper_lift_direction` at (0.01, 0, 4.5), (0.866, 0, -0.5) | direction 10 / 4 |

So the lower box carries a player up the shaft at 4 wu/s (12.2 m/s) with gravity off, held on
the middle; the upper box shoves him out over the top towards world -Y (into the map) and 37
degrees down, which kills most of the upward speed and puts him on the upper floor.
`crates/blam_h2/tests/lockout.rs` (`lockout_lift`) pins both boxes.

Walked headless on the real collision (`crates/mcc_foot/tests/lockout_lift.rs`, and
`mcc-rust-rewrite lockout --shot DIR --foot --view "-1.5,12.53,-11.9,-90,0"` with `MCC_FOOT_WALK=0,0`):
standing on the pad at z -12.2 he rises at 4 wu/s, peaks at z -6.2 after 2.2 s and lands at
(-2.07, 9.38, -8.10), on the upper walkway 4.1 wu above the pad, facing into the map.

### 12.2 Other maps

Every MCC multiplayer map's lifts read the same way (`probe_h2_lifts.py all`): Ascension and
Colossus jump pads (pills, direction 150 / 12 and 100 / 9), Coagulation's two `powerhaus` lifts
(pill up, pill out), Cyclotron, Foundation, Headlong and Midship (pills), Backwash and Gemini
(boxes), Derelict, Colossus and Dune's watchtower (polyhedra). Elongation's `crate_cushion` ignores players and is
skipped. Turf's springs are spheres. Only Lockout has been checked in motion.

### 12.3 How the game uses them (`blam_geometry::lift`)

Each step a body touching a volume (within its radius) gets, from each volume it touches:

- along the direction: the acceleration, until its speed along the direction reaches the top
  speed (a top speed of 0 is taken as no limit);
- towards the axis and the centre: the acceleration, steering its velocity across the direction
  at the line (or point), at most the top speed and slowing to arrive in about 0.2 s. Halo leaves
  the overshoot to Havok's damping; the settling time is our own choice;
- no gravity while any volume it touches negates gravity.

Walking (`mcc_foot`) and the dirt bike (`mcc_bike`, unless the volume ignores vehicles) take
the push through `mcc_core::lifts::Lifts`. The skateboard takes it all at once (2026-11-10, #29):
once in the air the Skate 3 engine follows the trajectory it predicted on take-off, so a push
spread over the steps is lost after the first few. While the board touches a volume,
`Lifts::kick` brings its speed along each volume's direction up to that volume's top speed in one
go (a volume with no top speed gives 0.1 s of its acceleration; the axis and centre pulls are left
out), added to the board, its hook and the skater's body through the vendored skate-host's
`Session::push`, at most once every 0.3 s (`mcc_skate::lift_kick`). Valhalla's cannon reaches its
11 wu/s in 0.18 s anyway, so the kick is close to the game's push. Swimming has no lifts.

### 12.4 Halo 3

Halo 3 keeps its lifts the same way (2026-11-10, #29; `blam_geometry::objects::lifts`, test
`blam_geometry/tests/lifts.rs`): a placed scenery, crate, machine or control whose model (`hlmt`
**+0x30**) names a `phmo` with phantom types. Offsets found on the bytes of Valhalla's
`man_cannon_river` (a crate) and checked across every multiplayer map's lift objects:

| `phmo` offset | element | meaning |
|---|---|---|
| +0x34 | 0x68 | **phantom types**, the same layout and flags as Halo 2's (marker +0x08, dead radius +0x1C, centre / axis / direction pairs +0x20 / +0x28 / +0x30) |
| +0x70 | 0xA0 | spheres: radius +0x40, translation +0x90 |
| +0x88 | 0x70 | pills: radius +0x40, ends +0x50 and +0x60 |
| +0x94 | 0xE0 | boxes: half sizes +0x50, rotation rows +0xA0 / +0xB0 / +0xC0, translation +0xD0 |
| +0xAC | 0xA0 | polyhedra: bounding box half sizes +0x50 about the centre +0x60 |

Every shape has its name at +0x00 and its phantom type as an i8 at **+0x1E** (-1: none). The
marker (render model marker groups, `weapons::markers`) aims the push along its +X.

Valhalla's cannon has three phantom types on three boxes: one empty, one for players (flags
`0x40000040`: bit 6, vehicles left alone; `fire` marker, direction 60 wu/s^2 to 11 wu/s) and one
for vehicles (bit 3 set: players left alone; 20 to 8). The box sits 45 degrees up the ramp. The
Forge `man_cannon_forge` (30 to 9), Narrows' cannon, Construct's lifts, Isolation's launch tubes
and The Pit's `cyber_pad` (polyhedra), Guardian's cannons (boxes) and the equipment gravity lift
(a pill, 16 to 4 with axis pull 0.8) read the same way. Equipment and vehicles (bubble shield,
Phantom dropship) are not placed objects and carry no lift here. Walking and the bike take Halo
3's lifts too, as Halo 2's.

**Launch pads.** Valhalla's player phantom (also flag bit 30, not decoded) sits on a box 1.15 x
0.71 x 0.28 wu, 0.67 wu long along the push; 60 wu/s^2 needs 0.18 s and 1 wu to reach 11 wu/s.
Pushed step by step while its middle is in the box, a body leaves at 7 to 10 wu/s and flies a
third to four fifths as far as in Halo, where the phantom acts on the whole body for as long as
any of it overlaps. So a volume shorter along its push than `top^2 / (2 a)`, with no axis or
centre pull and gravity left on, is a launch pad (`LiftVolume::launch_speed`): it gives its top
speed at once (`mcc_core::lifts`). Only Valhalla's four are (the short side cannons: 30 to 9);
Avalanche's, Narrows' and Cold Storage's cannons are 3 to 11 wu long with an axis pull and carry
the body as lifts do. Halo's flight from a pad has no air drag and falls at 4.1712 wu/s^2: 28.6
wu over level ground from a base cannon, 19.1 from a side one.


## 13. Sound (`snd!`, `lsnd`, `ugh!`, `sounds_*.dat`)

Checked on Lockout with `scripts/probe_h2_sound.py` and `blam ambience lockout.map --wav DIR`
(`blam_h2::sound`). Field offsets were located with Assembly's `Halo2MCC` plugins (`snd!`,
`ugh!`, `lsnd`, `sbsp`, `scnr`, `ssce`; GPL-3.0, read for offsets only) and confirmed against
the bytes: every element size below divides its block exactly.

### 13.1 Where a map's ambience is named

- **Structure BSP background sound palette** at sbsp +0xD4 (0x64 each): name char[32] +0x00,
  background `lsnd` +0x20, inside-cluster `lsnd` +0x28, cutoff distance +0x44, scale flags
  +0x48. A **cluster** picks its entry with i16 +0x72, and its sound environment (the reverb)
  with i16 +0x74 into the palette at sbsp +0xDC (0x48 each: `snde` reference at +0x20).
- **Sound scenery**: scenario +0xD8 (0x50 each: palette index i16 +0x00, position +0x08), its
  palette at +0xE0 (0x28: `ssce` reference at +0x00). The `ssce` object carries its looping
  sound as an attachment (object +0x94, 0x18 each: tag reference +0x00).

Lockout: palette `lockout` (13 clusters), `lockout_hallway` (3), `lockout_lower_level` (0),
**all three naming `sound\ambience\lockout\lockout\lockout`**, no inside sounds; reverbs
`forest`, `cpaul_small_enclosed`, `cpaul_alley`. Four sound scenery: `particulate_wind`
(blowing snow), `lockout_offinthecanyon` (distant hawk echoes), `broken_flashy` and
`green_hum` (small machine sounds heard within 5 wu, by the lower level).

### 13.2 Looping sound (`lsnd`, 0x34)

| offset | meaning |
|---|---|
| +0x00 | remastered looping sound (reference; null on Lockout's) |
| +0x08 | flags |
| +0x14 | distance bounds: full volume within, silent beyond (world units) |
| +0x24 | tracks (0x58): name sid +0x00, flags +0x04, **gain dB +0x08**, fade in / out +0x0C / +0x10; `snd!` references in +0x14, **loop +0x1C**, out +0x24, alternate loop +0x2C, alternate out +0x34; output effect u16 +0x3C |
| +0x2C | details (0x34): name sid +0x00, `snd!` +0x04, **period** min, max (s) +0x0C, **gain dB +0x14** (read from the values: -6 on `particulate_wind`), flags +0x18, yaw bounds +0x1C, pitch bounds +0x24 (radians), distance bounds +0x2C |

Lockout's background has two tracks, `front_speakers` and `rear_speakers` (each -3 dB, each its
own stereo loop; output effects 1 and 2, the front and rear speaker pairs), and one detail,
`lockout_monogust`, every 40 to 50 s.

### 13.3 Sound (`snd!`, 0x1C) and the gestalt (`ugh!`, 0x78)

An MCC `snd!` is a handle into the **sound gestalt** of the file that holds it: the map's own
`ugh!` for local sounds, `shared.map`'s for shared ones.

| `snd!` offset | meaning |
|---|---|
| +0x00 | u16 flags: 0x80 pitch range is language (per-language samples), 0x400 legacy only, **0x800 remastered only** |
| +0x02 | u8 sound class (0x21 ambient nature) |
| +0x03 | i8 codec index |
| +0x06 | i16 playback parameters index |
| +0x08 / +0x0A | i16 first pitch range, i8 count |
| +0x0B | i8 scale index |
| +0x10 | i32 maximum play time (ms) |

| `ugh!` block | element | meaning |
|---|---|---|
| +0x00 | 3 | codecs: sample rate u8 (0 22,050, 1 44,100, 2 32,000, 3 48,000), encoding u8 (0 mono, 1 stereo, 2 codec, 3 quad), compression u8 (0 PCM big-endian, 1 **Xbox ADPCM**, 2 IMA ADPCM, 3 PCM little-endian, 4 WMA, 5 Opus) |
| +0x08 | 0x38 | playback parameters: distance min, max +0x00 / +0x04, skip fraction +0x08, **gain base dB +0x10** |
| +0x10 | 0x14 | scales (not read) |
| +0x18 | 4 | import names (string ids: permutation names) |
| +0x28 | 0x0C | pitch ranges: first permutation i16 +0x08, count i16 +0x0A |
| +0x30 | 0x10 | permutations: name index i16 +0x00, encoded skip fraction i16 +0x02 (/32767; 1 = only ever played first), gain i8 +0x04, chunk info +0x08 (a block of 8-byte entries, **one per language**: PCM bytes u32 +0x00, first chunk i16 +0x04, chunk count i16 +0x06) |
| +0x60 | 8 | **permutation chunks: one list per language** (nine on these maps, the last empty), each a block of 0x10-byte chunk entries |
| +0x70 | 0x14C | extra info (lip-sync and the like; not read) |

A chunk entry (0x10): **file offset u32 +0x00; u32 +0x04 whose low 24 bits are the stored size
and whose top byte is flags (0x20 on a permutation's last chunk); PCM bytes once decoded u32
+0x08**; runtime index -1 at +0x0C. A language-neutral sound lists the same chunks in every
language; the first (English) list is read.

### 13.4 The sample files

The samples are not in the `.map`. Beside it are `sounds_neutral.dat` (926 MB),
`sounds_remastered.dat` (1.5 GB), `sounds_en/de/fr/it.dat` (dialogue) and `sounds_sp.dat`. They
have no header and no wrapper: a chunk is the raw codec stream at its offset, chunks aligned to
0x80. **Which file** follows from the `snd!` flags: 0x800 (remastered only; exactly the tags
under `sound_remastered\`) is `sounds_remastered.dat`; every other language-neutral sound is in
`sounds_neutral.dat`. The chunk flags do not say: `green_hum`'s classic chunk has the same 0xA0
as remastered ones and sits in `sounds_neutral.dat`. With that rule all 54 Xbox ADPCM chunks of
Lockout's gestalt and all 1,381 of `shared.map`'s are whole blocks with valid headers, and decode
to the size the gestalt gives. Per-language sounds (flag 0x80) are not read.

**Xbox ADPCM**, the classic codec of every Lockout ambience sound: IMA ADPCM in blocks of 36
bytes per channel, 64 samples per channel per block. A block starts with one 4-byte header per
channel (i16 sample, u8 step index 0..88, u8 zero), then the channels' data in turns of 4 bytes
(8 samples, low nibble first), 8 turns. The sizes confirm 64 samples per block: the front loop's
`lockout1` stores 262,080 + 383,040 bytes = 8,960 stereo blocks, and the gestalt says 2,293,760
PCM bytes = 8,960 x 64 x 2 channels x 2 bytes. We output the header sample, then 63 nibbles (the
last is dropped). Whether the game outputs the header sample or the 64th nibble instead is not
known; it is one sample in 64. The remastered set is mostly Opus (codec 5) and is not decoded.

| Lockout sound | codec | permutations | length |
|---|---|---|---|
| `lockout\front_speakers\loop` | 44.1 kHz stereo | 6 (`lockout1` first only) | 13.0 + 8.5 + 7.6 + 6.6 + 12.7 + 3.2 = 51.6 s, RMS about 0.012 each |
| `lockout\rear_speakers\loop` | 44.1 kHz stereo | 6, same lengths | 51.6 s |
| `lockout\details\lockout_monogust` | 44.1 kHz mono, heard 20..40 wu | 7 | 2.3 to 19.9 s |
| `particulate_wind\details\particulate_wind` | 44.1 kHz stereo, -15 dB | 6 | 9 to 23 s |
| `lockout_offinthecanyon\details\...` | 44.1 kHz mono, skip 0.95 | 3 | 4.5 to 5.9 s |

The viewer plays them as `docs/viewer.md` "Halo 2 maps" says.
