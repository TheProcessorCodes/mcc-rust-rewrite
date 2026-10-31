# MCC Halo 3 (U13) resource system: from a tag's resource id to raw bytes

Target: MCC Halo 3 build `Dec 21 2023 22:31:37` = Reclaimer `CacheType.MccHalo3U13`.
Primary test file: `riverworld.map` (Valhalla). Shared resources: `shared.map`, `campaign.map`.

Everything here is a format fact (offsets, sizes, meanings, algorithms in our own words),
not Reclaimer code. Each fact names the Reclaimer source file and member it was read from
so a reviewer can re-check it. Reclaimer lives at <https://github.com/Gravemind2401/Reclaimer>
(GPL-3.0, read-only); file paths below are relative to `Reclaimer.Blam\Blam\` unless noted.

Verification was done with `tools\probe_resources.py` (run it as
`python -I tools\probe_resources.py "<maps dir>" riverworld.map`); its full output is kept at
`tools\probe_riverworld.txt`. All numbers marked **EXPECTED** come from that run.

---

## 0. How Reclaimer picks offsets per version (needed to resolve U13)

Source: `Reclaimer.Core\IO\Dynamic\Extensions.cs` `ValidateVersion`, and
`Reclaimer.Core\IO\Dynamic\StructureDefinition.cs` constructor + `FindVersionDefinition`.

- A version range `(MinVersion, MaxVersion)` matches when `version >= Min` **and** `version < Max`.
  **Min is inclusive, Max is exclusive.** The only exception is `Min == Max`, which matches that
  one version exactly. A missing Min or Max is unbounded.
- `[Offset(x, MinVersion=A, MaxVersion=B)]` on a property, `[FixedSize(n, MinVersion=A, MaxVersion=B)]`
  on a class and `AddVersion(A, B)` in a `DefinitionBuilder` all use that same rule.
- When several ranges match (common: `AddVersion(Halo3Retail, null)` and `AddVersion(MccHalo3, null)` both
  match U13), the ranges are sorted so that **exact versions come first, then the highest MinVersion
  wins**, and the unversioned default comes last. So for U13 the `MccHalo3..` branch beats the
  `Halo3Retail..` and `Halo3Delta..` branches. Confirmed against bytes: with the MccHalo3 branch the
  zone tag yields 7428 resource entries and a valid fixup pointer; the Retail and Delta branches yield
  zeros (see section 11).
- Enum order (`Common\Enums.cs` `CacheType`): `... Halo3Delta < Halo3Beta < Halo3Retail < MccHalo3 <
  MccHalo3U4 < MccHalo3F6 < MccHalo3U6 < MccHalo3U9 < MccHalo3U12 < MccHalo3U13 < Halo3ODST <
  MccHalo3ODST < MccHalo3ODSTF3 < MccHalo3ODSTU3 < ...`. U13 therefore satisfies every
  `MinVersion = MccHalo3 / MccHalo3U4 / MccHalo3U6` test and fails every `MaxVersion = Halo3ODST` test
  only when `version < Halo3ODST`, which is true, so ranges ending at `Halo3ODST` still include U13.

The cache type comes from the header build string (`Common\Enums.cs`, `[BuildString("Dec 21 2023 22:31:37", "U13")]`
on `MccHalo3U13`). The `"U13"` second argument is the string-id collection name, not a codec.

---

## 1. The pieces

| piece | tag class | Reclaimer type | job |
|---|---|---|---|
| resource gestalt | `zone` (`cache_file_resource_gestalt`) | `Halo3\CacheFileResourceGestaltTag` | one entry per resource: owner tag, salt, type, fixups, which segment |
| resource layout table | `play` (`cache_file_resource_layout_table`) | `Halo3\CacheFileResourceLayoutTableTag` | shared cache file names, pages (compressed blobs), segments (where inside a page a resource starts) |
| fixup data | a byte blob owned by `zone` | `FixupDataPointer` / `FixupDataSize` | holds every resource's "definition" structure (vertex/index buffer headers etc.) |
| resource id | an `i32` inside any tag | `Halo3\ResourceIdentifier` | `(salt << 16) | entry index` |

Both `zone` and `play` are **global tags**: Reclaimer finds them by class code while reading the tag
index (`MccHalo3\CacheFile.cs` `TagIndex.ReadItems`, `CacheFactory.SystemClasses = scnr, matg, ugh!,
play, zone`; `GetGlobalTag(classCode)`). There is exactly one of each per map. After opening a map,
Reclaimer kicks off a background task that pre-reads `play`, `zone` and `scnr` (`MccHalo3\CacheFileU6.cs`
constructor, `Task.Factory.StartNew`); that is only a warm-up, not a format fact.

**The `play` -> `zone+28` redirect** (`MccHalo3\CacheFile.cs`, `IndexItem.ReadMetadataInternal`):
if the `play` tag's index item has meta pointer `0`, Reclaimer reads the layout table at
`zone meta address + 28` instead. That is a workaround for maps whose `play` tag was stripped and the
layout table embedded in the gestalt. **riverworld.map does not need it**: its `play` tag (index
`0x1`, empty name) has a real meta pointer (`0x730EF8C4` shifted -> file `0x05791310`), and
`zone+28` is zeros. Implement the fallback anyway; it costs one comparison.

**EXPECTED (riverworld)**: `zone` = tag index `0x3`, name `there they are all standing in a row`,
meta pointer `0x731412C2` (shifted) -> file `0x058D7B08`. `play` = tag index `0x1`, meta at file `0x05791310`.

---

## 2. The resource identifier (the `i32` stored in a tag)

Source: `Halo3\ResourceIdentifier.cs` (`Value`, `ResourceIndex => Value & 0xFFFF`), confirmed on bytes.

| bits | meaning |
|---|---|
| 15..0 | index into the gestalt's resource entry table |
| 31..16 | salt; must equal the `u16` at `entry + 0x10` of that entry |

A reference whose salt does not match the entry's salt is stale: the entry was removed
(removed entries are all `0xFF` in the first 20 bytes: owner class `-1`, owner tag `-1`, salt `0xFFFF`,
type `0xFF`). Reclaimer does not check the salt; we should, and treat a mismatch as "no data".
An id of `-1` / `0xFFFFFFFF` means "none".

**EXPECTED (riverworld)**: the `Lbsp` render-geometry id `0xEE3D0CDA` -> index `0x0CDA` = 3290,
salt `0xEE3D`; entry 3290 has salt `0xEE3D` at `+0x10`. The `sbsp` id at `sbsp+0x254` is `0xFDE91C86`
-> index 7302, salt `0xFDE9`, matches. The `sbsp` id at `sbsp+0x36C` is `0xFDEA1C87` -> index 7303 whose
entry is wiped (salt `0xFFFF`): a dead reference. Salts in riverworld run `0xE163..0xFFFF`,
7176 distinct values over 7428 entries (the 253 wiped entries share `0xFFFF`).

---

## 3. `zone` tag layout (`cache_file_resource_gestalt`), U13

Reclaimer (`Halo3\Config\CacheFileResourceGestaltTag.config.cs`, `DefinitionBuilder.Halo3()`,
branch `AddVersion(CacheType.MccHalo3, null)`) only declares three fields; the rest below was
read from the bytes and is marked accordingly.

| offset (hex / dec) | type | field | source |
|---|---|---|---|
| 0x00 / 0 | i32 | unknown, value `1` in riverworld (map type?) | bytes; **unsure** |
| 0x04 / 4 | TagBlock (12) | resource types, 28 bytes each (section 3.3) | bytes + string ids |
| 0x10 / 16 | TagBlock (12) | resource structure types, 20 bytes each (section 3.3) | bytes + string ids |
| 0x1C..0x63 / 28..99 | 72 bytes | all zero in riverworld; this is where the embedded layout table sits when `play` is stripped (section 1) | bytes; Reclaimer hack |
| 0x64 / 100 | TagBlock (12) | **resource entries**, 64 bytes each | Reclaimer `ResourceEntries` HasOffset(100) |
| 0x148 / 328 | i32 | **fixup data size** | Reclaimer `FixupDataSize` HasOffset(328) |
| 0x14C / 332 | 8 bytes | zero (the rest of a 20-byte TagData: size, 2 x unused, pointer, unused) | bytes |
| 0x154 / 340 | i32 ptr (shifted) | **fixup data pointer** | Reclaimer `FixupDataPointer` HasOffset(340) |

For comparison, the retail Xbox 360 branch is 88 / 316 / 328 and the pre-release branches 24/48/60 and
36/132/144; MCC inserted 12 bytes before offset 88 (the resource-structure-types block, presumably).

**EXPECTED (riverworld)**: `zone+100` = {count 7428, ptr `0x7311C3AB`} -> entries at file `0x05843EAC`;
`zone+328` = `0xA2B10` = 666384 bytes of fixup data; `zone+340` = `0x7316D590` -> file `0x05988640`.

### 3.1 Resource entry (64 bytes, `FixedSize(64)` for `Halo3Retail..`; Reclaimer `ResourceEntryBlock`)

| offset | type | field | meaning | source |
|---|---|---|---|---|
| 0x00 / 0 | TagReference (16) | owner | class id at +0 (reversed 4CC), 8 skip, tag id (i32) at +12; low 16 bits = tag index | Reclaimer `OwnerReference` @0 |
| 0x10 / 16 | u16 | salt | high half of the resource id | Reclaimer reads an i32 "ResourcePointer" here but never uses it; bytes show u16 salt + 2 bytes |
| 0x12 / 18 | u8 | resource type index | index into the resource types table at `zone+4` (section 3.3) | bytes |
| 0x13 / 19 | u8 | flags | `2` on geometry/animation/bitmap entries, `4` on the collision (`structure_bsp_tag_resources`) entry; meaning **unsure** | bytes |
| 0x14 / 20 | i32 | fixup (definition) data offset | byte offset into the fixup data blob | Reclaimer `FixupOffset` @20 |
| 0x18 / 24 | i32 | fixup (definition) data length | length of this entry's slice of the blob | Reclaimer `FixupSize` @24 |
| 0x1C / 28 | i32 | secondary fixup offset | `-1` on every entry inspected | bytes; **unsure** |
| 0x20 / 32 | i16 | location type / flags | `0` = no pages (sounds: 2216 entries), `1` = primary page only (3644), `3` = primary + secondary page (1315). Looks like bit0 = has primary, bit1 = has secondary | Reclaimer `LocationType` @32; histogram from bytes |
| 0x22 / 34 | i16 | segment index | into the `play` segments table; `-1` = no data | Reclaimer `SegmentIndex` @34 |
| 0x24 / 36 | u32 | definition address | a cache address (3.2): type 1 = definition (top 3 bits `001`, shows as nibble `0x2`) + 29-bit offset of the root definition struct inside this entry's definition data (section 6). riverworld: 7175 entries type 1, the other 253 hold 0 (corrected: 29-bit form, was "nibble + offset") | bytes |
| 0x28 / 40 | TagBlock (12) | resource fixups, 8 bytes each | Reclaimer `ResourceFixups` @40 |
| 0x34 / 52 | TagBlock (12) | definition fixups, 8 bytes each | Reclaimer `ResourceDefinitionFixups` @52 |

Reclaimer's pre-release fields (`CacheIndex`, `PrimaryOffset`, ... at 36..60 for `..Halo3Delta` and
`Halo3Delta..Halo3Retail`, sizes 88 and 96) do not exist in U13.

### 3.2 Fixup records

Resource fixup, 8 bytes (`ResourceFixupBlock`, `FixedSize(8)`; Reclaimer names: `Unknown` @0, `Offset` @4,
`MaskedOffset = Offset & 0x0FFFFFFF`):

| offset | type | meaning |
|---|---|---|
| 0 | i32 | where to write: byte offset inside this entry's definition data |
| 4 | u32 | what to write: a "cache address" = **3-bit type (`value >> 29`) + 29-bit offset (`value & 0x1FFFFFFF`)**, TagTool `CacheAddress` (collision-bsp-research.md 4.4). Reclaimer masks with `0x0FFFFFFF` and its own source comment marks that "4 bit / 28 bit" split with a `?`; both readings give the same offset for every value in riverworld because bit 28 is never set (checked over all 28,906 resource fixups, 9,472 definition fixups and 7,428 definition addresses). Use the 29-bit form; it is also what tells definition / primary / secondary apart |

(corrected: this section used to call the value "top nibble region + 28-bit offset"; the nibbles `0x2`, `0x4`, `0x6` are types 1, 2, 3 with bit 28 clear.)

Types seen in riverworld (histogram over all 7428 entries: type 2 (nibble `0x4`) x15226, type 1 (`0x2`) x12365, type 3 (`0x6`) x1315):

| type (top nibble) | region the 29-bit offset is relative to | evidence |
|---|---|---|
| 1 (`0x2`) definition | this entry's definition data (fixup-data slice) | values land exactly on the 12-byte D3D structs / 28-byte buffer headers of the same slice |
| 2 (`0x4`) data | primary page data, relative to the **segment start** (`segment.primary offset`), not to the page start | Reclaimer `Halo3Common.GetMeshes` seeks `MaskedOffset` inside the segment-relative data; verified: index buffer bytes `00 00 01 00 02 00 03 00 ...` appear at `segment + offset` and garbage at `page + offset` |
| 3 (`0x6`) secondary data | secondary page data, relative to the secondary segment offset | count 1315 equals the number of entries with location type 3 (has secondary page) |

Definition fixup, 8 bytes (`ResourceDefinitionFixupBlock`, `FixedSize(8)`; Reclaimer names `Offset` @0, `Unknown` @4):

| offset | type | meaning |
|---|---|---|
| 0 | u32 | cache address (always type 1 = definition, nibble `0x2`, in riverworld: 9472 of 9472) of a D3D runtime struct inside the definition data |
| 4 | i32 | index into the resource structure types table (`zone+16`): `0` = `s_tag_d3d_vertex_buffer`, `1` = `s_tag_d3d_index_buffer`, ... |

In other words: resource fixups patch pointers into the definition data; definition fixups tell the
engine "a D3D object of type N lives here, create it". For a static reader we only need the resource fixups.

### 3.3 Type tables (bytes + string ids; not declared in Reclaimer)

Resource types at `zone+4`, 28 bytes each: `guid[16]`, 4 x i16 (unknown), string id name at +24.
**EXPECTED (riverworld, 7 entries)**:

| index | name | used by (owner class histogram) |
|---|---|---|
| 0 | `bitmap_texture_interop_resource` | `bitm` x2447 |
| 1 | `render_geometry_api_resource_definition` | `mode` x322, `pmdf` x19, `Lbsp` x1, `sbsp` x1 |
| 2 | `constant_buffer_resource_definition` | `rmsh` x563, `prt3` x578, `decs` x181, `ltvl` x64, `cntl` x41, `rmd ` x22, `beam` x9, `rmtr` x6, `rmfl` x5 (MCC-only shader constant buffers) |
| 3 | `sound_resource_definition` | `snd!` x2216 (location type 0: no page data in the map) |
| 4 | `model_animation_tag_resource` | `jmad` x698 |
| 5 | `structure_bsp_tag_resources` | `sbsp` x1 (the **collision BSP** resource; see section 9) |
| 6 | `structure_bsp_cache_file_tag_resources` | `sbsp` x1 |

Plus 253 wiped entries (type `0xFF`).

Resource structure types at `zone+16`, 20 bytes each: `guid[16]`, string id at +16.
**EXPECTED**: `0 s_tag_d3d_vertex_buffer`, `1 s_tag_d3d_index_buffer`, `2 s_tag_d3d_texture`,
`3 s_tag_d3d_texture_interleaved`, `4 s_constant_buffer`.

String ids were decoded with the U13 layout (index 19 bits, namespace 8 bits, length 5 bits, header
namespace table; `Common\StringIdTranslator.cs` header-based constructor).

---

## 4. `play` tag layout (`cache_file_resource_layout_table`), U13

Source: `Halo3\CacheFileResourceLayoutTableTag.cs`. The `Segments` offset is versioned:
`[Offset(60, MinVersion = MccHalo3, MaxVersion = Halo3ODST)]` applies to U13 (48 for retail 360 and 360 ODST,
60 again for MCC ODST).

| offset | type | field | source |
|---|---|---|---|
| 0x00 / 0 | TagBlock (12) | compression codecs, 16 bytes each (a GUID) | bytes; Reclaimer does not read it |
| 0x0C / 12 | TagBlock (12) | shared cache files, 264 bytes each | Reclaimer `SharedCaches` @12 |
| 0x18 / 24 | TagBlock (12) | pages, 88 bytes each | Reclaimer `Pages` @24 |
| 0x24 / 36 | TagBlock (12) | size groups, 16 bytes each (sounds only; count 0 in riverworld) | Reclaimer `SizeGroups` @36 |
| 0x30 / 48 | TagBlock (12) | unknown, zero in riverworld | bytes |
| 0x3C / 60 | TagBlock (12) | segments, 16 bytes each | Reclaimer `Segments` @60 (U13 branch) |
| 0x48 / 72 | 12 bytes | `01 00 00 00 / 00 00 00 00 / 56 56 7A E7` in riverworld; **unsure** | bytes |

**EXPECTED (riverworld)**: codecs 1 @ file `0x05753000` (GUID `b42ada7e-0549-cf80-bda8-43936cb3c7a0`),
shared caches 3 @ `0x05753010`, pages 1581 @ `0x05753328`, size groups 0, segments 7175 @ `0x057752A0`.

### 4.1 Shared cache block (264 bytes, `SharedCacheBlock`, `FixedSize(264)`)

| offset | type | field |
|---|---|---|
| 0 | char[256] NUL-terminated (Reclaimer reads 32) | file name, e.g. `maps\shared.map` |
| 256 | 8 bytes | flags/unknown: `00 00 0B 00 00 00 00 00` for english/mainmenu, `00 00 01 00 00 00 00 00` for shared |

**EXPECTED (riverworld)**: `[0] maps\english.map`, `[1] maps\mainmenu.map`, `[2] maps\shared.map`.
Only index 2 is referenced by any riverworld page (1420 pages in shared.map, 161 local). `english.map`
and `mainmenu.map` do not exist in the MCC `halo3\maps` folder; never try to open an unreferenced one.
The list is per map: all 11 campaign maps (`005_intro` .. `130_epilogue`) have a fourth entry
`[3] maps\campaign.map` (flags `0x0001`), so Rust must resolve `SharedCaches[page.CacheIndex]` from the
map being read and never assume three entries (corrected: this paragraph was riverworld-specific;
verified on every installed map with `.scratch/pages.py`, see collision-bsp-research.md 5).
Reclaimer opens the shared file as `<directory of the current map>\<last path component>`
(`ResourceIdentifier.ReadData`: `Utils.GetFileName` splits on `\` and takes the last part).

### 4.2 Page block (88 bytes, `PageBlock`, `FixedSize(88)`)

| offset | type | field | source |
|---|---|---|---|
| 0x00 / 0 | u16 | salt (`0` on every riverworld page) | bytes |
| 0x02 / 2 | u8 | flags: `1` x1337, `3` x243, `0x11` x1 (the big local geometry page); **unsure** | bytes |
| 0x03 / 3 | i8 | compression codec index into `play+0` (`0` on all 1581 pages = the one deflate codec; `-1` would mean stored) | bytes |
| 0x04 / 4 | i16 | shared cache index: `-1` = this map file, else index into shared caches | Reclaimer `CacheIndex` @4 |
| 0x06 / 6 | i16 | unknown; equals the page index on the shared pages inspected | bytes |
| 0x08 / 8 | i32 | data offset, relative to the file's raw-data base (section 5) | Reclaimer `DataOffset` @8 |
| 0x0C / 12 | i32 | compressed size in the file | Reclaimer `CompressedSize` @12 |
| 0x10 / 16 | i32 | uncompressed size | Reclaimer `DecompressedSize` @16 |
| 0x14 / 20 | u32 | checksum. Not CRC-32 of the compressed or of the inflated bytes (both tested); algorithm **unknown** | bytes |
| 0x18 / 24 | 20 bytes | hash of the whole page (SHA-1 sized; not verified) | bytes |
| 0x2C / 44 | 20 bytes | hash of the first chunk (unverified) | bytes |
| 0x40 / 64 | 20 bytes | hash of the last chunk (unverified) | bytes |
| 0x54 / 84 | i16 | asset / chunk count (number of segments in this page) | Reclaimer `DataChunkCount` @84 |
| 0x56 / 86 | i16 | `-1` on all pages inspected | bytes |

### 4.3 Segment block (16 bytes, `SegmentBlock`, `FixedSize(16)`)

| offset | type | field |
|---|---|---|
| 0 | i16 | primary page index (`-1` = none) |
| 2 | i16 | secondary page index (`-1` = none) |
| 4 | i32 | primary offset: byte offset of this resource's data inside the **inflated** primary page |
| 8 | i32 | secondary offset: same for the secondary page (`-1` = none) |
| 12 | i16 | primary size-group index (sounds only, `-1` otherwise) |
| 14 | i16 | secondary size-group index (sounds only) |

Size group (16 bytes, `SizeGroupBlock`): i32 total size @0, TagBlock of size blocks @4.
Size block (8 bytes, `SizeBlock`): i32 offset @0, i32 data size @4. Only `ReadSoundData` uses them;
MCC Halo 3 sounds are not in the map (location type 0), so these can be ignored for now.

---

## 5. Page -> file + byte offset

Source: `Halo3\ResourceIdentifier.cs` `ReadData` / local function `GetDataTableAddress`.
For `BlamEngine.Halo3` with `CacheType >= MccHalo3U6` (U13 included):

| page.cache index | file to open | raw-data base | file offset of the page |
|---|---|---|---|
| `-1` | the current map | the `u32` at header offset **0x4D0 (1232)** | `base + page.data offset` |
| `>= 0` | `<same directory>\basename(SharedCaches[idx].FileName)` | Reclaimer hard-codes **16384 (0x4000)** = the header size | `0x4000 + page.data offset` |

Header offset `0x4D0` is `sectionOffsetTable[1]` (the table starts at `0x4CC`; entry 1 = the
resources/raw section). So the general rule is **file offset = that file's own `sectionOffset[1]` +
`page.data offset`**, and the hard-coded 16384 for shared files is only a shortcut that happens to be
right because shared files put the raw section directly after the header. Prefer reading
`u32 @ 0x4D0` from whichever file is being opened; it is identical for all three files checked:

| file | size | `u32 @ 0x4D0` | `sectionOffset[]` | `sections[1]` (addr, size) | tag count |
|---|---|---|---|---|---|
| riverworld.map | `0x0952C000` | `0x00114000` | `[0xF9EDD000, 0x00114000, 0xFF51C000, 0x03405000]` | `(0x0, 0x0563F000)` | 11658 |
| shared.map | `0x37C81000` | `0x00004000` | `[0x0, 0x4000, 0x0, 0x0]` | `(0x0, 0x37C7D000)` | 0 |
| campaign.map | `0x04437000` | `0x00004000` | `[0x0, 0x4000, 0x0, 0x0]` | `(0x0, 0x04433000)` | 0 |

shared.map and campaign.map are pure resource containers: same `head`/version 13/16 KiB header and
build string, but tag count 0, virtual base 0, no partitions; their only non-empty section is
section 1. `0x4B0 (1200)` (the base used by the `MccHalo3F6..U6` branch) is garbage in U13 files.

Older branches in the same function, for reference only: early MCC (`< F6`) used header `0x4B8 (1208)`
and 12288 for shared; Xbox 360 used `0x470 (1136)`.

---

## 6. Compression

Source: `Common\Enums.cs` `CacheResourceCodec` + `Common\Annotations.cs` `CacheMetadataAttribute`
(default codec is `Deflate` for every engine `>= Halo3`; the MCC Halo 3 `[CacheMetadata(Halo3, PC, Mcc)]`
entries and the U13 `[BuildString]`s carry no override), `Common\ContentFactory.cs` `GetResourceData`.

- Codec for U13 = **Deflate**: one raw DEFLATE stream (RFC 1951, **no zlib header**, no footer) per page,
  covering the whole page. .NET `DeflateStream` is raw deflate; in Rust use `flate2::read::DeflateDecoder`
  (not `ZlibDecoder`), in Python `zlib.decompressobj(-15)`.
- If `compressed size == uncompressed size` the page is stored and copied as-is (Reclaimer checks this
  before the codec). Not seen in riverworld: all 1581 pages have `compressed != uncompressed`.
- Reclaimer inflates from the page start, discards `segment offset` bytes, then returns
  `min(maxLength, uncompressed size - segment offset)` bytes. So a "resource's data" is everything from
  the segment offset to the end of the page unless the caller limits it; the fixup offsets index into
  that segment-relative slice.
- `Utilities\XCompress.cs` (LZX via `xcompress32/64.dll`) is the Xbox 360 codec; `Reclaimer.Utilities\Oodle.cs`
  (`oo2core_8_win64.dll`) is for later MCC engines; `Utilities\AesHelper.cs` is string-table decryption for
  other games. **None of the three is used for MCC Halo 3 resources.** `UnknownDeflate` (chunked deflate
  with 7-bit lengths) is marked experimental and is not selected for any Halo 3 build.

**EXPECTED (riverworld)**: page 1420 (local, data offset 0): file `0x114000`, compressed `0x2B50006`,
uncompressed `0x57C3910` (about 92 MB); first two compressed bytes `44 9A` (not a `78 xx` zlib header);
raw-inflating the full `0x2B50006` bytes yields exactly `0x57C3910` bytes with the stream ending at
the last byte. Same for shared.map page 3 (`0x4000 + 0x104F000 = 0x1053000`, `0x38C17` -> `0x4E018`).
The single codec GUID `b42ada7e-0549-cf80-bda8-43936cb3c7a0` is therefore "deflate".

Note the cost: the local geometry page for Valhalla is one 92 MB stream, and every resource in it
(Lbsp geometry at +25.7 MB, sbsp geometry at +45.5 MB, collision at +87.5 MB) needs the stream
inflated up to its offset. Inflate the page once and cache it.

---

## 7. Definition data and the render-geometry definition

Source: `Halo3\Halo3Common.cs` `GetMeshes` (`VertexBufferInfo` `FixedSize(28)`: `VertexCount` @0,
`DataLength` @8; `IndexBufferInfo` `FixedSize(24)`: `IndexFormat` @0, `DataLength` @4; the comments
"12 byte struct here for each vertex buffer" and "4x 12 byte structs here"), the rest from bytes.

The entry's slice of the fixup blob (`fixup data pointer + entry.fixup offset`, `entry.fixup length`
bytes) is the resource **definition**. For type 1 (`render_geometry_api_resource_definition`) its layout,
with `V` vertex buffers and `I` index buffers, is:

| start (relative to the slice) | size | content |
|---|---|---|
| `0` | `28 * V` | vertex buffer definitions, 28 bytes each |
| `28V` | `12 * V` | `s_tag_d3d_vertex_buffer` runtime structs, 12 bytes each |
| `40V` | `24 * I` | index buffer definitions, 24 bytes each |
| `40V + 24I` | `12 * I` | `s_tag_d3d_index_buffer` runtime structs, 12 bytes each |
| `40V + 36I` = `length - 48` | 48 | root struct (4 x 12-byte blocks) = what `entry+36` (`0x2` + offset) points to |

Root struct (48 bytes): block @0 (count 0), block @12 (count 0), **vertex buffers block @24** (count V,
pointer fixed up to `28V` = the 12-byte vertex structs), **index buffers block @36** (count I, pointer
fixed up to `40V + 24I` = the 12-byte index structs). The third dword of each 12-byte block is a runtime value
(`0x5058923A`-style) and means nothing on disk. Reclaimer reads V at `length - 24` and I at `length - 12`.

Vertex buffer definition (28 bytes): **EXPECTED** first one in the Lbsp resource:
`BE 02 00 00 | 02 00 | 24 00 | B8 62 00 00 | 0 | 0 | 0 | 0`

| offset | type | meaning |
|---|---|---|
| 0 | i32 | vertex count (702) |
| 4 | i16 | vertex format (2); not the same enum as the section's vertex type byte (which is 1 here); see the geometry doc |
| 6 | i16 | stride in bytes (36): `702 * 36 = 25272` = data length |
| 8 | i32 | data length (`0x62B8` = 25272) |
| 12..19 | | zero |
| 20 | u32 | **data address slot**: resource fixup `[i]` writes `0x4` + offset here |
| 24 | | zero |

Index buffer definition (24 bytes): **EXPECTED** first one: `03 00 00 00 | E4 09 00 00 | 0 | 0 | 0 | 0`

| offset | type | meaning |
|---|---|---|
| 0 | i32 | index format (3 = triangle list in Reclaimer's `IndexFormat` enum, `Reclaimer.Core\Geometry\Enums.cs`; 5 would be triangle strip) |
| 4 | i32 | data length (`0x9E4` = 2532 bytes = 1266 u16 indices) |
| 8..15 | | zero |
| 16 | u32 | **data address slot** (resource fixup writes here) |
| 20 | | zero |

`s_tag_d3d_vertex_buffer` / `s_tag_d3d_index_buffer` (12 bytes): dword 0 is a pointer slot that a
nibble-`0x2` fixup fills with the address of the matching 28/24-byte definition, dword 1 is zero, dword 2 is runtime.

Order of the resource fixups for type 1 (verified: `1008 = 2*394 + 2*109 + 2`):

| fixup index range | writes at | value |
|---|---|---|
| `[0, V)` | vertex def `i` + 20 | `0x4` + offset of vertex data in the segment |
| `[V, 2V)` | d3d vertex struct `i` + 0 | `0x2` + `28 * i` (points at the def) |
| `[2V, 2V+I)` | index def `j` + 16 | `0x4` + offset of index data in the segment |
| `[2V+I, 2V+2I)` | d3d index struct `j` + 0 | `0x2` + `40V + 24j` |
| `2V+2I` | root + 28 | `0x2` + `28V` (vertex buffers block pointer) |
| `2V+2I+1` | root + 40 | `0x2` + `40V + 24I` (index buffers block pointer) |

This is why Reclaimer can take the vertex address straight from `ResourceFixups[section.VertexBufferIndex]`
and the index address from `ResourceFixups[2*V + section.IndexBufferIndex]` (`Halo3Common.cs` lines 128 and 144).
A section (mesh) picks its buffers through `SectionBlock` (`Halo3\RenderModelTag.cs`, 76 bytes for
`Halo3Delta..`: `VertexBufferIndex` i16 @24, `IndexBufferIndex` i16 @40, flags u8 @44, node u8 @45,
vertex format u8 @46, index format u8 @48; `IndexBufferIndex == -1` or the unindexed flag means
"implied 0..n-1 indices").

Definition fixups for type 1: `V + I` records, one per 12-byte D3D struct, struct type 0 for vertex
and 1 for index (**EXPECTED**: 503 = 394 + 109, addresses `0x2 + 0x2B18 + 12k` then `0x2 + 0x47C8 + 12k`).

---

## 8. The full algorithm: resource id -> buffers

```
read_resource(cache, id: u32, mode) -> bytes:
    index = id & 0xFFFF ; salt = id >> 16
    zone  = global tag "zone" ; play = global tag "play"      # play meta ptr 0 => play = zone + 28
    entry = zone.resource_entries[index]                      # 64 bytes each
    if entry.salt != salt or entry.type == 0xFF or entry.segment < 0: no data
    seg   = play.segments[entry.segment]                      # 16 bytes each
    use_secondary = (mode == Secondary) or (mode == Auto and seg.secondary_page >= 0)
    page_i, seg_off = secondary ? (seg.secondary_page, seg.secondary_off) : (seg.primary_page, seg.primary_off)
    if page_i < 0 or seg_off < 0: no data
    page = play.pages[page_i]                                 # 88 bytes each
    if mode == Auto and (page.data_off < 0 or page.compressed == 0): fall back to the primary page
    file = page.cache_idx < 0 ? this map : dir(this map) / basename(play.shared_caches[page.cache_idx].name)
    base = u32 at offset 0x4D0 of *that* file                 # == its sectionOffset[1]; 0x114000 / 0x4000
    blob = read(file, base + page.data_off, page.compressed)
    data = (page.compressed == page.uncompressed) ? blob : raw_deflate(blob)   # len == page.uncompressed
    return data[seg_off : seg_off + min(max_len, page.uncompressed - seg_off)]

geometry(cache, id, sections):
    entry = zone.resource_entries[id & 0xFFFF]
    def   = fixup_blob[entry.fixup_off : entry.fixup_off + entry.fixup_len]
    V = i32 at def[len-24] ; I = i32 at def[len-12]
    vdefs = 28-byte records at def[0..28V) ; idefs = 24-byte records at def[40V..40V+24I)
    fix   = entry.resource_fixups                              # (write_at, value) pairs
    data  = read_resource(cache, id, Auto)
    for each section s:
        va = fix[s.vb_index].value & 0x1FFFFFFF ; vertices = data[va : va + vdefs[s.vb_index].data_len]
        if indexed: ia = fix[2V + s.ib_index].value & 0x1FFFFFFF ; indices = data[ia : ia + idefs[s.ib_index].data_len]
        index type = u16 unless vertex count > 65535 (Reclaimer rule), layout from idefs[...].format
```

To apply fixups literally (what the game does): for each resource fixup write `value` as a u32 at
`def[write_at]`; a pointer with nibble `0x2` then means `def + offset`, `0x4` means
`primary segment data + offset`, `0x6` means `secondary segment data + offset`.

---

## 9. The `sbsp` and `Lbsp` resource ids (where the geometry actually is)

Source: `Halo3\Config\ScenarioStructureBspTag.config.cs` (`AddVersion(MccHalo3U4, null)` branch:
`ResourcePointer1` @596, `Sections` @756, `ResourcePointer2` @876, `ResourcePointer3` @908),
`Halo3\ScenarioLightmapBspDataTag.cs` (`BspIndex` i16 @2, `Sections` @308, `ResourcePointer` @428; the
`FixedSize(436)` only applies below `MccHalo3U4` when it was an embedded block), `Halo3\ScenarioLightmapTag.cs`
(`LightmapRefs` TagReference block @4 for `>= MccHalo3U4`), and `ScenarioStructureBspTag.GetModelContent`.

Important: for MCC `>= U4` Reclaimer does **not** draw the sbsp's own geometry resource. It finds the
`Lbsp` (`scenario_lightmap_bsp_data`) tag for the BSP (via the scenario's lightmap tag's `LightmapRefs`,
matching `BspIndex`, or by an `Lbsp` with the same tag name as the sbsp) and uses **its** `Sections` (@308)
and **its** resource id (@428). The sbsp's resource at +596 is a smaller render-geometry resource
(11 vertex buffers; probably instanced/pre-lightmap geometry) and the sbsp's `Sections` block is still at 756.

Scan of the riverworld sbsp meta for the three live gestalt entries owned by it (**EXPECTED**):

| sbsp offset | id | entry | type | fixup data length | segment | note |
|---|---|---|---|---|---|---|
| 0x254 / 596 | `0xFDE91C86` | 7302 | 1 render_geometry_api_resource_definition | `0x1E8` (11 vb, 11 ib) | 7051: page 1420, offset `45503816` (`0x2B65CC8`) | Reclaimer `ResourcePointer1` |
| 0x36C / 876 | `0xFDEA1C87` | 7303 | wiped (`0xFF`) | | | Reclaimer `ResourcePointer2`: **dead in U13** |
| 0x38C / 908 | `0xFE651D02` | 7426 | 5 structure_bsp_tag_resources | `0x4C00`, 831 fixups (760 x `0x4`, 71 x `0x2`), 0 definition fixups | 7173: page 1420, offset `87556096` (`0x5380000`) | Reclaimer `ResourcePointer3`; this is the **collision BSP** resource from HANDOFF's known gap |
| 0x394 / 916 | `0xFE661D03` | 7427 | 6 structure_bsp_cache_file_tag_resources | `0x30` | 7174 | not declared in Reclaimer |

Lbsp (**EXPECTED**): tag index `0x1ADF`, same name `levels\multi\riverworld\riverworld`, meta at file
`0x08254550`, `BspIndex` 0, 102 sections @ file `0x0824ADC8`, resource id `0xEE3D0CDA` at `Lbsp+428`.

---

## 10. EXPECTED VALUES for the Lbsp render-geometry resource `0xEE3D0CDA` (riverworld)

Entry 3290 raw (64 bytes at file `0x05843EAC + 64*3290 = 0x0587752C`):
```
7073624c 00000000 00000000 df1a55fc | 3dee 01 02 | f88c0400 | 144d0000 | ffffffff | 0100 b60c | e44c0020 | f0030000 1f7d1173 00000000 | f7010000 ff841173 00000000
```
| field | value |
|---|---|
| owner | class `Lbsp`, tag id `0xFC551ADF` (index `0x1ADF`) |
| salt / type / flags | `0xEE3D` / 1 / 2 |
| fixup offset / length | `0x48CF8` / `0x4D14` (19732) -> definition data at file `0x059D1338` |
| secondary fixup offset | `-1` |
| location type / segment | 1 / `0xCB6` = 3254 |
| definition address | `0x20004CE4` (= length - 48) |
| resource fixups | 1008 @ file `0x0583237C`; definition fixups 503 @ file `0x0583427C` |
| segment 3254 | primary page 1420, secondary -1, primary offset `25733816` (`0x188AAB8`), secondary -1, size groups -1/-1 |
| page 1420 | salt 0, flags `0x11`, codec 0, cache -1 (local), data offset 0, compressed `0x2B50006`, uncompressed `0x57C3910`, crc `0x01BECDF6`, asset count `0x74E` = 1870 |
| file location | `riverworld.map` @ `0x114000`, raw deflate |
| V / I | 394 vertex buffers, 109 index buffers; `28V = 0x2B18`, `40V = 0x3D90`, `40V+24I = 0x47C8`, root `0x4CE4` |
| first 64 bytes of the inflated page | `b7f4367c 6a6f0000 ced5d482 598bc403 71cb0000 a0341c02 578b0000 6efd1380 86fc0000 87f2ff7f 7a780000 69d21c80 838dffff c1c60000 3e39ffff 838d0000` |
| first 64 bytes at the segment start (= vertex buffer 0, 702 verts x 36 bytes) | `51342637 00000000 74116ff1 ffff0000 00800000 0080ffff 00000000 00000080 ffff0000 57347562 00000000 741101e6 00000000 00800000 00800000 00000000` |
| vertex buffer 1 (fixup offset `0x62B8`) | `4d2349ca 10b40000 838bfe87 00800400 08000000 f7ff0000 00800000 0400ff7f ...` |
| index buffer 0 (fixup `[788]`, offset `0xB578F0`, 2532 bytes, format 3) | `00000100 02000300 04000500 06000700 08000800 09000600 0a000b00 0c000c00` = u16 `0,1,2,3,4,5,6,7,8,8,9,6,10,11,12,12` |
| sections 0..3 | vb 0..3 / ib 0..3, flags `0x8`, node 255, vertex format 1, index format 3 |
| sum of all buffer lengths | `0xBB948C`; the last vertex-data fixup is `0x40BB8A1C` |

Shared-file example, entry 4788 (`jmad objects\characters\masterchief\masterchief`, type 4,
id `0xF41712B4`): segment `0x128B` -> page 3 (shared index 2, data offset `0x104F000`,
compressed `0x38C17`, uncompressed `0x4E018`, 8 assets), primary offset `189936` (`0x2E5F0`) ->
`shared.map` @ `0x1053000`; inflates to exactly `0x4E018` bytes; first 64 bytes at the segment start
`011e0200 00000000 00000000 10010000 28010000 00000000 00000000 00000000 00000000 63f74c80 00000000 815a815a 3aef70f9 fcdf6485 a00df3f7 23d32d89`.

---

## 11. Checks a Rust reader should reproduce

1. Zone offsets 100/328/340 give `7428` entries, fixup size `0xA2B10`, pointer `0x7316D590`; offsets
   88/316/328 and 36/132/144 give zeros (wrong version branch).
2. `(salt << 16) | index` of entry 3290 equals the id stored at `Lbsp+428`.
3. Every page: `u32 @ 0x4D0` of the target file + `data offset`; raw-deflate the `compressed` bytes;
   the output length equals `uncompressed` and the stream ends exactly at the last input byte.
4. Index buffer 0 of the Lbsp resource reads `0 1 2 3 4 5 6 7 8 8 9 6 ...` as u16 at `segment + 0xB578F0`.
5. `1008 == 2*394 + 2*109 + 2` and `503 == 394 + 109`.

---

## 12. Unsure spots / open questions

- **Page checksum** at page+20: neither CRC-32 of the compressed nor of the inflated bytes. Likely a
  different polynomial or a hash of chunks; irrelevant for reading, flagged so nobody wastes time.
- **Page flags** (`1`, `3`, `0x11`), page+6, page+86, the three 20-byte hashes, entry flags (`2` vs `4`),
  entry+28 (secondary fixup offset, always -1), entry+32 exact bit meaning, zone+0 (= 1), play+48 block,
  play+72 bytes: all read from bytes, meanings guessed or unknown.
- **Segment-relative vs page-relative**: fixup offsets are segment-relative (verified on the index
  buffer). Reclaimer's `maxLength` default is `int.MaxValue`, so it inflates the whole 92 MB page per
  resource read; a Rust reader should cache inflated pages or stream to `segment + last needed offset`.
- **Type 2 `constant_buffer_resource_definition`** on shader-like tags (1469 entries) is an MCC
  addition with no Reclaimer definition. Not needed for geometry.
- **Sounds** (type 3) have location type 0 and page index -1: MCC Halo 3 audio is not in the map files.
  Reclaimer's `ReadSoundData` reads header `0x470` (the 360 base) and will not work on MCC; ignore.
- **Reclaimer's sbsp `ResourcePointer2` offset 876** points at a wiped entry in U13; the live
  collision resource (type 5) is at 908 and the cache-file resource (type 6) at 916. Whether 876 is a
  leftover slot or a resource the game nulls for MP maps is unknown; check a campaign sbsp.
- **Collision BSP** (`structure_bsp_tag_resources`, entry 7426): 19456 bytes of definition data with
  831 fixups, 760 of which point into the page (segment offset `0x5380000` of page 1420). Decoding its
  definition is the open work from HANDOFF; this doc gives its exact location.
- The `play -> zone+28` fallback was not exercised (riverworld has a real `play` tag). Which MCC maps
  need it is unknown; keep the fallback.
- Vertex **formats** (the 28-byte def's +4 value vs the section's byte @46, stride 36 for format 2) and
  the per-format vertex layouts belong to the geometry document; Reclaimer's layouts are in
  `Reclaimer.Blam\Properties\Resources.resx` entry `MccHalo3VertexBuffer` (XML, selected with
  `XmlVertexBuilder` in `Halo3Common.GetMeshes`), not reviewed here.
