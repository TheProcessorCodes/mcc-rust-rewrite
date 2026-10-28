# The real collision BSP of MCC Halo 3 (U13): research notes

Research for milestone 3 ("the real collision BSP after render-geometry collision"). Written
2026-10-27 for MCC Halo 3 build `Dec 21 2023 22:31:37` = Reclaimer `CacheType.MccHalo3U13`.
HANDOFF.md lists this as an open gap: the collision BSP lives in the sbsp's tag resource
`structure_bsp_tag_resources` and Reclaimer has no definition for it. This document closes
the gap on paper: it records the layout from TagTool and Assembly (format facts only, no code
copied), states what Reclaimer does have, and **verifies the whole chain on real bytes** of
`riverworld.map` (Valhalla) and `120_halo.map` with
`docs\format\scripts\probe_collision_resource.py` (read-only, run with `python -I`).

Sources (all read today; URLs in section 17):

- **TagTool** (TheGuardians-CI/TagTool, `master`, GPL-3.0): C# definitions with
  `[TagStructure(Size, MinVersion, MaxVersion, Platform)]` / `[TagField(...)]` attributes.
- **Assembly** (XboxChaos/Assembly, `master`, GPL-3.0): plugin XMLs under
  `src/Assembly/Plugins/Halo3MCC/` (`sbsp.xml`, `zone.xml`, `play.xml`) and the 360 plugins
  under `Plugins/Halo3/` for comparison.
- **Reclaimer** (local clone `D:\reflex-tools\halo-tools\Reclaimer`, commit `36224d7`, GPL-3.0):
  the parts it does have (resource gestalt, layout table, page reading, sbsp resource pointers).

Container facts (header, tag index, `>>2` pointer expansion, StringId layout, section address
translation) are taken as given from the container document and
`docs\format\sbsp-render-geometry.md`; they are not re-derived here. Every offset below is
hex (decimal). Units are Halo world units (wu), Z up; 1 wu = 10 ft = 3.048 m
(Reclaimer `Blam\Common\BlamConstants.cs`, `WorldUnitScale = 10 * StandardUnits.Feet`).

Legend for the "verified" column: **V** = read from riverworld.map bytes by the probe script
and consistent (counts, index ranges, bounds); **S** = stated by the named source only;
**?** = not confirmed.

---

## 0. How each tool selects a version (confirmed before resolving anything)

### 0.1 Reclaimer: MinVersion inclusive, MaxVersion exclusive

`Reclaimer.Core\IO\Dynamic\Extensions.cs`, `ValidateVersion(version, min, max)`:
a version matches when `(no min OR version >= min) AND (no max OR version < max OR
(version == max AND max == min))`. So `MinVersion` is **inclusive**, `MaxVersion` is
**exclusive**, and a single exact version (`min == max`) matches on equality. The XML doc
comments in `Reclaimer.Core\IO\Annotations.cs` say the same ("inclusive minimum version",
"exclusive maximum version") for `[Offset]`, `[FixedSize]`, `[ByteOrder]` and the builder form
`AddVersion(min, max).HasFixedSize(n)` used in `*.config.cs` files
(`StructureDefinition.FindVersionDefinition` calls the same function).

CacheType ordinals (`Reclaimer.Blam\Blam\Common\Enums.cs`): `Halo3Retail = 15`,
`MccHalo3 = 16`, `MccHalo3U4 = 17`, `MccHalo3F6 = 18`, `MccHalo3U6 = 19`, `MccHalo3U9 = 20`,
`MccHalo3U12 = 21`, **`MccHalo3U13 = 22`**, `Halo3ODST = 23`, `MccHalo3ODST = 24`. The
`Dec 21 2023 22:31:37` build string is listed under `MccHalo3U13` with the comment
`[24-02-14] U14`. For U13 therefore: `AddVersion(MccHalo3U4, null)` matches,
`AddVersion(MccHalo3, null)` matches, `[Offset(60, MinVersion = MccHalo3, MaxVersion = Halo3ODST)]`
matches, `AddVersion(Halo3ODST, null)` does not. Everything below is resolved to U13 only.

### 0.2 TagTool: both bounds inclusive, plus a Platform axis

`TagTool/Cache/CacheVersionDetection.cs`:

- `IsBetween(compare, min, max)`: `min` ignored if `Unknown`, else `compare >= min`; `max`
  ignored if `Unknown`, else `compare <= max`. **Both ends inclusive.** `Version = X` on an
  attribute means exactly X. `Gen`/`BuildType` are extra filters.
- `ComparePlatform(attr, actual)`: attribute `Platform.All` matches everything, otherwise
  must equal the cache's platform. Platforms: `All = 0`, `Original = 1` (Xbox/360/old PC),
  `MCC`.
- `TestAttribute` = build type AND generation AND platform AND version.
- `CacheVersion` enum order (same file): `HaloXbox = 0`, ..., `Halo2Vista = 6`,
  `Halo3Beta = 7`, **`Halo3Retail = 8`**, `Halo3ODST = 9`, `HaloOnlineED = 10` ...
  `HaloOnline700123 = 26`, `HaloReach = 27`, `HaloReach11883 = 28`, `Halo4 = 29`.
- **MCC Halo 3 in TagTool is `(CacheVersion.Halo3Retail, CachePlatform.MCC)`.** So the
  applicable attributes are those with `MinVersion <= Halo3Retail <= MaxVersion` (or
  unbounded) and `Platform` in {`All`, `MCC`}.
- Caveat: TagTool's build-string switch maps only `"Sep 29 2021 09:17:56"` (MCC Update 6) to
  `(Halo3Retail, MCC)`; the Dec 21 2023 build is not in the list I read (`CacheVersionDetection.cs`
  lines 189-199, fetched today). Whether TagTool opens U13 maps was not tested (**?**).

### 0.3 Assembly: engine by build string, plugin folder by game id

`src/Blamite/Formats/Engines.xml`: `<engine name="Halo 3 MCC Update 14" version="13"
build="Dec 21 2023 22:31:37" inherits="Halo 3 MCC Update 6">` with layouts
`Formats/Halo3MCC/LayoutsU13` (alt `Formats/Halo3MCC/Layouts`). Tag plugins for all MCC Halo 3
builds are the single folder `src/Assembly/Plugins/Halo3MCC/` (`<plugin game="Halo3MCC">`).
Assembly plugins describe **tag data only**; resource definition data has no plugin (section 14).

---

## 1. The chain from the sbsp tag to the collision arrays

| step | what | where | source | verified |
|---|---|---|---|---|
| 1 | `sbsp` tag | tag of class `sbsp` (riverworld: index 9757, `levels\multi\riverworld\riverworld`, meta at file `0x8C9FCF4`) | container doc | V |
| 2 | collision resource datum | `sbsp + 0x38C (908)`: `TagResourceReference` (section 3). riverworld value `0xFE651D02` | TagTool `ScenarioStructureBsp.cs` field `CollisionBspResource`; Assembly `Halo3MCC/sbsp.xml` `[zone] Asset Datum 3` at `0x38C`; Reclaimer `ResourcePointer3` at 908 | V |
| 3 | `zone` tag | the one tag of class `zone` (riverworld: index 3) | Reclaimer `ResourceIdentifier.ReadData` (`GetGlobalTag("zone")`) | V |
| 4 | resource entry | `zone.Resources[datum & 0xFFFF]`, 64-byte entries (section 4). riverworld: entry 7426, type 5 = `structure_bsp_tag_resources`, owner = the sbsp | TagTool `ResourceGestalt.cs`/`ResourceData.cs`; Reclaimer `CacheFileResourceGestaltTag.config.cs` | V |
| 5 | definition data | slice `[entry.def_offset, +def_length)` of the zone's definition-data buffer (`zone + 0x148/0x154`); apply the entry's fixups (section 4.3) | TagTool `ResourceCacheGen3.GetResourceDefinition` | V |
| 6 | root struct | at `definition + (entry.def_address & 0x1FFFFFFF)`: `structure_bsp_tag_resources`, 0x30 bytes (section 7) | TagTool `StructureBspTagResources.cs`, `CacheAddress.cs` | V |
| 7 | `play` tag | the one tag of class `play` (riverworld: index 1, empty name). TagTool first tries the layout table inlined in `zone + 0x1C`; if its `Sections` count is 0 it reads `play` | TagTool `ResourceCacheGen3.LoadResourceCache`; Reclaimer always reads `play` | V |
| 8 | page bytes | `play.Segments[entry.segment]` -> required page -> deflate -> slice (section 5) | Reclaimer `ResourceIdentifier.ReadData`; TagTool `ResourceCacheGen3.ReadPageData` | V |
| 9 | arrays | every TagBlock address in the definition is a `CacheAddress`: type 1 = inside the definition slice, type 2 = inside the page bytes (section 4.4) | TagTool `CacheAddressType.cs` | V |

The three other sbsp datums, for orientation (riverworld, **V**): `sbsp + 0x254 (596)` =
`0xFDE91C86` -> resource 7302, type 1 `render_geometry_api_resource_definition` (Reclaimer
`ResourcePointer1`); `sbsp + 0x36C (876)` = `0xFDEA1C87` -> resource 7303, whose entry is
**empty** (salt `0xFFFF`, type -1; Reclaimer `ResourcePointer2`; on MCC the sbsp render
geometry lives in the `Lbsp` tag instead, see the render doc); `sbsp + 0x394 (916)` =
`0xFE661D03` -> resource 7427, type 6 `structure_bsp_cache_file_tag_resources` (TagTool
`PathfindingResource`, MCC-only field on Halo 3).

---

## 2. `scenario_structure_bsp` fields that matter for collision (U13, tag size 0x3A0 = 928)

Sources: TagTool `TagTool/Tags/Definitions/ScenarioStructureBsp.cs`
(`[TagStructure(Size = 0x3A0, MaxVersion = Halo3Retail, Platform = MCC)]`), Assembly
`Plugins/Halo3MCC/sbsp.xml` (`baseSize="0x3A0"`), Reclaimer
`Blam\Halo3\Config\ScenarioStructureBspTag.config.cs` (`AddVersion(MccHalo3U4, null)`).

| offset | size | field | notes | source | verified |
|---|---|---|---|---|---|
| `0x28 (40)` | 12 | TagBlock `CollisionMaterials`, element **0x18 (24)** | `+0x0` TagReference `rm  ` render method (16), `+0x10 (16)` i16 runtime global material index, `+0x12 (18)` i16 conveyor surface index, `+0x14 (20)` i16 seam mapping index, `+0x16 (22)` u16 flags (bit 0 "is seam"). Collision surfaces index this block with `material_index`. riverworld: 37 entries, surface material range -1..36 | TagTool `CollisionMaterial` (Size 0x18 MinVersion Halo3Retail); Assembly `Collision Materials` 0x28 | V (count/range) |
| `0x40 (64)`, `0x48 (72)`, `0x50 (80)` | 8 each | world bounds x, y, z (f32 min, max) | riverworld: x -40.85..206.53, y -285.43..92.49, z -15.52..1892.31; all collision vertices fall inside | Reclaimer `XBounds` 64 etc.; Assembly `World Bounds` | V |
| `0x58 (88)` | 12 | TagBlock `StructureSurfaces` | element 4 bytes per Assembly (`i16 first mapping index, i16 count`) but **8 bytes per TagTool** (`StructureSurface` Size 0x8 on MCC: `i32 first, i32 count`). Conflict, not resolved (**?**); not needed for collision | both | ? |
| `0x64 (100)` | 12 | TagBlock `LargeStructureSurfaces`, element 8 | riverworld and 120_halo: count 0 | TagTool (`Version = Halo3Retail, Platform = MCC`); Assembly | V (count 0) |
| `0x70 (112)` | 12 | TagBlock `StructureSurfaceToTriangleMapping`, element 8 (`i32 triangle index, i32 cluster index`) | count 0 on both maps | TagTool | V (count 0) |
| `0xF4 (244)` | 12 | TagBlock `BreakableSurfaces`, element **0x20 (32)** | 1024-bit vector as 8 x i32 | TagTool `BreakableSurfaceBits`; Assembly `Breakable Surface Sets` | S |
| `0x1C0 (448)` | 12 | TagBlock `InstancedGeometryInstances`, element **0x78 (120)** | section 10.2. riverworld: 262 instances | TagTool `InstancedGeometryInstance` (Size 0x78 MaxVersion Halo3ODST); Assembly 0x1C0; Reclaimer `GeometryInstances` 448, `HasFixedSize(120)` | V |
| `0x2CC (716)` | 12 | TagBlock `Breakable Surface Key Table`, element 0x20 | `i16 instanced geometry index, i8 set index, i8 surface index, i32 seed surface index, 6 x f32 bounds` | Assembly only | S |
| `0x380 (896)` | 12 | TagBlock `RawResources` of `StructureBspTagResources` | tag-data copy of the resource definition type; Assembly hides it ("Raw Resources Block"); normally count 0 | TagTool; Assembly | S |
| **`0x38C (908)`** | 8 | `TagResourceReference` **collision** | the structure_bsp_tag_resources datum | TagTool `CollisionBspResource`; Assembly `[zone] Asset Datum 3`; Reclaimer `ResourcePointer3` | V |
| `0x394 (916)` | 8 | `TagResourceReference` pathfinding | `structure_bsp_cache_file_tag_resources` (MCC-only on Halo 3) | TagTool `PathfindingResource` (`MinVersion = Halo3Retail, Platform = MCC`); Assembly `[zone] Asset Datum 4` | V |
| `0x39C (924)` | 4 | i32 `UseResourceItems` | riverworld: 1 | TagTool; Assembly | V |

Reclaimer reads only render-related fields of this tag plus the three `ResourceIdentifier`s;
it never dereferences `ResourcePointer3` (grep: declared in `Halo3\ScenarioStructureBspTag.cs`
line 29 and the config, used nowhere).

---

## 3. `TagResourceReference` (8 bytes)

TagTool `TagTool/Tags/TagResourceReference.cs` (`Size = 0x8, MinVersion = Halo3Beta`):

| offset | type | field | meaning | verified |
|---|---|---|---|---|
| `0x0` | u32 | `Gen3ResourceID` (DatumHandle) | low 16 bits = index into `zone.Resources`; high 16 bits = salt, equals the entry's salt (riverworld `0xFE651D02`: index 7426, salt `0xFE65` = entry salt) | V |
| `0x4` | i32 | `Unused` | 0 / padding | S |

Reclaimer `Halo3\ResourceIdentifier.cs` reads the same u32 ("actually two shorts",
`ResourceIndex = Value & 0xFFFF`).

---

## 4. `zone` = `cache_file_resource_gestalt` (U13)

TagTool `TagTool/Cache/Resources/ResourceGestalt.cs` (`Size = 0x228` for MCC up to Reach),
Assembly `Plugins/Halo3MCC/zone.xml` (`baseSize="0x228"`), Reclaimer
`Halo3\Config\CacheFileResourceGestaltTag.config.cs` (`AddVersion(MccHalo3, null)`).

### 4.1 Tag fields

| offset | field | element size | notes | source | verified |
|---|---|---|---|---|---|
| `0x0 (0)` | i16 map type, `0x2` i16 flags | | | TagTool | S |
| `0x4 (4)` | TagBlock `ResourceDefinitions` ("Resource Type Identifiers") | **0x1C (28)**: `+0x0` 16-byte GUID, `+0x10` 4 x i16 unknown, `+0x18 (24)` StringId name | riverworld has 7 types, index 5 = `structure_bsp_tag_resources` (GUID `9d35d372284ecf5ba50469a20ce623d9`), index 6 = `structure_bsp_cache_file_tag_resources` (`ccea4bf93f42abed235e6782ada114fa`), 0 bitmap, 1 render geometry, 2 constant buffer, 3 sound, 4 model animation | TagTool `ResourceDefinition.cs`; Assembly | V |
| `0x10 (16)` | TagBlock interop definitions | 0x14 | | TagTool; Assembly | S |
| `0x1C (28)` .. `0x63` | inline `ResourceLayoutTable` (same layout as `play`, section 5, 0x48 bytes) | | riverworld: its `Sections` count is 0, so the `play` tag is used | TagTool `LayoutTable`; Assembly blocks at 0x1C/0x28/0x34/0x40/0x4C/0x58 | V (fallback path) |
| **`0x64 (100)`** | TagBlock `TagResources` ("Resources") | **0x40 (64)**, section 4.2 | riverworld: 7428 entries at file `0x5843EAC` | TagTool; Assembly 0x64; Reclaimer `ResourceEntries` 100 | V |
| `0x148 (328)` | i32 definition-data size (start of a 20-byte dataref) | | riverworld: 666,384 (`0xA2B10`) bytes | Reclaimer `FixupDataSize` 328; Assembly dataRef `Native Resource Control Data` 0x148; TagTool `byte[] DefinitionData` | V |
| `0x154 (340)` | shifted pointer to the definition data | | riverworld: file `0x5988640` | Reclaimer `FixupDataPointer` 340 | V |

### 4.2 Resource entry (`TagResources` element, 0x40 = 64 bytes)

TagTool `TagTool/Cache/Resources/ResourceData.cs` (`Size = 0x40, MaxVersion = Halo3ODST`),
Assembly `zone.xml` "Resources", Reclaimer `ResourceEntryBlock` (`AddVersion(Halo3Retail, null)
.HasFixedSize(64)`).

| offset | type | TagTool name | Assembly / Reclaimer name | riverworld entry 7426 | verified |
|---|---|---|---|---|---|
| `0x00 (0)` | TagReference (16) | `ParentTag` | Owner Tag / `OwnerReference` | class `sbsp`, tag 9757 | V |
| `0x10 (16)` | u16 | `Salt` | Resource Salt / low half of `ResourcePointer` | `0xFE65` | V |
| `0x12 (18)` | i8 | `ResourceTypeIndex` | Resource Type Index | 5 (-1 = empty entry) | V |
| `0x13 (19)` | u8 | `Flags` | Control Alignment Bits | `0x04` (meaning unconfirmed; plausibly log2 alignment) | V (value) |
| `0x14 (20)` | i32 | `DefinitionDataOffset` | Control Offset Offset / `FixupOffset` | `0x9DEE0` | V |
| `0x18 (24)` | i32 | `DefinitionDataLength` | Control Size / `FixupSize` | `0x4C00` | V |
| `0x1C (28)` | i32 | `SecondaryFixupInformationOffset` | Optional Control Offset | -1 | V |
| `0x20 (32)` | i16 | `Unknown1` flags: bit 0 primary page valid, bit 1 secondary page valid | Flags "Has Pageable Data / Has Optional Data" / `LocationType` | 1 | V |
| `0x22 (34)` | i16 | `SegmentIndex` | `[play] Location Index` / `SegmentIndex` | 7173 | V |
| `0x24 (36)` | u32 | `DefinitionAddress` (CacheAddress) | Root Fixup | `0x20004BD0` = type 1, offset `0x4BD0` | V |
| `0x28 (40)` | TagBlock | `FixupLocations`, element 8 | Control Fixups / `ResourceFixups` | 831 entries | V |
| `0x34 (52)` | TagBlock | `InteropLocations`, element 8 | Interop Locations / `ResourceDefinitionFixups` | 0 entries | V |

Fixup location (8 bytes, TagTool `ResourceFixupLocation.cs`): `+0x0` u32 `BlockOffset`
(offset inside the definition slice), `+0x4` u32 `Address` (CacheAddress). Interop location
(TagTool `ResourceInteropLocation.cs`): `+0x0` u32 CacheAddress, `+0x4` i32 interop type index.
Reclaimer's `ResourceFixupBlock` comment says "mask 0x0FFFFFFF (4 bit ?, 28 bit offset)"; TagTool's
3-bit type / 29-bit offset is the one confirmed by the bytes (types seen: `0x20...` and `0x40...`).

### 4.3 Applying fixups (TagTool `ResourceCacheGen3.ApplyResourceDefinitionFixups`, in my words)

Copy the `DefinitionDataLength` bytes at `DefinitionDataOffset` out of the zone's definition
data. For every fixup location write its 4-byte `Address` value little-endian at `BlockOffset`
in that copy. Before the fixups the pointer slots contain stale runtime pointers (riverworld
root before fixups: `01 00 00 00 | 00 00 00 00 | 9a 64 5c 50 | ...`, after:
`01 00 00 00 | 00 00 00 20 | 9a 64 5c 50 | ...`). Counts are stored directly and are not
fixed up. **A block with count 0 keeps a garbage address** (riverworld `large_collision_bsps`:
count 0, address bytes `fa 64 5c 50`), so always test the count before touching the address.

### 4.4 `CacheAddress` (TagTool `TagTool/Cache/CacheFile/CacheAddress.cs`, `CacheAddressType.cs`)

u32: `type = value >> 29` (3 bits), `offset = value & 0x1FFFFFFF` (29 bits).
Types: 0 memory, **1 definition** (offset into the fixed-up definition slice), **2 data**
(offset into the required/primary page bytes), 3 secondary data (optional page), 4 tertiary
(Gen4 only). Assembly shows the same bits as flags on the top byte: bit 5 "Fixup" (`0x20` =
type 1) and bit 6 "Raw Page" (`0x40` = type 2). TagBlocks inside definition data are 12 bytes
`{i32 count, u32 CacheAddress, 4 unused}` (**V**: all 4+8 blocks walked with 12-byte stride;
riverworld fixups seen only with types 1 and 2).

---

## 5. `play` = `cache_file_resource_layout_table` (U13)

TagTool `TagTool/Cache/Resources/ResourceLayoutTable.cs` (`Size = 0x48, MinVersion =
Halo3Retail, Platform = MCC`), Assembly `Plugins/Halo3MCC/play.xml` (`baseSize="0x50"`),
Reclaimer `Blam\Halo3\CacheFileResourceLayoutTableTag.cs` (`Segments` at
`[Offset(60, MinVersion = MccHalo3, MaxVersion = Halo3ODST)]`).

| offset | field | element | notes | verified |
|---|---|---|---|---|
| `0x0 (0)` | TagBlock codec definitions | 0x10 GUID | | S |
| `0xC (12)` | TagBlock `SharedFiles` ("Shared Files" / `SharedCaches`) | **0x108 (264)**: `+0x0` 256-char path, `+0x100 (256)` i16, `+0x102 (258)` u16 flags, `+0x104 (260)` u32 block offset | riverworld: `maps\english.map` (flags `0x000B`), `maps\mainmenu.map` (`0x000B`), `maps\shared.map` (`0x0001`); 120_halo adds `maps\campaign.map` (`0x0001`). TagTool/Reclaimer put flags at `0x102`; Assembly puts them at `0x100`. The bytes have `0x0000` at `0x100` and the flag-like values at `0x102`, so TagTool's placement fits. Flag names (Assembly): bit 0 use header IO offset, 1 not required, 2 use header locations, 3 no resources | V |
| `0x18 (24)` | TagBlock `Pages` ("File Locations" / `Pages`) | **0x58 (88)**, see below | riverworld: 1581 pages | V |
| `0x24 (36)` | TagBlock `SubpageTables` ("Streaming Sublocation Tables" / `SizeGroups`) | 0x10: `+0x0` i32 total size, `+0x4` TagBlock of 8-byte `{i32 offset, i32 size}` | riverworld: 0 | V |
| `0x30 (48)` | TagBlock MCC-only `UnknownMCC` ("File Locations Patch Lookup") | 0xC: `i32 patch index, i32 pageable file location index, i32 resource index` | | S |
| **`0x3C (60)`** | TagBlock `Sections` ("Default Locations" / `Segments`) | **0x10 (16)**: `+0x0` i16 required page, `+0x2` i16 optional page, `+0x4` i32 required offset, `+0x8` i32 optional offset, `+0xC (12)` i16 required size index, `+0xE (14)` i16 optional size index | riverworld: 7175 segments | V |
| `0x48 (72)`, `0x4C (76)` | i32 required location count, i32 required DVD location count | Assembly only | | S |

Page element (0x58 = 88 bytes; TagTool `ResourcePage.cs`, Assembly "File Locations",
Reclaimer `PageBlock`):

| offset | type | field | riverworld page 1420 | verified |
|---|---|---|---|---|
| `0x0 (0)` | i16 | salt | | S |
| `0x2 (2)` | u8 | flags (bit 0 valid checksum, 1 shared and required, 2 DVD-only shared, 3 DVD-only, 4 referenced by header, 5 only full valid checksum, 6 shared pre-optimized) | | S |
| `0x3 (3)` | i8 | compression codec index (-1 = none) | 0 | V |
| `0x4 (4)` | i16 | shared cache index (-1 = this map) | -1 | V |
| `0x6 (6)` | i16 | shared file location index | | S |
| `0x8 (8)` | u32 | block address (offset inside the resource section) | `0x0` | V |
| `0xC (12)` | u32 | compressed block size | `0x2B50006` | V |
| `0x10 (16)` | u32 | uncompressed block size | `0x57C3910` | V |
| `0x14 (20)` | i32 | CRC | | S |
| `0x18 (24)`, `0x2C (44)`, `0x40 (64)` | 20 each | SHA-1 of whole / first chunk / last chunk | | S |
| `0x54 (84)` | i16 | resource reference count (Reclaimer `DataChunkCount`) | | S |
| `0x56 (86)` | i16 | streaming sublocation table index | | S |

Reading a page (Reclaimer `ResourceIdentifier.ReadData` + `GetDataTableAddress`, TagTool
`ReadPageData`):

1. If `shared cache index < 0`: file offset = **u32 at header `0x4D0 (1232)`** + block address.
   That header word is section offset [1] (the resource section): riverworld `0x114000`,
   120_halo `0x167000`. (Reclaimer: "latest mcc" branch for `MccHalo3U6+` reads 1232; older
   MCC used 1200/1208, 360 used 1136.) riverworld page 1420 -> file `0x114000`; 120_halo page
   1764 block `0x2E2C000` -> file `0x2F93000`. **V**
2. If `shared cache index >= 0`: open the named map next to this one. Reclaimer seeks
   `16384 + block address` (header size); TagTool uses that file's own section table when the
   shared-file flags bit 0 is set, else `SharedFiles[i].BlockOffset + block`. Not exercised by
   the collision resources of the two maps probed (both in-file). **?**
3. Read `compressed size` bytes. If codec index is -1 or compressed == uncompressed: raw.
   Otherwise **raw deflate** (zlib window -15; Reclaimer `CacheResourceCodec.Deflate`, the
   default for Gen3; Assembly `play.xml` comment "compressed using deflate"). riverworld page
   1420 inflated from `0x2B50006` to exactly `0x57C3910` bytes. **V**
4. Segment -> required page -> `bytes[required offset : + SubpageTables[size index].total]`,
   or to the end of the page when the size index is -1. riverworld segment 7173: page 1420,
   offset `0x5380000`, size index -1 -> 4,471,056 (`0x443910`) bytes of primary data; no
   optional page. **V**

---

## 6. Algorithm: datum -> parsed resource definition (pseudocode, own words)

```
datum      = u32 at sbsp + 0x38C
entry      = zone.Resources[datum & 0xFFFF]                  # 64-byte entries at zone+0x64
assert entry.type_index names "structure_bsp_tag_resources"  # zone+0x4 definitions, StringId at +0x18
defn       = zone_definition_data[entry.def_offset : entry.def_offset + entry.def_length]
for (block_offset, address) in entry.fixups: defn[block_offset..+4] = address (LE u32)
segment    = play.Segments[entry.segment]                    # 16-byte entries at play+0x3C
page       = play.Pages[segment.required_page]               # 88-byte entries at play+0x18
raw        = file[(u32 at 0x4D0) + page.block : + page.compressed]   (in-file page)
data       = inflate(raw) if page.codec >= 0 and page.compressed != page.uncompressed else raw
data       = data[segment.required_offset : + total(size index)]     (to end if index == -1)
secondary  = same with the optional page, if segment.optional_page >= 0
root_off   = entry.def_address & 0x1FFFFFFF   (type must be 1)
resolve(addr) = {1: defn, 2: data, 3: secondary}[addr >> 29], addr & 0x1FFFFFFF
```

Every TagBlock in `defn` is `{i32 count, u32 addr, 4 pad}` and resolves with `resolve`.
On riverworld the root sits at the **end** of the slice (`0x4BD0` of `0x4C00`) with the child
structures before it (collision bsp at `defn+0x0`, instance definitions at `defn+0x11A0`), and
the big arrays all live in `data` (type 2).

---

## 7. `structure_bsp_tag_resources` (MCC Halo 3: 0x30 = 48 bytes)

TagTool `TagTool/Tags/Resources/StructureBspTagResources.cs`:
`[TagStructure(Size = 0x30, MinVersion = Halo3Retail, MaxVersion = Halo3ODST, Platform = MCC)]`.
The Xbox 360 Halo 3 variant is `Size = 0x18` (`MaxVersion = Halo3Retail, Platform = Original`)
with only the two unconditional blocks.

| offset | field | element | present on 360 H3? | riverworld | 120_halo bsp_000 | verified |
|---|---|---|---|---|---|---|
| `0x0 (0)` | TagBlock `CollisionBsps` of `CollisionGeometry` | 0x60 (96), section 8 | yes | 1, at `defn+0x0` | 1 | V |
| `0xC (12)` | TagBlock `LargeCollisionBsps` of `LargeCollisionBspBlock` | 0x60 (96), section 9 | **no** (360 only from ODST on; `[TagField(MinVersion = Halo3Retail, Platform = MCC)]`) | 0 | 0 | V (count) |
| `0x18 (24)` | TagBlock `InstancedGeometry` of `InstancedGeometryBlock` | **0xC4 (196)** on MCC, section 10 | yes (0xB8 there) | 76, at `defn+0x11A0` | 17 | V |
| `0x24 (36)` | TagBlock `HavokData` of `HavokDatum` | 0x34 (52) | **no** (`MinVersion = Halo3Retail, MaxVersion = Halo3ODST, Platform = MCC`) | 0 | 0 | V (count) |

`HavokDatum` (TagTool `TagTool/Havok/HavokDatum.cs`, Size 0x34): `+0x0` i32 prefab index,
`+0x4` TagBlock `HavokGeometries`, `+0x10 (16)` TagBlock `HavokInvertedGeometries`, `+0x1C (28)`
3 x f32 bounds min, `+0x28 (40)` 3 x f32 bounds max. `HavokGeometry` (0x20): i32 unknown, i32
collision type, i32 shape count, 20-byte TagData. Count 0 on both maps probed; purpose for our
engine: none known. **S**

Breakable surfaces live in two places: per instance definition (section 10.1, block
`BreakableSurfaces` of 0x20-byte bit vectors) and in the sbsp tag (`0xF4`, `0x2CC`). The
surface fields `breakable_surface_set` / `breakable_surface_index` are -1 on every surface of
both maps probed.

---

## 8. `CollisionGeometry` (0x60 = 96 bytes) and its elements

TagTool `TagTool/Geometry/BspCollisionGeometry/CollisionGeometry.cs`:
`[TagStructure(Size = 0x60, MaxVersion = HaloOnline700123)]` (Reach adds supernodes and grows
to 0x6C; **Halo 3 has no supernodes**). Eight TagBlocks, in this order:

| offset | block | element size | riverworld count / data offset | 120_halo bsp_000 | verified |
|---|---|---|---|---|---|
| `0x00 (0)` | `Bsp3dNodes` | 8 | 76,575 @ `data+0x0` | 65,775 | V |
| `0x0C (12)` | `Planes` | 16 | 25,779 @ `data+0x95900` | 21,506 | V |
| `0x18 (24)` | `Leaves` | 8 | 42,101 @ `data+0xFA430` | 37,520 | V |
| `0x24 (36)` | `Bsp2dReferences` | 4 | 76,581 @ `data+0x14C7D8` | 68,181 | V |
| `0x30 (48)` | `Bsp2dNodes` | 16 | 8,096 @ `data+0x197470` | 2,687 | V |
| `0x3C (60)` | `Surfaces` | 12 | 24,682 @ `data+0x1B6E70` | 19,378 | V |
| `0x48 (72)` | `Edges` | 12 | 39,962 @ `data+0x1FF368` | 30,300 | V |
| `0x54 (84)` | `Vertices` | 16 | 15,163 @ `data+0x2744A0` | 10,928 | V |

(The data offsets are consistent with the element sizes: e.g. `0x95900 / 8 = 76,576` node
slots incl. alignment, `(0xFA430 - 0x95900) / 16 = 25,779` planes exactly.)

### 8.1 `Bsp3dNode` (8 bytes, `[TagStructure(Size = 0x8, Align = 0x8)]`)

One u64, little-endian, bit-packed (TagTool properties `Plane`, `BackChild`, `FrontChild`,
`GetChildIndex`, `GetChildType`):

| bits | field | meaning |
|---|---|---|
| 0..15 | plane | index into `Planes` |
| 16..39 | back child (24 bits) | child code, see below |
| 40..63 | front child (24 bits) | child code |

Child code (24 bits): bit 23 = type (0 = node, 1 = leaf); bits 0..22 = index, **sign-extended
from 23 bits** (`0x7FFFFF` -> -1 = no child, i.e. empty/solid space). **V**: every plane
index < plane count; every node index < node count; every leaf index < leaf count; riverworld
has 76,574 node references, 42,101 leaf references and 34,475 `-1` children.

### 8.2 `Plane` (16 bytes): `RealPlane3d` = f32 normal x, y, z at `0x0/0x4/0x8`, f32 distance at `0xC (12)`. **V**: |normal| = 1.0000 for all planes.

### 8.3 `Leaf` (8 bytes, `[TagStructure(Size = 0x8, MinVersion = Halo3Retail)]`)

| offset | type | field | notes | verified |
|---|---|---|---|---|
| `0x0 (0)` | u8 | flags | bit 0 = contains double-sided surfaces (`LeafFlags`) | V (values 0/1) |
| `0x1 (1)` | u8 | flags2 | meaning unknown | ? |
| `0x2 (2)` | u16 | bsp2d reference count | riverworld 0..12 | V |
| `0x4 (4)` | u32 | first bsp2d reference | index into `Bsp2dReferences`; **`0xFFFFFFFF` when count is 0** (605 of 42,101 leaves on riverworld; every leaf with count > 0 is in range) | V |

### 8.4 `Bsp2dReference` (4 bytes)

| offset | type | field | notes | verified |
|---|---|---|---|---|
| `0x0 (0)` | i16 | plane index | low 15 bits index `Planes`; the `0x8000` bit is set on 2,301 of 76,581 refs (riverworld), presumably "plane flipped" as for surfaces (section 8.6) | V (range), ? (bit meaning) |
| `0x2 (2)` | i16 | bsp2d node index | `>= 0`: index into `Bsp2dNodes`; **negative: a surface index directly, `value & 0x7FFF`** (71,644 of 76,581 refs; all in range) | V |

### 8.5 `Bsp2dNode` (16 bytes, Align 16)

| offset | type | field | notes | verified |
|---|---|---|---|---|
| `0x0 (0)` | f32 x3 | `RealPlane2d` (i, j, d) | 2-D plane in the parent plane's projection; |ij| = 1.0000 on all nodes | V |
| `0xC (12)` | i16 | left child | `>= 0` node index; negative = surface `value & 0x7FFF` (13,033 of 16,192 children on riverworld) | V |
| `0xE (14)` | i16 | right child | same encoding | V |

Which 2 of the 3 world axes the 2-D plane uses (projection axis from the parent plane's
dominant normal component) is engine logic not stated in the files read (**?**); for a
triangle soup it is not needed.

### 8.6 `Surface` (12 bytes, `[TagStructure(Size = 0xC, MinVersion = Halo3Retail)]`)

| offset | type | field | notes | verified |
|---|---|---|---|---|
| `0x0 (0)` | u16 | plane | low 15 bits index `Planes`. **Bit 15 (`0x8000`) = the polygon faces against the plane normal**: on riverworld 905 of 24,682 surfaces have it, and a winding test (normal of the first three loop vertices vs plane normal, flipped when the bit is set) agrees on 24,682 of 24,682 (120_halo: 19,378 of 19,378) | V |
| `0x2 (2)` | u16 | first edge | index into `Edges` | V |
| `0x4 (4)` | i16 | material index | into sbsp `CollisionMaterials` (`0x28`); -1 = none. riverworld -1..36 | V |
| `0x6 (6)` | i16 | breakable surface set | -1 on both maps | V |
| `0x8 (8)` | i16 | breakable surface index | -1 on both maps | V |
| `0xA (10)` | u8 | flags (`SurfaceFlags`) | 1 two-sided, 2 invisible, 4 climbable, 8 breakable, 16 invalid, 32 conveyor, 64 slip, 128 plane-negated. riverworld: two-sided 612, invisible 484, slip 1877, others 0 (values seen 0, 1, 3, 64, 67); 120_halo bsp_000: all 0 | V |
| `0xB (11)` | u8 | best plane calculation vertex | 0..7 seen | V |

### 8.7 `Edge` (12 bytes)

| offset | type | field | verified |
|---|---|---|---|
| `0x0 (0)` | u16 | start vertex | V (< vertex count) |
| `0x2 (2)` | u16 | end vertex | V |
| `0x4 (4)` | u16 | forward edge (next edge around the left surface) | V (< edge count) |
| `0x6 (6)` | u16 | reverse edge (next edge around the right surface) | V |
| `0x8 (8)` | u16 | left surface | V (< surface count; `0xFFFF` would mean none) |
| `0xA (10)` | u16 | right surface | V |

### 8.8 `Vertex` (16 bytes, Align 16)

| offset | type | field | verified |
|---|---|---|---|
| `0x0 (0)` | f32 x3 | point (wu, Z up) | V (inside the sbsp world bounds on both maps) |
| `0xC (12)` | u16 | first edge | V (< edge count) |
| `0xE (14)` | i16 | sink | riverworld all 0; 120_halo small values 0..31. Meaning unknown | ? |

---

## 9. The "large" variants (TagTool `LargeCollisionGeometry.cs`)

`LargeCollisionBspBlock` (`Size = 0x60`, same eight blocks in the same order, same 12-byte
block offsets as section 8) widens every index to 32 bits; `Planes` and `Leaves` are the
same element types as the normal BSP.

| element | normal size | large size | large layout |
|---|---|---|---|
| `LargeBsp3dNode` | 8 (packed) | **12** | `0x0` i32 plane, `0x4` i32 back child, `0x8` i32 front child (not bit-packed; child sign/leaf encoding as in 8.1 presumed, **?**) |
| `Plane` | 16 | 16 | unchanged |
| `Leaf` | 8 | 8 | unchanged |
| `LargeBsp2dReference` | 4 | **8** | `0x0` i32 plane index, `0x4` i32 bsp2d node index |
| `LargeBsp2dNode` | 16 | **20** | `0x0` RealPlane2d (12), `0xC (12)` i32 left child, `0x10 (16)` i32 right child |
| `LargeSurface` | 12 | **16** | `0x0` i32 plane, `0x4` i32 first edge, `0x8` i16 material, `0xA (10)` i16 breakable set, `0xC (12)` i16 breakable surface, `0xE (14)` u8 flags, `0xF (15)` i8 best plane vertex |
| `LargeEdge` | 12 | **24** | 6 x i32: start, end, forward, reverse, left surface, right surface |
| `LargeVertex` | 16 | **20** | `0x0` point (12), `0xC (12)` i32 first edge, `0x10 (16)` i32 sink |

When a BSP is stored large: TagTool's `ResizeCollisionBSP.GrowCollisionBsp` converts a normal
BSP to large when porting Reach content, and the HO porter clears `CollisionBsps` after
growing; for MCC Halo 3 the block exists but is **empty on both maps probed**. The threshold
the Halo 3 MCC compiler uses (presumably any count exceeding the 16-bit index ranges) is not
stated in the sources read (**?**). A reader must handle both: iterate `CollisionBsps` with
section 8 layouts and `LargeCollisionBsps` with these.

---

## 10. Instanced geometry collision

### 10.1 `InstancedGeometryBlock` (resource side; MCC Halo 3: 0xC4 = 196 bytes)

TagTool `TagTool/Geometry/BspCollisionGeometry/InstancedGeometry.cs`:
`[TagStructure(Size = 0xC4, MaxVersion = Halo3ODST, Platform = MCC)]` (360 Halo 3: `0xB8`,
the difference is the MCC-only `SmallSurfacesPlanes` block). Element stride **V**: with 0xC4
the per-definition `mesh index` equals the definition index 0..75 on riverworld, lightmap
scale is 1.0 everywhere and every bounding sphere is sane.

| offset | size | field | notes | verified |
|---|---|---|---|---|
| `0x00 (0)` | 4 | i32 checksum | | V (plausible) |
| `0x04 (4)` | 12 | f32 x3 bounding sphere offset (local) | riverworld def 0: (0.24, -1.20, 0.86) | V |
| `0x10 (16)` | 4 | f32 bounding sphere radius | def 0: 7.12 | V |
| **`0x14 (20)`** | 0x60 (96) | `CollisionGeometry` **inline** (section 8 layout, local space) | def 0 counts: 287 nodes, 100 planes, 193 leaves, 349 refs, 191 2-D nodes, 212 surfaces, 423 edges, 213 vertices | V |
| `0x74 (116)` | 12 | TagBlock `RenderBsp` of `CollisionGeometry` (0x60) | 0 on all 76 defs | V (count) |
| `0x80 (128)` | 12 | TagBlock `CollisionMoppCodes` of `TagHkpMoppCode` (0x40) | Havok MOPP byte code for the instance; 1 per def on riverworld. Not needed for our own collision | V (count) |
| `0x8C (140)` | 12 | TagBlock `BreakableSurfaces` of `BreakableSurfaceBits` (0x20) | 0 on riverworld | V (count) |
| `0x98 (152)` | 12 | TagBlock `SmallSurfacesPlanes` (4: u16 plane index, i16 plane count) **MCC only** | 0 on riverworld | V (count) |
| `0xA4 (164)` | 12 | TagBlock `SurfacePlanes` of `StructureSurface` (8 on MCC: i32 first mapping, i32 count) | count == surface count on every def (212 for def 0) | V |
| `0xB0 (176)` | 12 | TagBlock `Planes` of `StructureSurfaceToTriangleMapping` (8: i32 triangle index, i32 cluster index) | maps collision surfaces to render triangles; def 0: 422 | V (count) |
| `0xBC (188)` | 2 | i16 mesh index (render mesh in the sbsp/Lbsp geometry) | == definition index on riverworld | V |
| `0xBE (190)` | 2 | i16 compression index | == definition index on riverworld | V |
| `0xC0 (192)` | 4 | f32 global lightmap resolution scale | 1.0 | V |

`TagHkpMoppCode` (0x40 on MCC; TagTool `TagTool/Havok/HavokTypes.cs` `HkpMoppCode` 0x30 +
`TagHkpMoppCode` 0x10; Assembly sbsp "Collision MOPP Codes" elementSize 0x40): `0x0` u64 vtable
(runtime), `0x8` u16 size/flags + u16 refcount, `0x10 (16)` 4 x f32 code info offset,
`0x20 (32)` u64 data pointer (runtime) + u32 size + u32 capacity/flags, `0x30 (48)` TagBlock of
bytes = the MOPP program, `0x3C (60)` i8 build type, 3 pad. **S**. We do not need MOPP; it is
Havok's broadphase for the same surfaces.

### 10.2 `InstancedGeometryInstance` (sbsp tag, `0x1C0`, element 0x78 = 120)

TagTool `InstancedGeometryInstance` (`Size = 0x78, MaxVersion = Halo3ODST`), Assembly
`sbsp.xml` lines 524-640, Reclaimer `BspGeometryInstanceBlock` (`HasFixedSize(120)`).
(corrected: sbsp-render-geometry.md 2.2 used to carry a second field map for the same 120 bytes
with `0x38` "i32 unknown", `0x58` "i16 always 0", `0x5A` "i16 {1,2,3}"; the two are now merged. 2.2
holds the same names as this table plus riverworld value histograms (`0x38` -1 on 77 instances else
a distinct 0..184; `0x58` 0 on all 262; `0x5A` 3 x185, 1 x59, 2 x18; `BspPhysics` elements 176
bytes apart), all still **S** = named from TagTool/Assembly, unverified in game.)

| offset | size | field | notes | verified |
|---|---|---|---|---|
| `0x00 (0)` | 4 | f32 scale | uniform; Reclaimer `TransformScale` | S |
| `0x04 (4)` | 48 | 4x3 matrix: forward (3 f32), left (3), up (3), position (3) | rows at `0x4`, `0x10 (16)`, `0x1C (28)`, `0x28 (40)`; Reclaimer `Transform` at 4 | S |
| `0x34 (52)` | 2 | i16 instance definition index | into the resource's `InstancedGeometry`; -1 = none. Reclaimer calls it `SectionIndex` (valid because mesh index == definition index) | V (262 instances, 76 defs) |
| `0x36 (54)` | 2 | u16 flags | bit 0 not in lightprobes, 1 **render only**, 2 does not block AOE damage, 3 **collidable**, 4 decal spacing, 5 not for render (Assembly) / TagTool adds 6 "not for render" | S |
| `0x38 (56)` | 2 | i16 lightmap texcoord block index | | S |
| `0x3A (58)` | 2 | i16 unknown (padding in TagTool) | | S |
| `0x3C (60)` | 4 | runtime (TagTool `SeamBitVector[1]`; Assembly two runtime i16s) | | S |
| `0x40 (64)` | 12 | f32 x3 world bounding sphere center | | S |
| `0x4C (76)` | 4 or 8 | f32 world bounding sphere radius; Assembly then i32 checksum at `0x50 (80)`; TagTool reads an 8-byte radius bounds at 0x4C instead | conflict, irrelevant for geometry | ? |
| `0x54 (84)` | 4 | StringId name | Reclaimer `Name` at 84 | S |
| `0x58 (88)` | 2 | enum16 pathfinding policy (0 cut-out, 1 static, 2 none) | | S |
| `0x5A (90)` | 2 | enum16 lightmapping policy | | S |
| `0x5C (92)` | 4 | f32 lightmap resolution scale | | S |
| `0x60 (96)` | 12 | TagBlock `BspPhysics` of `InstancedGeometryPhysics`, element **0xB0 (176)** on MCC (0x70 on 360) | Havok shape wrapper: AABB center/half extents at `+0x30`/`+0x40`, `+0x68` i8 structure BSP index, `+0x69` u8 shape type, `+0x6A` i16 instance index, `+0x6C` f32 scale, then the MOPP bv-tree shape (Assembly) | S |
| `0x6C (108)` | 2+2 | u16 fade pixels start, end | | S |
| `0x70 (112)` | 2+2+4 | i16 cubemap 0, i16 cubemap 1, f32 blend factor | | S |

Placing an instance's collision: `world = position + scale * (x * forward + y * left + z * up)`
for each local vertex (Reclaimer applies `UniformScale` separately from the 4x3 `Transform`;
TagTool's OBJ extractor multiplies by the 4x3 matrix and ignores `scale`, so check on a scaled
instance before trusting either, **?**). Honour flags: skip `render only`
instances for collision; `collidable` marks instances the engine collides with.

---

## 11. Algorithm: collision BSP -> triangle soup (own words; TagTool `ExtractCollisionGeometryCommand.cs` does the same walk)

Each surface is a convex planar polygon stored as a winged-edge loop:

```
for i, surface in enumerate(surfaces):
    e = surface.first_edge; loop = []
    repeat:
        edge = edges[e]
        if edge.left_surface == i:   loop.append(edge.start_vertex); next = edge.forward_edge
        elif edge.right_surface == i: loop.append(edge.end_vertex);  next = edge.reverse_edge
        else: break   # corrupt
        if next == surface.first_edge: break
        e = next
    # loop is in consistent winding; the plane (flipped if surface.plane & 0x8000) is its normal
    fan: (loop[0], loop[k], loop[k+1]) for k in 1..len-2
```

**V** on riverworld cluster BSP: 24,682 loops, 0 corrupt, sizes {3: 19,550, 4: 4,538, 5: 479,
6: 89, 7: 15, 8: 11} -> **30,560 triangles**; 120_halo bsp_000: 19,378 loops -> 21,844
triangles. Instance definitions: riverworld 76 defs -> 13,902 triangles before instancing
(262 placements), 120_halo bsp_000 17 defs -> 4,600 (99 placements). Compare the render mesh:
`D:\reflex-tools\halo-tools\README.md` (120_halo notes) counts 4.1 M drawn triangles for the
whole level (3.1 M of them instanced geometry, 0.83 M foliage cards), so the collision mesh is
one to two orders of magnitude lighter than the render geometry and has no foliage at all.

Per-triangle attributes worth carrying into the skate world: `material_index` (-> sbsp
collision material -> render method, for surface type), flags `two-sided`, `invisible`
(invisible walls / soft boundaries are real collision here: keep them, maybe tag them),
`slip`, `climbable`, `conveyor`, `breakable`. Winding is already consistent with the plane, so
the plane normal (or its flip) is the face normal.

For point/ray queries without a triangle soup: descend `Bsp3dNodes` from node 0 with the
plane test (`dot(n, p) - d`), child codes per 8.1; a leaf lists `count` consecutive
`Bsp2dReferences` from `first`; each reference names a plane and either a 2-D BSP node tree
(`Bsp2dNodes`, children negative = surface) or directly a surface (negative index).

---

## 12. EXPECTED VALUES on riverworld.map (Valhalla), for a reader's unit test

- `sbsp` = tag 9757; `u32 @ meta+0x38C` = `0xFE651D02`; `u32 @ meta+0x394` = `0xFE661D03`.
- `zone` = tag 3: 7428 resource entries at file `0x5843EAC`; definition data 666,384 bytes at
  file `0x5988640`; 7 resource type names, index 5 = `structure_bsp_tag_resources`.
- entry 7426: owner `sbsp` 9757, salt `0xFE65`, type 5, flags `0x04`, definition
  `0x9DEE0 .. +0x4C00`, secondary -1, page flags 1, segment 7173, root `0x20004BD0`, 831
  fixups (first: `def+0x4 <- 0x40000000`, `def+0x10 <- 0x40095900`, `def+0x1C <- 0x400FA430`),
  0 interops.
- `play` = tag 1: 3 shared files, 1581 pages, 0 subpage tables, 7175 segments. Segment 7173
  = page 1420 @ offset `0x5380000`, size index -1. Page 1420: codec 0, in-file, block 0,
  `0x2B50006` compressed -> `0x57C3910` inflated; file offset `0x114000`. Primary data
  4,471,056 bytes.
- root (after fixups): `collision_bsps` 1 @ `def+0x0`; `large_collision_bsps` 0;
  `instanced_geometry` 76 @ `def+0x11A0`; `havok_data` 0.
- collision bsp 0: counts and offsets as in section 8; 30,560 triangles; vertex bounds
  x -38.09..206.53, y -285.43..89.62, z -9.29..1892.31 wu.
- 120_halo.map bsp_000 (tag 13608): entry 9301, definition `0xD3110 .. +0x1194`, 202
  fixups, segment 8777 -> page 1764 (block `0x2E2C000`, file `0x2F93000`, `0xAB2C2F` ->
  `0x19D2220`), root `def+0x1164`, 1 bsp / 17 defs / 0 large / 0 havok, 21,844 + 4,600
  triangles.

Reproduce with `python -I docs\format\scripts\probe_collision_resource.py [map]`.

---

## 13. MCC Halo 3 vs Xbox 360 Halo 3 (from the version attributes)

| item | Xbox 360 Halo 3 (`Halo3Retail`, `Original`) | MCC Halo 3 (`Halo3Retail`, `MCC`) | source |
|---|---|---|---|
| byte order | big-endian | little-endian | TagTool `IsLittleEndian`; Reclaimer metadata |
| tag-data pointers | plain 32-bit virtual | 32-bit stored `>> 2` | Reclaimer `MccHalo3\PointerExpander.cs` |
| sbsp tag size | `0x388 (904)` | `0x3A0 (928)` | TagTool; Assembly `baseSize` |
| collision resource datum | `sbsp+0x37C (892)` ("[zone] Tag Resources Asset Datum") | `sbsp+0x38C (908)` | Assembly `Halo3/sbsp.xml` vs `Halo3MCC/sbsp.xml` |
| pathfinding resource datum in sbsp | absent (first appears in ODST) | `sbsp+0x394 (916)` | TagTool `PathfindingResource` attributes |
| collision materials block | `0x24 (36)` | `0x28 (40)` | Assembly |
| instanced geometry instances block | `0x1B0 (432)` | `0x1C0 (448)` | Assembly |
| `StructureSurfaces` element | 4 bytes (u16 first, i16 count) | 8 bytes (i32, i32) per TagTool; Assembly still shows 4 | TagTool `Surface.cs` |
| `LargeStructureSurfaces` block | absent | present (`0x64`) | TagTool |
| `structure_bsp_tag_resources` | `0x18`: collision bsps, instanced geometry | `0x30`: + large collision bsps, + havok data | TagTool `StructureBspTagResources.cs` |
| `InstancedGeometryBlock` | `0xB8 (184)` | `0xC4 (196)`: + `SmallSurfacesPlanes` block at `0x98` | TagTool `InstancedGeometry.cs` |
| `InstancedGeometryPhysics` / `CollisionBspPhysicsDefinition` | `0x70 (112)` | `0xB0 (176)` (64-bit Havok pointers) | TagTool `CollisionBspPhysics.cs`; Assembly BSP Physics 0xB0 |
| `CollisionGeometryShape` | `0x38 (56)` | `0x40 (64)` (+ large bsp address) | TagTool `CollisionGeometryShape.cs` |
| `CollisionKdHierarchyStatic` | `0x4C` | `0x50` (4 pad) | TagTool |
| `ResourceLayoutTable` (`play`) | `0x3C`, segments at `0x30 (48)` | `0x48`, + patch-lookup block, segments at `0x3C (60)` | TagTool; Reclaimer `[Offset(48/60 ...)]` |
| `zone` size | `0x214` | `0x228` | TagTool `ResourceGestalt.cs` |
| resource section file base | u32 at header `1136 (0x470)` | u32 at header `1232 (0x4D0)` (U6+) | Reclaimer `ResourceIdentifier.GetDataTableAddress` |
| cluster block (sbsp) | 220 bytes | 280 bytes | Reclaimer `ClusterBlock` config |
| the collision element layouts themselves (`Bsp3dNode`, `Plane`, `Leaf`, `Bsp2dReference`, `Bsp2dNode`, `Surface`, `Edge`, `Vertex`) | identical sizes, big-endian | identical sizes, little-endian | TagTool has no `Platform = MCC` variants for them |

MOPP byte code also differs per platform (TagTool `HavokConverter.ConvertMoppCodes` is called
when porting from 360); irrelevant if we build our own acceleration structure.

---

## 14. What Reclaimer, Assembly and HaloExport have today

**Reclaimer** (`grep -rni collision Reclaimer.Blam\Blam`): the only hits are `TagReference
CollisionModel` properties in `Halo1\ObjectTag.cs`, `Halo2|Halo3|Halo4|Halo5|HaloReach\ModelTag.cs`
and `TagReference CollisionDamage` in `Halo3|HaloReach\ObjectTag.cs`. There is **no** `coll`
(collision_model) definition and **no** collision BSP definition for any game. What it does
have and we can lean on as facts: `Halo3\CacheFileResourceGestaltTag(.config).cs` (entry
layout of section 4.2, fixup/interop blocks, definition data pointer),
`Halo3\CacheFileResourceLayoutTableTag.cs` (section 5), `Halo3\ResourceIdentifier.cs`
(page reading incl. the `0x4D0` header word and deflate), and `Halo3\Halo3Common.GetMeshes`,
which already reads a resource's definition data (vertex/index buffer infos at
`entry.FixupOffset`) for render geometry. Reclaimer's sbsp reader declares `ResourcePointer3`
(= the collision datum) and never uses it.

**Assembly**: `Plugins/Halo3MCC/sbsp.xml`, `zone.xml`, `play.xml` describe the tag side
exactly as in sections 2, 4, 5 (plus `coll.xml` for the `collision_model` tag of objects, not
read here). Nothing in the repository tree (3,940 paths listed via the GitHub API) describes
`structure_bsp_tag_resources`; the plugin format has no concept of resource definition data.

**HaloExport** (`D:\reflex-tools\halo-exports\riverworld\halo_export.json`): top-level key
`"collision_bsp": "not exported (render geometry only); see README gaps"`, written by
`D:\reflex-tools\halo-tools\HaloExport\Program.cs` line 1085. `D:\reflex-tools\halo-tools\README.md`
"Gaps": "Collision BSP not exported. In MCC Halo 3 the collision BSP lives in the sbsp's tag
resource (structure_bsp_tag_resources), and Reclaimer has no definition for it. The pipeline
builds collision from render triangles anyway. Invisible walls and soft-kill volumes are
therefore absent."

---

## 15. Licenses

- **TagTool**: `LICENSE` in the repo root is the GNU General Public License version 3
  (fetched raw today; GitHub sidebar also says GPL-3.0). README: "A command-line tool designed
  to aid in the research and modification of Halo cache files. Currently supports Gen3
  (halo3, odst, partial support for reach)".
- **Assembly**: `LICENSE` is GPL version 3 (fetched raw; 35,149 bytes, header "GNU GENERAL
  PUBLIC LICENSE Version 3, 29 June 2007"). The plugin XML files are part of that GPL
  distribution.
- **Reclaimer**: GPL-3.0 (HANDOFF.md, CLAUDE.md).

Practical reading (not legal advice): the **facts** recorded here (offsets, sizes, field
meanings, the walk order) are what we implement from; the C# source and the XML plugins are
GPL-3.0 expression and must not be copied into this repo or ported line by line (that would
make the Rust reader GPL-3.0, conflicting with the Apache-2.0 IW4L lineage in HANDOFF.md).
This document deliberately contains no code from any of the three projects; the probe script
was written from the facts. Raise the GPL question with the user before anyone ports code.

---

## 16. Unsure / open

1. Threshold at which Halo 3 MCC emits `large_collision_bsps` instead of `collision_bsps`;
   none of the probed BSPs use it. Check more maps (big campaign BSPs, e.g. `070_waste`,
   `100_citadel`) with the probe before relying on 16-bit-only parsing.
2. `havok_data` (MCC-only `HavokDatum` block): empty on both maps; purpose unknown.
3. Shared-cache page offset rule (Reclaimer `16384 + block` vs TagTool header section table)
   is untested here; both sbsp collision resources were in their own map file.
4. Instance scale: Reclaimer applies `scale` separately from the 4x3 matrix; TagTool's OBJ
   extractor ignores it. Verify on an instance with scale != 1.
5. `Bsp2dReference.plane` high bit (set on 2,301 of 76,581 refs) presumed "flipped plane" by
   analogy with surfaces; not tested.
6. `Vertex.sink`, `Leaf.flags2`, resource entry `flags` (`0x04`): meanings unknown.
7. `StructureSurfaces` element size conflict (Assembly 4 vs TagTool 8) and the instance
   `0x4C..0x53` conflict (radius+checksum vs radius bounds). Not on the collision path.
8. TagTool recognises MCC Halo 3 only by the U6 build string in the code read; whether it
   handles U13 is untested. Assembly explicitly lists the U13/U14 build.
9. 2-D BSP projection axis selection for point queries (engine logic, not in the data files).
10. Collision materials -> physical surface type: the `rm  ` render method tag (and from
    there a global material) must be followed to know ground vs metal etc.; not researched.

---

## 17. Sources (URLs)

TagTool (github.com/TheGuardians-CI/TagTool, branch `master`, raw files fetched 2026-10-27):

- LICENSE: https://github.com/TheGuardians-CI/TagTool/blob/master/LICENSE
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Tags/Resources/StructureBspTagResources.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Tags/Resources/StructureBspCacheFileTagResources.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Geometry/BspCollisionGeometry/CollisionGeometry.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Geometry/BspCollisionGeometry/LargeCollisionGeometry.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Geometry/BspCollisionGeometry/InstancedGeometry.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Geometry/BspCollisionGeometry/BreakableSurface.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Geometry/BspCollisionGeometry/Surface.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Geometry/BspCollisionGeometry/CollisionBspPhysics.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Geometry/BspCollisionGeometry/CollisionGeometryShape.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Geometry/BspCollisionGeometry/CollisionKdHierarchy.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Tags/Definitions/ScenarioStructureBsp.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Tags/TagResourceReference.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Tags/TagResourceTypeGen3.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Tags/TagStructureAttribute.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Tags/TagFieldAttribute.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Cache/CacheVersionDetection.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Cache/Gen3/ResourceCacheGen3.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Cache/Resources/ResourceGestalt.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Cache/Resources/ResourceData.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Cache/Resources/ResourceDefinition.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Cache/Resources/ResourceFixupLocation.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Cache/Resources/ResourceInteropLocation.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Cache/Resources/ResourceLayoutTable.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Cache/Resources/ResourcePage.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Cache/Resources/ResourceSection.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Cache/Resources/ResourceSubpageTable.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Cache/Resources/ResourceSharedFile.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Cache/CacheFile/CacheAddress.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Cache/CacheFile/CacheAddressType.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Havok/HavokDatum.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Havok/HavokTypes.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Commands/ScenarioStructureBSPs/ExtractCollisionGeometryCommand.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Porting/Gen3/PortingContextGen3.BspCollision.cs
- https://github.com/TheGuardians-CI/TagTool/blob/master/TagTool/Common/PageableResource.cs

Assembly (github.com/XboxChaos/Assembly, branch `master`):

- LICENSE: https://github.com/XboxChaos/Assembly/blob/master/LICENSE
- https://github.com/XboxChaos/Assembly/blob/master/src/Assembly/Plugins/Halo3MCC/sbsp.xml
- https://github.com/XboxChaos/Assembly/blob/master/src/Assembly/Plugins/Halo3MCC/zone.xml
- https://github.com/XboxChaos/Assembly/blob/master/src/Assembly/Plugins/Halo3MCC/play.xml
- https://github.com/XboxChaos/Assembly/blob/master/src/Assembly/Plugins/Halo3/sbsp.xml (Xbox 360)
- https://github.com/XboxChaos/Assembly/blob/master/src/Blamite/Formats/Engines.xml
- https://github.com/XboxChaos/Assembly/blob/master/src/Blamite/Formats/Halo3MCC/LayoutsU13/H3MCC_LayoutsU13_Resources.xml

Local (read-only):

- `D:\reflex-tools\halo-tools\Reclaimer\Reclaimer.Core\IO\Dynamic\Extensions.cs`, `...\IO\Annotations.cs`, `...\IO\Dynamic\StructureDefinition.cs`
- `D:\reflex-tools\halo-tools\Reclaimer\Reclaimer.Blam\Blam\Common\Enums.cs`, `BlamConstants.cs`, `StringIdTranslator.cs`, `TagBlock.cs`, `CacheMetadata.cs`
- `D:\reflex-tools\halo-tools\Reclaimer\Reclaimer.Blam\Blam\Halo3\ScenarioStructureBspTag.cs`, `Config\ScenarioStructureBspTag.config.cs`, `CacheFileResourceGestaltTag.cs`, `Config\CacheFileResourceGestaltTag.config.cs`, `CacheFileResourceLayoutTableTag.cs`, `ResourceIdentifier.cs`, `Halo3Common.cs`, `ModelTag.cs`, `ObjectTag.cs`
- `D:\reflex-tools\halo-tools\Reclaimer\Reclaimer.Blam\Blam\MccHalo3\PointerExpander.cs`
- `D:\reflex-tools\halo-tools\README.md` (Gaps), `D:\reflex-tools\halo-tools\HaloExport\Program.cs` line 1085, `D:\reflex-tools\halo-exports\riverworld\halo_export.json`
- Map files read: `C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps\riverworld.map` (0x952C000 bytes), `...\120_halo.map` (0x1A251000 bytes)
- Downloaded source copies kept for re-checking under
  `D:\GAME ISO AND ROMS\PC - AI MASHUP\halo-3-rust-rewrite\.research-cache\{tagtool,assembly}\` (GPL-3.0 reference material, not part of our code).
