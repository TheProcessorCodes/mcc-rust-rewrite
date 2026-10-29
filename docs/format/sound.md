# Sound in MCC Halo 3 (U13): tags in the map, audio in an FMOD bank

Written 2026-10-28 from our own probes of the user's install (`riverworld.map`, `120_halo.map`,
`halo3\fmod\pc\sfx.fsb`, `halo3\halo3.dll`). No GPL code or plugin XML was used: the tag offsets
were found by walking block headers (`blam blocks`) and matching contents against things whose
meaning is obvious (names, durations, distances). FSB5 framing is public knowledge (FMOD's
format as described by python-fsb5, MIT, and vgmstream, ISC-style); nothing of theirs is copied.

Code: `crates/blam_sound` (`fsb.rs` the bank, `vorbis.rs` setups and decoding, `tags.rs` the
tags), CLI `blam sound <map> list | show TAG | wav TAG... [--out DIR] | check`, probe
`python -I docs/format/scripts/probe_fsb5.py "<halo3 dir>" sfx header|stats|info N|grep TEXT`.
Tests: `crates/blam_sound/tests/install.rs` pin the numbers below.

## 0. Where the audio is

Not in the maps. Every `snd!` resource (gestalt type 3 `sound_resource_definition`, 2216 on
riverworld) has location type 0 and no page (`resources.md` 12). MCC moved Halo 3's audio to
FMOD sound banks in `halo3\fmod\pc\`:

| file | size | what |
|---|---|---|
| `sfx.fsb` + `sfx.fsb.info` | 724 MB, 4.6 MB | every sound effect, ambience and music: 16,421 samples |
| `english.fsb` (+ french, german, italian, mexican, spanish) + `.info` | ~460 MB each | dialogue, one bank per language (not read yet) |

There is no `.bank` (FMOD Studio event) file: the game plays samples itself, the tags say how.
FMOD's runtime is linked into `halo3.dll` (no `fmod*.dll` in the Halo 3 folder).

## 1. FSB5 header and samples (`sfx.fsb`)

| offset | type | value on sfx.fsb |
|---|---|---|
| `0x00` | char[4] | `FSB5` |
| `0x04` | u32 | version 1 (header is `0x3C` bytes; version 0 is `0x40`) |
| `0x08` | u32 | sample count 16,421 |
| `0x0C` | u32 | sample headers size `0xB71A4` |
| `0x10` | u32 | name table size `0x4C580` (not needed: the `.info` has the names) |
| `0x14` | u32 | data size `0x2B1DCF00` |
| `0x18` | u32 | codec 15 = **Vorbis** for the whole bank |
| `0x1C` | u32 | flags 1 |
| `0x2C` | 16 bytes | hash |

Data starts at `0x3C + headers + names` = `0x103760`. Sample headers follow the file header: a
u64 bitfield, then chunks while bit 0 says "more":

| bits | meaning |
|---|---|
| 0 | has chunks |
| 1-4 | rate index (1 8000, 2 11000, 3 11025, 4 16000, 5 22050, 6 24000, 7 32000, 8 44100, **9 48000**) |
| 5 | stereo |
| 6-33 | data offset / **16** (sample 1 starts at 2528 = 158 x 16, right after sample 0's 2502 bytes rounded to 32) |
| 34-63 | frames |

Chunk: u32 (bit 0 more, bits 1-24 size, bits 25-31 type), then the body. Types seen: **11**
Vorbis data (u32 setup CRC, then a seek table), on every sample; **14** Vorbis intra layers (u32
layer count), on 270 samples. Every sample is 48 kHz; 12,339 mono, 4,082 stereo; no loop chunks.
Two setup CRCs only: `0x632B1FAC` (mono) and `0xAD11D38D` (stereo; also the 4-layer samples use
the mono one, see 3.3).

## 2. The `.info` sidecar: id, path, loop end

`sfx.fsb.info` is 280-byte records, one per sample in bank order (4,597,880 / 280 = 16,421):

| offset | type | meaning |
|---|---|---|
| `0x00` | u32 | **FMOD id**: what a tag permutation stores (4.2). It is djb2 (`h = h*33 + byte`, seed 5381) of the source path without `data\sound\` and `.aif`: `characters\bodyfalls\bodythud\bodythud1` -> `0x207132B0` |
| `0x04` | u32 | 16-bit PCM bytes = frames x channels x 2 |
| `0x08` | u32 | channels of the source (1, 2, 4, 6) |
| `0x0C` | u32 | 2 (bytes per sample) |
| `0x10` | u32 | 0 |
| `0x14` | u32 | **loop end**: last frame of the loop region for permutations of loop sounds (`halo_ext1`: 178,240 of 178,324 frames; music `new_lp1`: 1,474,559 = frames - 1), 0 for one-shots |
| `0x18` | char[256] | source path, `data\sound\...\name.aif` |

## 3. Vorbis without headers

### 3.1 Packets

A sample's data is Vorbis audio packets, each behind a u16 length (0 ends it). The identification
and setup headers are left out. The identification header is rebuilt: 30 bytes `\x01vorbis`,
version 0, channels, rate, three zero bitrates, block sizes byte `0xB8` (**256 and 2048**), framing
1. Checked by frame counts: 28 packets give 13,760 frames for sample 0's 13,725 (with 128 / 2048
it would be fewer than the header says). The comment header is not needed.

### 3.2 Setup headers: rebuilt from halo3.dll

FMOD keys setups by a CRC and keeps the table in its runtime, here `halo3.dll` (64-bit PE; virtual
address - image base `0x180000000` - section delta gives the file offset). The table is 32-byte
rows, found by scanning for the CRCs:

```
u32 n (0x1C..0x21 or 2)   u64 tail     u32 total      <- 16 bytes
u32 crc                   u64 base     u32 shared     <- 16 bytes
```

`base` points at a complete-looking setup packet (`\x05vorbis`, 33 in the DLL); several CRCs
share one base. The real packet for a CRC is `total` bytes: the first `shared` bytes of `base`,
then bytes from `tail`, then `base` again (`base[shared + k .. total]`). Where the tail's part
ends (`k`) was not found in the row, so the reader tries each `k` until **CRC-32 of the packet
equals the key**, which is what FMOD keys it by (verified for both CRCs the bank uses: stereo
`total` 3484, `shared` 3405, differing from `base` in bytes 3405 and 3435; mono 3006 / 2934). 129
of the DLL's 137 rows rebuild exactly this way. Taking `base` as it is parses and decodes, but its
residue setup belongs to another CRC: loud passages came out as broadband noise (peaks of 7 on
`halo_ext`, 18 on the Warthog run music; per-packet "roughness" 1.8 against 0.2 for the rest).
That is why the check matters.

The packets were cross-checked against python-fsb5's published table (it has both CRCs; the
rebuilt bytes are identical). Nothing is stored in this repo: setups are read from the user's own
DLL at run time.

### 3.3 Intra layers

A sample with chunk 14 (`n` layers) is one ordinary Vorbis stream of `n` x header channels
(vgmstream reads it the same way). 120_halo's `halo_ext_wind` and `for_int_big_quad` loops are
"mono" with 4 layers: 4-channel audio, durations as the header says. Interleaving per-packet across
layers was tried first and gives a quarter of the length. Channel order assumed FL FR BL BR
(`Pcm::to_stereo`), not checked.

### 3.4 Decoding

lewton 0.10 (pure Rust, MIT/Apache-2.0) with `read_header_setup` and `read_audio_packet_generic`.
About 2 ms per second of stereo audio in a debug build (`blam sound check`: riverworld 102
permutations, 534 s of audio, in 0.6 s; 120_halo 188 permutations, 2,633 s, in 3.8 s; none
failed, none silent).

## 4. `snd!` and the `ugh!` sound gestalt

### 4.1 `snd!`, 36 bytes (compact cache form)

| offset | type | meaning | weird_bird (riverworld #6786) |
|---|---|---|---|
| `0x00` | u16 | flags | 9 |
| `0x02` | u8 | sound class | 33 |
| `0x03` | u8 | sample rate enum | 1 |
| `0x04` | u8 | encoding: 0 mono, 1 stereo | 0 (`halo_ext\loop`: 1) |
| `0x06` | i16 | **first pitch range** (`ugh! + 0x40`) | 880 |
| `0x0C` | i16 | **playback parameters** (`ugh! + 0x10`) | 51 |
| `0x12` | i16 | per-sound index (consecutive across tags; not decoded) | 806 |
| `0x14` | i32 | longest permutation, ms | 3752 (weird_bird6 is 3.75 s) |
| `0x18` | u32 | resource id (no data in MCC) | |

### 4.2 `ugh!` blocks (offsets from the tag start; element sizes from the block gaps)

| offset | element | count (riverworld) | what |
|---|---|---|---|
| `0x10` | 68 | 259 | playback parameters (4.3) |
| `0x28` | 4 | 5,595 | import names (string ids: permutation names) |
| `0x40` | 12 | 2,371 | pitch ranges |
| `0x4C` | 20 | 7,625 | permutations |
| `0xBC` | ? | 2,290 | one per `snd!` (2,290 tags) |

**Pitch range, 12 bytes**: `0x00` i16 name, `0x02` i16 parameter index, `0x08` u16 **first
permutation**, `0x0A` u16 with the **permutation count in bits 4-9** (`0x0490` -> 9: weird_bird
has 9 files in the bank). Only the first pitch range of a sound is read.

**Permutation, 20 bytes**: `0x00` u16 import-name index (`weird_bird2`), `0x04` u32 about the
frame count, `0x08` u32 first chunk index, `0x0C` u8 chunk count, `0x10` u32 **FMOD id**
(`0x073D1E2E` = info record 14801 `weird_bird2.aif`). The ids of a sound's numbered permutations
are consecutive because djb2's last step adds the last character.

### 4.3 Playback parameters, 68 bytes

`0x00` u32 flags (bit n set: distance n unused; weird_bird `0x13`), `0x04` four f32 distances
(world units), `0x1C` f32 gain dB, `0x20` f32 gain variance? (weird_bird -12), `0x28`/`0x2C`
cone angles (pi). Entry 0: 10, 45, 50, 100; weird_bird: unused, unused, 35, 45. The viewer
takes the last two as full volume within / silent beyond (not checked against the game).

## 5. Looping sounds (`lsnd`)

| offset | what |
|---|---|
| `0x00` | u32 flags |
| `0x24` | i16 sound class |
| `0x28` | block of **tracks**, 144 bytes |
| `0x34` | block of **detail sounds**, 60 bytes |

**Track**: `0x00` string id name, `0x04` u32 flags, `0x08` f32 gain dB (halo_ext -13), `0x0C` /
`0x10` f32 fade in / out s, tag references (`snd!`) **`0x14` in, `0x24` loop, `0x34` out, `0x44`
alt loop, `0x54` alt out, `0x68` alt transition in, `0x78` alt transition out**. Pinned by
120_halo's `120_music_15` whose references are named `track1\in`, `loop`, `alt_loop`, `alt_out`,
`alt_trans_in`.

**Detail**: `0x00` string id, `0x04` `snd!` reference, `0x14` f32 x2 random period s (weird_bird
20-45), `0x1C` f32 gain dB, `0x20` u32 flags, `0x24` yaw bounds, `0x2C` pitch bounds (radians,
-pi..pi and -pi/2..pi/2), `0x34` distance bounds wu (15-20).

## 6. Sound scenery

`ssce` is an object; **`ssce + 0xBC`** is the object attachments block, whose first element's tag
reference (at +0) is the `lsnd` (`water_creek` ssce #5282 -> lsnd #5283).

Scenario: **`scnr + 0x180`** sound scenery placements, 112 bytes; **`scnr + 0x18C`** their palette
(16-byte `ssce` references; 12 on riverworld). Placement: the usual object header (`0x00` i16
palette, `0x02` i16 name, `0x08` position, `0x14` rotation, `0x38` object id with **origin BSP at
`0x3C`** and type 9), then `0x50` i32 volume type (1 on every riverworld one), `0x54` f32 height,
**`0x5C`, `0x60` f32 distance bounds** (0 = the sound's own; riverworld's far waterfall 50 / 120,
the creek 6 / 12), `0x64` cone angles. riverworld: 77 placements (waterfall, creek and brook along
the river, the lake shore).

## 7. BSP acoustics: background sound per cluster

**`sbsp + 0x118`**, 84 bytes ("acoustics palette"): `0x00` string id name, `0x04` `snde` (sound
environment, reverb), `0x14` f32 1.0, `0x18` f32 0.25, **`0x1C` `lsnd` background sound**,
**`0x2C` `lsnd` inside-cluster sound**, `0x3C` f32 1.0, `0x50` f32 0.5 (taken as the fade time).
riverworld: 0 `halo_ext` (outdoor wind, birds, cicadas), 1 and 2 `riverworld_interior`.

**Cluster (280 bytes) `+0x1C`**: i16 index into that palette, -1 none (most riverworld clusters 0,
the bases 1 / 2). Read into `ClusterInfo::acoustics` (`blam_geometry`). `+0x1E` i16 runs 0..7
over the clusters (not decoded).

## 8. Music

Music is `lsnd` tags started by HaloScript (`sound_looping_start`), not placed anywhere. The
script strings name them: 120_halo has globals `g_music_120_01` .. `g_music_120_15` and lsnd tags
`levels\solo\120_halo\music\120_music_NN`; **`120_music_15` is `sound\music\misc\halo3_trench`**,
the Warthog run (in 92 s, four loop permutations of 31-35 s, alt loop, alt out 77 s). The viewer
lists every `lsnd` under a `\music\` folder and plays them by hand.

## 9. Not decoded

Dialogue banks; pitch ranges past the first; `snde` reverb; cone gains; sound class gains and the
mix (`snmx`) (the classes' limits and default distances, and a reading of all four distances, are
in `weapon-sounds.md` 7); the per-sound `ugh! + 0xBC`
block; the 8 DLL setup rows that do not rebuild (unused by the bank).
