# MCC Halo: Combat Evolved (classic) maps

What `crates/blam_h1` reads, written 2026-10-31 for the 19 multiplayer maps in MCC's
`halo1\maps` (Blood Gulch is `bloodgulch.map`, and so on; build `01.03.43.0000`). Every fact
was checked against the user's files with the read-only probes in `docs/format/scripts/`
(`h1_common.py`, `probe_h1_bitmaps.py`, `probe_h1_sbsp.py`, `probe_h1_collision.py`,
`probe_h1_scenario.py`, `probe_h1_models.py`, `probe_h1_shaders.py`; run them with
`python -I`, an optional map name as the argument).

Where to look came from general knowledge of the classic Halo PC layout (the HEK tag
definitions as the community documents them); offsets, sizes and meanings were then confirmed
on the bytes. No code from Reclaimer, HaloExport, Invader or any other project was copied.
HaloExportH1's exports (`<HaloExport output>\h1-*`) gave the counts to check against.

All numbers little-endian, offsets hex. "Block" = 12 bytes: element count (u32), an address
(u32, section 2), one unused u32. "Tag reference" = 16 bytes: class fourcc, name address, name
length, then the tag's datum (salt high 16 bits, index low 16). Units are world units (1 wu =
10 ft = 3.048 m, as in Halo 2 and 3), +Z up. Angles are radians.

## 1. Header

The first `0x800` bytes. The file is not compressed.

| offset | type | meaning |
|---|---|---|
| 0x000 | u32 | `head` (0x68656164) |
| 0x004 | u32 | version, 13 (Halo 2 and Halo 3 MCC maps also say 13) |
| 0x008 | u32 | file length |
| 0x010 | u32 | tag data file offset |
| 0x014 | u32 | tag data size |
| 0x020 | char[32] | map name (`bloodgulch`) |
| 0x040 | char[32] | build (`01.03.43.0000`) |
| 0x060 | u32 | map type (1: multiplayer) |
| 0x7FC | u32 | `foot` |

**Telling Halo CE from the other games:** `head`, 13, `foot` at 0x7FC and `tags` at tag data
+0x24 (`blam_h1::is_halo1`). Halo 2's header is 0x380 bytes and keeps its scenario path at
0xD0; Halo 3's is longer. File names do not decide: Halo CE's `sidewinder` and `prisoner` share
their names with Halo 3 maps (`blam_h1/tests/maps.rs` checks both ways).

## 2. Tag data header and tag index

Tag data is loaded at address **0x50000000**: an address inside it, minus 0x50000000, plus the
tag data offset, is a file offset.

At the tag data offset:

| offset | meaning |
|---|---|
| +0x00 | address of the tag array (0x50000028: right after this header) |
| +0x04 | the scenario's datum |
| +0x0C | tag count (bloodgulch 2425, as HaloExportH1 reports) |
| +0x14 | model vertex area: file offset |
| +0x1C | model vertex area: size (the model index area follows it) |
| +0x20 | model vertex and index areas together: size |
| +0x24 | `tags` |

The tag array has one 0x20-byte entry per tag: three class fourccs (+0x0, +0x4, +0x8; the
first is the tag's own), the datum (+0xC), the name's address (+0x10), the data's address
(+0x14), two u32 not needed here. Class fourccs are stored byte-reversed (`rncs` for `scnr`).
The structure BSP's entry has no data of its own here: its data is in its BSP chunk (5).

## 3. bitmaps.map

`bitmaps.map` beside the maps (185 MB, shared by all of them) holds most bitmaps' pixels. Its
header starts with 1 (bitmaps), but nothing in it is needed: each bitmap image says its own
offset into the file (8). MCC ships no `sounds.map`: sound is in FMOD banks (19).

## 4. Scenario (`scnr`)

| offset | what | element |
|---|---|---|
| 0x030 | skies | 0x10, a tag reference to a `sky ` |
| 0x210 | scenery placements | 0x48 |
| 0x21C | scenery palette | 0x30, the object's tag reference at +0 |
| 0x240 / 0x24C | vehicles / palette | 0x78 / 0x30 (the game type's; not placed) |
| 0x294 / 0x2A0 | machines / palette | 0x40 / 0x30 |
| 0x2AC / 0x2B8 | controls / palette | 0x40 / 0x30 |
| 0x2C4 / 0x2D0 | light fixtures / palette | 0x58 / 0x30 |
| 0x2DC / 0x2E8 | sound scenery / palette | 0x28 / 0x30 |
| 0x354 | player starting locations | 0x34 |
| 0x360 | trigger volumes | 0x60 |
| 0x378 | netgame flags | 0x94 |
| 0x384 | netgame equipment | 0x90 |
| 0x390 | starting equipment | 0xCC |
| 0x5A4 | structure BSPs | 0x20 (section 5) |

Every object placement starts the same way: palette index (i16 +0), name index (i16 +2),
position (vec3 +0x8), yaw, pitch, roll (+0x14). Blood Gulch: 47 scenery from a palette of 19
(HaloExportH1: 47), 28 vehicles, 2 sound scenery.

**Objects.** An object tag (`scen`, `mach`, `ctrl`, `lifi`, `vehi`, ...) names its model at
+0x28 (a `mod2` reference).

**Player starting locations** (0x34): position (+0), facing (+0xC), team (i16 +0x10), BSP
(i16 +0x12), four game types (u16 +0x14). Multiplayer maps use them as the spawn points: Blood
Gulch has 72, all on the ground or a base floor.

**Netgame equipment** (0x90): four game types (u16 +0x4), position (+0x40), facing (+0x4C),
item collection (+0x50; Blood Gulch's first: shotgun, shotgun, assault rifle, plasma rifle).
**Netgame flags** (0x94): position (+0), facing (+0xC), type (u16 +0x10; Blood Gulch uses 0,
2, 3, 4, 6, 7, 8, meanings not checked), team (u16 +0x12). Neither is used yet.

**Trigger volumes** (0x60): name (char[32] at +4), then forward (+0x30), up (+0x3C), position
(+0x48, a corner), extents (+0x54) along forward, left (up x forward) and up, as in Halo 3.
Checked on The Maw (`d40`, 158 volumes): `grand_finale` lies across the Longsword's hangar
floor, `enc7_7` on the road into it, the `bsp a,b` volumes at the seams between BSPs.

**Object names** (`+0x204`, 0x24 each): name (char[32]), object type (u16 +0x20: 0 biped, 1
vehicle, ..., 7 machine), placement index (i16 +0x22). A placement names its entry with the
i16 at +2 (`d40`: `trench_jeep1` is vehicle placement 5, a Warthog at the top of the lift).

**BSP switch volumes** (`+0x39C`, 8 bytes each): trigger volume (i16), from BSP (i16), to BSP
(i16), an unused -1. The engine switches structure BSP when the player enters one; only one BSP
is loaded at a time. `d40` has 13, each over a volume named after it (`bsp 5,6` from 5 to 6).
`blam_h1` makes each decoded BSP a zone set of its own and these its switches.

## 5. Structure BSPs and their chunks

Each entry of the scenario's structure BSP block (0x20) is: chunk file offset (+0), chunk size
(+4), the address the chunk is loaded at (+8), then the sbsp tag reference (+0x10). Pointers
inside a chunk are relative to its own address. Every multiplayer map has one BSP.

The chunk starts with a header (0x18): the sbsp tag data's address (+0, the header's own
address + 0x18), the BSP **vertex buffer's size** (+4) and its **file offset** (+8; 0x800 on
every map, straight after the file header), then `sbsp` at +0x14.

The sbsp tag:

| offset | what |
|---|---|
| 0x000 | lightmap bitmap (tag reference) |
| 0x0A4 | collision materials, 0x14 each: the shader reference at +0 |
| 0x0B0 | collision BSP, 0x60 (section 7) |
| 0x0C8 | world bounds: x min, max, y min, max, z min, max |
| 0x0F8 | surfaces: 6 bytes each, three u16 vertex indices |
| 0x104 | lightmaps, 0x20 each |
| 0x134 | clusters, 0x68 each |

**Lightmaps** (0x20): the page of the lightmap bitmap (i16 +0; -1: none, the last lightmap of
each map gathers the unlit transparent surfaces), then its materials (block +0x14, 0x100
each). Each lightmap is one mesh in the scene: Blood Gulch 16, Death Island 22, Sidewinder 27,
Timberland 46, the same as HaloExportH1's lightmap meshes.

**Lightmap material** (0x100):

| offset | what |
|---|---|
| +0x00 | shader (tag reference) |
| +0x14 | first surface (i32), +0x18 surface count (i32) |
| +0xB4 | rendered vertex count, +0xB8 their offset in the vertex buffer |
| +0xC8 | lightmap vertex count (0 or the rendered count), +0xCC their offset |

A rendered vertex is 56 bytes: position, normal, binormal, tangent (vec3 each), texcoord. A
lightmap vertex is 20 bytes: normal, texcoord on the lightmap page (0..1). The surfaces'
indices count from the material's first rendered vertex.

**Winding:** surfaces are stored **clockwise**: (a, c, b) agrees with the vertex normals on
every triangle of every map checked (bloodgulch 0.9998, the rest 1.0000). The surface counts
match HaloExportH1's: bloodgulch 5503, deathisland 19581, sidewinder 10228, timberland 6995.

**Clusters** (0x68): the sky they see (i16 +0, -1: none). The size is confirmed by the blocks
inside each cluster (+0x28, +0x34, +0x44, +0x50, +0x5C portals) all pointing into the chunk.
Chillout, Putput and Rat Race have no cluster that sees a sky (indoor maps); the sky's fog is
outdoor fog, so they get none.

## 6. Lightmap pages

The lightmap bitmap's images are its pages: R5G6B5, no mips (Blood Gulch: 256 x 128, 65536
bytes each), stored in the map itself (no `bitmaps.map` flag, section 8). They are light
colours in gamma space, like Halo 2's. The viewer packs a BSP's pages into one atlas
(`blam_h2::lightmap::Atlas`) and writes them as Halo 3's constant SH term, as for Halo 2.

## 7. Collision BSP

Element 0x60: eight blocks, 12 bytes apart: 3D nodes (12 bytes each), planes (16), leaves (8),
2D references (8), 2D nodes (20), surfaces (12, at +0x3C), edges (24, +0x48), vertices (16,
+0x54).

- Surface (12): plane (i32 +0, top bit set: the plane's back side), first edge (i32 +4), flags
  (u8 +8: 1 two-sided, 2 invisible, 4 climbable, 8 breakable), breakable surface (i8 +9),
  material (i16 +0xA, into the collision materials).
- Edge (24): start vertex, end vertex, forward edge, reverse edge, left surface, right surface
  (i32 each).
- Vertex (16): point (vec3), first edge.

A surface's loop is walked as in Halo 2 and 3: from its first edge, take the start vertex and
the forward edge when the surface is on the left, the end vertex and the reverse edge when on the
right, until back at the first edge. Fanned from the first point, the triangles face the same way
as the surface's plane (flipped by the top bit) on every surface: 1.0000 on Blood Gulch and
Timberland, no broken loops. Player clips are flags 3 (two-sided, invisible). Blood Gulch: 5098
surfaces, 5940 triangles; Timberland 5395 and 7160.

## 8. Bitmaps (`bitm`)

The tag's images: block at +0x60, 0x30 each.

| offset | what |
|---|---|
| +0x00 | `bitm` |
| +0x04, +0x06, +0x08 | width, height, depth (u16) |
| +0x0A | type (u16): 0 2D, 1 3D, 2 cube map, 3 white |
| +0x0C | format (u16), the same numbers as Halo 2: 0 A8, 1 Y8, 2 AY8, 3 A8Y8, 6 R5G6B5, 8 A1R5G5B5, 9 A4R4G4B4, 10 X8R8G8B8, 11 A8R8G8B8, 14 DXT1, 15 DXT3, 16 DXT5, 17 P8 bump |
| +0x0E | flags (u16); **0x100: the pixels are in `bitmaps.map`**, otherwise in the map |
| +0x14 | mipmap count: levels after the first (u16) |
| +0x18 | pixel offset, in `bitmaps.map` or the map |
| +0x1C | pixel size |

Levels follow each other from the largest, unswizzled, BCn blocks at least one block per level:
a 256 x 256 DXT1 with 8 mipmaps is 43704 bytes, a 512 x 512 DXT3 with 9 is 349552, exactly
the stored sizes. Every external image of Blood Gulch lies inside `bitmaps.map`, and every 2D
map the shaders of the 19 multiplayer maps use decodes.

## 9. Models (`mod2`, gbxmodel)

| offset | what |
|---|---|
| +0x30, +0x34 | base map u and v scale: every texcoord is multiplied by them |
| +0xB8 | nodes, 0x9C each |
| +0xC4 | regions, 0x4C each: name (char[32] +0), permutations (block +0x40, 0x58 each) |
| +0xD0 | geometries, 0x30 each: parts (block +0x24, 0x84 each) |
| +0xDC | shaders, 0x20 each: the shader reference at +0 |

A permutation (0x58) lists its geometry per level of detail as five i16 at +0x40 (super low
first, super high at +0x48; -1: none). A part (0x84): shader index (i16 +4), index count minus
two (u32 +0x48), index offset (+0x4C) in the model index area, vertex count (+0x58), vertex
offset (+0x64) in the model vertex area (section 2).

A model vertex is 68 bytes: position, normal, binormal, tangent (vec3 each), texcoord, two node
indices (i16) and two weights (f32). **Positions are in model space** at the default pose: a
Warthog's wheel vertices sit at the wheels, so no node transform is applied. The indices are a
**triangle strip** (u16, 0xFFFF padding), wound **clockwise** like the BSP: of 87276 strip
triangles on Blood Gulch only 106 face along the normals as (a, b, c) on even steps, so even
steps are (a, c, b) and odd ones (a, b, c). Degenerate triangles join the strip's runs.

## 10. Shaders

Each kind of shader is its own class with its maps at fixed places. Every shader starts with a
0x28-byte common part.

- **`senv` (shader_environment; the BSP's ground and walls).** Flags (u16 +0x28, bit 0: alpha
  tested), type (u16 +0x2A: 0 normal, 1 blended, 2 blended with base specular). Base map
  reference +0x88 (drawn with the BSP's own texcoords), primary detail scale +0xB4 and map +0xB8,
  secondary detail scale +0xC8 and map +0xCC, micro detail scale +0xF8 and map +0xFC, bump scale
  +0x124 and map +0x128, reflection cube map +0x324. A scale of 0 means 1. Blended ground: the
  base map's alpha picks between the two detail maps, white picking the secondary (Blood
  Gulch's ground: primary `detail sand`, secondary `detail grass`, mean base alpha 0.82, and the
  ground is mostly grass with sand paths, as drawn).
- **`soso` (shader_model; objects).** Flags (u16 +0x28: bit 1 two-sided, bit 2 not alpha
  tested, bit 3 alpha-blended decal), map u, v scale +0x9C, +0xA0, base map +0xA4, multipurpose
  map +0xBC, detail scale +0xD8, detail map +0xDC, detail v scale +0xEC, reflection cube map
  +0x164.
- **`sotr` (transparent generic), `schi` (transparent chicago), `scex` (chicago extended).**
  First map type (u16 +0x2A; not 0: the first map is a cube map), framebuffer blend function
  (u16 +0x2C: 0 alpha blend, 1 multiply, 2 double multiply, 3 add, 4 subtract, 5 component min,
  6 component max, 7 alpha-multiply add), a stack of maps (block +0x54). `sotr` maps are 0x64
  bytes (u, v scale +0x4, +0x8; u, v offset +0xC, +0x10; bitmap +0x1C); `schi` and `scex` maps
  are 0xDC (u, v scale +0x54, +0x58; offset +0x5C, +0x60; bitmap +0x6C).
- **`swat` (water).** Base map +0x4C, reflection map +0x9C, ripple bump map +0xC8.
- **`sgla` (glass).** Background tint map +0x64, reflection map +0xAC, bump map +0xC0, diffuse
  scale +0x154 (1.0 on every glass shader) and diffuse map +0x158, diffuse detail map +0x16C,
  specular map +0x1A0.
- **`smet` (meter)** map +0x4C; **`spla` (plasma)** primary noise map +0xD4.

## 11. Sky (`sky `)

| offset | what |
|---|---|
| +0x00 | model (a `mod2` reference; none for indoor maps' skies) |
| +0x38 | indoor ambient colour, +0x44 its power |
| +0x48 | outdoor ambient colour, +0x54 its power |
| +0x58 | outdoor fog colour (Blood Gulch 1.0, 0.90, 0.77) |
| +0x6C | outdoor fog maximum density (0.4) |
| +0x70, +0x74 | outdoor fog start and opaque distance (3, 100 wu) |
| +0x78 | indoor fog colour, +0x8C maximum density, +0x90, +0x94 start and opaque distance |

The sky model's shaders are `schi` / `sotr` stacks: the dome's gradient is the last map (under
star layers), a cloud layer is a colour map with its alpha in a separate A8 `cloud mask` map,
planets and rings are single alpha-blended maps.

## 12. Scripts

Checked on `d40` with `probe_h1_scripts.py` (it decompiles a map's scripts). Halo 3's layout
(`halo-script.md`) with smaller pieces; `blam_h1::script` reads it into Halo 3's
`blam_script::ScriptData`, so `blam_script`'s decompiler prints it and `mcc_script` runs it.

| scenario offset | what |
|---|---|
| 0x474 | syntax data (a 20-byte tag data field: size, flags, file offset, address, unused) |
| 0x488 | string data (the same kind of field) |
| 0x49C | scripts, 0x5C each: name (char[32]), type (u16 +0x20: 0 startup, 1 dormant, 2 continuous, 3 static, 4 stub), return type (u16 +0x22), root node datum (+0x24) |
| 0x4A8 | globals, 0x5C each: name, type (u16 +0x20), initial value datum (+0x28) |
| 0x4B4 | tag references scripts use, 0x28 each, the reference at +0x18 |
| 0x4E4 | cutscene flags, 0x5C each: name (+4), position (+0x24), yaw and pitch (+0x30) |

The syntax data is a data array: a 0x38-byte header (name `script node`, maximum count u16
+0x20 = 32767, element size u16 +0x22 = 20, `@t@d` at +0x28), then the nodes. A node (20
bytes): salt (u16), opcode (u16; the script's index for a script call), value type (u16),
flags (u16; the same bits as Halo 3: 1 primitive, 2 script call, 4 global), next node datum
(u32), string offset (u32), value (u32). Halo 3's node adds a source line. As in Halo 3, a
call's first child is a function-name node whose string is the function's name, so no
opcode table is needed. `d40`: 298 scripts, 66 globals.

Halo CE numbers its value types differently from Halo 3 (`blam_h1::script::value_type` maps
them): 0 to 10 the same (void 4, boolean 5, real 6, short 7, long 8, string 9, script 10),
then 11 trigger volume, 12 cutscene flag, 13 camera point, 14 cutscene title, 15 recording,
16 device group, 17 ai (value `0x8000_0000 | squad << 16 | encounter`), 18 ai command list, 21
navpoint, 22 hud message, 23 object list, 24 sound, 25 effect, 26 damage, 27 looping sound, 28
animation graph, 29 actor variant, 31 object definition, 32 game difficulty (easy, normal,
hard, impossible as 0 to 3), 33 team, 36 hud corner, 37 object, 38 unit, 39 vehicle, 40
weapon, 41 device, 42 scenery, 43 to 48 object names. Seen as the constants' strings and
the functions that take them on `d40`.

The countdown the Warthog run races (`timer_begin`, `endgame_cinematics`): `hud_set_timer_time
minutes seconds` sets it, `hud_get_timer_ticks` reads it in 30ths of a second,
`pause_hud_timer`, `show_hud_timer`, `hud_set_timer_warning_time` (1:00 on `d40`).

## 13. Effects (`effe`, `part`, `pctl`, `ligh`, `pphy`)

Checked on `d40` with `probe_h1_effects.py` (`python -I probe_h1_effects.py d40 [effect path]`
prints an effect and everything it reaches; `--pctl` every particle system; `--hex PATH [SIZE]`
dumps a tag). Code: `blam_h1::effect` (reader), `blam_h1::effect_library` (into Halo 3's
effect types, 13.6). Tests: `blam_h1/tests/effects.rs`, `mcc_effects/tests/h1_run_effects.rs`.

The Warthog run's scripts name their explosions through effect globals (`(global effect
explosion_large "effects\explosions\large explosion")`, `effect_new explosion_large
enc6_2_blast1`): `explosion_small` (`effects\small explosion`), `explosion_grenade` (`weapons\frag
grenade\effects\explosion`), `explosion_medium` / `_no` (`effects\explosions\medium explosion` /
`... no objects`), `explosion_large` / `_no`, `explosion_steam` / `_no`; the `no objects` ones
leave out the debris (`garb`) parts. The scenario's script references (12, `+0x4B4`) list every
effect the scripts can name: 11 on `d40`. The second argument is a cutscene flag (12).

### 13.1 `effe` (0x40)

| offset | what |
|---|---|
| 0x00 | flags (u32; 4 on every explosion) |
| 0x04, 0x06 | loop start, loop stop event (i16, -1: no loop) |
| 0x28 | locations, 0x20 each: a marker name (`''` the origin, `normal`, `gravity`) |
| 0x34 | events, 0x44 each, played one after another |

**Event** (0x44): skip fraction (f32 +0x04), delay bounds (+0x08), duration bounds (+0x10; 0 on
every explosion: one burst), parts (block +0x2C, 0x68 each), particles (block +0x38, 0xE8 each).

**Part** (0x68): create in environment, violence mode (u16 +0, +2), location (i16 +4), flags
(u16 +6), the tag (reference +0x18: `snd!`, `ligh`, `pctl`, `jpt!`, `deca`, `garb`, `effe`),
velocity bounds (+0x40, wu/s), velocity cone (+0x48, rad), angular velocity bounds (+0x4C),
radius modifier bounds (+0x54), A and B scale flags (u32 +0x60, +0x64).

**Particle** (0xE8): create in (u16 +0, +2), camera mode (u16 +4), location (i16 +8),
direction yaw, pitch (+0x0C), offset (vec3 +0x14), the direction as a vector (+0x20, worked out;
`(1,0,0)` on every run effect), the `part` (reference +0x54), flags (u32 +0x64), distribution
function (u16 +0x68: when in the event particles are made), count bounds (i16 +0x6C, +0x6E),
distribution radius bounds (+0x70), velocity bounds (+0x84, wu/s), velocity cone (+0x8C, rad; pi
every way), angular velocity bounds (+0x90, rad/s), radius bounds (+0xA0, wu), tint lower and
upper (argb reals, +0xB0, +0xC0).

The large explosion: a `jpt!` damage, its sound, a shock wave `jpt!`, the `pctl` fireball, a
scorch decal (at location `gravity`), three `garb` debris, the light; particles: drifting smoke
(15 to 20), panel impacts and panel debris.

### 13.2 `part` (0x164)

| offset | what |
|---|---|
| 0x00 | flags (u32) |
| 0x04 | bitmap (reference) |
| 0x14 | point physics (`pphy` reference) |
| 0x24 | a `foot` reference |
| 0x38 | lifespan bounds, seconds |
| 0x40, 0x44 | fade in time, fade out time, seconds (drifting smoke: 0.3 and 3 of a 2 to 4 s life) |
| 0x48, 0x58 | collision effect, death effect (references; `effe` or `snd!`) |
| 0x68 | minimum size |
| 0x74 | radius animation bounds: the radius is multiplied from the first to the second over the life |
| 0x80 | animation rate bounds, frames a second (0: one frame) |
| 0x88 | contact deterioration |
| 0x8C, 0x90 | fade start size, fade end size (pixels) |
| 0x98 .. 0x9E | first sequence, initial, looping and final sequence counts (i16 each) |
| 0xAC | orientation (u16): 0 screen facing, 1 parallel to direction, 2 perpendicular to it |
| 0xD4 | the particle shader (13.4) |

Values at 0x6C..0x73 are big-endian leftovers in padding. 0xA8 is a worked-out float.

### 13.3 `pctl` (0x68): particles that pass through states

The explosions' fireballs: each particle is fire for a moment, turns to a smoke plume, then
fades into `death`.

| offset | what |
|---|---|
| 0x38 | point physics (reference) |
| 0x50 | physics constants, 4 bytes each |
| 0x5C | particle types, 0x80 each |

**Type** (0x80): name (char[32]; `fire to smoke`), flags (u32 +0x20), initial particle count (i16
+0x24; 30 on the large explosion), complex sprite render mode (u16 +0x28), radius (+0x2C; 1.5 wu,
where particles are made), particle creation physics (u16 +0x54; 1 explosion on all fifteen of
`d40`'s), physics flags (u32 +0x58), physics constants (block +0x5C, 4 bytes; for explosion
creation the first two read as speed bounds in wu a tick: 0.07 and 0.125 on the large explosion,
0.015 and 0.04 on the small; our reading), states (block +0x68, 0xC0 each), particle states
(block +0x74, 0x178 each).

**State** of the system (0xC0): name (`birth`, `death`), duration bounds (+0x20), transition
bounds (+0x28), nine multipliers from +0x34 (1 in `birth`; `death` has zeros among them; their
order not settled), creation physics (u16 +0xB0), physics constants (block +0xB4). Not used.

**Particle state** (0x178):

| offset | what |
|---|---|
| 0x00 | name (`fire`, `smoke plume`, `death`) |
| 0x20 | duration bounds, seconds in this state |
| 0x28 | transition bounds, seconds to become the next state |
| 0x30 | bitmap (reference) |
| 0x40 | sequence index (i16) |
| 0x48 | scale bounds: **world units per pixel** of the sprite (fire cloud sprites are about 36 pixels: 0.04 to 0.07 makes the large explosion's fire 1.4 to 2.5 wu across) |
| 0x50 | animation rate bounds (frames a second) |
| 0x58 | rotation rate bounds (rad/s) |
| 0x60, 0x70 | colour 1, colour 2 (argb reals; picked between) |
| 0x80 | radius multiplier |
| 0x84 | point physics (reference) |
| 0xDC | the particle shader (13.4) |
| 0x168 | creation physics (u16), then physics constants (block +0x16C) |

The large explosion: `fire` 0.1 to 0.3 s, turning over 0.8 to 1 s into `smoke plume` (1 to 2 s,
then 0.8 to 1.5 s into `death`, alpha 0). Fire is added (colour `1.0 0.9 0.55` to `0.93 0.57
0.35`, alpha 0), smoke alpha blended (light grey, alpha 0.2 to 0.3).

### 13.4 The particle shader (0x2C, in `part` at 0xD4 and particle states at 0xDC)

| offset | what |
|---|---|
| +0x00 | 1 on every one seen (not used) |
| +0x04 | shader flags (u16: sort bias, nonlinear tint, don't overdraw first-person weapon) |
| +0x06 | framebuffer blend function (u16): 0 alpha blend, 1 multiply, 2 double multiply, 3 add, 4 subtract, 5 component min, 6 component max, 7 alpha-multiply add |
| +0x08 | framebuffer fade mode (u16): 0 none, 1 fade when perpendicular, 2 when parallel |
| +0x0A | map flags (u16) |
| +0x28 | secondary map (reference; a `zmap` on steam) |

The shader's map animation functions follow (u16 function, period, phase, scale ...); all zero on
the run's particles.

### 13.5 `ligh`, `pphy`, bitmap sequences

**`ligh`**: flags (u32 +0), radius (+0x04, 10 wu on the explosions), radius modifier bounds
(+0x08), falloff and cutoff angles (+0x10, +0x14), colour interpolation flags (u32 +0x34), colour
lower and upper (argb reals +0x38, +0x48), gel map (reference +0x64), lens flare (reference
+0xAC), **duration in ticks** (+0xF4: 30 on the explosions, 4.5 on the grenade's quick flash,
180 on the reactor's), falloff function (u16 +0xFA: 0 linear, 1 early, 2 very early, 3 late, 4
very late, 5 cosine). The explosions' lights have an orange upper colour (`1.0 0.8 0.18`) and a
black lower one: we play a light from the upper colour to the lower as the falloff runs out
(`effect_library::falloff`, our reading of the function names: early loses its light early).
Lights not made by effects (flashlights, glows) have a duration of 0.

**`pphy`**: flags (u32 +0), density (+0x20, g/ml; air 0.0011, smoke 0.0004 to 0.0015, stones
2.4), air friction, water friction, surface friction, elasticity (+0x24 .. +0x30). Worked out by
the tools, and settled by their numbers on all 25 of `d40`'s: +0x04 the density times 118,625;
+0x08 `(1 - density) / (1 + density)` (buoyancy in water); **+0x0C `(0.0011 - density) / (0.0011
+ density)`**: the share of gravity that lifts it in air (smoke 0.16 to 0.47, stones -0.999).

**Bitmap sequences** (`bitm` +0x54, 0x40 each): name (char[32]), first bitmap and bitmap count
(i16 +0x20, +0x22; sequences of whole images, the lens flares), sprites (block +0x34, 0x20 each:
image index i16 +0, left, right, top, bottom as fractions of that image +0x08). A sprite sheet
(bitmap type 3) has its sprites spread over its images: `fire cloud`'s sequence 0 has 60 sprites
on image 0, sequence 1 four on image 1.

### 13.6 How they are played (`effect_library`)

Halo CE's tags are turned into Halo 3's (`effects.md`), and `mcc_effects` plays them:

- **Events** as they are. **Parts**: `snd!` stays a sound part (`mcc_effects` hands it to
  `mcc_core::script_sound` as an impulse with the Halo CE tag index and path and where the
  effect is: the hook for Halo CE sound), `ligh` becomes a light flash, `effe` a child effect,
  `jpt!` a camera-shake request (not used), `pctl` systems (below); `deca` and `garb` stay parts
  that are not played.
- **Event particles**: one emitter of `count` particles, placed out to the distribution radius,
  leaving in the cone at the velocity, `2 x radius` across, the radius animation over the life,
  tinted between the two tints, alpha from the fade in and out (as shares of the mean life),
  spinning at the angular velocity, the `part`'s bitmap and blend.
- **A `pctl` type** becomes one emitter per particle state but the last: all made at once, `initial
  count` of them in the type's radius, leaving every way at the creation speed. On the timeline at
  the states' mean durations each state's emitter is fully seen for its duration, fades out over
  its transition while the next one fades in (sizes moving towards the next state's), and the
  last seen state fades into `death`. The two emitters' particles are not the same particles (our
  renderer draws one blend mode per emitter): the fire puffs and the smoke puffs are made apart,
  with the same spread.
- **Sizes**: a particle state's scale times the mean sprite width in pixels of its sequence.
- **Physics**: gravity scale is minus the air buoyancy (+0x0C), drag the air friction over the
  worked-out mass (+0x04): the fireball's `fire explosion` physics stops a particle within a
  tenth of a second, drifting smoke rises slowly, stones fly (our reading).
- **Lights**: the `ligh` radius is the reach, intensity `LIGHT_INTENSITY` (our choice by eye)
  times the colour from upper to lower.
- **Bitmaps** become one atlas per tag (every 2D image stacked, mips by averaging), every
  sequence a run of sprites on it, so sprite sheets and image sequences draw alike.
- **Colours** are real colours in gamma space, made linear, times `COLOUR_SCALE` (1/3: undoes
  `mcc_effects`' x3 exposure, so a colour reaches the screen as the tag has it).

Not played: decals (`deca`), debris objects (`garb`), damage (`jpt!`), lens flares on lights,
`part` collision effects, the system states, distribution functions over an event's duration,
looping effects, the particles' secondary (`zmap`) maps, fade modes and map animations.
## 14. AI: encounters and actor variants

Checked on `d40` with `probe_h1_ai.py` (`python -I probe_h1_ai.py d40 enc6_1`); read by
`blam_h1::ai`, placed as `mcc_ai` actors by `crates/mcc-rust-rewrite/src/halo1_ai.rs`. Where to look
came from general knowledge of the HEK scenario layout; every offset below was confirmed on
the bytes (counts, names, positions on the run's road, values that match their meaning).

| scenario offset | what |
|---|---|
| 0x420 | actor variant palette, 0x10 each: an `actv` reference (`d40`: 35) |
| 0x42C | encounters, 0xB0 each (`d40`: 56) |
| 0x438 | command lists, 0x60 each, the name first |

**Encounter** (0xB0): name (char[32]), flags (u32 +0x20; bit 0: not created when its BSP
loads, the scripts place it; every run encounter has it), team (u16 +0x24; 0 = each
actor's unit's own, on every `d40` encounter), manual BSP (u16 +0x2A, used when flag 0x40
is set), the precomputed BSP (u16 +0x7E: 5 for `enc6_0`..`enc6_4`, 6 for `enc6_5`, 7 for
`enc6_8`, `enc6_9`, `enc7_*`), squads (block +0x80, 0xE8 each), platoons (block +0x8C,
0xAC each, the name first), firing positions (block +0x98, 0x18 each).

**Squad** (0xE8): name (char[32]), actor variant (i16 +0x20, palette index; -1: each
starting location names its own), platoon (i16 +0x22), initial and return state (u16
+0x24, +0x26), flags (u32 +0x28), actors on normal (i16 +0x7C) and on legendary (i16 +0x7E),
starting locations (block +0xD0, 0x1C each). Our difficulty rule (not checked against the
game): the normal count on easy and normal, the legendary count on legendary, halfway
(rounded up) on heroic.

**Starting location** (0x1C): position (vec3), facing (f32 +0xC, radians), cluster (i16
+0x10), actor variant (i16 +0x18, -1: the squad's), command list (i16 +0x1A).

**Firing position** (0x18): position (vec3), group letter (u16 +0xC, 0 = A), cluster (i16
+0xE).

**Names.** Scripts name a squad `encounter/squad`, a platoon `encounter/platoon` and a whole
encounter by its name, in lower case (`enc6_1_flood/squada` for the squad stored as
`squadA`; `ai_defend enc6_5_cov/evacs` names a platoon of `evacs1` and `evacs2`). The AI
constants are the scripts' nodes of value type ai (17, section 12) with that text; every one
of `d40`'s resolves (`blam_h1/tests/ai.rs`). In `mcc_ai` each encounter is a squad group and
an AI zone, each platoon a group under it, each squad a squad named `encounter/squad`.

**Actor variant** (`actv`):

| offset | what |
|---|---|
| +0x04 | actor (`actr`) |
| +0x14 | unit (`bipd`, or a vehicle for turret gunners) |
| +0x24 | major variant (`actv`) |
| +0x64 | weapon (`weap`) |
| +0x74 | maximum firing distance (wu) |
| +0x78 | rate of fire (shots a second) |
| +0x7C | projectile error (radians) |
| +0x80, +0x84 | first burst delay (s, min, max) |
| +0x9C, +0xA0 | desired combat range (wu, min, max) |
| +0xBC, +0xC0 | target tracking, target leading (0..1) |
| +0xCC, +0xD0 | burst origin radius (wu) and angle (radians) |
| +0xE0, +0xE4 | burst duration (s) |
| +0xE8, +0xEC | burst separation (s) |
| +0x200, +0x204 | body and shield vitality (0: the unit's own) |

`d40`: Flood combat elite with an assault rifle 120 body, 12 wu, 6 shots/s, combat range 4..8;
elite spec-ops 125 body, 200 shield; grunt spec-ops 75; hunter 225; carrier 50; infection
form 0.01; sentinel 80 (its unit's shield 80).

**Actor** (`actr`): type (u16 +0x14: 0 elite, 1 jackal, 2 grunt, 3 hunter, 4 engineer, 5
assassin, 6 player, 7 marine, 8 crew, 9 combat form, 10 infection form, 11 carrier form, 12
monitor, 13 sentinel, 14 none, 15 mounted weapon), maximum vision distance (+0x18, wu),
central and maximum vision angle (+0x1C, +0x20), peripheral vision angle (+0x28), peripheral
distance (+0x2C), hearing distance (+0x4C).

**Unit and biped** (`bipd`, past the object's 0x17C bytes): model (`mod2` +0x28), animation
graph (`antr` +0x38), collision model (`coll` +0x70), default team (u16 +0x180: 1 player, 2
human, 3 covenant, 4 flood, 5 sentinel; Halo 3's numbers for these), melee damage (`jpt!`
+0x288); biped standing camera height (+0x400), standing collision height (+0x424) and
collision radius (+0x42C): the Flood elite 0.60, 0.70, 0.20; the infection form and the
sentinel store no height (taken from the model's bounds). The collision model (`coll`) holds
the maximum body vitality (+0x8) and shield vitality (+0xCC): the cyborg (the player) 75 and
75.

**Weapon** (`weap`): model (+0x28), triggers (block +0x4FC, 0x114 each): rounds per second
(+0x4, +0x8), projectiles per shot (i16 +0x6E; the shotgun 15), minimum error (+0x78), error
angle (+0x7C, +0x80), projectile (`proj` +0x94). **Projectile** (`proj`): maximum range
(+0x1C8), initial and final velocity (+0x1E4, +0x1E8) **per tick** (x30 for per second: the
assault rifle's bullet 10.8 = 324 wu/s, the plasma rifle's bolt 50), attached detonation
damage (`jpt!` +0x214), impact damage (+0x224). **Damage** (`jpt!`): radius (+0x0, +0x4),
damage lower bound (+0x1D0), upper bounds (+0x1D4, +0x1D8): the assault rifle bullet 10, the
sentinel beam 1 (30 a second), the Flood elite's melee 10..16.

**Model nodes and markers** (`mod2`): nodes (block +0xB8, 0x9C each): name, next sibling
(i16 +0x20), first child (+0x22), parent (+0x24), translation (+0x28), rotation (+0x34, i j k
w), distance from parent (+0x44). Parents come before children. **Rotations are stored
conjugated**: read as stored, the Flood elite's `bip01 spine` (+0.038 along its parent's X)
would sit beside the pelvis, whose stored rotation is (0.5, 0.5, 0.5, 0.5); conjugated it
sits above it, the thighs to either side, the head 0.69 wu up, the feet at 0.11. Markers
(block +0xAC, 0x40 each): name, instances (block +0x34, 0x20 each): region, permutation and
node (i8 +0, +1, +2), translation (+0x4), rotation (+0x10, conjugated too). Bipeds hold their
weapon at `right hand`; weapons fire from `primary trigger`.

**Skinning.** A model vertex's two node indices (i16 +56, +58) and weights (f32 +60, +64,
section 9) are the model's node numbers, except in parts with flag 0x2 (u32 +0 of the part),
which index the part's own table of node numbers (u8 x 22 at part +0x6C). With that, 95 % or
more of the Flood elite's vertices lie within 0.6 wu of their first node.

## 15. Animation graphs (`antr`)

Checked with `probe_h1_ai.py` and `blam_h1/tests/ai.rs`; read by `blam_h1::animation`, played
by `crates/mcc-rust-rewrite/src/halo1_anim.rs`.

| offset | what |
|---|---|
| +0x0C | units, 0x64 each (labels and per-weapon action tables; not read) |
| +0x68 | nodes, 0x40 each: name first (the model's nodes, same order) |
| +0x74 | animations, 0xB4 each |

**Animation** (0xB4): name (char[32]; `stand pistol move-front`, `alert unarmed idle%1`; the
`%N` suffix numbers variants), type (u16 +0x20: 0 base, 1 overlay, 2 replacement), frame
count (+0x22), frame size (+0x24), frame info type (+0x26), node count (+0x2C), loop frame
(+0x2E), frame info (data field +0x48), translation flags (two u32 +0x5C), rotation flags (two
u32 +0x6C), scale flags (two u32 +0x7C), offset to compressed data (+0x88; 0 on every
animation of the run's actors), default data (data field +0x8C), frame data (data field
+0xA0). A data field is 20 bytes: size (+0), then its address at +0xC (tag data addresses,
section 2); the file offset at +0x8 is not one in the map.

Frame data: frame after frame, node after node: the node's rotation (4 x i16 over 32767, i j k
w, conjugated as the model's) if its rotation bit is set, translation (3 x f32) if its
translation bit is, scale (f32) if its scale bit is. The frame size is exactly 8, 12 and 4 per
set bit (the Flood elite's idle: 34 rotations, 1 translation, 284 bytes). Grouped by kind
instead, the quaternions come out with lengths 0.6 to 1.5; interleaved they are all 1.000.
Default data: what the clear bits leave out, once, laid out the same way (676 bytes for that
idle: 6 rotations, 39 translations, 40 scales). Frame info, per frame: dx, dy (type 1, 8 bytes),
dx, dy, dyaw (type 2), dx, dy, dz, dyaw (type 3): the root's motion over the ground, world
units and radians; its mean over a run is the walking speed.

Which animation an actor plays: `<mode> <class> <action>`. Modes `stand`, `alert`,
`crouch`, `flee`, `flaming`, seats (`CD-passengerL01`, `GT-gunner`); classes as the graph has
them, which are not the weapons' names: the Flood combat forms and the Covenant hold their
guns under `pistol`, the grunt's fuel rod under `missle`, the elite's sword under `sword`, the
sentinel under `fixed`, the infection and carrier forms and the hunter under `unarmed`.
Actions: `idle`, `move-front/back/left/right`, `turn-left/right`, `melee`, `airborne`,
`land-soft/hard`, `<weapon> fire-1` (`ar`, `pp`, `pr`, `ne`, `sg`, `fr`), and unit-wide
`h-kill front gut`, `s-ping left gut`, `stand landing-dead`...
## 16. Which BSPs an object stands in

Checked on `d40` with `probe_h1_objects.py`. Halo CE loads one structure BSP at a time, and an
object belongs to the BSPs whose **open space** holds its position: walk the BSP's collision
3D nodes (collision BSP `+0x0`, 12 bytes: plane i32, back child i32, front child i32; planes at
`+0xC`, 16 bytes: normal, d) from node 0, front when `n . p - d >= 0`; a child of -1 is solid
(outside), any other negative child (top bit set) is a leaf (inside). A BSP with no 3D nodes
holds everything (d40's BSP 9). Scenery placements store this mask themselves: **u16 at
`+0x20`**, bit k for BSP k; it equals the point test on all 293 of d40's scenery. Machines and
controls keep zeros there; the point test is used for them.

On d40 the Warthog run's BSPs (5, 6, 7) hold 123 scenery, 6 machines (the lift at the bottom of
its shaft, x -113, z 7.4: `elevator`, `elevator_door`, two blast doors, `enc5_2_door`; and
`asspain_13` at the top, never created by any script) and `poa_explosion` (created only by the
losing cutscene). BSP 5's stated bounds also take in the ship's insides (BSPs 1 to 4; the
interior doors), which the point test leaves out.

## 17. Collision models (`coll`)

The object tag names it at `+0x70` (model `+0x28`, animations `antr` `+0x38`).

| offset | what |
|---|---|
| 0x234 | materials, 0x48 each |
| 0x240 | regions, 0x54 each: name (char[32]), permutations (block `+0x48`, 0x40 each, a name) |
| 0x280 | pathfinding spheres, 0x20 each |
| 0x28C | nodes, 0x40 each |

A **node** (0x40): name (char[32]), region (i16 `+0x20`, -1: none), parent (i16 `+0x22`), next
sibling (`+0x24`), first child (`+0x26`), and its collision BSPs (block `+0x34`, 0x60 each),
**one per permutation of its region**, laid out exactly as the structure BSP's (section 7).
`c_field_generator`'s region `shield` has permutations `shield` and `` (broken) and its node two
BSPs, the second smaller. The placement's desired permutation is the i16 at `+0x6`.

A node's BSP is in that node's frame: put it on the render model's node of the same name
(`mod2` nodes, `+0xB8`, 0x9C each: name, parent i16 `+0x24`, default translation `+0x28`,
rotation `+0x34` as i j k w, relative to the parent). **The stored rotation is the inverse**:
conjugated, the collision lands on the drawn model (mean distance 0.00 against 0.02 wu on
d40's `signal light`; 0.01 against 0.76 on a10's `cryotube`, 0.29 against 0.99 on the crashed
Pelican); identity nodes agree either way. Placed with the object's yaw, pitch, roll and
position (section 4).

Not every object has one: effects scenery (`burning_flame`, `sparks`, the `poa_fire_emitter`s)
and the Longsword in d40's hangar (`fighterbomber_scenery`, radius 12) reference none, so the
game passes through them. On the run the solid ones are 60 `h oxy tank` (a cylinder 1.0 wu
across, 0.83 high, 163 surfaces) and 5 `c_field_generator`. `door_blast_collision` is a
two-surface invisible wall (`elevator_blocker` at the lift's bottom, placed by scripts).

## 18. Devices (`devi`, `mach`, `ctrl`)

`devi` follows `obje` at `+0x17C`: flags (u32), power transition time (f32 `+0x180`), power
acceleration time (`+0x184`), **position transition time** (`+0x188`, seconds closed to open:
`door small` 0.8, `door blast` 14, `elevator` 30), position acceleration time (`+0x18C`); near
the end the engine's per-tick inverses (`+0x284` = 1 / (30 x acceleration time), `+0x288` = 1 /
(30 x transition time)).

`mach` after it: **type** (u16 `+0x290`: 0 door, 1 platform, 2 gear), flags (u16 `+0x292`: 1
pathfinding obstacle, 2 but not when open, 4 elevator), **door open time** (f32 `+0x294`,
seconds a door stays open; `door small` 1.5). Many of a10's "doors" are platforms
(`door small no glass`, `door blast`, `door cryo chamber`): only scripts move them.

Machine and control placements (0x40): after the common start, power group (i16 `+0x28`),
position group (i16 `+0x2A`), **device flags** (u32 `+0x2C`: 1 initially open, 2 initially off,
4 can change only once, 8 position reversed, 16 not usable from any side), and for machines
**machine flags** (u32 `+0x30`: 1 does not operate automatically, 2 one-sided, 4 never appears
locked, 8 opened by melee attack). The flag names are the HEK's as the community documents
them; the values seen on d40 fit them (the lift's top door `asspain_13`: 3, initially open and
off).

The rule we use for a door the game opens itself (a door machine without "does not operate
automatically", powered): it opens over its transition time when the player comes within its
bounding sphere (`obje +0x4` radius, `+0x8` offset) plus 0.6 wu, and closes once he has been
away for its door open time. The distance is ours; the game's exact test was not read.

`antr` (the devices' animations) is not read: in the viewer an open door (position past one
half) loses its collision and is not drawn, a platform keeps its rest pose.

## 19. Sound

Probed 2026-11-03 on The Maw (`d40`) with `probe_h1_sound.py` (`raw TAG`, `lsnd NAME`,
`snd NAME`, `lst BANK [grep]`, `fsb BANK [first [n]]`, `find NAME`, `check NAME`); code
`blam_h1::sound` (tags, banks), `fsb_celt` (the codec), `mcc_audio::halo1` (playing).
Field meanings come from the values (names, lengths, which tags set what) and the public
HEK field order; no other project's code was read.

### 19.1 Where the audio is

MCC's Halo CE has **no `sounds.map`**. A permutation's sample data reference in the map
(`+0x40`: size, flags 1 "external", offset) points into the original release's
`sounds.map`, which MCC does not ship. The audio is in FMOD FSB5 banks in
`halo1\sound\pc\` (`game_paths::halo1_sound()`), found **by tag path** through an index per
bank in `halo1\sound\pc\lst\`:

| bank | samples | codec | what (on `d40`, by tag count) |
|---|---|---|---|
| `sounds_dialogs.fsb` | 6,093 | CELT | every line, English (194 `d40` tags) |
| `sounds_dialogs_french` / `_german` / `_italian` / `_spanish.fsb` | 5,840 (French) ... | CELT | the localised lines, each bank with its own `.lst.bin` (the Chinese, Japanese and Korean `.lst.bin` have no bank) |
| `sounds_music.fsb` | 1,028 | CELT | music, and some long ambience loops (48 tags) |
| `sounds_stream.fsb` | 254 | CELT | streamed loops (29 tags) |
| `sounds_debug.fsb` | 2,245 | CELT | most effects, despite the name (497 tags) |
| `sounds_adpcm.fsb` | 3,106 | IMA ADPCM (7) | other levels' effects, 22.05 / 44.1 kHz (no `d40` tag) |

**`<bank>.lst.bin`**: u32 version (1), u32 entry count, then per entry: u32 length
(including the NUL), the tag path (`sound\music\perc\loops`), u32 first sample, u32 sample
count. The entries' samples follow each other through the bank. Paths compare case-blind.

**A tag's samples** are `first .. first + count`. For 678 of `d40`'s 768 tags in a bank the
count is the tag's actual permutation count and sample `first + k` carries permutation `k`'s
name (`sounds_music` #140 `lp1` .. #144 `lp5` for `perc\loops`). For the rest MCC has its
own takes: names sorted alphabetically instead of in tag order (`strnglp3\loops`: tag `lp4,
lp1, lp2, lp3`, bank `lp1 .. lp4`), or more takes than the tag has permutations (remastered
weapon sounds: tag `shotgun_fire`, bank `shotgun_fire_1 .. _3`). So a play picks among the
bank's samples, not the tag's permutations. 4 `d40` tags are in no bank (the Warthog's
engine, `warthog_7_*`).

**Lookup order** (`Banks::open`, our choice): the player's language bank
(`game_paths::language()`; `mexican` takes the Spanish bank), English dialogue, music,
stream, debug, ADPCM. A line missing from a language bank plays in English.

### 19.2 The banks

FSB5 as in `sound.md` 1 (version 1 header of `0x3C` bytes, the 64-bit sample headers and
their chunks, data at `0x3C + headers + names`), with these differences:

- Codec (`+0x18`) **12, CELT**; header flags (`+0x1C`) `0x80000010`, which marks CELT
  **0.11.0**. The ADPCM bank: codec 7, flags 0.
- Every sample 44.1 kHz (the ADPCM bank also 22.05). Mono or stereo; 218 music-bank samples
  have a channels chunk (type 1) saying 4.
- The name table is filled: one name per sample (`D40_440_Cortana`, `lp1`, `desertwindlp1`).
- Dialogue and effect samples carry a loop chunk (type 3) from 0 to the last frame; music has
  none. Not used: music loops by playing its permutations back to back.
- No `.info` sidecar (Halo 3's banks have one).

**CELT framing.** A sample's data is frames back to back: the 4 bytes `17 C3 0D F3`, a u32
length (464 bytes stereo, 370 mono on the music and dialogue), then one CELT 0.11.0 packet
of 512 frames per channel in a custom mode of 44.1 kHz and 512-frame packets. The packets
decode with the Xiph CELT 0.11.0 library (`third_party/celt-0.11.0`, BSD-3-Clause, built by
`crates/fsb_celt`) to the header's frame count once the last packet is trimmed: `perc\in`
204,288 frames, 4.63 s, -14.5 dBFS RMS, 17 of 408,576 samples at full scale; every first
sample of every `d40` tag in a bank decodes to its header's length. Zero padding follows the
last frame up to the next 16-byte offset.

**4-channel CELT** (ambience: `desertwindlp1`, `tonalghost`): two stereo streams whose
packets take turns (A, B, A, B, ...): `tonalghost`'s 353,792 frames are 691 packets per
pair, and 1,382 packets of 472 bytes are exactly its data, followed by padding and a stray
partial frame. Decoded
as two stereo pairs and folded to stereo, front pair first (assumed).

**Not decoded:** the cutscenes' `*_5.1mix` music (`d40_bridge_music01_5.1mix_v1` ...): the
header says mono, the data has no CELT sync and runs about 2.8 bytes per frame. None is on the
run.

**IMA ADPCM** (codec 7): blocks of 36 bytes per channel, a 4-byte header per channel (sample
i16, step index u8, pad), then 4-byte groups of 8 nibbles taking turns per channel, 64
samples per block (`blam_h1::sound`; not checked, no `d40` tag uses it).

### 19.3 `snd!`

| offset | type | what | evidence |
|---|---|---|---|
| `0x00` | u32 | flags | 2 on music, 0 on the lines |
| `0x04` | u16 | **sound class** (below) | 32 on music, 44 on The Maw's lines |
| `0x06` | u16 | sample rate: 0 22.05 kHz, 1 44.1 kHz (the original release's) | lines 0, music 1 |
| `0x08`, `0x0C` | f32 | **distance**: full volume within, silent beyond (wu) | music 0.9 / 5, lines 3 / 20 |
| `0x10` | f32 | skip fraction | |
| `0x14`, `0x18` | f32 | random pitch bounds | 1, 1 |
| `0x1C`, `0x20`, `0x24` | f32 | inner and outer cone angles (2 pi), outer cone gain | |
| `0x28` | f32 | **gain**, linear | 0.85 on The Maw's lines, 1 on music |
| `0x3C`, `0x40`, `0x44` | f32 | skip, gain, pitch at script scale 0 | 1, 0, 1 |
| `0x54`, `0x58`, `0x5C` | f32 | skip, gain, pitch at script scale 1 | 1, 1, 1 |
| `0x6C` | u16 | encoding: 0 mono, 1 stereo | |
| `0x6E` | u16 | the original release's compression: 3 Ogg | |
| `0x70` | tag ref | promotion sound | |
| `0x98` | block | **pitch ranges**, 0x48 each | |

**Pitch range** (0x48): name (char[32]), natural pitch (f32 `0x20`), bend bounds (`0x24`),
**actual permutation count** (i16 `0x2C`), permutations (block `0x3C`, 0x7C each).

**Permutation** (0x7C): name (char[32]), skip fraction (`0x20`), gain (`0x24`), compression
(u16 `0x28`), **next permutation** (i16 `0x2A`, -1 none), **PCM bytes** of the original
release's data (u32 `0x38`), the sample data reference (`0x40`). The first `actual`
permutations are the plays; the rest continue them through `next` (the original release split
long sounds: `perc\loops` has 10 permutations, 5 actual, `lp1` continued by the second
`lp1`, ...). MCC's bank has one sample per actual permutation, the pieces joined.

**Length from the tag alone**: a permutation's and its continuations' PCM bytes over (rate x
2 x channels): `d40_440_cortana` 581,808 bytes mono at 22.05 kHz = 13.19 s against the bank
take's 13.20 s (French 13.20 s); `perc\loops` `lp1` and its continuation 9.23 s, bank 9.24 s.
`blam_h1::sound::script_seconds` answers the scripts' `sound_impulse_time` from it.

**Classes** (Halo CE's numbering in the HEK's order; checked on `d40` for 32 and 44): 0 and 1
projectile impact and detonation, 4 to 10 weapons, 13 to 15 impacts, 18 footsteps, 19 unit
dialogue, 22 and 23 vehicle collision and engine, 26 to 30 devices, **32 music**, **33 to 35
ambience** (nature, machinery, computers), 39 first-person damage, **44 scripted dialogue
(player), 45 scripted effect, 46 scripted dialogue (other), 47 scripted dialogue (forced
unspatialised)**, 50 game event. There is no class tag: the class levels are in the engine.

### 19.4 `lsnd`

Header, 0x54 bytes:

| offset | type | what |
|---|---|---|
| `0x00` | u32 | flags: bit 1 **not a loop** (`d40_09`, the finale, plays its one bar once; every cutscene `sinomatixx` piece), bit 2 **stops music** (`d40_01` .. `d40_06`) |
| `0x04` .. `0x18` | f32 | 1.0 everywhere (detail periods and gains at scale 0 and 1; not used) |
| `0x20` | f32 | a distance: 5 on music, 3 to 300 on placed loops (120 on the trench ambience) |
| `0x2C` | tag ref | continuous damage effect (none) |
| `0x3C` | block | **tracks**, 0xA0 each |
| `0x48` | block | **detail sounds**, 0x68 each |

**Track** (0xA0): flags (u32 `0x00`: bit 0 fade in, bit 1 fade out on stop, bit 2 fade to the
alternate; Halo 3's meanings, `sound.md` 5), gain (f32 `0x04`, linear), fade in and fade out
times (`0x08`, `0x0C`), then tag references: **start** `0x30`, **loop** `0x40`, **end**
`0x50`, **alternate loop** `0x80`, **alternate end** `0x90`. No alternate transitions and no
cross-fade time.

**Detail** (0x68): sound (tag ref `0x00`), random period bounds s (`0x10`, `0x14`), gain
(`0x18`), flags (`0x1C`), yaw bounds (`0x50`, `0x54`, -pi..pi), pitch bounds (`0x58`, `0x5C`),
distance bounds (`0x60`, `0x64`). Checked on `emitters\fire` (1 to 3 s, `fire_detail`) and the
computer loops.

**The run's music:**

| piece | tracks | rules |
|---|---|---|
| `d40_07` | start `music\perc\in` (4.6 s), loop `perc\loops` (5 bars of 9.2 s), alternate loop `misc\string_idea` | fade out 6 s, fade to the alternate |
| `d40_08` | loop `misc\long` | fade out 6 s |
| `d40_09` | loop `cstrng\cstrng3` | not a loop: plays once |

### 19.5 Scripts, BSP ambience, sound scenery

- **Script tag references** (`scnr + 0x4B4`, 0x28 each, the reference at `+0x18`): every
  `snd!` and `lsnd` the scripts name (`blam_h1::sound::script_sounds`).
- **The run's sound calls** (`.dev\d40_scripts.txt`): `enc6_0_dialog` stops `d40_06` and starts
  `d40_07` at the jeeps; `enc6_6_1` sets its alternate (the Pelican scene) and plays
  `d40_370_cortana`; `trench_scene` plays `d40_400_cortana`; `trench_scene_continue` stops
  `d40_07`, starts `d40_08` and plays `d40_440_cortana`; `enc6_9_1` starts `d40_09`;
  `test_for_endgame` plays `d40_450_cortana` and stops `d40_08` at `grand_finale`. The lines are
  static scripts `d40_NNN_cortana` / `_pilot`: `(sound_impulse_start snd none
  cortana_dialogue_scale)` then `(sleep (max 0 (- (sound_impulse_time snd) 30)))`, so each line
  waits for the one before; `sound_impulse_time` is the sound's length in ticks. Our insertion
  counts `enc6_0_dialog` as passed, which makes its script start `d40_07` at once
  (`mcc_script/tests/h1_sound.rs`).
- **Background sound per cluster**: `sbsp + 0x1FC` is the background sound palette (0x74
  each: name, `lsnd` at `+0x20`), `sbsp + 0x208` the sound environment palette (0x50 each:
  name, `snde` at `+0x20`; not used). A cluster (0x68) names its background sound entry at
  **`+0x4`** and its sound environment at `+0x6` (i16, -1 none). The run's BSPs (`d40f` ..
  `d40h`) share an 8-entry palette of `interior_*` loops (the `_trench` ones for the run).
- **Which cluster a point is in**: the collision BSP's 3D nodes (12 bytes: plane i32, back
  child i32, front child i32; a child with the top bit set is a leaf, its index in the low 31
  bits; -1 solid) from node 0 (front when `n . p - d >= 0`), then the leaf's render leaf
  (`sbsp + 0xE0`, 16 bytes each, as many as the collision leaves: 15,171 on `d40f`) names the
  cluster at `+0x8` (i16). Checked: the first Warthog's bay and the run's trigger volumes all
  land in clusters, the start in one with `interior_base_trench`.
- **Sound scenery**: placements `scnr + 0x2DC` (0x28 each: palette i16 `+0`, position
  `+0x8`), palette `+0x2E8` (0x30, the `ssce` at `+0`); the `ssce`'s object attachments (block
  `+0x140`) name the `lsnd` at `+0`. `d40` has 37 (desert wind, computer loops), all on the
  bridge and at the mission's start, none on the run; not played.

### 19.6 How we play it (`mcc_audio::halo1`)

The tags become Halo 3's player data (`SoundData`): every `snd!` a `SoundInfo` whose
"permutations" are bank sample ids (bank index in the top 8 bits), every `lsnd` a
`blam_sound::tags::Looping` with the same flags (a track that fades to its alternate
cross-fades over its fade-out time, else 2 s: our choice, the tag has no time), so `music.rs`
plays the scripts' music, one-shots and lines as it does Halo 3's (`sound.md` 8). Samples are
decoded on demand and resampled to 48 kHz (Catmull-Rom). Buses by class: 32 music; 19, 44,
46, 47 dialogue; 33 to 35 ambience; the rest effects. Levels: the tags' linear gains; music
and lines as tagged, effects 6 dB and ambience 12 dB down (our choice; at the tag gain the trench's background sat over the music); lines duck the music by 3 dB
and effects by 6 dB (Halo 3's numbers). Background loops follow the listener's cluster
through the ambience in `lib.rs`. Not done: pitch (natural, random, bend), skip fractions,
cone gains, sound environments (reverb), sound scenery, `sound_class_set_gain` beyond music and
ambience, the 5.1 cutscene music.

## What is not read yet

Weapons on foot (netgame equipment through item collections to `weap` tags), campaign vehicles, the AI's command lists, platoons' behaviour, actor
states, grenades and the `actr` behaviour numbers beyond perception, compressed animations,
the graph's unit and weapon action tables, device animations (`antr`), the netgame flags
(teleporters, CTF flags), the multipurpose map's specular
and self-illumination, micro detail, bump and reflection maps, water animation, the indoor fog,
the decals and detail objects.
