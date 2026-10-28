# The MCC Halo 3 `.map` container (cache file), build U13

Spec for the `blam_cache` crate: everything needed to open a map, find its tags,
read tag names and string ids, and locate the scenario. Tag *definitions* (what is
inside a `scnr` or `sbsp`) are other documents; this one stops at the tag's meta
pointer and the three primitives every definition is built from (TagBlock,
TagReference, StringId).

- Target: MCC Halo 3 build string `Dec 21 2023 22:31:37`, cache version 13 =
  Reclaimer `CacheType.MccHalo3U13` (enum value 22; corrected: was given as 23, see section 1).
- Facts come from Gravemind2401's Reclaimer (GPL-3.0, clone at
  `D:\reflex-tools\halo-tools\Reclaimer`, commit `36224d7`). Only format facts are
  recorded here (offsets, sizes, semantics, algorithms in our own words); no code was
  copied. Each fact names the Reclaimer file and attribute it came from so a reviewer
  can re-check it.
- Every number was verified on real files with
  `python -I tools/verify_container.py` (read-only; prints everything quoted here).
  Primary test file: `riverworld.map` (Valhalla), file size 0x0952C000 = 156,418,048.
  Also checked: `shared.map`, `campaign.map`, `deadlock`, `cyberdyne`, `005_intro`,
  `010_jungle`, `120_halo`.
- Everything is little-endian. All offsets are given as hex and decimal.

Contents: 1 version rule, 2 file layout, 3 header, 4 address translation, 5 partitions,
6 tag index, 7 tag classes, 8 tag items, 9 tag names, 10 strings and string ids,
11 TagBlock/TagReference/StringId, 12 scenario and global tags, 13 locales,
14 shared.map and campaign.map, 15 expected values for Rust tests, 16 open questions.

---

## 1. How Reclaimer picks offsets per version (so the U13 values below are right)

Reclaimer declares struct layouts with `[Offset(x, MinVersion=A, MaxVersion=B)]` and
`[FixedSize(n, MinVersion=A, MaxVersion=B)]` attributes (`Reclaimer.Core\IO\Annotations.cs`).
The version passed in is the integer value of the `CacheType` enum
(`reader.ReadObject<CacheHeaderU6>((int)CacheType)` in `Blam\MccHalo3\CacheFileU6.cs`).

The selection rule is `Reclaimer.Core\IO\Dynamic\Extensions.cs`, `ValidateVersion`:

> an attribute applies when `(version >= Min or no Min)` **and**
> `(version < Max or no Max or (version == Max and Max == Min))`.

So **MinVersion is inclusive, MaxVersion is exclusive**, with one exception: if
Min == Max the attribute applies to exactly that version. `GetVersion` prefers a
versioned attribute over an unversioned one (the unversioned one is the fallback), and
`StructureDefinition.FindVersionDefinition` takes the *first* version definition that
validates. The attribute doc-comments say the same ("inclusive minimum", "exclusive
maximum").

`CacheType` values that matter (`Blam\Common\Enums.cs`, `Unknown = -1` then counting up):

| value | CacheType | build strings (U13 ones) |
|---|---|---|
| 14 | Halo3Beta | |
| 15 | Halo3Retail | |
| 16 | MccHalo3 | |
| 17 | MccHalo3U4 | |
| 18 | MccHalo3F6 | first build that uses the "U6" header class |
| 19 | MccHalo3U6 | |
| 20 | MccHalo3U9 | |
| 21 | MccHalo3U12 | |
| **22** | **MccHalo3U13** | `May 22 2023 20:10:02`, `Jun 27 2023 08:39:36`, `Sep  1 2023 11:01:27`, `Sep  1 2023 11:01:50`, **`Dec 21 2023 22:31:37`**, `Jan  3 2024 10:08:58`, `Jan  3 2024 10:01:45` (all tagged string-id set `"U13"`) |
| 23 | Halo3ODST | |
| 24 | MccHalo3ODST | |
| 25 | MccHalo3ODSTF3 | |
| 26 | MccHalo3ODSTU3 | |

(corrected: every row of this table was off by one. Re-counted from `Blam\Common\Enums.cs`:
`Unknown = -1`, `Halo1Xbox = 0`, `Halo1PC`, `Halo1CE`, `Halo1AE`, `MccHalo1`, `Halo2E3`, `Halo2Beta = 6`,
`Halo2Xbox = 7`, `Halo2Vista`, `MccHalo2`, `MccHalo2U1`, `Halo3PreAlpha = 11`, `Halo3Alpha`, `Halo3Delta = 13`,
`Halo3Beta = 14`, ... `MccHalo3U13 = 22`, `Halo3ODST = 23`. The attributes are written with symbols, so
every range shifts with the values and no resolved U13 offset below changes.)

Resolved for U13 (= 22):

| struct (Reclaimer file) | attribute | U13 result |
|---|---|---|
| `CacheHeaderU6` (`MccHalo3\CacheFileU6.cs`) | `[FixedSize(16384)]` | header is 0x4000 = 16384 bytes |
| `CacheHeaderU6.SectionOffsetTable` | `[Offset(1196, Max=U6)]`, `[Offset(1228, Min=U6, Max=ODSTF3)]`, `[Offset(1196, Min=ODSTF3, Max=ODSTU3)]`, `[Offset(1228, Min=ODSTU3)]` | 22 is in [19, 25) → **1228 = 0x4CC** |
| `CacheHeaderU6.SectionTable` | same pattern with 1212 / 1244 | **1244 = 0x4DC** |
| `TagIndex` (`MccHalo3\CacheFile.cs`) | `[FixedSize(72)]` | 72 = 0x48 |
| `TagClass` | `[FixedSize(16)]` | 16 |
| `IndexItem` | `[FixedSize(8)]` | 8 |
| `PartitionLayout64` (`Common\Gen3\PartitionTable64.cs`) | `[FixedSize(16)]` | 16, six of them |
| `SectionLayout` (`Common\Gen3\SectionTable.cs`) | `[FixedSize(8)]` | 8, four of them |
| `TagBlock` (`Common\TagBlock.cs`) | `[FixedSize(12, Max=Halo2Xbox)]`, `[FixedSize(8, Min=Halo2Xbox, Max=Halo3Beta)]`, `[FixedSize(12, Min=Halo3Beta)]` | **12** |
| `TagReference` (`Common\TagReference.cs`) | `[FixedSize(16, Max=Halo2Xbox)]`, `[FixedSize(8, ...)]`, `[FixedSize(16, Min=Halo3Beta)]` | **16** |
| `LocaleTable.LocaleEntry` (`Common\Gen3\LocaleIndex.cs`) | `[FixedSize(8)]` | 8 |
| string ids (`Resources\MccHalo3Strings.xml`, set `"U13"`) | `indexBits="19" namespaceBits="8" lengthBits="5"` | see section 10 |
| locale index (`MccHalo3\CacheFileU6.cs`) | `Halo3 && CacheType >= MccHalo3U12 → LocaleIndex(cache, 24, 80, 12)` | section 13 |

Which reader class: `Blam\Common\CacheFactory.cs` → Halo 3 MCC with `CacheType >= MccHalo3F6`
uses `MccHalo3.CacheFileU6` + `CacheHeaderU6`. Older MCC builds (16, 17) use
`CacheHeader` (12288 bytes, different offsets); not our target, listed only in the
source file.

How Reclaimer identifies the build (`Blam\Common\CacheArgs.cs`): magic at 0 is `head`
(LE int 0x68656164; `daeh` would mean big-endian/360), version at 4. For version 13 it
peeks the i32 at 0x40 (64): zero would mean an older (pre-August-2021) MCC Gen3 header
with the build string at 0x120 (288). It is non-zero here (our string-namespace count
10), so it reads a 32-byte string at 0x40; if that matches a Halo 1 MCC build pattern the
file is MccHalo1, otherwise it reads 32 bytes at 0xA0 (160) and accepts them if they parse
as a date → build string `Dec 21 2023 22:31:37` (a non-date there would mean a Gen4 MCC
header with the string at 0x98). `CacheMetadata.FromBuildString` maps it to `MccHalo3U13`; if a
date-like build string is unknown it falls back to the newest CacheType for the engine
guessed from the folder name (`halo3\maps` → `MccHalo3U13`). Rust should do the same:
accept version 13 + known U13 build strings, and treat unknown later builds as U13 with
a warning.

---

## 2. File layout

A level map is a 16 KiB header followed by four **sections** that tile the rest of the
file exactly (verified: the four ranges below are contiguous and end at the file size).

| section | Reclaimer name / use | riverworld file range | size |
|---|---|---|---|
| header | `CacheHeaderU6` | 0x0 .. 0x4000 | 0x4000 = 16,384 |
| 0 | "debug": string index, string table, string-namespace table, tag names, tag-name index (`SectionAddressTranslator(cache, 0)` = `HeaderTranslator`) | 0x4000 .. 0x114000 | 0x110000 = 1,114,112 |
| 1 | resources / raw pages (bitmaps, sounds, geometry; addressed through the `play` tag, out of scope here) | 0x114000 .. 0x5753000 | 0x563F000 = 90,435,584 |
| 2 | tag data (`TagAddressTranslator` = `MetadataTranslator`) | 0x5753000 .. 0x93D3000 | 0x3C80000 = 63,438,848 |
| 3 | locales (`SectionAddressTranslator(cache, 3)`) | 0x93D3000 .. 0x952C000 | 0x159000 = 1,413,120 |

Inside section 0 on riverworld (all from header pointers, section 4):
string index 0x4000, string table 0x17000 (size 0x5377C), namespace table 0x6B000
(40 bytes), tag names 0x6C000 (size 0x9B515), tag-name index 0x108000 (11658 × 4).

Inside section 2 the tag data is further divided into six **partitions** (section 5),
and the **tag index** structure (section 6) sits inside partition 1.

---

## 3. Header (0x4000 bytes, `CacheHeaderU6`)

Columns: offset, type, Reclaimer property (file `Blam\MccHalo3\CacheFileU6.cs` unless
noted; fields inherited unchanged from `CacheHeader` in `CacheFile.cs` are marked), and
the riverworld value. Fields marked *observed* are not read by Reclaimer; their meaning
is inferred from the bytes of several maps in this session (evidence in section 14).

| offset | type | field (Reclaimer attribute) | riverworld | notes |
|---|---|---|---|---|
| 0x000 (0) | char[4] | magic (`CacheArgs.LittleHeader = 0x68656164`) | bytes `64 61 65 68` = "head" as LE u32 | `daeh` LE = big-endian file |
| 0x004 (4) | i32 | version (`CacheArgs.Version`) | 13 | 13 for all MCC Gen3 caches since 2021 |
| 0x008 (8) | i32 | `FileSize` (`[Offset(8)] [StoreType(int)]`) | 0x0952C000 = 156,418,048 | equals the real file size |
| 0x00C (12) | u32 | *observed* unknown | 0x202 = 514 | 0x202 in every map checked, incl. shared/campaign |
| 0x010 (16) | i32 | `TagDataAddress` (`[Offset(16)]`) | 0x06237000 | equals `sections[2].address`; Reclaimer only uses it for Beta/Alpha caches (`Halo3\BetaTagAddressTranslator.cs`) |
| 0x014 (20) | i32 | `VirtualSize` (`[Offset(20)]`) | 0x03C80000 = 63,438,848 | equals `sections[2].size` and the sum of the six partition sizes; read but unused by Reclaimer |
| 0x018 (24) | i16 | *observed* cache type | 1 | 0 campaign (`005_intro`, `010_jungle`, `120_halo`), 1 multiplayer (riverworld, deadlock, cyberdyne, s3d_edge, sandbox), 3 `shared.map`, 4 `campaign.map`. (2 is presumably main menu; no mainmenu.map in `halo3\maps`.) |
| 0x01A (26) | i16 | *observed* shared-cache slot | -1 | -1 in every level map; 2 in `shared.map`, 3 in `campaign.map` = the slot of its id in the dependents' table at 0x58 (see 0x058) |
| 0x01C (28) | i16 | *observed* unknown | 230 | 230 in MP maps and 005_intro, 214 in 010_jungle/120_halo, 4 in shared, 6 in campaign |
| 0x01E (30) | i16 | *observed* unknown | 4 | 4 / 12 / 0 / 0 for the same groups |
| 0x020 (32) | i32 | `FileCount` (`[Offset(32)]`) = tag count | 0x2D8A = 11,658 | equals the tag index's tag count |
| 0x024 (36) | ptr32 (sec 0) | `FileTablePointer` (`[Offset(36)]`) = tag-name table | 0x0618F000 → file 0x6C000 | concatenated NUL-terminated names |
| 0x028 (40) | i32 | `FileTableSize` (`[Offset(40)]`) | 0x9B515 = 636,181 | bytes of the name table |
| 0x02C (44) | ptr32 (sec 0) | `FileTableIndexPointer` (`[Offset(44)]`) | 0x0622B000 → file 0x108000 | `tag count` × i32 byte offsets into the name table; -1 = no name |
| 0x030 (48) | i32 | `StringCount` (`[Offset(48)]`) | 0x4976 = 18,806 | |
| 0x034 (52) | ptr32 (sec 0) | `StringTablePointer` (`[Offset(52)]`) | 0x0613A000 → file 0x17000 | concatenated NUL-terminated strings |
| 0x038 (56) | i32 | `StringTableSize` (`[Offset(56)]`) | 0x5377C = 341,884 | |
| 0x03C (60) | ptr32 (sec 0) | `StringTableIndexPointer` (`[Offset(60)]`) | 0x06127000 → file 0x4000 | `string count` × i32 byte offsets; <0 = none |
| 0x040 (64) | i32 | `StringNamespaceCount` (`[Offset(64)]`) | 10 | |
| 0x044 (68) | ptr32 (sec 0) | `StringNamespaceTablePointer` (`[Offset(68)]`) | 0x0618E000 → file 0x6B000 | 10 × u32 |
| 0x048 (72) | u64 | *observed* unknown | 0 | 0 in level maps; 0xFFFFFFFF00000000 in shared; 0xFFFFFFFFFFFFFFFF in campaign |
| 0x050 (80) | u64 | *observed* this cache's id | 0x01DA34A99B164C65 | for level maps the value decodes as a Windows FILETIME (2023-12-22 07:36:41 UTC, i.e. build time); for shared/campaign it does not |
| 0x058 (88) | u64[5] | *observed* ids of required shared caches, indexed by slot | slot 2 (0x068) = 0x797B2A3473952700 = `shared.map`'s 0x050; slot 3 (0x070) = 0 | campaign maps 010_jungle/120_halo also have slot 3 = 0x198AC984A3BD6780 = `campaign.map`'s 0x050; 005_intro has only slot 2 |
| 0x080 (128) | 32 bytes | *observed* hash-like data | non-zero | not needed for reading |
| 0x0A0 (160) | char[32] NUL-terminated | `BuildString` (`[Offset(160)] [NullTerminated(Length=32)]`) | `Dec 21 2023 22:31:37` | |
| 0x0C0 (192) | char[32] | *observed* map name | `riverworld` | Reclaimer does not read it; shared/campaign say `shared` / `campaign` |
| 0x0E0 (224) | char[256] NUL-terminated | `ScenarioName` (`[Offset(224)] [NullTerminated(Length=256)]`) | `levels\multi\riverworld\riverworld` | tag path of the `scnr`; shared/campaign hold `maps\shared.map` / `maps\campaign.map` |
| 0x1E0 .. 0x2E0 | | zero | | |
| 0x2E0 (736) | i64 | `VirtualBaseAddress` (`[Offset(736)]`) | 0x1CC380000 | virtual address of the first byte of section 2 |
| 0x2E8 (744) | ptr64 (tag) | `IndexPointer` (`[Offset(744)]`), re-bound to `MetadataTranslator` after reading | 0x1CC549AF0 → file 0x0591CAF0 | the tag index structure, section 6 |
| 0x2F0 .. 0x300 | | zero | | |
| 0x300 (768) | 6 × {u64 addr, u64 size} | `PartitionTable` (`[Offset(768)]`, `PartitionTable64`/`PartitionLayout64`) | see section 5 | |
| 0x360 (864) | u64 + ... | *observed* hash/checksum-like region 0x360..0x3C0, zeros 0x3C0..0x3CC, 256 hash-like bytes 0x3CC..0x4CC | 0x360 = 0x771338053 | not needed for reading |
| 0x4CC (1228) | 4 × u32 | `SectionOffsetTable` (`[Offset(1228, Min=U6, Max=ODSTF3)]`) | `[0xF9EDD000, 0x00114000, 0xFF51C000, 0x03405000]` | section 4 |
| 0x4DC (1244) | 4 × {u32 addr, u32 size} | `SectionTable` (`[Offset(1244, Min=U6, Max=ODSTF3)]`, `SectionLayout`) | `[{0x06127000, 0x110000}, {0x0, 0x0563F000}, {0x06237000, 0x03C80000}, {0x05FCE000, 0x159000}]` | section 4 |
| 0x4FC .. 0x540 | | *observed* hash-like | | |
| 0x2820 .. 0x2860, 0x28B8 | | *observed* small non-zero values (i32 0x2824 = 1; u32 0x282C and 0x2830 both 0x165FA2FF; then a bitmask-looking run of mostly 0xFF bytes to 0x2855; i32 0x28B8 = 1) | | unknown, not needed |
| 0x3FFC (16380) | char[4] | *observed* footer | bytes `74 6F 6F 66` = "foot" as LE u32 (0x666F6F74) | present in all three maps |

Everything else in the 16 KiB is zero on riverworld (non-zero rows: 0x0-0x70, 0x80-0xD0,
0xE0-0x110, 0x2E0-0x2F0, 0x300-0x540, 0x2820-0x2860, 0x28B0-0x28C0, 0x3FF0-0x4000).

Other tools (Assembly, TagTool) call the 16 bytes at 0x4CC plus the section table the
header's "interop" block and give the four u32s individual names; Reclaimer treats them
as one per-section offset each, and that arithmetic (next section) is what we verified.

---

## 4. Address translation

Three kinds of pointer exist. None of them is a plain file offset.

### 4.1 Header pointers (32-bit, section 0)

`Common\Gen3\SectionAddressTranslator.cs`: for section *i* the translator's `Magic` is
`sections[i].address - (sections[i].address + sectionOffsets[i])` in **u32 arithmetic**,
and `address = pointer - Magic`. Equivalent, simpler form:

```
file_offset = (pointer + section_offsets[i]) mod 2^32        // i = 0 for header pointers
```

riverworld, section 0: `section_offsets[0] = 0xF9EDD000`.
`0x06127000 + 0xF9EDD000 = 0x1_0000_4000 → 0x4000` (string index). Likewise
0x0613A000 → 0x17000, 0x0618E000 → 0x6B000, 0x0618F000 → 0x6C000, 0x0622B000 → 0x108000.

The same formula with *i* = 1 or 3 gives the file base of the resource and locale
sections: `(0 + 0x114000) = 0x114000`, `(0x05FCE000 + 0x03405000) = 0x093D3000`.
Locale offsets inside `matg` are section-3 pointers (section 13).

### 4.2 Tag-data addresses (64-bit virtual, section 2)

`Common\Gen3\TagAddressTranslator.cs`: `Magic = VirtualBaseAddress - (sections[2].address + sectionOffsets[2])`
where the parenthesis is u32 arithmetic; `file = virtual - Magic`. Simpler form:

```
tag_section_file_base = (sections[2].address + section_offsets[2]) mod 2^32   // 0x05753000 on riverworld
file_offset           = virtual - virtual_base + tag_section_file_base
```

riverworld: `virtual_base = 0x1CC380000`, so `file = virtual - 0x1C6C2D000`.
Tag index: `0x1CC549AF0 → 0x0591CAF0`. Valid virtual range is
`[0x1CC380000, 0x1CC380000 + 0x3C80000 = 0x1D0000000)`; Rust should bounds-check.

### 4.3 32-bit pointers inside tag data (shifted)

`Blam\MccHalo3\PointerExpander.cs`: `Expand(p) = (p << 2) + 0` (magic is 0 for Halo 3
MCC), `Contract(v) = v >> 2`. Every 32-bit pointer stored inside tag data (the meta
pointer of a tag index item, TagBlock pointers, data pointers) is a 64-bit virtual
address divided by 4. `Utilities\Pointer.cs`: `Address = translator(expander(value))`.

```
file_offset = ((u64)stored << 2) - virtual_base + tag_section_file_base
```

Example: `scnr` meta pointer stored 0x731D5856 → `<<2` = 0x1CC756158 → file 0x5B29158.
The expanded values of all 11,658 riverworld meta pointers lie in
[0x1CC3BE310, 0x1CFFF34A0], inside the tag section. 64-bit pointers (header
`IndexPointer`, the pointers inside the tag index) are **not** shifted
(`Utilities\Pointer64.cs` has no expander).

---

## 5. Partition table (0x300, six × {u64 address, u64 size})

The six partitions carve the tag section (virtual addresses) into contiguous ranges;
their sizes sum to `VirtualSize`. Reclaimer reads them (`PartitionTable64`) and exposes
them through `IGen3Header.PartitionTable` but never uses them, so **a reader does not
need them**. The contents below were determined by sorting riverworld's tags by meta
pointer into partitions; the names are the conventional engine names for the six Halo 3
partitions (resources, sound resources, global tags, shared tags, base-map tags, map
tags), which match what we found.

| # | riverworld address | size | what lives there (riverworld) |
|---|---|---|---|
| 0 | 0x1CC380000 | 0x40000 = 262,144 | 1 tag: `play` (resource layout table) |
| 1 | 0x1CC3C0000 | 0x190000 = 1,638,400 | 2,292 tags: all 2,290 `snd!`, `ugh!`, `zone`; also the global-tags table, tag table, class table and tag index (0x1CC532300 .. 0x1CC549B38) |
| 2 | 0x1CC550000 | 0x110000 = 1,114,112 | 1 tag: `matg` (globals) |
| 3 | 0x1CC660000 | 0xE0000 = 917,504 | 316 tags shared between maps: 233 `phmo`, 47 `weap`, 36 `eqip` |
| 4 | 0x1CC740000 | 0 | empty for a multiplayer map (base-map tags) |
| 5 | 0x1CC740000 | 0x38C0000 = 59,506,688 | 9,048 tags: `draw`, `sncl`, `spk!`, `scen`, `scnr`, `wind`, `sbsp`, 2,204 `bitm`, 939 `rmsh`, ... ; 170 of them are named `levels\multi\riverworld\...` |

Sum = 0x3C80000 = `VirtualSize`. Partition 0 starts at `VirtualBaseAddress`.

---

## 6. Tag index (72 bytes at `IndexPointer`, `TagIndex` in `MccHalo3\CacheFile.cs`)

riverworld: virtual 0x1CC549AF0 → file 0x0591CAF0.

| offset | type | Reclaimer (`[FixedSize(72)]`) | riverworld | notes |
|---|---|---|---|---|
| 0x00 (0) | i32 | `TagClassCount` (`[Offset(0)]`) | 0xB7 = 183 | |
| 0x04 (4) | 4 bytes | padding | 0 | |
| 0x08 (8) | ptr64 | `TagClassDataPointer` (`[Offset(8)]`) | 0x1CC548F80 → file 0x0591BF80 | 183 × 16 bytes, ends 0x0591CAF0 = the tag index itself |
| 0x10 (16) | i32 | `TagCount` (`[Offset(16)]`) | 0x2D8A = 11,658 | same as header 0x20 |
| 0x14 (20) | 4 bytes | padding | 0 | |
| 0x18 (24) | ptr64 | `TagDataPointer` (`[Offset(24)]`) | 0x1CC532330 → file 0x05905330 | 11,658 × 8 bytes |
| 0x20 (32) | i32 | *observed* global-tag count (360 analogue: `TagInfoHeaderCount` in `Halo3\CacheFile.cs`) | 6 | |
| 0x24 (36) | 4 bytes | padding | 0 | |
| 0x28 (40) | ptr64 | *observed* global-tags table (360: `TagInfoHeaderPointer`) | 0x1CC532300 → file 0x05905300 | 6 × 8 bytes, immediately before the tag table |
| 0x30 (48) | i32 | *observed* second count (360: `TagInfoHeaderCount2`) | 0 | |
| 0x34 (52) | 4 bytes | padding | 0 | |
| 0x38 (56) | ptr64 | *observed* second pointer (360: `TagInfoHeaderPointer2`) | 0x1CC532300 | same as 0x28 because the count is 0 |
| 0x40 (64) | u32 | *observed* per-map value (checksum?) | 0x628889E4 | deadlock 0x332BB35B, cyberdyne 0x174392E4, 005_intro 0xC30DC063 |
| 0x44 (68) | char[4] | *observed* footer | bytes `73 67 61 74` = "tags" as LE u32 (0x74616773) | same in every map checked |

Reclaimer only reads the first four fields; the rest is what the task called "the six
unknown pointers after the tag table pointer".

### 6.1 Global-tags table (6 × 8 bytes at 0x28's pointer)

Each entry is `{u32 class id, u32 tag datum}` (class id is the 4CC stored as a LE int,
see section 7; the datum's low 16 bits are the tag index, the high 16 bits the salt, see
section 8). riverworld, file 0x05905300:

| class (4CC) | datum | index = datum & 0xFFFF | salt = datum >> 16 | tag item class / salt | tag name | consistent |
|---|---|---|---|---|---|---|
| `draw` | 0xE1750000 | 0 | 0xE175 | `draw` / 0xE175 | `` | True |
| `play` | 0xE1760001 | 1 | 0xE176 | `play` / 0xE176 | `` | True |
| `ugh!` | 0xE1770002 | 2 | 0xE177 | `ugh!` / 0xE177 | `i've got a lovely bunch of coconuts` | True |
| `zone` | 0xE1780003 | 3 | 0xE178 | `zone` / 0xE178 | `there they are all standing in a row` | True |
| `scnr` | 0xE17E0008 | 8 | 0xE17E | `scnr` / 0xE17E | `levels\multi\riverworld\riverworld` | True |
| `matg` | 0xE1790004 | 4 | 0xE179 | `matg` / 0xE179 | `globals\globals` | True |

This is the fastest way to find the scenario and the global tags, and it is the only
place `draw` (`rasterizer_cache_file_globals`) is singled out. Reclaimer instead scans
the tag table for classes `scnr`, `matg`, `ugh!`, `play`, `zone`
(`CacheFactory.SystemClasses`); both ways give the same tags on riverworld.

---

## 7. Tag class table (`TagClass`, 16 bytes each)

| offset | type | Reclaimer | notes |
|---|---|---|---|
| 0x0 (0) | i32 | `ClassId` (`[Offset(0)]`) | the 4-char code stored as a LE int, i.e. **the bytes in the file are the code reversed** (`sbsp` is stored `70 73 62 73` = "psbs"; `ClassCode` reverses the bytes). Codes may contain spaces (` rmc`, ` rmp`, `rm  ` with two trailing spaces, `rmb `, `rmd `, `rmw `, `fog `) and symbols (`$#!+`, `/**/`, `<fx>`). Keep the raw 4 bytes in Rust; do not trim. |
| 0x4 (4) | char[4] | `ParentClassCode` (`[Offset(4)] [FixedLength(4)]`) | parent class code, same byte order as above; 0xFFFFFFFF = none |
| 0x8 (8) | char[4] | `ParentClassCode2` (`[Offset(8)]`) | grandparent, same encoding |
| 0xC (12) | i32 | `ClassName` (`[Offset(12)]`, `StringId`) | string id of the long name, resolved via section 10 |

riverworld has 183 classes, sorted by the 4CC's byte values (big-endian string order),
110 of them used by at least one tag. Class 0 is ` rmc`, class 182 is `zone`. The full
list with every string id resolved (ns = namespace bits, idx = index bits, len bits =
the top 5 bits, table index = position in the string table) is the expected value for
the Rust class-name test:

| # | code | parent | grandparent | name string id | ns | idx | len bits | table index | name |
|---|---|---|---|---|---|---|---|---|---|
| 0 | ` rmc` | `rm  ` | `none` | 0x00004045 | 0 | 0x4045 | 0 | 18640 | `shader_contrail` |
| 1 | ` rmp` | `rm  ` | `none` | 0x00004046 | 0 | 0x4046 | 0 | 18641 | `shader_particle` |
| 2 | `$#!+` | `none` | `none` | 0x00004047 | 0 | 0x4047 | 0 | 18642 | `cache_file_sound` |
| 3 | `*cen` | `none` | `none` | 0x00004048 | 0 | 0x4048 | 0 | 18643 | `scenario_scenery_resource` |
| 4 | `*eap` | `none` | `none` | 0x00004049 | 0 | 0x4049 | 0 | 18644 | `scenario_weapons_resource` |
| 5 | `*ehi` | `none` | `none` | 0x0000404A | 0 | 0x404A | 0 | 18645 | `scenario_vehicles_resource` |
| 6 | `*fsc` | `none` | `none` | 0x0000404B | 0 | 0x404B | 0 | 18646 | `scenario_effect_scenery_resource` |
| 7 | `*igh` | `none` | `none` | 0x0000404C | 0 | 0x404C | 0 | 18647 | `scenario_lights_resource` |
| 8 | `*ipd` | `none` | `none` | 0x0000404D | 0 | 0x404D | 0 | 18648 | `scenario_bipeds_resource` |
| 9 | `*qip` | `none` | `none` | 0x0000404E | 0 | 0x404E | 0 | 18649 | `scenario_equipment_resource` |
| 10 | `*rea` | `none` | `none` | 0x0000404F | 0 | 0x404F | 0 | 18650 | `scenario_creature_resource` |
| 11 | `*sce` | `none` | `none` | 0x00004050 | 0 | 0x4050 | 0 | 18651 | `scenario_sound_scenery_resource` |
| 12 | `/**/` | `none` | `none` | 0x00004051 | 0 | 0x4051 | 0 | 18652 | `scenario_comments_resource` |
| 13 | `<fx>` | `none` | `none` | 0x00004052 | 0 | 0x4052 | 0 | 18653 | `sound_effect_template` |
| 14 | `BooM` | `none` | `none` | 0x00004053 | 0 | 0x4053 | 0 | 18654 | `stereo_system` |
| 15 | `Lbsp` | `none` | `none` | 0x00004054 | 0 | 0x4054 | 0 | 18655 | `scenario_lightmap_bsp_data` |
| 16 | `adlg` | `none` | `none` | 0x00004055 | 0 | 0x4055 | 0 | 18656 | `ai_dialogue_globals` |
| 17 | `ai**` | `none` | `none` | 0x00004056 | 0 | 0x4056 | 0 | 18657 | `scenario_ai_resource` |
| 18 | `ant!` | `none` | `none` | 0x00000F1D | 0 | 0xF1D | 0 | 6056 | `antenna` |
| 19 | `beam` | `none` | `none` | 0x00004057 | 0 | 0x4057 | 0 | 18658 | `beam_system` |
| 20 | `bink` | `none` | `none` | 0x00004058 | 0 | 0x4058 | 0 | 18659 | `bink` |
| 21 | `bipd` | `unit` | `obje` | 0x00004059 | 0 | 0x4059 | 0 | 18660 | `biped` |
| 22 | `bitm` | `none` | `none` | 0x0000405A | 0 | 0x405A | 0 | 18661 | `bitmap` |
| 23 | `bkey` | `none` | `none` | 0x0000405B | 0 | 0x405B | 0 | 18662 | `gui_button_key_definition` |
| 24 | `bloc` | `obje` | `none` | 0x0000405C | 0 | 0x405C | 0 | 18663 | `crate` |
| 25 | `bmp3` | `none` | `none` | 0x0000405D | 0 | 0x405D | 0 | 18664 | `gui_bitmap_widget_definition` |
| 26 | `bsdt` | `none` | `none` | 0x0000405E | 0 | 0x405E | 0 | 18665 | `breakable_surface` |
| 27 | `cddf` | `none` | `none` | 0x0000040A | 0 | 0x40A | 0 | 1034 | `collision_damage` |
| 28 | `cfxs` | `none` | `none` | 0x0000405F | 0 | 0x405F | 0 | 18666 | `camera_fx_settings` |
| 29 | `chad` | `none` | `none` | 0x00004060 | 0 | 0x4060 | 0 | 18667 | `chud_animation_definition` |
| 30 | `char` | `none` | `none` | 0x00004061 | 0 | 0x4061 | 0 | 18668 | `character` |
| 31 | `chdt` | `none` | `none` | 0x00004062 | 0 | 0x4062 | 0 | 18669 | `chud_definition` |
| 32 | `chgd` | `none` | `none` | 0x00004063 | 0 | 0x4063 | 0 | 18670 | `chud_globals_definition` |
| 33 | `chmt` | `none` | `none` | 0x00004064 | 0 | 0x4064 | 0 | 18671 | `chocolate_mountain_new` |
| 34 | `cin*` | `none` | `none` | 0x00004065 | 0 | 0x4065 | 0 | 18672 | `scenario_cinematics_resource` |
| 35 | `cine` | `none` | `none` | 0x00004066 | 0 | 0x4066 | 0 | 18673 | `cinematic` |
| 36 | `cisc` | `none` | `none` | 0x00004067 | 0 | 0x4067 | 0 | 18674 | `cinematic_scene` |
| 37 | `clu*` | `none` | `none` | 0x00004068 | 0 | 0x4068 | 0 | 18675 | `scenario_cluster_data_resource` |
| 38 | `clwd` | `none` | `none` | 0x00004069 | 0 | 0x4069 | 0 | 18676 | `cloth` |
| 39 | `cmpu` | `none` | `none` | 0x0000406A | 0 | 0x406A | 0 | 18677 | `compute_shader` |
| 40 | `cntl` | `none` | `none` | 0x0000406B | 0 | 0x406B | 0 | 18678 | `contrail_system` |
| 41 | `coll` | `none` | `none` | 0x0000406C | 0 | 0x406C | 0 | 18679 | `collision_model` |
| 42 | `colo` | `none` | `none` | 0x0000406D | 0 | 0x406D | 0 | 18680 | `color_table` |
| 43 | `crea` | `obje` | `none` | 0x0000406E | 0 | 0x406E | 0 | 18681 | `creature` |
| 44 | `crte` | `none` | `none` | 0x0000406F | 0 | 0x406F | 0 | 18682 | `cortana_effect_definition` |
| 45 | `ctrl` | `devi` | `obje` | 0x00004070 | 0 | 0x4070 | 0 | 18683 | `device_control` |
| 46 | `cub*` | `none` | `none` | 0x00004071 | 0 | 0x4071 | 0 | 18684 | `scenario_cubemap_resource` |
| 47 | `dc*s` | `none` | `none` | 0x00004072 | 0 | 0x4072 | 0 | 18685 | `scenario_decorators_resource` |
| 48 | `dctr` | `none` | `none` | 0x00004073 | 0 | 0x4073 | 0 | 18686 | `decorator_set` |
| 49 | `dec*` | `none` | `none` | 0x00004074 | 0 | 0x4074 | 0 | 18687 | `scenario_decals_resource` |
| 50 | `decs` | `none` | `none` | 0x00004075 | 0 | 0x4075 | 0 | 18688 | `decal_system` |
| 51 | `devi` | `obje` | `none` | 0x000000C6 | 0 | 0xC6 | 0 | 198 | `device` |
| 52 | `devo` | `none` | `none` | 0x00004076 | 0 | 0x4076 | 0 | 18689 | `cellular_automata` |
| 53 | `dgr*` | `none` | `none` | 0x00004077 | 0 | 0x4077 | 0 | 18690 | `scenario_devices_resource` |
| 54 | `dobc` | `none` | `none` | 0x00004078 | 0 | 0x4078 | 0 | 18691 | `detail_object_collection` |
| 55 | `draw` | `none` | `none` | 0x00004079 | 0 | 0x4079 | 0 | 18692 | `rasterizer_cache_file_globals` |
| 56 | `drdf` | `none` | `none` | 0x0000407A | 0 | 0x407A | 0 | 18693 | `damage_response_definition` |
| 57 | `dsrc` | `none` | `none` | 0x0000407B | 0 | 0x407B | 0 | 18694 | `gui_datasource_definition` |
| 58 | `effe` | `none` | `none` | 0x000007CA | 0 | 0x7CA | 0 | 4181 | `effect` |
| 59 | `effg` | `none` | `none` | 0x0000407C | 0 | 0x407C | 0 | 18695 | `effect_globals` |
| 60 | `efsc` | `obje` | `none` | 0x0000407D | 0 | 0x407D | 0 | 18696 | `effect_scenery` |
| 61 | `egor` | `none` | `none` | 0x0000407E | 0 | 0x407E | 0 | 18697 | `screen_effect` |
| 62 | `eqip` | `item` | `obje` | 0x000016BA | 0 | 0x16BA | 0 | 8005 | `equipment` |
| 63 | `flck` | `none` | `none` | 0x0000407F | 0 | 0x407F | 0 | 18698 | `flock` |
| 64 | `fldy` | `none` | `none` | 0x00004080 | 0 | 0x4080 | 0 | 18699 | `fluid_dynamics` |
| 65 | `fog ` | `none` | `none` | 0x00004081 | 0 | 0x4081 | 0 | 18700 | `planar_fog` |
| 66 | `foot` | `none` | `none` | 0x00004082 | 0 | 0x4082 | 0 | 18701 | `material_effects` |
| 67 | `fpch` | `none` | `none` | 0x00004083 | 0 | 0x4083 | 0 | 18702 | `patchy_fog` |
| 68 | `frag` | `none` | `none` | 0x00004084 | 0 | 0x4084 | 0 | 18703 | `fragment` |
| 69 | `gint` | `unit` | `obje` | 0x00004085 | 0 | 0x4085 | 0 | 18704 | `giant` |
| 70 | `glps` | `none` | `none` | 0x00004086 | 0 | 0x4086 | 0 | 18705 | `global_pixel_shader` |
| 71 | `glvs` | `none` | `none` | 0x00004087 | 0 | 0x4087 | 0 | 18706 | `global_vertex_shader` |
| 72 | `goof` | `none` | `none` | 0x00004088 | 0 | 0x4088 | 0 | 18707 | `multiplayer_variant_settings_interface_definition` |
| 73 | `grup` | `none` | `none` | 0x00004089 | 0 | 0x4089 | 0 | 18708 | `gui_group_widget_definition` |
| 74 | `hlmt` | `none` | `none` | 0x00080156 | 1 | 0x156 | 0 | 1519 | `model` |
| 75 | `hlsl` | `none` | `none` | 0x0000408A | 0 | 0x408A | 0 | 18709 | `hlsl_include` |
| 76 | `hsc*` | `none` | `none` | 0x0000408B | 0 | 0x408B | 0 | 18710 | `scenario_hs_source_file` |
| 77 | `item` | `obje` | `none` | 0x00080010 | 1 | 0x10 | 0 | 1193 | `item` |
| 78 | `jmad` | `none` | `none` | 0x0000408C | 0 | 0x408C | 0 | 18711 | `model_animation_graph` |
| 79 | `jmrq` | `none` | `none` | 0x0000408D | 0 | 0x408D | 0 | 18712 | `sandbox_text_value_pair_definition` |
| 80 | `jpt!` | `none` | `none` | 0x0000408E | 0 | 0x408E | 0 | 18713 | `damage_effect` |
| 81 | `lens` | `none` | `none` | 0x00000814 | 0 | 0x814 | 0 | 4255 | `lens_flare` |
| 82 | `ligh` | `none` | `none` | 0x000015C3 | 0 | 0x15C3 | 0 | 7758 | `light` |
| 83 | `lsnd` | `none` | `none` | 0x0000408F | 0 | 0x408F | 0 | 18714 | `sound_looping` |
| 84 | `lst3` | `none` | `none` | 0x00004090 | 0 | 0x4090 | 0 | 18715 | `gui_list_widget_definition` |
| 85 | `lswd` | `none` | `none` | 0x00004091 | 0 | 0x4091 | 0 | 18716 | `leaf_system` |
| 86 | `ltvl` | `none` | `none` | 0x00004092 | 0 | 0x4092 | 0 | 18717 | `light_volume_system` |
| 87 | `mach` | `devi` | `obje` | 0x00004093 | 0 | 0x4093 | 0 | 18718 | `device_machine` |
| 88 | `matg` | `none` | `none` | 0x00004094 | 0 | 0x4094 | 0 | 18719 | `globals` |
| 89 | `mcsr` | `none` | `none` | 0x00004095 | 0 | 0x4095 | 0 | 18720 | `mouse_cursor_definition` |
| 90 | `mdl3` | `none` | `none` | 0x00004096 | 0 | 0x4096 | 0 | 18721 | `gui_model_widget_definition` |
| 91 | `mdlg` | `none` | `none` | 0x00004097 | 0 | 0x4097 | 0 | 18722 | `ai_mission_dialogue` |
| 92 | `metr` | `none` | `none` | 0x00000678 | 0 | 0x678 | 0 | 3843 | `meter` |
| 93 | `mffn` | `none` | `none` | 0x00004098 | 0 | 0x4098 | 0 | 18723 | `muffin` |
| 94 | `mode` | `none` | `none` | 0x00004099 | 0 | 0x4099 | 0 | 18724 | `render_model` |
| 95 | `mply` | `none` | `none` | 0x0000409A | 0 | 0x409A | 0 | 18725 | `multiplayer_scenario_description` |
| 96 | `mulg` | `none` | `none` | 0x0000409B | 0 | 0x409B | 0 | 18726 | `multiplayer_globals` |
| 97 | `nclt` | `none` | `none` | 0x0000409C | 0 | 0x409C | 0 | 18727 | `new_cinematic_lighting` |
| 98 | `obje` | `none` | `none` | 0x000000C7 | 0 | 0xC7 | 0 | 199 | `object` |
| 99 | `perf` | `none` | `none` | 0x0000409D | 0 | 0x409D | 0 | 18728 | `performance_throttles` |
| 100 | `phmo` | `none` | `none` | 0x000016BF | 0 | 0x16BF | 0 | 8010 | `physics_model` |
| 101 | `pixl` | `none` | `none` | 0x0000409E | 0 | 0x409E | 0 | 18729 | `pixel_shader` |
| 102 | `play` | `none` | `none` | 0x0000409F | 0 | 0x409F | 0 | 18730 | `cache_file_resource_layout_table` |
| 103 | `pmdf` | `none` | `none` | 0x000040A0 | 0 | 0x40A0 | 0 | 18731 | `particle_model` |
| 104 | `pmov` | `none` | `none` | 0x000040A1 | 0 | 0x40A1 | 0 | 18732 | `particle_physics` |
| 105 | `pphy` | `none` | `none` | 0x000040A2 | 0 | 0x40A2 | 0 | 18733 | `point_physics` |
| 106 | `proj` | `obje` | `none` | 0x00000CB1 | 0 | 0xCB1 | 0 | 5436 | `projectile` |
| 107 | `prt3` | `none` | `none` | 0x000040A3 | 0 | 0x40A3 | 0 | 18734 | `particle` |
| 108 | `rasg` | `none` | `none` | 0x000040A4 | 0 | 0x40A4 | 0 | 18735 | `rasterizer_globals` |
| 109 | `rm  ` | `none` | `none` | 0x000040A5 | 0 | 0x40A5 | 0 | 18736 | `render_method` |
| 110 | `rmb ` | `rm  ` | `none` | 0x000040A6 | 0 | 0x40A6 | 0 | 18737 | `shader_beam` |
| 111 | `rmcs` | `rm  ` | `none` | 0x000040A7 | 0 | 0x40A7 | 0 | 18738 | `shader_custom` |
| 112 | `rmct` | `rm  ` | `none` | 0x000040A8 | 0 | 0x40A8 | 0 | 18739 | `shader_cortana` |
| 113 | `rmd ` | `rm  ` | `none` | 0x000040A9 | 0 | 0x40A9 | 0 | 18740 | `shader_decal` |
| 114 | `rmdf` | `none` | `none` | 0x000040AA | 0 | 0x40AA | 0 | 18741 | `render_method_definition` |
| 115 | `rmfl` | `rm  ` | `none` | 0x000040AB | 0 | 0x40AB | 0 | 18742 | `shader_foliage` |
| 116 | `rmhg` | `rm  ` | `none` | 0x000040AC | 0 | 0x40AC | 0 | 18743 | `shader_halogram` |
| 117 | `rmlv` | `rm  ` | `none` | 0x000040AD | 0 | 0x40AD | 0 | 18744 | `shader_light_volume` |
| 118 | `rmop` | `none` | `none` | 0x000040AE | 0 | 0x40AE | 0 | 18745 | `render_method_option` |
| 119 | `rmsh` | `rm  ` | `none` | 0x000040AF | 0 | 0x40AF | 0 | 18746 | `shader` |
| 120 | `rmsk` | `rm  ` | `none` | 0x000040B0 | 0 | 0x40B0 | 0 | 18747 | `shader_skin` |
| 121 | `rmt2` | `none` | `none` | 0x000040B1 | 0 | 0x40B1 | 0 | 18748 | `render_method_template` |
| 122 | `rmtr` | `rm  ` | `none` | 0x000040B2 | 0 | 0x40B2 | 0 | 18749 | `shader_terrain` |
| 123 | `rmw ` | `rm  ` | `none` | 0x000040B3 | 0 | 0x40B3 | 0 | 18750 | `shader_water` |
| 124 | `rsod` | `none` | `none` | 0x000040B4 | 0 | 0x40B4 | 0 | 18751 | `render_skins_object_globals` |
| 125 | `rwrd` | `none` | `none` | 0x000040B5 | 0 | 0x40B5 | 0 | 18752 | `render_water_ripple` |
| 126 | `sFdT` | `none` | `none` | 0x000040B6 | 0 | 0x40B6 | 0 | 18753 | `scenario_faux_data` |
| 127 | `sLdT` | `none` | `none` | 0x000040B7 | 0 | 0x40B7 | 0 | 18754 | `scenario_lightmap` |
| 128 | `sbsp` | `none` | `none` | 0x000040B8 | 0 | 0x40B8 | 0 | 18755 | `scenario_structure_bsp` |
| 129 | `scen` | `obje` | `none` | 0x000040B9 | 0 | 0x40B9 | 0 | 18756 | `scenery` |
| 130 | `scn3` | `none` | `none` | 0x000040BA | 0 | 0x40BA | 0 | 18757 | `gui_screen_widget_definition` |
| 131 | `scnr` | `none` | `none` | 0x000040BB | 0 | 0x40BB | 0 | 18758 | `scenario` |
| 132 | `sddt` | `none` | `none` | 0x000040BC | 0 | 0x40BC | 0 | 18759 | `structure_design` |
| 133 | `sdzg` | `none` | `none` | 0x000040BD | 0 | 0x40BD | 0 | 18760 | `scenario_required_resource` |
| 134 | `sefc` | `none` | `none` | 0x000040BE | 0 | 0x40BE | 0 | 18761 | `area_screen_effect` |
| 135 | `sfx+` | `none` | `none` | 0x000040BF | 0 | 0x40BF | 0 | 18762 | `sound_effect_collection` |
| 136 | `sgp!` | `none` | `none` | 0x000040C0 | 0 | 0x40C0 | 0 | 18763 | `sound_global_propagation` |
| 137 | `shit` | `none` | `none` | 0x000040C1 | 0 | 0x40C1 | 0 | 18764 | `shield_impact` |
| 138 | `sily` | `none` | `none` | 0x000040C2 | 0 | 0x40C2 | 0 | 18765 | `text_value_pair_definition` |
| 139 | `skn3` | `none` | `none` | 0x000040C3 | 0 | 0x40C3 | 0 | 18766 | `gui_skin_definition` |
| 140 | `sky*` | `none` | `none` | 0x000040C4 | 0 | 0x40C4 | 0 | 18767 | `scenario_sky_references_resource` |
| 141 | `skya` | `none` | `none` | 0x000040C5 | 0 | 0x40C5 | 0 | 18768 | `sky_atm_parameters` |
| 142 | `smap` | `none` | `none` | 0x000040C6 | 0 | 0x40C6 | 0 | 18769 | `shared_cache_file_layout` |
| 143 | `sncl` | `none` | `none` | 0x000040C7 | 0 | 0x40C7 | 0 | 18770 | `sound_classes` |
| 144 | `snd!` | `none` | `none` | 0x00001006 | 0 | 0x1006 | 0 | 6289 | `sound` |
| 145 | `snde` | `none` | `none` | 0x000040C8 | 0 | 0x40C8 | 0 | 18771 | `sound_environment` |
| 146 | `snmx` | `none` | `none` | 0x000040C9 | 0 | 0x40C9 | 0 | 18772 | `sound_mix` |
| 147 | `spk!` | `none` | `none` | 0x000040CA | 0 | 0x40CA | 0 | 18773 | `sound_dialogue_constants` |
| 148 | `ssce` | `obje` | `none` | 0x000040CB | 0 | 0x40CB | 0 | 18774 | `sound_scenery` |
| 149 | `sslt` | `none` | `none` | 0x000040CC | 0 | 0x40CC | 0 | 18775 | `scenario_structure_lighting_resource` |
| 150 | `stli` | `none` | `none` | 0x000040CD | 0 | 0x40CD | 0 | 18776 | `scenario_structure_lighting_info` |
| 151 | `stse` | `none` | `none` | 0x000040CE | 0 | 0x40CE | 0 | 18777 | `structure_seams` |
| 152 | `styl` | `none` | `none` | 0x000040CF | 0 | 0x40CF | 0 | 18778 | `style` |
| 153 | `tag+` | `none` | `none` | 0x000040D0 | 0 | 0x40D0 | 0 | 18779 | `tag_database` |
| 154 | `term` | `devi` | `obje` | 0x0000291E | 0 | 0x291E | 0 | 12713 | `device_terminal` |
| 155 | `trak` | `none` | `none` | 0x000040D1 | 0 | 0x40D1 | 0 | 18780 | `camera_track` |
| 156 | `trg*` | `none` | `none` | 0x000040D2 | 0 | 0x40D2 | 0 | 18781 | `scenario_trigger_volumes_resource` |
| 157 | `ttag` | `none` | `none` | 0x000040D3 | 0 | 0x40D3 | 0 | 18782 | `test_tag` |
| 158 | `txt3` | `none` | `none` | 0x000040D4 | 0 | 0x40D4 | 0 | 18783 | `gui_text_widget_definition` |
| 159 | `udlg` | `none` | `none` | 0x000040D5 | 0 | 0x40D5 | 0 | 18784 | `dialogue` |
| 160 | `ugh!` | `none` | `none` | 0x000040D6 | 0 | 0x40D6 | 0 | 18785 | `sound_cache_file_gestalt` |
| 161 | `uise` | `none` | `none` | 0x000040D7 | 0 | 0x40D7 | 0 | 18786 | `user_interface_sounds_definition` |
| 162 | `unic` | `none` | `none` | 0x000040D8 | 0 | 0x40D8 | 0 | 18787 | `multilingual_unicode_string_list` |
| 163 | `unit` | `obje` | `none` | 0x000040D9 | 0 | 0x40D9 | 0 | 18788 | `unit` |
| 164 | `uttt` | `none` | `none` | 0x000040DA | 0 | 0x40DA | 0 | 18789 | `tag_template_unit_test` |
| 165 | `vehi` | `unit` | `obje` | 0x000000C3 | 0 | 0xC3 | 0 | 195 | `vehicle` |
| 166 | `vtsh` | `none` | `none` | 0x000040DB | 0 | 0x40DB | 0 | 18790 | `vertex_shader` |
| 167 | `wacd` | `none` | `none` | 0x000040DC | 0 | 0x40DC | 0 | 18791 | `gui_widget_animation_collection_definition` |
| 168 | `wclr` | `none` | `none` | 0x000040DD | 0 | 0x40DD | 0 | 18792 | `gui_widget_color_animation_definition` |
| 169 | `weap` | `item` | `obje` | 0x000000C2 | 0 | 0xC2 | 0 | 194 | `weapon` |
| 170 | `wezr` | `none` | `none` | 0x000040DE | 0 | 0x40DE | 0 | 18793 | `game_engine_settings_definition` |
| 171 | `wfon` | `none` | `none` | 0x000040DF | 0 | 0x40DF | 0 | 18794 | `gui_widget_font_animation_definition` |
| 172 | `wgan` | `none` | `none` | 0x000040E0 | 0 | 0x40E0 | 0 | 18795 | `gui_widget_animation_definition` |
| 173 | `wgtz` | `none` | `none` | 0x000040E1 | 0 | 0x40E1 | 0 | 18796 | `user_interface_globals_definition` |
| 174 | `whip` | `none` | `none` | 0x000040E2 | 0 | 0x40E2 | 0 | 18797 | `cellular_automata2d` |
| 175 | `wigl` | `none` | `none` | 0x000040E3 | 0 | 0x40E3 | 0 | 18798 | `user_interface_shared_globals_definition` |
| 176 | `wind` | `none` | `none` | 0x000040E4 | 0 | 0x40E4 | 0 | 18799 | `wind` |
| 177 | `wpos` | `none` | `none` | 0x000040E5 | 0 | 0x40E5 | 0 | 18800 | `gui_widget_position_animation_definition` |
| 178 | `wrot` | `none` | `none` | 0x000040E6 | 0 | 0x40E6 | 0 | 18801 | `gui_widget_rotation_animation_definition` |
| 179 | `wscl` | `none` | `none` | 0x000040E7 | 0 | 0x40E7 | 0 | 18802 | `gui_widget_scale_animation_definition` |
| 180 | `wspr` | `none` | `none` | 0x000040E8 | 0 | 0x40E8 | 0 | 18803 | `gui_widget_sprite_animation_definition` |
| 181 | `wtuv` | `none` | `none` | 0x000040E9 | 0 | 0x40E9 | 0 | 18804 | `gui_widget_texture_coordinate_animation_definition` |
| 182 | `zone` | `none` | `none` | 0x000040EA | 0 | 0x40EA | 0 | 18805 | `cache_file_resource_gestalt` |

Observations on the class-name string ids (used in section 10):
181 ids are namespace 0, two are namespace 1 (`hlmt` 0x00080156 → `model`, `item`
0x00080010 → `item`); 7 ids have an index below the namespace-0 "min" of 1177 (engine
built-in strings: `cddf`, `devi`, `obje`, `vehi`, `weap`, ...), the other 176 are
map-table strings; the top 5 "length" bits are **0 in all 183**.

---

## 8. Tag index items (`IndexItem`, 8 bytes each)

| offset | type | Reclaimer | riverworld tag 8 (`scnr`) |
|---|---|---|---|
| 0x0 (0) | i16 | `ClassIndex` (`[Offset(0)]`) — index into the class table; **-1 = empty slot** | 131 (`scnr`) |
| 0x2 (2) | u16 | `Unknown` (`[Offset(2)]`) — the datum **salt**: the full 32-bit tag id used by every TagReference is `(salt << 16) \| index` | 0xE17E |
| 0x4 (4) | ptr32, shifted | `MetaPointer` (`[Offset(4)]`, `Pointer` with `PointerExpander`) — start of the tag's data; expand with section 4.3 | 0x731D5856 → 0x1CC756158 → file 0x5B29158 |

Facts verified on riverworld:

- `TagReference.TagId` in Reclaimer is `(short)(tagId & 0xFFFF)` (`Common\TagReference.cs`),
  i.e. only the low 16 bits select the tag. The high 16 bits equal the item's salt: all
  six global-tags entries match (`scnr` datum 0xE17E0008 → index 8, salt 0xE17E), and
  the `sbsp` reference inside the scenario reads 0x8E58261D → index 9757 whose item has
  salt 0x8E58.
- The salt is **not** derivable from the index. Tags 0..7 have salt 0xE175 + index,
  tag 8 has 0xE17E (0xE17D is skipped), and `salt − index` takes 39 distinct values
  across the 11,658 items. Always read it.
- Meta pointers are not monotonic in index order (tags 0..4 point into partitions 5, 0,
  1, 1, 2).
- No riverworld tag has a meta pointer of 0 and no name index is -1. Reclaimer has a
  special case for maps where the `play` tag's meta pointer is 0: it then reads the
  resource layout table at `zone`'s meta + 28 (`IndexItem.ReadMetadata`). Keep that rule.
- **The empty tag.** Reclaimer's comment says "every Halo3 map has an empty tag" and it
  skips items with `ClassIndex < 0`. riverworld, deadlock and cyberdyne have **none**;
  `005_intro` and `010_jungle` have one at index 11, `120_halo` one at index 9. The
  empty item is the 8 bytes `FF FF FF FF 00 00 00 00` (class -1, salt 0xFFFF, meta 0)
  and its tag-name index is -1. Rust: skip, but keep the index numbering (tag ids after
  it are unaffected).

Tags 0..9 of riverworld (expected values):

| index | class | salt | meta ptr (stored) | <<2 (virtual) | file offset | name |
|---|---|---|---|---|---|---|
| 0 | `draw` | 0xE175 | 0x731D0000 | 0x1CC740000 | 0x5B13000 | `` |
| 1 | `play` | 0xE176 | 0x730EF8C4 | 0x1CC3BE310 | 0x5791310 | `` |
| 2 | `ugh!` | 0xE177 | 0x73114933 | 0x1CC4524CC | 0x58254CC | `i've got a lovely bunch of coconuts` |
| 3 | `zone` | 0xE178 | 0x731412C2 | 0x1CC504B08 | 0x58D7B08 | `there they are all standing in a row` |
| 4 | `matg` | 0xE179 | 0x73196054 | 0x1CC658150 | 0x5A2B150 | `globals\globals` |
| 5 | `sncl` | 0xE17A | 0x731D0001 | 0x1CC740004 | 0x5B13004 | `sound\sound_classes` |
| 6 | `spk!` | 0xE17B | 0x731D0004 | 0x1CC740010 | 0x5B13010 | `sound\combat_dialogue_constants` |
| 7 | `scen` | 0xE17C | 0x731D000E | 0x1CC740038 | 0x5B13038 | `objects\cinematics\cinematic_anchor\cinematic_anchor` |
| 8 | `scnr` | 0xE17E | 0x731D5856 | 0x1CC756158 | 0x5B29158 | `levels\multi\riverworld\riverworld` |
| 9 | `wind` | 0xE17F | 0x731D5A7C | 0x1CC7569F0 | 0x5B299F0 | `levels\multi\riverworld\wind_riverworld` |

The `sbsp` and the system tags (expected values):

| index | class | salt | meta ptr (stored) | <<2 (virtual) | file offset | name |
|---|---|---|---|---|---|---|
| 9757 | `sbsp` | 0x8E58 | 0x73E3333D | 0x1CF8CCCF4 | 0x8C9FCF4 | `levels\multi\riverworld\riverworld` |
| 8 | `scnr` | 0xE17E | 0x731D5856 | 0x1CC756158 | 0x5B29158 | `levels\multi\riverworld\riverworld` |
| 4 | `matg` | 0xE179 | 0x73196054 | 0x1CC658150 | 0x5A2B150 | `globals\globals` |
| 1 | `play` | 0xE176 | 0x730EF8C4 | 0x1CC3BE310 | 0x5791310 | `` |
| 3 | `zone` | 0xE178 | 0x731412C2 | 0x1CC504B08 | 0x58D7B08 | `there they are all standing in a row` |
| 2 | `ugh!` | 0xE177 | 0x73114933 | 0x1CC4524CC | 0x58254CC | `i've got a lovely bunch of coconuts` |
| 0 | `draw` | 0xE175 | 0x731D0000 | 0x1CC740000 | 0x5B13000 | `` |

Class histogram (top): `snd!` 2290, `bitm` 2204, `rmsh` 939, `effe` 887, `prt3` 584,
`pixl` 473, `vtsh` 472, `rmt2` 367, `mode` 322, `hlmt` 255, `phmo` 233, `coll` 220.
Last tag: 11657 `bitm` salt 0x8F3D meta 0x73FFCD28 (→ file 0x93C64A0)
`levels\multi\riverworld\riverworld_lightmap_riverworld_16f_lp_array_intensity_dxt5`.

---

## 9. Tag names

`TagIndex.ReadItems` (`MccHalo3\CacheFile.cs`): read `tag count` i32s at
`FileTableIndexPointer`; for each tag, if the value is -1 the tag has no name, otherwise
it is a byte offset into the name table at `FileTablePointer` where a NUL-terminated
string starts. Names are tag paths with backslashes and no extension
(`levels\multi\riverworld\riverworld`); lower-case in practice, and Reclaimer compares
them case-insensitively. The class is not part of the name, so names are
**not unique**: the `scnr` and the `sbsp` of riverworld both are
`levels\multi\riverworld\riverworld`. Names may be empty: tags 0 (`draw`) and 1 (`play`)
have offsets 0 and 1, both pointing at NUL bytes. riverworld offsets are strictly
increasing: `0, 1, 2, 38, 75, ...`; `FileTableSize` = 0x9B515 bytes.

---

## 10. String table and string ids

### 10.1 The table (`StringIndex.ReadItems`, `MccHalo3\CacheFile.cs`)

Read `string count` i32s at `StringTableIndexPointer` (riverworld file 0x4000); entry
*k* < 0 means "no string", otherwise it is a byte offset into the blob at
`StringTablePointer` (file 0x17000, `StringTableSize` = 0x5377C bytes) of a
NUL-terminated string. riverworld: 18,806 strings, no negative entries, offsets start
`0, 1, 9, 18, 27, 37`; strings 0..7 are `""`, `default`, `reload_1`, `reload_2`,
`chamber_1`, `chamber_2`, `fire_1`, `fire_2`. Entry 18805 (the last) is
`cache_file_resource_gestalt`.

### 10.2 String id bit layout (U13)

`Resources\MccHalo3Strings.xml`, set `id="U13"`: `indexBits=19, namespaceBits=8,
lengthBits=5` (19 + 8 + 5 = 32). From a 32-bit id:

```
index     = id & 0x7FFFF            // low 19 bits
namespace = (id >> 19) & 0xFF       // next 8 bits
length    = id >> 27                // top 5 bits; ignored (0 in every id checked)
```

The XML comment says: from U6 on the namespaces come from the header, but the bit
layout still needs this definition. The same file's `U6` set is 17/8/7; using 17 index
bits on a U13 id would mis-resolve the two namespace-1 class names (0x00080156 would
become namespace 4), so the 19-bit layout is confirmed by data.

### 10.3 Namespaces from the header (`StringIdTranslator`, header-based constructor, `Common\StringIdTranslator.cs`)

Read `StringNamespaceCount` u32s at `StringNamespaceTablePointer` (riverworld file
0x6B000) and mask each with `0x7FFFF` (the upper bits are 0 in practice). Entry *k* is
the number of strings of namespace *k* that sit in the table's "prefix". Build the
mapping like this (Reclaimer's comment: "namespace 0 always starts at the end of the
rest"):

```
mask  = (1 << 19) - 1
start = ns_raw[0] & mask                       // namespace 0's prefix strings occupy [0, start)
for k in 1 .. count-1:
    ns[k] = { min: 0, start: start }
    start += ns_raw[k] & mask
ns[0] = { min: ns_raw[0] & mask, start: start }  // the rest of namespace 0 follows all others

resolve(id):
    index = id & mask
    k     = (id >> 19) & 0xFF
    while k not in ns and k > 0: k -= 1        // unknown namespace falls back downwards
    if index < ns[k].min: table_index = index   // only possible for k = 0
    else:                 table_index = index - ns[k].min + ns[k].start
```

If `StringNamespaceCount <= 1` Reclaimer leaves the namespace map empty (older/odd
caches); a Rust implementation should then treat the id's index as the table index.

riverworld namespace table (raw u32s): `[0x499, 0x67A, 0x92, 0x56, 0x10, 0x4C, 0x39, 0x14, 0xD, 0x73]`
= `[1177, 1658, 146, 86, 16, 76, 57, 20, 13, 115]`, sum 3364. The same ten values
appear in deadlock, cyberdyne, 005_intro, 010_jungle and 120_halo, so they are constants
of this build (the engine's static string sets), and the remaining
18,806 − 3364 = 15,442 strings are map-specific namespace-0 strings. Resulting mapping:

| namespace | min | start (table index) | first strings there |
|---|---|---|---|
| 0 | 1177 (0x499) | 3364 (0xD24) | `material`, `liquid`, `liquid_thin` (table [0, 1177) holds namespace 0 ids with index < 1177: `""`, `default`, `reload_1`, ...) |
| 1 | 0 | 1177 (0x499) | `primary_label`, `secondary_label`, `tertiary_label` |
| 2 | 0 | 2835 (0xB13) | `alpha_custom_games_disabled`, ... |
| 3 | 0 | 2981 (0xBA5) | |
| 4 | 0 | 3067 (0xBFB) | |
| 5 | 0 | 3083 (0xC0B) | |
| 6 | 0 | 3159 (0xC57) | |
| 7 | 0 | 3216 (0xC90) | |
| 8 | 0 | 3236 (0xCA4) | |
| 9 | 0 | 3249 (0xCB1) | |

Worked examples (all verified against the class table and the locale table):

| string id | namespace | index | rule | table index | string |
|---|---|---|---|---|---|
| 0x000040D4 | 0 | 0x40D4 = 16596 | ≥ 1177 → 16596 − 1177 + 3364 | 18783 | `gui_text_widget_definition` (`txt3`) |
| 0x000040EA | 0 | 16618 | 16618 − 1177 + 3364 | 18805 | `cache_file_resource_gestalt` (`zone`), the last string |
| 0x00004045 | 0 | 16453 | | 18640 | `shader_contrail` (class 0 ` rmc`) |
| 0x000000C3 | 0 | 195 | < 1177 → unchanged | 195 | `vehicle` (`vehi`) |
| 0x0000040A | 0 | 1034 | < 1177 | 1034 | `collision_damage` (`cddf`) |
| 0x00080156 | 1 | 0x156 = 342 | 342 − 0 + 1177 | 1519 | `model` (`hlmt`) |
| 0x00080010 | 1 | 16 | 16 + 1177 | 1193 | `item` (`item`) |
| 0x00080076 | 1 | 118 | 118 + 1177 | 1295 | `campaign`, the key of the English locale string `Campaign` (section 13) |

`StringIndexBase.TryGetValue` returns false (Reclaimer shows `<invalid>`) when the
resolved table index is outside `[0, string count)`; a Rust `Option<&str>` should do the
same. Reverse lookup (`GetStringId`) picks the namespace with the largest `start` ≤ table
index and returns `(k << 19) | (table_index - start + min)`.

### 10.4 Reading a StringId field

`Common\StringId.cs`: for Halo 3 and later a StringId is an **i32** in tag data
(Halo 1/2 used i16). No pointer, no translation.

---

## 11. The three tag-data primitives

| primitive | size (U13) | layout | Reclaimer |
|---|---|---|---|
| TagBlock | 12 | `i32 count`, `i32 pointer` (shifted, section 4.3), 4 bytes padding | `Common\TagBlock.cs`; a block is treated as empty/invalid when `count <= 0` or the translated address is < 0 or beyond the file |
| TagReference | 16 | `i32 class id` (4CC as LE int, section 7), 8 bytes skipped (zero in the file; the engine's run-time name pointer slot), `i32 tag datum` (`(salt << 16) \| index`; -1 = null) | `Common\TagReference.cs`: reads class, seeks +8, reads id; valid when `(short)(id & 0xFFFF) >= 0` |
| StringId | 4 | `i32` id, section 10 | `Common\StringId.cs` |

Data pointers (raw byte buffers inside tags) follow the TagBlock pattern of a 32-bit
shifted pointer; Reclaimer has no separate class for them in the files read here.
Matrices are read as 3×4 floats (`CacheFactory.CreateReader` registers `ReadMatrix3x4`
for Halo 2 Xbox and later).

End-to-end check performed (`Halo3\Config\ScenarioTag.config.cs`, U13 version:
`StructureBsps` at scenario + 24, `StructureBspBlock.BspReference` at + 0):
scenario meta file 0x5B29158, at +24 the TagBlock is `{count 1, pointer 0x731D004E, pad 0}`;
0x731D004E << 2 = 0x1CC740138 → file 0x5B13138; there the 16-byte TagReference is
class `sbsp`, 8 zero bytes, datum 0x8E58261D → tag 9757 (`sbsp`, salt 0x8E58,
`levels\multi\riverworld\riverworld`). Shifted pointers, TagBlock, TagReference and the
salt rule all agree.

---

## 12. Finding the scenario; what the global tags are

`TagIndex.ReadItems` (`MccHalo3\CacheFile.cs`): the scenario is the **single** tag whose
class is `scnr` and whose name equals the header's `ScenarioName` (case-insensitive);
Reclaimer throws `AmbiguousScenarioReference` if there is not exactly one. riverworld
has exactly one `scnr`, index 8, name `levels\multi\riverworld\riverworld`. The
global-tags table (section 6.1) gives the same answer directly (`scnr` datum
0xE17E0008). Campaign maps follow the same rule (`005_intro` →
`levels\solo\005_intro\005_intro`).

Reclaimer's "system classes" (`CacheFactory.SystemClasses`) and what they are, with the
class names resolved from riverworld's own string table:

| code | long name (from class table) | role | riverworld |
|---|---|---|---|
| `scnr` | `scenario` | the level: BSP list, object placements, scripts | tag 8 |
| `matg` | `globals` | engine globals (`globals\globals`); also holds the locale language definitions (section 13) | tag 4 |
| `play` | `cache_file_resource_layout_table` | where resource pages live (this file's section 1, `shared.map`, `campaign.map`), compression codecs | tag 1, empty name |
| `zone` | `cache_file_resource_gestalt` | the resource index: every bitmap / geometry / sound resource of the map, pointing into `play`'s pages | tag 3, name `there they are all standing in a row` |
| `ugh!` | `sound_cache_file_gestalt` | the same for sounds | tag 2, name `i've got a lovely bunch of coconuts` |
| `draw` | `rasterizer_cache_file_globals` | in the engine's global-tags table but not in Reclaimer's list | tag 0, empty name, meta at partition 5's start |

Reclaimer eagerly reads `play`, `zone` and `scnr` in a background task right after
opening the cache (`CacheFileU6` constructor), because almost every other read needs
them. Resource pages themselves (section 1 and the shared caches) are outside this
document.

---

## 13. Locale index (noted only; we skip it)

`MccHalo3\CacheFileU6.cs`: for Halo 3 with `CacheType >= MccHalo3U12`,
`LocaleIndex(cache, offset 24, size 80, count 12)`: twelve `LanguageDefinition`s
(`Common\Gen3\LanguageDefinition.cs`: `i32 StringCount` @0, `i32 StringsSize` @4,
`i32 IndicesOffset` @8, `i32 StringsOffset` @12) at `matg meta + 24 + 80 × language`.
The two offsets are **section-3** pointers (`SectionAddressTranslator(cache, 3)`,
i.e. `+ section_offsets[3]`). At `IndicesOffset` there are `StringCount` entries of
`{i32 string id, i32 byte offset}` (`LocaleEntry`, 8 bytes; offset < 0 = none), the
strings are NUL-terminated UTF-8 at `StringsOffset + byte offset`. Halo 3 locales are not
encrypted (only Reach/Halo 4 are). Verified on riverworld (`matg` meta file 0x5A2B150):
language 0 = `{3182, 0x14964, 0x05FCE000 → file 0x93D3000, 0x05FD5000 → file 0x93DA000}`,
entry 0 = `{0x00080076, 0}` → `Campaign`; language 2 → `Kampagne`; language 1 is
Japanese UTF-8. Locale section file range 0x93D3000 .. 0x952C000.

---

## 14. `riverworld.map` vs `shared.map` vs `campaign.map`

| field | riverworld | shared.map | campaign.map |
|---|---|---|---|
| file size | 0x0952C000 = 156,418,048 | 0x37C81000 = 935,858,176 | 0x04437000 = 71,528,448 |
| version / 0x0C | 13 / 0x202 | 13 / 0x202 | 13 / 0x202 |
| 0x18 cache type | 1 (multiplayer) | 3 (shared) | 4 (campaign shared) |
| 0x1A slot | -1 | 2 | 3 |
| 0x1C, 0x1E | 230, 4 | 4, 0 | 6, 0 |
| tag count (0x20) | 11,658 | **0** | **0** |
| string count (0x30), namespaces (0x40) | 18,806 / 10 | 0 / 0 | 0 / 0 |
| all section-0 pointers (0x24..0x44) | set | 0 | 0 |
| 0x48 | 0 | 0xFFFFFFFF00000000 | 0xFFFFFFFFFFFFFFFF |
| 0x50 own id | 0x01DA34A99B164C65 | 0x797B2A3473952700 | 0x198AC984A3BD6780 |
| 0x68 (slot 2) | 0x797B2A3473952700 (= shared.map) | 0 | 0 |
| 0x70 (slot 3) | 0 | 0 | 0 |
| build string | `Dec 21 2023 22:31:37` | same | same |
| map name (0xC0) | `riverworld` | `shared` | `campaign` |
| scenario path (0xE0) | `levels\multi\riverworld\riverworld` | `maps\shared.map` | `maps\campaign.map` |
| virtual base, index pointer | 0x1CC380000, 0x1CC549AF0 | 0, 0 | 0, 0 |
| partitions | 6 ranges, sum 0x3C80000 | all zero | all zero |
| section offsets | `[0xF9EDD000, 0x114000, 0xFF51C000, 0x3405000]` | `[0, 0x4000, 0, 0]` | `[0, 0x4000, 0, 0]` |
| sections | 4 non-empty | only [1] `{0, 0x37C7D000}` → file 0x4000..0x37C81000 | only [1] `{0, 0x04433000}` → file 0x4000..0x4437000 |
| footer 0x3FFC | `foot` | `foot` | `foot` |

So `shared.map` and `campaign.map` have **no tags, no strings, no tag index**: they are a
header plus one resource section (raw pages for bitmaps, sounds, geometry) that level
maps reference through their `play` tag. Every multiplayer and campaign map we checked
carries `shared.map`'s id in slot 2 (0x68); the campaign levels that need it
(`010_jungle`, `120_halo`, but not `005_intro`) also carry `campaign.map`'s id in slot 3
(0x70). `campaign.map` additionally has non-zero data at 0x500..0x1720 in its header
(unknown, not needed). Opening either shared file with the level-map reader must not
fail: tag count 0 and index pointer 0 are legitimate; the Rust `Header` parser should
succeed and the tag index loader should return an empty index.

---

## 15. Expected values for `blam_cache` unit tests (riverworld.map, build `Dec 21 2023 22:31:37`)

Header: see the riverworld column of section 3 (magic, version 13, file size 0x0952C000,
tag count 11658, string count 18806, namespace count 10, build string, map name,
scenario path, virtual base 0x1CC380000, index pointer 0x1CC549AF0, section offsets,
sections, partitions, footer `foot`).

Translation: string index file 0x4000; string table 0x17000; namespace table 0x6B000;
tag names 0x6C000; tag-name index 0x108000; tag section file base 0x05753000;
tag index file 0x0591CAF0; class table 0x0591BF80; tag table 0x05905330; global-tags
table 0x05905300; resource section base 0x114000; locale section base 0x093D3000.

Tag index: 183 classes, 11658 tags, 6 global tags, footer `tags`, u32 at +0x40 = 0x628889E4.

Classes: 183 rows of section 7; class 0 ` rmc` = `shader_contrail` (id 0x00004045),
class 128 `sbsp` = `scenario_structure_bsp` (0x000040B8), class 131 `scnr` = `scenario`
(0x000040BB), class 182 `zone` = `cache_file_resource_gestalt` (0x000040EA); all 183
names resolve; `vehi` parent `unit` grandparent `obje`.

Namespaces: raw `[1177, 1658, 146, 86, 16, 76, 57, 20, 13, 115]`; starts
`ns1 1177, ns2 2835, ns3 2981, ns4 3067, ns5 3083, ns6 3159, ns7 3216, ns8 3236,
ns9 3249, ns0 (min 1177) 3364`; the worked examples of section 10.3.

Tags: the two tables of section 8 (tags 0..9; `sbsp` 9757 salt 0x8E58 meta 0x73E3333D →
file 0x8C9FCF4; `scnr` 8; `matg` 4; `play` 1; `zone` 3; `ugh!` 2; `draw` 0); no empty
tag; no -1 name; no zero meta pointer; exactly one `scnr`; class histogram `snd!` 2290,
`bitm` 2204; last tag 11657.

End-to-end: scenario + 24 → TagBlock `{1, 0x731D004E}` → file 0x5B13138 → TagReference
`sbsp`, datum 0x8E58261D → tag 9757.

Other maps (for a second fixture): deadlock 11848 tags, cyberdyne 11358, 005_intro 6690
(empty tag at 11), 010_jungle 14592 (empty at 11), 120_halo 13900 (empty at 9); all
183 classes, 6 global tags, identical namespace counts.

---

## 16. Open questions

- Header fields marked *observed* (0x0C = 0x202, 0x1C/0x1E, 0x48, the hash-like blocks
  at 0x80, 0x360-0x4CC, 0x4FC-0x540, 0x2820-0x28C0, and campaign.map's 0x500-0x1720)
  are not needed to read a map and were not decoded. The cache-type at 0x18 and the id
  slots at 0x50/0x58+ are inferred from eleven maps, not from Reclaimer.
- The u32 at tag index + 0x40 differs per map; we did not check whether it is a
  checksum of anything.
- The 5 "length" bits of string ids were 0 in every id we looked at (183 class names,
  one locale key); whether any id in tag data uses them is unknown. Mask them off.
- Partition names are the conventional Halo 3 names and match the observed contents,
  but no tool we read uses them; treat them as informational.
- Resource pages (section 1, `play`, `zone`, `shared.map`/`campaign.map` lookup) and
  the real collision BSP are separate documents.

Re-verify everything with: `python -I tools/verify_container.py` (default maps folder is
the Steam MCC install; pass another folder as the first argument).
