# Scenario (`scnr`) tag and Reclaimer's tag-reading model, MCC Halo 3 U13

Target: MCC Halo 3 build `Dec 21 2023 22:31:37` = Reclaimer `CacheType.MccHalo3U13`.
Primary test file: `riverworld.map` (Valhalla). Written 2026-10-27.

Everything below is a **format fact** (offsets, sizes, semantics, algorithms in words).
Reclaimer (GPL-3.0, <https://github.com/Gravemind2401/Reclaimer>, commit `36224d7`) is cited
as the source so a reviewer can re-check; no Reclaimer code is reproduced. Facts marked
**VERIFIED** were checked byte-for-byte on the real map with
`docs/format/verify/scnr_riverworld.py` (`python -I`, read-only).

Container facts (header, sections, address translation, tag index, string ids) are taken as
already verified and are only repeated where a worked example needs them.

---

## 1. How Reclaimer selects bytes: the attribute model

Reclaimer describes a structure as a C# class whose properties carry attributes. A generic
reader (`Reclaimer.Core\IO\EndianReader.cs` + `Reclaimer.Core\IO\Dynamic\*`) turns those
attributes into "seek to offset, read type" operations. Nothing is laid out sequentially by
default: **every field has an explicit offset from the start of the structure.**

### 1.1 Version numbers

The "version" passed around is a `double`; for cache files it is the integer value of the
`CacheType` enum (`Reclaimer.Blam\Blam\Common\Enums.cs`). Values that matter here
(computed by position in the enum; `Unknown = -1`, everything after counts up from 0):

| CacheType | int | note |
|---|---|---|
| Halo2Beta | 6 | |
| Halo2Xbox | 7 | boundary used by TagBlock/TagReference sizes |
| Halo3Delta | 13 | |
| Halo3Beta | 14 | boundary: Halo 3-style 12-byte TagBlock, 16-byte TagReference from here on |
| Halo3Retail | 15 | Xbox 360 retail |
| MccHalo3 | 16 | |
| MccHalo3U4 | 17 | lightmap moved into separate `Lbsp` tags from here on |
| MccHalo3F6 | 18 | |
| MccHalo3U6 | 19 | |
| MccHalo3U9 | 20 | |
| MccHalo3U12 | 21 | |
| **MccHalo3U13** | **22** | **our target** (`[BuildString("Dec 21 2023 22:31:37", "U13")]`) |
| Halo3ODST | 23 | numerically *after* U13: ODST definitions never match a U13 cache |
| MccHalo3ODST .. MccHalo3ODSTU8 | 24 .. 29 | |

### 1.2 The inclusive / exclusive rule (proof)

`Reclaimer.Core\IO\Dynamic\Extensions.cs`, method `ValidateVersion(version, min, max)`
(lines 23-26) is the single predicate every versioned attribute goes through. In words:

```
matches  =  (min is unset  OR  version >= min)
        AND (max is unset  OR  version <  max  OR  (version == max AND max == min))
```

So **MinVersion is inclusive, MaxVersion is exclusive**, with one exception: when
`min == max` the range is an exact single-version match. The XML doc comments in
`Reclaimer.Core\IO\Annotations.cs` say the same ("inclusive minimum version", "exclusive
maximum version") for `FixedSizeAttribute`, `OffsetAttribute`, `ByteOrderAttribute`,
`StoreTypeAttribute`, `DataLengthAttribute`, `MinVersionAttribute`, `MaxVersionAttribute`.

Consequences for us (version = 22):

* `[Offset(x, MinVersion = (int)CacheType.MccHalo3U13)]` applies to U13 and later.
* `[Offset(x, MaxVersion = (int)CacheType.MccHalo3U13)]` applies to U12 and earlier, **not** U13.
* `AddVersion(CacheType.MccHalo3U12, null)` in a builder covers U12 **and** U13 unless a more
  specific definition exists (see 1.3).
* `AddVersion(CacheType.Halo3ODST, null)` never matches a U13 cache (23 > 22).

### 1.3 Which definition wins when several match

A class ends up with a list of `VersionDefinition`s (each = a (min,max) range + its fields +
optional fixed size + byte order). `StructureDefinition<T>` constructor
(`Reclaimer.Core\IO\Dynamic\StructureDefinition.cs` lines 342-352) sorts them:

1. ranges that have any bound before the unbounded default `(null, null)`;
2. exact versions (`min == max`) before ranges;
3. ranges with a MinVersion before ranges with none;
4. **higher MinVersion first**;
5. unbounded MaxVersion before bounded; then higher MaxVersion first.

`FindVersionDefinition(version)` (line 181) takes the **first** entry that passes the rule in
1.2. Practical reading: among open-ended `AddVersion(X, null)` definitions, **the one with the
largest X that is <= the cache version wins**; the `(null, null)` default is the fallback.

Two ways a class gets its definitions:

* **Attribute-based classes** (`[Offset]` on properties, e.g. `ObjectTag`, `ModelTag`,
  `ScenarioLightmapTag`): `StructureDefinition.FromAttributes` (lines 186-333) collects every
  version boundary mentioned on the class or its properties, sorts them, and makes one range
  per consecutive pair (plus unbounded ends and the default). For each range it keeps the
  properties whose `[MinVersion]`/`[MaxVersion]`/`[VersionSpecific]` allow the range's
  minimum (`versionTest = min ?? max - 1`), and picks the `[Offset]`, `[ByteOrder]`,
  `[StoreType]` attribute valid for that test version. A class with no versioned attributes has
  exactly one definition that matches every version (including "no version").
* **Builder-based classes** (`[StructureDefinition<T, Builder>]`, e.g. `ScenarioTag` and all
  its block types in `Blam\Halo3\Config\ScenarioTag.config.cs`): the builder calls
  `AddVersion(min, max)` / `AddVersion(exact)` / `AddDefaultVersion()` and lists
  `Property(...).HasOffset(n)` for that range; `.HasFixedSize(n)` sets the stride. When a
  builder exists **all other dynamic-IO attributes on that class are ignored**
  (`Reclaimer.Core\IO\Annotations.cs`, `StructureDefinitionAttribute` remarks).
  `DefinitionBuilder.AddVersion(Enum a, Enum b)` converts to ints and uses the same rule.

### 1.4 Attribute semantics

Source: `Reclaimer.Core\IO\Annotations.cs` unless noted.

| attribute | on | meaning |
|---|---|---|
| `[Offset(n)]` (+ `MinVersion`/`MaxVersion`) | property | byte offset of the field from the **start of its containing structure**. Required for a property to be read at all. Several `[Offset]` with disjoint version ranges may coexist; overlap throws at type load. |
| `[FixedSize(n)]` (+ versions) | class | the structure occupies exactly `n` bytes: after the fields are read the stream is moved to `origin + n` (`StructureDefinition.Populate`, line 119). This is what gives **array / tag-block element stride**. Without it the stream is left after the last field read. |
| `[MinVersion(v)]`, `[MaxVersion(v)]` | property | the property exists only for `version >= v` / `version < v`. Not inherited. |
| `[VersionSpecific(v)]` | property | property exists only for exactly `v`. |
| `[VersionNumber]` | property | this field *is* the version: if `ReadObject<T>()` is called without a version, Reclaimer first reads this field, then re-reads the structure with that version. (Not used by the Halo 3 tag classes here.) |
| `[DataLength]` | property | this integer is the structure's byte size (variable-size records). Overridden by `[FixedSize]`. |
| `[ByteOrder(o)]` (+ versions) | class or property | endianness for the field/all fields; otherwise the reader's current byte order (MCC = little). |
| `[StoreType(T)]` (+ versions) | property | read as type `T` from the stream, then convert to the property type (e.g. a `short` stored as `byte`). |
| `[FixedLength(n, Trim=..)]` | string property | exactly `n` bytes, decoded (UTF-8 reader), optional `TrimEnd()` of whitespace (`EndianReader.ReadString(int, bool)`). |
| `[NullTerminated]` | string property | read bytes until the first 0 (no fixed stride). |
| `[NullTerminated(Length = n)]` / builder `.IsNullTerminated(n)` | string property | read exactly `n` bytes, keep up to the first 0; **the stream always advances `n`** (`EndianReader.ReadNullTerminatedString(int)`). |
| `[LengthPrefixed]` | string property | i32 byte count followed by the bytes. |
| `[Interned]` | string property | `string.Intern` on the result (no format meaning). |

Which types are "primitive" (read directly, no nested structure): `Reclaimer.Core\IO\Dynamic\DelegateHelper.cs`
`IsTypeSupported`: all `TypeCode` primitives (bool, sbyte..uint64, float, double, decimal, char,
string handled separately), `Half`, `Guid`, enums (via underlying type) and any
`IBufferable<T>` struct. `RealVector3` and `EulerAngles`
(`Reclaimer.Core\Geometry\Vectors\*.cs`) are `IBufferable` with `SizeOf = 12`, `PackSize = 4`:
three consecutive little-endian floats.

Any other property type is a nested structure read with `ReadObject<TField>()` **without a
version** (`Reclaimer.Core\IO\Dynamic\DynamicFieldDefinition.cs`, `StreamRead`). See 1.6.

### 1.5 The read algorithm (what the Rust port must reproduce)

`StructureDefinition<T>.Populate(instance, reader, version, origin)`
(`StructureDefinition.cs` lines 96-127):

1. If no version was given and the type has a `[VersionNumber]` field, read it and use it.
2. Pick the version definition (1.3).
3. For every field in it: `seek(origin + field.Offset)`, read the field (field byte order,
   else the definition's, else the reader's).
4. If the definition has a fixed size: `seek(origin + size)`; else if it has a `[DataLength]`
   field: `seek(origin + thatValue)`; else leave the stream where the last read ended.

`origin` is the stream position when `ReadObject` was called, captured **before** the object is
constructed (because Blam types read bytes in their constructors, see 1.6).

### 1.6 Where the version comes from, and dependency injection

Blam's reader is `DependencyReader` (`Reclaimer.Blam\Utilities\DependencyReader.cs`), an
`EndianReader` that overrides `CreateInstance<T>`: when a type has a public constructor whose
parameters are all satisfiable (the reader itself, or a registered instance/type), that
constructor is used instead of the parameterless one. `CacheFactory.CreateReader`
(`Reclaimer.Blam\Blam\Common\CacheFactory.cs`) registers the `ICacheFile` and the
`IAddressTranslator`; `MccHalo3.CacheFile.CreateReader(translator, expander)` also registers
the `IPointerExpander`; `IndexItem.ReadMetadata` registers the `IIndexItem` being read.

This is how the Blam primitives read their own bytes in constructors
(`TagBlock(DependencyReader, ICacheFile, IAddressTranslator, IPointerExpander)`,
`TagReference(ICacheFile, EndianReader)`, `StringId(EndianReader, ICacheFile)`,
`Pointer(DependencyReader, IAddressTranslator, IPointerExpander)`).

Version flow:

| where | call | version used |
|---|---|---|
| tag root | `MccHalo3.IndexItem.ReadMetadata<T>()` (`Reclaimer.Blam\Blam\MccHalo3\CacheFile.cs`): seek to `MetaPointer.Address`, then `ReadObject<T>((int)cache.CacheType)` | **22** for U13 |
| tag-block elements | `BlockCollection<T>` constructor (`Reclaimer.Blam\Blam\Common\BlockCollection.cs`): seek to the block pointer, then `Count` times `ReadObject<T>((int)cache.CacheType)` | **22** |
| any other nested struct property | `DynamicFieldDefinition.StreamRead` -> `ReadObject<TField>()` | **none** -> only the type's unversioned/default definition applies |

Port gotcha: version-dependent offsets only take effect at tag roots and inside tag blocks.
A versioned `[Offset]` on a plain nested struct is silently resolved against the default range.
`BlockCollection` is itself a nested property of `ScenarioTag`, but it does not matter because
its bytes are consumed by the constructor, not by `[Offset]` fields.

### 1.7 Blam primitives as they apply to Halo 3 U13

| type | size | layout | source |
|---|---|---|---|
| `TagBlock` | 12 (`[FixedSize(12, MinVersion = Halo3Beta)]`) | `+0` i32 count, `+4` i32 pointer (shifted, see below), `+8` 4 bytes unused | `Blam\Common\TagBlock.cs` |
| `BlockCollection<T>` | 12 | a `TagBlock` whose elements are read immediately; **invalid** (left empty) when `count <= 0`, or the translated address is `< 0` or `>= file length` | `Blam\Common\BlockCollection.cs` |
| `Pointer` | 4 | `Address = translator.GetAddress(expander.Expand(value))`; MCC Halo 3 `Expand(p) = (p << 2) + 0`; `TagAddressTranslator.GetAddress(a) = a - Magic`, `Magic = VirtualBaseAddress - (SectionTable[2].Address + SectionOffsetTable[2])` | `Blam\Utilities\Pointer.cs`, `Blam\MccHalo3\PointerExpander.cs`, `Blam\Common\Gen3\TagAddressTranslator.cs` |
| `TagReference` | 16 (`[FixedSize(16, MinVersion = Halo3Beta)]`) | `+0` i32 class id (fourcc, bytes reversed), `+4` 8 bytes skipped, `+12` i32 tag id. `TagId = (short)(tagId & 0xFFFF)`; valid when `>= 0`. Reclaimer ignores the high 16 bits. | `Blam\Common\TagReference.cs` |
| `StringId` | 4 | i32 for engine >= Halo3 (i16 before). U13 bit layout index 19 / namespace 8 / length 5; header-table namespace translation in `Blam\Common\StringIdTranslator.cs` (header-based constructor): `index = id & 0x7FFFF`, `ns = (id >> 19) & 0xFF`, step `ns` down until a known namespace, `real = index < ns.Min ? index : index - ns.Min + ns.Start`, where namespaces 1..n-1 start at the running sum of `(table[i] & 0x7FFFF)` beginning at `table[0] & 0x7FFFF`, and namespace 0 has `Min = table[0] & 0x7FFFF`, `Start = total`. **VERIFIED**: decodes `default`, `initial` for the variant names below. | `Blam\Common\StringId.cs` |
| `IndexItem` (tag index entry) | 8 (`[FixedSize(8)]`) | `+0` i16 class index (-1 = the empty tag), `+2` i16 "Unknown", `+4` i32 meta pointer (shifted) | `Blam\MccHalo3\CacheFile.cs` |

Observation (not used by Reclaimer): on riverworld the high 16 bits of a `TagReference` tag id
equal the `+2` i16 of the referenced index item (sbsp 9757: reference id `0x8E58261D`, index
item `+2` = `0x8E58`). Treat as a salt/identifier; match tags by the low 16 bits only.

Riverworld numbers for the formulas: `VirtualBaseAddress = 0x1CC380000`,
`(SectionTable[2].Address + SectionOffsetTable[2]) mod 2^32 = 0x05753000`, so
`Magic = 0x1C6C2D000` and `file = (p << 2) - 0x1C6C2D000` for any shifted tag-data pointer.

---

## 2. The scenario tag (`scnr`) for MccHalo3U13

Source: `Reclaimer.Blam\Blam\Halo3\ScenarioTag.cs` (properties, `GetContent`) and
`Reclaimer.Blam\Blam\Halo3\Config\ScenarioTag.config.cs` (offsets). Builder versions present
for Halo 3: `(null, Halo3Delta)` empty, `Halo3Delta` exact, `Halo3Beta` exact,
`(Halo3Retail, null)`, `(MccHalo3F6, null)`, `(MccHalo3U12, null)`, `(MccHalo3U13, null)`, plus
four ODST ranges. For version 22 the rule in 1.3 selects **`(MccHalo3U13, null)`**. (The U12
definition only knows `StructureBsps`, `Skies` and the lightmap reference; U13 is the first MCC
build Reclaimer maps fully.)

### 2.1 Top-level fields (offsets from the start of the scnr tag data)

| field | offset hex | dec | type | element size (U13) | element type |
|---|---|---|---|---|---|
| StructureBsps | 0x18 | 24 | TagBlock | 108 | `StructureBspBlock` |
| Skies | 0x34 | 52 | TagBlock | 20 | `SkyReferenceBlock` |
| ObjectNames | 0xA8 | 168 | TagBlock | 36 | `ObjectNameBlock` |
| Scenery (placements) | 0xB4 | 180 | TagBlock | 180 | `SceneryPlacementBlock` |
| SceneryPalette | 0xC0 | 192 | TagBlock | 16 | `ObjectPaletteBlock` |
| Machines (placements) | 0x138 | 312 | TagBlock | 136 | `MachinePlacementBlock` |
| MachinePalette | 0x144 | 324 | TagBlock | 16 | `ObjectPaletteBlock` |
| Controls (placements) | 0x168 | 360 | TagBlock | 120 | `ControlPlacementBlock` |
| ControlPalette | 0x174 | 372 | TagBlock | 16 | `ObjectPaletteBlock` |
| Crates (placements) | 0x5BC | 1468 | TagBlock | 176 | `CratePlacementBlock` |
| CratePalette | 0x5C8 | 1480 | TagBlock | 16 | `ObjectPaletteBlock` |
| ScenarioLightmapReference | 0x6AC | 1708 | TagReference | 16 | -> `sLdT` tag |

These are **not** the Xbox 360 retail offsets (retail: 20, 48, 176, 188, 200, 320, 332, 368,
380, 1512, 1524, 1776). Do not reuse Assembly/retail plugin offsets without re-checking.

Reclaimer reads nothing else from `scnr`: **no player starting locations, no zone sets, no
vehicles/weapons/equipment/bipeds, no trigger volumes, no cluster/BSP policy.** See section 6.

**Vehicles, equipment, weapons (our own probe, 2026-10-29).** Scanning the block headers between
the scenery palette and the machines on `shrine.map` (Sandtrap) with
`scripts/probe_scnr_blocks.py`, then listing them with `scripts/probe_scnr_objects.py`:

| block | offset hex | element size | palette offset | palette tags on `shrine` |
|---|---|---|---|---|
| Vehicles | 0xE4 | 168 | 0xF0 (16 each) | 11 `vehi`: `objects\levels\multi\shrine\behemoth\behemoth` (the Elephant), mongoose, warthog, scorpion, machinegun turret, banshee, wraith, chopper, `objects\levels\multi\shrine\shrine_defender\shrine_defender`, mauler, hornet |
| Equipment | 0xFC | 140 | 0x108 (16 each) | 10 `eqip` (grenades, power-ups, deployables) |
| Weapons | 0x114 | 168 | 0x120 (16 each) | 14 `weap` |

The placement header (palette index, name index, position at +0x08, yaw/pitch/roll at +0x14,
scale at +0x20) is the same as for scenery: Sandtrap's two Elephants decode at
(-0.27, -58.56, -22.26) and (8.56, 33.12, -21.77), on the sand under the three spawn points on
each Elephant's deck (1.65 wu up), and the six defenders at z 52 on top of the six obelisks
round the desert, named `turret_north0` ... `turret_side1`. The i16 at +0x3C (the origin BSP for
scenery, `zone-sets.md` 2) is -1 for every vehicle there, and the i16 at +0x3E reads `0x0101`
(scenery `0x0106`): the object type in the high byte's place (1 vehicle, 6 scenery), as in the
object names' type field below. Vehicles under `objects\levels\` are the map's own set pieces;
the other vehicles there are the game type's.

### 2.2 Element layouts

All from `Config\ScenarioTag.config.cs`; the U13 definition is named in each row.

**StructureBspBlock**, U13 via `AddVersion(Halo3Retail, null).HasFixedSize(108)`:

| offset | type | field |
|---|---|---|
| 0x00 (0) | TagReference | BspReference (`sbsp`) |
| 0x10..0x6B | 92 bytes | not read by Reclaimer |

**SkyReferenceBlock**, `AddDefaultVersion().HasFixedSize(20)`:

| offset | type | field |
|---|---|---|
| 0x00 (0) | TagReference | SkyReference (a `scen` tag; its render model is the sky) |
| 0x10 (16) | 4 bytes | not read |

**ObjectNameBlock**, `AddDefaultVersion().HasFixedSize(36)`:

| offset | type | field |
|---|---|---|
| 0x00 (0) | 32-byte null-terminated string (`IsNullTerminated(32)`) | Name |
| 0x20 (32) | i16 | ObjectType (same enum as the `obje` tag's ObjectType, see 3) |
| 0x22 (34) | i16 | PlacementIndex into the placement block of that type |

**ObjectPaletteBlock**, U13 via `AddVersion(MccHalo3U13, null).HasFixedSize(16)` (48 bytes for
Halo3Delta..U12; Reclaimer's comment "TODO: confirm which version this changed in"):

| offset | type | field |
|---|---|---|
| 0x00 (0) | TagReference | TagReference (`scen` / `mach` / `ctrl` / `bloc`) |

**Placement blocks** share `PlacementBlockBase.DefinitionBuilderBase.Common` +
`ObjectPlacementBlockBase...Halo3Retail` (VariantName at 84). U13 element sizes:

| block | U13 definition | size |
|---|---|---|
| SceneryPlacementBlock | `AddVersion(Halo3Retail, null)` | **180** (0xB4) |
| MachinePlacementBlock | `AddVersion(MccHalo3U13, null)` | **136** (0x88) (retail 112) |
| ControlPlacementBlock | `AddVersion(MccHalo3U13, null)` | **120** (0x78) (retail 100) |
| CratePlacementBlock | `AddVersion(Halo3Retail, null)` | **176** (0xB0) |

Fields inside every placement element (identical for the four kinds):

| offset hex | dec | type | field | semantics |
|---|---|---|---|---|
| 0x00 | 0 | i16 | PaletteIndex | index into the matching palette block; **-1 = empty slot** (seen in `020_base`; Reclaimer's `ElementAtOrDefault` yields null and the placement is skipped) |
| 0x02 | 2 | i16 | NameIndex | index into ObjectNames, -1 = unnamed |
| 0x04 | 4 | 4 bytes | not read (flags in other tools' plugins; unverified) | |
| 0x08 | 8 | 3 x f32 | Position (x, y, z) | world units, 1 wu = 10 ft = 3.048 m (`BlamConstants.WorldUnitScale = 10 * StandardUnits.Feet`), right-handed, +Z up |
| 0x14 | 20 | 3 x f32 | Rotation (yaw, pitch, roll) | radians; see 2.3 |
| 0x20 | 32 | f32 | Scale | **0 means 1** (`ObjectPlacement.SetTransform`: `scale == 0 ? 1 : scale`) |
| 0x24..0x53 | 36..83 | 48 bytes | not read | |
| 0x54 | 84 | StringId (i32) | VariantName | 0 = none / default |
| 0x58.. | 88.. | rest of element | not read | |

Note the layout is identical for scenery/crate/machine/control up to +88; only the element
stride differs. `ObjectNames.ObjectType`/`PlacementIndex` and `NameIndex` point at each other
(**VERIFIED** on `020_base`: 199/199 scenery, 82/82 crate, 50/50 machine, 22/22 control names
round-trip).

### 2.3 Rotation convention and the transform Reclaimer builds

Storage: three floats at +20, Reclaimer type `EulerAngles(Yaw, Pitch, Roll)`
(`Reclaimer.Core\Geometry\Vectors\EulerAngles.cs`): yaw about the up axis (Z), pitch about
the left/right axis (Y), roll about the front/back axis (X), radians.

Quaternion (same file, `explicit operator Quaternion`, with the comment "currently assuming
DX coordinate system"):

```
q = AxisAngle(+X, roll)  *  AxisAngle(-Y, pitch)  *  AxisAngle(+Z, yaw)      // System.Numerics '*'
```

Transform (`Reclaimer.Core\Geometry\Scene.cs`, `ObjectPlacement.SetTransform(scale, pos, q)`):

```
M = Scale(s)  *  FromQuaternion(q)  *  Translation(pos)      // System.Numerics Matrix4x4, row-vector convention
```

`ScenarioTag.ConfigurePlacements` calls it as `SetTransform(placement.Scale,
placement.Position, (Quaternion)placement.Rotation)`; HaloExport (`HaloExport\Program.cs`,
`ExportObjects`) builds the identical matrix. Row-vector convention means `world = local * M`
and the translation sits in row 4 (`M41..M43`).

What the quaternion expression actually does, measured with
`docs/format/verify/rotcheck/Program.cs` (.NET 9, `System.Numerics`), **VERIFIED**:

* `System.Numerics` quaternion product applies the **right-hand factor first**:
  `(Rx90 * Rz90)` sends +X to +Z (Rz first, then Rx).
* Therefore Reclaimer rotates a model by **yaw about world Z first, then pitch about world -Y,
  then roll about world X** (all extrinsic / world-axis rotations in that order).

| yaw, pitch, roll (deg) | Reclaimer: local +X -> | local +Z -> | Blam forward formula `(cos y cos p, sin y cos p, sin p)` |
|---|---|---|---|
| 90, 30, 0 | (0, 1, 0) | (-0.5, 0, 0.866) | (0, 0.866, 0.5) **differs** |
| 0, 30, 0 | (0.866, 0, 0.5) | (-0.5, 0, 0.866) | same |
| 0, 90, 0 | (0, 0, 1) | (-1, 0, 0) | same |
| 0, 0, 90 | (1, 0, 0) | (0, -1, 0) | same |
| -87.53, 0, 0 (riverworld scenery[0]) | (0.043, -0.999, 0) | (0, 0, 1) | same |
| -180, 90, -180 (zanzibar dome_light) | (0, 0, 1) | (-1, 0, 0) | same forward |

Reading of this: positive pitch is nose-up (matches Blam's `sin(pitch)` up component), and
Reclaimer agrees with the usual Blam convention (intrinsic yaw, then pitch, then roll;
equivalently column-vector `Rz(yaw) * Ry(-pitch) * Rx(roll)`) **only while at most one angle is
non-zero**. With yaw and pitch (or yaw and roll) both non-zero the two orders give different
orientations. Riverworld has 83 scenery and 48 crate placements with |pitch| or |roll| > 0.01
rad, mostly small tilts on spawn points and loose crates, so HaloExport/IW4L stand-in maps
inherit Reclaimer's order for those. Which order `halo3.dll` uses is **open** (section 7);
the Rust port should keep the composition order switchable and check one tilted crate in game.

**Settled by the map data (2026-10-30): Reclaimer's order.** 120_halo's Forward Unto Dawn
(`trench_frigate`, a machine) is placed with yaw 180 degrees and pitch +0.0428 rad (2.45
degrees) over the end of the Warthog run, and its hangar is the run's goal, so the fit shows
which order the level was built with (`halo3_anim/tests/frigate.rs`):

| | intrinsic `Rz(yaw) Ry(-pitch) Rx(roll)` | extrinsic (Reclaimer) `Rx(roll) Ry(-pitch) Rz(yaw)` |
|---|---|---|
| stern underside over the run's last ramp (lip at z -71.2) | z -72.1: the ramp runs into the hull | z -66.4: 4.9 wu clear |
| hangar floor, mouth / inner end | -79.5 / -78.1 (tips down to the mouth) | -76.5 / -77.9 |
| the end trigger `end_trilogy01`, model z | -16.7 .. -8.7 (half inside the stern) | -20.1 .. -12.1 (the mouth is -20 .. -12) |

So `scenario::rotation` composes yaw about world Z, then pitch about world -Y, then roll about
world X. (With yaw 180 the two orders tip the object opposite ways, which is why this
placement tells them apart.) Riverworld's tilted spawn markers and crates did not tell them
apart (vertex coincidence with neighbours is the same within 8 of 182,000 vertices).

Full worked transform for riverworld scenery[0] (yaw -1.52768, scale 0 -> 1,
pos (79.7632, -56.7296, 1.5002)), Reclaimer/System.Numerics layout:

```
row1   0.0431  -0.9991   0.0000   0
row2   0.9991   0.0431   0.0000   0
row3   0.0000   0.0000   1.0000   0
row4  79.7632 -56.7296   1.5002   1
```

### 2.4 What `ScenarioTag.GetContent` does with the blocks

(`Blam\Halo3\ScenarioTag.cs`) The scene is: one group of BSP models (each valid, distinct
`StructureBsps[i].BspReference` read as `ScenarioStructureBspTag`), one group of skies (each
`Skies[i].SkyReference` read as `SceneryTag`, then its render model), then for scenery,
machines, controls and crates: group placements by PaletteIndex, resolve the palette tag ->
`ObjectTag.ReadRenderModel()` (section 3), skip the whole group if the model fails, else one
`ObjectPlacement` per placement with the transform of 2.3. Scene coordinate system =
default scaled by `WorldUnitScale` (3.048 m per wu).

### 2.5 Lightmap chain (as far as `scnr` is concerned)

* `scnr +0x6AC (1708)`: TagReference to the `sLdT` (scenario_lightmap) tag.
  **VERIFIED** riverworld: `sLdT` tag 6878 `levels\multi\riverworld\riverworld_faux_lightmap`.
* `ScenarioLightmapTag` (`Blam\Halo3\ScenarioLightmapTag.cs`), attribute-based:
  `+4` is `BlockCollection<ScenarioLightmapBspDataTag>` for `[MaxVersion(MccHalo3U4)]`
  (embedded data, Xbox/early MCC) and `BlockCollection<TagReference>` `LightmapRefs` for
  `[MinVersion(MccHalo3U4)]` -> **U13 reads a block of 16-byte TagReferences at +4**, each to an
  `Lbsp` (scenario_lightmap_bsp_data) tag.
* `ScenarioLightmapBspDataTag` (`Blam\Halo3\ScenarioLightmapBspDataTag.cs`): `+2` i16
  BspIndex, `+308 (0x134)` BlockCollection of `SectionBlock`, `+428 (0x1AC)` ResourceIdentifier;
  `[FixedSize(436)]` only applies below U4 (when it was an embedded block).
* `ScenarioStructureBspTag.GetModelContent` (`Blam\Halo3\ScenarioStructureBspTag.cs` lines
  38-57): `bspIndex` = position of the sbsp's reference inside `scnr.StructureBsps`; for
  `CacheType >= MccHalo3U4` pick the `Lbsp` whose `BspIndex == bspIndex`, falling back to the
  `Lbsp` tag whose tag name equals the sbsp's tag name. The lightmap tag supplies the render
  `Sections` and the geometry resource pointer for the BSP (the sbsp reader's topic).

---

## 3. The object tag chain: `obje` -> `hlmt` -> `mode`

`ObjectTag` (`Blam\Halo3\ObjectTag.cs`) is the shared base for `SceneryTag` (`scen`),
`CrateTag` (`bloc`), `DeviceMachineTag` (`mach`), `DeviceControlTag` (`ctrl`) (the four
derived files are empty). Attribute-based, **no version attributes** -> same offsets for every
Halo 3 cache; read at the tag's meta address.

| offset hex | dec | type | field | note |
|---|---|---|---|---|
| 0x00 | 0 | i16 | ObjectType | **VERIFIED**: 6 for every `scen`, 7 `mach`, 8 `ctrl`, 10 `bloc` (riverworld, zanzibar, 020_base). `ObjectNames.ObjectType` histogram on 020_base also shows 0, 1, 2, 3, 9, 13 (by the usual Blam order: biped 0, vehicle 1, weapon 2, equipment 3, sound scenery 9, effect scenery 13; inferred, not verified). |
| 0x30 | 48 | StringId | DefaultVariant | e.g. `default`, `initial` |
| 0x34 | 52 | TagReference | Model (`hlmt`) | |
| 0x44 | 68 | TagReference | CrateObject | |
| 0x54 | 84 | TagReference | CollisionDamage | |

`ModelTag` (`Blam\Halo3\ModelTag.cs`, `hlmt`), also unversioned:

| offset hex | dec | type | field |
|---|---|---|---|
| 0x00 | 0 | TagReference | RenderModel (`mode`) |
| 0x10 | 16 | TagReference | CollisionModel (`coll`) |
| 0x20 | 32 | TagReference | Animation (`jmad`) |
| 0x30 | 48 | TagReference | PhysicsModel (`phmo`) |
| (0x64 / 100) | | BlockCollection | Variants, commented out in Reclaimer |

`ObjectTag.ReadRenderModel()` = `Model` -> read `ModelTag` -> `RenderModel` -> read
`RenderModelTag`; any null link returns null and the placement group is dropped.

**VERIFIED** riverworld chain for scenery palette[0]: `scen` 171
`objects\multi\spawning\respawn_point` (ObjectType 6, DefaultVariant `default`) -> `hlmt` 172
`objects\multi\models\mp_spawn_point\mp_spawn_point` -> `mode` 173 (same path). Class codes of
every Model reference were `hlmt` and every RenderModel reference `mode` across all palettes of
the three maps checked.

---

## 4. EXPECTED VALUES, riverworld.map (file size 0x0952C000)

All **VERIFIED** with `python -I docs/format/verify/scnr_riverworld.py`.

### 4.1 Locating the tag

| item | value |
|---|---|
| build string | `Dec 21 2023 22:31:37` |
| tag count / string count / namespaces | 11658 (0x2D8A) / 18806 (0x4976) / 10 |
| tag magic (`VirtualBase - 0x05753000`) | 0x1C6C2D000 |
| tag index file offset | 0x0591CAF0 (183 classes; class table file 0x0591BF80; tag table file 0x05905330) |
| scnr tag index | 8; name `levels\multi\riverworld\riverworld`; index item bytes `83 00 7E E1 56 58 1D 73` (class index 0x83 = 131, +2 = 0xE17E, meta ptr 0x731D5856) |
| scnr tag data file offset | (0x731D5856 << 2) - 0x1C6C2D000 = **0x05B29158** |

### 4.2 Block headers (raw 12 bytes at scnr + offset)

| block | raw bytes | count | pointer -> file |
|---|---|---|---|
| StructureBsps +24 | `01 00 00 00 4E 00 1D 73 00 00 00 00` | 1 | 0x731D004E -> 0x05B13138 |
| Skies +52 | `01 00 00 00 69 00 1D 73 00 00 00 00` | 1 | 0x05B131A4 |
| ObjectNames +168 | `01 00 00 00 10 02 1D 73 00 00 00 00` | 1 | 0x05B13840 |
| Scenery +180 | `8E 00 00 00 19 02 1D 73 00 00 00 00` | **142** | 0x731D0219 -> **0x05B13864** |
| SceneryPalette +192 | `1A 00 00 00 0F 1B 1D 73 00 00 00 00` | **26** | 0x05B19C3C |
| Machines +312 / palette +324 | | 0 / 0 | |
| Controls +360 / palette +372 | | 0 / 0 | |
| Crates +1468 | `66 00 00 00 BD 40 1D 73 00 00 00 00` | **102** | 0x731D40BD -> **0x05B232F4** |
| CratePalette +1480 | `2B 00 00 00 45 52 1D 73 00 00 00 00` | **43** | 0x05B27914 |
| Lightmap ref +1708 | `54 64 4C 73 00 00 00 00 00 00 00 00 DE 1A 54 FC` | class `sLdT`, id 0xFC541ADE -> tag **6878** `levels\multi\riverworld\riverworld_faux_lightmap` |

### 4.3 Contents

* `StructureBsps[0]` raw `70 73 62 73 | 00 x8 | 1D 26 58 8E`: class `sbsp`, tag id 0x8E58261D ->
  index **9757**, name **`levels\multi\riverworld\riverworld`**. Exactly 1 BSP, as expected.
* `Skies[0]`: `scen` tag 11 `levels\multi\riverworld\sky\riverworld`.
* `ObjectNames[0]`: name `""`, ObjectType 3, PlacementIndex 30 (an equipment placement; the
  equipment block is not read by Reclaimer).
* Scenery: palette 26, placements **142**; crates: palette 43, placements **102**. Every
  palette entry resolves through `obje -> hlmt -> mode`, so HaloExport's "142 scenery nodes,
  102 crates" equal the raw placement counts on this map (on `020_base` it would be 581 of
  604 scenery placements, because 2 of 71 palette entries have no render model).
* All 142 + 102 placements pass the plausibility filter (palette index in range, name index in
  [-1, 1), scale in [0, 10], finite position/rotation), which confirms the 180/176 strides.
* Palette[0..7] are all multiplayer spawn/zone markers (`respawn_point`,
  `slayer_initial_spawn_point`, `oddball_initial_spawn_point`, `slayer_respawn_zone`,
  `ctf_flag_away_respawn_zone`, `ctf_flag_at_home_respawn_zone`, `ctf_respawn_zone`,
  `ctf_initial_spawn_point`), i.e. **MP spawn points live in the scenery block on this map**.

### 4.4 First 5 scenery placements (palette, name, position wu, rotation rad / deg, scale, variant)

| i | file | palette -> tag | name | position (x, y, z) | rotation (yaw, pitch, roll) | scale | variant |
|---|---|---|---|---|---|---|---|
| 0 | 0x05B13864 | 0 -> `objects\multi\spawning\respawn_point` | -1 | (79.7632, -56.7296, 1.5002) | (-1.52768, 0, 0) = (-87.53 deg, 0, 0) | 0 (=1) | 0 |
| 1 | 0x05B13918 | 0 -> same | -1 | (80.3468, -56.7353, 1.5002) | (-1.52768, 0, 0) = -87.53 deg | 0 | 0 |
| 2 | 0x05B139CC | 0 -> same | -1 | (82.8028, -56.7262, 1.5002) | (-1.62594, 0, 0) = -93.159 deg | 0 | 0 |
| 3 | 0x05B13A80 | 0 -> same | -1 | (83.3749, -56.7363, 1.5002) | (-1.62348, 0, 0) = -93.019 deg | 0 | 0 |
| 4 | 0x05B13B34 | 0 -> same | -1 | (82.9796, -59.0567, -0.0498) | (1.98901, -0, 0) = 113.962 deg | 0 | 0 |

Raw bytes of scenery[0], +0..+0x23:
`00 00 | FF FF | 00 00 00 00 | C2 86 9F 42  21 EB 62 C2  FA 07 C0 3F | 20 8B C3 BF  E5 20 6A 30  61 1D 48 36 | 00 00 00 00`
= palette 0, name -1, 4 unread bytes, position, rotation (pitch/roll are ~1e-9 / ~3e-6, i.e.
zero), scale 0. Bytes +0x54..+0x57 (VariantName) = `00 00 00 00`.

Tilted placements for a future in-game rotation check (2.3): scenery at file 0x05B13E04
`respawn_point` pos (58.012, -59.468, 3.802) rot (-0.2407, -0.0903, 0.3009); crate at
0x05B23664 `objects\gear\human\military\crate_packing\crate_packing` pos (55.278, -94.43, 3.378)
rot (-0.0028, 0.0255, 0.0692); crate at 0x05B23714 `generator` rot (2.0748, -0.0249, -0.0006).

### 4.5 Cross-checks on other U13 maps (same script, same offsets)

| map | BSPs | object names | scenery pal/placed | crates | machines | controls | notes |
|---|---|---|---|---|---|---|---|
| `zanzibar.map` (Last Resort) | 1 | 2 | 24 / 178 | 42 / 168 | **3 / 3** | **1 / 1** | validates the 136 and 120 strides; machine/control chains all resolve |
| `020_base.map` (campaign) | **10** (`020_bsp_000` .. `020_bsp_110`, all `sbsp`) | 411 | 71 / 604 | 64 / 833 (1 with palette -1) | 15 / 82 | 3 / 24 | validates the 108 BSP stride and the ObjectNames round-trip |

---

## 5. Re-running the checks

```
cd docs\format\verify
python -I scnr_riverworld.py                      # riverworld (default), prints sections 4.1-4.4
python -I scnr_riverworld.py "<path>\zanzibar.map"
dotnet run --project rotcheck\rotcheck.csproj -c Release   # rotation probe (2.3)
```

Both tools only read the map files. The rotation probe re-expresses the one-line quaternion
formula from `EulerAngles.cs` in order to measure it; no Reclaimer code is copied.

---

## 6. Things Reclaimer does not read (and what to do instead)

* **Player starting locations / spawn points**: no `scnr` field for them in Reclaimer. On
  riverworld the usable spawns are the `objects\multi\spawning\respawn_point` and
  `*_initial_spawn_point` scenery placements (section 4.3), which is how Halo 3 multiplayer
  places players. Campaign `player_starting_locations` offsets must come from another source
  (Assembly plugin / TagTool) and be byte-verified before use.
* **Zone sets, BSP switching policy, cluster/BSP index per placement**: not read. The 4 unread
  bytes at placement +4 and the 48 bytes at +36..+83 almost certainly include the
  per-placement BSP policy/flags, but no offset is verified here.
* **Vehicles, weapons, equipment, bipeds, sound scenery, effect scenery, trigger volumes,
  scripts**: not read by Reclaimer. Our reader places the vehicles under `objects\levels\`
  (block offsets in 2.1, found by our own probe) and reads the trigger volumes
  (`zone-sets.md` 1); the rest are still not read.
* **Collision**: the `hlmt +16` `coll` reference is read but never used by Reclaimer; the real
  BSP collision is in the sbsp resources (known gap, see `HANDOFF.md`).

---

## 7. Open questions

1. Rotation composition order of `halo3.dll` for placements with two or more non-zero angles
   (2.3). Reclaimer/HaloExport use "yaw, then pitch about world -Y, then roll about world X";
   the standard Blam convention is the reverse order. Decide by inspecting a tilted crate in game.
2. Meaning of placement bytes +4..+7 and +36..+83 (flags, BSP policy, editor folder, etc.).
3. Which MCC update changed `ObjectPaletteBlock` from 48 to 16 bytes and the `scnr` offsets
   from the retail layout (Reclaimer only asserts "U13"). Irrelevant for a U13-only reader, but
   it means the U13 offsets must not be applied to older MCC caches.
4. Whether the "ObjectType" enum values 0, 1, 2, 3, 9, 13 seen in `020_base` object names map
   to biped, vehicle, weapon, equipment, sound_scenery, effect_scenery as assumed.
5. Whether the high 16 bits of a tag id (observed equal to index item +2) matter to the engine
   or are a pure salt.

---

## Gap fill: render_model (mode) tag layout for objects and the Spartan rig

Added 2026-10-27. Picks up where section 3 stops (`hlmt +0` -> `mode`). Source of every format
fact: `Reclaimer.Blam\Blam\Halo3\RenderModelTag.cs` (tag class and the block classes),
`Blam\Halo3\Halo3Common.cs` (`GetMeshes`, node maps), `Blam\Common\CacheFactory.cs`
(`CreateReader`, matrix reader), `Blam\Utilities\Utils.cs` (`CreateMatrix`, `CreateWorldMatrix`),
`Reclaimer.Core\IO\IOExtensions.cs` (`ReadMatrix3x4`), all at Reclaimer commit `36224d7`
(GPL-3.0; facts only, no code reproduced). Everything marked **VERIFIED** was checked on
`riverworld.map` with three read-only probes in `docs/format/verify/`:

```
python -I mode_riverworld.py              # tag layout dump of mode 173 + mp_masterchief, then a survey of all 322 mode tags
python -I mode_resource_decode.py         # decodes the two models' vertex/index buffers from the resource system
python -I mode_extra_checks.py            # geometry instances, node maps, masterchief 'base' permutation, markers
```

### G3.1 Version rule and what it selects for U13

The predicate is the one proven in 1.2 (`Reclaimer.Core\IO\Dynamic\Extensions.cs` lines 23-26):
**MinVersion inclusive, MaxVersion exclusive**, exact match when `min == max`. `RenderModelTag`
is attribute-based (1.3), read at the tag root and inside `BlockCollection`s with version 22, so
versioned attributes do take effect. Only two things in the file are versioned:

| class | attribute | U13 (22) result |
|---|---|---|
| `PermutationBlock` | `[FixedSize(16, MaxVersion = MccHalo3U9)]` / `[FixedSize(24, MinVersion = MccHalo3U9)]`; `InstanceMask3`/`InstanceMask4` carry `[MinVersion(MccHalo3U9)]` | 20 <= 22 -> **24 bytes**, masks 3 and 4 present. The 16-byte form covers MccHalo3 .. MccHalo3U6 (16..19) only. |
| `SectionBlock` | `[FixedSize(56, MaxVersion = Halo3Delta)]` / `[FixedSize(76, MinVersion = Halo3Delta)]` plus the per-field offsets listed in the sbsp doc 3.1 | **76 bytes**, same as the Lbsp sections |

Correction to the gap question: the block that is 16 bytes before U9 and 24 from U9 on is the
**permutation** block, not the node block. `NodeBlock` is `[FixedSize(96)]` on every version.
All other blocks and the tag-root offsets are unversioned (identical for every Halo 3 cache).
**VERIFIED** 24-byte permutations: on all 322 mode tags of riverworld every permutation name
decodes as a string id under a 24-byte stride; where two regions' permutation blocks are laid out
back to back, 101 of 143 gaps equal `count * 24` and **none** equals `count * 16`.

### G3.2 Tag root (offsets from the start of the `mode` tag data)

| offset hex | dec | type | field | note |
|---|---|---|---|---|
| 0x00 | 0 | StringId | Name | model's own name, e.g. `mp_spawn_point`, `mp_masterchief` (**VERIFIED**, 322/322 decode) |
| 0x04 | 4 | i32 flags | Flags | only bit 2 (value 4) is named: `UseLocalNodes`. Reclaimer's Halo 3 reader never tests it (it relies on NodeMaps instead, G3.6) |
| 0x08 | 8 | 4 bytes | not read | |
| 0x0C | 12 | TagBlock | **Regions** | 16-byte `RegionBlock` (G3.3). (Not +28.) |
| 0x18 | 24 | 4 bytes | not read | |
| 0x1C | 28 | i32 | InstancedGeometrySectionIndex | **-1 = no instanced geometry**; else the index of the one section whose subsets are the instances (G3.7) |
| 0x20 | 32 | TagBlock | GeometryInstances | 60-byte `GeometryInstanceBlock` |
| 0x2C | 44 | 4 bytes | not read | |
| 0x30 | 48 | TagBlock | **Nodes** | 96-byte `NodeBlock` (G3.4) |
| 0x3C | 60 | TagBlock | MarkerGroups | 16-byte `MarkerGroupBlock` (G3.8) |
| 0x48 | 72 | TagBlock | Shaders | 36-byte `ShaderBlock`: TagReference at +0, bytes 16..35 not read (all zero in 430 entries checked) |
| 0x54 | 84 | 20 bytes | not read | |
| 0x68 | 104 | TagBlock | Sections | 76-byte `SectionBlock`, identical to the Lbsp layout in `sbsp-render-geometry.md` 3.1-3.3 (submeshes 16 bytes, subsets 8 bytes) |
| 0x74 | 116 | TagBlock | BoundingBoxes | 56-byte `BoundingBoxBlock`; **one entry for the whole model** (count 1 on both test models), applied to every section |
| 0x80 | 128 | 48 bytes | not read | |
| 0xB0 | 176 | TagBlock | NodeMaps | 12-byte `NodeMapBlock` = one TagBlock of bytes per section |
| 0xBC | 188 | 36 bytes | not read | |
| 0xE0 | 224 | i32 | ResourcePointer | render-geometry resource id; low 16 bits = index into `zone` ResourceEntries (`resources.md` 2). **VERIFIED** the entry's owner tag is the mode tag itself, 322/322 |

### G3.3 Block layouts

**RegionBlock, 16 (0x10) bytes**: `0x0` StringId Name, `0x4` TagBlock Permutations.

**PermutationBlock, 24 (0x18) bytes on U13**:

| offset | type | field | semantics |
|---|---|---|---|
| 0x0 (0) | StringId | Name | `default`, `base`, `legacy`, `mp_recon`, ... |
| 0x4 (4) | i16 | SectionIndex | first section of this permutation; **-1 = the permutation has no geometry in this cache** (**VERIFIED** on 30 of the 309 masterchief permutations, e.g. `arms/legacy`, `chest_shared/empty`) |
| 0x6 (6) | i16 | SectionCount | number of consecutive sections (1 everywhere on riverworld) |
| 0x8 (8) | i32 | InstanceMask1 | bit `i % 32` of mask `i / 32` set = this permutation includes geometry instance `i` (Reclaimer `HasInstance`). All masks are 0 on every riverworld model; the 11 models that have instances have **no regions at all**, so the mask semantics are Reclaimer's claim only |
| 0xC (12) | i32 | InstanceMask2 | |
| 0x10 (16) | i32 | InstanceMask3 | U9+ only |
| 0x14 (20) | i32 | InstanceMask4 | U9+ only |

**GeometryInstanceBlock, 60 (0x3C) bytes**: `0x0` StringId Name, `0x4` i32 NodeIndex, `0x8` f32
TransformScale, `0xC` 3x4 matrix (48 bytes, layout below). **VERIFIED** on the 11 decorator models
(`levels\shared\decorators\wildgrass\wildgrass` etc.): all names decode (`%wildgrassd`, `%rock_c`,
...), node 0, scale 1, identity rotation, small translations.

**Matrix layout** (`NodeBlock.InverseTransform`, `GeometryInstanceBlock.Transform`): Reclaimer
declares these as `Matrix4x4`, but `CacheFactory.CreateReader` registers `ReadMatrix3x4` as the
reader for that type on every cache `>= Halo2Xbox`, so a matrix field is **12 floats = 48 bytes**
in the order M11 M12 M13, M21 M22 M23, M31 M32 M33, M41 M42 M43, with M14 = M24 = M34 = 0 and
M44 = 1. Row-vector convention: rows 1-3 are the images of the X, Y, Z axes, row 4 is the
translation; a column-vector matrix is its transpose. **VERIFIED**: read this way, every node's
bind-pose world matrix times its InverseTransform is the identity (within 2e-3 on all 1078 nodes
of 322 models; 8e-7 on the 51 Spartan nodes, exactly 0 on the spawn point's root).

**NodeBlock, 96 (0x60) bytes**:

| offset hex | dec | type | field | semantics |
|---|---|---|---|---|
| 0x00 | 0 | StringId | Name | `pelvis`, `l_thigh`, ... |
| 0x04 | 4 | i16 | ParentIndex | -1 = root. **VERIFIED** parent index < own index on all 1078 nodes, so one forward pass builds world matrices |
| 0x06 | 6 | i16 | FirstChildIndex | -1 = none; **VERIFIED** `nodes[child].parent == self` |
| 0x08 | 8 | i16 | NextSiblingIndex | -1 = none; **VERIFIED** same parent |
| 0x0A | 10 | i16 | not read | 0 on every node seen ("import node index" in other tools, unverified) |
| 0x0C | 12 | 3 x f32 | Position | local to the parent, wu |
| 0x18 | 24 | 4 x f32 | Rotation | quaternion **x, y, z, w** (w last), local to the parent. **VERIFIED** unit length on all nodes; roots often store w = -1 (same rotation as identity) |
| 0x28 | 40 | f32 | InverseScale | 1.0 on every riverworld node; Reclaimer ignores it ("TODO: incorporate scale") |
| 0x2C | 44 | 3x4 matrix | InverseTransform | **inverse of the node's bind-pose world (model-space) matrix**: model space -> node space |
| 0x5C | 92 | f32 | DistanceFromParent | **VERIFIED** = `|Position|` for non-roots, 0 for roots |

Transforms, in Reclaimer's words: `LocalTransform = CreateFromQuaternion(q) * CreateTranslation(p)`
(System.Numerics row-vector product: rotate, then translate) and `WorldTransform = inverse(InverseTransform)`.
In column-vector form: `local = T(position) * R(quaternion)`, `world = world[parent] * local`.
**VERIFIED** that this chain reproduces `inverse(InverseTransform)` for all 1078 nodes.

**MarkerGroupBlock, 16 bytes**: `0x0` StringId Name, `0x4` TagBlock Markers.
**MarkerBlock, 36 (0x24) bytes**: `0x0` u8 RegionIndex, `0x1` u8 PermutationIndex (255 = any),
`0x2` u8 NodeIndex, `0x3` not read (0), `0x4` 3 x f32 Position, `0x10` (16) 4 x f32 Rotation
(x, y, z, w), `0x20` (32) f32 Scale (0.0 on every marker seen). Position and rotation are
**relative to the marker's node** (G3.8).

**ShaderBlock, 36 bytes**: `0x0` TagReference (16 bytes) to the `rm**` shader tag; rest unread.

**BoundingBoxBlock, 56 (0x38) bytes**: `0x0` u32 not read (3 on both test models), `0x4` X
(min, max), `0xC` (12) Y, `0x14` (20) Z, `0x1C` (28) U, `0x24` (36) V, `0x2C` (44) 12 bytes
not read. Same decompression rule as the sbsp doc section 6, but `BoundingBoxes[0]` is used for
**every** section of the model.

**NodeMapBlock, 12 bytes**: one TagBlock whose elements are single bytes (`0x0` i32 count,
`0x4` pointer): entry `i` belongs to section `i`.

### G3.4 The Spartan rig: `objects\characters\masterchief\mp_masterchief\mp_masterchief`

riverworld tag **8096**, meta file offset `0x86CD400`, 51 nodes at file `0x86BDC4C`. Units wu,
+Z up, origin between the feet: pelvis at z 0.3944 wu (1.20 m), head node at z 0.609 wu
(1.86 m), model bounds Z `[-0.0052, 0.8028]` wu (helmets). Bone convention: a child sits on its
parent's **+X axis** (`l_calf` at (0.1803, 0, 0) from `l_thigh`), i.e. +X runs along the bone;
the pelvis quaternion (0.5, 0.5, 0.5, -0.5) maps local +X to model +Z (up), local +Y to model +X
(forward) and local +Z to model +Y (left), so `l_thigh` at local (0, 0, +0.0422) is on the
Spartan's left. Local positions below are exact map values (4 decimals).

| idx | name | parent | local position (x, y, z) wu | idx | name | parent | local position |
|---|---|---|---|---|---|---|---|
| 0 | pelvis | -1 | (0, 0, 0.3944) | 11 | r_clavicle | 6 | (0.0635, 0, -0.0598) |
| 1 | l_thigh | 0 | (0, 0, 0.0422) | 12 | l_toe | 7 | (0.0682, 0.0576, 0) |
| 2 | r_thigh | 0 | (0, 0, -0.0422) | 13 | r_toe | 8 | (0.0682, 0.0576, 0) |
| 3 | spine | 0 | (0.0374, -0.0001, 0) | 14 | l_upperarm | 9 | (0.0304, 0, 0) |
| 4 | l_calf | 1 | (0.1803, 0, 0) | 15 | head | 10 | (0.0172, 0, 0) |
| 5 | r_calf | 2 | (0.1803, 0, 0) | 16 | r_upperarm | 11 | (0.0304, 0, 0) |
| 6 | spine1 | 3 | (0.0740, -0.0001, 0) | 17 | l_forearm | 14 | (0.1086, 0, 0) |
| 7 | l_foot | 4 | (0.1563, 0, 0) | 18 | r_forearm | 16 | (0.1086, 0, 0) |
| 8 | r_foot | 5 | (0.1563, 0, 0) | 19 | l_hand | 17 | (0.1150, -0.0003, 0) |
| 9 | l_clavicle | 6 | (0.0635, 0, 0.0598) | 20 | r_hand | 18 | (0.1150, -0.0003, 0) |
| 10 | neck | 6 | (0.0861, 0, 0) | | | | |

Nodes 21-50 are the fingers, in a fixed pattern: 21-25 `l_{index,middle,pinky,ring,thumb}_low`
(parent 19), 26-30 the same for `r_` (parent 20), 31-35 `l_*_mid` (parents 21-25), 36-40
`r_*_mid` (26-30), 41-45 `l_*_tip` (31-35), 46-50 `r_*_tip` (36-40). The other biped-class
models in riverworld: `mp_elite\mp_elite` 55 nodes (largest rig on the map), `fp_body` 51,
`fp` 37 (first-person arms), `gear` 1.

Suggested correspondence to the 35 skate bones of `docs/skate-interface.md` section 4 (a
starting point for `rig.rs`, not verified against any animation): HIPS = pelvis, SPINE = spine,
SPINE1 = spine1, NECK = neck, HEAD = head, LEFTSHOULDER = l_clavicle, LEFTARM = l_upperarm,
LEFTFOREARM = l_forearm, LEFTHAND = l_hand, LEFTUPLEG = l_thigh, LEFTLEG = l_calf,
LEFTFOOT = l_foot, LEFTTOEBASE = l_toe, and the `r_` mirrors. SPINE2, SPINE3, NECK1 and the
seven board bones have no Halo counterpart. Note the axis mismatch: Halo bones point +X along
the bone with the model Z-up in wu; the skate rig is Y-up metres, so a per-bone basis change is
needed on top of the `B * m * B^-1` conversion that doc already describes.

### G3.5 Vertex space: rigid and skinned sections

`SectionBlock.NodeIndex` (u8 at section +45): **255 = skinned** (vertex format 2
`s_skinned_vertex`, blend indices and weights per vertex), otherwise the one node that drives
the whole section (vertex format 1 `s_rigid_vertex`). Masterchief: 106 skinned sections, 173
rigid ones on `head` (45, helmets), `l_upperarm` (44), `r_upperarm` (44), `spine1` (40, chests).

**All vertex positions, rigid or skinned, are stored in model space at the bind pose, not in
node space.** Three independent confirmations:

1. mp_spawn_point (tag 173): its only node `root` sits at (0, 0, 0.5) wu, yet the disc section
   decodes to z `[-0.000, 0.033]` and the plasma column to `[0.013, 0.968]`, inside the model
   bounds Z `[-0.0002, 0.9679]`. HaloExport's glTF (`<HaloExport output>\riverworld\scene.gltf`,
   node `respawn_point#scenery-0000`, mesh 76) has vertex Y (up) in `[-0.0007, 0.1015]` m for
   the disc and up to 2.95 m for the column, with the node's translation 4.5727 m / 3.048 =
   1.50024 wu = the placement z exactly: **no node offset was applied**, and that export is the
   Valhalla the user already skates in IW4L.
2. mp_masterchief section 1 (node 15 `head`): decoded centroid (0.020, 0.000, 0.668) wu is at
   the head node's bind origin (0.002, 0.000, 0.609); multiplying by the node's world matrix would
   move it to (0.006, 0.668, 0.629), out of the body.
3. Reclaimer's own Assimp export (`Reclaimer\Plugins\ModelViewerPlugin.cs`, the vertex-weights
   block near line 433) gives a rigid mesh weight 1.0 on `BoneIndex` and uses the **inverse** of
   the bone's world transform as the bone offset, which is the standard "mesh is in model space"
   skinning setup; HaloExport (`HaloExport\Program.cs` line 457) applies no transform to
   non-instanced permutations.

So the runtime rule is `v_world = placement * boneWorld_animated[n] * InverseTransform[n] * v_model`
(InverseTransform is exactly the stored 3x4), and a static viewer draws every section with the
identity. **Consequence for our code**: `crates\blam_geometry\src\render_model.rs` currently
sets `Piece.transform = node_transform(section.node_index)` (the node's bind-pose world
matrix) for rigid sections, which lifts every spawn disc by 0.5 wu (1.52 m) and misplaces any
part whose node is not at the origin; the transform should be the identity (keep the node index
for animation). Not changed here, flagged for the code owner.

Skinned vertices (`sbsp-render-geometry.md` 5, format 2, stride 44): blend indices `UInt8_4` at
+0x24, weights `UInt8_N4` at +0x28. **VERIFIED** on masterchief section 0 (4332 vertices): weight
bytes sum to 255 on every vertex, max blend index 50 with 51 nodes, no weighted index out of range.

### G3.6 Node maps and `UseLocalNodes`

Reclaimer (`Halo3Common.GetMeshes`): if the model has NodeMaps, the blend indices of section `i`
are replaced by `NodeMaps[i].Indices[blendIndex]` (indices beyond the map are left as they are);
the mapping is applied once per vertex buffer. Without NodeMaps the blend indices are global node
indices. **VERIFIED**: on riverworld exactly 6 models have NodeMaps, the six
`objects\powerups\*_ammo` models, each with 1 node, 1 rigid section and 1 map `[0]`, and these
are also exactly the 6 models with flag bit 2 (`UseLocalNodes`) set; the masterchief has 0 node
maps, so its blend indices are global. A section index with no map entry means "no remap".

### G3.7 Geometry instances (decorators) and which permutations to draw

`InstancedGeometrySectionIndex >= 0` names one section whose **subsets** are the instances:
subset `i` <-> `GeometryInstances[i]` (**VERIFIED** subset count == instance count on all 11
instanced models, all `levels\...\decorators\...` with vertex format 0x0F). Reclaimer slices
that section's index buffer per subset (`IndexStart`, `IndexLength`), re-bases the indices to the
subset's own vertex range, takes the material from `Submeshes[subset.SubmeshIndex].ShaderIndex`,
and places the piece with `Scale(TransformScale) * matrix` under `Nodes[NodeIndex]`. How the
engine scatters these decorator models over a BSP is outside this doc (Reclaimer does not do it).

Permutations: Reclaimer exports them all (`MeshRange = (SectionIndex, SectionCount)`); HaloExport
takes the first non-instanced permutation of each region; the Rust reader takes `perm[0]`. For
the Spartan that rule yields an empty model: the first permutation of `arms`, `chest`,
`chest_shared` and `legs` is `legacy`/`empty` with SectionIndex -1. Use the permutation named
**`base`** for a default MP Spartan (**VERIFIED**):

| region | perms | first perm -> section | `base` -> section | section node / format / vertices / shaders |
|---|---|---|---|---|
| arms | 28 | legacy -> -1 | 4 | skinned, 572 verts, `mp_masterchief` |
| body | 17 | legacy -> 0 | 5 | skinned, 3266 verts, `mp_techsuit`, `mp_techsuit_fp` |
| chest | 48 | legacy -> -1 | 6 | rigid on node 6 `spine1`, 996 verts, `mp_masterchief`, `mp_masterchief_rubber`, `lights` |
| chest_shared | 2 | empty -> -1 | -1 (none) | |
| helmet | 70 | el_air_assault -> 1 | 190 (perm 35) | rigid on node 15 `head`, 953 verts, `mp_masterchief`, `mp_masterchief_black`, `mp_masterchief_rubber`, `visor` |
| leftshoulder | 56 | legacy -> 2 | 8 | skinned, 459 verts |
| legs | 32 | legacy -> -1 | 9 | skinned, 1213 verts |
| rightshoulder | 56 | legacy -> 3 | 10 | skinned, 586 verts |

The engine's real choice comes from the `hlmt` Variants block (`hlmt +100`, not read by
Reclaimer, section 3) and the placement's VariantName; out of scope here.

### G3.8 Markers

Marker positions and rotations are **node-relative** (Reclaimer stores them with `BoneIndex =
NodeIndex`; its exporters place them under the bone). **VERIFIED** by plausibility on
mp_spawn_point: node `root` at z 0.5; marker group `spawn` has one marker at (0, 0, -0.3986)
relative to node 0 = model z 0.1014, just above the disc (top at 0.033); group `effect` at
(0, 0, -0.4569) = model z 0.043. Read as model space they would be underground. Region /
permutation bytes are 255 ("any") on every marker checked.

Masterchief marker groups (27): `antenna` (2), `autoaim_melee`, `backpack` (38), `body` (node
`spine1`), `drop`, `drop_left`, `drop_right`, `flaming_ninja`, `flashlight`, `fp_body_cam`,
`fx_fire_large`, `fx_fire_small` (4), `hammer_detonation`, `head` (node `head`, local
(0.0395, 0.004, 0)), `infection`, `left_foot` (node `l_foot`), `left_hand` (node `l_hand`),
`right_foot` (`r_foot`), `right_hand` (`r_hand`), `shield_recharge`, `target_head`,
`target_leg_l` (2), `target_leg_r` (2), `target_main` (2), `visor_reflection`, `weapon_back`
(node `spine1`), `weapon_thigh`. mp_spawn_point: `effect` (1), `spawn` (1).

### G3.9 Geometry resource of a render model

Same `render_geometry_api_resource_definition` as the Lbsp (`resources.md` 7, sbsp doc 4):
`ResourcePointer` -> `zone` entry -> fixup slice (vertex/index buffer definitions) -> `play`
page. Differences from the BSP, all **VERIFIED** with `mode_resource_decode.py`:

* Render-model index buffers are **triangle strips** (`IndexFormat` 5 in both the resource
  definition and the section's +48 byte) on both test models, where the BSP used lists. Unstrip
  each submesh's `[IndexStart, IndexStart + IndexLength)` slice with the rule in the sbsp doc 7;
  degenerate stitch triangles are frequent (411 of 809 for the disc pointer).
* Each section owns **two** vertex buffers in the definition (173: 4 buffers for 2 sections,
  section `VertexBufferIndex` 0 and 2; 8096: 558 for 279 sections, index `2 * section`). Reclaimer
  reads only the one the section names; the second one's meaning is unknown (unread).
* The section byte at +47 (unread by Reclaimer) is 1 on mp_spawn_point and 3 on every masterchief
  section, where it was 0 on the BSP sections.
* Positions and texcoords decompress with `BoundingBoxes[0]` for every section (sbsp doc 6).
  Decoded normals have unit length.
* mp_spawn_point's resource is in `riverworld.map` itself (page 1420, cache index -1); the
  masterchief's is in shared cache index 2 of the `play` table (page 228), i.e. the biped lives
  in a shared map, as expected.

### G3.10 EXPECTED VALUES, riverworld.map

| item | mode 173 `objects\multi\models\mp_spawn_point\mp_spawn_point` | mode 8096 `objects\characters\masterchief\mp_masterchief\mp_masterchief` |
|---|---|---|
| index item meta pointer -> file | `0x732AC9EC` -> `0x05E857B0` | `0x73CBE900` -> `0x086CD400` |
| +0 Name / +4 Flags | `0x7C9` = `mp_spawn_point` / 0 | `0x29F8` = `mp_masterchief` / 0 |
| +12 Regions | 2 (`default`, `plasma`) at file `0x05E854F8` | 8 at `0x086BDBCC` |
| +28 InstancedGeometrySectionIndex / +32 instances | -1 / 0 | -1 / 0 |
| +48 Nodes | 1 (`root`, pos (0, 0, 0.5), quat (0, 0, 0, -1)) at `0x05E85518` | 51 at `0x086BDC4C` |
| +60 MarkerGroups | 2 at `0x05E855C0` | 27 at `0x086BF968` |
| +72 Shaders | 3 at `0x05E855E0`: `rmsh` 174 `spawn_point_disc_pointer`, `rmsh` 179 `spawn_point_disc`, `rmhg` 180 `spawn_point_plasma2` | 427 at `0x086BFB18` |
| +104 Sections | 2 at `0x05E85694` (both rigid on node 0, format 1, index format 5) | 279 at `0x086C6D4C` (106 skinned, 173 rigid, all index format 5, flags 0) |
| +116 BoundingBoxes | 1 at `0x05E8572C`: X (-0.2062, 0.2580) Y (-0.2039, 0.2039) Z (-0.0002, 0.9679) U (-0.9649, 1.8553) V (-1.5970, 2.9995) | 1 at `0x086CC020`: X (-0.1038, 0.2411) Y (-0.1938, 0.1938) Z (-0.0052, 0.8028) U (-0.9820, 4.9975) V (-1.6514, 1.4303) |
| +176 NodeMaps | 0 | 0 |
| +224 ResourcePointer | `0xE1960033` -> resource 51 (owner: this tag), 14 fixups, 4 vertex / 2 index buffers | `0xF0B30F50` -> resource 3920 (owner: this tag), 1676 fixups, 558 / 279 buffers |
| section 0 | vb 0: 1818 verts, stride 36; ib 0: 2730 u16 strip indices; submesh 0 shader 0 `[0, 811)` -> **398** triangles; submesh 1 shader 1 `[811, 1919)` -> **672** | vb 0: 4332 skinned verts, stride 44; ib 0: 8190 indices; 5 submeshes -> 2379 + 41 + 1482 + 172 + 24 triangles |
| section 1 | vb 2: 120 verts; ib 1: 153 indices; 1 submesh shader 2 -> **112** triangles | vb 2: 1025 rigid verts on node 15 `head`; 2184 indices -> 1140 + 108 triangles |

Cross-check against HaloExport's riverworld totals (`reference-numbers.md` 6.1): 398 x 119 =
47,362 (`spawn_point_disc_pointer`), 672 x 119 = 79,968 (`spawn_point_disc`), 112 x 119 = 13,328
(`spawn_point_plasma2`), i.e. 119 placements on the map use this model and the per-submesh
strip-decoded triangle counts are **exact**.

Survey over all 322 mode tags of riverworld (`mode_riverworld.py`): names decode 322/322,
resource owner == tag 322/322, node checks (tree, unit quaternions, distance, inverse matrix)
pass on 322/322 (1078 nodes), vertex formats seen in sections: 1 (rigid) x 707, 2 (skinned)
x 419, 15 (decorator) x 11; 11 models have instanced geometry, 6 have node maps.

### G3.11 Remaining uncertainty

1. `InstanceMask1..4` semantics (which permutation owns which instance) could not be exercised:
   every mask on riverworld is 0 and the instanced models have no regions.
2. Unread bytes: tag +8, +24, +44, +84..+103, +128..+175, +188..+223; node +10; bounding box +0
   (always 3) and +44..+55; section +47 (1 for the spawn point, 3 for the Spartan); the second
   vertex buffer per section; marker +3 and marker Scale (always 0); shader bytes 16..35.
3. `InverseScale` is 1.0 everywhere seen; if a model ever stores another value the exact way it
   combines with InverseTransform is unknown (Reclaimer ignores it).
4. The 16-byte pre-U9 permutation stride is Reclaimer's claim; no cache older than U13 was read.
5. Which permutation the engine draws per region comes from `hlmt` variants and the placement
   VariantName, both unread; `base` is only the sensible default for the MP Spartan.
6. The bone-basis change needed to retarget Halo nodes (+X along the bone, Z up, wu) onto the
   skate rig (Y up, metres) is not worked out here; the local positions above give the bind pose
   to do it from.
