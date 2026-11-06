# Effects in an MCC Halo 3 map (U13): `effe`, `prt3`, `pmov`, `cntl`, `ligh`

How Halo 3 stores its effects (explosions, sparks, smoke, flashes) and how our runtime plays
them. Written 2026-10-31 from the bytes of the user's `120_halo.map`, the Warthog run's own
effects first. No GPL code or plugin definition was used: the layouts were found by dumping
tags (`blam dump`) and walking their blocks with a small explorer, then pinned by reading every
effect in the map without an error and by values whose meaning is plain (a fireball that lives
0.5 to 1.5 s, a light that lasts 2 s, a white-to-orange tint, a `snd!` reference).

Code: `crates/blam_effect` (reader, no renderer), `crates/mcc_effects` (the runtime: `sim.rs`
plays, `draw.rs` and `shaders/particle.wesl` draw), the light term in
`crates/mcc_world/src/shaders/lightmap.wesl` (`effect_irradiance`).

Probe (`python -I`, read-only, in `docs/format/scripts/`):

| probe | what it shows |
|---|---|
| `probe_effects.py <map> [effe path]` | an effect and everything it reaches: events, parts, particle systems, emitters with every property and its function, particles with their render method and bitmaps, physics, contrails, lights |
| `probe_effects.py <map> --census` | the function kinds and curve segment shapes of every emitter in the map |

Pinned by `cargo test`:

- `crates/blam_effect/tests/run_effects.rs`: the plate explosion's five events, its parts and
  particle systems, the fireball emitter's numbers, the fireball particle's shader, the ejecta's
  attachment and contrail, the light; the giant explosion's events and sparks; every one of
  120_halo's 937 effects (and the 3,193 emitters in them) reads.
- `crates/mcc_effects/tests/trench_effects.rs`: the run's scripts blowing up plate `100_001c`
  ask for its chunks and `plate_explosion` at the plate, upright, and the player then makes one
  flash, 50 rising fireball particles, the two sounds, five to eight ejecta with contrails that
  follow them, and ends after the 5 s hold; `big_explosions_ambient` plays `giant_explosion` at
  points of `big_explosions_100_01`, then `big_explosions_100_02`.

All offsets below are from the start of the element or tag data; little endian. Tag blocks are
12 bytes (count, packed pointer, 4 bytes), tag references 16 (class, 8 bytes, datum), data
references 20 (size, 8 bytes, packed pointer, 4 bytes), as everywhere else in the map
(`scenario-and-tag-reading.md`).

## 1. What the Warthog run plays

| from | effect | what it is |
|---|---|---|
| `blow_away` (damage section `destroy` of a plate) | `objects\levels\solo\120_halo\plates\<plate>\<plate>_explosion_chunks` (or `_explosion01`) and `levels\solo\120_halo\fx\trench_run\plate_explosion\plate_explosion` | the plate's chunks (five `bloc` debris objects and a `jpt!`), then the flash, sparks, fiery smoke, fireball, light, ejecta with flaming contrails, a fiery plume, a shock wave and two sounds |
| `fall_off`, `fall_off_immediate` (sections `fall`, `fall_immediate`) | `<plate>_fall` | only a `bloc` (the plate itself as a physics object) and a `jpt!`: nothing to see; the plate's own fall is ours (`halo-script.md`) |
| `big_explosions_ambient` (every 15 to 20 s from `120_08_trench_encounter_100`) | `levels\solo\120_halo\fx\trench_run\giant_explosion\giant_explosion` at a random point of `g_location` | far off in the canyon: long sparks, an enormous fireball, ejecta, a screen effect (`sefc`), its sound 1.5 s later |
| `blow_away_flag` (`trench_run_110_event_01`) | `plate_explosion` at `blow_out_110/p0` | |
| `tunnel_fall01` | `objects\levels\solo\120_halo\plates\tunnel_pieces01` | one `bloc` (a half girder): nothing to see |
| `100_hw_explosions_loop`, `110_hw_explosions_loop` | `fx\scenery_fx\explosions\human_explosion_aerial\human_explosion_aerial` | never woken by any script |

A damage section response has two effect references (`+0x10` and `+0x20`, `halo-script.md` 7);
a plate's `destroy` uses both. Both are played.

## 2. Function blobs

Every property's function is a data reference to a blob:

| offset | field |
|---|---|
| 0x00 | u8 kind: 0 identity (the input itself), 1 constant, 3 periodic, 8 curve |
| 0x01 | u8 flags: bit 0 **ranged**: a second output, picked between by the range input |
| 0x02 | u8 colour stops: 0 for a scalar, 1 to 4 for a colour gradient |
| 0x04 | f32 minimum, or colour slot 0 (u32 `xRGB`, alpha byte unused) |
| 0x08 | f32 maximum, or colour slot 1 |
| 0x0C, 0x10 | colour slots 2 and 3 |
| 0x14, 0x18 | two floats (0.49 / 0.5 and similar on ranged curves; not used here) |
| 0x1C | u32 bytes of curve or wave data after 0x20 |
| 0x20 | the data |

The output is `minimum + (maximum - minimum) * t`, where `t` is the shaped input:

- **identity**: `t = x`.
- **constant**: `t = 0`; ranged, `t = range`. A plain constant has minimum = maximum.
- **curve**: a u32 segment count, then per segment a u32 shape (low byte; the other bytes are
  sometimes `0xCD` fill), an f32 end (the segment is used for inputs up to it; the last is
  `f32::MAX`) and its values. Shape 4: two values `c d`, `t = c x + d`. Shape 7: four, `t = a
  x^3 + b x^2 + c x + d`. Shape 10: seven, the first four the same cubic (the last three are
  not used here). The polynomial is in the input itself, not the segment's local input: the
  plate explosion fireball's intensity curve (`to 0.26: -67.14 17.21 0 1; then -3.669 10.09
  -9.168 2.75`) meets at 0.26 (0.983 and 0.984) only that way. A ranged curve has a second curve
  straight after the first; `t = lerp(first(x), second(x), range)`.
- **periodic**: 20 bytes per wave (u8 wave kind, f32 frequency, phase, minimum, maximum); two
  when ranged. Our reading of the wave kinds is rough (`function.rs`); none is used by the run.

Colour functions use the same `t` across their stops: one stop is slot 0; two stops run from
slot 0 to slot 3 (slots 1 and 2 hold stale numbers: the fireball's tint has `1.0f` in slot 1);
three from slot 0 through slot 1 to slot 3; four through all four. The explosive flash's
three-colour tint (`d62509 ffc258 000000 ffffff`, `t` falling from 1 to 0) goes white, orange,
red that way. Colours are read as 8-bit sRGB.

Census of 120_halo's emitters (`--census`): kinds (kind, ranged, stops) constant 22,401, ranged
constant 15,030, curve 5,949, ranged curve 1,014, colours 1,861 + 974 + ..., identity 210,
periodic 92 + 6; segment shapes 7: 7,881, 4: 3,102, 10: 148, nothing else.

## 3. Properties

A property is 32 bytes:

| offset | field |
|---|---|
| 0x00 | u8 input variable |
| 0x01 | u8 range variable |
| 0x02 | u8 output modifier (0 on every run effect; not used) |
| 0x03 | u8 output modifier input |
| 0x04 | data reference: the function blob (section 2) |
| 0x18 | f32 a constant the engine worked out (equals the function for constants) |
| 0x1C | u32 runtime flags |

A **vector property** (56 bytes) is a property followed by two `f32x3`, `a` and `b`; the value
is `lerp(a, b, property)`.

### 3.1 Input variables (particles)

Our reading, from how the emitters use them; the numbers that matter for the run are firm:

| value | meaning | evidence |
|---|---|---|
| 0 | the particle's age over its lifespan | size, scale, tint, alpha and black point of almost every emitter (2,500 to 3,100 each) |
| 1 | the event's age over its duration | starting count, rate, every constant |
| 2, 4 to 7, 15, 18 to 24 | random numbers per particle; the same number for the same variable | the fireball's lifespan, speed, radius and angle all use 4 (fast particles live longest) |
| 3, 8, 9 | random per emitter | ranges of emitter-wide constants |
| 10 | the event's age when the particle was made | lifespans |
| 11 | level of detail (1 here) | |
| 12 | game time | |
| 13, 14 | the effect's A and B scale (1 here) | max counts, sizes of effects that are scaled |

## 4. `effe`: an effect

| offset | kind | field |
|---|---|---|
| 0x00 | u32 | flags |
| 0x10 | f32 | (0.1) |
| 0x18 | i16 | loop start event (-1: does not loop) |
| 0x20 | block, 12 bytes | **locations**: string id marker name (`up`, `root`, `plate_01`...) |
| 0x2C | block, 0x44 bytes | **events** |
| 0x38 | tag ref | looping sound |
| 0x5C | block | (empty on the run's effects) |

The main struct is 0x68 bytes.

### 4.1 Event (0x44 bytes)

| offset | kind | field |
|---|---|---|
| 0x00 | string id | name (`flash`, `fireball`, `fiery_plume`, `rumble`, `event_4`) |
| 0x04 | u32 | flags |
| 0x0C | f32 | skip fraction |
| 0x10 | f32 x2 | delay bounds, seconds |
| 0x18 | f32 x2 | duration bounds, seconds (1/60 for a one-off burst; 5 s for a hold) |
| 0x20 | block, 0x60 bytes | **parts** |
| 0x2C | block | accelerations (empty on the run's effects; not read) |
| 0x38 | block, 0x5C bytes | **particle systems** |

### 4.2 Part (0x60 bytes): a tag made when the event starts

| offset | kind | field |
|---|---|---|
| 0x00, 0x02 | u16 x2 | create in (environment, violence mode) |
| 0x04 | i16 | location (index into the locations) |
| 0x06 | i16 | (-1) |
| 0x0C | fourcc | the tag's base class (`obje` for a `bloc`) |
| 0x10 | tag ref | **what**: `snd!`, `ligh`, `effe`, `cntl`, `bloc`, `jpt!`, `sefc`, `decs` ... |
| 0x20 | f32 x2 | velocity bounds (world units a second; debris) |
| 0x28 | f32 x2 | velocity yaw and pitch, radians, in the location's frame (pitch pi/2 is straight up: the plate chunks) |
| 0x30 | f32 | velocity cone half-angle, radians |
| 0x34 | f32 x2 | angular velocity bounds, radians a second |
| 0x3C | f32 x2 | radius modifier bounds |
| 0x44 | f32 x3 | origin offset |
| 0x50 | f32 x2 | origin rotation |
| 0x58, 0x5C | u32 | A / B scales values |

### 4.3 Particle system (0x5C bytes)

| offset | kind | field |
|---|---|---|
| 0x04 | tag ref | the particle (`prt3`) |
| 0x14 | i32 | location |
| 0x18 | u32 | 1 when the particles move with what made them (flashes on aerial explosions) |
| 0x20 | i16 | sort bias |
| 0x22 | u16 | flags |
| 0x40 | f32 | level-of-detail distance (20 wu on the fireball) |
| 0x44, 0x48 | f32 | LOD feather in / out |
| 0x4C | block, 0x2F0 bytes | **emitters** |
| 0x58 | f32 | (1.5) |

### 4.4 Emitter (0x2F0 bytes)

| offset | kind | field |
|---|---|---|
| 0x00 | string id | name (`emitter_0`) |
| 0x04 | u8 | emission shape (0, 1, 2, 4, 5, 7, 9, 10 seen; see 9.2) |
| 0x08 | f32 | bounding radius the engine worked out |
| 0x18 | vector property | translational offset |
| 0x50 | vector property | relative direction: a direction vector (`(1,0,0)`, `(0,0,1)`...) or zero for the default axis |
| 0x88 | property | emission radius, world units |
| 0xA8 | property | emission angle, degrees |
| 0xC8 | property | a second angle, degrees (0 to 45 ranged on many emitters; not used) |
| 0xE8 | property | **starting count**: particles made when the event starts (1 flash, 50 fireballs, 20 sprays, 64 smokes, 5 to 8 ejecta, 60 sparks) |
| 0x108 | property | maximum alive (0: no limit) |
| 0x128 | property | **emission rate**, particles a second while the event lasts (thrusters 30 to 50, smoke trails 5 to 15) |
| 0x148 | property | **lifespan**, seconds |
| 0x168 | tag ref | physics template (`pmov`) |
| 0x178 | u32 | physics flags |
| 0x17C | block, 0x18 bytes | the emitter's own movements (section 6) |
| 0x188 | vector property | self acceleration |
| 0x1C0 | property | initial speed, world units a second |
| 0x1E0 | property | rotation, turns |
| 0x200 | property | rotation rate, turns a second |
| 0x220 | property | **size**, world units across |
| 0x240 | property | scale (times the size) |
| 0x260 | property | **tint** (colour) |
| 0x280 | property | alpha |
| 0x2A0 | property | alpha black point |
| 0x2C0 .. 0x2EF | | engine-built blocks and counts |

The starting count / maximum / rate reading was settled by their values: read the other way
round, every explosive flash would have made up to 45 flashes.

## 5. `prt3`: a particle

| offset | kind | field |
|---|---|---|
| 0x00 | u32 | flags |
| 0x04 | block, 0x14 bytes | **attachments**: tag ref (an `effe`, `snd!`, ...) and u8 trigger at +0x10 (0: while it lives, 1: when it hits something, 2: when it dies; our reading of the values seen) |
| 0x10 | u32 | appearance flags |
| 0x14 | u16 | billboard: 0 screen facing, 1 camera facing, 2 along its velocity (sparks, sprays), 3 facing its velocity, 4 vertical, 5 horizontal |
| 0x18, 0x1A | i16 | first sequence and sequence count of the base map's sprite sheet |
| 0x1C | f32 | (-0.35 on sparks; not used) |
| 0x20 | f32 | centre offset along a stretched quad (-0.5: the head leads) |
| 0x3C | render method | `rmdf shaders\particle`: options at +0x10, parameters at +0x1C, the template and its textures in the block at +0x28, laid out like a shader's (`shaders.md`); `blam_texture::shader::resolve_render_method` reads it |
| 0x7C | property | aspect (height over width; the length of a stretched quad) |
| 0x9C | property | colour |
| 0xBC | property | intensity |
| 0xDC | property | alpha |
| 0xFC | u32 | animation flags |
| 0x100 | property | frame index, 0..1 across the sprite frames |
| 0x120 | property | animation rate |
| 0x140 | property | palette row (for palettized albedo) |
| 0x160 | tag ref | model (`pmdf`; mesh particles, not drawn here) |

The particle render method's options: `albedo` (diffuse_only, diffuse_plus_billboard_alpha,
palettized, palettized_plus_billboard_alpha, diffuse_plus_sprite_alpha,
palettized_plus_sprite_alpha), `blend_mode` (additive, add_src_times_srcalpha, alpha_blend,
...), `depth_fade`, `black_point`, `frame_blend`, `render_targets`, `fog`, `lighting`,
`specialized_rendering`, `self_illumination`. Its maps: `base_map`, `alpha_map`, `palette`.
The run's particle bitmaps are A8R8G8B8 with the linear colour curve; palettes are 256 x 32
gradients (`explosion_animated`: black at the left, orange in the middle, near white at the
right, darker and redder row by row).

The ejecta and fiery plume particles draw almost nothing (a white 1x1 or a debug bitmap at size
0): they carry an attachment, an effect whose only part is a contrail.

## 6. `pmov`: particle physics

| offset | kind | field |
|---|---|---|
| 0x00 | tag ref | template (another `pmov`; `_default` points at `_nil`) |
| 0x10 | u32 | flags |
| 0x14 | block, 0x18 bytes | movements |

Movement (0x18 bytes): u16 kind at 0x00 (0 physics, 1 and 7 also seen), a block of 0x24-byte
parameters at 0x04 (u32 id, then a property), and at 0x10 a u32 mask of the ids present (7 for
ids 0, 1, 2). For kind 0: **0 gravity scale, 1 drag, 2 not used here** (our reading:
`explosion_fireball` has -0.2 and 3, a fireball that rises and slows; `_default` 1 and 2;
`small_rocks` 1 and 0.05). An emitter's own movements (`+0x17C`) are used when present, else its
template's.

## 7. `cntl`: contrails

The tag is one block (`+0x00`) of 0x26C-byte contrails (the block's data sits before the tag's
own 12 bytes):

| offset | kind | field |
|---|---|---|
| 0x00 | string id | name |
| 0x04 .. 0x13 | f32 x4 | (1, 1, 15, 10 and 0, 5, 15, 10: fade ranges, not used) |
| 0x24 | property | points a second (25 on the plate ejecta, 10 on the giant's) |
| 0x44 | property | point lifespan, seconds (1 to 0.75; 3 to 2) |
| 0x64 | vector property | (zero) |
| 0x9C | property | width over a point's life (0 to 0.35 wu; 0.15 to 2) |
| 0x130 | render method | `rmdf shaders\contrail`: `albedo`, `blend_mode`, `black_point`, `fog`; `base_map` `fx\contrails\_bitmaps\flaming_trail` |
| 0x170 | f32 x2 | texture repeats (0.75, 1) |
| 0x180 | property | colour (three stops of yellow and orange) |
| 0x1A0 | property | alpha over a point's life |
| 0x1C0 | property | intensity |
| 0x1E0 | property | black point (0 to 0.75) |
| 0x248 .. | | engine-built blocks |

Contrail properties read input 7 for colour, intensity and black point; we take it as random
per contrail (our reading). Input 0 is a point's age, as for particles.

## 8. `ligh`: lights

| offset | kind | field |
|---|---|---|
| 0x00 | u32 | flags |
| 0x04 | u16 | 0 sphere, 1 spot (flashlights) |
| 0x08 | f32 | size, world units (6 on the plate explosion, 2 on a large human explosion, 7 to 8 on flashlights) |
| 0x14 | f32 | spot field of view (radians) |
| 0x24 | property | colour (`ffc258` to `fba638` orange) |
| 0x48 | property | intensity over the light's life (20 falling to 0 on the plate explosion) |
| 0x60 | tag ref | gel map (bitmap) |
| 0x70 | f32 | duration, seconds (2 on the plate explosion; 0.01 on muzzle flashes) |

How Halo turns size and intensity into a falloff is not decoded (9.5).

## 9. How we play them (`mcc_effects`)

### 9.1 Events

Events play **one after another**: event n starts its delay after event n-1 ends, and lasts a
duration picked in its bounds (the same rule as Halo CE's effects; Halo 3 keeps the
fields). Skipped by their skip fraction. An effect ends when its last event has ended and
nothing it made lives. Loops (`+0x18`) are not played: the scripts' effects are one-offs.

Parts at an event's start: `snd!` goes to the sound side as an impulse at the effect
(`mcc_core::script_sound`), `ligh` becomes a light flash, `effe` a child effect at the same
place, `cntl` a contrail on the effect. **Not played**: `bloc` debris (the plate chunks, the
tunnel half girders), `jpt!` (shock waves, camera shake), `sefc` screen effects, `decs` decals.

### 9.2 Emitters

At the event's start an emitter makes its starting count; while the event lasts, its rate.
Each particle gets a random seed (so its random inputs, section 3.1), a lifespan, then:

- the **axis**: the effect's z (up; or the relative direction when it is not zero). Effects from
  the scripts' points and flags are upright; a damaged object's effects take the object's axes.
  Our reading: with x as the axis the plate explosion's narrow fiery plume (0 to 15 degrees, 25
  wu/s) would lie on its side, and the parts' velocity pitch of pi/2 means "up" in the same
  frame.
- a **direction** in the cone of the emission angle round the axis;
- a **place**: out along that direction at the emission radius (on a disc or ring round the axis
  for shapes 1 and 5), plus the offset;
- a **speed** along the direction.

Each frame: gravity (`GRAVITY` 3.21 wu/s^2 times the gravity scale, down), self acceleration,
drag (`v *= exp(-drag * dt)`), move, age; gone at the end of its life. Particles carry their
attachments: trigger 0 starts the effect riding on the particle (the ejecta's contrails), 2 at
its death.

### 9.3 Contrails

While the effect carrying a contrail plays and rides on a live particle, a point is left at the
contrail's rate, each with its own lifespan; the newest point follows the carrier. Points stay
where they were left. Drawn as a ribbon facing the camera, as wide as the width property at
each point's age.

### 9.4 Drawing

One mesh per emitter (and per contrail), rebuilt each frame. A quad per particle, `size *
scale` across, `aspect` times that high (or long, along the velocity), turned by rotation and
rotation rate, coloured `tint * colour * intensity`, alpha `alpha * alpha` (emitter times
particle), sprite frame from the frame index plus the animation rate over the life. The shader
does the albedo options: palettized looks up the palette at the base map's red (sampled linear;
it is a coordinate) and the palette row; the billboard alpha map is sampled by the quad's own
corner, the sprite one by the sprite UV; then the black point (as the sky shader's) and the
blend mode (additive and add_src_times_srcalpha added; alpha blend, pre-multiplied, multiply).
Not done: depth fade (soft edges where a particle meets a surface), frame blend, fog on
particles, distortion, mesh (`pmdf`) particles, lit particles.

Brightness is **our choice**: the particles' colour times 3 (`MCC_EFFECT_EXPOSURE`). With the
world's exposure (x8 on 120_halo, which makes up for our lightmaps' scale) every fireball was a
white blob; at 1 they were faint. How Halo weighs particles against its lightmaps is not
decoded.

### 9.5 Lights

A `ligh` part becomes a bevy point light for its duration with the light's colour and intensity
over its life. The world shader adds `albedo * colour * intensity * 0.05 * f^2` (f = 1 -
(d / r)^2, half wrapped round the surface normal) to the lightmap's irradiance, with `r` = 4 x
the light's size. The 0.05 and the 4 are ours: Halo's falloff is not decoded.

## 10. Not decoded / not done

- `bloc` debris parts: the plate chunks flying up (5 to 7 wu/s, spinning 14 to 17.5 rad/s) are
  read (velocity, cone, spin) but not drawn; they need the objects' render models at run time.
- `jpt!` (camera shake, shock waves), `sefc` (screen effects), `decs` (decals).
- The part fields' locations: every effect plays at the object's origin; markers are not looked
  up (the plates have none named `base_effect` anyway).
- Particle depth fade, frame blend, mesh particles, distortion; emission shapes beyond disc and
  ring; the second emitter angle (`+0xC8`); output modifiers; looping effects.
- The weapons' and AI's own impact and muzzle effects still use their stand-ins
  (`mcc_weapons/src/fire.rs`); switching them to `EffectQueue` needs their effect tags read at
  load, which is not done.
