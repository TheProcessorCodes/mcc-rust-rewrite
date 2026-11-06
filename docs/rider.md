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
