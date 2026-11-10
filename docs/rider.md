# The rider: drawing a Spartan the skate engine can pose

Design note written 2026-10-27, before implementation. The skate engine returns a posed
skeleton of 35 named bones (`docs/skate-interface.md` section 4); the MW2 mashup draws it by
retargeting those bones onto the MW2 soldier's skeleton (`rig.rs`). Here the body is the
Halo 3 multiplayer Spartan.

## The model

`objects\characters\masterchief\mp_masterchief\mp_masterchief` (`mode`, tag 8096 in
`riverworld.map`; the `bipd` -> `hlmt` chain is `objects\characters\masterchief\mp_masterchief\mp_masterchief`).
From its header (offsets per `crates/blam_geometry/src/render_model.rs`):

| field | value |
|---|---|
| regions (`+12`) | 8 |
| instanced geometry section (`+28`) | -1 (none) |
| nodes (`+48`) | **51** |
| marker groups (`+60`) | 27 |
| shaders (`+72`) | 427 (every armour piece has its own) |
| sections (`+104`) | 279 (all permutations; the first permutation of each region is the default Mark VI) |
| bounding boxes (`+116`) | 1 |
| node maps (`+176`) | 0 (skinned sections index the model's nodes directly) |

Each node (96 bytes): `+0` name (string id), `+4` parent i16, `+6` first child, `+8` next
sibling, `+12` position f32x3, `+24` rotation quaternion xyzw, `+40` inverse scale f32,
`+44` inverse transform 3x4 (the inverse bind matrix, row-vector layout), `+92` distance
from parent. `render_model.rs` already builds node world transforms from position and
rotation; the inverse bind matrices can be taken from `+44` or computed as the inverse of
the world transform (check they agree on node 0).

Skinned sections use vertex format `0x02` (`s_skinned_vertex`, 44 bytes): the rigid layout
plus `+0x24` four blend indices (u8) and `+0x28` four blend weights (u8 / 255). Weights of
a vertex sum to 1; an index of 0 with weight 0 is unused. Rigid sections carry a `node_index`
instead (one bone, weight 1).

## bevy 0.20 skinning

- Mesh attributes: `Mesh::ATTRIBUTE_JOINT_INDEX` (`Uint16x4`) and
  `Mesh::ATTRIBUTE_JOINT_WEIGHT` (`Float32x4`).
- Component `bevy::mesh::skinning::SkinnedMesh { inverse_bindposes: Handle<SkinnedMeshInverseBindposes>, joints: Vec<Entity> }`;
  `SkinnedMeshInverseBindposes` is an asset built `From<Vec<Mat4>>`. Joint `i` of the
  mesh is `joints[i]`, an entity whose `GlobalTransform` is the bone's world pose; bevy
  multiplies `joint_world * inverse_bindpose[i]` on the GPU. `MAX_JOINTS` is 256.
- The joints are ordinary entities in a parent/child hierarchy with `Transform`s. For a bind
  pose, spawn the 51 nodes as children following the parent indices with the node's local
  position/rotation; for a skate pose, write each bone's world matrix straight into its
  entity's `Transform` (flatten the hierarchy, or set `GlobalTransform` on root-level
  entities).
- Frame: bevy is Y up, the model is Z up; the conversion is the same rotation the world
  uses (`world::to_render`), applied as the root entity's transform with the 3.048 m scale.

## Plan

1. `vertex.rs`: decode blend indices and weights for `Skinned`; `render_geometry.rs`
   carries them in `Vertices` (`Option<Vec<[u16; 4]>>`, `Option<Vec<[f32; 4]>>`).
2. `render_model.rs`: return the node list (name, parent, local position/rotation, inverse
   bind) and, per piece, either a bone index (rigid) or the skin arrays.
3. `scene.rs`: `Mesh.skin: Option<Skin>` and `Scene.skeletons`; a `Placement` may name a
   skeleton.
4. `mcc-rust-rewrite --model <mode tag path>`: opens a map, decodes one render model, shows it in bind
   pose with its bones as small gizmo axes (gizmos are enabled); `--bones` prints the node
   tree with names. This checks skinning before any skate code exists.
5. Retarget table: Halo node names -> the 35 skate bones (`rig.rs` has the MW2 table;
   expect `pelvis`, `spine`, `spine1`, `neck`, `head`, `l_clavicle` ... on the Spartan; the
   `--bones` listing decides). Same "copy the parent" trick for head/neck.
6. Pose path: the skate `Pose.bones[name]` -> retarget -> joint entity transforms, every
   frame, plus the board from `board.json` (its mesh is in the Skate 3 assets, not Halo).

Open: whether `+44` inverse transforms include the inverse scale; whether any Spartan
section uses node maps on other maps (campaign bipeds); how the skate pose's scale `k`
(`docs/skate-interface.md` section 10) applies to the rider (draw at Spartan size: divide
bone positions by `k`).

## Findings (2026-10-27, `blam nodes riverworld "mode:objects\characters\masterchief\mp_masterchief\mp_masterchief"`)

51 bones, Z up, model units (the head bone sits at 0.609 wu = 6.1 ft). The default
permutations give one skinned body section (4,098 triangles, joints 0..50) and three rigid
pieces: section 1 on `head`, sections 2 and 3 on `l_upperarm` / `r_upperarm`. Bone chains
run along local +X (each child is offset by +X of its parent), like IW4's joints.

Proposed retarget onto the 35 skate bones (same shape as `rig.rs::mapping`):

| Halo bone | skate bone | aligned toward |
|---|---|---|
| `pelvis` | HIPS | `spine` -> SPINE |
| `spine` | SPINE | `spine1` -> SPINE1 |
| `spine1` | SPINE1 (and SPINE3) | `neck` -> NECK |
| `neck` | NECK | `head` -> HEAD |
| `head` | HEAD | terminal |
| `l_clavicle` / `r_clavicle` | LEFTSHOULDER / RIGHTSHOULDER | `*_upperarm` -> LEFTARM / RIGHTARM |
| `l_upperarm` / `r_upperarm` | LEFTARM / RIGHTARM | `*_forearm` -> LEFTFOREARM / RIGHTFOREARM |
| `l_forearm` / `r_forearm` | LEFTFOREARM / RIGHTFOREARM | `*_hand` -> LEFTHAND / RIGHTHAND |
| `l_hand` / `r_hand` | LEFTHAND / RIGHTHAND | terminal |
| `l_thigh` / `r_thigh` | LEFTUPLEG / RIGHTUPLEG | `*_calf` -> LEFTLEG / RIGHTLEG |
| `l_calf` / `r_calf` | LEFTLEG / RIGHTLEG | `*_foot` -> LEFTFOOT / RIGHTFOOT |
| `l_foot` / `r_foot` | LEFTFOOT / RIGHTFOOT | `*_toe` -> LEFTTOEBASE / RIGHTTOEBASE |
| `l_toe` / `r_toe` | LEFTTOEBASE / RIGHTTOEBASE | terminal |

Fingers (30 bones) are unmapped and inherit the hand. Halo has only two spine bones to the
skater's four, so `spine1` takes SPINE1 and SPINE3 is skipped; `neck`/`head` can copy their
parent as IW4L does if the neck twists badly.

## Without Skate 3 (2026-11-09, #1)

The 35 skate bones and their bind poses come from Skate 3's `rig.json`, which setup makes from
the player's Skate 3. A player who skipped Skate 3 has none, so the game makes the rig from
Master Chief himself as each map loads: `mcc_rider::chief_rig` (`crates/mcc_rider/src/halo_rig.rs`)
builds the `mcc_core::skate::HaloRig` resource from the model the map load already decodes, in
memory, with no Skate 3 numbers. The rider, the Spartan on foot and the bike rider pose it in
place of `rig.json`. Skating is off without Skate 3, so the board never poses it.

27 bones, under Skate 3's names, in skate metres at scale `k`:

| bones | bind |
|---|---|
| the 21 in `MAPPING` (`mcc_rider/src/lib.rs`; there `spine` takes SPINE and `spine1` SPINE2, not as the table above proposed) | `T(F·H·w)`: a place on his own joint, no turn |
| SPINE1, SPINE3, NECK1 | places halfway along `spine`-`spine1`, `spine1`-`neck`, `neck`-`head` |
| SKATEBOARD_ROOT, TRUCK_FRONT, TRUCK_BACK | identity; foot and bike put them under him every frame |

`w` is the Halo node's joint in model space (units, Z up, facing +X), `H` is `halo_to_skate(k)`
(to skate metres, Y up), and `F` is the quarter turn about Y that the retarget falls back to for
his facing, so he faces +Z with his left at +X, as the skater does. The model's origin is
between his feet, so his soles are at y = 0.

Why the retarget is then exact (matrices applied right to left):

- A skinned Halo bone is drawn with `P · B⁻¹ · fit · H` on top of his own bind: `P` is the skate
  bone's pose this frame, `B` its bind, and `B⁻¹ · fit · H` the offset `follow` works out once.
- Off the board, foot and bike write `P = X · B`, or `[turn · R_B | p]`, which is the same with
  `X = T(p) · turn · T(-b)`. So the bone is drawn with `X · fit · H` and `B`'s turn never shows:
  a bind with no turn loses nothing, and only the joints' places `b = F·H·w` matter. Other
  crates read bone positions only, never their axes.
- `follow`'s facing compares his heel-to-toe line, flattened, `H·(toe - foot)`, with the rig's,
  `F·H·(toe - foot)`. A turn about Y keeps a flat line flat, so the facing comes out as `F`.
- For each mapped bone the segment to its child, turned by the facing, is `F·H·(child - w)`,
  and the rig's is `F·H·child - F·H·w`, the same line, so the extra turn is none and the bone's
  turn is `F`. Its place is `F·H·w - F·(H·w) = 0`, so `fit = F`. The flat-feet case drops y
  before the same sum and gives `F` too (the toes move with their foot); the head and hands
  take their parent's segment and give `F` the same way.
- So in the bind pose (`X = I`) every bone is drawn with `F · H`, one rigid move for the whole
  body: he looks exactly as his model has him, in his own proportions. A pose moves each bone
  by its `X` about his own joint. `the_retarget_leaves_him_as_he_is` checks this on a made-up
  biped.

With `rig.json` the binds sit on the skater's joints and carry Skate 3's turns, so `fit` moves
each of his bones onto the skater's instead. Skate 3 owners keep that rig on foot and on the bike.
On the board the Halo rig would not do: the skate engine poses whole bone frames against Skate
3's own binds. It is never asked to, as skating needs Skate 3 and the Halo rig is made only
without it.

`halo_rig` refuses, naming the bone, a joint that is missing or not finite, a mapped segment
under 1 mm and a limb segment under 0.01; the map then has no rider drawn, but walking and riding
still work. `crates/mcc_rider/tests/halo_rig.rs` builds the rig from riverworld's and
120_halo's Master Chief and checks it with ranges only (hips, head, toes, leg length, left and
facing), skipping when MCC is not set up.
