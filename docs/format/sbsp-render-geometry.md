# scenario_structure_bsp (sbsp) render geometry, MCC Halo 3 U13

Decode spec for turning a `scenario_structure_bsp` tag of an MCC Halo 3 `.map`
(build `Dec 21 2023 22:31:37`, Reclaimer `CacheType.MccHalo3U13`) into a list of
`(material, world-space triangles)`. Written 2026-10-27 from Reclaimer commit `36224d7`
(<https://github.com/Gravemind2401/Reclaimer>, GPL-3.0; **format facts only, no code copied**) and
verified on `riverworld.map` (Valhalla) with `scripts\probe_sbsp_geometry.py` (read-only,
run with `python -I`). Every offset is given as hex (decimal). "Section" and "mesh" mean
the same thing here (Reclaimer: `SectionBlock`, Halo: `mesh`).

Container facts (header, tag index, pointer expansion, string ids) are taken as given from
the container document and are not re-derived here. Reminders that matter below:

- 32-bit pointers inside tag data are stored `>> 2`: `virtual = p << 2`, then
  `file = virtual - 0x1CC380000 + 0x05753000` on riverworld.
- TagBlock = 12 bytes `{i32 count, i32 shifted ptr, 4 pad}`.
- TagReference = 16 bytes `{4CC class, 8 skip, i32 tag id}`; tag index = low 16 bits, `-1` = null.
- StringId = i32; U13 layout indexBits 19, namespaceBits 8, lengthBits 5.
- Units: Halo world units (wu), Z up. 1 wu = 10 ft = 3.048 m (Reclaimer `BlamConstants.WorldUnitScale`).

---

## 0. Reclaimer's version-selection rule (confirmed)

Source: `Reclaimer.Core\IO\Dynamic\Extensions.cs`, `ValidateVersion(version, min, max)`:

> matches when `(min is null OR version >= min) AND (max is null OR version < max OR (version == max AND max == min))`

So **MinVersion is inclusive, MaxVersion is exclusive**, except an exact single version
(`min == max`) matches on equality. The same function serves both the attribute form
`[Offset(x, MinVersion=A, MaxVersion=B)]` / `[FixedSize(n, MinVersion, MaxVersion)]` and the
builder form `AddVersion(min, max)` used in the `*.config.cs` files (`DefinitionBuilder.cs`
converts enums to their integer ordinal). When several versions match, `StructureDefinition.cs`
picks: ranges before the unversioned default; exact versions first; then the **highest
MinVersion** first.

CacheType ordinals that matter (`Reclaimer.Blam\Blam\Common\Enums.cs`, counted from
`Halo1Xbox = 0`): `Halo3Delta = 13`, `Halo3Beta = 14`, `Halo3Retail = 15`, `MccHalo3 = 16`,
`MccHalo3U4 = 17`, `MccHalo3F6 = 18`, `MccHalo3U6 = 19`, `MccHalo3U9 = 20`, `MccHalo3U12 = 21`,
**`MccHalo3U13 = 22`**, `Halo3ODST = 23`, `MccHalo3ODST = 24`. Consequences for U13:

- `AddVersion(MccHalo3U4, null)` matches (22 >= 17). `AddVersion(Halo3ODST, null)` does **not** (22 < 23).
- `[FixedSize(28, MinVersion = MccHalo3U4, MaxVersion = Halo3ODST)]` matches (17 <= 22 < 23).
- `[MaxVersion(MccHalo3U4)]` does not match (22 >= 17).

Everything below is already resolved to the U13 value only.

---

## 1. The tag chain

| step | tag | how to find it | source |
|---|---|---|---|
| 1 | `scnr` scenario | the tag whose class is `scnr` and whose name equals the header scenario path (header `0xE0`) | `MccHalo3\CacheFile.cs` `TagIndex.ReadItems` |
| 2 | `sbsp` structure BSPs | `scnr + 0x18 (24)`: TagBlock `StructureBsps`, entries **108 (0x6C) bytes**, `BspReference` TagReference at `+0` | `Halo3\Config\ScenarioTag.config.cs` (`AddVersion(MccHalo3U13, null)`), `StructureBspBlock` `AddVersion(Halo3Retail, null).HasFixedSize(108)` |
| 3 | `sLdT` scenario lightmap | `scnr + 0x6AC (1708)`: TagReference `ScenarioLightmapReference` | same file |
| 4 | `Lbsp` lightmap BSP data | `sLdT + 0x4 (4)`: TagBlock `LightmapRefs` of TagReferences (16 bytes each) | `Halo3\ScenarioLightmapTag.cs` (`[MinVersion(MccHalo3U4)]`) |
| 5 | pick the `Lbsp` | the one whose `BspIndex` (`Lbsp + 0x2 (2)`, i16) equals the index of the sbsp inside `StructureBsps`; fallback: any tag of class `Lbsp` with the same tag name as the sbsp | `Halo3\ScenarioStructureBspTag.cs` `GetModelContent` |
| 6 | `zone` resource gestalt | the global tag of class `zone` (one per map) | `Halo3\CacheFileResourceGestaltTag.cs` |
| 7 | `play` resource layout table | the global tag of class `play`; if its meta pointer is 0, the table starts at `zone meta + 0x1C (28)` | `MccHalo3\CacheFile.cs` `IndexItem.ReadMetadata` |

### Why the Lbsp, not the sbsp, supplies the geometry

Reclaimer builds the sbsp model from `lightmapData.Sections` and `lightmapData.ResourcePointer`
(the Lbsp's), while the sbsp tag itself only contributes clusters, instances, shaders and the
compression bounds (`ScenarioStructureBspTag.cs`, `Halo3GeometryArgs`). Verified on riverworld:

- `sbsp + 0x2F4 (756)` Sections and `Lbsp + 0x134 (308)` Sections are **the same block**: same count (102) and the same shifted pointer (file `0x824ADC8`). Likewise `sbsp + 0x300 (768)` BoundingBoxes and `Lbsp + 0x140 (320)` are the same block (76 entries, file `0x824CC10`).
- The sbsp's own render-geometry resource (`sbsp + 0x36C (876)` = `0xFDEA1C87`, resource index 7303) is an **empty entry** in `zone`: fixup size 0, segment -1, no owner. The Lbsp's resource (`Lbsp + 0x1AC (428)` = `0xEE3D0CDA`, index 3290) is the live one: fixup size 19732, segment 3254.

So on MCC the lightmapper's output tag owns the final vertex/index data; the sbsp's copy of the
render-geometry struct is a shell that points at the same section blocks but has no resource.
(Unsure whether this holds for every map; campaign maps should be checked the same way.)

---

## 2. `scenario_structure_bsp` tag, U13 offsets

Source: `Halo3\Config\ScenarioStructureBspTag.config.cs`, builder `AddVersion(MccHalo3U4, null)`.

| offset | size | type | name | notes |
|---|---|---|---|---|
| `0x40` (64) | 8 | 2 x f32 | `XBounds` (min, max) | world bounds, wu |
| `0x48` (72) | 8 | 2 x f32 | `YBounds` | |
| `0x50` (80) | 8 | 2 x f32 | `ZBounds` | riverworld Z max is 1892.3, far above the playable map, probably sky geometry; unsure |
| `0xC4` (196) | 12 | TagBlock | `Clusters` | entry 280 bytes, see 2.1 |
| `0xD0` (208) | 12 | TagBlock | `Shaders` (materials) | entry 36 bytes, see 2.4 |
| `0x1C0` (448) | 12 | TagBlock | `GeometryInstances` | entry 120 bytes, see 2.2 |
| `0x254` (596) | 4 | i32 | `ResourcePointer1` | resource id; on riverworld index 7302, owner = this sbsp, fixup size 488. Not render geometry (collision/structure resource? HANDOFF's open item). Not used here |
| `0x2F4` (756) | 12 | TagBlock | `Sections` | entry 76 bytes, see 3. Same data as the Lbsp's on riverworld |
| `0x300` (768) | 12 | TagBlock | `BoundingBoxes` (compression info) | entry 44 bytes, see 2.3 |
| `0x36C` (876) | 4 | i32 | `ResourcePointer2` | render-geometry resource of the sbsp itself; **empty on MCC** (see 1) |
| `0x38C` (908) | 4 | i32 | `ResourcePointer3` | resource index 7426, owner = this sbsp, fixup size 19456; purpose unknown (another structure resource). Not used here |

Offsets `0x2F4`, `0x300`, `0x36C` are 12 bytes apart then 108 bytes: the familiar
`render_geometry` struct layout (meshes block, compression-info block, ..., resource at +120 from
the meshes block). The Lbsp has the same struct at `0x134`: meshes `0x134`, compression `0x140`,
resource `0x1AC` = `0x134 + 120`.

### 2.1 ClusterBlock, 280 (0x118) bytes

`ClusterBlock` builder `AddVersion(MccHalo3, null).HasFixedSize(280)`.

| offset | type | name |
|---|---|---|
| `0x0` (0) | 2 x f32 | `XBounds` |
| `0x8` (8) | 2 x f32 | `YBounds` |
| `0x10` (16) | 2 x f32 | `ZBounds` |
| `0xD8` (216) | i16 | `SectionIndex` (mesh index into Sections) |

Everything else in the 280 bytes is not read by Reclaimer (portals, visibility, decorators...).
Verified: riverworld clusters 0..25 use sections 76..101 in order; cluster 2's bounds equal the
min/max of its decoded vertices exactly. Clusters 0 and 1 have bounds of `+FLT_MAX` in all six
floats and point at sections 76 and 77, which have no vertex buffer (see 7).

### 2.2 BspGeometryInstanceBlock, 120 (0x78) bytes

`BspGeometryInstanceBlock` default version `.HasFixedSize(120)`. Reclaimer reads only the four
named fields (`TransformScale`, `Transform`, `SectionIndex`, `Name`). The other rows take their
names from TagTool `InstancedGeometryInstance` (`Size = 0x78, MaxVersion = Halo3ODST`) and Assembly
`Halo3MCC/sbsp.xml`, with the values seen on riverworld's 262 instances; marked **S** = named by
those tools, not verified against game behaviour. (corrected: merged with collision-bsp-research.md
10.2, which used to carry a second, differently named field map for the same 120 bytes.)

| offset | type | name | notes |
|---|---|---|---|
| `0x0` (0) | f32 | `TransformScale` | uniform scale, 1.0 on all three riverworld samples (0.99999988) |
| `0x4` (4) | 12 x f32 | `Transform` 3x4 | rows 1..3 = rotation rows (the images of local X, Y, Z), row 4 = translation; read as `ReadMatrix3x4` (`Reclaimer.Core\IO\IOExtensions.cs`): M11 M12 M13, M21 M22 M23, M31 M32 M33, M41 M42 M43, with column 4 = (0,0,0,1) |
| `0x28` (40) | 3 x f32 | translation (part of the above) | |
| `0x34` (52) | i16 | `SectionIndex` (TagTool: instance definition index) | mesh index into Sections (0..75 on riverworld; 76 distinct); -1 would mean none. Valid as a mesh index because mesh index == definition index on riverworld (collision-bsp-research.md 10.2) |
| `0x36` (54) | u16 | flags: bit 0 not in lightprobes, 1 render only, 2 does not block AOE damage, 3 collidable, 4 decal spacing, 5 not for render (Assembly; TagTool adds 6 "not for render") | riverworld: 8 (collidable) x248, 0 x12, 2 (render only) x2. Not read by Reclaimer. **S** |
| `0x38` (56) | i16 | lightmap texcoord block index (TagTool) | -1 on 77 instances, otherwise a distinct value 0..184 per instance. Equals SectionIndex only for the first three (0, 1, 2), which is why it used to look like a definition index. **S** |
| `0x3A` (58) | i16 | unknown (padding in TagTool) | 0 on all 262 |
| `0x3C` (60) | i32 | runtime (TagTool `SeamBitVector[1]`; Assembly two runtime i16s) | 0 on all 262 |
| `0x40` (64) | 3 x f32 | world bounding sphere centre (TagTool/Assembly) | instance 0 centre (81.6, -149.7, 10.71) vs translation (81.36, -148.50, 9.85); decoded vertex 0 in world space lies 6.9 wu from the centre, inside the radius. Not read by Reclaimer. **S** |
| `0x4C` (76) | f32 | world bounding sphere radius | 7.119, 8.885, 2.663 on samples; consistent with the mesh bounds (instance 0 local Z spans -5.94..7.66). TagTool reads an 8-byte radius *bounds* at `0x4C` instead, Assembly a single f32 then an i32 checksum at `0x50`: conflict, irrelevant for geometry. **?** |
| `0x50` (80) | 4 | checksum (Assembly) / second half of TagTool's radius bounds | wildly varying values (45159.6, 4.0, -24.3 as float); a checksum fits |
| `0x54` (84) | StringId | `Name` | e.g. `!basespiresmall07`, `!basespire08`, `!door_frame03`, `vent04` |
| `0x58` (88) | enum16 | pathfinding policy: 0 cut-out, 1 static, 2 none (TagTool/Assembly) | 0 on all 262. **S** |
| `0x5A` (90) | enum16 | lightmapping policy (TagTool/Assembly) | riverworld: 3 x185, 1 x59, 2 x18. **S** |
| `0x5C` (92) | f32 | lightmap resolution scale (TagTool/Assembly) | 1.0 on all 262. **S** |
| `0x60` (96) | TagBlock (12) | `BspPhysics` of `InstancedGeometryPhysics`, element **0xB0 (176)** on MCC (TagTool/Assembly; Havok shape wrapper, details in collision-bsp-research.md 10.2) | count (`0x60`) 1 on 248 instances, 0 on 14; shifted pointer at `0x64` (instance 0 -> file `0x5AE8980`), 4 pad at `0x68`. Consecutive element pointers are 176 bytes apart (44 in the `>>2` shifted form, which is where the old "44 bytes apart" came from), confirming the 176-byte element. **S** |
| `0x6C` (108) | 2 x u16 | fade pixels start, end (TagTool/Assembly) | 0 on all 262. **S** |
| `0x70` (112) | i16, i16, f32 | cubemap 0, cubemap 1, blend factor (TagTool/Assembly) | 0 on all 262. **S** |

There is **no separate "instanced geometry definitions" block read by Reclaimer**: the
instance's `SectionIndex` is used directly as the mesh index, and the mesh's compression bounds
are `BoundingBoxes[SectionIndex]`. (Halo 3's sbsp does have an instanced-geometry-definitions
block; Reclaimer bypasses it. A scan of the sbsp meta for a TagBlock with count 76 found only the
BoundingBoxes block at `0x300`, so if the definitions block exists in U13 its count is not 76 or it
is not a plain TagBlock in the first 1100 bytes. Open.)

### 2.3 BspBoundingBoxBlock (compression info), 44 (0x2C) bytes

`BspBoundingBoxBlock` default version `.HasFixedSize(44)`.

| offset | type | name | notes |
|---|---|---|---|
| `0x0` (0) | u32 | flags (not read by Reclaimer) | 3 on 75 of the 76 riverworld entries = bit0 compressed position, bit1 compressed texcoord (Halo compression_flags; unsure of the names but consistent with the formats). **Entry 34 has flags 0 and all ten bounds 0.0**: it belongs to section 34, the instance section with `VertexBufferIndex -1` that section 9 skips. Never assert `flags == 3`; read the entry as-is and let the empty section be skipped (corrected: used to say 3 on every entry) |
| `0x4` (4) | 2 x f32 | `XBounds` min, max | position decompression |
| `0xC` (12) | 2 x f32 | `YBounds` | |
| `0x14` (20) | 2 x f32 | `ZBounds` | |
| `0x1C` (28) | 2 x f32 | `UBounds` | texcoord decompression |
| `0x24` (36) | 2 x f32 | `VBounds` | |

**Which entry applies to which section:** `BoundingBoxes[i]` applies to `Sections[i]`
(`ScenarioStructureBspTag.cs`: loop over `BoundingBoxes.Count`, assigns `Meshes[i]`). On
riverworld there are only 76 entries for 102 sections: exactly the 76 instance sections
(0..75, vertex format `s_rigid_vertex`, compressed). The 26 cluster sections (76..101) have no
entry; they use `s_world_vertex` with float positions and need no decompression. A section with
no bounds entry must be treated as uncompressed (Reclaimer: `IsCompressed == false`, identity
expansion matrix). The render_model tag does it differently (one `BoundingBoxes[0]` for the whole
model, `RenderModelTag.cs`); do not confuse the two.

### 2.4 ShaderBlock (materials), 36 (0x24) bytes

`RenderModelTag.cs` `[FixedSize(36)] ShaderBlock`: TagReference `ShaderReference` at `0x0`.
The referenced tag is a render method (`rmsh` shader, `rmw ` water, `rmtr` terrain, `rmfl`
foliage, `rmd ` decal, ...). Submesh `ShaderIndex` indexes this block. Material identity for our
purposes = the referenced tag (name/id). riverworld: 64 shaders; `[0]` =
`levels\multi\riverworld\shaders\forerunner\cust_base_floor`, `[19]` = `riverworld_water_rough`
(class `rmw `), `[40]` = `forerunner\panel_generic_simple`.

For texture lookup later (`Halo3\ShaderTag.cs`, U13 values): `ShaderTag + 0x0`
TagReference `RenderMethodDefinitionReference` (rmdf); `+ 0x10 (16)` TagBlock `ShaderOptions`
of 2-byte `{i16 OptionIndex}`; `+ 0x28 (40)` TagBlock `ShaderProperties` of 132-byte entries
`{0x0 TagReference TemplateReference (rmt2), 0x10 (16) TagBlock ShaderMaps, 0x1C (28) TagBlock
TilingData of 16-byte float4}`; `ShaderMapBlock` is **28 bytes** on U13 (`[FixedSize(28,
MinVersion = MccHalo3U4, MaxVersion = Halo3ODST)]`) with `BitmapReference` at `0x0` and
`TilingIndex` byte at `0x16 (22)`. Material/texture resolution itself is out of scope here
(`Common\Gen3\Gen3MaterialHelper.cs` maps rmt2 usage names like `base_map` to bitmaps).

---

## 3. `scenario_lightmap_bsp_data` (Lbsp) tag

Source: `Halo3\ScenarioLightmapBspDataTag.cs` (offsets are unversioned; the `[FixedSize(436,
MaxVersion = MccHalo3U4)]` only applies when the struct is embedded in `sLdT` on pre-U4 builds).

| offset | type | name |
|---|---|---|
| `0x2` (2) | i16 | `BspIndex` |
| `0x134` (308) | TagBlock | `Sections` (76-byte entries, 3.1) |
| `0x140` (320) | TagBlock | compression info (44-byte entries, 2.3) — not read by Reclaimer from here, but identical data on riverworld |
| `0x1AC` (428) | i32 | `ResourcePointer` (render geometry api resource id; low 16 bits = resource index) |

### 3.1 SectionBlock (mesh), 76 (0x4C) bytes

`RenderModelTag.cs` `[FixedSize(76, MinVersion = Halo3Delta)]`.

| offset | type | name | notes |
|---|---|---|---|
| `0x0` (0) | TagBlock | `Submeshes` (parts), 16-byte entries | |
| `0xC` (12) | TagBlock | `Subsets`, 8-byte entries | |
| `0x18` (24) | i16 | `VertexBufferIndex` | index into the resource's vertex-buffer array; **-1 = no geometry** |
| `0x1A` (26) | 14 bytes | unused vertex buffer slots | all `0xFFFF` on riverworld (7 more i16 slots) |
| `0x28` (40) | i16 | `IndexBufferIndex` | -1 = none |
| `0x2A` (42) | i16 | second index buffer slot? | `0xFFFF` on riverworld; not read |
| `0x2C` (44) | u8 | `Flags` (MeshFlags) | bit0 `MeshHasVertexColor`, bit1 `UseRegionIndexForSorting`, bit2 `UseVertexBuffersForIndices`, bit3 `MeshHasPerInstanceLighting`, bit4 `MeshIsUnindexed` |
| `0x2D` (45) | u8 | `NodeIndex` | 255 = none (always 255 in a BSP) |
| `0x2E` (46) | u8 | `VertexFormat` | vertex type id, see 5 |
| `0x2F` (47) | u8 | ? (prt type) | 0 on riverworld |
| `0x30` (48) | u8 | `IndexFormat` | 3 = triangle list, 5 = triangle strip (used only when the mesh is unindexed, see 6) |

riverworld: instance sections have `Flags = 8` (per-instance lighting), `VertexFormat = 1`;
cluster sections have `Flags = 0`, `VertexFormat = 0`; all have `IndexFormat = 3`.

Reclaimer: `IsUnindexed = IndexBufferIndex == -1 || Flags has MeshIsUnindexed`.

### 3.2 SubmeshBlock (part), 16 (0x10) bytes

| offset | type | name | notes |
|---|---|---|---|
| `0x0` (0) | i16 | `ShaderIndex` | into sbsp Shaders |
| `0x2` (2) | 2 | (unused by Reclaimer) | |
| `0x4` (4) | u16 | `IndexStart` | first index in the section's index buffer |
| `0x6` (6) | u16 | `IndexLength` | number of indices |
| `0x8` (8) | u16 | `SubsetIndex` | first subset |
| `0xA` (10) | u16 | `SubsetCount` | |
| `0xD` (13) | u8 | `Flags` (SubmeshFlags) | bit0 `DislikesPhotons`, bit1 `IgnoredByLightmapper`, bit2 `HasTransparentSortingPlane`, bit3 `IsWaterSurface` |
| `0xE` (14) | u16 | `VertexCount` | |

Verified: section 78 (cluster 2) has 17 submeshes whose `IndexLength`s sum to 31122 = the index
buffer length; its submesh 0 has flags 8 and shader 19 = the `rmw ` water shader.
`IndexStart`/`IndexLength` are 16-bit, so one section cannot address more than 65535 indices.

### 3.3 SubsetBlock, 8 (0x8) bytes

`{0x0 u16 IndexStart, 0x2 u16 IndexLength, 0x4 u16 SubmeshIndex, 0x6 u16 VertexCount}`.
Subsets are finer draw ranges inside a submesh (per instance in render_models). For the sbsp
they are not needed: Reclaimer builds segments from submeshes only.

---

## 4. The render geometry api resource

Source: `Halo3\Halo3Common.cs` `GetMeshes`, `Halo3\ResourceIdentifier.cs`,
`Halo3\CacheFileResourceGestaltTag.cs` + `Config\CacheFileResourceGestaltTag.config.cs`,
`Halo3\CacheFileResourceLayoutTableTag.cs`.

### 4.1 `zone` (cache_file_resource_gestalt), U13 = builder `AddVersion(MccHalo3, null)`

| offset | type | name |
|---|---|---|
| `0x64` (100) | TagBlock | `ResourceEntries`, 64-byte entries |
| `0x148` (328) | i32 | `FixupDataSize` |
| `0x154` (340) | i32 | `FixupDataPointer` (shifted tag-data pointer; the field is 12 bytes after the size, i.e. a Halo data reference `{size, ..., pointer}`) |

riverworld: 7428 entries, fixup data 666384 bytes at file `0x5988640`.

**ResourceEntryBlock, 64 (0x40) bytes** (builder `AddVersion(Halo3Retail, null).HasFixedSize(64)`):

| offset | type | name | notes |
|---|---|---|---|
| `0x0` (0) | TagReference | `OwnerReference` | the tag owning the resource (the Lbsp) |
| `0x10` (16) | i32 | `ResourcePointer` | **not** the same value as the owner's resource id (riverworld: `0x0201EE3D` vs tag value `0xEE3D0CDA`; the low 16 bits `EE3D` equal the tag value's high 16 bits, so it looks like `{i16 salt, i16 0x0201}`). Unused; do not match on it |
| `0x14` (20) | i32 | `FixupOffset` | offset of this resource's definition inside the fixup data |
| `0x18` (24) | i32 | `FixupSize` | size of the definition |
| `0x20` (32) | i16 | `LocationType` | 1 on the Lbsp resource |
| `0x22` (34) | i16 | `SegmentIndex` | into `play` Segments; `< 0` = no data |
| `0x28` (40) | TagBlock | `ResourceFixups`, 8-byte `{u32 patch offset inside the definition, u32 target}` | the target is a cache address: 3-bit type `target >> 29` + 29-bit offset `target & 0x1FFFFFFF` (TagTool `CacheAddress`, resources.md 3.2). Every fixup of this entry is type 2 (top nibble `0x4`) = byte offset of a buffer inside the resource's primary data, relative to the segment start. Reclaimer's `MaskedOffset = target & 0x0FFFFFFF` gives the same number because bit 28 is never set (corrected: was described as a 4-bit nibble + 28-bit offset) |
| `0x34` (52) | TagBlock | `ResourceDefinitionFixups`, 8-byte `{u32 0x20000000 | definition offset, u32 type}` | internal pointers inside the definition; not needed |

The resource id in a tag (`ResourceIdentifier.Value`) is an i32 whose **low 16 bits are the
`ResourceEntries` index** (`ResourceIndex = Value & 0xFFFF`); the high 16 bits are a salt.

### 4.2 Definition layout inside the fixup data (`fixupData + FixupOffset`, `FixupSize` bytes)

Reclaimer reads the two counts from the **last 24 bytes** of the definition and the two info
arrays from its **start**:

```
+0                                  VertexBufferInfo[vbCount]   28 bytes each
+28*vbCount                         12-byte struct per vertex buffer (D3D object placeholders; skip)
+28*vbCount + 12*vbCount            IndexBufferInfo[ibCount]    24 bytes each
+ ... + 24*ibCount                  12-byte struct per index buffer (skip)
+ ... + 12*ibCount                  4 x 12-byte tag-block headers {i32 count, i32 ptr, i32 ptr}
FixupSize - 24                      i32 vbCount        (third header's count)
FixupSize - 12                      i32 ibCount        (fourth header's count)
```

Verified on riverworld: `FixupSize = 19732 = 28*394 + 12*394 + 24*109 + 12*109 + 48`,
`vbCount = 394 (0x18A)`, `ibCount = 109 (0x6D)`.

**VertexBufferInfo, 28 (0x1C) bytes** (`[FixedSize(28)]` struct in `Halo3Common.cs`):

| offset | type | name | notes |
|---|---|---|---|
| `0x0` (0) | i32 | `VertexCount` | 0 (or a -1 index into this array) = skip the section |
| `0x4` (4) | i16 | vertex format (resource enum) | riverworld: 2 for sections with `VertexFormat 1` (rigid), 1 for `VertexFormat 0` (world). Looks like `section.VertexFormat + 1`; **unverified in general**, not read by Reclaimer |
| `0x6` (6) | i16 | vertex stride | 36 for rigid, 44 for world; equals `DataLength / VertexCount`. Not read by Reclaimer (it derives the stride from the XML) |
| `0x8` (8) | i32 | `DataLength` | bytes |
| `0xC` (12) | 8 | zero | |
| `0x14` (20) | u32 | runtime data address (patched by `ResourceFixups[i]`) | the fixup's patch offset for buffer i is `28*i + 20` |
| `0x18` (24) | 4 | zero | |

**IndexBufferInfo, 24 (0x18) bytes** (`[FixedSize(24)]`):

| offset | type | name | notes |
|---|---|---|---|
| `0x0` (0) | i32 | `IndexFormat` | Reclaimer enum: 0 Default, 1 LineList, 2 LineStrip, **3 TriangleList**, 4 TriangleFan, **5 TriangleStrip**, 6 QuadList, 7 RectList. All 109 on riverworld are 3 |
| `0x4` (4) | i32 | `DataLength` | bytes |
| `0x8` (8) | 8 | zero | |
| `0x10` (16) | u32 | runtime address (patched) | fixup patch offset `28v + 12v + 24*j + 16` |
| `0x14` (20) | 4 | zero | |

### 4.3 How section i picks its buffers

- `vInfo = vertexBufferInfo[section.VertexBufferIndex]`, `iInfo = indexBufferInfo[section.IndexBufferIndex]`.
- Vertex data offset inside the resource blob: `ResourceFixups[section.VertexBufferIndex].MaskedOffset`.
- Index data offset: `ResourceFixups[2 * vbCount + section.IndexBufferIndex].MaskedOffset`.

So the fixup list is ordered `[vb data x vbCount][vb placeholder x vbCount][ib data x ibCount]
[ib placeholder x ibCount][2 for the tail headers]`; riverworld has `1008 = 2*394 + 2*109 + 2`.
Verified: section 78's vertex buffer 83 is at `0x1060E4` (9177 x 44 = 403788 bytes) and its index
buffer 75 at `0xB7B118`, and the highest index found there is 9176 = `VertexCount - 1`.

Several sections may share a vertex buffer (Reclaimer caches by index); an index buffer belongs
to one section.

### 4.4 Getting the resource bytes (`play` tag, pages, deflate)

`play` offsets for U13 (`CacheFileResourceLayoutTableTag.cs`, `[Offset(60, MinVersion =
MccHalo3, MaxVersion = Halo3ODST)]` for Segments):

| offset | type | name | entry |
|---|---|---|---|
| `0xC` (12) | TagBlock | `SharedCaches` | 264 bytes; `0x0` null-terminated file name, 32 chars, e.g. `maps\shared.map` |
| `0x18` (24) | TagBlock | `Pages` | 88 bytes: `0x4` i16 `CacheIndex` (-1 = this map, else SharedCaches index), `0x8` i32 `DataOffset`, `0xC` i32 `CompressedSize`, `0x10` i32 `DecompressedSize`, `0x54` i16 `DataChunkCount` |
| `0x24` (36) | TagBlock | `SizeGroups` | 16 bytes (sounds only) |
| `0x3C` (60) | TagBlock | `Segments` | 16 bytes: `0x0` i16 `PrimaryPageIndex`, `0x2` i16 `SecondaryPageIndex`, `0x4` i32 `PrimaryPageOffset`, `0x8` i32 `SecondaryPageOffset`, `0xC` i16, `0xE` i16 size-group indices |

Algorithm (`ResourceIdentifier.ReadData(PageType.Auto)`):

1. `entry = zone.ResourceEntries[id & 0xFFFF]`; require `entry.SegmentIndex >= 0`.
2. `segment = play.Segments[entry.SegmentIndex]`. Use the **secondary** page if `SecondaryPageIndex >= 0`, else the primary. If the chosen page has `DataOffset < 0` or `CompressedSize == 0`, fall back to the primary.
3. `page = play.Pages[pageIndex]`. File = this map if `page.CacheIndex < 0`, else `maps folder \ SharedCaches[CacheIndex].FileName` (basename only). riverworld lists three (`english.map`, `mainmenu.map`, `shared.map`); all 11 campaign maps `005_intro`..`130_epilogue` list a fourth, index 3 = `campaign.map`. Always resolve `SharedCaches[page.CacheIndex]` from the map being read; never hard-code a list of three (corrected: the old wording was riverworld-specific; see collision-bsp-research.md 5).
4. Raw-data base: for this map, `u32 at header 0x4D0 (1232)` = section offset table entry 1 (riverworld `0x114000`); for a shared cache **16384 (0x4000)** = the header size (verified: shared.map's own `0x4D0` holds `0x4000`). Seek `base + page.DataOffset`.
5. If `CompressedSize == DecompressedSize`: the page is stored raw. Otherwise it is a **raw deflate stream** (no zlib header; .NET `DeflateStream`), decompress `CompressedSize` bytes to `DecompressedSize`.
6. The resource's bytes start at `segmentOffset` (the page offset chosen in step 2) inside the decompressed page and run to the end of it (`DecompressedSize - segmentOffset`, Reclaimer caps at `int.MaxValue`). All fixup `MaskedOffset`s are relative to this start.

riverworld Lbsp resource: segment 3254, primary page 1420 (secondary -1), page offset
25733816, `CacheIndex -1`, `DataOffset 0`, compressed 45416454 -> decompressed 92027152 bytes
(first bytes `44 9A 77 34`, raw deflate). That is 45 MB to inflate just to read one BSP; a Rust
reader should cache the inflated page or stream-skip to the offset.

Byte order: MCC caches are little-endian x86, so no endian swap of the buffers (Reclaimer only
swaps for the Xbox 360 PowerPC caches).

---

## 5. Vertex formats (`Reclaimer.Blam\Resources\MccHalo3VertexBuffer.xml`)

Only stream 0 fields are read; the stride is `offset of the last field + size of its type`
(`Common\XmlVertexBuilder.cs`, no `size` attribute in this file). The MCC file differs from the
Xbox `Halo3VertexBuffer.xml` only in normals: MCC stores normal/tangent/binormal as `Int16_N4`
(8 bytes) where the Xbox used `DHenN3` (4 bytes), which is why the strides grew (rigid 36 vs 28).

| id | name | stride | fields (offset, type, usage) |
|---|---|---|---|
| `0x00` | `s_world_vertex` | 44 (0x2C) | `0x0 Float32_3 position`, `0xC Float32_2 texcoords`, `0x14 Int16_N4 normal`, `0x1C Int16_N4 tangent`, `0x24 Int16_N4 binormal` |
| `0x01` | `s_rigid_vertex` | 36 (0x24) | `0x0 UInt16_N4 position`, `0x8 UInt16_N2 texcoords`, `0xC Int16_N4 normal`, `0x14 Int16_N4 tangent`, `0x1C Int16_N4 binormal` |
| `0x02` | `s_skinned_vertex` | 44 (0x2C) | as rigid, plus `0x24 UInt8_4 blendindices`, `0x28 UInt8_N4 blendweight` |
| `0x0F` | `s_decorator_vertex` | 20 (0x14) stream 0 | `0x0 UInt16_N4 position`, `0x8 UInt16_N2 texcoords`, `0xC Int16_N4 normal` (streams 1, 2 ignored) |
| `0x10` | `s_tiny_position_vertex` | 8 (0x8) | `0x0 UInt16_N4 position` |

BSPs use `0x00` for clusters and `0x01` for instances (riverworld: 26 and 76 sections). The
other formats belong to render models and decorators; any other id must be rejected (the XML has
no entry, Reclaimer would throw).

Per-field decode rules (`Reclaimer.Core\Geometry\Vectors\PackedVectorHelper.cs` and the
generated vector structs). All little-endian.

| type | storage | per component | range |
|---|---|---|---|
| `Float32_2/3/4` | f32 each | as is | |
| `UInt16_N2/N4` | u16 each | `v / 65535.0` | 0..1 (then expand with bounds) |
| `Int16_N2/N4` | i16 each (two's complement) | `v / 32767.0`, clamp to -1..1 (so -32768 -> -1) | -1..1 |
| `UInt8_N4` | u8 each | `v / 255.0` | 0..1 |
| `Int8_N4` | i8 each | `v / 127.0` clamp | -1..1 |
| `UInt8_4`, `UInt16_2/4` | integers | as is (blend indices) | |
| `DecN4` | one u32; bits 0-9 X, 10-19 Y, 20-29 Z, 30-31 W | sign-extend each field (two's complement, 10 bits -> `/511`, 2 bits -> `/1`) | -1..1 |
| `UDecN4` | same bit layout | unsigned `/1023`, W `/3` | 0..1 |
| `DHenN3` | u32; bits 0-9 X (10), 10-20 Y (11), 21-31 Z (11) | signed `/511`, `/1023`, `/1023` | -1..1 |
| `HenDN3` | u32; 11, 11, 10 bits | signed | -1..1 |
| `UDHenN3`, `UHenDN3` | as above, unsigned | `/ (2^n - 1)` | 0..1 |
| `NxAAA2` | 10,10,10,2 "sign shifted" | `v / 511 - 1` | -1..1 |
| `UxAAC0` | 10, 10, 12 unsigned | | 0..1 |

General rule for an n-bit signed field: `value / (2^(n-1) - 1)` after sign extension, clamped;
unsigned: `value / (2^n - 1)`. The 4th component (W) of `Int16_N4` normals is padding/sign; use
XYZ only. Verified: decoded normals of real vertices have length 1.000.

---

## 6. Position / texcoord decompression

Source: `Reclaimer.Core\Geometry\RealBounds3D.cs` / `RealBounds2D.cs` `CreateExpansionMatrix`,
applied by consumers as `Vector3.Transform(pos, matrix)` (row vector). In plain terms:

```
pos.x = Xmin + px * (Xmax - Xmin)      px, py, pz in 0..1 from UInt16_N4 (W ignored)
pos.y = Ymin + py * (Ymax - Ymin)
pos.z = Zmin + pz * (Zmax - Zmin)
uv.u  = Umin + tu * (Umax - Umin)      tu, tv from UInt16_N2
uv.v  = Vmin + tv * (Vmax - Vmin)
```

with the bounds from `BoundingBoxes[sectionIndex]` (2.3). If the section has no bounds entry
(index >= count) or the bounds are empty (min == max on every axis), apply nothing. Float
positions (`s_world_vertex`) are already in BSP space; Reclaimer never has a bounds entry for them
on riverworld, so even if one existed it would be wrong to apply it to a float position without
checking the compression flags. Verified: instance 0's 702 decoded positions span exactly
`[-0.41, 0.89] x [-3.20, 0.80] x [-5.94, 7.66]` = its bounds entry.

Normals, tangents, binormals are already unit vectors; no bounds.

---

## 7. Index buffers

- Element size: Reclaimer uses **u16 unless `VertexCount > 65535`**, then u32
  (`Halo3Common.cs`). No riverworld buffer exceeds 65535 vertices. (Unsure whether a 32-bit
  buffer ever occurs in a BSP; the submesh `IndexStart/IndexLength` are u16 anyway.)
- Index count = `DataLength / elementSize`. Indices are relative to the section's vertex buffer.
- Layout = `IndexBufferInfo.IndexFormat` (4.2). **3 = triangle list**: every 3 indices are a
  triangle in order. **5 = triangle strip** and also **0 = Default** are un-stripped
  (`Reclaimer.Core\Geometry\GeometryExtensions.cs` `GetTriangleIndicies` / `Unstrip`):

```
unstrip(indices):
    n = 0
    for each index x in order:
        (a, b, c) = (b, c, x)
        n += 1
        if n < 3: continue
        if a == b or a == c or b == c: continue        # degenerate stitch triangle
        if n is odd:  emit (a, b, c)
        else:         emit (a, c, b)                   # flip every other triangle
```

  (Reclaimer's parity test is on the running position, so the first full triangle, at n = 3, is
  emitted in natural order.) Apply this to the submesh's `[IndexStart, IndexStart + IndexLength)`
  slice, not to the whole buffer.
- Unindexed sections (`IndexBufferIndex == -1` or `MeshIsUnindexed` flag): the implied index
  buffer is `0, 1, 2, ..., VertexCount-1` with layout = the section's own `IndexFormat` byte at
  `0x30` (`BlamUtils.CreateImpliedIndexBuffer`). In riverworld this never happens for a section
  that has vertices; decorator models use it.
- All 109 riverworld BSP index buffers are triangle lists with 16-bit indices.

---

## 8. From a cluster or instance to world-space triangles

Clusters: `Clusters[c].SectionIndex` names a mesh; its vertices are already in BSP (world)
space. No transform. Reclaimer puts them into one region `<Clusters>`, permutation name
`"{c:D3}"`, `MeshRange = (SectionIndex, 1)`.

Instances: `GeometryInstances[i].SectionIndex` names a mesh in instance-local space; per
instance (`Reclaimer.Core\Geometry\Model.cs` `ModelPermutation.GetFinalTransform`):

```
world = ((local_decompressed * TransformScale) * R) + T
```

where `R` is the 3x3 at `0x4` and `T` the translation at `0x28`, **row-vector convention**:
`world_c = scale * sum_k( local_k * R[k][c] ) + T_c` (row k of R = the world image of local axis
k). `GetFinalTransform` decomposes the 3x4 into scale/rotation/translation and re-multiplies as
`Scale(uniform) * Scale(decomposed) * Rotation * Translation`; for BSP instances the decomposed
scale is 1 and the rotation is proper (instance 1 has rows `(0,1,0) (0,0,-1) (-1,0,0)`, det
+1), so the formula above is equivalent. If a matrix ever has a negative determinant (mirrored
instance), flip the triangle winding; our HaloExport does this and reports 99.96% of windings
agree with the normals. Which side is the front (counter-clockwise seen from the normal side; no
flip for bevy or for the skate soup) is spelled out in the gap-fill section at the end of this document.

Normals transform with `R` only (`Vector3.TransformNormal`), then renormalise.

Check: instance 0 vertex 0 local `(-0.144, -2.341, -5.944)` -> world `(81.216, -150.838, 3.908)`,
6.9 wu from the instance's bounding-sphere centre (radius 7.12).

Material per triangle = `Shaders[submesh.ShaderIndex]`.

The 1 wu = 3.048 m scale and the Z-up to Y-up swap are applied after this; Reclaimer's
`Scene.WrapSingleModel(model, WorldUnitScale)` carries the unit as metadata.

---

## 9. What gets skipped

| rule | source | riverworld |
|---|---|---|
| section whose `VertexBufferIndex` is `-1`, or whose `VertexBufferInfo.VertexCount == 0` | `Halo3Common.cs`: `ElementAtOrDefault` gives a zero struct for -1, then `continue`; the mesh is `null` and consumers skip null meshes | sections 34, 76, 77, 99. Clusters 0 and 1 (sections 76, 77) are the two empty clusters with `+FLT_MAX` bounds |
| mesh with no segments / submesh with null material | consumer choice (HaloExport skips both). **Rust policy: keep the triangles under a placeholder material; see the section "Gap fill: Triangles whose submesh has a null shader reference" below** | 0 null shaders, 0 affected submeshes |
| submesh flag `IsWaterSurface` (bit3) | not skipped by Reclaimer; it is just a flag. For skating collision we probably want to drop or tag water (shader class `rmw `) | section 78 submesh 0, shader `riverworld_water_rough` |
| "sky blocker" | **not an sbsp thing** on the maps checked: no riverworld instance name contains `sky` or `blocker`. `bsp_sky_blocker_giant` is a *scenery object* in the scenario (Outskirts), which HaloExport drops as backdrop by size. Sky itself is a separate scenery/render model listed in `scnr.Skies` | none |
| instance names starting with `!` | convention only (ground/base geometry?), not a rule; unsure | `!basespire08`, `!door_frame03` |

Reclaimer applies **no** flag-based skipping to clusters or instances; everything with a vertex
buffer is exported.

---

## 10. Algorithm: sbsp -> list of (material, world-space triangles)

```
read_bsp_geometry(map, sbsp_tag_index):
    scnr   = tag of class 'scnr' named header.scenario_path
    bsps   = block(scnr + 0x18, stride 108);  bsp_index = position of sbsp in bsps (BspReference at +0)
    sbsp   = meta(sbsp_tag_index)
    sldt   = tag referenced at scnr + 0x6AC
    lbsp   = first tag in block(sldt + 0x4, stride 16) with i16(meta + 0x2) == bsp_index
             (fallback: class 'Lbsp' with the sbsp's tag name)

    shaders   = block(sbsp + 0xD0, 36)   -> material[i] = tag at +0
    clusters  = block(sbsp + 0xC4, 280)  -> section = i16 at +216
    instances = block(sbsp + 0x1C0, 120) -> scale f32 +0, R rows f32[9] +4, T f32[3] +40, section i16 +52, name +84
    bounds    = block(sbsp + 0x300, 44)  -> {flags, X, Y, Z, U, V} pairs at +4..+44
    sections  = block(lbsp + 0x134, 76)  (same data as sbsp + 0x2F4 on riverworld)
    rid       = i32(lbsp + 0x1AC) & 0xFFFF

    # resource definition
    zone  = global 'zone'; entry = block(zone + 0x64, 64)[rid]
    fx    = expand(i32(zone + 0x154)) + i32(entry + 0x14);  fsz = i32(entry + 0x18)
    vbN   = i32(fx + fsz - 24);  ibN = i32(fx + fsz - 12)
    vbInfo[i] = {count i32 @fx+28i, length i32 @fx+28i+8}
    ibInfo[j] = {format i32, length i32} @ fx + 40*vbN + 24j
    fixups    = block(entry + 0x28, 8) -> offset[k] = u32(+4) & 0x1FFFFFFF   # 29-bit offset, type = u32(+4) >> 29 (corrected: was 0x0FFFFFFF; same result, bit 28 is never set)

    # resource bytes
    play = global 'play' (or zone + 0x1C);  seg = block(play + 0x3C, 16)[i16(entry + 0x22)]
    page, segOff = secondary if seg.secondaryPage >= 0 else primary (fallback to primary if page empty)
    file = this map if page.cacheIndex < 0 else maps\<SharedCaches[cacheIndex].name>
    base = u32(header + 0x4D0) if same map else 0x4000
    blob = bytes at file[base + page.dataOffset .. + compressedSize]
    if compressedSize != decompressedSize: blob = raw_inflate(blob)
    data = blob[segOff ..]

    # meshes
    for s in 0..len(sections):
        sec = sections[s]
        vbi = i16(sec + 0x18); ibi = i16(sec + 0x28); flags = u8(sec + 0x2C); vfmt = u8(sec + 0x2E)
        if vbi < 0 or vbInfo[vbi].count == 0: mesh[s] = None; continue
        verts = decode(data[fixups[vbi] ..], vfmt, vbInfo[vbi].count)      # section 5
        if s < len(bounds) and vfmt uses UInt16_N4 position: expand positions/uvs with bounds[s]   # section 6
        if ibi < 0 or flags & 0x10: idx = 0..count-1 as layout u8(sec + 0x30)
        else: idx = data[fixups[2*vbN + ibi] ..] as u16 (u32 if count > 65535), layout ibInfo[ibi].format
        mesh[s] = {verts, idx, layout, submeshes = block(sec + 0x0, 16)}

    out = []
    emit(mesh, M):                     # M = None for clusters
        for sm in mesh.submeshes:
            tris = triangles(mesh.idx[sm.indexStart .. +sm.indexLength], mesh.layout)   # section 7
            for (a, b, c) in tris:
                p = [mesh.verts[k].pos for k in (a, b, c)]
                if M: p = [(v * M.scale) * M.R + M.T for v in p]; if det(M.R) < 0: swap b, c
                out.append((material[sm.shaderIndex], p))

    for c in clusters:  if mesh[c.section]: emit(mesh[c.section], None)
    for i in instances: if mesh[i.section]: emit(mesh[i.section], i)
    return out
```

---

## 11. EXPECTED VALUES on riverworld.map (Valhalla)

Reproduce with `python -I scripts\probe_sbsp_geometry.py`.

| item | value |
|---|---|
| scenario | tag 8 `levels\multi\riverworld\riverworld`, meta file `0x5B29158` |
| sbsp | tag **9757** `levels\multi\riverworld\riverworld`, meta file `0x8C9FCF4`; `StructureBsps` count 1, bsp index 0 |
| sLdT | tag 6878 `levels\multi\riverworld\riverworld_faux_lightmap`; `LightmapRefs` count 1 |
| Lbsp | tag 6879 `levels\multi\riverworld\riverworld`, `BspIndex` 0 |
| world bounds (wu) | X `-40.852 .. 206.534`, Y `-285.427 .. 92.487`, Z `-15.517 .. 1892.309` |
| **cluster count** | **26** |
| **instance count** | **262** (76 distinct sections, 0..75) |
| **material (shader) count** | **64** |
| sections | 102 (sbsp `0x2F4` and Lbsp `0x134` both -> file `0x824ADC8`) |
| bounding boxes | 76 (file `0x824CC10`), flags 3 on 75; entry 34 (the empty section) has flags 0 and zero bounds (corrected: was "flags 3 on all") |
| sbsp resource ids | `0xFDE91C86` (7302), `0xFDEA1C87` (7303, empty), `0xFE651D02` (7426) |
| Lbsp resource id | `0xEE3D0CDA` -> index **3290**; zone entry: fixup offset 298232, size 19732, segment 3254, 1008 fixups |
| definition | vbCount **394**, ibCount **109**; `vb[0] = {702 verts, fmt/stride 0x0024_0002, 25272 bytes}`; `ib[0] = {3, 2532 bytes}` |
| page | 1420, `CacheIndex -1`, `DataOffset 0`, compressed 45416454, decompressed 92027152, resource starts at page offset 25733816; raw base `0x114000` |
| cluster sections | 76..101 in order; vertex format 0 (world, stride 44); 76 and 77 have no vertex buffer |
| cluster 2 | bounds `[-40.85, 206.53] [-285.43, 92.49] [-9.29, 90.88]`, section 78: vb 83 (9177 verts @ `0x1060E4`), ib 75 (31122 u16 @ `0xB7B118`), 17 submeshes, first vertex `(83.430, -145.447, 16.045)` |
| compression bounds `[0]` (section 0) | X `-0.41 .. 0.89`, Y `-3.2029 .. 0.7969`, Z `-5.9445 .. 7.6591`, U `-0.05 .. 1.85`, V `-1.9378 .. 1.85` |
| `[1]` | X `-2.2932 .. 3.5077`, Y `-10.4947 .. 6.1`, Z `-1.7 .. 0.9`, U `-2.1221 .. 2.1207`, V `-3.5338 .. 6.415` |
| `[2]` | X `-5.6205 .. -2.3274`, Y `-6.4027 .. -3.1095`, Z `6.5622 .. 9.1452`, U `-2.0009 .. 2.4268`, V `-1.3511 .. 1.7243` |
| instance 0 | `!basespiresmall07`, section 0, scale 0.9999999, R = identity, T `(81.360, -148.497, 9.852)`; sphere? `(81.6, -149.7, 10.709)` r 7.119 |
| instance 1 | `!basespire08`, section 1, scale 0.9999999, R rows `(0,1,0) (0,0,-1) (-1,0,0)`, T `(81.200, -145.207, 11.200)` |
| instance 2 | `!door_frame03`, section 2, scale 0.9999999, R rows `(1,0,0) (0,-1,0) (0,0,-1)`, T `(81.217, -147.699, 9.990)` |
| instance 0 mesh | vb 0 (702 verts @ 0), ib 0 (1266 u16 @ `0xB578F0`), 1 submesh (shader 40), 10 subsets; vertex 0 local `(-0.144, -2.341, -5.944)` uv `(0.08, 1.635)` normal `(0, 0, -1)` |

---

## 12. Unsure / open

1. Whether the sbsp's render-geometry resource (`0x36C`) is empty on *every* MCC map, or only on
   maps with a `faux_lightmap`. Check a campaign map before relying on the Lbsp-only path; keep
   Reclaimer's fallback order (Lbsp by BspIndex, then Lbsp by name).
2. `VertexBufferInfo` field at `+4`: looks like `{i16 format = section.VertexFormat + 1, i16 stride}`; only two formats observed.
3. Instance fields at `0x36`, `0x38`, `0x40..0x50`, `0x5A`, `0x60`, `0x64` (2.2) are named from TagTool/Assembly and only checked against riverworld's bytes; only the four Reclaimer fields are trusted (corrected: 2.2 is now the single merged field map, shared with collision-bsp-research.md 10.2).
4. The sbsp's `ResourcePointer1`/`ResourcePointer3` (`0x254`, `0x38C`): present, owned by the sbsp, with 488 and 19456-byte definitions. Likely the collision / structure resources HANDOFF mentions; untouched here.
5. No separate instanced-geometry-definitions block was identified in the U13 sbsp; Reclaimer does not need one. If a Rust reader wants per-definition bounds, they are `BoundingBoxes[section]` plus the per-instance sphere guess above.
6. World bounds Z max 1892 wu on a map whose geometry tops out near 91 wu.
7. 32-bit index buffers: supported by the rule in 7 but never seen in a BSP.
8. `IndexFormat 0 (Default)` being treated as a strip is Reclaimer's choice; not observed in a BSP.
9. Section byte `0x2F` and i16 `0x2A` meanings.
10. Shared-cache resources: the 16384 base is verified from shared.map's header, but no BSP page on riverworld lives in a shared cache, so the full shared-cache path was not exercised.

---

## Gap fill: Triangles whose submesh has a null shader reference

Written 2026-10-27 (second reading round). Question: Reclaimer's `Halo3Common.GetMaterials` yields a
`null` material when a `ShaderBlock` entry's `ShaderReference.Tag` is null, and HaloExport then drops
every submesh (segment) whose material is null. What must the Rust reader do with those triangles?

**Short answer: keep them.** A null shader reference is a *material* fact, not a *geometry* fact. The
triangles are real, indexed, placed geometry; only their render method is missing. Reclaimer itself
keeps them (viewer, Assimp export and RMF writer, see 2 below); only our local HaloExport tool drops
them, and that was a glTF convenience, not a format rule. For collision (milestones 3 and 4) dropping
them would punch holes. Verified on all 41 `.map` files in the MCC Halo 3 install: exactly **one BSP in
the whole install (Turf, `s3d_turf.map`) has triangles under a null shader**: 2 submeshes, 468
unplaced / **1,248 placed triangles**, all lamp-post instances. riverworld has none.

### 1. How "null" is decided (Reclaimer facts, resolved to U13)

| fact | value | source |
|---|---|---|
| ShaderBlock entry | 36 (0x24) bytes; `0x0` TagReference `ShaderReference` (16 bytes `{4CC class, 8 skip, i32 tag id}`); bytes `0x10..0x24` not read by Reclaimer | `Blam\Halo3\RenderModelTag.cs` `[FixedSize(36)] ShaderBlock` |
| TagReference null test | `TagId = (i16)(raw_tag_id & 0xFFFF)`; the reference is null iff `TagId < 0` (`IsValid` = `TagId >= 0 && cache != null`; `Tag` = `IsValid ? TagIndex[TagId] : null`). **The class-id bytes are not consulted.** | `Blam\Common\TagReference.cs` lines 19-21 |
| TagReference size on U13 | 16 bytes: `[FixedSize(16, MinVersion = Halo3Beta)]` (14 <= 22; the 8-byte variant is `MinVersion Halo2Xbox, MaxVersion Halo3Beta`, exclusive, so it does not apply) | same file, attributes at the top |
| GetMaterials | iterates `shaders.Select(s => s.ShaderReference.Tag)`; `if (tag == null) { yield return null; continue; }`. So the materials list stays **index-aligned** with the Shaders block; the null is a placeholder at the same index. A non-null tag whose `ReadMetadata<ShaderTag>()` returns null still yields a Material (id, file name, source tag name) with no textures | `Blam\Halo3\Halo3Common.cs` lines 42-88 |
| Segment material | `Material = materials.ElementAtOrDefault(submesh.ShaderIndex)`; `IndexStart`, `IndexLength` and the submesh flags are copied regardless. `ElementAtOrDefault` returns null for a negative or out-of-range index too, without throwing. **Reclaimer never drops a segment or a mesh because its material is null.** | `Halo3Common.cs` lines 187-199 (`GetMeshes`); same pattern for render models in `RenderModelTag.cs` line 196 |

So there are two distinct ways a submesh ends up with no material, and the Rust reader must treat both
the same way: (a) `ShaderIndex` names a Shaders entry whose tag id low 16 bits are negative (the only
kind seen on disk, see 3); (b) `ShaderIndex < 0` or `>= Shaders.count` (never seen on disk, 129 BSPs
checked, but must not panic).

### 2. What the consumers do with a null-material segment

| consumer | behaviour | source |
|---|---|---|
| Reclaimer model viewer | draws the segment; `textureLoader[sub.Material?.Id]` falls back to the loader's default material for a null id | `Reclaimer\Controls\DirectX\MeshLoader.cs` line 104 |
| Reclaimer Assimp export (OBJ/FBX/DAE...) | mesh is always added; `MaterialIndex` is only set when `sub.Material != null`, so the mesh gets Assimp's default material 0 | `Reclaimer\Plugins\ModelViewerPlugin.cs` line 472 |
| Reclaimer RMF scene writer | writes the segment with material index `-1` (`LazyList.AddIfNew(null)` returns -1 instead of adding) | `Reclaimer.Core\Geometry\Utilities\SceneWriter.cs` line 443, `LazyList.cs` `AddIfNew` |
| Reclaimer material list | `Model.EnumerateMaterials` filters nulls out of the *material pool*, not out of the geometry | `Reclaimer.Core\Geometry\Model.cs` lines 55-61 |
| (Halo 1 Anniversary Saber3D only) | a permutation is skipped only when **all** its segments have a null material; Halo 3 code has no such rule | `Blam\Saber3D\Halo1X\Scene.cs` line 60 |
| **HaloExport** (ours) | `foreach (var seg in mesh.Segments) { if (seg.Material == null) continue; ... }` — the triangles never reach the glTF, the sidecar does not count them | `HaloExport\Program.cs` line 490 (`BuildMesh`) |

HaloExport's drop is therefore the odd one out, and the Rust reader should **not** copy it.

### 3. Evidence from the map bytes

`python -I scripts\scan_null_shaders.py` (read-only; scans every `.map` in the MCC `halo3\maps` folder,
or one map given as argument). For each `sbsp` tag it reads the Shaders block (`sbsp + 0xD0`, 36-byte
entries), applies Reclaimer's null test to each entry, then walks the Sections block Reclaimer uses
(the Lbsp's, `Lbsp + 0x134`, found through `scnr + 0x6AC` -> `sLdT + 0x4` -> `BspIndex`, with the
by-name fallback) and every submesh in it, flagging `ShaderIndex` values that are out of range or hit a
null entry. Placed triangle counts multiply by the number of clusters (`sbsp + 0xC4`, `+216`) and
instances (`sbsp + 0x1C0`, `+52`) that use the section.

| item | result |
|---|---|
| map files scanned | 41; 39 have a tag index (`shared.map`, `campaign.map` have none, and no sbsp) |
| sbsp tags (BSPs) | **129** |
| BSPs with at least one null Shaders entry | **22**, on 10 maps: `005_intro` (1), `010_jungle` (5), `020_base` (3), `030_outskirts` (2), `070_waste` (1), `110_hc` (2), `120_halo` (5), `construct` (1), `s3d_turf` (1), `s3d_waterfall` (1) |
| null Shaders entries in total | **57**: 56 are *fully* null (class bytes `FF FF FF FF`, tag id `FFFFFFFF`), 1 is *half* null (class `rmsh`, tag id `FFFFFFFF`: `s3d_turf` shader[67]) |
| submeshes with `ShaderIndex` out of range | **0** anywhere |
| submeshes whose shader is null | **2**, both in `s3d_turf.map` (every other null entry is an unreferenced slot in the Shaders block; e.g. `120_halo` `120_bsp_010` has 7 null entries and no submesh uses any of them) |
| riverworld | 64 shaders, 0 null; 314 submeshes, 0 affected (matches reference-numbers.md section 3) |
| cyberdyne | 85 shaders, 0 null; 819 submeshes, 0 affected |
| BSPs whose Lbsp Sections block is **not** the same block as `sbsp + 0x2F4` | 10 (on `005_intro` 2, `010_jungle` 6, `030_outskirts` 1, `spacecamp` 1); their sbsp-side copy was scanned too: 0 affected submeshes. On the other 119 BSPs the two fields point at the same block (count and shifted pointer equal), as on riverworld |

The one real case, `s3d_turf.map` (Turf, one of the three `s3d_` maps MCC added to Halo 3 from Halo
Online assets, alongside `s3d_edge` and `s3d_waterfall`; sbsp tag 9972 `levels\multi\s3d_turf\s3d_turf`, Lbsp 7101, 246 shaders, 540 sections, 50 clusters, 1,451
instances):

| item | value |
|---|---|
| Shaders block | file `0x127F7AE4`; entry [67] at `0x127F8450` (`+ 36 * 67`) |
| shader[67] raw 36 bytes | `68 73 6D 72` (`rmsh` reversed) `00 x8` `FF FF FF FF` `00 x12` `DA 00 00 00` `FF 00 00 00` -> class set, tag id -1 -> **null by Reclaimer's rule**; neighbours [66] `rmsh` `unsc_panel_indus_05`, [68] `rmhg` `turf_banners_01` |
| section 74 | rigid vertices (`VertexFormat 1`, flags `0x8`, `IndexFormat 3`), vb 74, ib 74, 6 submeshes; **submesh 1: ShaderIndex 67, IndexStart 216, IndexLength 234 (78 triangles), 82 vertices, flags 0**. Siblings use `turf_metal_01_a`, `turf_metal_01_b`, `turf_metal_01`, `turf_banners_01`, `lights\lamp_03_wide_yelow` |
| section 74 placements | 0 clusters, **11 instances** 500..510 named `?turf_lamp_02`, `?turf_lamp_024`, `?turf_lamp_025`, `?turf_lamp_036`, `?turf_lamp_037`, `?turf_lamp_042` ... `?turf_lamp_048` -> 858 placed triangles |
| section 251 | same layout, vb 251, ib 251, 6 submeshes; **submesh 1: ShaderIndex 67, IndexStart 1080, IndexLength 1170 (390 triangles), 410 vertices, flags 0** |
| section 251 placements | 0 clusters, **1 instance** 1212 `?turf_lamp_02_merged` -> 390 placed triangles |
| total | 2 submeshes, 468 unplaced triangles, **1,248 placed triangles**, all instance geometry (lamp posts), none in clusters |

So on the one map where it happens, the null-shader geometry is a part of a lamp-post mesh sitting
between its metal parts. Dropping it would leave a hole in every lamp post on Turf; it would not affect
the ground. On every other map the policy changes nothing.

### 4. Rust policy

1. **Nullness rule.** A Shaders entry is null iff `(tag_id & 0xFFFF) as i16 < 0`. Do not look at the
   class bytes (Turf's entry has a valid `rmsh` class and a null id). Read the Shaders block into
   `Vec<Option<TagIndex>>` (or `Vec<MaterialSlot>` with a `Missing` variant) so indices stay aligned
   with `ShaderIndex`, exactly as Reclaimer's materials list does.
2. **Never drop geometry for a material reason.** A submesh with a null shader, a negative
   `ShaderIndex`, or an out-of-range `ShaderIndex` keeps its `IndexStart`/`IndexLength` triangles and
   is emitted like any other submesh. Resolve its material to one **placeholder material per BSP**
   (suggested name `<null shader>`, no tag id, category `opaque`). Out-of-range indices should log a
   warning (never seen on disk); null entries should not even warn per triangle, only once per BSP
   with the counts from 3.
3. **Tolerate null entries when loading materials.** 57 Shaders entries across the install are null and
   56 of them are referenced by nothing. Material/texture loading must skip a null slot without trying
   to read tag `-1`, and must not fail the BSP load.
4. **Collision (milestones 3 and 4).** Placeholder-material triangles go into the collision soup with
   the default surface type, like every other triangle. Rationale: the real collision BSP
   (`collision-bsp-research.md` section 8) has its own `CollisionMaterials` block at `sbsp + 0x28` and
   never looks at render shaders, so a missing render method cannot mean "no collision"; and the
   render-geometry collision approximation of milestone 3 must not have holes the real one does not.
   The skate-world material classifier must map the placeholder to a solid, skateable surface, never
   to water, decal, foliage or "invisible".
5. **Rendering.** Draw them by default with a visible flat placeholder (e.g. mid grey, or magenta in a
   debug build) so missing shaders are noticed, not hidden. Skipping them is allowed only as an explicit
   option (a renderer setting), never in the reader. Do not mirror HaloExport's `continue`.
6. **Unit tests / expected values.** riverworld and cyberdyne: 0 null entries, 0 affected submeshes.
   `s3d_turf`: shader[67] raw bytes as in 3, 2 affected submeshes (sections 74 and 251, submesh 1),
   468 unplaced and 1,248 placed triangles; the Lbsp Sections block equals `sbsp + 0x2F4` there
   (540 entries at file `0x11DFBDC0`). `120_halo` `120_bsp_010`: 7 null entries, 0 affected submeshes.

### 5. Remaining uncertainty

- Why Turf's lamp meshes reference a null shader is unknown (most likely a Halo Online light/emissive
  shader that was stripped in the MCC port). Whether the retail engine draws that part with a default
  shader or not at all was not checked (the game is never launched for this project), so rendering it
  with a placeholder may show a surface the game hides. This does not affect the collision rule: the
  real collision comes from the collision BSP, and the lamp part is enclosed by metal parts anyway.
- The 20 bytes after the TagReference in a 36-byte ShaderBlock entry (`+0x10..+0x24`) were not decoded
  (Turf's null entry has `DA 00 00 00` at `+0x1C` and `FF 00 00 00` at `+0x20`; the Halo 3 `global
  material index` / lightmap flags probably live there). Not needed for this policy.
- Side observation for section 1 / open item 1 of this document: on 10 campaign-and-DLC BSPs
  (`005_intro`, `010_jungle`, `030_outskirts`, `spacecamp`) the Lbsp's Sections block is **not** the
  same block as the sbsp's `0x2F4` copy (different shifted pointers, same counts). The null-shader
  scan found no difference in affected submeshes between the two copies, but a reader must still take
  the submesh list from the Lbsp (Reclaimer's path) and only fall back to the sbsp copy when no Lbsp is
  found; whether the two copies differ in `ShaderIndex` or index ranges on those BSPs was not checked
  beyond the null-shader test.

---

## Gap fill: Triangle winding / front-face convention

Added 2026-10-27 (second reading round). Reproduce with
`python -I docs\format\scripts\check_winding.py [map path] [--detail <section>]` (read-only; inflates
the 45 MB resource page, takes about a minute).

### The rule

A decoded triangle `(i0, i1, i2)` is **counter-clockwise when seen from the side its vertex normals
point to**. Equivalently, in Halo's right-handed Z-up frame, `cross(p1 - p0, p2 - p0)` points to the
front (outside, lit side). For ground that side is the **top**: the side you stand on. The order
comes straight from the index buffer; nothing in the decode chain reorders it:

- Reclaimer passes triangle-list indices through unchanged (`Reclaimer.Core\Geometry\GeometryExtensions.cs`
  `GetTriangleIndicies`, lines 49-57: it only un-strips `TriangleStrip`/`Default` layouts) and has no
  determinant, winding or clockwise handling anywhere (grep of `Reclaimer.Core\Geometry`,
  `Blam\Halo3`, `Blam\Common`: no hits).
- The `Int16_N4` normal decode used for the check is `i16 / 32767`, clamped (`Int16N4.cs` line 19:
  `PackedVectorHelper.CreateSignExtended(16)`; `PackedVectorHelper.cs` line 116: `SignedScale =
  2^(bits-1) - 1`, line 75: signed bits divided by it).

What each consumer must do:

| consumer | action | why |
|---|---|---|
| renderer (bevy 0.20 / wgpu) | **no flip**; hand over the decoded order | bevy_pbr 0.20 builds its mesh pipeline as `PrimitiveState { cull_mode: Some(Face::Back), ..default() }` (`bevy_pbr-0.20.0\src\render\mesh.rs` lines 3774-3780), `front_face` is left at wgpu's default `FrontFace::Ccw` (`wgpu-types-29.0.4\src\render.rs` lines 323-325, `#[default] Ccw`, "the default with right handed coordinate spaces"), and `StandardMaterial::cull_mode` defaults to `Some(Face::Back)` (`pbr_material.rs` line 933). The Z-up to Y-up change `(x, y, z) -> (x, z, -y)` is a proper rotation (det +1) and keeps the winding |
| skate collision soup | **no flip**; do NOT swap vertices 1 and 2 the way IW4L's `extract()` does | the skate engine computes `normal = (b - a) x (c - a)` and treats every triangle as `ONE_SIDED` (`skate-host\src\skate_world.rs` lines 237-246); it wants counter-clockwise seen from the standable side (`render_anim\src\skate\collision.rs` lines 49-54: "IW4 traces clockwise faces; Skate uses counterclockwise faces"). IW4's clip triangles are clockwise, Halo's are already counter-clockwise, so the IW4 swap must not be copied. `to_skate_halo` is the same proper rotation as above |
| mirrored instance placement (`det(R) < 0`, section 2.2) | swap `i1` and `i2` for that placement only | a reflection reverses the handedness of the triangle; HaloExport does exactly this (`HaloExport\Program.cs` lines 520-525: `det < 0` swaps the 2nd and 3rd index) and so does the Valhalla stand-in (`halo3_world\src\lib.rs` line 530 `flip = determinant3(&matrix) < 0.0`). **Never exercised**: all 262 riverworld and all 834 cyberdyne placements have det exactly +1.0000 (checked by the script) |
| two-sided materials | not a winding question; if a shader is two-sided, emit the triangle in both orders (what `halo3_world` does for `double_sided` materials, `lib.rs` lines 592-597) | which `rmsh` options mean two-sided is out of scope here |

### Evidence

1. **HaloExport's measurement.** `HaloExport\Program.cs` `FixNormals`
   (lines 567-590) counts, for every exported triangle with a non-zero face normal and non-zero summed
   vertex normal, whether `dot(cross(pb - pa, pc - pa), na + nb + nc) > 0`, and writes the ratio as
   `winding_agrees_with_normals` (line 1058). `<HaloExport output>\riverworld\halo_export.json`
   line 21: **1**; `...\cyberdyne\halo_export.json` line 21: **0.9996**. HaloExport measures in glTF
   space after `ToG(x, y, z) = 3.048 * (x, z, -y)` (line 465), a proper rotation, so the sign is the
   same as in Halo space; its instance meshes have already had the `det < 0` swap applied (none
   needed on these maps).

2. **Byte-level re-check straight from the `.map` files** (`check_winding.py`: decodes the Lbsp resource
   as in sections 4-6, walks every submesh of every cluster section and every instance section as a
   triangle list, and applies the same test in Halo world units, Z up; instance sections are measured
   in local space, and the first three placements again after rotating both vectors by `R`):

   | map | cluster sections: CCW-from-normal / CW | instance sections (local space): CCW / CW | mirrored placements | zero-area or zero-normal | HaloExport figure |
   |---|---|---|---|---|---|
   | riverworld (Valhalla) | **37260 / 0** (24 sections with data) | **24236 / 4** (75 sections) | 0 of 262 | 0 | 1 (script: 0.9999) |
   | cyberdyne (The Pit) | **12238 / 2** (44 sections) | **78742 / 50** (307 sections) | 0 of 834 | 0 | 0.9996 (script: 0.9994) |

   The small gap between the script's figure and HaloExport's is counting: the script counts each
   instance section once, HaloExport once per placement.

3. **The exceptions are bad normals, not reversed winding.** Riverworld's 4 clockwise triangles are all in
   section 73, used by a single placement (instance 242, the crashed Pelican: shaders
   `objects\levels\shared\pelican_crashed_h3\shaders\cr_pelican_hull`, `longsword_burn_unlit`,
   `cr_pelican_interior`). They are slivers (areas 5.4e-3 down to 7.8e-6 wu^2) whose three vertex
   normals contradict each other, e.g. `(0.105, -0.967, 0.231)`, `(0.271, -0.960, 0.065)`,
   `(-0.174, 0.786, -0.593)` on one triangle (`--detail 73`). Cyberdyne's 52 are in instance sections
   94, 97, 195, 196 and cluster sections 332, 340 (one each); not inspected further.

4. **Front = top for the ground.** On riverworld's 24 cluster sections, of the near-horizontal
   triangles (|unit face normal z| > 0.65) **21241 have `cross(p1 - p0, p2 - p0)` pointing +Z and 1534
   pointing -Z** (the undersides and ceilings of the bases); the terrain cluster 2 alone is 4065 up /
   367 down. Combined with the 100 % agreement above, the lit side of the terrain is its top, so
   "counter-clockwise from the normal side" and "counter-clockwise from the standable side" are the
   same thing.

5. **The proven chain in the Valhalla stand-in** (`mashup-halo3-valhalla`): glTF triangles (counter-
   clockwise front, the Halo order) -> `halo3_world` swaps to clockwise for IW4's tracer
   (`lib.rs` lines 586-588, comment at line 116 "wound clockwise seen from the side that blocks")
   -> IW4L's `extract()` swaps back to counter-clockwise for the skate engine (`collision.rs` lines
   49-54). Net effect: the skate engine receives the Halo order unchanged, and Valhalla and The Pit
   skate correctly. A native runtime must therefore hand the decoded order over directly.

### Remaining uncertainty

- Triangle strips (`IndexFormat 5`, and `0` treated as a strip): no BSP buffer on either map uses them,
  so the un-strip rule in section 7 (swap on every even triangle) is Reclaimer's and is assumed, not
  verified, to yield the same counter-clockwise convention.
- Only two multiplayer maps were checked. Campaign BSPs and `mode` render models go through the same
  vertex formats and the same exporter, so the rule should hold; the campaign sidecars'
  `winding_agrees_with_normals` can be read to confirm without new code.
- Deciding which shaders are two-sided (for emitting both orders) is not covered.
- This is the convention of the **render** geometry. The real collision BSP
  (`collision-bsp-research.md`) stores planes and edges in its own representation; its orientation
  rule is not covered by this section.
