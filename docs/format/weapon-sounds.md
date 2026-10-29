# Weapon sounds: from a `weap` tag to the `snd!` tags it plays (MCC Halo 3 U13)

Written 2026-10-29 from our own probes of the user's `riverworld.map` (Valhalla). Read by
`crates/blam_sound/src/weapons.rs` (`WeaponSoundReader`) and the sound classes by
`blam_sound::tags::SoundTags::classes`; played by `crates/halo3/src/audio/weapons.rs`; probed by
`scripts/probe_weapon_sounds.py [map] [--all] [--classes]`; pinned by
`crates/blam_sound/tests/weapons.rs`. Offsets were found by scanning tag data for references and
blocks (`h3_common.Map.scan`) and comparing the same field across weapons; names are ours, from what
the values do. No tool's source code or plugin XML was used. "Inferred" marks a meaning that rests on
the value pattern only. Layouts of the `weap` tag itself are in `weapons.md`; of `snd!`, `lsnd` and
the sound gestalt in `sound.md`.

## 0. Where each sound lives

| action | where | notes |
|---|---|---|
| fire | barrel (`weap +0x43C`, 308 bytes) `+0x128` firing effects, element 0 `+0x04` -> `effe` -> sound parts | trigger 0 `+0x08` names the primary barrel |
| charged fire | the secondary barrel's (trigger 0 `+0x0A`) firing effect | plasma pistol overcharge, Spartan laser beam |
| fire, held (SMG) | object attachments (`+0xBC`) whose function is `loop_fire_single` -> `lsnd` | the SMG's firing effect has no sound |
| first-person extra | first-person animation graph, animation `first_person:fire_1[:varN]` | shotgun pump (`sg_fire`) |
| empty click | firing effect element 0 `+0x24` -> `effe` or a `snd!` directly | |
| reload | first-person graph, animation `first_person:reload_full[...]`, else `reload_empty`, else `reload_continue_*` | |
| ready | first-person graph, animation `first_person:ready` | |
| charge | object attachment with function `primary_charged` -> `lsnd` (in / loop / out) | |
| impact | barrel `+0x98` `proj`: detonation effect `+0x148`, impact effect `+0x1D0`, material effects `+0x80` (`foot`) type 15 | |
| ammo pickup | `weap +0x3A0` `snd!` | not tied to a message |
| zoom in / out | `weap +0x3B0` / `+0x3C0` `snd!` | not used yet |

## 1. `weap` fields used here

| offset | type | meaning | evidence |
|---|---|---|---|
| 0x0BC | block, 32 bytes | **object attachments** (section 6) | AR: firing light, flashlight light, flashlight effect |
| 0x3A0 | tag ref | **ammo pickup sound** (inferred from names) | `battle_rifle_ammo` on AR and BR, `rocket_ammo` on the laser, `sniper_ammo` on turrets; null on plasma weapons |
| 0x3B0, 0x3C0 | tag ref | zoom in, zoom out sound | `zoom_in` / `zoom_out`, `battle_rifle_zoom_in` |
| 0x3FC | block, 32 bytes | first-person models; element 0 `+0x10` is the Spartan's animation graph (`weapons.md` 2) | |

**Firing effect element (barrel +0x128), 0x80 bytes** (one element on every riverworld barrel, so
the size is taken from the reference spacing, not from a gap):

| offset | type | meaning | evidence |
|---|---|---|---|
| 0x00 | 2 x i16 | shot count range (inferred) | 1..1, 0..0 on the laser tracer |
| 0x04 | tag ref | **firing effect** | `assault_rifle\fx\firing` |
| 0x14 | tag ref | misfire effect (inferred; null on the AR, plasma pistol and laser) | |
| 0x24 | tag ref | **empty effect**: an `effe` or a `snd!` | `battle_rifle\fx\empty`, `sound\weapons\plasma_pistol\dryfire` (an `effe` filed under sound), `battle_rifle\dryfire` (`snd!`) |
| 0x44 | tag ref | firing rumble (`drdf`) | `trigger_fullauto`, `trigger_semiauto`, `trigger_launcher` |
| 0x54 | tag ref | empty / misfire rumble (`drdf`) | `trigger_missfire` |

## 2. Effects (`effe`): where the sound parts are

| offset | element | what |
|---|---|---|
| effe +0x20 | 12 bytes | locations (marker names); AR firing has 3 |
| effe +0x2C | **68 (0x44)** | **events**: `+0x00` string id name (`1st_person`, `3rd_person`, `casings`, `flash`, `event_0`), **`+0x20` parts block**, `+0x38` a block with 3 / 4 / 1 elements on AR firing's three events (particle systems, inferred) |
| event +0x20 | **96 (0x60)** | **parts**: `+0x0C` the referenced tag's class again (`snd!` fourcc), **`+0x10` tag reference** (`snd!`, `ligh`, `effe`, ...) |

Sizes from the packed data: AR firing's locations (3 x 12) end exactly where its parts start; part
references at `+0x34` and `+0x94` of the location block are 0x60 apart; the event block's parts
blocks recur every 0x44 (`+0x20`, `+0x64`, `+0xA8`). The event names do not decide who hears a
part: the AR's near shot and its far layer both sit in the event named `1st_person`.

## 3. First-person animation graph (`jmad`): reload and ready

| offset | element | what |
|---|---|---|
| jmad +0x20 | **20 bytes** | **sound references**: `+0x00` tag ref (`snd!`), `+0x10` u32 flags (8 on the posing sounds) |
| jmad +0x50 | **136 bytes** | **animations**: `+0x00` string id name (`first_person:reload_full`), `+0x10` i16 frame count (AR reload 58, ready 20), `+0x2C` frame events block (4 bytes), **`+0x38` sound events block** |
| sound event | **8 bytes** | `+0x00` i16 sound reference index, `+0x02` i16 frame, `+0x04` string id marker (empty) |

AR graph: sounds `ar_reload, ar_ready, ar_melee2, ar_melee1, pose_var1, pose_var2`; animation
`first_person:ready` has one event (sound 1, frame 0), `reload_full` and `reload_empty` one each
(sound 0, frame 0). Every sound event seen is at frame 0, so a reload sound is one long clip
(`ar_reload` 2.0 s, `br_reload_full` 2.2 s) started with the animation.

Names vary: `reload_full`, `reload_full:var1` (spiker), `dual:reload_full:var0` (dual wield, skipped),
the shotgun's `reload_enter` (silent), `reload_continue_empty` (`sg_reload_loop`, one shell) and
`reload_exit`. Energy weapons (plasma pistol, laser) have no reload animation.

## 4. Projectile (`proj`) fields for sound

| offset | type | meaning | evidence |
|---|---|---|---|
| 0x080 | tag ref | **material effects** (`foot`) | `fx\material_effects\objects\weapons\battle_rifle` (AR and BR share it) |
| 0x138 | tag ref | detonation effect in the air (inferred: "airborne" in the names) | brute shot `airborne_detonation`, missile pod `detonation_airborne` |
| 0x148 | tag ref | **detonation effect** (on the ground) | brute shot `fx\detonation`, laser beam `fx\impact` |
| 0x158 | tag ref | detonation damage (`jpt!`) | |
| 0x1C0 | tag ref | **flyby sound** (`snd!`), not played yet | `bullet_flyby_short` (AR), `_medium` (BR), `_long` (sniper, shotgun), `spike_flyby` |
| 0x1D0 | tag ref | **impact effect** | spiker `fx\impact` (no sound part), missile pod `fx\detonation` |
| 0x1E0 | tag ref | object impact effect (inferred) | spiker `fx\object_impact` |
| 0x1F0 | tag ref | impact damage (`jpt!`) (`weapons.md` 6) | |
| 0x270 | block | material responses (not read) | AR 15, sniper 19 |

Bullets have no detonation or impact effect: their impact sound is in the material effects.

## 5. Material effects (`foot`)

`foot +0x00` is a block of **36-byte effect types**, 23 on every weapon `foot` (the size checks: 23
x 36 ends exactly at the tag root). Each type: blocks at `+0x00` (old materials, empty everywhere),
**`+0x0C` sounds**, **`+0x18` effects**. Sound and effect entries are **40 bytes**: `+0x00` tag ref
(the sound or effect), `+0x10` a second tag ref (sand sweeteners, cardboard), **`+0x20` string id
material name**, `+0x24` i16 runtime material index, `+0x26` i16 (2 on sounds, 1 on effects).

**Type 15 is the projectile landing** (inferred: it is the one filled on every weapon `foot`: BR 37
sounds / 70 effects, sniper 38 / 68, rocket and missile pod 2 / 18). Type 10 is the item hitting
the ground (`battle_rifle_drop`), the casings `foot` tags fill only 10. Types 16 to 22 not settled.

The impact message carries no material, so the reader assumes **`tough_terrain_grass`** (Valhalla's
field) and walks up its name: `tough_terrain_grass`, `tough_terrain`, `tough`, `default_material`.
Sounds and effects are looked up separately (the missile pod's explosion is the `default_material`
sound, its dirt the `tough_terrain` effect). That the game's material parents follow the names is
inferred, not read from `matg`.

## 6. Object attachments (any object, +0xBC), 32 bytes

| offset | type | meaning | evidence |
|---|---|---|---|
| 0x00 | tag ref | attached tag (`ligh`, `effe`, `lsnd`, `lens`) | |
| 0x10 | string id | marker | `primary_trigger`, `secondary_trigger`, `flashlight`, empty |
| 0x14 | string id | change color function (empty everywhere seen) | |
| 0x18 | string id | **function that drives it** | `primary_charged` (charging loops), `loop_fire_single` / `loop_fire_left` / `loop_fire_right` (SMG loops), `illumination`, `flashlight_intensity` |
| 0x1C | string id | secondary function (empty) | |

Size from the spacing (refs at 0x00, 0x20, 0x40 of the block; the `sound.md` reader had assumed 24,
which only mattered past the first element; fixed). Only `primary_charged` and `loop_fire_single`
are played; the `_left` / `_right` loops are for dual wielding.

## 7. Sound classes (`sncl`) and the four distances

One `sncl` tag (`sound\sound_classes`); `+0x00` a block of **152-byte (0x98) classes**, 54 on
riverworld, indexed by a `snd!`'s class byte (`sound.md` 4.1). Size from the stride of a repeating
run of three 0.5 floats (152 apart). Fields used:

| offset | type | meaning | class 4 (weapon fire) |
|---|---|---|---|
| 0x00 | i16 | most copies of one sound at once (inferred) | 8 |
| 0x02 | i16 | **most copies one object plays at once** (inferred) | 2 |
| 0x04 | i16 | preemption time, ms (inferred) | 0 |
| 0x34 | 4 x f32 | **default distances**, wu | 0, 0, 3, 55 |

A sound's playback parameters (`sound.md` 4.3) hold the same four distances; **bit n of their
flags set means distance n is not the sound's own and the class's is used**. Flags 0x1F (AR shot,
most weapon sounds) take all four from the class; 0x13 (sniper shot: 0, 0, 6, 65) the first two;
0x10 (sniper far layer: 6, 45, 50, 100) none. Bit 4's meaning is unknown.

The four distances, as read (inferred from the far-away layers): **silent within the first, fading
in up to the second, full volume up to the third, falling to silence at the fourth.** Class 15, the
`*_lod_far` layers, is (3, 20, 35, 70): silent in the hand, full across the map; class 3 (distant
explosions) is (15, 40, 45, 100). Classes seen on weapon sounds: 0 bullet impacts (2, 15), 1 large
impacts and explosions (8, 50), 4 weapon fire (3, 55), 5 ready (1.5, 20), 6 reload and ammo (1.5,
20), 7 empty click (1, 15), 8 plasma pistol charge, 9 overheat (3, 25), 13 dirt burst, 15 far
layers. `blam_sound::tags::distance_gain` implements the curve (the fall is squared, as the
ambience code already did; the game's curve is not known).

## 8. Valhalla's weapons

Tag names below drop the leading `sound\weapons\`. "-" = none found.

| weapon (tag) | fire | empty | reload | ready | charge | impact (grass) |
|---|---|---|---|---|---|---|
| battle rifle (990) | `battle_rifle\fire_burst_h3`, `eject_br`, `br_lod_far` | `battle_rifle\dryfire` | `br_reload_full` | `br_ready` | - | `impacts_riccs\bullet_impact_dirt` |
| assault rifle (280, not placed) | `assault_rifle\ar_fire`, `assault_rifle_lod_far` | `battle_rifle\dryfire` | `ar_reload` | `ar_ready` | - | `bullet_impact_dirt` |
| SMG (1872) | loops `smg\smg_loop\smg_loop` (in 0.1 s, loop 0.9 s, out 0.8 s) and `smg_lod_far` | `smg\dryfire` | `smg_reload` | `smg_ready` | - | `bullet_impact_dirt` |
| sniper rifle (1945) | `sniper_fire_h3` (5.1 s with its echo), `sniper_eject`, `fire_lod_far` | `battle_rifle\dryfire` | `sr_reload` | `sr_ready` | - | `bullet_impact_dirt` |
| shotgun (1691) | `shotgun\fire`, `shotgun_lod_far`; first person `sg_fire` (pump) | `battle_rifle\dryfire` | `sg_reload_loop` (one shell) | `sg_ready` | - | `bullet_impact_dirt` |
| plasma pistol (4831) | `plasma_pistol\fire`, `plasma_pistol_lod_far`; charged `chargefire`, `plasma_charge_lod_far` | `plasma_dryfire` | - (no magazine) | `plasma_pistol_ready` | `plasmapistol_charging` (in 2.6 s, loop) | `impacts_riccs\plasma_impacts_h3_up`; charged `plasma_charge_hit` |
| spike rifle (4642) | `spike_rifle_fire`, `spike_lod_far` | `smg\dryfire` | `fp_spike_rifle_reload_full` | `fp_spike_rifle_ready` | - | `spike_rifle\spike_impacts_soft` |
| brute shot (5076) | `brute_shot_fire_new`, `brute_shot_lod_far` | `battle_rifle\dryfire` | `fp_brute_shot_reload` | `fp_brute_shot_ready` | - | `brute_round_lod_far`, `brute_round_explode`, `materials\...\dirt_burst` |
| missile pod (5032) | `missile_launcher\missile_launch`, `rocket_launcher_lod_far` | `battle_rifle\dryfire` | - (no first-person graph) | - | - | `visual_fx\h3_dist_explosion`, `h3_lrg_explosion`, `dirt_burst` |
| Spartan laser (1084) | primary barrel (tracer) silent; charged `spartan_laser_overheat`, `spartan_laser\fire`, `spartan_laser_lod_far` | `battle_rifle\dryfire` | - (battery) | `fp_spartan_laser_ready` | `spartan_laser\charging\charging` (in 4.8 s, loop, out) | `spartan_laser_impacts_lod_far`, `spartan_laser_impacts` |

Not found: a reload sound for the missile pod, plasma pistol and laser (they have none in the data
read); third-person reload and ready sounds (the Spartan's own third-person graph was not searched;
the first-person ones are used for everyone); melee sounds (in the same graphs, `melee_strike_*`,
not played); flyby sounds (read in the probe, not played).

## 9. Not settled

1. The meaning of the four distances, and the squared fall-off.
2. Types 16 to 22 of material effects; the impact material (assumed grass, parents by name).
3. When the game picks the charged shot: the code takes it after a 0.6 s charge (plasma pistol)
   or always when the weapon has no plain shot (laser).
4. How loud weapons are against the rest: class gains and the mix (`snmx`) are not decoded; the
   code plays weapons at 0.7 and the tag gain.
5. Whether the `1st_person` / `3rd_person` event names mean anything for sound parts.
