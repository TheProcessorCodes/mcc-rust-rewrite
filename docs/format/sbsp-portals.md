# Cluster portals and point-to-cluster lookup (MCC Halo 3, U13)

What a structure BSP (`sbsp`) says about which of its clusters can see which: the portals between
clusters, the list of portals each cluster touches, and the table that turns a collision BSP leaf
into a cluster. Written 2026-10-28 from our own probes of `riverworld.map` (Valhalla) and
`120_halo.map` (BSPs 000, 100, 110, 120). No GPL code or plugin XML was used: the blocks were
found by walking the sbsp's block headers and matching counts and contents against things whose
meaning is plain (cluster indices in range, a radius equal to the farthest vertex, a leaf count
equal to the collision BSP's).

Code: `crates/blam_geometry/src/sbsp.rs` (`read_portals`, `leaf_clusters`),
`collision.rs` (`tree`), `scene.rs` (`Portal`, `ClusterLocator`), viewer
`crates/mcc_world/src/culling.rs`. Test: `zones::tests::riverworld_portals_and_spawn_clusters`.

Probe: `python -I docs/format/scripts/probe_sbsp_portals.py <map> MODE <bsp index>`, MODE `root`
(every block header in the first 0x400 bytes of the sbsp, with the gap to the next block over the
count), `cluster` (blocks inside the first clusters), `portals` (decode and cross-check portals),
`leaves` (histogram of the leaf cluster bytes), `dump` (`OFF:STRIDE:N` raw elements).

## 1. Cluster portal, 40 bytes, block at sbsp + `0x7C`

| offset | type | meaning |
|---|---|---|
| `0x00` | i16 | back cluster |
| `0x02` | i16 | front cluster |
| `0x04` | i32 | plane index (the same few values for portals on one plane: 663 for every riverworld portal at z 4.8; not used) |
| `0x08` | 3 x f32 | centroid, wu (not the mean of the vertices; probably the area centroid) |
| `0x14` | f32 | radius: distance from the centroid to the farthest vertex, **exactly** on every portal of all five BSPs probed |
| `0x18` | u32 | flags: 0 on every portal seen |
| `0x1C` | block | vertices, 12 bytes each (3 x f32, wu), 3 to 12 per portal: a convex polygon |

Counts: riverworld 60 portals over 26 clusters; 120_halo BSP 000 16 over 16, BSP 100 41 over 25,
BSP 110 47 over 29, BSP 120 119 over 48. Some portals are degenerate (riverworld portal 0 joins
clusters 1 and 17 with radius 0.01).

## 2. Cluster's portal list, cluster + `0x3C`

Inside the 280-byte cluster element (`sbsp-render-geometry.md` 2.1): a block of i16 portal indices.
On all five BSPs, each cluster's list is exactly the set of portals whose back or front is that
cluster (the probe's `portals` mode checks this; 0 mismatches). The viewer builds the adjacency
from the portals' own back / front instead, which is the same.

Other blocks seen inside the cluster element, not decoded: `+0xC8` (count 1), `+0xE8` (11 and 43
elements on riverworld clusters 2 and 3), `+0x10C` (a few elements).

## 3. Point to cluster

The structure BSP has no BSP tree of its own in the tag; the collision BSP's
(`collision-bsp-research.md` 8) is used, and the sbsp maps its leaves to clusters:

- **sbsp + `0x34`**: block of **1-byte** elements, one signed byte per collision leaf: the leaf's
  cluster. riverworld: 42,101 elements = the collision BSP's 42,101 leaves; values 0..25 (the 26
  clusters), most in cluster 2 (the outdoors, 15,828 leaves). 120_halo BSP 120: 36,163 leaves,
  values 0..47.
- Walk the collision BSP 3D nodes from node 0: plane test `d = n . p - w`; **`d >= 0` takes the
  front child** (bits 40..63), else the back child (bits 16..39). A leaf child (bit 23 of the child
  code) gives the leaf index; `-1` is solid space (inside a wall, or outside the sealed level).

The side rule is pinned by Valhalla's 107 spawn points: every one (raised 0.6 wu to eye height)
lands in a cluster, and every such cluster's bounding box holds the point. A point far under the
map lands in solid space.

## 4. Visibility as the viewer uses it

Halo 3 draws a cluster's geometry when the cluster can be seen; one cluster sees another only
through portals. `culling.rs` finds the camera's cluster in every shown BSP (section 3), then
floods through portals, keeping per step the screen rectangle of the portal polygon (clipped
against the near plane) intersected with the rectangle it was reached through. Chunks of clusters
never reached are hidden. A camera in solid space sees all of that BSP. Instances and objects are
tied to every cluster whose bounding box their own box touches (conservative), and never culled by
portals when their centre is outside every cluster (backdrops).

On 120_halo's Warthog run tunnel (zone set 6, BSP 120, camera at (410, 590, -45) looking +Y) the
camera is in cluster 8 and sees 23 of 48 clusters; on Valhalla's first spawn 13 of 26; over
Valhalla's middle, in the outdoor cluster, all 26.

## 5. Atmosphere palette, sbsp + `0x94`

8-byte elements: string id name, i16 index into the `skya` settings, pad. A cluster's byte at
`+0x19` indexes this palette (`zone-sets.md` 3 has the 120_halo and 010_jungle contents and the
correction this made).

## 6. Not decoded

The scenario's per-zone-set block at scnr + `0x40` (about 1,904 bytes per zone set; probably a
cluster PVS across BSPs), the cluster blocks at `+0xC8`, `+0xE8`, `+0x10C`, the portal plane's
block (planes are recomputed from the vertices), how the game itself finds the camera's cluster
when several BSPs are loaded (the viewer asks each shown BSP).
