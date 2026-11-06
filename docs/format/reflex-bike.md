# The Reflex bike: Dakumoto 450F in the Cortana kit

The dirt bike is drawn with MX vs ATV Reflex's own Dakumoto 450F, read at start-up from the
user's Reflex install (`crates/reflex_assets`, drawn by `crates/mcc_bike/src/model.rs`).
Nothing from Reflex is in this repo: the code finds the install, reads it read-only and keeps
the result in memory. Without Reflex the bike is the old stick model.

The formats below were worked out with the Reflex tools (the mw2-to-reflex pipeline's `pkg.py`,
`bxml.py`, `rtex.py` and the Master Chief skin's `surf.py`, `rmodel.py`); this is our own
write-up of the facts, checked against the user's install (Steam, 2026-10-29).

## Finding Reflex

`$REFLEX_GAME` (a developer override), else `steamapps\common\MX vs ATV Reflex` under Steam's
own folder and every library in `steamapps\libraryfolders.vdf`. An install counts when it has
`Database\MXs.dx9.package`. When the first-run setup lands (`first-run-setup` branch) it should
save this path with the MCC and Skate 3 ones; Reflex stays optional.

## Database and package pairs

`Database\<name>.dx9.database` is BXML (Reflex's binary XML): nine u32 (`BXML`, version,
string count, pool offset, pool size, attribute count, node count, unknown, packed size), the
body zlib-packed when version bit 16 is set. The body: NUL-terminated strings, a value pool,
12-byte attributes `{name, value, flag u16, type u16}`, 32-byte nodes `{name, value, flag, type,
first child, child count, first attribute, attribute count}`. Type 1 values are string indices;
the others are pool offsets (3 i32, 4 u32, 5 f32).

The tree is `Packages/Package(name, offset)/Asset(name, type)/Heap(offset, size)`, with a
`Compress` child when the asset is packed. Its bytes are at `Package.offset + Heap.offset` in
the `.dx9.package`. A `Package` is one model or one colourway (a "subpackage" below).

A packed asset: u32 where the chunks end, chunks `{u32 size, u32 packed size, bytes}`, then u32
the real size (stock files often give only the bytes used in the last 64 KiB chunk; add the
full chunks before it). A chunk whose two sizes match is stored; the others are **Xbox XMem LZX,
128 KiB window**, one stream per chunk, in XNB-style frames: `FF` + u16 BE frame size + u16 BE
block size, or u16 BE block size with a 32 KiB frame. The `lzxd` crate decodes them; checked
byte for byte against the Reflex tools' helper on the 450F's surface, model and a texture.

## Assets

- `.surface` (`00 CC BB 99`): a 0x160-byte record per group (+8 skinned flag, +12 primitive
  type, vertex count, index bytes, +24 a D3DVERTEXELEMENT9 declaration, +188 vertex bytes,
  +252 a u8 bone palette size), then per group the palette, u16 indices (a strip; a list for
  type 4) and the vertices. Skinned groups are in bind pose in model space; the rest are in
  their bone's space.
- `.model` (`00 BB AA 88`): u32 0x0C bone count, 0x14 LOD count. Bone matrices from 0xAC (16
  f32, row vectors, row 3 the translation), 44-byte bone records `{name offset, parent, ...}`,
  names. The material slot of each group is among the last strings before the final
  `5 × groups` bytes; per-group `{slot, group, bone, 0, 0}` records give each group's bone.
- `.texture` (`00 CC DD 77`): 0x28 mips, 0x2C width, 0x30 height, 0x34 format (`DXT1`, `DXT3`,
  `DXT5` or D3DFMT 0x15 / 0x16), mip table at 0x38, data from `max(0x80, 0x38 + 8 × mips)`.
- `.material` is BXML; its `Texture textureName` nodes list the textures. The colour map is
  the last one that is not mud, a mask (`_SFX`, `_Combo`), a normal map or the number sheet.

Model space is feet, +Y up, +Z forward, +X the rider's right (left-handed). `reflex_assets`
turns it into the bike frame: metres, +X the rider's left, triangles re-wound.

## What makes the bike

| piece | package / subpackage / asset |
|---|---|
| bike, skeleton | `HR_MXs` / `HR_NeptuneF` / `HR_MX_NeptuneF` (else `MXs` / `NeptuneF` / `MX_NeptuneF`); the `MX_Stand*` groups (the garage stand) are left out |
| graphics kit | `HR_MX_GraphicKits` / `HR_MX_NeptuneF_THQ_Green` (else without `HR_`); the plastics material's sheet, 2048 × 1024 (1024 × 512); plastics and seat both draw with it |
| tyres and rims | `HR_MX_Tires` / `HR_MX_Tire_THQ` / `HR_OBJ_FRT_Tire_THQ`, `HR_OBJ_BCK_Tire_THQ`, on the `OBJ_FRT_Tire` / `OBJ_BCK_Tire` bones |
| pipe | `MX_Exhausts` / `MX_Exh_StockF` / `MX_Exh_Stock4`, finish from `MX_Exh_StockF_Alm` |

18 bones: `OBJ_Frame`; `OBJ_UPR_Forks` (bars, triple clamps, front fender, levers, the hands)
→ `OBJ_LWR_Forks` → `OBJ_FRT_Tire`; `OBJ_Swingarm` → `OBJ_BCK_Tire`; `OBJ_BCK_Fender`; the
`NDE_*` nodes mark the hands, feet, rider and fender wobble.

## Measured (bind pose, metres from the model origin, bike frame)

| | |
|---|---|
| front axle | (0, -0.252, 0.697) |
| rear axle | (0, -0.266, -0.764) |
| wheelbase | 1.461 m |
| tyre radius (to the knobs) | front 0.345, rear 0.335 |
| ground under the origin | 0.60 m (both tyres) |
| steering axis | through (0, 0.329, 0.338), 29.2° back from vertical (the triple clamp's Y) |
| fork legs | 0.683 m down the axis from the clamp, 2.3 cm ahead of it |
| swingarm pivot | (0, -0.147, -0.212) |
| grips (`NDE_*_Hand`) | left (0.292, 0.600, 0.178), right (-0.285, 0.590, 0.167) |
| pegs (`NDE_*_Foot`) | left (0.165, -0.143, -0.208), right (-0.201, -0.157, -0.214) |
| lowest frame / engine point | -0.30 (0.30 m ground clearance) |
| overall | 0.81 wide (bars), 1.25 tall, 2.14 long |

`cargo test -p reflex_assets -- --nocapture` prints these from the install. Reflex's own
physics file for the bike (`mx450f_vehicle.json`) gives tyre radii
1.17 / 1.04 ft (0.357 / 0.317 m); the drawn tyres are 0.345 / 0.335, and the physics uses the
drawn ones so the tyres sit on the ground.

## The physics fitted to it (Reflex v1)

`crates/mcc_bike/src/reflex.rs` (`DAKUMOTO_450F`) and `physics.rs`:

| | Unleashed Pro 250 (before) | now (the 450F) |
|---|---|---|
| wheelbase | 1.57 m | 1.46 m |
| tyre radius | 0.366 m both | 0.345 front, 0.335 rear |
| axles under the centre of gravity | 0.234 / 0.212 m | 0.252 / 0.266 m |
| rake | 26.9° | 29.2° |
| bar half width | 0.40 m | 0.29 m |
| pegs (frame) | (0.19, -0.25, -0.12) | (0.183, -0.285, -0.211) |
| hips (frame) | (0, 0.55, -0.30) | (0, 0.515, -0.39), moved with the pegs |
| skid plate probe | y -0.40, z 0.05 | y -0.30, z 0 (0.30 m clearance) |
| fender probes | z -1.08 / 0.98 | z -0.98 / 0.85, at the plastics |
| rider | the Spartan's own size (2.1 m) | 0.85 of it (about 1.8 m), on the bike only |
| head probe | y 1.30, r 0.24 | y 1.18, r 0.22, for the smaller rider |

The gearing keeps Reflex's road speed for the new rear tyre (`final_ratio = 0.335 / 0.317`).
The grips now turn about the steering axis through the triple clamp. The plain Unleashed model
in the tuning menu keeps the Pro 250's shape; with it the drawn wheels sit a few centimetres off
the forks and swingarm.

The Spartan is 7 ft; on the real-sized 450F that looked like a child's bike, so on the bike
(only) he is drawn at `pose::RIDER_SIZE` (0.85): posed full size to targets 1 / 0.85 further
out, then shrunk about the bike's origin, so the hands and feet still land on the grips and pegs.
On foot and on the board he keeps his size.

## Drawing

The bones are entities; each frame the root takes the bike's pose (its origin is the centre of
gravity, `FRAME_LIFT` under the frame's), `OBJ_UPR_Forks` turns about its own Y by the bar
angle, `OBJ_LWR_Forks` slides up that axis by the front compression over cos(rake), the
swingarm turns about its pivot until the rear axle has risen by the rear compression, and the
tyres roll (rear at the drive's spin, front with the ground speed). Materials are unlit, tinted
by Halo's baked light where the bike is (as the rider), over a fixed soft shade in the vertex
colours. Holes in DXT1/5 alpha (chain, spokes) are alpha-masked.

## The Cortana kit

The user painted it for Reflex itself: holographic blue plastics over the THQ / Green sheet with the number plates
kept stock. An install where that kit is in place (the sheet is mostly blue) is used as is.
Otherwise `reflex_assets::cortana::paint` repaints the stock sheet the same way at start-up, in
the background.
