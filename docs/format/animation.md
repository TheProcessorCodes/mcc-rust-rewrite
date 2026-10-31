# Animation graphs (`jmad`, model_animation_graph), MCC Halo 3 U13

Written 2026-10-29 from the bytes of the user's own MCC maps (`120_halo.map`,
`shared.map`, `campaign.map`, `riverworld.map` and every other installed Halo 3
map). Reclaimer has no Halo 3 animation reader, so nothing here comes from its
source; the field meanings are worked out from the bytes and from what the
values do when decoded (a pose that looks like a pose, a root motion that
matches the run speed). Every layout below is checked by
`docs/format/scripts/probe_animation.py` (Python, read-only) and by
`crates/blam_anim/tests/maps.rs`. The reader is `crates/blam_anim`; the CLI is
`blam anim`.

Conventions: offsets are from the start of the structure, little endian. A
*block* is the usual 12-byte tag block (count u32, shifted pointer u32, 4
unused bytes), a *tagref* the 16-byte tag reference, a *sid* a 4-byte string
id (`scenario-and-tag-reading.md` 1.7).

## 1. Where the pieces are

- The object (`bipd`, `mach`, `vehi`, ...) names its model (`hlmt`) at
  `+0x34`; the model names its graph (`jmad`) at `hlmt+0x20` and its render
  model (`mode`) at `hlmt+0`.
- The `jmad` tag holds the skeleton names, the animation list, the action tree
  and a list of *resource groups*. The animation data itself lives in
  resources of type `model_animation_tag_resource` (`resources.md` 3.3): one
  resource per group, holding several animations (the group's *members*).
- Most groups of the campaign characters are in `shared.map` (page cache index
  2) or `campaign.map` (3); a few are in the map itself. On `120_halo` 976
  groups live in 570 shared, 328 campaign and 78 local pages; 10 groups have
  no page at all (the data is not shipped with that map: first-person weapon
  animations of weapons the level never gives you) and one id is stale.

## 2. `jmad` root (260 bytes)

| offset | type | field | check (`120_halo`, flood combat form #9417) |
|---|---|---|---|
| 0x00 | tagref | parent graph (`jmad`) | null; the dervish's points at the elite's |
| 0x10 | u8, u8, i16 | flags (bytes `00 65 06 00` on 169 of the 170 graphs, `02 65 06 00` on one; meaning unknown) | |
| 0x14 | block, 32 B | skeleton nodes | 34 @ `0x194BE44C` |
| 0x20 | block | sound references | 18 |
| 0x2C | block | effect references | 1 |
| 0x38 | block, 28 B | blend screens (aim grids, section 5) | 1 |
| 0x44 | block | (empty on every graph seen) | 0 |
| 0x50 | block, 136 B | animations | 150 @ `0x194BF23C` |
| 0x5C | block, 40 B | modes (the action tree, section 4) | 4 @ `0x194C4E34` |
| 0x68, 0x74 | block | vehicle suspension, object overlays (not read) | 0 |
| 0x80 | block, 48 B | inherited graphs (section 4.3) | 0 |
| 0x8C | block | weapon list (not read) | 11 |
| 0x98 | 32 bytes | left arm node bit flags | `1DD40800 00000003 0 ...` |
| 0xB8 | 32 bytes | right arm node bit flags | |
| 0xD8 | 20 bytes | data ref, zero in cache files (import results) | |
| 0xEC | block | additional node data (empty) | |
| 0xF8 | block, 12 B | resource groups | 16 @ `0x194C4F2C` |

The block counts of the three graphs used in the tests (tag, nodes /
animations / modes / groups): flood combat form #9417 34 / 150 / 4 / 16,
sentinel aggressor #9733 23 / 20 / 4 / 4, Master Chief #7901 51 / 730 / 21 /
190.

### 2.1 Skeleton node (32 bytes)

| offset | type | field |
|---|---|---|
| 0 | sid | name (matches the render model's node names) |
| 4 | i16 | next sibling (-1 none) |
| 6 | i16 | first child |
| 8 | i16 | parent |
| 10 | u8 | model flags |
| 11 | u8 | joint flags |
| 12 | f32 x3 | base vector (zero on the bipeds seen) |
| 24 | f32 | vector range |
| 28 | f32 | a per-node value, small (+-0.07 on the sentinel); meaning unknown |

Check, sentinel node 0: `D7440000 FFFF0100 FFFF0500 ...` = `root_fuselage`,
next -1, child 1, parent -1, flags 5 / 0. Node order is the same as the render
model's on every biped checked, but the reader matches nodes by name.

The graph has **no rest pose**: a node an animation does not drive keeps the
render model's rest transform (`mode+48`, 96-byte nodes, translation at +12,
quaternion at +24; `scenario-and-tag-reading.md` G3.3).

## 3. Animation (136 bytes)

| offset | type | field | notes |
|---|---|---|---|
| 0 | sid | name | `combat:any:any:move_front:var1`, `device:position` |
| 4 | f32 | weight | chance among variants; 0 on most |
| 8 | i16 | loop frame | 0 on most; 32, 40 on a few |
| 10 | u16 | playback flags | 0, 8, 12, 24 seen |
| 12 | i8 | blend screen index | -1 none; aim overlays name one |
| 13 | u8 | desired compression | 0 or 2 |
| 14 | u8 | current compression | 0 or 2 |
| 15 | u8 | node count | 23 on the sentinel, 51 on the Chief |
| 16 | u16 | frame count | |
| 18 | u8 | kind | 0 base, 1 overlay, 2 replacement |
| 19 | u8 | movement (frame info) | 0 none, 1 dx dy, 2 dx dy dyaw, 3 dx dy dz dyaw |
| 20 | u16 | production flags | |
| 22 | u16 | internal flags | `0x80` or `0xA0` |
| 24 | u32 | node list checksum | same for every animation of a graph |
| 28 | u32 | checksum | equals the member's checksum in the resource (section 6) |
| 32 | 4 bytes | zero | |
| 36 | i16 | previous variant | -1 none |
| 38 | i16 | next variant | |
| 40 | i16 | resource group | index into root `+0xF8` |
| 42 | i16 | member | index inside that group |
| 44 | 6 blocks | frame events, sound events, effect events, dialogue events, object-space parent nodes, foot tracking | not read |
| 116 | f32 x3 | heading (`1, 0, 0`) | |
| 128 | f32 | heading angle | |
| 132 | 4 bytes | zero | |

Check, sentinel animation 0 (dwords): `00001EA4 00000000 00000000 170000FF
00010003 00800000 15047240 0007FAAD 00000000 FFFFFFFF 00000002` =
`combat:acc_front_back`, weight 0, blend screen -1, compression 0/0, 23 nodes,
3 frames, overlay, no movement, internal flags `0x80`, checksum `0x0007FAAD`,
no variants, group 2 member 0. The kind histogram on the Chief's 730 (0 base
with movement for the `move_*` animations, 1 overlay, 2 replacement) is what
gave the meaning of byte 18.

Variants: an action names the first variant (`idle:var1`); the others follow
through the previous / next links and are chosen by weight (a weight of 0
throughout means equal chances).

## 4. The action tree

`mode -> weapon class -> weapon type -> actions / overlays`; an animation is
looked up as `mode:class:type:action`, e.g. `combat:rifle:ar:move_front`.

| structure | size | fields |
|---|---|---|
| mode | 40 | sid label @0, weapon classes block @4, mode IK block @16, a third block @28 (empty on the graphs seen) |
| weapon class | 28 | sid label @0, weapon types block @4, weapon IK block @16 |
| weapon type | 52 | sid label @0, actions block @4, overlays block @16, death-and-damage block @28, transitions block @40 |
| action / overlay | 8 | sid label @0, i16 graph @4, i16 animation @6 |

Check, flood mode 0: `0000008C 00000005 73CE16E7 ...` = `combat` with 5
weapon classes; its class 0 is `unarmed` (1 type `any`, 21 actions, 4
overlays, 2 death-and-damage entries); action 0 is `melee` = animation 104.

### 4.1 The graph index

`-1` means this graph. Any other value indexes the inherited-graph list
(section 4.3): on `120_halo` 1563 references carry graph 0 and every one of
them is in a graph that has an inheritance list (1471 of them name an
animation index past the referencing graph's own count, so they cannot mean
"this graph").

### 4.2 Fallbacks (the reader's rule)

Halo falls back when a stance has no entry. The reader tries the exact weapon
type, then `any`; the exact class, then `any`; the exact mode, then `combat`,
then `any`. E.g. the flood has no `rifle` class, so
`combat:rifle:ar:move_front` gives `combat:any:any:move_front:var1` (#42).
The game may use more rules (crouch to combat, vehicle seats); add them when
something needs them.

### 4.3 Inherited graph (48 bytes)

| offset | type | field |
|---|---|---|
| 0 | tagref | the graph inherited from |
| 16 | block | node map (empty on every graph seen) |
| 28 | block | node map flags (empty) |
| 40 | f32 | root z offset (`1.0`) |
| 44 | u32 | flags (`2`) |

Check: the dervish (#7516's graph) inherits `objects\characters\elite\elite`
and `objects\characters\elite\lipsync\lipsync`. With empty node maps the
reader matches nodes by name.

## 5. Blend screen (28 bytes)

| offset | type | field |
|---|---|---|
| 0 | sid | label (`blend_aim`, `aim_stand`, `blend_screen_#0`) |
| 4 | f32 | yaw range left (radians) |
| 8 | f32 | yaw range right |
| 12 | i16 x2 | yaw steps each side (1, 1) |
| 16 | f32 | pitch range up |
| 20 | f32 | pitch range down |
| 24 | i16 x2 | pitch steps each side (1, 1) |

Flood: `0x3F9C61AA` x2 = 70 degrees each way of yaw, `0x3FC90FDB` x2 = 90
degrees of pitch. With one step each side the aim overlay is a 3 x 3 grid of 9
frames. Which frame is where was read from the rotation axes of the flood's
`aim_still_up` spine deltas taken into model space: frame 4 is the centre
(identity), frames 3 and 5 turn about the vertical (yaw), frames 1 and 7 about
the sideways axis (pitch). Yaw varies fastest; columns run from right to left
(negative to positive yaw) and rows from down to up. The up/down reading is
the least certain part (a sign of the overlay rotation order, 10.2).

## 6. Resource group and its definition

Resource group (12 bytes, root `+0xF8`): i32 member count @0, resource id @4
(the usual salt/index id, `resources.md` 2), 4 zero bytes.

The resource's definition (`resources.md` 7, fixups applied) is the member
records, 48 bytes each, followed by a 12-byte block whose pointer (a type 1
cache address, offset 0) points back at the first member: count, pointer,
runtime value. Length = `48 * members + 12` (sentinel group 0: 8 members, 396
bytes). Each member's data address is a resource fixup (type 2, offset inside
the group's segment); one more fixup fills the trailing block's pointer.

Member (48 bytes):

| offset | type | field |
|---|---|---|
| 0 | u32 | zero (name, cleared in cache files) |
| 4 | u32 | checksum = the animation's `+28` |
| 8 | u16 | frame count |
| 10 | u8 | node count |
| 11 | u8 | movement kind (as animation `+19`) |
| 12 | u8 | static node flags size |
| 13 | u8 | animated node flags size |
| 14 | u16 | movement data size |
| 16 | u16 | (pill offset data size, always 0) |
| 18 | u16 | static codec data size |
| 20 | u32 | (uncompressed data size, always 0) |
| 24 | u32 | animated codec data size |
| 28 | 20 bytes | data reference: size @28, address @40 (a fixup writes `0x4000_0000` + offset) |

The sizes add up to the data reference's size on all 5881 animations with data on
`120_halo` (probe `--codecs`). Check, sentinel `crouch:idle:2:combat:idle`:
`... 23001701 0c0c1801 00004000 00000000 10060000 80070000` = 35 frames, 23
nodes, movement 1, flags 12 / 12, movement 280 (35 x 8), static 64, animated
1552, total 1920 = 64 + 1552 + 12 + 12 + 280.

## 7. Animation data layout

In this order, back to back:

1. static codec block (`static size` bytes; absent when 0),
2. animated codec block,
3. static node flags,
4. animated node flags,
5. movement data.

Each node-flags region is three bit arrays, rotation then translation then
scale, each `w` little-endian u32s; bit `n` set means node `n` has a track of
that kind in that codec block, and the tracks are stored in node order of the
set bits. `w = ceil(nodes / 32)` normally (12 bytes for the 23-node sentinel,
24 for the 51-node Chief, 60 for a 159-node cinematic rig), but some cinematic
animations of 116- and 134-node rigs carry only 24 bytes: they drive the first
64 nodes. The reader takes `w = size / 12`. A node in neither codec keeps the
pose under it.

Check, sentinel `h_ping:back:gut:var0` (536 animated bytes + 12): the flags
`E1C10000 00000000 00000000` name nodes 0, 5, 6, 7, 8, 14, 15 for the 7
rotation tracks, no translations, no scales.

Movement data: per frame, 2, 3 or 4 f32 (dx, dy[, dz], dyaw): the step from
this frame to the next in the object's own frame (world units, x forward, y
left; radians, left positive). The last entry is zero on the looping walks.
The bones themselves do not travel: the flood's pelvis stays near x = 0 over
`move_front` while the steps add up to 1.525 wu over 24 frames. The Chief's
`combat:rifle:move_front` steps 0.077 wu a frame = 2.3 wu/s at 30 frames a
second, Halo's run speed.

## 8. Codec blocks

Byte 0 of a block is the codec, bytes 1..3 the number of rotation,
translation and scale tracks. Values are always: rotation = 4 x i16 (x, y, z,
w) / 32767, normalised (8 bytes), or 4 x f32 in codec 8; translation = 3 x
f32 in world units; scale = 1 f32.

Histogram, `120_halo` (static, animated): codec 1 x4300 static; 3 x2830, 4
x1472, 6 x1131, 8 x440, 7 x7, 5 x1 animated. The same set on every installed
map; no other value occurs.

### 8.1 Codec 1, static (32-byte header)

| offset | field |
|---|---|
| 0..3 | codec, counts |
| 4, 8 | f32, f32 (zero) |
| 12 | u32 offset of the translations |
| 16 | u32 offset of the scales (= end) |
| 20..31 | zero |
| 32 | rotations, 8 bytes each |

One value per track. Check, sentinel `combat:any:any:idle` static block:
`01060200 0 0 50000000 68000000 0 0 0 | 0000FCF4 00007B80 ...` = 6 rotations
from 32, 2 translations from 0x50, end 0x68 = 104 bytes; the first rotation
is (0, -0.086, 0, -0.996).

### 8.2 Codecs 3 and 8, every frame (32-byte header)

| offset | field |
|---|---|
| 4, 8 | two f32 (error figures?) |
| 12 | offset of the translations |
| 16 | offset of the scales |
| 20 | rotation track stride = frames x 8 (codec 3) or frames x 16 (codec 8) |
| 24 | translation track stride = frames x 12 |
| 28 | scale track stride = frames x 4 |
| 32 | rotation tracks, one after another, each `frames` values |

Codec 3 stores rotations as i16, codec 8 as f32 (the aim overlays use 8).
Check, sentinel `combat:acc_front_back`: `03080000 a78e9938 9ed8093f e0000000
e0000000 18000000 24000000 0c000000` = 8 rotations, 3 frames: 32 + 8 x 24 =
224 bytes. `combat:any:any:aim_still_up`: `08040000 0 3F800000 60020000
60020000 90000000 6C000000 24000000` = 4 float rotations over 9 frames,
32 + 4 x 144 = 608. Codec 2 (never seen) is read like 3.

### 8.3 Codecs 4, 5, 6, 7, keyframes (48-byte header)

| offset | field |
|---|---|
| 4, 8 | two f32 |
| 12 | offset of the translation track headers |
| 16 | offset of the scale track headers |
| 20 | offset of the rotation key frame numbers |
| 24 | offset of the translation key frame numbers |
| 28 | offset of the scale key frame numbers |
| 32 | offset of the rotation values |
| 36 | offset of the translation values |
| 40 | offset of the scale values |
| 44 | zero |
| 48 | rotation track headers, then translation, then scale (u32 each) |

A track header is `count | first << 12`: the low 12 bits are the number of
keys, the high 20 bits the index of the track's first key in that kind's key
list and value list. Codecs 4 and 6 store key frame numbers as u8, 5 and 7 as
u16. In 6 and 7 the tracks' runs are laid out last node first (the `first`
indices descend), in 4 and 5 first node first; the headers say where each run
is, so all four decode the same way. Between keys the value is interpolated;
before the first and after the last it holds.

Check, scorpion cannon `combat:fixed:cannon:fire_1` (codec 4, 22 frames, 1
rotation, 1 translation): offsets `34 38 38 40 46 48 88 D0`, headers
`00000008` (8 keys from 0) and `00000006`, rotation keys `00 01 03 09 0A 0C 12
15`, translation keys `00 01 04 09 12 15`, 8 rotations at 0x48, 6
translations at 0x88 (the first is (-0.296, 0, 0)). Sentinel
`h_ping:back:gut:var0` (codec 6): headers `0001E015 0001C002 00018004
00011007 0000C005 00006006 00000006` = (21 keys from 30), (2 from 28), ...,
(6 from 0): 51 keys, 51 x 8 bytes of rotations from 0x80 to 0x218. Flood
carrier `combat:unarmed:idle:var2` (codec 5, 290 frames): u16 key numbers;
the flood carrier, the 120_halo control room hologram (codec 7, 480 frames)
and the frigate graphs decode to smooth curves.

## 9. Interpolation and looping (the reader's choices)

- Between frames: normalised linear blend of quaternions on the short arc, and
  linear for translation and scale. Halo plays at 30 frames a second.
- A looping animation's last frame repeats its first (the flood's
  `move_front:var1` frame 23 vs 0 differ by 1e-4 in the quaternion dot, far
  less than one frame's step), so the loop period is `frames - 1`.
- A device's `device:position` animation maps position 0..1 to frames
  0..`frames - 1`.

## 10. Poses

### 10.1 Base and replacement

Values are local to the parent node in the render model's convention:
glam/Hamilton quaternions (x, y, z, w), translation in world units. Check:
frame 0 of the flood's idle lands within 0.85..1.0 (|dot|) of the render
model's rest rotation on every driven node, and its pelvis 0.08 wu from the
rest pelvis. A base animation replaces the nodes it drives; replacement
animations (gripping hands, heads) do the same on top of a base.

### 10.2 Overlays

Overlay values are differences from a reference pose: the centre frame of the
flood's aim grid is exactly identity (`0, 0, 0, -1`) on every track, and its
translations are zero. The reader applies them as `rotation = pose * delta`
(in the node's own frame) and `translation = pose + delta`, scale multiplied.
The order of the rotation product is not proven by the data (both orders give
the authored result when the base is the reference pose); `pose * delta` is
the guess. The sentinel's aim centre has one node turned 5 degrees; the rest
are identity.

### 10.3 Device machines

A machine's tracks (`device:position`, 120_halo's island `tilt_up` / `other_fall`)
are marked overlays (kind 1) but hold whole node transforms, not differences:
the island's root sits at z 55.6 in `device:position` frame 0 and 55.8 in the
render model's rest pose; the frigate's root is at the rest translation
(-0.032, 0, 0) but turned a quarter turn about Z (`0, 0, 0.707, -0.707`) from
its rest rotation (identity), and its last frame is 845 wu away, turned half
round (it flies off). The game never shows a machine unanimated, so a machine
at rest stands in its `device:position` pose at position 0, laid over the
rest pose by the clip's kind (`AnimModel::pose_at`, `devices::place_machines`).
Taking the tracks as whole values instead sank the islands 56 wu below their
placement (the user saw them vanish); laid as overlays they stay within 0.3 wu
and the frigate still turns its quarter turn. For the frigate that
is the difference between hanging broadside across the Warthog run (the
model's nose is +Y) and pointing its open hangar, under the stern, at the
run's end, where the map's end trigger waits. Motion the scripts start is
measured from the tracks' first frames on top of that (`machine_pose`).

## 11. Pinned numbers (`cargo test -p blam_anim`)

| map | graphs | decoded | no data in the map |
|---|---|---|---|
| `120_halo` | 170 | 5881 | 115 (114 groups without a page, 1 stale id) |
| `riverworld` | 104 | 3522 | 2 stale |

Every other installed Halo 3 map decodes every animation that has data
(`blam anim <map> --all`; e.g. `100_citadel` 9661 of 9662, `005_intro` 3412
of 3412).

## 12. Open

- The two f32 after the counts in every codec header (error figures?), the
  flags at root `+0x10`, playback, production and internal flags, the node
  `+28` value, the mode's third block: unknown, not needed to play.
- Frame events (footsteps, sounds, effects), IK points, death-and-damage
  directions, transitions and the weapon list are not read yet.
- Overlay rotation order (10.2) and the aim grid's up/down (5) want a
  side-by-side with the real game.
- Halo's full action fallback rules (crouch to combat, vehicles, `any`
  stances) beyond section 4.2.
