# On-foot movement: speeds, jump, body, slopes (MCC Halo 3, U13)

The numbers that make the player move like Halo 3: run and crouch speeds, accelerations, jump,
gravity, capsule size, eye heights, slope limits, fall damage and look rates. Written 2026-10-29
from our own probes of the user's `riverworld.map` (Valhalla), and checked to be identical on
`guardian`, `zanzibar`, `cyberdyne`, `sandbox` and `120_halo`.

No GPL code or plugin XML was used. The tags were found by walking the block headers of the
globals tag and the biped tag and dumping every float. Field names come from the general,
long-public shape of Halo's player and biped tags (the same fields with the same names have been
in the Halo 1 and Halo 2 tag tools since 2004). Each name was then checked against the bytes:
the value has to make sense, and where the tag also keeps a value the engine works out from
another field (a cosine, a sine, a speed), the two must agree. Section 7 lists what was checked
and how, and what was not.

Code: `crates/blam_geometry/src/movement.rs` (reader, `Movement`, test pinning Valhalla),
`crates/blam_cli/src/movement.rs` (`blam movement <map>`).

Probe: `python -I docs/format/scripts/probe_movement.py [map ...]` prints every field below
with its offset and the cross-checks.

Units: lengths in world units (wu), speeds wu/s, accelerations wu/s², angles radians unless the
field says degrees. This project uses 1 wu = 3.048 m (`WORLD_UNIT_METRES`).

## 1. Where the numbers live

- **Globals** (`matg`, `globals\globals`, one per map). Its data starts with 12 language-pack
  structs of 0x50 bytes (`+0x18` .. `+0x3C8`), then a run of 12-byte block headers
  (`container.md`). The four blocks used here each hold one element, except player
  representation.
- **The player biped** (`bipd`). Found through the globals' player representation block, not by
  name: the first representation whose third-person unit is a `bipd`. On multiplayer maps that is
  `objects\characters\masterchief\mp_masterchief\mp_masterchief`, on campaign maps
  `objects\characters\masterchief\masterchief`. Both have the same numbers.

| globals offset | element | what |
|---|---|---|
| `0x3C8` | | havok cleanup (effect ref) - not used |
| `0x3D4` | | sound globals (`sncl`, `sfx+`, `snmx` refs) - not used |
| `0x3E0` | | AI globals - not used |
| `0x3F8` | | camera globals (`globals\default_unit_camera_track`) - not used |
| `0x404` | 1 x 0x6C | **player control** (section 4) |
| `0x410` | | difficulty scales - not used |
| `0x41C` | 4 x 0x34 | grenades (frag, plasma, claymore = spike, firebomb) |
| `0x428` | | interface tags (`ui\chud\globals` ...) |
| `0x434` | 1 x 0xC0 | **player information** (section 2) |
| `0x440` | 6 x 0x5C | **player representation**: `+0x08` first-person hands `mode`, `+0x18` first-person body `mode`, `+0x28` third-person unit `bipd` |
| `0x458` | 1 | **falling damage** (section 5) |

Element sizes are the distance between neighbouring blocks' data; the player representation size
was confirmed by the `bipd` refs landing at the same `+0x28` in elements 2, 3 and 5.

## 2. Player information (globals `+0x434`, one element)

| offset | field | Valhalla | m |
|---|---|---|---|
| `0x00` | walk speed (name inferred, see 7) | 0.512 | 1.561 m/s |
| `0x04` | run forward | 2.25 | 6.858 m/s |
| `0x08` | run backward | 2.0 | 6.096 m/s |
| `0x0C` | run sideways | 2.0 | 6.096 m/s |
| `0x10` | run acceleration | 9.6 | 29.26 m/s² |
| `0x14` | crouch ("sneak") forward | 0.9 | 2.743 m/s |
| `0x18` | crouch backward | 0.65 | 1.981 m/s |
| `0x1C` | crouch sideways | 0.6 | 1.829 m/s |
| `0x20` | crouch acceleration | 4.8 | 14.63 m/s² |
| `0x24` | airborne acceleration | 1.05 | 3.20 m/s² |
| `0x28` | grenade origin (x, y, z) | 0, 0.05, 0 | |
| `0x34` | stun movement / turning / jumping penalties | 0.8, 0.8, 1.0 | |
| `0x40` | minimum / maximum stun time | 0, 4 | |
| `0x48` | first-person idle time range, s | 15 .. 45 | |
| `0x50` | first-person skip fraction | 0.75 | |
| `0x74` | coop respawn effect (`effe`) | `masterchief\fx\coop_teleport` | |
| `0x84` | binocular zoom count, range | 1; 3 .. 3 | |
| `0x90`, `0xA0` | flashlight on / off sounds | | |
| `0xB0` | default damage response (`drdf`) | | |

Only `0x00`..`0x24` are read by the code. At full run, 2.25 wu/s is reached in
2.25 / 9.6 = 0.23 s from a standstill.

## 3. The biped (`bipd` data, offsets from the start of the tag)

The tag is object, then unit, then biped fields, then the character physics. Only the fields
below are read.

| offset | field | Valhalla | notes |
|---|---|---|---|
| `0x10C` | unit camera field of view | 1.22173 (70.0°) | `+0xF8` is the unit flags (0x1004), `+0x128` the camera tracks block |
| `0x2BC` | moving turning speed, rad/s | 8.48579 (486°/s) | first biped field |
| `0x2C4` | stationary turning threshold | 1.39626 (80°) | |
| `0x2C8` | **jump velocity** | 3.08 | 9.39 m/s |
| `0x2CC` | maximum soft landing time, s | 0.6 | |
| `0x2D0` | maximum hard landing time, s | 0.1 | order of these two not checked |
| `0x2D4` | minimum soft landing velocity | 0.5 | |
| `0x2D8` | minimum hard landing velocity | 3.8 | above jump speed: a jump never lands hard |
| `0x2DC` | maximum hard landing velocity | 10 | |
| `0x2E0` | death hard landing velocity | 0 | |
| `0x2E4` | stun duration | 1.7 | |
| `0x2E8` | **standing camera height** (eye, above the feet) | 0.62 | 1.890 m |
| `0x2EC` | **crouching camera height** | 0.45 | 1.372 m |
| `0x2F0` | crouch transition time, s | 0.2 | |
| `0x2F4`, `0x2F8` | camera interpolation start / end | 15°, 90° | |
| `0x2FC`..`0x304` | camera forward / side / vertical movement scale | 0.75, 0.5, 0.5 | |
| `0x308` | camera exclusion distance | 0.13 | |
| `0x30C` | autoaim width | 0 | |
| `0x314` | lock-on distance | 15 | |
| `0x33C` | character physics flags | 4 | |
| `0x340` | **height standing** | 0.725 | 2.210 m |
| `0x344` | **height crouching** | 0.5 | 1.524 m |
| `0x348` | **radius** | 0.175 | 0.533 m |
| `0x34C` | mass | 125 | |
| `0x350`, `0x354` | living / dead material (string ids) | | |
| `0x35C` | dead sphere shapes block (3) | | |
| `0x368` | **pill shapes** block (2 x 0x70) | | section 3.1 |
| `0x380` | **maximum slope angle** | 0.872665 (50°) | ground physics starts here |
| `0x384` | downhill falloff angle | 45° | |
| `0x388` | downhill cutoff angle | 50° | |
| `0x38C` | uphill falloff angle | 45° | |
| `0x390` | uphill cutoff angle | 50° | |
| `0x394` | downhill velocity scale | 1.5 | |
| `0x398` | uphill velocity scale | 0.5 | |
| `0x39C` | runtime: cos(maximum slope) | 0.642788 | engine-computed, = cos 50° |
| `0x3A0`, `0x3A4` | runtime: -sin(downhill falloff), -sin(downhill cutoff) | -0.707107, -0.766044 | |
| `0x3A8`, `0x3AC` | runtime: sin(uphill falloff), sin(uphill cutoff) | 0.707107, 0.766044 | |
| `0x3B0` | climb inflection angle | -0.349066 (-20°) | |
| `0x3B4`..`0x3C0` | four more floats, all 0 | | not identified (see 7) |
| `0x3F0` | contact points block (2) | | |

### 3.1 Pill shapes (the collision capsules)

Each 0x70-byte element is a Havok capsule. `+0x40` is the radius; `+0x50` and `+0x60` are the
two end points as four floats (x, y, z, radius again):

| pill | radius | end A z | end B z | covers |
|---|---|---|---|---|
| 0 (standing) | 0.175 | 0.55 | 0.175 | z 0 .. 0.725 |
| 1 (crouching) | 0.175 | 0.325 | 0.175 | z 0 .. 0.5 |

So the capsule's bottom is at the feet (the biped origin), and the two pills reach exactly the
standing and crouching heights at `0x340` / `0x344`. The reader checks pill 0 against `0x340`.

## 4. Player control (globals `+0x404`, one element)

| offset | field | Valhalla |
|---|---|---|
| `0x00` | magnetism friction | 0.6 |
| `0x04` | magnetism adhesion | 0.6 |
| `0x08` | inconsequential target scale | 0.35 |
| `0x0C` | crosshair location (x, y) | 0, 0.17 |
| `0x14` | seconds to start | 4 |
| `0x18` | seconds to full speed | 0.8 |
| `0x1C` | decay rate | 2 |
| `0x20` | full speed multiplier | 1 |
| `0x24` | pegged magnitude | 0.9 |
| `0x28` | pegged angular threshold, degrees | 10 |
| `0x2C` | **look default pitch rate, degrees/s** | 60 |
| `0x30` | **look default yaw rate, degrees/s** | 120 |
| `0x34` | look peg threshold | 0.95 |
| `0x38`, `0x3C` | look yaw acceleration time, scale | 0.8, 2.5 |
| `0x40`, `0x44` | look pitch acceleration time, scale | 0.8, 2.5 |
| `0x48` | look autoleveling scale | 0.75 |
| `0x50` | two shorts: 0, 15 (minimum autoleveling ticks?) | |
| `0x54` | minimum angle for vehicle flipping | 0.785398 (45°) |
| `0x58` | look function block (6 elements; probably the stick response curve) | |

The look rates are the full-stick turn speeds at the base sensitivity; the controller sensitivity
setting scales them. The acceleration fields ramp the yaw rate up by "scale" over "time" once the
stick is pegged (past 0.95). The reader keeps only the two rates and the magnetism pair.

## 5. Falling damage (globals `+0x458`, one element)

| offset | field | Valhalla |
|---|---|---|
| `0x00` | harmful falling distance (from, to) | 6 .. 10 wu (18.3 .. 30.5 m) |
| `0x08` | falling damage (`jpt!` `globals\falling`) | |
| `0x28`, `0x38` | soft / hard landing damage (`jpt!`) | |
| `0x48` | `globals\hs_damage` | |
| `0x58` | maximum falling distance (fatal) | 14 wu (42.7 m) |
| `0x5C` | `globals\distance` (`jpt!`) | |
| `0x6C` | runtime: speed after falling the maximum distance | 9.47853 |
| `0x70` | runtime: speed after falling the harmful "from" distance | 6.20515 |
| `0x74` | runtime: speed after falling the harmful "to" distance | 8.01082 |

The three runtime speeds are `sqrt(2 g h)` for their distances: each gives
g = v² / 2h = **3.20866 wu/s²** to six figures.

## 6. Valhalla's numbers in one place

`blam movement riverworld` prints these, in wu and metres. The test
`movement::tests::riverworld_movement` pins them.

| | wu | metres |
|---|---|---|
| run forward / backward / sideways | 2.25 / 2.0 / 2.0 per s | 6.86 / 6.10 / 6.10 m/s |
| run acceleration | 9.6 /s² | 29.3 m/s² |
| crouch forward / backward / sideways | 0.9 / 0.65 / 0.6 per s | 2.74 / 1.98 / 1.83 m/s |
| crouch acceleration | 4.8 /s² | 14.6 m/s² |
| airborne acceleration | 1.05 /s² | 3.20 m/s² |
| jump velocity | 3.08 /s | 9.39 m/s |
| gravity (derived) | 3.20866 /s² | 9.780 m/s² |
| jump apex (derived, v²/2g) | 1.478 | 4.51 m (too high; see below) |
| on-foot gravity used (`FOOT_GRAVITY`) | 4.5 /s² | 13.7 m/s² |
| standing / crouching height | 0.725 / 0.5 | 2.21 / 1.52 m |
| capsule radius | 0.175 | 0.533 m |
| standing / crouching eye height | 0.62 / 0.45 | 1.89 / 1.37 m |
| crouch transition | 0.2 s | |
| max walkable slope | 50° | |
| slope slow-down / speed-up | from 45°, cut off at 50°; uphill x0.5, downhill x1.5 | |
| field of view | 70° | |
| look rates | yaw 120°/s, pitch 60°/s | |
| hard landing from | 3.8 /s | 11.6 m/s |
| fall damage | from 6, fatal at 14 | 18.3 m, 42.7 m |

## 7. What was verified, and what was not

Verified against the bytes:

- **Offsets are stable**: every value above is identical on six maps (five multiplayer, one
  campaign), and the campaign map reaches a different biped tag with the same layout.
- **Ground physics**: the engine's own precomputed `cos(max slope)` at `0x39C` and the sines at
  `0x3A0`..`0x3AC` match the angles at `0x380`..`0x390`, which fixes those five angles and their
  order (max slope, downhill falloff/cutoff, uphill falloff/cutoff). The reader refuses a biped
  where the cosine disagrees.
- **Heights and radius**: both pill shapes span exactly 0 .. height with radius 0.175, so
  `0x340`/`0x344`/`0x348` are standing height, crouching height and radius.
- **Gravity**: the three falling-damage runtime speeds give the same g, 3.20866 wu/s². That is
  9.780 m/s² at 3.048 m/wu, real-world gravity, which also supports the 1 wu = 10 ft scale.
- **Player biped**: reached by tag reference from the globals, not guessed from the name.

Inferred from the shape of the tag and the values, not proven:

- **Gravity is an engine constant, not a tag field.** No tag stores g directly; it is recovered
  as above. It matches the figure long documented for Halo 1, 0.0035651 wu per tick² at 30 ticks
  per second (= 3.2086 wu/s²). Game-type traits ("gravity 50%" ...) scale it at run time.
- **Jump height** 1.478 wu (4.5 m) is v²/2g from the two numbers above, assuming the jump
  velocity is the whole take-off speed and nothing else scales it. **Too high**: that is 1.92 s in
  the air, and a 2011 Bungie.net measurement on the Halo 3 disc (forum post 3291870, default
  traits) gives about 1.4 s, with the feet rising about 1.45 Spartan heights (~1.05 wu). Both fit
  the tag's 3.08 wu/s jump with g ≈ 4.5 wu/s², so the Spartan's own gravity seems stronger than
  the falling-damage g. `halo3_foot` uses 4.5 wu/s² on foot (`FOOT_GRAVITY`: 1.37 s, 1.05 wu).
  The 4.5 is measured by eye, not read from the engine. A frame count of a standing jump in MCC
  would pin it down: g = 2 × 3.08 / airtime.
- **Walk speed** at player information `+0x00` (0.512): the field before "run forward" in Halo's
  player globals is the walking speed; Halo 3 players have no walk key, so its use is unclear.
- The order of the two **landing times** at `0x2CC`/`0x2D0`, and the names of `0x2E0`/`0x2E4`.
- **Field of view** 70°: the only 70° angle in the unit's camera fields, and the well-known Halo 3
  default. MCC on PC has its own FOV setting, which overrides this.
- **Look rates**: the 60/120 °/s pair sits where Halo's player control keeps the default
  pitch/yaw rates; the neighbouring field order follows the same layout and was only checked for
  plausible values.
- The four zero floats at biped `0x3B4`..`0x3C0` are probably the ground physics scale fields
  (airborne reaction time, ground adhesion velocity, gravity scale, airborne acceleration scale)
  with 0 meaning "default"; not confirmed, not read.

Not stored in tags:

- **Step-up height.** There is no step height field. Halo moves the biped as the pill shapes over
  the collision BSP: small steps are climbed because the capsule's round bottom rides up them,
  and anything flatter than the 50° maximum slope counts as ground. A stand-in for our
  controller: treat ledges up to about one radius (0.175 wu, 0.53 m) as steppable; this is our
  estimate, not a Halo number.
- **Tick rate**: Halo 3 simulates at 30 ticks per second (public, long known; not in a tag).
- **Multiplayer traits** (player speed, gravity, jump in custom games) live in the game variant,
  not in the map; at default they are 100%.
