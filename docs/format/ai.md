# AI: squads, starting locations, AI zones, characters and units (MCC Halo 3, U13)

What a campaign scenario says about its enemies and allies, and the tags behind them. Written
2026-10-29 from our own probes of the user's `120_halo.map` (Halo, the last mission),
cross-checked on `030_outskirts.map` (same strides and offsets, marines and brutes in the right
places). No GPL code or plugin XML was used: the blocks were found by walking the scenario's block
headers (`scripts/probe_scnr_blocks.py`), the element sizes from the distance between consecutive
elements' child blocks, and the fields by comparing the same slot across all 262 squads, 267 cells
and 872 starting locations of 120_halo (`scripts/probe_ai.py survey`) and checking values whose
meaning is plain (names, positions on the ground, indices that land on the right palette entry: the
squad `343_spark` places palette character 0 `monitor` holding palette weapon 17 `monitor_beam`,
`johnson` places `marine_johnson_halo` with the Spartan laser, the `sentinel_*` squads place
`sentinel_aggressor`). Field names are ours; "inferred" marks a name that rests on the values only.

Code: `crates/blam_geometry/src/ai.rs` (reader), `crates/blam_cli/src/ai.rs` (`blam ai <map>
[--squads] [--zones] [--characters]`), `crates/blam_geometry/tests/ai.rs` (pinned counts), the game
side `crates/halo3_ai`. Probe: `python -I docs/format/scripts/probe_ai.py <map>
[summary|squads|survey|chars|zones]`.

Units: world units (wu, 1 wu = 3.048 m), radians, seconds; +Z up.

## 1. Scenario blocks (offsets from the start of the `scnr` tag data)

| offset | element | count on 120_halo | what |
|---|---|---|---|
| `0xF0` | 16 | 7 | vehicle palette (tag references; also read by `scenario.rs`) |
| `0x108` | 16 | 4 | equipment palette |
| `0x120` | 16 | 19 | weapon palette: what cells and starting locations hand their actors |
| `0x378` | 40 | 19 | **squad groups** (section 2) |
| `0x384` | 64 | 262 | **squads** (section 3) |
| `0x390` | 64 | 11 | **AI zones** (section 6) |
| `0x3A8` | 16 | 15 | **character palette**: references to `char` tags |

030_outskirts: 60 groups, 171 squads, 187 cells, 500 starting locations, 18 zones (7,949 firing
positions), 15 characters, at the same offsets.

## 2. Squad group, 40 bytes

| offset | type | meaning | evidence |
|---|---|---|---|
| `0x00` | char[32] | name | `all`, `valley_flood`, `sentinels`, `trench_110` |
| `0x20` | i16 | **parent group** (-1 none) | `sentinel_left` and `sentinel_right` name 7 `sentinels`; `valley_flood` names 1 `all` |
| `0x22` | i16 | 0 | |
| `0x24` | i16 | objective (inferred), -1 on every 120_halo group | |

Scripts address a group as one (`ai_place sentinels`): every squad whose group is it or a group under
it (`AiData::squads_in_group`).

## 3. Squad, 64 bytes

| offset | type | meaning | evidence |
|---|---|---|---|
| `0x00` | char[32] | name | `343_spark`, `flood_exterior01`, `trench_sentinel_110_1` |
| `0x20` | u32 | flags, 0 on every 120_halo squad | |
| `0x24` | i16 | team; 0 on every squad = the unit's own team (section 8) | |
| `0x26` | i16 | **squad group** | `flood_exterior*` -> 18 `combat_reinforcements`, `trench_sentinel_110_*` -> 15 `trench_110` |
| `0x28` | i16 | **AI zone** | `trench_*_100` -> zone 7 `trench_run_100`, `*_110` -> 5 `trench_run_110`, `*_120` -> 8 `trench_run_120`; every zone's BSP matches its squads' locations |
| `0x2A` | i16 | 0 | |
| `0x2C` | i16 | objective (inferred) | 1..10 or -1 |
| `0x2E` | i16 | objective task (inferred) | -1 but once (28) |
| `0x30` | block | **cells**, 96 bytes each (section 4) | one per squad, a few with two |
| `0x3C` | i16 | editor folder (inferred) | 22, 40, 42, 44, 46, 49 or -1 |

The cell's stride was found from a squad with one cell and one location: the next squad's cell
starts 232 bytes after this one's location, which is 136 + 96.

## 4. Cell, 96 bytes

A cell is one kind of actor in a squad: how many, carrying what, and where they start.

| offset | type | meaning | evidence |
|---|---|---|---|
| `0x00` | u32 | not decoded: 0, or 12 / 15 on the finale "boss" squads | |
| `0x04` | i16 | **count**: actors placed | 1 .. 13; `trench_sentinel_100_1` 8 for 8 locations |
| `0x06` | i16 | major upgrade (inferred: 0 normal, 3 none, 4 all) | |
| `0x08` | i16 | **character** (palette index) | `343_spark` 0 `monitor`, `johnson` 14, `arbiter` 10 `dervish`, `sentinel_*` 8 |
| `0x0A` | i16 | **initial weapon** (weapon palette) | `343_spark` 17 `monitor_beam`, `johnson` 0 `spartan_laser`, `combat_01` (a brute combat form) 11 `spike_rifle`; -1 for infection forms and sentinels |
| `0x0C` | i16 | secondary weapon | `arbiter` 2 (plasma rifle) |
| `0x0E` | i16 | grenade type (inferred, 0 .. 3) | |
| `0x10` | i16 | equipment (palette) | -1, 0, 1 |
| `0x12` | i16 | **vehicle** (palette) | 0 warthog, 5 chopper on the `warthog*` / `chopper*` squads |
| `0x18` | char[32] | placement script name, empty for none | 9 cells; the i16 at `0x38` is then 540 .. 574 (the script's index, inferred) |
| `0x54` | block | **starting locations**, 136 bytes each (section 5) | |

## 5. Starting location, 136 bytes

| offset | type | meaning | evidence |
|---|---|---|---|
| `0x00` | u32 | 0 | |
| `0x04` | string id | name | `starting_locations_0`, or `01`, `02` |
| `0x08` | 3 x f32 | **position**, feet | on the ground (or in the air for sentinels) |
| `0x14` | i16 | reference frame (inferred), -1 or 3 .. 5 | |
| `0x16` | i16 | **BSP** it lies in | 2 (`120_bsp_020`) in the ziggurat, 7 / 8 / 9 in the trench; **-1 on all the trench run's flood and carrier squads**, which then go by their AI zone's BSP (`AiData::squad_bsps`) |
| `0x18` | 3 x f32 | **facing**: yaw, pitch, roll | `johnson` -1.54 |
| `0x24` | u32 | flags (inferred); 4 once | |
| `0x28` | i16 | **character override** | the finale's `*_boss` squads: 4 and 3 (combat forms) |
| `0x2A` | i16 | **weapon override** | the boss forms: 13 `flak_cannon` (fuel rod), 1 `rocket_launcher` |
| `0x36` | i16 | swarm count (inferred), 0 or 3 .. 9 | on infection squads |
| `0x40` | f32 | 3 .. 50 on 58 locations, not decoded | |
| `0x4C` | char[32] | placement script, as on the cell | |

## 6. AI zone, 64 bytes; firing position, 40; area, 108

| offset | type | meaning | evidence |
|---|---|---|---|
| `0x00` | char[32] | name | `control_room`, `ziggurat`, `ziggurat_air`, `trench_run_110` |
| `0x20` | u32 | flags (1 on every zone) | |
| `0x24` | i16 | **BSP** | `trench_run_100` 7, `_110` 8, `_120` 9, `ziggurat` 2 |
| `0x28` | block | **firing positions**, 40 bytes | 47, 1,004, 374, ... (3,842 in all); stride 40 between zones' blocks |
| `0x34` | block | **areas**, 108 bytes | 5, 81, 23, ...; stride 108 |

Firing position:

| offset | type | meaning |
|---|---|---|
| `0x00` | 3 x f32 | position (on the ground) |
| `0x0C` | i16 | reference frame (-1) |
| `0x0E` | i16 | BSP (-1 in `control_room`, whose positions are not compiled) |
| `0x10` | u16 | flags (65, 66, ...; not decoded) |
| `0x16` | i16 | area (within the zone) |
| `0x18` | i16 | cluster of the BSP |
| `0x1C` | 2 x f32 | ground normal as yaw and pitch (pitch pi/2 = flat) |

Area: `0x00` char[32] name (`areas_0`), `0x24` 3 x f32 centre, `0x34` f32 radius, `0x38` u16 first
firing position, `0x3A` u16 count (area 0 of `ziggurat` covers positions 0 .. 13, area 1 13 .. 28).

## 7. Character (`char`)

A character names its parent and only fills the blocks it changes; Halo takes each block from the
nearest character in the chain that has it. `flood_combat_human` <- `floodcombat_base` <-
`ai\generic`: the human form's own tag only names its unit, style, variants and dialogue. The root
(offsets from the start of the tag):

| offset | type | meaning |
|---|---|---|
| `0x04` | tag ref | **parent character** |
| `0x14` | tag ref | **unit** (`bipd`) |
| `0x24` | tag ref | **creature** (`crea`): the infection form has this instead of a unit |
| `0x34` | tag ref | style (`styl`), not read |
| `0x54`, `0x60` | blocks | variants, unit dialogue, not read |
| `0x6C` | block, 28 | **general**: `+0x04` i16 actor type: 9 flood combat, 10 infection, 11 carrier, 12 monitor, 13 sentinel, 20 stalker, 21 tank, 22 ranged pure form, 7 marine, 0 elite |
| `0x78` | block, 128 | **vitality**: `+0x04` body, `+0x08` shield (normal), `+0x0C` body, `+0x10` shield (legendary); flood combat 40 / 0 (80 / 0), stalker 150, tank 100, sentinel 25, Johnson 200, the Arbiter 200 / 160 |
| `0x84` | block, 52 | placement, not read |
| `0x90` | block, 44 | **perception** (the first element; `ai\generic` has 4): `+0x04` max vision distance (30), `+0x08` central vision angle (20 deg), `+0x0C` max vision angle (50 deg), `+0x10` peripheral vision angle (105 deg), `+0x14` peripheral distance (4), `+0x18` hearing distance (40; 50 on combat forms) |
| `0x9C` | block, 80 | look, not read |
| `0xA8` | block, 44 | **movement**: `+0x04` pathfinding radius (0.2 .. 0.3) |
| `0xE4` | block, 124 | **charge** (melee): `+0x04` melee consider range, `+0x08` melee chance, `+0x0C` melee attack range, `+0x10` abort range, `+0x14` attack timeout, `+0x18` attack delay, `+0x1C` leap range (min, max), `+0x24` leap chance. Combat forms 14, 0.8, 1.2, 18, 7, 1.5, (4, 14), 0.35: they charge from far; stalker 5, 1, 1.4 |
| `0x174` | block, 224 | **weapons**: how the character uses each weapon (section 7.1) |
| `0x180` | block, 28 | **firing patterns** per weapon (section 7.2) |
| `0x18C` | block, 60 | grenades, not read |
| `0x198` | block | vehicles, not read |

The other blocks between `0x9C` and `0x174` (ready, engage, evasion, cover, retreat, search, idle,
vocalisation, boarding) hold behaviour timings not used yet.

### 7.1 Character weapon, 224 bytes

| offset | type | meaning | combat form, spike rifle |
|---|---|---|---|
| `0x04` | tag ref | weapon (element 0 of `ai\generic`'s block is null) | |
| `0x14` | f32 | maximum firing range | 14 |
| `0x18` | f32 | minimum firing range | 0.5 |
| `0x1C` | 2 x f32 | **normal combat range** (min, max): where it likes to stand | 2.5, 7 |
| `0x2C` | 2 x f32 | timid combat range (inferred) | 4, 9 |
| `0x34` | 2 x f32 | aggressive combat range (inferred) | 2, 5 |
| `0x50` | 2 x f32 | **first burst delay** (min, max) | 0.1, 0.8 |
| `0x58`..`0x64` | 4 x f32 | surprise delay, surprise fire-wildly chance, death fire-wildly chance and time (inferred) | 1, 0.5, 0.2, 0.8 |

### 7.2 Firing patterns: 28 bytes per weapon, then 64 per difficulty

The 28-byte element is a weapon reference at `+0x00` and a block at `+0x10` of 64-byte patterns:
two on every weapon, the second firing about twice as fast (8 / s against 4 for the combat form's
spike rifle), so read as normal and legendary (inferred).

| offset | type | meaning | combat form, spike rifle (normal) |
|---|---|---|---|
| `0x00` | f32 | **rate of fire**, shots / s (0: the weapon's own) | 4 |
| `0x04` | f32 | target tracking, 0..1 | 0.5 |
| `0x08` | f32 | target leading, 0..1 | 0.25 |
| `0x0C` | f32 | burst origin radius (wu) | 0.15 |
| `0x10` | f32 | burst origin angle | 20 deg |
| `0x14` | 2 x f32 | burst return length (inferred) | 0.2, 0.3 |
| `0x1C` | f32 | burst return angle (inferred) | 20 deg |
| `0x20` | 2 x f32 | **burst duration** (min, max) | 0.6, 1.6 |
| `0x28` | 2 x f32 | **burst separation** (min, max) | 1.4, 2.8 |
| `0x30` | f32 | weapon damage modifier (inferred; 0) | |
| `0x34` | f32 | **projectile error** | 0.75 deg |
| `0x38` | f32 | burst angular velocity (inferred; 0) | |
| `0x3C` | f32 | **maximum error angle** | 2 deg |

## 8. Units (`bipd`, `crea`) and their models

On top of `movement.md` 3 (the biped fields) and `scenario-and-tag-reading.md` 3 (the model chain):

| offset | type | meaning | evidence |
|---|---|---|---|
| `0xFC` | i16 | **default team**, in units and creatures alike: 1 player, 2 human, 3 covenant, 4 flood, 5 sentinel, 8 guilty (343 Guilty Spark) | Chief 1, marine 2, elite and Arbiter 3, every Flood biped and the infection creature 4, sentinel 5, monitor 8 |
| `0x1A8` | tag ref | biped **melee damage** (`jpt!`) | `floodcombat_human_charge_first_melee`: damage 10 .. 25 |
| `0x24C` | block, 16 | the unit's **own weapons** | sentinel: `sentinel_gun`; ranged pure form `flood_ranged_weapon`; monitor `monitor_beam` |
| `0x33C` | u32 | character physics flags: **bit 0 set only on the two flyers** (monitor, sentinel: `0x13`), read as "flies" (inferred) | stalker `0x82`, the rest 0 |
| `0x340`..`0x348` | 3 x f32 | height standing, crouching, radius (`movement.md`) | combat form 0.6 / 0.45 / 0.18, tank 0.85 / 0.4, sentinel 0.7 / 0.35 |
| crea `0x1C4` | tag ref | the creature's **bite** (`jpt!`) | `infection_form_bite` |

The creature has no character physics; its capsule is taken from its model's bounds.

**Model damage info.** The model (`hlmt`) block at `0x88` holds the damage info (one element):
`+0x0C` f32 **maximum body vitality**, `+0x20` f32 **maximum shield vitality**. The campaign Chief
reads 45 and 70, the figures players know; `+0x2C` 5 and `+0x30` 2 read as the shield recharge delay
and time (inferred). For AI bipeds the character's vitality is what the game uses (a combat form's
model says 100, its character 40, and combat forms die to about six rifle rounds); for creatures the
model's (the infection form's says 1: they pop at a touch; its character inherits `ai\generic`'s 50).

**Difficulty scales (globals).** The globals (`matg`) block at `0x410` holds one element of 0x230
bytes: 35 rows of four floats, one per difficulty (easy, normal, heroic, legendary). On 120_halo
the first row is **0.1, 0.775, 1.0, 1.6**, which by its shape and its place (Halo CE's difficulty
block starts with "enemy damage") reads as the scale on the damage enemies deal to the player;
`halo3_ai` applies it (`blam_geometry::ai::enemy_damage_scales`). The other rows (for example
`+0x30` 1.4 / 1.0 / 0.7 / 0.3, `+0x130` 0.1 / 0.5 / 0.75 / 1.0, `+0x150` 4 / 2 / 1 / 0.25) are not
named or used.

## 9. 120_halo, by zone set

Squads whose starting locations (or else AI zone) lie in a zone set's BSPs, with their actors by
character (`blam ai 120_halo`):

| zone set | squads | actors |
|---|---|---|
| 0 `120_00_01_02` (the ziggurat) | 174 | 50 elite, 48 human and 39 brute combat forms, 39 infection forms, 23 stalkers, 2 tanks, 6 sentinels, the Arbiter |
| 1 `120_01_02_03` | 177 | the same, plus the ziggurat's way out |
| 2 `120_03_05` (control room) | 8 | 343 Guilty Spark (as `monitor` and `monitor_combat`), Johnson, 6 elite combat forms, 4 infection forms |
| 3 `120_07_06_04_03` | 15 | combat forms, a carrier, infection forms, 8 sentinels, Johnson |
| 4 `120_04_100_110` (the Warthog run starts) | 52 | 78 sentinels, combat forms, 12 carriers, 46 infection forms, 17 stalkers; 7 Warthog / chopper crews (no character) |
| 5 `120_100_110` | 50 | the same less the ziggurat end |
| 6 `120_110_120` | 44 | 102 sentinels, 34 elite combat forms, ... |
| 7 `120_120_130` (the end) | 9 | 45 sentinels, 14 elite combat forms, the Arbiter |

Counts are the cells' counts, so a script that places a squad twice places more. The finale squads
(`ziggurat_finale_*`, 48 of them, one actor each) and the `secret_egg*` / `test` squads are in there
too; the game places almost all of these by script (`ai_place`).

## 10. Not read or not settled

1. Squad and cell flags, the cell's first u32 (12 / 15 on boss squads), objectives and tasks, the
   location's `0x40` float and its block at `0x7C` (the trench sentinels: 2 or 3 entries).
2. "Major upgrade", grenade type, swarm count and the placement script index are inferred from their
   values only.
3. Which perception element applies when (`ai\generic` has four, the pure forms one).
4. Speeds: Halo moves actors by their animations' root motion (`jmad`), not read; `halo3_ai` uses its
   own per-type speeds until the animation code supplies them.
5. The damage effects' head-shot and material responses: `halo3_ai` doubles a head hit on an
   unshielded non-Flood actor as a stand-in.
