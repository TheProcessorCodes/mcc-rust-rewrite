# MCC Halo 3 (U13) bitmap tag (`bitm`) and its pixel data

Spec for a Rust decoder that turns a `bitm` tag into `(width, height, format, bytes)` per mip
level, handing raw BC1/BC2/BC3/BC5 blocks and raw RGBA/BGRA pixels to wgpu without decoding.

- Target: MCC Halo 3 build `Dec 21 2023 22:31:37` = Reclaimer `CacheType.MccHalo3U13`
  (ordinal 22, `MccHalo3 16 < ... < MccHalo3U13 22 < Halo3ODST 23`; Reclaimer version ranges are
  `Min` inclusive, `Max` exclusive, see `resources.md` section 0). Every field below is resolved to
  its U13 value only.
- Facts come from Gravemind2401's Reclaimer (GPL-3.0, <https://github.com/Gravemind2401/Reclaimer>,
  read-only). Only format facts are recorded (offsets, sizes, enums, meanings, algorithms in our own
  words); no code was copied. Each fact names the Reclaimer file and member it was read from. Paths
  are relative to `Reclaimer.Blam\Blam\` unless they start with `Reclaimer.Core` or `Utilities`.
- Everything marked **bytes** was found by reading the map files, not in Reclaimer. Everything marked
  **EXPECTED** was printed by `python -I docs\format\scripts\probe_bitmaps.py "<maps dir>" riverworld.map`
  (read-only, ~40 s; the output holds bytes of the map, so it is not kept in the repo; run it to see it). Primary test
  file `riverworld.map` (Valhalla); 4 other maps spot-checked and all 36 installed maps scanned for
  the per-map histograms in section 9.
- Little-endian throughout. Offsets are hex / decimal. Container primitives (TagBlock 12 bytes
  `{i32 count, shifted ptr, pad}`, TagReference 16 bytes, StringId, DataReference, shifted 32-bit
  pointers) are in `container.md`; the resource system (`zone`, `play`, pages, segments, fixups,
  deflate) is in `resources.md` and is assumed below.

Contents: 1 the chain in one paragraph, 2 bitm tag layout, 3 bitmap data block, 4 the resource and
its definition, 5 formats, 6 where the bytes are (pages, mips, faces, slices), 7 algorithm,
8 EXPECTED values on riverworld, 9 per-map scan, 10 Reclaimer behaviour that does not apply to MCC,
11 checks for `cargo test`, 12 unsure spots.

---

## 1. The chain in one paragraph

A `bitm` tag holds a block of **bitmap data blocks** (one per image: width, height, depth, type,
format, mip count, colour curve) and a parallel block of **resources** (one `i32` resource id per
image, same index). The resource id leads through `zone` (entry: owner, salt, type, definition
data, fixups, segment) and `play` (segment: primary page + offset, optional secondary page + offset)
to one or two deflate pages. The entry's 68-byte **definition** repeats the image header and gives
the byte size of the data in each page plus the DXGI format the engine will create. If the image
has a **secondary page**, that page holds exactly **mip 0** (all faces) and the primary page holds
**mips 1..n**; otherwise the primary page holds the whole chain. Levels are packed back to back
with no padding, largest first; cube maps store all six faces of a level together (mip-major);
texture arrays store each slice's full chain in turn (slice-major). There is no tiling, swizzling,
endian swap or dimension padding on MCC PC. Interleaved resources exist in the tag layout but are
unused by every installed map.

---

## 2. `bitm` tag layout (U13)

Source: `Halo3\BitmapTag.cs` `BitmapTag` (unversioned `[Offset]`s, so they apply to U13). The
header is 164 (0xA4) bytes: on every riverworld tag the first child block begins at `meta + 0xA4`
(**bytes**). Reclaimer reads only the four blocks.

| offset | type | field | source |
|---|---|---|---|
| 0x00 / 0 .. 0x53 / 83 | 84 bytes | bitmap-group settings (type, usage flags, detail fade, sharpen amount, bump height, ...). Observed: i32 `0`/`2` at +0, i16 flags `0x10`/`0x50` at +4, i16 `4` at +6, floats at +8 and +12, i16 `0x31` at +16 on the bump map. **Not read; meanings unsure** | bytes |
| 0x54 / 84 | TagBlock | **Sequences**, 64-byte `SequenceBlock` (section 2.1) | Reclaimer `Sequences` `[Offset(84)]` |
| 0x60 / 96 | TagBlock | **Bitmaps**, 56-byte `BitmapDataBlock` (section 3) | Reclaimer `Bitmaps` `[Offset(96)]` |
| 0x6C / 108 .. 0x8B / 139 | 32 bytes | zero on every tag inspected | bytes |
| 0x8C / 140 | TagBlock | **Resources**, 8-byte `BitmapResourceBlock` (section 4) — one per bitmap, same index | Reclaimer `Resources` `[Offset(140)]` |
| 0x98 / 152 | TagBlock | **InterleavedResources**, 8-byte `BitmapResourceBlock` — **count 0 in all 36 installed maps** | Reclaimer `InterleavedResources` `[Offset(152)]` |

**EXPECTED (riverworld)**: 2204 `bitm` tags, 2464 bitmap data blocks; `Resources.count ==
Bitmaps.count` on every tag (2189 tags with 1, up to 80 per tag; `riverworld_riverworld_cubemaps`
has 55); `InterleavedResources.count == 0` everywhere; `Sequences.count` 0 on 2045 tags, up to 63.

### 2.1 SequenceBlock, 64 (0x40) bytes, and SpriteBlock, 32 (0x20) bytes

Source: `Halo3\BitmapTag.cs` `SequenceBlock` `[FixedSize(64)]`, `SpriteBlock` `[FixedSize(32)]`.
Only needed for UI/particle sprite sheets, not for world textures.

| offset | type | field |
|---|---|---|
| 0x00 / 0 | char[32] | name (NUL-terminated) |
| 0x20 / 32 | i16 | first submap (bitmap) index |
| 0x22 / 34 | i16 | bitmap count |
| 0x24 / 36 .. 0x33 / 51 | 16 bytes | zero on the tags inspected (bytes) |
| 0x34 / 52 | TagBlock | sprites, 32 bytes each |

Sprite: `0x0` i16 submap index, `0x2..0x7` 6 bytes (zero), `0x8` f32 left, `0xC` f32 right,
`0x10` f32 top, `0x14` f32 bottom (all UV fractions 0..1), `0x18` f32x2 registration point.
**EXPECTED**: `fx\decals\impact_bullet\_bitmaps\impact_default` has 1 sequence `import #0`, first 0,
count 0, 4 sprites; sprite 0 = `left 0.5 right 1.0 top 0.5 bottom 1.0 reg (0.25, 0.25)`.

---

## 3. BitmapDataBlock, 56 (0x38) bytes on U13

Source: `Halo3\BitmapTag.cs` `BitmapDataBlock`: `[FixedSize(48, MaxVersion = MccHalo3)]`,
`[FixedSize(56, MinVersion = MccHalo3, MaxVersion = Halo3ODST)]` -> **56 bytes for U13** (48 on Xbox 360).
Field offsets are unversioned.

| offset | type | field | values seen (riverworld) | source |
|---|---|---|---|---|
| 0x00 / 0 | char[4] | class, always `bitm` (stored reversed: `6d 74 69 62`) | | Reclaimer `Class` |
| 0x04 / 4 | i16 | **width** in pixels | 1 .. 2048 | Reclaimer `Width` |
| 0x06 / 6 | i16 | **height** | | Reclaimer `Height` |
| 0x08 / 8 | u8 | **depth**: 1 for 2D and cube; slice count for arrays (2, 3, 8, 16 seen); depth for 3D | | Reclaimer `Depth` |
| 0x09 / 9 | u8 | **flags**. Reclaimer names only bit 3 (`8`) `Swizzled`, never set on MCC. **bit 5 (`0x20`) is set exactly when the resource has a secondary page** (1315/1315, 0/1124 otherwise; bytes). bits 2 and 4 (`0x04`, `0x14`) appear on arrays and one 2D; meaning **unsure** | `0` x1139, `0x20` x1319, `4` x2, `0x14` x4 | Reclaimer `Flags`; bytes |
| 0x0A / 10 | i16 | **type**: 0 = 2D, 1 = 3D, 2 = cube map, 3 = array | 2D x2388, cube x71, array x5; no 3D in any map | Reclaimer `BitmapType`, enum `TextureType` |
| 0x0C / 12 | i16 | **format** (section 5) | | Reclaimer `BitmapFormat`, enum `TextureFormat` |
| 0x0E / 14 | i16 | "more flags"; copied into the definition at +54. Values 0..6; **unsure** | 1 x705, 2 x206, 3 x1479 | Reclaimer `MoreFlags` |
| 0x10 / 16 | i16 | registration point x | | Reclaimer `RegX` |
| 0x12 / 18 | i16 | registration point y | | Reclaimer `RegY` |
| 0x14 / 20 | u8 | **mipmap count, excluding the base level** (0 = only mip 0). Levels = count + 1 | 0 .. 11 | Reclaimer `MipmapCount` |
| 0x15 / 21 | u8 | **curve** (colour space): 0 unknown, 1 xRGB, 2 gamma 2.0, 3 linear, 4 offset log, 5 sRGB | xRGB x1119, linear x1115, sRGB x194, unknown x35, gamma2 x1 | Reclaimer `Curve`, enum `ColorSpace` |
| 0x16 / 22 | u8 | interleaved resource index (only meaningful when `InterleavedResources` is non-empty; 0 or 255 here) | | Reclaimer `InterleavedIndex` |
| 0x17 / 23 | u8 | "index 2" (second texture of an interleaved pair?); always 0 | | Reclaimer `Index2` |
| 0x18 / 24 | i32 | legacy pixel data offset of this bitmap inside the tag's concatenated pixel buffer: 0 for bitmap 0, increasing for later bitmaps. Not used on MCC (every bitmap has its own resource). Reclaimer declares it as a byte | | Reclaimer `PixelsOffset`; bytes |
| 0x1C / 28 | i32 | legacy pixel data size: equals the full mip-chain size (1284 bitmaps) or 0 (1155). Not used | | Reclaimer `PixelsSize`; bytes |
| 0x20 / 32 .. 0x37 / 55 | 24 bytes | `00000000 0000XX00 ffffffff 0...`: a `-1` at +40 and a small value at +38 (`0x10`, `0x04`, 0); 9 distinct patterns; **unsure**, not needed | | bytes |

Gamma per curve (Reclaimer `IBitmap.GetSubmapGamma`): linear 1.0, gamma2 2.0, sRGB 2.2, everything
else (xRGB, unknown) 1.95. For wgpu use the DXGI format from the definition (section 4.1), which
already encodes the sRGB choice: **UNORM for curve linear(3) and gamma2(2), `_SRGB` for xRGB(1),
sRGB(5) and unknown(0)**, for the formats that have an sRGB variant (BC1/BC2/BC3/B8G8R8A8).

---

## 4. The resource: `BitmapResourceBlock` (8 bytes) and the definition data

Source: `Halo3\BitmapTag.cs` `BitmapResourceBlock` `[FixedSize(8)]`: `ResourcePointer`
(`ResourceIdentifier`, i32) at `0x0`, `Unknown` i32 at `0x4` (always 0 on riverworld, bytes).
`Resources[i]` belongs to `Bitmaps[i]` (Reclaimer `ToDds(index)` uses `Resources[index]` when there
are no interleaved resources, else `InterleavedResources[submap.InterleavedIndex]`).

The resource id resolves as in `resources.md` section 2 (low 16 bits = `zone` entry index, high 16
bits = salt that must match `entry + 0x10`). **EXPECTED (riverworld)**: 2447 of 2464 ids resolve to live
entries; 17 point at wiped entries (salt `0xFFFF`, type `0xFF`) and must be treated as "no texture".

Live bitmap entries (**bytes**, all 2447): owner class `bitm`; **resource type = the type table
entry named `bitmap_texture_interop_resource`** (index 0 in riverworld, **index 1 in cyberdyne and
most other maps** — the table order differs per map, so resolve by string-id name, never by index);
entry flags `2`; `fixup length` **68** on every one; definition address `0x20000038` (= nibble `0x2` +
offset 56); location type `1` (primary only, 1132) or `3` (primary + secondary, 1315); 2 or 3 resource
fixups; exactly 1 definition fixup `(0x20000038, structure type "s_tag_d3d_texture")` (index 2 in
riverworld's structure-type table; again resolve by name).

### 4.1 Definition data: 56-byte texture description + 12-byte D3D struct (**bytes**; Reclaimer ignores it)

The entry's slice of the `zone` fixup blob (`fixup data pointer + entry.fixup offset`, 68 bytes). All
2439 known-format bitmaps on riverworld pass every check in `probe_bitmaps.py` pass 2.

| offset | type | field | check (riverworld) |
|---|---|---|---|
| 0x00 / 0 | i32 | **primary data size** in bytes: whole chain when there is no secondary page, mips 1..n when there is | 2439/2439 |
| 0x04 / 4 | 8 bytes | zero | 2439/2439 |
| 0x0C / 12 | u32 | **primary data address slot**: resource fixup 0 writes `0x40000000` here (nibble 4 = primary page, offset 0 inside the segment) | 2439/2439 |
| 0x10 / 16 | i32 | zero | |
| 0x14 / 20 | i32 | **secondary data size**: size of mip 0 (all faces) when there is a secondary page, else 0 | 2439/2439 |
| 0x18 / 24 | 8 bytes | zero | |
| 0x20 / 32 | u32 | **secondary data address slot**: resource fixup 1 writes `0x60000000` here (nibble 6 = secondary page, offset 0) when a secondary page exists | 1315/1315 |
| 0x24 / 36 | i32 | zero | |
| 0x28 / 40 | u16 | width (== block +4) | 2439/2439 |
| 0x2A / 42 | u16 | height (== block +6) | 2439/2439 |
| 0x2C / 44 | u8 | depth (== block +8) | 2439/2439 |
| 0x2D / 45 | u8 | **mip level count including the base** (== block +20 plus 1) | 2439/2439 |
| 0x2E / 46 | u8 | type (== block +10) | 2439/2439 |
| 0x2F / 47 | u8 | flags: **bit 0 = "high-resolution mip 0 lives in the secondary page"** (1 exactly when the segment has a secondary page) | 2439/2439 |
| 0x30 / 48 | i32 | **DXGI_FORMAT** the engine creates (section 5.1 table): 71/72 BC1, 74/75 BC2, 77/78 BC3, 84 BC5_SNORM, 87/91 B8G8R8A8, 65 A8, 61 R8, 51 R8G8_SNORM, 115 B4G4R4A4, 10 R16G16B16A16_FLOAT | see 5.1 |
| 0x34 / 52 | u8 | format (== block +12) | 2439/2439 |
| 0x35 / 53 | u8 | curve (== block +21) | 2439/2439 |
| 0x36 / 54 | u8 | == block +14 low byte ("more flags") | 2439/2439 |
| 0x37 / 55 | u8 | zero | 2439/2439 |
| 0x38 / 56 | 12 bytes | `s_tag_d3d_texture` runtime struct: dword 0 is a pointer slot (the last resource fixup writes `0x20000000` = "this definition + 0"), dword 1 zero, dword 2 a runtime value (`0xb2f55d50` on every entry) | 2439/2439 |

Resource fixup order: `[0] (write 12, 0x40000000)`, then `(write 32, 0x60000000)` only if a
secondary page exists, then `(write 56, 0x20000000)` last. Both data offsets are always 0: the data
starts exactly at the segment offset in its page.

A Rust reader can decode a bitmap from the **definition alone** (dims, levels, type, DXGI format,
primary/secondary sizes) and use the tag's bitmap block only to find the resource id; the two agree
on every field.

---

## 5. Formats

### 5.1 The format enum and what MCC PC stores

Enum values and names: `Halo3\BitmapTag.cs` `TextureFormat` (values 0..44). Bits per pixel, block
width and bytes per block: `Reclaimer.Core\Drawing\DdsImageDescriptor.cs` constructor (`BitsPerPixel`,
`BlockWidth`, `BytesPerBlock` switches). DXGI mapping: `Utilities\TextureUtils.cs` `dxgiLookup` /
`xboxLookup` + `Reclaimer.Core\Drawing\DrawingExtensions.cs` `ToDxgiFormat(XboxFormat)` + the
`DXNSwap` rule in `BitmapTag.ToDds` (on `CachePlatform.PC` DXN is read as `DXN_SNorm` = BC5_SNORM).
"DXGI in def" is the value found at definition +48 on the installed maps (**bytes**).

| value | Reclaimer name | bpp | block | bytes/block | DXGI (Reclaimer) | DXGI in def (bytes) | wgpu | seen in MCC H3 maps |
|---|---|---|---|---|---|---|---|---|
| 0 | A8 | 8 | 1 | 1 | A8_UNORM | **65** A8_UNORM | no A8 in wgpu: upload as `R8Unorm`, read `.r` as alpha in the shader | yes (117 in riverworld; UI, masks) |
| 1 | Y8 | 8 | 1 | 1 | R8_UNORM (Xbox `Y8`: R copied to G,B; A = 1) | **61** R8_UNORM | `R8Unorm` | yes (1) |
| 2 | AY8 | 8 | 1 | 1 | A8_UNORM (value copied to all four channels) | | `R8Unorm` + swizzle | no |
| 3 | A8Y8 | 16 | 1 | 2 | R8G8_UNORM (byte 0 = alpha, byte 1 = luminance; Reclaimer `Y8A8`) | | `Rg8Unorm` + swizzle | no |
| 4, 5, 7, 12, 13 | unused | | | | | | | no |
| 6 | R5G6B5 | 16 | 1 | 2 | B5G6R5_UNORM | | none (expand to RGBA8) | no |
| 8 | A1R5G5B5 | 16 | 1 | 2 | B5G5R5A1_UNORM | | none (expand) | no |
| 9 | A4R4G4B4 | 16 | 1 | 2 | B4G4R4A4_UNORM | **115** B4G4R4A4_UNORM | none (expand to RGBA8) | yes (160; UI emblems) |
| 10 | X8R8G8B8 | 32 | 1 | 4 | B8G8R8X8_UNORM | | `Bgra8Unorm`, force alpha 1 | 1 bitmap in `040_voi` |
| 11 | A8R8G8B8 | 32 | 1 | 4 | B8G8R8A8_UNORM | **87** / **91** (_SRGB) | `Bgra8Unorm` / `Bgra8UnormSrgb` | yes (394) |
| 14 | DXT1 | 4 | 4 | 8 | BC1_UNORM | **71** / **72** (_SRGB) | `Bc1RgbaUnorm` / `Bc1RgbaUnormSrgb` | yes (323) |
| 15 | DXT3 | 8 | 4 | 16 | BC2_UNORM | **74** / **75** | `Bc2RgbaUnorm` / `Bc2RgbaUnormSrgb` | yes (3) |
| 16 | DXT5 | 8 | 4 | 16 | BC3_UNORM | **77** / **78** | `Bc3RgbaUnorm` / `Bc3RgbaUnormSrgb` | yes (1012, the common colour format) |
| 17 | P8_bump | 8 | 1 | 1 | treated as Y8 | | | no |
| 18 | P8 | 8 | 1 | 1 | treated as Y8 | | | no |
| 19 | ARGBFP32 | 128 | 1 | 16 | R32G32B32A32_FLOAT | | `Rgba32Float` | no |
| 20 | RGBFP32 | 96 | 1 | 12 | R32G32B32_FLOAT | | none | no |
| 21 | RGBFP16 | 64 | 1 | 8 | (Reclaimer lists RGBAFP16 -> R16G16B16A16_FLOAT, but has no lookup from enum 21) | | `Rgba16Float` | no |
| 22 | U8V8 | 16 | 1 | 2 | R8G8_SNORM (Xbox `V8U8`, Z reconstructed) | **51** R8G8_SNORM | `Rg8Snorm` (2-channel normal/offset map) | yes (97; water/distortion) |
| 23..30 | unknown | | | | | | | 25 only |
| **25** | Unknown25 in Reclaimer | **64** | 1 | 8 | none in Reclaimer (it cannot export these) | **10** R16G16B16A16_FLOAT | `Rgba16Float` | **yes, 8 per map**: UI gradient strips `ui\halox\...` (406x1, 285x1, ...) and `rasterizer\cc0236`, `c78d78`, `dd0236` (32x32 colour-correction LUTs). Size check: `0xCB0 = 406 * 8` bytes |
| 31 | DXT5a | 4 | 4 | 8 | BC4_UNORM (alpha-only BC3 = BC4 layout) | | `Bc4RUnorm` | no |
| 32 | unknown | | | | | | | no |
| 33 | **DXN** | 8 | 4 | 16 | BC5_UNORM on Xbox; **BC5_SNORM on PC** (`DXNSwap`) | **84** BC5_SNORM | `Bc5RgSnorm` | yes (349; every `*_bump` normal map) |
| 34 | CTX1 | 4 | 4 | 8 | none (Xbox 2-channel BC1) | | | no |
| 35 | DXT3a_alpha | 4 | 4 | 8 | none (alpha-only BC2) | | | no |
| 36 | DXT3a_mono | 4 | 4 | 8 | none | | | no |
| 37 | DXT5a_alpha | 4 | 4 | 8 | BC4_UNORM, value -> alpha | | `Bc4RUnorm` | no |
| 38 | DXT5a_mono | 4 | 4 | 8 | BC4_UNORM, value -> RGB | | `Bc4RUnorm` | no |
| 39 | DXN_mono_alpha | 8 | 4 | 16 | BC5_UNORM, R -> RGB, G -> A | | `Bc5RgUnorm` + swizzle | no |
| 40..44 | unknown | | | | | | | no |

So for the installed MCC Halo 3 maps a decoder needs exactly **11 formats**: A8, Y8, A4R4G4B4,
X8R8G8B8, A8R8G8B8, DXT1, DXT3, DXT5, U8V8, 25 (RGBA FP16), DXN. The only one wgpu cannot take
directly is A4R4G4B4 (expand to RGBA8 on the CPU; 160 UI bitmaps). Everything else uploads as-is;
the simplest rule is **"take the DXGI value at definition +48 and map it to the wgpu format"**.

### 5.2 Which formats carry what

- **Normal maps are DXN = BC5_SNORM**: two signed channels X (red block) and Y (green block),
  Z reconstructed in the shader (`z = sqrt(1 - x*x - y*y)`); Reclaimer's `DecompressDXN` does the same
  (`CalculateZVector`). Verified signed: `cust_base_floor_bump` mip 0 is 84 % zero bytes and its first
  blocks have endpoints `0,0` — a flat normal is `(0,0)` only in the signed interpretation (unsigned
  would mean `(-1,-1)`). Tag naming: `*_bump`.
- **U8V8 = R8G8_SNORM**: 2-channel signed, same reconstruction; used for water/ripple/distortion maps.
- **DXT5 (BC3)** is the standard colour (+ alpha) format; **DXT1 (BC1)** colour without alpha.
  `cliff_rock_base_brown` is a 2048x2048 DXT5 with a usable alpha (specular mask: the rmsh option
  `specular_mask_from_diffuse`, see `Gen3MaterialHelper.cs`).
- **A8R8G8B8** bytes are stored `B, G, R, A` in memory (e.g. `gray_50_percent` = `40 40 40 ff`), which
  is DXGI `B8G8R8A8_UNORM`, wgpu `Bgra8Unorm`.
- sRGB vs linear comes from the **curve** byte and is already baked into the definition's DXGI value
  (table in section 4.1). Colour maps are mostly xRGB (-> sRGB), normal/mask/lightmaps linear.
- Lightmaps: `levels\multi\riverworld\riverworld_lightmap_riverworld_16f_lp_array_dxt5` is a
  **1024x1024x8 DXT5 array, 0 mips, linear** (8 MB in the local page), the baked lighting the sbsp
  render geometry references. Out of scope here but it is the same decoder.

---

## 6. Where the bytes are

### 6.1 Primary vs secondary page

Reclaimer (`Halo3\ResourceIdentifier.cs` `ReadData(PageType.Auto, maxLength)`) uses the **secondary
page when the segment has one**, else the primary, and `BitmapTag.ToDds` asks for exactly
`mip 0 size` bytes (`TextureUtils.GetBitmapDataLength(props, includeMips = false)` except for MCC
arrays, see 6.4). That works because of the following layout (**bytes**, 2439/2439):

| segment | what the primary page holds | what the secondary page holds |
|---|---|---|
| location type 1, no secondary page (1132 bitmaps; all cube maps and arrays, and small 2D) | the whole chain: mip 0, mip 1, ..., mip n (`definition +0` bytes) | — |
| location type 3, secondary page present (1315 bitmaps; bitmap flags bit 5 and definition +47 bit 0 set) | **mips 1..n** only (`definition +0` bytes) | **mip 0 only**, all faces (`definition +20` bytes) |

The high-resolution secondary page is **private to one bitmap**: its segment offset is 0 and the
page's uncompressed size equals the mip 0 size on all 1315 cases. The primary page is shared with
many resources (segments are allocated in 64 KiB / `0x10000` granules; the gap to the next segment
is **not** the data size, use the definition sizes). Both pages of a bitmap are always in the same
file: riverworld has 160 local+local, 138 local-only, 1155 shared+shared, 986 shared-only; none mixed.

Reading mip 0 (what the game streams in first / what Reclaimer exports) = secondary page if present
else the first `mip0 bytes` of the primary. Reading the full chain = secondary (mip 0) followed by
primary (mips 1..n), or just the primary.

### 6.2 Mip sizes and packing

For level `l` (0 = base): `w_l = max(1, width >> l)`, `h_l = max(1, height >> l)`;
block formats: `ceil(w_l / 4) * ceil(h_l / 4) * bytes_per_block`; linear formats: `w_l * h_l * bpp / 8`.
(Reclaimer's `DdsImageDescriptor.GetPixelDataLength` uses `frame * 0.25^l` with a one-block minimum;
same numbers for power-of-two sizes.) Levels follow each other **with no alignment or padding**:
`plasma_sm_red_mask` (A8 512x512, 9 mips, secondary page) has `definition +0 = 0x15555` = exact
unpadded sum of mips 1..9 (4-byte alignment per level would give `0x15558`); `default_alpha_test`
(A8 64x64, 6 mips, primary only) sits alone in a page of `5461 = 0x1555` bytes = the exact chain.
Mip 1 of every DXT1/DXT5 2D texture tested starts at primary offset 0 and matches a 2x2 box
downsample of mip 0 with mean absolute error 0.35..4.3 (random alignment: 70..100). See section 8.

### 6.3 Cube maps (type 2): mip-major, 6 faces per level

`faces = 6`, `depth = 1`. Within one region the layout is **level 0 of faces 0..5, then level 1 of
faces 0..5, ...**: `offset(level, face) = sum_{l < level} 6 * size_l + face * size_level`.
Verified on `riverworld_riverworld_cubemaps[0]` (128x128 DXT5, 2 mips, primary only, `definition +0 =
0x1F800 = 6 * (0x4000 + 0x1000 + 0x400)`): for every face the mip-major candidate downsample-matches
with MAE 1.4..2.7 while the face-major candidate gives 9..15; face 0 mip 2 at `+0x1E000` matches mip 1
(MAE 2.7). Reclaimer reads `6 * mip0` bytes from the segment start as six consecutive faces, consistent.

Face order (Reclaimer `Common\CacheFactory.cs` `MccGen3CubeLayout`, used for MCC by
`BitmapTag.CubeLayout`): face 1 = Right (rotate 270), face 2 = Back (rotate 180), face 3 = Left
(rotate 90), face 4 = Front, face 5 = Top, face 6 = Bottom (rotate 180). The Xbox 360 order
(`Gen3CubeLayout`) differs (Right, Left, Back, Front, Top, Bottom). These names are Reclaimer's
cross-layout labels, not D3D `+X/-X/...` axes; **mapping to wgpu cube faces is unsure** until a cube map
is viewed in the renderer (section 12). No cube map in the installed maps has a secondary page.

### 6.4 Arrays (type 3): slice-major

`depth` = slice count (2, 3, 8, 16 seen). Layout is **each slice's full chain, slice after slice**:
`offset(slice, level) = slice * chain_size + sum_{l < level} size_l`. Verified on
`levels\shared\bitmaps\nature\water\water_ripples` (256x256x3 A8R8G8B8, 8 mips, primary only):
slice 0's mip 1 at `+0x40000` (slice-major) matches its downsampled mip 0 with MAE 0.39; the
mip-major candidates (`+0xC0000`, `+0xD0000`) give 31..33. Reclaimer reads MCC arrays with
`includeMips = true` (`useMips` in `ToDds`) and emits a DDS array, whose layout is also slice-major, so
it passes the bytes through unchanged (`ArrayMipLayout = Fragmented` is set but never consumed
anywhere in Reclaimer). No array in the installed maps has a secondary page (array flags are `4` or
`0x14`, never `0x20`); the riverworld lightmap array has 0 mips.

3D textures (type 1): none in any installed map; layout unverified (Reclaimer: `Depth = depth`,
`FrameCount = depth`, same as an array).

---

## 7. Algorithm: `bitm` tag -> for each mip: (width, height, format, bytes)

```
decode_bitmap(cache, bitm_tag, i):                     # i = index into Bitmaps (usually 0)
    bm   = bitm.Bitmaps[i]                             # 56 bytes at tag+96 block
    rid  = bitm.Resources[i].id                        # 8-byte block at tag+140; InterleavedResources is empty on MCC
    entry = zone.entries[rid & 0xFFFF]
    if entry.salt != rid >> 16 or entry.type != type_named("bitmap_texture_interop_resource")
       or entry.segment < 0: return None               # 17 dead refs in riverworld
    def  = fixup_blob[entry.fixup_off .. +68]
    w, h       = u16 def[40], u16 def[42]              # == bm.width, bm.height
    depth      = def[44] ; levels = def[45] (= bm.mips + 1) ; ty = def[46]
    dxgi       = i32 def[48]                           # the wgpu format (table 5.1); sRGB already chosen
    fmt        = def[52] (= bm.format) ; curve = def[53]
    primary_size   = i32 def[0] ; secondary_size = i32 def[20] ; hi_res_split = def[47] & 1
    faces      = 6 if ty == cube else (depth if ty in (array, 3d) else 1)
    seg = play.segments[entry.segment]
    primary   = read_segment(seg.primary_page,   seg.primary_off,   primary_size)       # resources.md section 8
    secondary = read_segment(seg.secondary_page, seg.secondary_off, secondary_size) if hi_res_split else b""
    # (both segment offsets are 0 inside the fixups; the data begins at the segment offset)

    size(l) = block ? ceil(w_l/4)*ceil(h_l/4)*bpb : w_l*h_l*bpp/8   with w_l = max(1, w >> l), h_l = max(1, h >> l)

    if ty == array:                                    # slice-major; never hi_res_split in practice
        chain = sum(size(l) for l in 0..levels)
        bytes(slice, l) = primary[slice*chain + sum(size(k) for k < l) ..][: size(l)]
    else:                                              # 2D and cube: mip-major, `faces` copies per level
        if hi_res_split:
            assert len(secondary) == faces * size(0)
            region(l) = secondary if l == 0 else primary ; start(l) = 0 if l == 0 else sum(faces*size(k) for 1 <= k < l)
        else:
            region(l) = primary ; start(l) = sum(faces*size(k) for k < l)
        bytes(l, face) = region(l)[start(l) + face*size(l) ..][: size(l)]
    yield for each l: (w_l, h_l, dxgi -> wgpu format, bytes(l, face) for face in 0..faces)
```

Notes for the Rust side:
- Inflate pages once and cache them (`resources.md` section 6): most riverworld world textures sit
  in the 92 MB local page 1420 (primary) plus one private page per high-res mip 0; shared textures are
  in `shared.map` pages.
- The upload for wgpu: per level, `bytes_per_row = ceil(w_l/4) * bpb` for BC formats or `w_l * bpp/8`
  for linear ones (row pitch is tight, no padding), `rows = ceil(h_l/4)` or `h_l`.
- A4R4G4B4 (and R5G6B5 / A1R5G5B5 if ever met) must be expanded to RGBA8 on the CPU.
- `Resources.count == Bitmaps.count`; only the first bitmap is used for a material (Reclaimer's
  `Gen3MaterialHelper.PopulateTextureMappings` takes `ToDds(0)` and `GetSubmapGamma(0)`); multi-bitmap
  tags are UI sprite sheets, cubemap sets and animated particle frames.

---

## 8. EXPECTED VALUES on riverworld.map

All from the output of `probe_bitmaps.py` on riverworld. "primary page 1420" is the local
92 MB page (`riverworld.map @ 0x114000`, see `resources.md` section 10); the secondary pages are also
local (cache index -1).

| bitmap tag | tag index | size | type | format | mips | curve | resource id -> zone entry | segment: primary / secondary |
|---|---|---|---|---|---|---|---|---|
| `levels\multi\riverworld\sky\bitmaps\mountains_snow` | 112 (0x70) | 1024x512 | 2D | DXT1 (14) -> DXGI 71 BC1_UNORM | 10 | linear | `0xE1780015` -> 21 | seg 14: page 1420 (riverworld.map) `+0xE0000`, 87400 bytes (= `0x15568` mips 1..10) / page 1424 (riverworld.map) `+0`, 262144 bytes (= `0x40000` mip 0) |
| `levels\multi\riverworld\bitmaps\forerunner\cust_base_floor_dif` | 11437 (0x2CAD) | 1024x1024 | 2D | DXT5 (16) -> DXGI 77 BC3_UNORM | 8 | linear | `0xFDF21C8F` -> 7311 | seg 7059: page 1420 `+0x2CF0000`, `0x55550` bytes / page 1524 `+0`, `0x100000` bytes |
| `levels\multi\riverworld\bitmaps\forerunner\cust_base_floor_bump` | 11438 (0x2CAE) | 1024x1024 | 2D | DXN (33) -> DXGI 84 BC5_SNORM | 8 | linear | `0xFDF31C90` -> 7312 | seg 7060: page 1420 `+0x2D50000`, `0x55550` / page 1525 `+0`, `0x100000` |
| `objects\levels\multi\riverworld\bitmaps\man_cannon_diff` | 6631 (0x19E7) | 256x512 | 2D | DXT5 (16) -> DXGI 78 BC3_UNORM_SRGB | 7 | xRGB | `0xEDDA0C77` -> 3191 | seg 3155: page 1420 `+0x1330000`, `0xAAB0` / page 1506 `+0`, `0x20000` |
| `levels\multi\riverworld\riverworld_riverworld_cubemaps` [0 of 55] | 6882 (0x1AE2) | 128x128 | cube | DXT5 (16) -> DXGI 78 | 2 | xRGB | `0xEE3E0CDB` -> 3291 | seg 3255: page 1420 `+0x2450000`, `0x1F800` bytes (whole chain, 6 faces) / none |
| `levels\multi\riverworld\bitmaps\cliff_rock_base_brown` | 11455 (0x2CBF) | 2048x2048 | 2D | DXT5 -> 77 | 11 | linear | `0xFE001C9D` -> 7325 | seg 7073: page 1420 `+0x2F30000`, `0x155570` / page 1532 `+0`, `0x400000` |
| `shaders\default_bitmaps\bitmaps\gray_50_percent` | 18 (0x12) | 1x1 | 2D | A8R8G8B8 -> 87 | 0 | linear | `0xE1650002` -> 2 | seg 2: shared.map page 0 (shared index 2) `+0x30000`, 4 bytes / none |
| `shaders\default_bitmaps\bitmaps\default_alpha_test` | 53 (0x35) | 64x64 | 2D | A8 -> 65 | 6 | linear | `0xE16D000A` -> 10 | seg 6: shared.map page 17 `+0`, `0x1555` bytes (page uncompressed size is exactly that) / none |
| `levels\shared\bitmaps\nature\water\water_ripples` | — | 256x256 x3 | array | A8R8G8B8 -> 87 | 8 | linear | `0xFA5218EF` -> 6383 | seg 6132: shared.map page 0 `+0x1CC0000`, `0xFFFFC` bytes (3 slice-major chains) / none |

Definition dumps (68 bytes), with the fields of section 4.1 visible:

```
mountains_snow-like example, clouds_patchy_mask (256x256 DXT5, 6 mips, linear, secondary page), entry 19:
  50550000 00000000 00000000 00000000 | 00000000 00000100 00000000 00000000
  00000000 00000000 00010001 01070001 | 4d000000 10030300 00000000 00000000 | b2f55d50
  = primary 0x5550, secondary 0x10000, 256x256, depth 1, 7 levels, type 0, flags 1, DXGI 0x4D = 77, fmt 16, curve 3, more 3
riverworld_riverworld_cubemaps-like example, default_dynamic_cube_map (64x64 DXT5 cube, 6 mips, xRGB), entry 12:
  a0800000 ... 40004000 01070200 | 4e000000 10010300 ...  = primary 0x80A0 = 6*(0x1000+..+0x10), 64x64, 7 levels, type 2, DXGI 78 (sRGB because curve 1)
random4_warp (128x128 DXN, 5 mips, linear), entry 0:
  50550000 ... 80008000 01060000 | 54000000 21030300 ...  = DXGI 0x54 = 84 BC5_SNORM, fmt 0x21 = 33
```

First 16 bytes of mip 0 and the first block decoded:

- **DXT1** `mountains_snow` (secondary page 1424, offset 0): `55 84 4e 63 2d a9 2b 2b | 59 a5 55 84 ad ad 2f 2b`.
  Block 0: `c0 = 0x8455` -> RGB565 `(132, 138, 173)`, `c1 = 0x634E` -> `(99, 105, 115)`, `c0 > c1` so
  4-colour mode, palette `[(132,138,173), (99,105,115), (121,127,153), (110,116,134)]`, index word
  `0x2B2BA92D` -> pixel indices `1 3 2 0 / 1 2 2 2 / 3 2 2 0 / 3 2 2 0`: a blue-grey snow/sky pixel block,
  plausible for a mountain sky dome. Mip 1 (512x256 DXT1 = `0x10000` bytes, the first bytes of the
  primary segment: page 1420 offset `0xE0000`) downsample-matches mip 0 with MAE 4.32 (control 86.5).
- **DXT5** `cust_base_floor_dif` (secondary page 1524): `00 1b 49 42 86 db 24 c0 | c7 31 41 08 00 8a ff 57`.
  Block 0: alpha endpoints `a0 = 0, a1 = 27` (8-value mode since `a0 <= a1`), alphas
  `27 27 27 27 16 16 27 16 10 10 10 5 5 0 0 0`; colour `c0 = 0x31C7 (49, 57, 58)`, `c1 = 0x0841 (8, 8, 8)`:
  a dark grey Forerunner floor with a low-alpha (specular mask) block; plausible. Mip 1 at primary
  offset 0 matches with MAE 4.30 (control 88.8).
- **DXT5** `man_cannon_diff` (secondary page 1506): `a1 4d ed c0 72 75 e9 a7 | 96 b5 ef 7b e0 e0 e0 70`:
  alpha `161..77`, colour `(181,178,181)` / `(123,125,123)`, grey metal; MAE 2.51 (control 70.6).
- **DXN** `cust_base_floor_bump` (secondary page 1525): `00 00 00 00 00 00 00 00 | 00 8f 00 00 00 00 90 24`:
  red (X) block endpoints `0, 0` (flat), green (Y) block endpoints `0, 143`; 884459 of 1048576 bytes
  are zero. In BC5_SNORM, `0` means a 0.0 component, so most of the texture is a flat `(0, 0, 1)`
  normal; in BC5_UNORM it would be `(-1, -1)` everywhere, impossible. Confirms signed.
- **A8R8G8B8** `gray_50_percent`: `40 40 40 ff` = B=G=R=0x40, A=0xFF.
- **A8** `default_alpha_test`: `00 ff ff ff ff ff 00 00 ff ff ff ff ff ff ff 00` (a checker mask).

Histograms (riverworld): formats `A8 117, Y8 1, A4R4G4B4 160, A8R8G8B8 394, DXT1 323, DXT3 3,
DXT5 1012, U8V8 97, fmt25 8, DXN 349`; types `2D 2388, cube 71, array 5`; mip counts 0..11 (`0` x494,
`10` x503); curve `xRGB 1119, linear 1115, sRGB 194, unknown 35, gamma2 1`; (format, curve) -> DXGI
table in the verify file, one DXGI value per pair, e.g. `('DXT5','xRGB') -> 78`, `('DXT5','linear') ->
77`, `('A8R8G8B8','unknown') -> 91`, `('DXT1','sRGB') -> 72`, `('A8', any) -> 65`.

---

## 9. Per-map scan (all 36 installed maps, `probe_bitmaps.py` run per map)

- `InterleavedResources.count == 0` on every tag of every map (campaign `005_intro` .. `130_epilogue`,
  all multiplayer maps, `armory`, `s3d_*`, `chill`). A Rust reader may reject a non-empty block with a
  clear error rather than implement the interleaved layout.
- Formats present anywhere: A8, Y8, A4R4G4B4, X8R8G8B8 (one bitmap, `040_voi`), A8R8G8B8, DXT1, DXT3,
  DXT5, U8V8, 25 (RGBA FP16, always the same 8 UI/rasterizer tags), DXN. Never: DXT5a, CTX1,
  DXT3a/DXT5a variants, DXN_mono_alpha, 16-bit colour, FP32, P8.
- Types: 2D, cube, array. No 3D anywhere.
- Bitmap flags byte: `0`, `0x20`, `0x04`, `0x14`, and `0x34` (bits 2,4,5) on 1..5 bitmaps in some maps
  (`120_halo`, `chill`, `construct`, `salvation`, ...).
- Definition length 68 and fixup pattern identical in every map checked (riverworld, cyberdyne,
  005_intro, deadlock, 120_halo). The bitmap resource type index is **0 in riverworld, deadlock,
  bunkerworld, ghosttown, isolation, zanzibar** and **1 in cyberdyne, armory, chillout, docks,
  fortress, lockout, midship, sandbox, shrine, snowbound, spacecamp, warehouse, s3d_*, 130_epilogue,
  ...**: resolve by name.

---

## 10. Reclaimer behaviour that does not apply to MCC PC (and why)

Source: `Halo3\BitmapTag.cs` `ToDds`, `Utilities\TextureUtils.cs` `GetDds`, `Utilities\BitmapProperties.cs`,
`Utilities\XG.cs`.

| Reclaimer step | condition | MCC Halo 3 U13 |
|---|---|---|
| big-endian unit swap (`GetDds`: reverse every `ReadUnitSize` bytes) | `ByteOrder == BigEndian` | **no**: `CacheArgs.FromFile` sets little-endian for MCC files (`Common\CacheArgs.cs`); the BC blocks above decode correctly as little-endian |
| Xbox 360 tiling (`XTextureUnscramble`) | `Swizzled` flag = bitmap flags bit 3 | **no**: bit 3 never set in any map |
| Xbox One tiling (`XG.Detile2D`) | `X1TileMode > 0` | **no**: only set by `Halo5\BitmapTag.cs` |
| 128/32-pixel dimension padding (`UsesPadding`, `ApplyCrop`) | `!IsMcc` | **no**: `UsesPadding = false` on MCC; data sizes match the exact dimensions (section 6.2) |
| `DXNSwap` (DXN -> DXN_SNorm) | `Platform == PC` | **yes**: DXN is BC5_SNORM (confirmed by DXGI 84 in the definition and by the zero blocks) |
| `useMips` / `ArrayMipLayout.Fragmented` | MCC and type Array | reads the whole array chain; `Fragmented` is never consumed; bytes are slice-major (6.4) |
| Xbox LZX (`XCompress`), Oodle | other engines | no: pages are raw deflate (`resources.md` 6) |
| `ReadData(PageType.Auto, mip0 length)` | always | matches: secondary page = mip 0 when present |

HaloExport (`HaloExport\Program.cs`) only ever calls `ToDds(0)` and
decodes to BGRA32 PNGs; it never used mips or the other bitmaps of a tag, so the reference exports say
nothing about mip layout; the checks above are the first on this machine.

---

## 11. Checks a Rust reader should reproduce (`cargo test` candidates)

1. riverworld: 2204 `bitm` tags, 2464 bitmap blocks; every tag has `Resources.count == Bitmaps.count`
   and `InterleavedResources.count == 0`.
2. Tag 112 `mountains_snow`: block = 1024x512, depth 1, flags `0x20`, type 0, format 14, mips 10,
   curve 3; resource id `0xE1780015` -> entry 21 (salt `0xE178`), fixup length 68, definition
   `primary = 0x15568`, `secondary = 0x40000`, levels 11, DXGI 71; segment 14 -> primary page 1420 offset
   `0xE0000`, secondary page 1424 offset 0 with uncompressed size `0x40000`.
3. Mip 0 of `mountains_snow` begins `55 84 4e 63 2d a9 2b 2b`; mip 0 of `cust_base_floor_dif` begins
   `00 1b 49 42 86 db 24 c0`; `gray_50_percent` is `40 40 40 ff`.
4. For every live bitmap entry: `definition +0 == sum(sizes of mips 1..n)` when `+47 & 1` else the
   full chain; `+20 == faces * size(0)` when split else 0; `+45 == mips + 1`; `+52 == format`.
5. Cube map tag 6882 bitmap 0: `definition +0 == 0x1F800 == 6 * (0x4000 + 0x1000 + 0x400)`.
6. Every (format, curve) pair maps to exactly one DXGI value; DXN always 84; A8 always 65.
7. 17 resource ids in riverworld point at wiped entries (`type 0xFF`) and must yield "no texture",
   not a panic.

---

## 12. Unsure spots / open questions

- **Cube face order and orientation for wgpu.** Reclaimer's `MccGen3CubeLayout` (Right, Back, Left,
  Front, Top, Bottom with rotations 270/180/90/0/0/180) is a DDS-cross display layout, not the D3D
  `+X -X +Y -Y +Z -Z` order. The six 128x128 faces of `riverworld_riverworld_cubemaps[0]` all average
  ~(66, 66, 65) so the brightest-face heuristic does not identify "up". Decide when a reflection cube
  map is first rendered; try D3D order first (the engine is D3D-based), fall back to Reclaimer's table.
- **Arrays or cube maps with a secondary page** never occur in the installed maps; the algorithm
  assumes the secondary page would hold `faces * size(0)` bytes (mip-major) for cubes and is left
  undefined for arrays.
- **3D textures** (type 1): none exist; untested.
- **Bitmap flags** bits 2 and 4 (`0x04`, `0x14`, `0x34`) on arrays and a few 2D bitmaps; "more flags"
  (block +14 / definition +54) values 0..6; the 24 tail bytes of the bitmap block; the 84-byte tag
  header: all read from bytes, meanings unknown, none needed for decoding.
- **Format 25**: identified as RGBA FP16 from the DXGI value 10 and the byte size (64 bpp); the
  channel order inside the 8 bytes is taken from DXGI (`R16 G16 B16 A16`), not verified visually.
- **Interleaved resources**: layout (`s_tag_d3d_texture_interleaved`, two textures in one page) is not
  documented here because no map uses it; `InterleavedIndex` / `Index2` semantics are from Reclaimer's
  field names only.
- **Gamma**: Reclaimer's 1.95 for xRGB/unknown is its own display choice; the game's DXGI value treats
  xRGB and unknown as sRGB, which is what a wgpu renderer should use.
- `PixelsSize` is the full chain size on 1284 bitmaps and 0 on 1155 with no obvious rule; ignore it.
