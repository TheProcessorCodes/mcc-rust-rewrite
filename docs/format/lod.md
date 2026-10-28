# Level of detail and draw distance (MCC Halo 3, U13)

What Halo 3's tags say about drawing less at a distance, from our own probes on 2026-10-28
(`docs/format/scripts/probe_lod.py <map> fade|perm|obje`) of `riverworld`, `120_halo` and
`010_jungle`. No GPL code or plugin XML was used.

## 1. Structure BSP instances: fade by screen size

The instance element (`sbsp-render-geometry.md` 2.2, 120 bytes) has two u16 at `+0x6C` and
`+0x6E`. Other tools call them "fade pixels start / end"; the values fit that reading: the start
is always the larger, so the instance starts fading when it covers `start` pixels on screen and
is gone at `end`.

| map | instances | values (start, end) x count |
|---|---|---|
| riverworld | 262 | (0, 0) x 262 |
| 120_halo, all 11 BSPs | 3,987 | (0, 0) everywhere |
| 010_jungle BSP 020 | 374 | (0, 0) x 297, (20, 10) x 77 |
| 010_jungle BSP 030 | 421 | (0, 0) x 384, (15, 10) x 6, (20, 15) x 31 |
| 010_jungle BSP 050 | 833 | (0, 0) x 709, (8, 4) x 20, (15, 10) x 22, (20, 10) x 7, (20, 15) x 40, (25, 18) x 35 |

So only foliage-heavy campaign BSPs use it, and 0 means "never". Not known: what "pixels" measures
(the bounding sphere's projected diameter is assumed) and at which screen height (the viewer
uses its own 720-line view).

Viewer (`world/build.rs`, `world/culling.rs`): instances are chunked by their `end` value too, and
a chunk is hidden when its largest instance's projected diameter is under `end` pixels. The fade
itself (start to end) is not drawn; the instance pops out at `end`.

## 2. Render models: no LOD meshes

A render model permutation (24 bytes in the region's permutation block) holds a string id name at
`+0x00`, the first section `+0x04` (i16) and a section count `+0x06` (i16); the rest was zero on
every model looked at (`010_jungle` `rock_a`: one region, one permutation, section 0, count 1, one
section in the model). Halo 2 kept six LOD section indices per permutation; Halo 3 keeps one run
of sections, all drawn. There is nothing to switch between.

## 3. Objects: no draw distance

The object header (`obje` part of scenery, crates, machines): `+0x00` i16 object type (6 scenery),
`+0x04` f32 bounding radius (0.25 for a jungle tree object, 3.05 for `rock_a`), `+0x08` 3 x f32
bounding offset, then flags and tag references; no field reads as a distance or a pixel size in
the first 0x80 bytes of the scenery looked at. (Later games add LOD fade distances to objects;
Halo 3 does not appear to.)

## 4. What the viewer does instead

A size rule for everything that is not structure or sky: a chunk of instances or objects is hidden
when its largest placement's bounding radius covers less than 1.5 pixels of a 720-line view at the
chunk's nearest point (`MIN_PIXELS`, `HALO3_MIN_PIXELS` to change, 0 to turn off). Clusters and
the sky are never dropped by size; the sky is never portal-culled either. Props are chunked by
size class (bounding radius under 1.5 wu, under 6 wu, larger) so the small ones go first.
