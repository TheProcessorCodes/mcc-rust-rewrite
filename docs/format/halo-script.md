# HaloScript in an MCC Halo 3 scenario (U13), and the Warthog run's floor

How the scripts a campaign map runs are stored in its scenario (`scnr`) tag, the tables they
name things in, the object data the Warthog run's falling floor uses, and how our runtime runs
them. Written 2026-10-29 from our own probes of the user's `120_halo.map` (Halo, the last
mission), cross-checked on `010_jungle` and `riverworld`. No GPL code or plugin XML was used:
offsets were found by walking the scenario's block headers (`scripts/probe_scnr_blocks.py`) and
matching contents against things whose meaning is plain (script names, function names that
make sense of their arguments, positions that land in the level).

Code: `crates/blam_script` (reader, decompiler, `blam scripts`), `crates/blam_geometry/src/objects.rs`
(object collision models and damage sections), `crates/halo3_script` (the runtime).

Probes (`python -I`, read-only, in `docs/format/scripts/`):

| probe | what it shows |
|---|---|
| `probe_scripts.py <map> [text]` | every script decompiled straight from the bytes; `--stats` counts node kinds, constant types and functions |
| `probe_script_tables.py <map>` | cutscene flags, point sets, editor folders, placement flags and folders |
| `probe_plate_damage.py <map> [scen path]` | an object's damage sections and responses, and its `coll` model |

Pinned by `cargo test`: `crates/blam_script/tests/maps.rs` (counts below),
`crates/blam_geometry/tests/objects.rs` (plate collision, damage sections, no BSP floor under the
plates), `crates/halo3_script/tests/warthog_run.rs` (the run's scripts dropping plates).

## 1. Where the scripts are (offsets from the start of the `scnr` tag data)

| offset | kind | element | 120_halo | what |
|---|---|---|---|---|
| `0x3D8` | data reference (20 bytes: size, 8 bytes, pointer, 4 bytes) | bytes | 80,186 | **script string data**: NUL-terminated tokens |
| `0x3EC` | block | 52 | 731 | **scripts** (section 2) |
| `0x3F8` | block | 40 | 113 | **globals** (section 2) |
| `0x404` | block | 16 | 69 | tag references the scripts use (plain 16-byte tag references) |
| `0x41C` | block | ? | 1 | "scripting data"; its first block (`+0x00`) is the **point sets** (section 5) |
| `0x428` | block | 28 | 86 | **cutscene flags** (section 5) |
| `0x49C` | block | 24 | 54,952 | **syntax datums**: the expression nodes (section 3) |
| `0x614` | block | 260 | 54 | **editor folders** (section 5) |

The string data ends two bytes before the expression table starts (both are laid out one after
the other in the tag data). The counts are pinned in `blam_script/tests/maps.rs`.

## 2. Scripts and globals

**Script, 52 bytes**

| offset | type | meaning |
|---|---|---|
| `0x00` | char[32] | name, NUL-padded |
| `0x20` | i16 | script type: 0 startup, 1 dormant, 2 continuous, 3 static, 4 command script, 5 stub |
| `0x22` | u16 | return type (value type, section 4) |
| `0x24` | u32 | root expression: a datum (salt << 16 \| index), always a `begin` call |
| `0x28` | block, 36 each | parameters: char[32] name, u16 value type, 2 bytes |

The type numbers are pinned by the names: `mission_halo` and `beginning_mission_segment` are
the only 0s and are the mission's entry points; the 2s are `kill_player01`, `kill_stragglers`
and other watchers that loop; the 1s are the encounters other scripts `wake`; the 3s include
`player0` (`(unit (list_get (players) 0))`, return type 66, unit) and `fall_off (obj_name 65)`.
120_halo has 2 startup, 18 continuous, 224 dormant, 467 static and 20 command scripts.

**Global, 40 bytes**: char[32] name, `0x20` u16 value type, 2 bytes, `0x24` u32 datum of the
initial value expression. E.g. `data_mine_mission_segment` type 9 (string), `perspective_running`
type 5 (boolean), `trench_speed` type 7 (short).

## 3. Syntax datums (expression nodes), 24 bytes

| offset | type | meaning |
|---|---|---|
| `0x00` | u16 | salt; **0 = free slot** (the rest of a free slot is filled with `0xBABA`) |
| `0x02` | u16 | opcode: the function's index for a call and for its function-name node; the script's index for a script call; else the value type again |
| `0x04` | u16 | value type (section 4) |
| `0x06` | u16 | flags (below) |
| `0x08` | u32 | next sibling datum, `0xFFFFFFFF` = none |
| `0x0C` | u32 | offset of the token's text in the string data |
| `0x10` | u32 | value (below) |
| `0x14` | u16 | source line |
| `0x16` | u16 | 0 |

Datums point at nodes as `salt << 16 | index`; every reference checked (all 50,390 used nodes on
120_halo) carries the target's own salt, so the index alone is enough.

**Flags** seen: 8, 9, 10, 13, 29, read as bits:

| bit | meaning | value field |
|---|---|---|
| 0 (1) | a primitive (not a call) | |
| 1 (2) | a script call (`opcode` = script index) | first child |
| 2 (4) | a global reference | global index; low 16 bits with bit 15 set = an engine global, named only by its text (`global_playtest_mode`, `editor`) |
| 3 (8) | set on every node | |
| 4 (16) | a parameter reference (always with 4) | parameter index in the enclosing static script |

So 8 = function call, 9 = constant, 10 = script call, 13 = global, 29 = parameter. 120_halo:
13,310 function calls, 2,377 script calls, 33,366 constants, 1,230 globals, 107 parameters,
4,562 free slots.

**A call's children**: the value field is the first child's datum; children chain through
`0x08`. The first child is always a constant of type 2 (function name) whose text is the
function's name (`begin`, `sleep`, `object_damage_damage_section`) and whose opcode equals the
call's. **The map carries no function table**: the engine's list lives in the game's code, so
the name comes from the head node's text and the opcode is only an index. 120_halo calls 281
distinct functions (`blam scripts 120_halo --functions`): e.g. 0 `begin`, 2 `if`, 4 `set`,
19 `sleep`, 21 `sleep_forever`, 22 `sleep_until`, 23 `wake`, 51 `volume_test_players`, 71
`object_create`, 80 `object_destroy`, 116 `object_damage_damage_section`, 289
`device_set_position`, 330/331 `ai_place` (two overloads: `(ai_place ai)` and `(ai_place ai
short)`). A call node's own text offset is not meaningful.

**`(if c a)`** compiles with a third child, a void constant: the decompiler drops it.

**Constants**: the value field holds the literal by type: boolean in the low byte, real as
f32, short in the low 16 bits (the high 16 bits are `0xFFFF`), long as i32; names (object,
trigger volume, cutscene flag, zone set, folder, starting profile, script) as an index in the
low 16 bits (-1 = none); AI, point references and tags as full 32-bit ids (sections 4, 5). The
text offset is kept for names, strings, enums (`easy`, `normal`) and tags; for numbers it points
at an unrelated token and is ignored.

## 4. Value types

The scenario does not name them; each name below is fixed by what the 120_halo data does with
the type (functions that take or return it, and what its constants index):

| n | name | evidence |
|---|---|---|
| 2 | function_name | the head of every call |
| 4 | void | `begin`'s and most statements' return type |
| 5 | boolean | `volume_test_players` returns it; values 0 / 1 |
| 6 | real | `object_get_shield`, `1.0`, `30.0` |
| 7 | short | `sleep`'s argument, `random_range` |
| 8 | long | `game_coop_player_count`, `current_zone_set` |
| 9 | string | `print`'s argument, `data_mine_set_mission_segment` |
| 10 | script | `wake`'s argument: the low 16 bits are the script index (`md_01_crash_site_cortana` = 74) |
| 11 | string_id | `object_damage_damage_section`'s section name (`fall`), `ai_set_objective` |
| 12 | unit_seat_mapping | `warthog_d`, `mongoose_p` |
| 13 | trigger_volume | `volume_test_players`; index into the trigger volumes (`end_trilogy03` = 116) |
| 14 | cutscene_flag | `object_teleport`; index into the cutscene flags (`cin_120lb_anchor` = 35, checked) |
| 16 | cutscene_title | `title_1`, `obj_0` |
| 19 | ai | `ai_place`; a 32-bit id (`warthog01/driver` = `0x805E0000`, `all` = `0x40000001`, `johnson` = `0x2000001F`; the top bits look like the kind: squad, starting location, ...; not decoded) |
| 21 | ai_command_script | `cs_run_command_script` |
| 24 | ai_line | `ai_play_line`; a string id (`120mc_010`) |
| 25 | starting_profile | `unit_add_equipment` (`teleport_profile`) |
| 27 | zone_set | `switch_zone_set`; index into the zone sets (`120_04_100_110` = 4) |
| 29 | point_reference | `effect_new_random`, `ai_teleport`; set << 16 \| point, point `0xFFFF` = the whole set (section 5) |
| 31 | object_list | `players`, `volume_test_objects`, `ai_actors` |
| 32 | folder | `object_create_folder`; index into the editor folders (`trench_run_100` = 1) |
| 33 / 34 / 35 / 36 | sound / effect / damage / looping_sound | tag constants: the value is the full tag id, the text the tag path |
| 37 / 39 | animation_graph / object_definition | tag constants (`scenery_animation_start`, `unit_has_weapon`) |
| 45 / 46 | cinematic_definition / cinematic_scene_definition | `cinematic_set`, `camera_set_cinematic_scene` |
| 50 | game_difficulty | `easy` 0, `normal` 1, `heroic` 2, `legendary` 3 |
| 51 | team | `player` 1, `human` 2, `covenant` 3, `sentinel` 5 |
| 62 | model_state | `object_set_region_state` (`minor damage` 1 ... `major damage` 3) |
| 65 | object | `object_destroy`, `fall_off`'s parameter; object name index |
| 66 | unit | `unit`'s return, `unit_kill` |
| 67 | vehicle | `ai_vehicle_get_from_starting_location`'s return, `vehicle_test_seat_list` |
| 69 | device | `device_set_position` |
| 70 | scenery | `scenery_animation_start` |
| 72 | object_name | `object_create`'s argument |

## 5. The names scripts point at

* **Object names** (`scnr + 0xA8`, 36 bytes: char[32], i16 object type, i16 placement index;
  `scenario-and-tag-reading.md` 2.2). Object types seen with names on 120_halo: 6 scenery, 7
  machine, 8 control, 9 **sound scenery**, 10 crate (and 0..3). Sound scenery lives at
  `scnr + 0x180` with its palette at `0x18C` (`ssce` tags): `plates01`..`plates63` are the
  `plate_dest_combined` crumbling sound the Warthog run creates and destroys as it goes.
* **Trigger volumes** (`zone-sets.md` 5), by index.
* **Cutscene flag, 28 bytes**: `0x00` string id name, `0x04` 3 x f32 position, `0x10` f32 yaw,
  `0x14` f32 pitch, `0x18` i16 (-1), 2 bytes. `trench_run_player0` = (-176.50, -351.47, -1.30),
  yaw 0: where the Warthog run insertion puts player 0.
* **Point set, 52 bytes** (in the scripting data element, `scnr + 0x41C`, block at `+0x00`):
  char[32] name, `0x20` block of points, `0x2C` i16 BSP index, `0x2E` i16 (-1), `0x30` 4 bytes.
  **Point, 60 bytes**: char[32] name (`p0`, `p1`...), `0x20` 3 x f32 position, `0x2C` i16 (-1),
  `0x2E` i16 BSP index, `0x30` 4 bytes, `0x34` f32 yaw, f32 pitch. `big_explosions_100_02` has 9
  points in BSP 7 (the run's first BSP). A point reference constant is set << 16 \| point
  (`exterior02/p0` = `0x00030000`, `ziggurat02` = `0x0015FFFF`, the whole set).
* **Editor folder, 260 bytes**: i32 parent (-1), char[256] name. 120_halo: 0 `meteor`, 1
  `trench_run_100`, 2..5 `trench_run_110_1..4`, 6 `trench_run_100_hw`, 7 `trench_run_110_hw`,
  8..11 `trench_run_120_1..4`, 12/13 `trench_run_100_crates` / `trench_run_110_crates`, ...

## 6. Placement fields scripts depend on

In every placement (scenery 180, machine 136, control 120, crate 176 bytes;
`scenario-and-tag-reading.md` 2.2):

| offset | type | meaning |
|---|---|---|
| `0x04` | u32 | placement flags. **Bit 0: not created when the map starts** (a script creates it). On 120_halo the crates of folders 12/13 and the meteors (folder 0) have bit 0 set and are only ever made by `object_create_folder` / `object_create`; the floor plates have `6` (bits 1, 2; meaning not pinned) and exist from the start |
| `0x42` | i16 | **editor folder** index, -1 none. Plates `100_*` are in folder 1, `110_*` in 2..7, `120_*` in 8..11, matching the folder names |
| `0x50` | u16 | on the run's plates, `1 << origin BSP` (0x80 for BSP 7); not used |

## 7. Object collision models (`coll`)

`obje + 0x34` -> `hlmt`; `hlmt + 0x10` -> `coll` (`scenario-and-tag-reading.md` 3). In the `coll`
tag:

| offset | what |
|---|---|
| `0x14` | materials (first field a string id: `hard_metal_thick_for...`) |
| `0x20` | **regions**, 16 bytes: string id name (`static`), block of permutations |
| `0x2C` | pathfinding spheres |
| `0x38` | nodes, 12 bytes: string id name, i16 parent / child / sibling |

**Permutation, 40 bytes**: string id name (`base`), `0x04` block of collision BSPs, then two
more blocks (BSP physics, mopp; not used). **Collision BSP, 100 bytes**: i16 node index, 2
bytes, then the same 96-byte collision geometry as the structure BSP's
(`collision-bsp-research.md`: 3D nodes `0x00`, planes `0x0C`, leaves `0x18`, 2D references
`0x24`, 2D nodes `0x30`, surfaces `0x3C`, edges `0x48`, vertices `0x54`, 12 / 12 / 16-byte
elements), except that its blocks are ordinary tag blocks instead of resource addresses. The
same triangulator reads both (`collision.rs`, `GeometrySource`).

`plate_flat`: region `static`, permutation `base`, 10 surfaces, 20 edges, 12 vertices: a slab
7.5 x 7.5 wu, 0.45 thick, top at +0.375. Vertices are in the node's frame; our reader takes
every node at the origin, exact for the one-node props.

**The Warthog run has no floor but the plates**: straight down from plate `100_001a` the first
surface is the plate itself and there is no structure BSP surface within 20 wu
(`warthog_run_floor_is_the_plates`). Before object collision existed, the whole run was a hole.

## 8. Damage sections

`hlmt + 0x88`: "new damage info", one element; its **damage sections** block is at `+0x68`.

**Damage section, 44 bytes**: `0x00` string id name, `0x04` u32 flags, `0x08` f32 vitality
percentage, `0x0C` block of **instant responses**, `0x18..` more (not read).

**Instant response, 136 bytes**: `0x00` u16 (2 on `destroy`, 0 on `fall`), `0x10` tag reference
to an effect, `0x20` a second tag reference, `0x58` / `0x5C` string ids (marker names: `root`,
`base_effect`), `0x60` f32 (1.0 on `fall`, 0.0 on `fall_immediate`). The rest is not decoded
(what makes the plate come loose is not pinned: the plates have a `phmo`, so it is probably the
physics model's constraint that the response breaks).

Every plate model (`objects\levels\solo\120_halo\plates\*`) has the same three sections:

| section | response effects | what the scripts use it for |
|---|---|---|
| `destroy` | `<plate>_explosion_chunks` (or `_explosion01`), `levels\solo\120_halo\fx\trench_run\plate_explosion\plate_explosion` | `blow_away`: the plate explodes |
| `fall` | `<plate>_fall` | `fall_off`: the plate drops away |
| `fall_immediate` | `<plate>_fall` | `fall_off_immediate` |

## 9. The Warthog run as the scripts drive it

The run (BSPs 100 -> 110 -> 120, zone sets 4..7) is about 1,160 scenery **floor plates**
(`plate_flat`, `plate_2x2_flat_hill`, `plate_ramp`, `plate_edge`... 34 plate tags in the scenery
palette), named `<bsp>_<row><letter>` (`100_001a` ... `120_035f`), in editor folders by stretch,
plus two device machines `100_island01` and `110_island01` (`mach ...\plates\island_test`), the
islands that tip and sink. What happens, in the scripts' own words (`blam scripts 120_halo
--grep trench`):

1. **Start.** `mission_halo` (startup) waits for `g_insertion_index` and wakes each chapter in
   turn; the Warthog run is `120_08_trench_run`, woken when the player enters
   `120_08_trench_run_start` or an insertion set the index to 8. The designers' shortcut is the
   static script `ins_trench_run`: `(switch_zone_set 120_04_100_110)`, `(object_teleport
   (player0) trench_run_player0)`, `(ai_place arbiter_trench/trench)`, `(set g_insertion_index
   8)`. `start` itself only knows insertion points 0 (`ins_crash_site`) and 1 (`ins_exit_run`).
2. **Folders.** `object_management` destroys the run's plate folders as soon as zone set 4 is
   fully loaded (`object_destroy_folder trench_run_100` ... `trench_run_110_crates`), and
   `plate_management` (woken by `trench_run_events`) creates them again stretch by stretch as
   the player drives into `trench_run_plate_100_trig`, `trench_run_plate_100_hw_trig`,
   `trench_run_plate_110_1_trig` ... `trench_run_plate_120_4_trig`. So the floor ahead is only
   there once you are close.
3. **The collapse.** `trench_run_events` places the Warthog (`ai_place warthog01`), the Mongoose
   easter egg (`secret_egg`), sets `trench_speed` by difficulty (easy 55, normal 45, heroic 30,
   legendary 25 ticks) and wakes `trench_run_100` / `_110` / `_120`, which wake the timers:
   `100_trench_run_timer_01` waits for `trench_run_100_trig01`, sleeps 90 ticks, then calls
   `trench_timer_100_001` ... `_013` in order, creating and destroying the crumbling sound
   scenery `plates01` ... as it goes. Each timer drops one row of plates behind and beside the
   player and then sleeps `trench_speed`:

   ```
   (script static void trench_timer_100_001
     (fall_off 100_001f) (fall_off 100_001e) (fall_off 100_001a)
     (sleep (random_range 0 30))
     (fall_off 100_001b) (fall_off 100_001c)
     (sleep (random_range 0 30))
     (fall_off 100_001d)
     (sleep trench_speed))
   (script static void fall_off ((object obj_name))
     (sleep (random_range 0 5))
     (object_damage_damage_section obj_name fall 1.0))
   (script static void blow_away ((object obj_name))
     (if (> (game_difficulty_get) normal) (sleep 7))
     (object_damage_damage_section obj_name destroy 1.0)
     (sleep 1))
   ```

   About 1,190 `fall_off`, 31 `fall_off_immediate` and 360 `blow_away` calls. The
   `trench_run_*_event_*` scripts blow plates up ahead of the player as they cross
   `trench_run_*_trig*` volumes (`(blow_away 100_003a)`); `trench_timer_*_amb*` drop side plates
   for show; `big_explosions_ambient` sets off `giant_explosion` at random points of the
   `big_explosions_*` point set nearest the player every 15-20 s.
4. **The islands.** `trench_timer_100_island01`: on `100_island_trig01`, `(device_set_position_track
   100_island01 other_fall 5.0)`, an overlay `tilt_up` animated over 4 s, then on
   `trench_run_100_trig07` `(device_animate_position 100_island01 1.0 10.0 0.25 0.25 true)`: the
   machine plays its `other_fall` animation over 10 s.
5. **Kill volumes.** `trench_killer` enables `kill_100_01` ... `kill_120_05` behind the player
   as they pass each trigger (`object_management` disabled them all when the zone set came in).

Nothing in the run is a crate or effect scenery: the floor is scenery, its collision is the
scenery's `coll`, and "blow up" and "fall away" are damage sections of the plate's model.

## 10. Our runtime (`crates/halo3_script`)

* **Threads.** One per startup, continuous and dormant script; static scripts are called inline.
  Each thread is an explicit stack of frames, so `sleep`, `sleep_until` (default period 30
  ticks, optional timeout) can stop it anywhere in an expression and resume on a later tick. 30
  ticks a second. `wake`, `sleep_forever` (self or another script), `script_finished`,
  `begin_random`, short-circuit `and`/`or`, `set`, script parameters. A thread that takes 50,000
  steps without sleeping is stopped and logged.
* **Done for real:** arithmetic and comparisons, `random_range`, difficulty (`--difficulty`),
  `players`/`list_get`/`unit`, trigger volume tests against the player (the skater's hips, the
  rider, the Spartan, else the camera), `object_create`/`_anew`/`object_destroy`/`object_hide`,
  `object_create_folder`/`object_destroy_folder`, `object_damage_damage_section` (`destroy`:
  effect and gone; anything else: the plate's collision goes at once, it tumbles away under
  gravity for 6 s, then is gone), `object_teleport` of the player to a cutscene flag (moves the
  fly camera, or the skater through `Respawn`), `switch_zone_set`, `current_zone_set`,
  `device_set_position`/`_immediate`/`device_animate_position`/`device_set_position_track`/
  overlay tracks/`device_get_position` (a 0..1 position animated over time), `effect_new*` at
  flags, points and objects (drawn as a stand-in flash), `kill_volume_enable`/`_disable` (below).
* **Deliberate difference:** `object_destroy_folder` only removes objects that start absent.
  `object_management` drops the run's plate folders purely to save memory and
  `plate_management` brings them back as you drive in; keeping them means the floor is there
  wherever you start riding or walking (a bike dropped ahead of the plate triggers used to
  fall through).
* **Kill volumes** follow the scripts: `kill_volume_disable` / `_enable` switch the volume off
  and on for the bike, the Spartan and the skater (`halo3_core::falls`). The run's start flag
  lies inside `kill_100_01`, which is on until `object_management` turns it off; the insertion
  is run for 15 ticks before the first frame so that has happened before anyone can press B, J
  or F. A script teleport also becomes the fallback spot for a fall.
* **Stubbed:** anything not listed logs once (`script: X is not implemented; does nothing`) and
  returns its type's default (0, false, no object). Cinematics, HUD, fades, saves. The sound functions
  (`sound_looping_*`, `sound_impulse_start` / `_stop`, `sound_class_set_gain`) are queued for the
  sound side as `halo3_core::script_sound::SoundRequest`s (`sound.md` 8).
* **Objects drawn on their own.** Every scene object a script constant names, every object in a
  folder a script names, and every object that starts absent is `SceneObject::dynamic`: the
  world builds its chunks apart (`ObjectChunk`), so the runtime can hide one (`ScriptHidden`) or
  move it (the chunk's `Transform`). On 120_halo that is about 1,300 objects.
* **Collision.** Every placed object's `coll` is added to the collision (`objects::add_collision`,
  `CollisionSource::Object`). When an object stops being solid (destroyed, dropped, absent, a
  device on a falling track past 2 %) the runtime tells `halo3_core::objects`; the bike's and
  the Spartan's collision grid skip its triangles at once, and the skater's collision is rebuilt
  without them a second after the last change (on a helper thread, like a zone-set swap).
* **Starting it.** Off unless asked: `--insertion trench_run` runs `ins_trench_run` as the
  designers did at the console (with the `editor` global on, so `mission_halo` does not run its
  own `start`), `--insertion N` starts at insertion point N, `--scripts` runs from the mission's
  own start. `HALO3_SCRIPTS=0` turns them off, `HALO3_OBJECT_COLLISION=0` drops object collision.

### Interface for the AI (enemies agent)

`halo3_script::hooks::AiHooks`, installed with `app.insert_resource(ScriptAi::new(MyAi))`:

```rust
pub trait AiHooks: Send + Sync + 'static {
    fn call(&mut self, function: &str, args: &[Value], ctx: &HookContext<'_>) -> Option<Value>;
    fn object_position(&self, object: ObjectRef) -> Option<Vec3> { None }
    fn tick(&mut self, ctx: &HookContext<'_>) {}
}
```

Every call whose name starts with `ai_`, `cs_`, `vs_`, `unit_` or `vehicle_` (and
`object_get_ai`, `objects_can_see_object`, `objects_can_see_flag`) goes to `call` with its
evaluated arguments. An AI argument arrives as `Value::Ref { ty: ValueType::AI, value, text }`,
`text` being the source's name (`warthog01/driver`, `arbiter_trench/trench`). Return `None` for
"not handled" and the script gets the default: `ai_living_count` 0, `ai_vehicle_get_from_starting_location`
no object. Objects the AI makes are `ObjectRef(ObjectRef::DYNAMIC + n)`; `object_position`
gives their place for volume tests. `HookContext` has the player's position, the script data
(names, flags, point sets) and the time. On the Warthog run the AI side is `ai_place warthog01`
(the Warthog the player drives, with its driver seat reserved `warthog_d`), `secret_egg`
(Mongoose), `arbiter_trench/trench`, the `chopper`s in co-op, and the encounters
`100_trench_run_encounters01` ... (Flood and Sentinels).

### Interface for animation (biped-anim agent)

`halo3_script::hooks::DevicePoser`, installed with `app.insert_resource(ScriptAnimation::new(p))`:

```rust
pub trait DevicePoser: Send + Sync + 'static {
    fn pose(&self, object: u32, tag: u32, track: Option<&str>, position: f32,
            overlay_track: Option<&str>, overlay: f32) -> Option<Mat4>;
}
```

`object` is the `Scene::objects` index and `tag` its `mach` tag (`hlmt + 0x20` is the `jmad`).
The animation crate on branch `biped-anim` (`halo3_anim`: `AnimModel::load(cache, mach_tag)`,
`Animator::set_position(p)`, `play_named("fall", false)`) fits behind it once both are on
master: an adapter loads one `AnimModel` per `tag`, sets the position, and returns the root
node's transform. Until then the stand-in below is used.
`track` the position track a script picked (`other_fall`), `position` 0..1 along it, likewise
the overlay (`tilt_up`). Return the object's transform relative to its placement (Halo frame);
`None` keeps the stand-in (a falling track sinks 60 wu and pitches; an overlay tilts a little).
One rigid transform per object: multi-node machines need the drawing side to grow node poses.

## 11. Not decoded

The AI ids' layout (squads, starting locations, actors), the rest of the scripting-data element
besides point sets, the response fields beyond those above, what placement flag bits 1..5 mean,
and the `0x50` placement field. HaloScript source files are not in cache maps (the `0x41C`
element's other blocks are empty on 120_halo).
