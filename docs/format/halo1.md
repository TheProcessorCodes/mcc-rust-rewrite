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
offset into the file (8). `sounds.map` is the same for sound.

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
`blam_script::ScriptData`, so `blam_script`'s decompiler prints it and `halo3_script` runs it.

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

## 13. Which BSPs an object stands in

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

## 14. Collision models (`coll`)

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

## 15. Devices (`devi`, `mach`, `ctrl`)

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

## What is not read yet

Weapons on foot (netgame equipment through item collections to `weap` tags), sound
(`sounds.map`), campaign vehicles, bipeds and AI (encounters), device animations (`antr`), the
netgame flags (teleporters, CTF flags), the multipurpose map's specular
and self-illumination, micro detail, bump and reflection maps, water animation, the indoor fog,
the decals and detail objects.
