# Zone sets, starting locations, trigger volumes, skies per BSP (MCC Halo 3, U13)

What a campaign scenario says about which structure BSPs are loaded together, where the player
starts and which sky is seen where. Written 2026-10-28 from our own probes of the user's
`120_halo.map` (Halo, the last mission), cross-checked on `010_jungle`, `030_outskirts`,
`100_citadel` and `riverworld`. No GPL code or plugin XML was used: the offsets were found by
walking the scenario's block headers (`docs/format/scripts/probe_scnr_blocks.py`) and matching
contents against things whose meaning is obvious (zone set names that list their BSPs, positions
that land on the ground, string ids that name zone sets).

Code: `crates/blam_geometry/src/zones.rs` (reader and tests), `sbsp.rs` (cluster sky index),
`scenario.rs` (object origin BSP, sky masks), viewer `crates/halo3/src/world/zones.rs`.

Probe: `python -I docs/format/scripts/probe_scnr_blocks.py <map> [scan length hex] [mode]`, mode
`OFF:STRIDE:N` dumps N elements of the block at scnr + OFF, `origin:OFF:STRIDE` groups a
placement block by origin BSP, `clusters` lists every BSP's cluster bytes.

## 1. Scenario blocks (offsets from the start of the `scnr` tag data)

Blocks are the usual 12-byte headers (`container.md`). Element sizes are the distance between
consecutive blocks' elements divided by the count, exact on every map checked.

| offset | element | count on 120_halo | what |
|---|---|---|---|
| `0x18` | 108 | 11 | structure BSPs (`scenario-and-tag-reading.md` 2.2). **A BSP's index here is its bit in every mask below** |
| `0x34` | 20 | 4 | skies (section 3) |
| `0x40` | ~1904 | 8 | one per zone set: probably the zone set's PVS (big, not decoded) |
| `0x4C` | 100 | 8 | one per zone set, holds data references (audibility?; not decoded) |
| `0x58` | 40 | 8 | **zone sets** (section 2) |
| `0x64` | 12 | 12 | BSP zones: string id `120_bsp_000` ..., u32 mask of the BSP itself, u32 second mask (neighbours?; not used) |
| `0x24C` | 24 | 4 | **player starting locations** (section 4) |
| `0x258` | 68 | 254 | **trigger volumes** (section 5) |
| `0x270` | 8 | 17 | **zone-set switch trigger volumes** (section 5) |

## 2. Zone set, 40 bytes

| offset | type | meaning |
|---|---|---|
| `0x00` | string id | name, e.g. `120_00_01_02` |
| `0x04` | i32 | an index (a permutation of 0..n-1 on 120_halo; PVS slot?) |
| `0x08` | u32 | flags (bit 0 set on the last zone sets of 120_halo and 030_outskirts) |
| `0x0C` | u32 | **BSP mask**: bit n set = structure BSP n is loaded |
| `0x10..0x27` | | more masks and indices (design zones, cinematic zones?); not used |

The names spell the BSPs out, which pins the mask: on 120_halo the BSP block is `000 010 020 030
040 050 060 100 110 120 070` (indices 0..10), and

| # | name | mask | BSPs |
|---|---|---|---|
| 0 | `120_00_01_02` | `0x007` | 000 010 020 |
| 1 | `120_01_02_03` | `0x00E` | 010 020 030 |
| 2 | `120_03_05` | `0x028` | 030 050 |
| 3 | `120_07_06_04_03` | `0x458` | 030 040 060 070 |
| 4 | `120_04_100_110` | `0x190` | 040 100 110 |
| 5 | `120_100_110` | `0x180` | 100 110 |
| 6 | `120_110_120` | `0x300` | 110 120 |
| 7 | `120_120_130` | `0x201` | 000 120 |

So the Warthog run is BSPs 100 -> 110 -> 120 (zone sets 4-7). BSPs can stand for **different
versions of one space**: `120_bsp_010` and `120_bsp_070` have the same bounds and the same
20,888 collision triangles, and are never in one zone set; loading every BSP at once (the
viewer's Home key) draws both on top of each other. 010_jungle has 13 zone sets over 12 BSPs,
riverworld one (`mask 1`).

The game starts in zone set 0. The viewer loads the first zone set whose BSP bounds contain the
first starting location, which is zone set 0 on every map checked.

## 3. Skies per BSP and per cluster

**Sky block entry (20 bytes)**: `0x00` tag reference (`scen`), `0x10` i16 object name index,
`0x12` u16 **mask of the BSPs the sky is active in**. 120_halo: `120_halo_sky` `0x0025` (000 020
050), `sky_01` `0x0001` (000), `sky_02` `0x0000` (nowhere; its model has no geometry resource),
`sky_03` `0x03C0` (060 100 110 120, the Warthog run's sky). 010_jungle: `0x07FF` and `0x0800`
(the intro BSP 11 has its own). riverworld: `0x0001`.

**Cluster (280 bytes, sbsp + 0xC4)**: `0x00` bounds as three f32 pairs (x min max, y, z;
`FLT_MAX` in all six when the cluster has none), `0x18` **i8 sky index** into the scenario's sky
block, -1 indoors. Every non-negative value seen is a sky whose mask includes that BSP (120_halo:
000's clusters say 0 or 1, 020 and 050 say 0, 060/100/110/120 say 3, the interiors -1).
riverworld: only its big outdoor cluster says 0. `0x19` **i8 index into the BSP's atmosphere
palette**, -1 = none (every riverworld cluster).

**Atmosphere palette (sbsp + 0x94, 8 bytes)**: `0x00` string id name, `0x04` i16 **index into the
`skya` tag's settings** (`sky-and-atmosphere.md` 3), `0x06` pad. Corrected 2026-10-28: the cluster
byte was first read as a `skya` index directly, which put the Warthog run in `interior` (thin,
white Rayleigh air: the washed-out run). Every 120_halo BSP has the same seven entries
`snow_light` 1, `snow_heavy` 2, `haze` 0, `interior` 4, `trench` 5, `zigg_activate` 6, `space` 7
(palette name, setting index; the skya's eight settings are `haze_used_for_halo_zig_sky`,
`snow_light`, `snow_heavy`, `haze`, `interior`, `trench`, `zigg_activated`, `space`). 010_jungle's
palette `misty_fog` 0, `light_fog_and_haze` 1, `light_haze` 2, `fog_grunt_camp` 3, `fog_night` 5,
`no_fog` 6 skips its setting 4 `sky_fog` and names 5 `sky_fog_night`: the index, not the
position, picks the setting, and the names agree with the settings they point at. riverworld's
palette is empty. Through the palette, 120_halo's clusters say: the crash site (000) `snow_heavy`
and two `space`, 010 `snow_light` and `interior`, 020 `snow_light`, 030 `snow_light` and
`interior`, 040 and 050 `interior`, 060 `zigg_activated`, 070 `interior` and `zigg_activated`, and
the Warthog run (100/110/120) **`trench`**: thin air with a dim red sun (0.22, 0.06, 0.04 x 0.5),
which gives the run its dark red distance and leaves the ring sky clear. Probe:
`probe_sbsp_portals.py <map> air 0`; test `halo_cluster_atmospheres_go_through_the_palette`.

Viewer rule: the sky and the haze of the loaded cluster the camera is in (found through the
collision tree, `sbsp-portals.md` 3; else the smallest cluster box around it); indoors (no
sky) the last sky stays, outside every cluster box both stay; at load time the sky most loaded
clusters name, and the old rule's world setting (`sky-and-atmosphere.md` 3.3) until the camera
is in a cluster that names one.

## 4. Player starting location, 24 bytes

| offset | type | meaning |
|---|---|---|
| `0x00` | 3 x f32 | position, wu (feet on the ground) |
| `0x0C` | f32 | facing yaw, radians from +X about +Z |
| `0x10` | f32 | pitch, radians (tiny, -0.0046 on 010_jungle) |
| `0x14` | i16 | 0 on every entry seen |
| `0x16` | i16 | 0, 1, 4, 4 on the four co-op entries of 120_halo / 010_jungle / 100_citadel (player slots?) |

Campaign maps list four, the co-op players standing side by side; the viewer and `--skate` use
the first. 120_halo: (-263.64, -433.77, -5.05), yaw 0. 010_jungle: (-9.14, 175.69, 4.76), yaw
-2.27. riverworld has one at (78.1, -56.8, 1.56), but multiplayer maps start on their respawn
points (`Scene::spawns`).

## 5. Trigger volumes and zone-set switches

**Trigger volume, 68 bytes**:

| offset | type | meaning |
|---|---|---|
| `0x00` | string id | name, e.g. `120_03_valley_floor_start`, `begin_zone_set:120_01_02_03`, `zone_set:120_04_100_110:*` |
| `0x04` | i16 | object name index (-1) |
| `0x06` | i16 | -1 |
| `0x08` | string id | node name (0) |
| `0x0C` | 3 x f32 | forward (1, 0, 0 on most) |
| `0x18` | 3 x f32 | up (0, 0, 1) |
| `0x24` | 3 x f32 | position: a corner of the box, wu |
| `0x30` | 3 x f32 | extents along forward, left, up, wu |
| `0x3C` | f32 | bounding radius: half the box diagonal (21.9 for extents 3.95, 34.2, 27.1; checked on several) |
| `0x40` | i16 | -1 |
| `0x42` | i16 | 16 or 17 (flags?) |

Read as: the box spans `position + a*forward + b*(up x forward) + c*up` for `0 <= (a, b, c) <=
extents`; the radius at `0x3C` being half the diagonal supports a corner origin. Not yet checked
against the game's own containment test.

**Kill volumes.** 120_halo has about 30 volumes named `kill`, `kill_100_01` ... `kill_120_05`,
`ziggurat_kill_player01`: big boxes under the Warthog run and the crash site. The rail finder
(`rails::outside_kill_volumes`) cuts rails inside them: on the Warthog run zone sets that drops
31,500 rails to 6,600 (most were ledges of the structure far under the track).

**Zone-set switch trigger volume, 8 bytes** (four i16):

| offset | meaning |
|---|---|
| `0x00` | flags (0 or 1) |
| `0x02` | zone set to begin loading (prepare), -1 none |
| `0x04` | trigger volume index |
| `0x06` | zone set to commit (switch to), -1 none |

Pinned by the names: switch 0 = (0, begin 1, volume 1 `begin_zone_set:120_01_02_03`, -1);
switch 2 = (1, begin 4, volume 3 `begin_zone_set:120_04_100_110:*`, -1); switch 1 = (1, -1,
volume 2 `zone_set:120_04_100_110:*`, commit 4). So the game prepares the next zone set when the
player enters a `begin_zone_set:` volume and switches when they enter the `zone_set:` one.
Decoded into `Scene::trigger_volumes` and `Scene::zone_switches`; the viewer does not act on them
yet (zone-set streaming is a later step).

## 6. Objects and their BSP

Scenery, machine, control and crate placements carry an object id at `+0x38`: u32 unique id,
then **i16 origin BSP at `+0x3C`**, u8 object type (6 for scenery), u8 source. On 120_halo the
scenery whose origin is BSP n sits inside BSP n's bounds, and the origins follow the level
(000/010/020 near the start, 100/110/120 along the Warthog run). -1 (all crates, 29 scenery near
the start) = no BSP: the viewer shows those whatever is loaded.

## 7. Not decoded

The zone set PVS (`0x40`), the `0x4C` block, the rest of the zone set element, the BSP zones'
second mask, the `zone` resource gestalt's own per-zone-set resource
lists (which resources a zone set needs: everything is read from the map regardless).
