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
| `english.fsb` (+ french, german, italian, mexican, spanish) + `.info` | ~460 MB each | dialogue, one bank per language: 32,984 samples each (section 11) |

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

Probe: `python -I docs/format/scripts/probe_lsnd.py <map> [name filter] [--raw]` (header words,
each track's 144 bytes, every slot's permutations with their lengths from `sfx.fsb.info`).
The meanings below come from surveying every `lsnd` in every installed map (about 6,500) and
matching the values against what the sounds are: a field that is only ever set where its
meaning would matter is taken to mean that.

**Header**

| offset | type | what | evidence |
|---|---|---|---|
| `0x00` | u32 | flags (below) | |
| `0x04` | f32 | **music time**, s: a piece ends by itself after this long (inferred) | non-zero only on music: 120, 180 or 300 on most of 120_halo's pieces, 0 on the Warthog run and the stinger |
| `0x08` | u32 | 0 everywhere | |
| `0x0C`, `0x10` | f32 x2 | **distance**: full volume within, silent beyond (world units) | `smg_loop` 3 / 55 and `smg_lod_far` 35 / 70, the same pairs as the gun's near and far `snd!` layers (`weapon-sounds.md` 7); engine LODs 40 / 75 and 70 / 110; music 0.9 / 5 (unused: music is not placed) |
| `0x14`, `0x20` | i32 | -1 everywhere | |
| `0x24` | i16 | **sound class**: 32 on every music piece | |
| `0x28` | block | **tracks**, 144 bytes | |
| `0x34` | block | **detail sounds**, 60 bytes | |

Header flags seen: 2, 4, 8, 16, 24, 48, 50, 64, 112.

| bit | meaning | evidence |
|---|---|---|
| 1 (2) | **not a loop**: the loop sound plays once | `120_music_12`, whose only sound is `sound\music\stingers\orch3`; `020_music_10`; one cinematic glue sound |
| 2 (4) | **stops music**: starting it ends the other music | the Warthog run (`120_music_15`) and 29 other pieces across the campaign; never on non-music |
| 3-6 | on vehicle engines, weapon projectiles, multi-track ambience, stereo winds | not used here |

**Track**, 144 bytes:

| offset | type | what |
|---|---|---|
| `0x00` | string id | name (`track1` on every music track) |
| `0x04` | u32 | flags (below) |
| `0x08` | f32 | gain dB (halo_ext -13; the Warthog run -3) |
| `0x0C`, `0x10` | f32 | fade in, fade out, s |
| `0x14` | tag ref | **in** (intro) |
| `0x24` | tag ref | **loop** |
| `0x34` | tag ref | **out** (outro) |
| `0x44` | tag ref | **alt loop** |
| `0x54` | tag ref | **alt out** |
| `0x64` | u32 | 0 everywhere |
| `0x68` | tag ref | **alt trans in** |
| `0x78` | tag ref | **alt trans out** |
| `0x88` | f32 | **alternate cross-fade time**, s (10 on `120_music_13`, `020_music_06`) |
| `0x8C` | f32 | a second time: 0 on all music, equal to `0x88` on vehicle skid loops; not used |

Track flags seen: 0x0-0x7, 0x14, 0x17.

| bit | meaning | evidence |
|---|---|---|
| 0 (1) | **fade in** over `0x0C` when started | set exactly where the fade-in time is non-zero (577 + 366 + 324 tracks with 0.1-2 s) |
| 1 (2) | **fade out** over `0x10` when stopped | set exactly where the fade-out time is non-zero; on music, set on the pieces with no outro (`120_music_02`: 30 s, the Warthog run: 10 s) |
| 2 (4) | **cross-fade to the alternate loop** over `0x88` | set on `120_music_13` (alt loop, no transitions, `0x88` = 10) and on material loops with an alt loop (`0x88` 0.1-0.5) |
| 4 (16) | with both `0x88` and `0x8C` set (skids, water); not used | |

Slot references pinned by `120_music_15`, whose sounds are named `track1\in`, `loop`, `alt_loop`,
`alt_out`, `alt_trans_in`. A slot is a whole `snd!` with its permutations; the loop's
permutations are the bars the game picks from at random (the Warthog run: 4 loops of 31-35 s),
and each loop permutation's sample has its loop end in the bank's `.info` (frames - 1 on music),
so they join back to back without gaps.

**Detail**: `0x00` string id, `0x04` `snd!` reference, `0x14` f32 x2 random period s (weird_bird
20-45), `0x1C` f32 gain dB, `0x20` u32 flags, `0x24` yaw bounds, `0x2C` pitch bounds (radians,
-pi..pi and -pi/2..pi/2), `0x34` distance bounds wu (15-20). No music piece on 120_halo has any.

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

Music is `lsnd` tags started by HaloScript, not placed anywhere. The scenario's script tag
references (`scnr + 0x404`, `halo-script.md` 1) list every sound the scripts use: on 120_halo all
16 music pieces and 30 `snd!`s (dialogue, sparks, the run's island creaks and tower crash;
`MapSounds::script_sounds`). `blam sound 120_halo music` prints each piece with its rules, its
slots' permutations and lengths, and the script lines that drive it. Pinned in
`blam_sound/tests/install.rs` (`halo_music_pieces`).

**120_halo's pieces** (one track each, sound class 32, no details):

| piece | sound | slots | rules |
|---|---|---|---|
| `120_music_01` | `amb\pad\d_low` | in, loop, out | 180 s |
| `120_music_02` | `amb\glue\d_glue_hi` | in, loop | fade out 30 s; 300 s |
| `120_music_03` | `amb\tech\d_tech_lowperc` | in, loop, alt loop | fade out 15 s; 300 s |
| `120_music_04` | `amb\pad\d_hi` | in, loop, out | 120 s |
| `120_music_045` | `amb\spooky\a_spook_fx` | loop | fade out 10 s; 180 s |
| `120_music_05` | `numbers\14perc` | in, loop, out | 300 s |
| `120_music_06` / `07` | `amb\pulse\d_orch_amb` / `amb\glue\d_glue` | in, loop | fade out 15 s; 300 s |
| `120_music_08` | `amb\spooky\a_spook_fx` | loop | fade out 10 s; 300 s |
| `120_music_09` / `10` / `11` | `spook_tran_fx` / `more_spook_pulse` / `atonal_glue_voxlo` | in, loop | fade out 10 / 10 / 6 s; 300 s |
| `120_music_12` | `stingers\orch3` | loop | not a loop (a stinger); no script starts it |
| `120_music_13` | `amb\spooky\amb_rhythm` | in, loop, alt loop | fade out 10 s; cross-fade 10 s to the alternate; 300 s |
| `120_music_14` | `amb\pad\e_low` | in, loop, out | 180 s |
| `120_music_15` | `misc\halo3_trench` | in (92 s), loop (4), alt loop (3), alt out (77 s), alt trans in (15 s) | stops music; fade out 10 s |

**How the scripts drive them.** Each piece has a dormant script `120_music_NN` that waits for
the global `g_music_120_NN`, `(sound_looping_start <piece> none 1.0)`, waits for the global to go
false, then `(sound_looping_stop <piece>)`. The mission's chapter scripts `wake` those and set and
clear the globals; three places switch a piece to its alternate loop
(`sound_looping_set_alternate`: `120_music_03` in `sentinel_encounter`, `120_music_13` in
`120vc_arbiter_revives`, `120_music_15` in `trench04_navpoint_deactive`). The Warthog run:

1. `120_08_trench_run` wakes `120_music_15` and, once the player is in
   `120_08_trench_encounter_100` (the first stretch), sets `g_music_120_15`: the piece starts
   (intro, then its loops).
2. `trench04_navpoint_deactive`, on leaving the second island (`110_island02_trig_exit`), sets the
   alternate: after the playing loop, the 15 s transition, then the alternate loops.
3. `120_trench_run_timer_04`, at the last stretch (`trench_run_120_trig12`, then `..._trig12x` or
   10 s), clears `g_music_120_15`: the piece stops, which on the alternate loop plays the 77 s
   alternate outro (the finale) after the playing loop.

Pinned by `mcc_script/tests/trench_music.rs`, which drives the player through those volumes.

**How we play a piece** (`mcc_audio/src/music.rs`):

* Start: each track plays its intro, then picks loop permutations at random (never the same one
  twice running) with no gap. A track with the fade-in flag ramps up over its fade-in time. A
  "not a loop" piece plays one loop permutation and ends. A "stops music" piece ends every other
  music piece (by their own stop rules) when it starts.
* Alternate on: with the cross-fade flag, the alternate loop starts at once and fades in over
  `0x88` while the loop fades out; otherwise, when the playing permutation ends, the alt
  transition in (if any) plays, then the alternate loops. Off: the same back, through alt trans
  out.
* Stop: when the playing permutation ends, the alt out (on the alternate loop) or the out plays and
  the piece is over; with neither, a track with the fade-out flag fades over its fade-out time;
  else the playing permutation is the last. `sound_looping_stop_immediately` cuts it.
* Music time: a piece that has played that long stops itself (as `sound_looping_stop`). Inferred
  from the field only being set on music; not checked against the game.
* Pieces overlap freely, so one piece's outro or fade runs under the next one's intro: that is
  the cross-fade between pieces.
* `sound_class_set_gain` ramps the `music` and `amb` classes (music pieces; ambience, sound
  scenery and one-shots); other classes are logged and left alone. On top of that every sound
  plays at its class's gain (section 9: music 0 dB, effects and ambience 12 to 15 dB down) and
  the player's volume for its bus (pause menu, Sound tab).
* `sound_impulse_start` plays one permutation of a `snd!` once, at the object when one is named
  (by its distances), else at the listener. A dialogue `snd!` (its permutations are in the
  language bank, section 11) plays the same way, on the dialogue bus.

## 9. Sound class gains: why music sits on top

The `sncl` tag (`sound\sound_classes`, 54 classes of 152 bytes; limits and default distances in
`weapon-sounds.md` 7) also holds each class's level. Read 2026-10-30 from 120_halo by dumping
every class's non-zero words and naming the classes by the sounds that use them (the `snd!`
class byte, `lsnd + 0x24`; probe `python -I probe_sound_classes.py [map]`). No plugin XML or GPL
code was used; the meanings are inferred.

| offset | type | what (inferred) | evidence |
|---|---|---|---|
| `0x10` | f32 | dB, -64 = none; probably the reverb send | -64 on music (32) and the ambient loops (33 to 35), -3 to -9 on weapons, impacts, footsteps; music is dry in the game |
| `0x20`, `0x30` | f32 | dB, mostly 0 or -64 | not used |
| `0x34` | 4 x f32 | default distances | `weapon-sounds.md` 7 |
| **`0x44`, `0x48`** | f32 x2 | **gain range, dB (lower, upper)** | 0 / 0 on music (32); -3 / -3 on dialogue (19 unit, 47 mission, 48 cinematic) and cinematic music (38); -12 / -12 on weapon fire (4), reloads, ambient loops and sound scenery (33 to 35, 37 upper); -15 on vehicle engines (23), ready sounds, flybys; -10 footsteps (18); ranges only on physics impacts (13: -30 / -12), particles (14: -22 / -14), crashes (22: -24 / -12) |
| `0x4C`, `0x50`..`0x58` | f32, 3 x f32 | a ducking level, dB, with fade-in / sustain / fade-out times (0.5 s): probably while cutscenes play | -6 on effects and ambience, **0 on music** |
| `0x5C`, `0x60`, `0x68` | f32 x3 | a second ducking, taken as **under scripted dialogue**, with fade down / up times (0.5 s); applied since 2026-10-31 (section 11) | -6 effects and unit dialogue (19), -3 music, 0 ambience and the mission dialogue classes (46 to 48) |
| `0x6C`, `0x70`.. | f32, times | a third ducking | -12 effects, -3 music |
| `0x7C`, `0x80` | f32 | 15 and 1 on most classes | not used |

So in Halo 3 the music is mixed at 0 dB and nearly everything else 12 to 15 dB below it (dialogue
3 dB below). The bank's samples are normalised alike (the Warthog run's loops are -14 to -17 dBFS
RMS, an assault rifle shot -16, a falling-plate loop -7), so without the class gains our effects
and ambience played 9 to 12 dB too loud against the music: by the plates on the run, about 8 dB
over the music (user report 2026-10-30). `SoundClass::gain_bounds_db`; the viewer adds the upper
end to every sound's gain (`SoundInfo::gain_db`). Pinned in `blam_sound/tests/install.rs`
(`halo_class_gains`).

The sound mix tag (`snmx`, `sound\sound_mix`, one per map) is not decoded: a few dB values
(`0x04` -2, `0x10` -2, `0x24`..`0x34` -6 / -9, `0x40` -10, `0x48` 4) and a `snd!` reference at
`0x94`; probably the first-person and speaker mix.

**Our mix on top** (`mcc_audio`): music at 0 dB class gain as tagged. Sound scenery a further
-9 dB and weapons +6 dB over their class (`SCENERY_GAIN`, `WEAPON_GAIN`: our choices, see the
code). Every voice is on a bus (music: classes 32, 38; dialogue: 19, 46 to 48; ambience:
backgrounds, details, sound scenery, classes 33 and 34; effects: the rest, weapons, board sounds)
whose volume is the player's slider. Measured headless on the Warthog run (`MCC_AUDIO_TRACE=1`,
`audio levels` lines, RMS before the master volume): music -18 dBFS, ambience next to two plates
-25 dBFS (before: about -11).

## 10. Not decoded

The dialogue's sound effects (`cortana_mission`, the radio filters; section 11); AI combat dialogue (`udlg`); `lsnd` layers (`sound_looping_activate_layer`, unused on 120_halo) and the header
flag bits 3-6; pitch ranges past the first; `snde` reverb; cone gains; the classes' ducking and
the mix (`snmx`) (section 9; the classes' limits and default distances, and a reading of all four
distances, are in `weapon-sounds.md` 7); the per-sound `ugh! + 0xBC`
block; the 8 DLL setup rows that do not rebuild (unused by the bank).

## 11. Dialogue: the language banks and the mission dialogue tag

Probed 2026-10-31 on the user's install (`probe_fsb5.py <halo3 dir> english header|stats|info
N|grep TEXT`, `probe_dialogue.py`); no GPL code or plugin XML was used.

### 11.1 The banks

`halo3\fmod\pc\<language>.fsb` for `english`, `french`, `german`, `italian`, `mexican`,
`spanish`, each with its `.fsb.info`. They are laid out exactly like `sfx.fsb` (sections 1 to 3):
FSB5 version 1, Vorbis, 32,984 samples, all 48 kHz, 32,965 mono and 19 stereo, only the two
setup CRCs `sfx.fsb` uses (`0x632B1FAC`, `0xAD11D38D`), no intra layers, no loop chunks, and
280-byte `.info` records. So the existing reader decodes them with no change: `Library::add_dialog`
opens one beside `sfx.fsb` and `Library::find` looks an id up in the sfx bank, then the language
bank.

| fact | value | how checked |
|---|---|---|
| sample ids | the same 32,984 ids, in the same order, in all six banks | `.info` id columns compared |
| clash with `sfx.fsb` | none in any bank | id sets intersected |
| paths | `data\sound\...` in English; the other banks' localised takes are under `data_fr\`, `data_de\`, `data_it\`, `data_mx\`, `data_sp\` (about 29,770 each; the rest, about 3,210, are the English `data\` takes, kept where nothing was recorded) | prefixes counted |
| what the id is | for 29,873 records in every bank, djb2 of the path under `data*\sound\` without `.aif` (section 2), so the language prefix does not change it; for the other 3,111 (the campaign levels' own lines, e.g. `120mg_030_cor` = `0x87AAD115`, whose path hashes to `0xC88DCF5D`) it is not that hash, and we have not worked out what it is. Not needed: the `snd!` stores the id | hashed every path |
| what a `snd!` stores | the bank id (`snd! 120mg_030_cor` permutation `+0x10` = `0x87AAD115`, English record 32,776) | `blam sound 120_halo show 8979` |
| contents | `data\sound\dialog\...` for all but 31 English records (a few HUD and radio sounds: `visual_fx\cortana_hud_on`, `dsp_effects\radio_static`) | paths counted |
| length | the take's own: `120mg_030_cor` is 102,400 frames in English, 102,358 in French | `.info` PCM bytes |
| 6 "channel" records | 33 English records say 6 channels in the `.info`; the bank sample is mono and `PCM bytes / (6 x 2)` is its frame count | `info 25589` |

So a line's length in the player's language is `PCM bytes / (channels x 2) / 48000` s from the
`.info` alone (`blam_sound::info_seconds`). The `snd!`'s own longest-permutation field
(`+0x14`, 2,133 ms for `120mg_030_cor`) is the English length.

The language is the setup's `LANGUAGE` (`game_paths::language`; `MCC_LANGUAGE` overrides),
English when unset. Setup does not ask yet.

### 11.2 `mdlg`, mission dialogue

One per campaign map (`sound\dialog\120_halo\120_halo`), found by class. The scripts name its
lines, not tags: `(sleep (ai_play_line_on_object none 120mg_030))`.

| offset | type | what |
|---|---|---|
| `0x00` | block | **lines**, 20 bytes (120_halo: 213) |

**Line**, 20 bytes:

| offset | type | what | evidence |
|---|---|---|---|
| `0x00` | string id | name, `120mg_030` | the names the scripts use |
| `0x04` | block | **variants**, 24 bytes | 212 lines have one, `120lb_220` two (the Chief's and Guilty Spark's takes); 6 lines' variant has no `snd!` (cut lines: `120ma_020`, five of the Gravemind's), so 207 lines play |
| `0x10` | string id | the line's sound effect: `cortana_mission` (47 lines, all Cortana's), `radio_default` (16), `radio_3d` (6), `object` (1), none (143) | names; Cortana's run lines all `cortana_mission` |

**Variant**, 24 bytes:

| offset | type | what |
|---|---|---|
| `0x00` | string id | speaker code: `cor` Cortana (82), `jon` Johnson (44), `gsp` Guilty Spark (37), `grv` the Gravemind (30), `arb` the Arbiter (10), `mas` the Master Chief (7), `sma` a marine (4) |
| `0x04` | tag ref | the `snd!` (class 47, mission dialogue: -3 dB, section 9) |
| `0x14` | string id | a per-variant effect, `radio_default` (17) or `radio_3d` (4), else none |

The scenario does not seem to reference the `mdlg` in its first 4 KiB; it is read by class.

### 11.3 How the scripts use it, and what we do

* `ai_play_line_on_object OBJECT LINE`, `ai_play_line AI LINE`, `vs_play_line AI BOOL LINE` play
  the line's sound and **return its length in ticks**; the scripts sleep on that
  (`(sleep (ai_play_line_on_object none 120mg_030))`, or `(- ... 10.0)` to overlap). With
  `none` the line has no speaker in the world: Cortana in the Chief's head, Johnson on the radio.
  We play it at the listener; at the object when one is named; `ai_play_line` lines at the
  listener (no actor objects yet).
* `sound_impulse_start` names a dialogue `snd!` directly in some scripts (Guilty Spark's boss
  lines, `120me_*_gsp`), and `sound_impulse_language_time SND` returns its length in ticks, in the
  language being played.
* The Warthog run's lines are all Cortana's, from `md_09_cortana_hints` (`120mg_010` to `090`,
  one every 30 s once the player is on the run) and `md_cortana_countdown` (`120mg_100` to `160`,
  the charge percentages, at `trench_run_110_trig09`, `trench_run_110_island_trig02`,
  `120_08_trench_encounter_120`, `trench_run_120_trig02`, `_trig10`, `_trig14`).
  `md_09_cortana_hints` first waits for the hallway before the run (`120_07_c_hall_end`), which
  `ins_trench_run` puts the player past; our `trench_run` insertion counts that volume as passed
  (`setup::PASSED_ON_THE_WAY`). Pinned by `mcc_script/tests/trench_dialogue.rs`.
* Levels: the class's -3 dB plus the sound's playback gain (-3 dB on most of Cortana's run
  lines, -5 on `120mg_010`), on the dialogue bus (the pause menu's Dialogue slider). A line
  decodes at about -16 dBFS RMS (`120mg_030_cor`), so it plays near -22 dBFS against the run's
  music at about -18 to -21. While a line plays the other buses duck by the classes' `+0x5C`
  (section 9): music -3 dB (class 32), effects -6 dB (class 4), ambience 0 (class 33), fading
  over 0.5 s. Measured headless on `warthog_run` (`MCC_AUDIO_TRACE=1`, riding from just before
  the first volume): Cortana about -22 to -25 dBFS, the music -18.6 dBFS, -19.6 over a window
  that was mostly ducked. Not compared with the real game.
* Not done: the effects (`cortana_mission`, the radio filters, `radio_3d`); AI combat dialogue
  (`udlg`, `ai_dialogue_enable`: the AI has no vocalisation system yet); `ai_play_line` lines
  are not placed at the actor.
