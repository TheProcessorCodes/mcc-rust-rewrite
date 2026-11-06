# Weapons (`weap`), their projectiles and markers, and where maps place them (MCC Halo 3 U13)

Read by `crates/blam_geometry/src/weapons.rs`; listed by `blam weapons <map> [--all] [--meshes]
[--markers]`; probed by `scripts/probe_weapons.py` (helpers in `scripts/h3_common.py`). Every
offset below was found and checked against the bytes of the user's `riverworld.map` (Valhalla) by
comparing the same field across all 47 `weap` tags of the map (`probe_weapons.py --all`), then
spot-checked on guardian, cyberdyne, zanzibar and sandbox through the CLI. No tool's source code
was used; names are ours, chosen from what the values do. "Inferred" marks a name that rests on the
value pattern only.

Units: world units (wu, 1 wu = 3.048 m), radians, seconds. Frame: +Z up, weapon +X forward.

## 1. The `weap` tag root

Offsets from the start of the tag's data. A weapon is an object (same header as scenery,
`scenario-and-tag-reading.md` 3), followed by item and weapon fields.

| offset | type | meaning | riverworld evidence |
|---|---|---|---|
| 0x000 | i16 | object type: **2 = weapon** (confirms the inferred order in the scenario doc) | 2 on all 47 |
| 0x034 | tag ref | model (`hlmt`); its +0 is the render model (`mode`) | e.g. `objects\weapons\rifle\assault_rifle\assault_rifle` |
| 0x24C..0x2AC | 6 tag refs | melee damage effects (`jpt!`), not read | `strike_melee`, `trigger_melee`, `smash_melee` |
| 0x31C | i16 | melee damage reporting kind, not read (3 for guns, 25 sword, 26 hammer, 31..34 objectives) | |
| 0x31E | i16 | **zoom levels**: 0, 1 or 2 | sniper and beam rifle 2; BR, carbine, laser, rocket, fuel rod 1; AR 0 |
| 0x320 | 2 x f32 | **zoom magnification range** (min, max) | sniper (4, 9), beam rifle (3.5, 9.5), BR (2, 2), laser (2.5, 2.5), rocket (1.8, 1.8) |
| 0x328 | 6 x f32 | aim assist: autoaim angle (rad), range, falloff range; magnetism angle, range, falloff range. Not read | AR 5 deg, 15, 7.5; 10 deg, 20, 10 |
| 0x3DC | string id | weapon class (inferred) | `rifle`, `pistol`, `missile`, `support`, `turret`, `sword`, `hammer`, `flag`, `ball`, `fixed` |
| 0x3E0 | string id | short name (inferred) | `ar`, `br`, `sr`, `sl`, `smg`, `sg`, `spr`, `pp`, `mp`, `bs` |
| 0x3FC | block, 32 bytes | **first-person models** (section 2) | 2 for every hand-held gun |
| 0x408 | tag ref | HUD (`chdt`), not read | `ui\chud\assault_rifle` |
| 0x424 | block, 128 bytes | **magazines** (section 3) | 1, or 0 for energy weapons |
| 0x430 | block, 144 bytes | **triggers** (section 4) | 1, or 2 (Banshee, Hornet, Scorpion cannon) |
| 0x43C | block, 308 bytes | **barrels** (section 5) | 1, or 2 (laser, plasma pistol, missile pod, Banshee, Hornet) |
| 0x4D8 | string id | **holster marker**: the biped marker the weapon hangs on while not held | `weapon_back` (rifles, laser, rocket, sniper), `weapon_thigh` (magnum, SMG, spiker, needler, plasma pistol and rifle); empty for vehicle guns and objectives |

The element sizes were found from the packed layout of the tag data (the next block's data starts
right after: first person 0x40 bytes for 2 elements, magazines 0x80 for 1, triggers 0x90 for 1)
and confirmed on the weapons with two elements: the Banshee's second trigger names barrel 1, whose
projectile is `banshee_bomb`; the laser's and plasma pistol's second barrel holds their charged
shot.

## 2. First-person element (32 bytes)

| offset | type | meaning |
|---|---|---|
| 0x00 | tag ref | first-person render model (`mode`), e.g. `...\assault_rifle\fp_assault_rifle\fp_assault_rifle` |
| 0x10 | tag ref | first-person animation graph (`jmad`) |

Element 0's graph is under `objects\characters\masterchief\fp\...` (the Spartan's), element 1's
under `objects\characters\dervish\fp\...` (the Elite's); both name the same model. The missile pod,
turrets and vehicle guns have none.

## 3. Magazine element (128 bytes)

| offset | type | meaning | AR / BR / sniper / shotgun |
|---|---|---|---|
| 0x00 | u32 | flags, not read | |
| 0x04 | i16 | rounds recharged, not read | 0 |
| 0x06 | i16 | **rounds in the weapon when it spawns**, loaded and spare together | 96 / 108 / 12 / 18 |
| 0x08 | i16 | **most rounds carried**, loaded and spare | 384 / 144 / 24 / 36 |
| 0x0A | i16 | **magazine size** (rounds loaded maximum) | 32 / 36 / 4 / 6 |
| 0x0C | i16 | most spare rounds (= 0x08 - 0x0A on every gun), not read | 352 / 108 / 20 / 30 |
| 0x14 | i16 | **rounds put in per reload** | 32 / 36 / 4 / 1 |

Plasma weapons and the laser have no magazine (they run on a battery, not read).

## 4. Trigger element (144 bytes)

| offset | type | meaning |
|---|---|---|
| 0x00 | u32 | flags, not read |
| 0x06 | i16 | **firing behaviour** (inferred names): 0 fires while held (AR, SMG, needler, spiker, plasma rifle, turrets); 1 one shot per pull (magnum, shotgun, sniper, rocket, brute shot, mauler); 4 one burst per pull (BR, carbine); 2 charge-up with a quick shot on release (plasma pistol); 6 charge-up then fire (laser) |
| 0x08 | i16 | primary barrel index (-1 none) |
| 0x0A | i16 | secondary barrel index (the charged shot: 1 on the laser and plasma pistol) |

## 5. Barrel element (308 bytes)

| offset | type | meaning | evidence |
|---|---|---|---|
| 0x00 | u32 | flags, not read | |
| 0x04 | 2 x f32 | **rounds per second** (min, max; they differ when the barrel spins up) | AR 10, BR 15, SMG 15, spiker 8, needler 7..10, plasma rifle 6..9, turret 5..10; 30 on single-shot guns (the trigger, not the barrel, limits those) |
| 0x0C | 2 x f32 | spin-up and spin-down time, not read | SMG 1.0 / 0.2, turret 1.25 / 2.0 |
| 0x1C | 2 x i16 | **shots per trigger pull** (min, max) | BR and carbine 3, 0 on automatics |
| 0x20 | f32 | **fire recovery time**: seconds before the next pull fires | BR 0.28, magnum 0.4, sniper 0.7, rocket 0.8, shotgun 1.0 |
| 0x24 | f32 | soft recovery fraction, not read | |
| 0x28 | i16 | **magazine** index; -1 for energy weapons | |
| 0x2A | i16 | **rounds used per shot** (0 for energy weapons) | |
| 0x2C | i16 | minimum rounds loaded, not read | |
| 0x2E | i16 | rounds between tracers, not read | |
| 0x74 | 2 x i16 | 0 and 1 on every barrel seen; not read (see 9.2) | |
| 0x7C | f32 | **minimum error** (rad) | AR 0.1 deg |
| 0x80 | 2 x f32 | **error angle** (min, max, rad): the spread cone | AR 0.1..3 deg, SMG 1.25..2.75 deg, sniper 0.5 deg |
| 0x88 | block | unknown (1 on laser, rocket, brute shot, needler; 3 on plasma pistol and spiker), not read | |
| 0x94 | i16 | damage reporting kind; equals the projectile's own at proj +0x1A8 | AR 16, sniper 14, magnum 5 |
| 0x98 | tag ref | **projectile** (`proj`) | class `proj` on every barrel |
| 0x128 | block | firing effects, 0x80 bytes: the firing and empty effects (`weapon-sounds.md` 1) | AR: one element, `assault_rifle\fx\firing` |

## 6. Projectile (`proj`) and damage effect (`jpt!`)

Halo 3 has no hitscan barrel: every gun fires a `proj` object (object type at +0 is **5**). The
fast ones behave like hitscan at game ranges (sniper 1200 wu/s, laser 4000); the battle rifle's
bullet is 180 wu/s and has to be led. `Projectile::is_near_hitscan` uses 400 wu/s as the line.

| offset | type | meaning | evidence |
|---|---|---|---|
| 0x1F0 | tag ref | **damage effect** (`jpt!`) on impact | `sniper_rifle_bullet`, `battle_rifle_bullet`, ...; null on rockets, missiles and grenades (they deal damage through their detonation, not read) |
| 0x22C | f32 | maximum range (inferred) | AR 40, sniper 120, laser 500, shotgun 4 |
| 0x23C | f32 | **initial velocity**, wu/s | AR 80, BR 180, SMG 400, sniper 1200, rocket 8, needle 11, spike 25 |
| 0x240 | f32 | **final velocity**, wu/s | rocket 16 (speeds up), grenade 7 (slows), spike 17.5 |

Damage effect:

| offset | type | meaning | evidence |
|---|---|---|---|
| 0x00 | 2 x f32 | area radius (min, max); 0 for bullets | Chopper shell 0.2 / 0.35 |
| 0x1C | f32 | damage lower bound | shotgun 3, plasma pistol 4, overcharge 400 |
| 0x20 | 2 x f32 | **damage upper bound** (min, max) | AR 7.5, BR 6 (x3 per burst), magnum 15, SMG 5, sniper 80, shotgun pellet 10, spike 9, plasma pistol 7, overcharge 400 |

These are the tag's raw numbers. What a hit takes off a target is further scaled by the target's
shield and body material responses (not read).

### 6a. Vehicle collision damage (`cddf`), the splatter

Every object block carries a collision damage reference at **+0x54** (tag ref, group `cddf`, the
index at +0x60): the Mongoose (`objects\vehicles\mongoose\mongoose`, `vehi`) points at
`globals\collision_damage\vehicle_small`, the Flood combat elite at another. The global damage effect
for collisions is `jpt! globals\collision_damage\collision`: radius 0.5 / 0.7, damage upper bound
**100 / 100** (the layout above). `blam_geometry::weapons::read_vehicle_collision` reads both.

`cddf`, the first ten floats as found on 120_halo (field meanings beyond "scale" and "a pair of
bounds" are inferred, not confirmed):

| tag | +0x00 | +0x04 | +0x08, +0x0C | +0x10, +0x14 | +0x18, +0x1C | +0x20, +0x24 |
|---|---|---|---|---|---|---|
| `vehicle_small` (Mongoose) | 1.2 | 1.0 | 3, 8 | 0, 0 | 0, 0 | 0, 0 |
| `vehicle_medium` | 1.2 | 1.0 | 3, 8 | 0, 0 | 8, 14 | 0, 0 |
| `biped` | 0.5 | 0.25 | 0, 0 | 0, 0 | 6.5, 8.5 | 0, 0 |
| `object_fragile` | 0 | 0 | 0, 0 | 0, 0 | 0.025, 0.5 | 0, 0 |

Our reading, used by `mcc_ai` for the dirt bike running into actors: +0x00 is the damage scale
the vehicle applies (1.2) and +0x08 / +0x0C a speed band in wu/s (3 to 8, about 33 to 88 km/h)
over which a hit goes from no damage to the full `collision` effect. So a bike hit deals
`100 x 1.2 x clamp((closing speed - 3) / (8 - 3), 0, 1)`. The engine's own use of acceleration,
mass and the victim's `cddf` is not reproduced.

## 7. Markers on weapon models, and the Spartan's

Markers use the render model layout in `scenario-and-tag-reading.md` G3.8: groups of 16 bytes
(name, block of 36-byte markers), each marker a node index and a position and rotation **relative
to that node**. A marker's bind-pose model-space frame is `node_world * T(position) * R(rotation)`
(`Marker::model`).

Weapon models (third person) carry, on node 0 `gun` unless noted:

| marker | meaning | AR (local to `gun`) |
|---|---|---|
| `primary_trigger` | the muzzle: where the primary barrel's projectile starts; X points down the barrel | (0.2334, 0, 0.0513) |
| `muzzle_flash` | flash effect points (5 on the AR, fanned) | (0.2278, ...) |
| `left_hand` | where the off hand grips | (0.1264, 0.0040, 0.0151) |
| `primary_ejection` | shell ejection | (-0.0292, -0.0122, 0.0524) |
| `flashlight` | | (0.2003, 0.0004, 0.0260) |
| `weapon_stow_anchor` | where it meets the holster marker | (0.0184, 0.0007, 0.0468) |
| `right_hand` | the grip, only on some models (sniper, shotgun, missile pod, brute shot) | |

Some models have per-species hand markers (`left_hand_mc`, `left_hand_elite`, `left_hand_brute`,
`right_hand_brute`, `left_hand_marine`); the plasma pistol has `secondary_trigger`; the sniper and
spiker `muzzle_brake`.

**The weapon model's origin is its grip.** The `gun` node is not always at the origin (AR, BR, SMG
and spiker yes; sniper at (0.056, 0, 0.037), shotgun (0.111, 0, 0.034), laser (-0.1125, 0,
0.0275), missile pod and brute shot rotated), but every weapon with its own `right_hand` marker has
that marker within 4 cm of the model origin in model space: sniper (-0.011, -0.003, 0.000),
shotgun (0.001, -0.001, -0.007) (its `right_hand` rides on the `pump` node), missile pod and brute
shot about 3 cm. (Pinned by `tests/weapons.rs`.)

The Spartan (`objects\characters\masterchief\mp_masterchief\mp_masterchief`, riverworld tag 8096):

| marker | node | local position | local rotation (x, y, z, w) | bind-pose model position |
|---|---|---|---|---|
| `right_hand` | 20 `r_hand` | (0.0465, 0.0043, -0.0044) | (-0.016, -0.074, 0.085, -0.994) | (0.1987, -0.1610, 0.4054) |
| `left_hand` | 19 `l_hand` | (0.0386, 0.0011, 0.0004) | (-0.994, 0.085, -0.074, -0.016) | (0.1950, 0.1592, 0.4140) |
| `weapon_back` | 6 `spine1` | (0.0742, -0.0799, 0) | (0, -0.707, -0.707, 0) | (-0.0800, 0, 0.5800) |
| `weapon_thigh` | 2 `r_thigh` | (0.0574, 0.0149, 0.0601) | (-0.995, 0.001, 0.007, -0.095) | (0.0100, -0.1100, 0.3450) |

The Spartan's bind pose has both hands forward at hip height (not a T-pose); the `right_hand`
marker's +X there points forward and down, (0.83, -0.26, -0.50).

**Holding a weapon.** Put the weapon model's origin and axes on the biped's `right_hand` marker:
`weapon_to_biped = hand_bone_now * T(marker.position) * R(marker.rotation)`
(`weapons::held_weapon_transform`), with `hand_bone_now` the animated model-space frame of node 20
`r_hand` (its bind-pose frame for a static figure). The off hand then reaches for the weapon's
`left_hand` marker (inverse kinematics, not done here). A weapon not in hand hangs on the marker
its tag names at 0x4D8 (`weapon_back` / `weapon_thigh`), meeting it with its
`weapon_stow_anchor`. Shots leave from `primary_trigger` along its +X.

## 8. Where weapons sit on a map

Halo 3 multiplayer maps place weapons in the scenario itself; there are **no item collections**
(`itmc`, 0 tags on riverworld) and no netgame-equipment block. Two scenario blocks, each with a
palette of 16-byte tag references:

| block | placements | stride | palette | riverworld |
|---|---|---|---|---|
| weapons | scnr +0x114 | 168 | scnr +0x120 | 42 placements, palette of 14 `weap` |
| equipment | scnr +0xFC | 140 | scnr +0x108 | 32 placements, palette of 10 `eqip` (grenades, equipment, power-ups) |

The placement header is the scenery one: palette index i16 +0x00, name index +0x02, position
+0x08, yaw / pitch / roll +0x14, scale +0x20; the byte at +0x3E is the object type (2 weapon,
3 equipment). Then:

| offset (weapon / equipment) | type | meaning |
|---|---|---|
| +0x80 / +0x64 | i16 | **respawn time**, seconds (inferred from values: sniper and laser 180, missile pod 150, shotgun 90, base BRs and spikers 15, grenades 15 or 30; 0 on the map-wide BRs, SMGs and plasma pistols) |
| +0x7E / +0x62 | u8 | unknown (2, 4, 6, 8 or 24), not read |

Weapons lie on their sides on the ground, so most have large pitch or roll (the battle rifles'
roll is near +-90 deg).

**riverworld (Valhalla), 42 weapon placements:**

| weapon | count | respawn | where (wu) |
|---|---|---|---|
| battle rifle | 19 | 15 at the bases (4), 0 elsewhere | spread over the map |
| SMG | 6 | 0 | a pair in each base (at (76.77, -58.8, 2.52) and (76.76, -145.5, 3.41)), two in the field |
| spike rifle | 4 | 15 | |
| plasma pistol | 4 | 0 | |
| sniper rifle | 2 | 180 | (82.32, -56.59, 0.12) and (80.84, -147.78, 1.01), one per base |
| missile pod | 2 | 150 | (83.43, -60.79, 2.14) and (83.17, -143.40, 3.04) |
| shotgun | 2 | 90 | (95.68, -86.63, 3.35) and (92.40, -114.44, 3.25) |
| brute shot | 2 | 0 | (50.33, -88.05, 2.76) and (51.70, -108.63, 3.34) |
| Spartan laser | 1 | 180 | (82.80, -102.47, 5.00), mid-map |

Other maps seen: guardian 20 weapon placements (14 kinds) and 12 equipment; cyberdyne 38 and 19;
zanzibar 44 and 32; sandbox 17 and 16.

## 9. Not read or not settled

1. **Pellets per shot.** The shotgun's barrel has nothing that reads 15 (or any pellet count);
   the i16 pair at barrel +0x74 is (0, 1) on every barrel, the shotgun's included. Where Halo 3
   gets the shotgun's spread of pellets is not found.
2. The trigger behaviour names, the projectile's maximum range and the placements' respawn time are
   inferred from the values only.
3. Zoom: whether the game shows the stored range directly (sniper 4..9) or adds one (5x / 10x as
   players know it) is not checked.
4. The `right_hand` marker's rotation is applied as stored (the same convention as node
   quaternions, verified for nodes only). Holding was checked for position (origin = grip), not
   yet seen on screen with an animated Spartan.
5. Melee damage, heat and battery, aim assist and the game-type bytes of the placements are not
   read. Firing effects and sounds are in `weapon-sounds.md`.
