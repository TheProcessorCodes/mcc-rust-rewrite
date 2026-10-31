# HaloExport notes: how the C# exporter walks an MCC Halo 3 map with Reclaimer

Written 2026-10-27 from reading `HaloExport\Program.cs` (1113 lines),
`halo-tools\README.md` and the Reclaimer sources it calls into
(<https://github.com/Gravemind2401/Reclaimer>, commit `36224d7`, GPL-3.0). This file records **format facts and
the order of operations** in plain words. No Reclaimer code is copied; every fact names the Reclaimer
source file and property so a reviewer can re-check it. Offsets are given as decimal (hex).

Target cache: MCC Halo 3 build `Dec 21 2023 22:31:37` = Reclaimer `CacheType.MccHalo3U13` ("U13" below).
The container layout (header, tag index, string ids, pointer shifting, section translation) is taken as
already verified; see the container facts in the task hand-off and `docs/format/`. Where this file repeats
a container number it is only to show which Reclaimer property reads it.

Companion file with the numbers to test against: `docs/format/reference-numbers.md`.
Scripts that re-derive those numbers from the raw `.map` bytes: `docs/format/scripts/*.py`
(`python -I script.py <map>`; they never write).

---

## 1. Reclaimer's version rule (read first, used everywhere below)

Source: `Reclaimer.Core\IO\Dynamic\Extensions.cs`, method `ValidateVersion(version, min, max)`, and the
attribute doc comments in `Reclaimer.Core\IO\Annotations.cs`.

A field or struct size applies to cache version `v` when

* `v >= MinVersion` (if a MinVersion is given) **and**
* `v < MaxVersion` (if a MaxVersion is given), **except** that a range whose MinVersion equals its
  MaxVersion matches exactly that one version.

So **MinVersion is inclusive, MaxVersion is exclusive.** `[Offset(16, MaxVersion = U9)]` means "versions
before U9", `[Offset(24, MinVersion = U9)]` means "U9 and later". `[FixedSize(n, MinVersion, MaxVersion)]`
follows the same rule for struct sizes.

Two ways Reclaimer declares layouts:

1. **Attributes** on properties (`[Offset]`, `[FixedSize]`, `[MinVersion]`, `[MaxVersion]`,
   `[VersionSpecific]`). `StructureDefinition.FromAttributes` (`Reclaimer.Core\IO\Dynamic\StructureDefinition.cs`)
   collects every version boundary mentioned, builds consecutive non-overlapping ranges, and tests each
   attribute against the range with the rule above. Overlapping attribute ranges of the same kind are an
   error, so there is never ambiguity.
2. **Definition builders** (`AddVersion(min, max)` in the `*.config.cs` files, e.g. `Blam\Halo3\Config\ScenarioTag.config.cs`).
   These ranges may overlap (`[Halo3Retail, ∞)`, `[MccHalo3F6, ∞)`, `[MccHalo3U13, ∞)` all contain U13).
   The `StructureDefinition` constructor sorts them: versioned ranges before the default range, exact
   versions first, then **highest MinVersion first**, bounded MaxVersion before unbounded, and
   `FindVersionDefinition` takes the first match. So for U13 the `[MccHalo3U13, ∞)` range wins over the
   older ranges, and an ODST range `[Halo3ODST, ∞)` does not match because U13 sorts before `Halo3ODST`.

Enum order that matters here (`Blam\Common\Enums.cs`, `CacheType`): `... Halo3Delta < Halo3Beta < Halo3Retail
< MccHalo3 < MccHalo3U4 < MccHalo3F6 < MccHalo3U6 < MccHalo3U9 < MccHalo3U12 < MccHalo3U13 < Halo3ODST <
MccHalo3ODST ...`. The U13 build strings are listed on `MccHalo3U13` with the `[BuildString]` attribute;
`Dec 21 2023 22:31:37` is annotated "[24-02-14] U14" but still resolves to `MccHalo3U13`.

Everything below is already resolved to U13.

---

## 2. Opening the cache (what `CacheFactory.ReadCacheFile` does)

Sources: `Blam\Common\CacheFactory.cs`, `Blam\Common\CacheMetadata.cs`, `Blam\MccHalo3\CacheFileU6.cs`,
`Blam\MccHalo3\CacheFile.cs` (shared `TagIndex`, `StringIndex`, `TagClass`, `IndexItem`),
`Blam\Common\BlamUtils.cs` (`FindResourceFile`).

1. `CacheArgs.FromFile` reads the build string (header 160 (0xA0), 32 chars) and matches it against the
   `[BuildString]` attributes to get `CacheType` and `CacheMetadata` (engine Halo3, platform PC, flag Mcc,
   resource codec **Deflate**: `CacheMetadataAttribute` defaults to Deflate for engines Halo 3 and later).
2. Engine Halo3 + Mcc + type >= `MccHalo3F6` selects `MccHalo3.CacheFileU6`.
3. `CacheFileU6` constructor:
   * `HeaderTranslator = SectionAddressTranslator(section 0)`, `MetadataTranslator = TagAddressTranslator`,
     `PointerExpander` (the `<< 2` expansion for 32-bit pointers inside tag data).
   * Reads `CacheHeaderU6` (`[FixedSize(16384)]`). U13-resolved header fields it uses:
     FileSize 8 (0x8, stored i32), TagDataAddress 16 (0x10), VirtualSize 20 (0x14), FileCount 32 (0x20),
     FileTablePointer 36 (0x24), FileTableSize 40 (0x28), FileTableIndexPointer 44 (0x2C), StringCount 48
     (0x30), StringTablePointer 52 (0x34), StringTableSize 56 (0x38), StringTableIndexPointer 60 (0x3C),
     StringNamespaceCount 64 (0x40), StringNamespaceTablePointer 68 (0x44), BuildString 160 (0xA0),
     ScenarioName 224 (0xE0, 256 chars), VirtualBaseAddress 736 (0x2E0, i64), IndexPointer 744 (0x2E8, i64),
     PartitionTable 768 (0x300), **SectionOffsetTable 1228 (0x4CC)** and **SectionTable 1244 (0x4DC)**
     (these two are the `[MinVersion = MccHalo3U6, MaxVersion = MccHalo3ODSTF3]` offsets).
   * Re-binds `IndexPointer` to the tag-data translator, seeks to it and reads `TagIndex`
     (`[FixedSize(72)]`: TagClassCount 0, TagClassDataPointer 8 (i64), TagCount 16, TagDataPointer 24 (i64)).
   * `TagIndex.ReadItems`: reads `TagClass[TagClassCount]` (16 bytes: ClassId i32, parent 4 chars,
     grandparent 4 chars, ClassName string id; the class code is the 4 bytes of ClassId reversed), then
     `IndexItem[TagCount]` (8 bytes: ClassIndex i16, Unknown i16, MetaPointer 32-bit shifted). Items with
     ClassIndex < 0 are dropped ("every Halo3 map has an empty tag"). Tag names: `int[TagCount]` at
     FileTableIndexPointer, each an offset into the table at FileTablePointer (-1 = no name). Global
     ("system") tags are found by class code: `scnr`, `matg`, `ugh!`, `play`, `zone`; the scenario is the
     `scnr` whose name equals the header ScenarioName (ambiguous otherwise).
   * `StringIndexU6`: `int[StringCount]` offsets at StringTableIndexPointer into StringTablePointer, plus a
     `StringIdTranslator` built from the header namespace table (the U13 bit layout 19/8/5).
   * `LocaleIndex(this, 24, 80, 12)` for Halo 3 >= U12 (not used by HaloExport).
   * Starts a background read of the `play`, `zone` and `scnr` tags.
4. HaloExport `Open()` then builds `tagById` = every index item keyed by `Id` (its position in the tag table).

Shared resource maps: `ResourceIdentifier.ReadData` takes the shared cache **file name** from the `play`
tag (`SharedCaches[page.CacheIndex].FileName`, e.g. `maps\shared.map`), keeps only the base name and opens
it from the same directory as the map (`Blam\Halo3\ResourceIdentifier.cs`). `BlamUtils.FindResourceFile`
(Steam manifest / workshop search) exists but is not on the Halo 3 path.

---

## 3. From scenario to triangles: the exact order

### 3.1 `scnr` (ScenarioTag), U13 offsets

Source: `Blam\Halo3\Config\ScenarioTag.config.cs`, builder range `AddVersion(MccHalo3U13, null)`;
block layouts in the same file. HaloExport reads `cache.TagIndex.GetGlobalTag("scnr").ReadMetadata<ScenarioTag>()`.

| property | offset | type | notes |
|---|---|---|---|
| StructureBsps | 24 (0x18) | TagBlock of 108 (0x6C)-byte `StructureBspBlock` | `BspReference` TagReference at +0 (class `sbsp`) |
| Skies | 52 (0x34) | TagBlock of 20 (0x14)-byte `SkyReferenceBlock` | `SkyReference` TagReference at +0 (class `scen`) |
| ObjectNames | 168 (0xA8) | TagBlock of 36 (0x24)-byte `ObjectNameBlock` | Name 32-char null-terminated at +0, ObjectType i16 +32, PlacementIndex i16 +34 |
| Scenery | 180 (0xB4) | TagBlock of 180 (0xB4)-byte `SceneryPlacementBlock` | placement layout below |
| SceneryPalette | 192 (0xC0) | TagBlock of 16 (0x10)-byte `ObjectPaletteBlock` | TagReference at +0. **16 bytes only from U13 on** (48 before; Reclaimer comment: "confirm which version this changed in") |
| Machines | 312 (0x138) | TagBlock of 136 (0x88)-byte `MachinePlacementBlock` | |
| MachinePalette | 324 (0x144) | 16-byte palette blocks | |
| Controls | 360 (0x168) | TagBlock of 120 (0x78)-byte `ControlPlacementBlock` | |
| ControlPalette | 372 (0x174) | 16-byte palette blocks | |
| Crates | 1468 (0x5BC) | TagBlock of 176 (0xB0)-byte `CratePlacementBlock` | |
| CratePalette | 1480 (0x5C8) | 16-byte palette blocks | |
| ScenarioLightmapReference | 1708 (0x6AC) | TagReference | class `sLdT` |

Placement block fields common to scenery / crate / machine / control (`PlacementBlockBase` builder):

| field | offset | type |
|---|---|---|
| PaletteIndex | 0 (0x0) | i16 |
| NameIndex | 2 (0x2) | i16 (into ObjectNames) |
| Position | 8 (0x8) | 3 x f32, world units |
| Rotation | 20 (0x14) | 3 x f32 **yaw, pitch, roll in radians** (`EulerAngles`) |
| Scale | 32 (0x20) | f32, **0 means 1** |
| VariantName | 84 (0x54) | string id (`ObjectPlacementBlockBase`, Halo3Retail layout) |

Verified on riverworld.map: StructureBsps 1, Skies 1, ObjectNames 1, Scenery 142, SceneryPalette 26,
Machines 0, Controls 0, Crates 102, CratePalette 43 (`docs/format/scripts/check_scnr_sbsp_counts.py`).

### 3.2 `sbsp` (ScenarioStructureBspTag), U13 offsets

Source: `Blam\Halo3\Config\ScenarioStructureBspTag.config.cs`, range `AddVersion(MccHalo3U4, null)`
(U13 is inside it; the `[Halo3ODST, ∞)` range does not match U13). Class file `Blam\Halo3\ScenarioStructureBspTag.cs`.

| property | offset | type |
|---|---|---|
| XBounds / YBounds / ZBounds | 64 (0x40) / 72 (0x48) / 80 (0x50) | RealBounds = (min f32, max f32) |
| Clusters | 196 (0xC4) | TagBlock of 280 (0x118)-byte `ClusterBlock` (MccHalo3+ size) |
| Shaders | 208 (0xD0) | TagBlock of 36 (0x24)-byte `ShaderBlock`: ShaderReference TagReference at +0 |
| GeometryInstances | 448 (0x1C0) | TagBlock of 120 (0x78)-byte `BspGeometryInstanceBlock` |
| ResourcePointer1 | 596 (0x254) | ResourceIdentifier (i32) |
| Sections | 756 (0x2F4) | TagBlock of 76 (0x4C)-byte `SectionBlock` (**not used for MCC geometry, see 3.3**) |
| BoundingBoxes | 768 (0x300) | TagBlock of 44 (0x2C)-byte `BspBoundingBoxBlock` |
| ResourcePointer2 / 3 | 876 (0x36C) / 908 (0x38C) | ResourceIdentifier |

`ClusterBlock` (280 bytes): XBounds 0, YBounds 8, ZBounds 16, **SectionIndex i16 at 216 (0xD8)**.
`BspGeometryInstanceBlock` (120 bytes): **TransformScale f32 at 0**, **Transform at 4 (0x4): 12 x f32 read
as a 4x3 matrix** (see 5.1), **SectionIndex i16 at 52 (0x34)**, **Name string id at 84 (0x54)**.
`BspBoundingBoxBlock` (44 bytes): XBounds 4, YBounds 12, ZBounds 20, UBounds 28, VBounds 36 (each min/max f32).

Important: the sbsp **bounds field is not the render-geometry box**. On riverworld the X and Y ranges equal
the cluster geometry exactly but Z is (-15.52, 1892.31) while the geometry spans Z (-9.29, 90.88) wu. On
cyberdyne X also differs. Use it only as an envelope.

### 3.3 Where the BSP's render geometry really is: the `Lbsp` tag

Source: `ScenarioStructureBspTag.GetModelContent`, `Blam\Halo3\ScenarioLightmapTag.cs`,
`Blam\Halo3\ScenarioLightmapBspDataTag.cs`.

For `CacheType >= MccHalo3U4` (so U13):

1. Read `scnr.ScenarioLightmapReference` -> `sLdT` tag. In it, `LightmapRefs` = TagBlock at **4 (0x4)** of
   16-byte TagReferences (class `Lbsp`). (Before U4 the data was embedded here as 436-byte blocks instead.)
2. Read each `Lbsp` as `ScenarioLightmapBspDataTag`: **BspIndex i16 at 2 (0x2)**, **Sections TagBlock at
   308 (0x134)** (76-byte `SectionBlock`s, same layout as render models), **ResourcePointer i32 at 428 (0x1AC)**.
   Pick the one whose BspIndex equals the index of this sbsp in `scnr.StructureBsps`. Fallback: any tag of
   class `Lbsp` with the same tag name as the sbsp.
3. The geometry arguments are: Shaders = **sbsp**.Shaders; Sections = **Lbsp**.Sections;
   ResourcePointer = **Lbsp**.ResourcePointer. The sbsp's own Sections block has the same count (102 on
   riverworld) but Reclaimer never reads it for MCC.

Verified on riverworld: `sLdT` = `levels\multi\riverworld\riverworld_faux_lightmap` (tag 6878), one
LightmapRef -> `Lbsp` tag 6879 `levels\multi\riverworld\riverworld`, BspIndex 0, 102 sections, resource
index 3290 (`docs/format/scripts/check_lbsp_resource.py`).

### 3.4 Section / submesh / subset layout (shared by `Lbsp` and `mode`)

Source: `Blam\Halo3\RenderModelTag.cs` (`SectionBlock`, `SubmeshBlock`, `SubsetBlock`, `MeshFlags`).
U13 is `>= Halo3Delta`, so the 76-byte layout applies.

`SectionBlock`, 76 (0x4C) bytes:

| field | offset | type | notes |
|---|---|---|---|
| Submeshes | 0 (0x0) | TagBlock of 16-byte `SubmeshBlock` | one glTF primitive piece each |
| Subsets | 12 (0xC) | TagBlock of 8-byte `SubsetBlock` | used only for render_model instances |
| VertexBufferIndex | 24 (0x18) | i16 | into the resource's vertex buffer list |
| IndexBufferIndex | 40 (0x28) | i16 | -1 = none |
| Flags | 44 (0x2C) | u8 | bit 0x10 `MeshIsUnindexed`; also 0x01 vertex colour, 0x02 region sort, 0x04 vertex buffers for indices, 0x08 per-instance lighting |
| NodeIndex | 45 (0x2D) | u8 | 255 = none |
| VertexFormat | 46 (0x2E) | u8 | selects the layout in `Reclaimer.Blam\Resources\MccHalo3VertexBuffer.xml` |
| IndexFormat | 48 (0x30) | u8 | Reclaimer `IndexFormat`: 0 Default, 3 TriangleList, 5 TriangleStrip (others unused here) |

A section is "unindexed" when IndexBufferIndex == -1 or Flags has 0x10; it then gets an implied index
buffer 0..VertexCount-1 with the section's IndexFormat (`BlamUtils.CreateImpliedIndexBuffer`).

`SubmeshBlock`, 16 bytes: ShaderIndex i16 0 (into sbsp/mode Shaders), IndexStart u16 4, IndexLength u16 6,
SubsetIndex u16 8, SubsetCount u16 10, Flags u8 13, VertexCount u16 14.
`SubsetBlock`, 8 bytes: IndexStart u16 0, IndexLength u16 2, SubmeshIndex u16 4, VertexCount u16 6.

Riverworld Lbsp: all 102 sections have IndexFormat 3 (TriangleList); VertexFormat 1 for the 76 instance
sections and 0 for the 26 cluster sections; 4 sections unindexed. Cyberdyne: 359 sections, all
TriangleList, VertexFormat 1 x 311 / 0 x 48, 8 unindexed.

### 3.5 Resource data: `zone`, `play`, fixups, deflate

Sources: `Blam\Halo3\Halo3Common.cs` (`GetMeshes`), `Blam\Halo3\ResourceIdentifier.cs` (`ReadData`),
`Blam\Halo3\Config\CacheFileResourceGestaltTag.config.cs`, `Blam\Halo3\CacheFileResourceLayoutTableTag.cs`,
`Blam\Common\ContentFactory.cs` (`GetResourceData`).

`zone` = cache_file_resource_gestalt, U13 (`AddVersion(MccHalo3, null)` range):

| property | offset | notes |
|---|---|---|
| ResourceEntries | 100 (0x64) | TagBlock of 64-byte `ResourceEntryBlock` (Halo3Retail+ layout) |
| FixupDataSize | 328 (0x148) | i32 |
| FixupDataPointer | 340 (0x154) | 32-bit shifted tag-data pointer to the fixup blob |

`ResourceEntryBlock`, 64 bytes: OwnerReference TagReference 0, ResourcePointer i32 16, FixupOffset 20,
FixupSize 24, LocationType i16 32, SegmentIndex i16 34, ResourceFixups TagBlock 40 (8-byte: Unknown 0,
Offset 4 masked with 0x0FFFFFFF), ResourceDefinitionFixups TagBlock 52 (8-byte).

`play` = cache_file_resource_layout_table, U13 (`[MccHalo3, Halo3ODST)` offsets): SharedCaches 12 (0xC)
(264-byte blocks, 32-char name at 0), Pages 24 (0x18) (88-byte: CacheIndex i16 4, DataOffset i32 8,
CompressedSize 12, DecompressedSize 16, DataChunkCount i16 84), SizeGroups 36 (0x24), **Segments 60 (0x3C)**
(16-byte: PrimaryPageIndex i16 0, SecondaryPageIndex i16 2, PrimaryPageOffset i32 4, SecondaryPageOffset
8, PrimarySizeIndex i16 12, SecondarySizeIndex i16 14). Special case: if the `play` index item's
MetaPointer is 0, Reclaimer reads the layout table at `zone` meta + 28.

A `ResourceIdentifier` is one i32 ("actually two shorts"); the resource index is its **low 16 bits**.

`ReadData(PageType.Auto)` for U13, in order:

1. entry = zone.ResourceEntries[resource index]; SegmentIndex < 0 -> "Data not found" (this is the error
   the README reports for `objects\multi\wall_m\wall_m`, whose data is in a map that is not installed).
2. segment = play.Segments[entry.SegmentIndex]. Use the secondary page if SecondaryPageIndex >= 0, else
   the primary; if the chosen page has DataOffset < 0 or CompressedSize == 0, fall back to the primary.
3. File: this map, or, when page.CacheIndex >= 0, the base name of play.SharedCaches[CacheIndex].FileName
   (`maps\english.map`, `maps\mainmenu.map`, `maps\shared.map` on both MP maps) in the map's directory.
4. Data table address (where page DataOffsets start): for Halo 3 >= U6: **16384 (0x4000) in a shared
   map; in the map itself the u32 at file offset 1232 (0x4D0)** = the second entry of the section offset
   table = sectionOffset[1] (0x114000 on riverworld, 0x111000 on cyberdyne; the raw section).
5. Seek dataTable + page.DataOffset and call `GetResourceData(codec = Deflate, maxLength, segmentOffset,
   CompressedSize, DecompressedSize)`: if CompressedSize == DecompressedSize copy bytes from segmentOffset;
   otherwise open a raw **DeflateStream** (no zlib header) over the page, discard `segmentOffset`
   decompressed bytes in <= 0x80000 steps, then read `min(maxLength, DecompressedSize - segmentOffset)`
   bytes. The page's DataChunkCount is ignored; the page is treated as a single deflate stream.

Riverworld Lbsp resource: entry 3290, FixupOffset 298232, FixupSize 19732, LocationType 1, SegmentIndex
3254, 1008 ResourceFixups; segment -> primary page 1420 (CacheIndex -1 = local), PrimaryPageOffset
25,733,816; page: DataOffset 0, CompressedSize 45,416,454, DecompressedSize 92,027,152, 1870 chunks.
So Reclaimer inflates about 25.7 MB of the page before reaching the BSP buffers.

`Halo3Common.GetMeshes` then:

1. Reads the **fixup blob** at zone.FixupDataPointer: at `entry.FixupOffset + entry.FixupSize - 24`:
   vertexBufferCount i32, skip 8, indexBufferCount i32. At `entry.FixupOffset`: `VertexBufferInfo[vbCount]`
   (28 (0x1C) bytes each: VertexCount i32 at 0, DataLength i32 at 8), then 12 x vbCount bytes skipped, then
   `IndexBufferInfo[ibCount]` (24 (0x18) bytes: IndexFormat i32 at 0, DataLength i32 at 4). More 12-byte
   structs follow and are not read. Riverworld: 394 vertex buffers (319,934 vertices, 11,892,976 bytes),
   109 index buffers (400,284 bytes).
2. Reads the whole resource with `ReadData(PageType.Auto)` into memory.
3. For each section (skip when its VertexBufferInfo.VertexCount == 0): vertex bytes at
   `entry.ResourceFixups[section.VertexBufferIndex].MaskedOffset`, length DataLength, decoded with the XML
   layout for `section.VertexFormat`; index bytes at `entry.ResourceFixups[vbCount * 2 +
   section.IndexBufferIndex].MaskedOffset`, element size **u16 unless VertexCount > 65535 (then i32)**,
   layout = IndexBufferInfo.IndexFormat. Buffers are cached by index so sections sharing one are read once.
   Unindexed sections get the implied buffer. x86 caches need no byte swap.
4. Materials: one per sbsp/mode `ShaderBlock` (null if the reference is null): Id = the shader tag's index
   item Id, Name = its file name, custom property `tag_name` = full tag path; texture mappings from
   `Gen3MaterialHelper` (see 6).
5. One `Mesh` per section, in section order (null when a buffer is missing): BoneIndex = NodeIndex unless
   255; `Segments` = one per Submesh with Material = materials[ShaderIndex], IndexStart, IndexLength.

### 3.6 Model assembly for the BSP: clusters vs instances

Source: `ScenarioStructureBspTag.GetModelContent`, `Blam\Common\BlamConstants.cs`, `Blam\Common\BlamUtils.cs`
(`GroupGeometryInstances`), `Reclaimer.Core\Geometry\Model.cs`.

* Region **`<Clusters>`** (`BlamConstants.SbspClustersGroupName`): one permutation per cluster, Name = the
  cluster index as 3 digits (`000`...), MeshRange = (cluster.SectionIndex, 1), `IsInstanced = false`, no
  transform.
* One region per **instance group**: `GroupGeometryInstances` sorts instances by name (leading symbol
  characters ignored, 1-2 digit numbers zero-padded to 3) and groups them by the longest underscore-joined
  word run in the name (e.g. `backet_bottom01` -> `backet_bottom`, `!base_int_thing00` -> `base_int_thing`,
  `?*bay_glass01` -> `bay_glass`), groups ordered by key. Each instance is a permutation: Name = instance
  name, `Transform` = the 4x3 matrix, `UniformScale` = TransformScale, MeshRange = (SectionIndex, 1),
  `IsInstanced = true`, custom properties `geometry_instance`, `instance_name`, `instance_group`.
* Meshes = `GetMeshes`. Then for i in 0..BoundingBoxes.Count: Meshes[i].PositionBounds / TextureBounds =
  BoundingBoxes[i] (X/Y/Z and U/V). Riverworld has 76 bounding boxes for sections 0..75 (the instance
  sections); the cluster sections 76..101 get none, so their positions are used as stored (identity
  expansion). Cyberdyne: 311 boxes, clusters are sections 311..358.

HaloExport (`Export()`) separates them with exactly this test: a permutation goes to the **World** mesh when
`region.Name == "<Clusters>"` and `!perm.IsInstanced`; every other permutation with a mesh becomes a
**bsp_instance** node with transform `perm.GetFinalTransform()` (unless `--no-instances`;
`--flatten-instances` bakes them into World instead). Permutations whose mesh is null or has no segments
are skipped: that is why riverworld shows 23 cluster meshes for 26 clusters (clusters 0, 1, 23 ->
sections 76, 77, 99 have no vertex data) and 261 placements for 262 instances (instance 71, section 34).
Cyberdyne: 44 of 48 clusters, 830 of 834 instances.

Instance meshes are shared: the glTF mesh cache key is `bsp{bi}/section{sectionIndex}`, so 261 riverworld
placements use 75 distinct meshes. Node name `<group>#bsp<N>-<nnnnn>` (global counter), `extras.iw4l.model`
= group name, `extras.halo` = {kind `bsp_instance`, bsp, section, instance_name}.

`GetFinalTransform()` (`Model.cs`, `ModelPermutation`): decompose `Transform` into scale, rotation,
translation, then return `S(Scale) * S(scale) * R(rotation) * T(translation)` (row-vector matrices, see 5.1).
For the sbsp instances this equals `S(TransformScale) * Transform` to float precision (verified on all 261
and 830 instance nodes, `docs/format/scripts/check_placement_matrix.py`).

### 3.7 Mesh to triangles (HaloExport `BuildMesh`)

Sources: `Program.cs` `BuildMesh`, `FixNormals`; `Reclaimer.Core\Geometry\GeometryExtensions.cs`
(`GetTriangleIndicies`, `Unstrip`, `GetPositions/GetTexCoords/GetNormals`), `RealBounds3D.CreateExpansionMatrix`.

For each (mesh, optional transform) part, and each segment with a non-null material:

1. `posT` = expansion matrix of `PositionBounds`: scale by (XLength, YLength, ZLength), translate by Min;
   identity when the bounds are empty. `texT` likewise for `TextureBounds` (2D). This turns normalised
   compressed vertex values into world units / raw UVs.
2. `tri` = the index buffer subset [IndexStart, IndexStart + IndexLength); if the buffer layout is Default
   or TriangleStrip it is unstripped (triangles with two equal indices dropped; winding alternates every
   other triangle). All MCC sections seen are TriangleList, so this is a straight copy. Fewer than 3
   indices -> skip.
3. Vertex window = [min(tri), max(tri)]; positions, UVs and normals are read for that window only.
4. Position: `v * posT`, then `* xf` for instances, then `ToG` (section 5.2). UV: `uv * texT` is
   TEXCOORD_1 (raw); TEXCOORD_0 = raw UV times the base-colour texture's tiling (x, y). Normal: transformed
   by `xf` as a normal, then `DirToG`, normalised; zero normals are left zero and filled by `FixNormals`
   (area-weighted face normals) afterwards.
5. Indices rebased to the primitive; degenerate triples skipped; **if det(xf) < 0 the second and third
   indices are swapped**.
6. One glTF primitive per glTF material (see 6), primitives ordered by material index; accessors:
   POSITION (with min/max), NORMAL, TEXCOORD_0, TEXCOORD_1 (float32), indices uint32, mode 4.

Statistics kept: triangles/vertices per kind, placed bounds per kind (mesh AABB corners transformed by the
node matrix), triangles per material, winding-agreement ratio (face normal vs vertex normal).

---

## 4. Scenario objects: palette -> object -> model -> render model

Sources: `Program.cs` `ExportObjects`, `ModelParts`; `Blam\Halo3\ObjectTag.cs`, `ModelTag.cs`,
`SceneryTag.cs`, `CrateTag.cs`, `DeviceMachineTag.cs`, `DeviceControlTag.cs`, `RenderModelTag.cs`.

Kinds and their scnr blocks: `scenery` (Scenery / SceneryPalette), `crate` (Crates / CratePalette),
`machine` (Machines / MachinePalette), `control` (Controls / ControlPalette). Vehicles, weapons, bipeds,
equipment, lights, effects and decals are **not** exported.

1. Placements are grouped by `PaletteIndex` (first-appearance order). `palette[PaletteIndex].TagReference`
   must be valid (tag id != -1), else the group is skipped.
2. The object tag (classes `scen`, `bloc`, `mach`, `ctrl` all use the `ObjectTag` layout): ObjectType i16
   at 0, DefaultVariant string id at 48 (0x30), **Model TagReference at 52 (0x34)** (class `hlmt`),
   CrateObject TagReference at 68 (0x44), CollisionDamage TagReference at 84 (0x54).
3. `ReadRenderModel()`: `hlmt` **RenderModel TagReference at 0** (class `mode`) -> `RenderModelTag`
   (`hlmt` also has CollisionModel at 16, Animation at 32, PhysicsModel at 48; variants are not read).
4. `RenderModelTag` fields (not versioned except where noted): Name 0, Flags 4, Regions 12 (16-byte
   `RegionBlock`: Name 0, Permutations TagBlock 4), InstancedGeometrySectionIndex i32 28, GeometryInstances
   32 (60-byte: Name 0, NodeIndex i32 4, TransformScale 8, Transform 4x3 at 12), Nodes 48 (96-byte),
   MarkerGroups 60, Shaders 72 (36-byte), Sections 104 (76-byte), BoundingBoxes 116 (56-byte: X 4, Y 12, Z
   20, U 28, V 36), NodeMaps 176 (12-byte: block of bytes), ResourcePointer 224. `PermutationBlock` is
   **24 bytes for >= U9** (16 before): Name 0, SectionIndex i16 4, SectionCount i16 6, InstanceMask1..4
   i32 at 8, 12, 16, 20.
5. `GetContent()`: bones from Nodes, markers, one `ModelRegion` per region with one permutation per
   permutation block (MeshRange = (SectionIndex, SectionCount)), meshes via the same `GetMeshes` (with
   NodeMaps for skinning), **all meshes get BoundingBoxes[0]** as compression bounds, then
   `CreateInstanceMeshes`: if InstancedGeometrySectionIndex >= 0 the single instance section is split into
   one mesh per GeometryInstance using `Subsets[i]` (index slice rebased, vertex slice [min, max]), placed
   in region **`<Instances>`** as instanced permutations with Transform / UniformScale, sorted by name.
6. HaloExport `ModelParts`: for every region take **the first non-instanced permutation only** (other
   permutations are damage states or variants and would overlap) plus **every instanced permutation at
   `GetFinalTransform()`**. The resulting mesh is cached per `<kind>/<tag path>`.
7. Placement transform (Halo units): `S(scale, 0 -> 1) * R(quaternion from Rotation) * T(Position)`.
   This is the same rule as Reclaimer's own `ObjectPlacement.SetTransform` in `ScenarioTag.GetContent`.
8. Backdrop rule: see 7. Node name `<tag file name>#<kind>-<nnnn>` with a counter per palette group that
   also counts dropped objects; `extras.iw4l.model` = tag file name; `extras.halo` = {kind, tag, palette_index}.
9. `--objects a,b` / `--no-objects` filter kinds. Failures (exception reading the tag or the resource) are
   counted and listed in `warnings` as `"<kind> <tag>: <message>"`.

Verified: all 142 riverworld scenery, 102 crates, all 132 / 131 / 8 cyberdyne scenery / crates / machines
rebuilt from the raw placement bytes match the glTF matrices to < 5e-5 m.

---

## 5. Matrix conventions, units and axes

### 5.1 Matrices

* Reclaimer and HaloExport use `System.Numerics.Matrix4x4` in the **row-vector** convention: a point is
  transformed as `v' = v * M`; the translation is the fourth row (M41, M42, M43); matrices compose
  left-to-right in application order (`A * B` applies A first).
* A Halo **4x3 matrix** in tag data is 12 floats: three rows of 3 (the transformed X, Y, Z axes), then
  the translation row. `CacheFactory.CreateReader` registers `IOExtensions.ReadMatrix3x4` for every cache
  type >= Halo2Xbox, which fills M11..M13, M21..M23, M31..M33, M41..M43 and sets M44 = 1 (the fourth column
  is 0, 0, 0, 1). Both `BspGeometryInstanceBlock.Transform` and the render-model `GeometryInstanceBlock.Transform`
  and `NodeBlock.InverseTransform` are read this way.
* Euler rotation of a placement: `EulerAngles(Yaw, Pitch, Roll)` (radians, stored in that order).
  `Reclaimer.Core\Geometry\Vectors\EulerAngles.cs` converts it to a quaternion as
  `Qaxis(+X, roll) * Qaxis(-Y, pitch) * Qaxis(+Z, yaw)` with the Hamilton product (System.Numerics
  `operator *`, left factor applied last). Applied to a vector that is: rotate about +Z by yaw first, then
  about **-Y** by pitch, then about +X by roll, all world axes. Reclaimer's own comment: "assuming DX
  coordinate system". `Matrix4x4.CreateFromQuaternion` then gives the row-vector rotation matrix.
* A placement with Scale 0 is treated as scale 1 (HaloExport and `ObjectPlacement.SetTransform` agree).

### 5.2 Units and axes

* `BlamConstants.WorldUnitScale = 10 * StandardUnits.Feet` = **3.048 m per world unit** (`WU_M` in Program.cs;
  `WU_FT = 10`). The README's proof is the campaign Master Chief render model: 0.703 wu tall = 7.03 ft.
  On multiplayer maps the exporter's `unit_probe` finds `objects\characters\masterchief\mp_masterchief\fp\fp`
  (first-person arms, 0.0901 wu) instead, so the MP sidecar probe value is meaningless; ignore it.
* Axis map: Halo is right-handed, +Z up. glTF is right-handed, +Y up.
  **glTF (x, y, z) = 3.048 * Halo (x, z, -y)**; direction vectors (x, z, -y) without the scale. This is a
  rotation, no mirroring, so winding is preserved. Inverse: Halo (x, y, z) = (gx, -gz, gy) / 3.048.
* Node matrices: `G = C^-1 * M * C` with `C` the row-vector matrix with M11 = 3.048, M23 = -3.048,
  M32 = 3.048, M44 = 1 (so `v * C` = the axis map above). The 16 numbers of G are written in M11, M12, ...,
  M44 order; because glTF stores column-major column-vector matrices, those are the same 16 numbers.
  Equivalently the translation of G is `ToG(translation of M)` and the 3x3 part is `C3^-1 * R * C3`.
* The World node has no matrix; everything in it is already in metres.

---

## 6. Materials: classification, naming, textures

Sources: `Program.cs` `Analyse`, `MaterialIndex`, `ImageIndex`, `ChannelWeights`, `CheckImage`;
`Blam\Halo3\Halo3Common.cs` `GetMaterials`; `Blam\Common\Gen3\Gen3MaterialHelper.cs`; `Blam\Halo3\ShaderTag.cs`,
`RenderMethodTemplateTag.cs`, `RenderMethodDefinitionTag.cs`.

### 6.1 Shader tag reads (U13)

* Shader tag (`rmsh`, `rmtr`, `rmd `, `rmfl`, `rmw `, `rmhg`, `rmcs`, `rmtd`, `rmct`, ...; all read with
  `ShaderTag`): RenderMethodDefinitionReference (class `rmdf`) at 0; ShaderOptions TagBlock at 16 (0x10)
  of 2-byte blocks (OptionIndex i16); ShaderProperties TagBlock at 40 (0x28) of 132 (0x84)-byte blocks.
* `ShaderPropertiesBlock` (132 bytes): TemplateReference (`rmt2`) at 0; ShaderMaps TagBlock at 16 (0x10);
  TilingData TagBlock at 28 (0x1C) of 16-byte RealVector4 (x scale, y scale, x offset, y offset).
* `ShaderMapBlock` is **28 (0x1C) bytes for U13** (`[FixedSize(28, MinVersion = MccHalo3U4, MaxVersion =
  Halo3ODST)]`; 24 before U4 and for ODST): BitmapReference at 0, TilingIndex u8 at 22 (0x16).
* `rmt2`: Arguments (string ids) TagBlock at 72 (0x48); Usages (string ids) TagBlock at 108 (0x6C).
* `rmdf`: Categories TagBlock at 16 (0x10) of 24-byte blocks (Name string id 0, Options TagBlock 4 of
  28-byte blocks with Name at 0).

Option names: zip `rmdf.Categories` with `shader.ShaderOptions`; option i is
`Categories[i].Options[ShaderOptions[i].OptionIndex].Name` (skipped if out of range). HaloExport keeps the
whole map (`extras.halo.options`) and uses `blend_mode` (default `opaque`) and `alpha_test` (default `none`).

Texture i of the template = `Usages[i]`; its bitmap = `ShaderMaps[i].BitmapReference`; its xform =
`TilingData[Arguments.IndexOf(Usages[i])]` (default 1, 1, 0, 0 when the usage is not an argument).
Non-texture arguments (not in Usages) are float/colour parameters, kept raw in `extras.halo.shader_args`.
`Gen3MaterialHelper` parses a usage like `base_map_m_2` as usage `base_map` with blend channel bit `1 << 2`
(terrain layers), maps the usage name to a `TextureUsage` (Diffuse, BlendMap, DiffuseDetail, Normal,
Specular, ColorChange, ...), keeps only the xform's x/y scale as `Tiling`, and adds synthetic mappings:
Specular (alpha channel of the diffuse) when `specular_mask == specular_mask_from_diffuse` or a terrain
`material_N == diffuse_plus_specular`, and Transparency (alpha of the albedo) when the material is not
opaque. `Material.AlphaMode` from `blend_mode` (additive -> Add, multiply -> Multiply, alpha_blend -> Blend,
pre_multiplied_alpha -> PreMultiplied, else Opaque) and `alpha_test != none` -> Clip.

### 6.2 glTF material identity and name

`Material.Id` = the shader tag's index-item Id, so `MaterialIndex` maps shader tag -> glTF material 1:1
(with a fallback lookup by `tag_name` + class if the item's name differs). Name = shader tag file name,
lower case; duplicates get `__2`, `__3`, ... in first-seen order (riverworld: `forerunner_grill__2` is
`objects\levels\multi\riverworld\shaders\forerunner_grill`; `human_military_decals__2` is the `rmsh` that
shares the basename of the `rmd` decal; `invisible__2` is `objects\multi\shaders\invisible` after
`fx\null_object\shaders\invisible`).

### 6.3 Classification table (`Analyse`)

| shader class | rule | category |
|---|---|---|
| `rmw` | always | `water` |
| `rmtr` | always | `terrain` |
| `rmfl` | always | `foliage` |
| `rmd` | blend_mode multiply / double_multiply | `decal_multiply` |
| `rmd` | blend_mode additive / add_src_times_dstalpha / add_src_times_srcalpha / multiply_add / maximum | `decal_additive` |
| `rmd` | blend_mode opaque | `decal_opaque` |
| `rmd` | any other blend_mode (alpha_blend, pre_multiplied_alpha, absent) | `decal_blend` |
| `rmsh`, `rmcs`, `rmhg`, `rmtd` | blend_mode additive / add_src_times_dstalpha / add_src_times_srcalpha / maximum / multiply_add | `additive` |
| same | blend_mode multiply / double_multiply | `multiply` |
| same | blend_mode alpha_blend / pre_multiplied_alpha | `alpha_blend` |
| same | otherwise, alpha_test != none | `alpha_test` |
| same | otherwise | `opaque` |
| anything else (`rmct` Cortana, `rmzo` z-only, `rmbk` black, `rmlv`, beams, contrails, ...) | always | `special` |
| any class | lower-case tag path has a path segment matching `(invisible|sky_portal|skyportal|portal|z_only|zonly|black)` followed by `\`, `_` or end | overrides to `special` |

Derived outputs:

| category | `extras.iw4l.technique_set` | glTF `alphaMode` | cull / `doubleSided` | PNG keeps alpha |
|---|---|---|---|---|
| opaque, terrain, decal_opaque | `h3_r0_<cat>_<class>` | OPAQUE | `Some(Back)` / false | no (`_rgb.png`) except decal_* |
| alpha_test, foliage | `h3_t0_<cat>_<class>` | MASK | foliage: `None` / true | yes |
| alpha_blend, decal_blend | `h3_b0_<cat>_<class>` | BLEND | decal: `None` / true | yes |
| multiply, decal_multiply | `h3_multiply_<cat>_<class>` | OPAQUE | decal: `None` / true | yes |
| additive, decal_additive, water, special | `h3_ua_<cat>_<class>` | OPAQUE | decal_additive: `None` | additive yes; water/special no |

(`<class>` = the class code with non-word characters removed, e.g. `rmd`, `rmsh`.) The sidecar's
`materials_by_category.technique_set` is just the first two underscore parts (`h3_r0`, `h3_t0`, ...).
`pbrMetallicRoughness` is metallic 0, roughness 1. `extras.iw4l.color_transform = "square"` and
`extras.iw4l.source_name` = tag path are pipeline hints.

### 6.4 Texture handling

* Base colour: for `terrain`, the Diffuse mapping whose blend channel (R, G, B, A = layers 0..3) has the
  largest mean value in the blend map (`ChannelWeights` decodes the blend bitmap to BGRA32 and averages each
  channel; without a blend map the red layer wins). Otherwise the first Diffuse with no blend channel, else
  the first Diffuse, else the first ColorChange mapping. Its tiling is baked into TEXCOORD_0.
* Which bitmaps are written (default): the base map; every Diffuse and BlendMap usage; terrain
  DiffuseDetail maps (for the colour bake). **Detail, normal, specular, bump and the other maps are skipped
  unless `--all-textures`.** Transparency and specular-from-alpha mappings are never written (they are the
  diffuse bitmap again). `--no-textures` writes nothing but keeps the file names.
* Image: `bitmap.ToDds(0)` = submap 0 only, saved as PNG; RGBA (`DecompressOptions.Default`) when the
  material keeps alpha (6.3) or the mapping is a blend map, else BGR24 with the `_rgb` suffix, because Halo
  stores specular masks in alpha and the pipeline would read them as holes. File name
  `textures/<sanitised bitmap base name>-<bitmap tag id & 0xFFFF, 5 digits>[_rgb].png`; sanitising replaces
  anything outside `A-Za-z0-9_.-` with `_`. Images are deduplicated by (bitmap tag id, alpha flag).
* After writing, `CheckImage` reads the PNG back and adds a `warnings` entry when it is missing/empty or
  all black (max RGB < 2); `shaders\default_bitmaps\*` and `rasterizer\*` are marked "expected".
  Cyberdyne has one: `plasma_sm_red_mask-00175.png` is all black (it is an alpha mask).
* Sidecar per texture (`extras.halo.textures[]`): usage, blend_channel, tiling, bitmap tag, image uri,
  is_base_color, and `parameter` + `xform` matched positionally against the raw template usages that have a
  bitmap (Reclaimer adds its mappings in template order before the synthetic ones).
* `extras.halo.shader_maps` lists every template usage (also those without a bitmap) with xform;
  `terrain_blend_weights` = the four channel means. `textures.materials_without_base_color` lists materials
  with no base image.

---

## 7. Sky and backdrop rules, foliage renaming

* **Sky**: `scnr.Skies` entries are `scen` tags. Every valid one is listed in the sidecar `skies` (tag
  paths). Geometry is exported **only with `--sky`**: `SceneryTag.ReadRenderModel()` (same `hlmt` -> `mode`
  chain as objects) -> `ModelParts` -> one node `sky#<file name>` with no matrix (identity), kind `sky`,
  `extras.iw4l.model = "sky_<file name>"`. Reason it is off by default: riverworld's sky is 8,462 triangles
  spanning +-23,037 m (151,160 ft); Outskirts' dome is about 178,000 ft across. Failures go to `warnings`
  as `"sky <tag>: <message>"`. Reclaimer's own `ScenarioTag.GetContent` puts skies in a `skies` group the
  same way.
* **Backdrop**: after computing an object's placed bounds (mesh AABB corners through the glTF matrix), if
  the largest side exceeds `--max-object-m` (default **300 m**) and `--keep-backdrop` is not set, the object
  is **not placed**; it is recorded in `backdrop_excluded` as {kind, tag, size_m, centre_m}. On Outskirts
  that removes the Ark (11.6 km), the Forerunner ship, Covenant cruisers and `bsp_sky_blocker_giant`. Both
  multiplayer maps have an empty list. BSP instances are never subject to this rule.
* **Foliage rename**: when more than 30 % of a node mesh's triangles use `foliage` materials, the node's
  `extras.iw4l.model` gets a `foliage_` prefix and `extras.halo.foliage_share` is written. Riverworld: 28
  nodes (`foliage_tree_pine_small` etc.), cyberdyne: 14. `pipeline_hints.foliage_models_renamed` counts them.

---

## 8. The sidecar (`halo_export.json`)

Top level: `map` (full path), `scenario` (tag path), `cache_type` (`MccHalo3U13`), `build`, `units`
{gltf, halo_world_unit_m 3.048, halo_world_unit_ft 10, source, unit_probe}, `axes` {halo, gltf, map,
winding_agrees_with_normals}, `bsps[]` {index, tag, clusters, instances, bounds_wu {x, y, z} (the sbsp
bounds fields), cluster_meshes, instance_placements, error?}, `skies[]`, `sky_exported`, `counts` {nodes,
triangles, vertices per kind; triangles_total, vertices_total, unique_meshes, materials}, `bounds` per kind
and `all` {min_m, max_m, size_ft}, `materials_by_category`, `materials[]` {name, class, category,
technique_set, triangles, tag, base_color} sorted by triangles descending, `textures` {written, missing,
missing_list, materials_without_base_color}, `pipeline_hints` {terrain_regex, ground_regex, note,
foliage_models_renamed}, `backdrop_excluded[]`, `collision_bsp` (always the string "not exported (render
geometry only); see README gaps"), `warnings[]` (distinct).

`pipeline_hints` regexes are anchored alternations of World material names: terrain = `rmtr` materials plus
opaque ones whose name has a natural word (cliff, rock, dirt, grass, mud, road, sand, snow, gravel, ground,
silt, terrain, boulder) and no man-made word (innard, clamp, panel, trim, edge, pipe, metal, girder, wall,
floor); ground = the `rmtr` ones plus opaque dirt/grass/mud/sand/snow/gravel/silt, minus cave/rock/cliff/boulder.

---

## 9. Caveats for the Rust reader

* The structure BSP render geometry comes from the **`Lbsp` tag's** Sections and resource (3.3), not from
  the sbsp. The sbsp supplies clusters, instances, shaders and the instance-section bounding boxes.
* The sbsp bounds fields are an envelope, not the geometry AABB (3.2).
* Resource pages are deflate-compressed as one stream per page and resources are sub-ranges of a page
  (3.5); the whole page prefix must be inflated (or an index of chunk boundaries built) to reach a resource.
* Collision: the README states MCC Halo 3 keeps the collision BSP in `structure_bsp_tag_resources` and
  Reclaimer has no definition for it; HaloExport exports render geometry only (`collision_bsp` string).
* The unit probe is meaningless on multiplayer maps (5.2).
* `Ext.ToBlockless` in Program.cs is an identity helper; `K.Clusters` duplicates the internal Reclaimer
  constant `"<Clusters>"` because `BlamConstants` is internal.
* Object placement uses only the first permutation per region and the default device pose; variants
  (`VariantName`) and device animation states are ignored.
* Licensing: this file describes behaviour and layouts; porting Reclaimer's code itself would make the
  port GPL-3.0 (see CLAUDE.md).
