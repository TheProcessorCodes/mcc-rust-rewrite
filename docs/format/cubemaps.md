# Reflection cube maps and parallax (Halo 3, MCC)

How a Halo 3 level stores the cube maps its `dynamic` environment maps reflect, and what the
viewer does with them and with `parallax` (2026-11-02). Read from the user's own maps with
`blam cubemaps <map>`; no GPL code involved.

## 1. Where the cubes are

The scenario's structure BSP block (`scnr + 0x18`, 108 bytes an entry) holds, besides the
`sbsp` (+0x00), design (+0x10) and lighting (+0x20) references, a **`bitm` reference at
+0x48**: the BSP's cube maps, one tag per BSP named `<scenario>_<bsp>_cubemaps`
(`levels\multi\riverworld\riverworld_riverworld_cubemaps`,
`levels\solo\120_halo\120_halo_120_bsp_010_cubemaps`). A `wind` reference follows at +0x58.
120_halo's `120_bsp_050` has none.

Every bitmap of the tag is a cube (type 2): DXT5, curve 1 ("xRGB"), 128x128 with 2 levels or
64x64 with 1. Riverworld: 55 cubes, all 128. 120_halo: 6 to 15 per BSP, mixed 128 and 64.
Faces in the D3D order `+X -X +Y -Y +Z -Z` (`bitmap.md` 12), Halo's own axes: looking a
cube up with the Halo-frame direction itself is right (section 4).

## 2. Which cluster uses which cube

Each cluster (`sbsp + 0xC4`, 280 bytes) has a block at **+0x10C** of 16-byte entries:

| offset | type | meaning |
|---|---|---|
| 0x0 | 3 x f32 | the capture point (world units) |
| 0xC | i16 | the cube's number across the scenario |
| 0xE | i16 | the cube's bitmap in this BSP's cube map tag |

On riverworld the two numbers are equal; on 120_halo the first runs to 71 while each BSP's
tag holds only its own handful, and a capture shared by two BSPs keeps its first number in
both (cube 70 is in BSPs 0 and 7, bitmap 4 of BSP 0's tag). Riverworld: 61 entries over its
26 clusters, every cluster has at least one, 55 distinct cubes. Most clusters list one or two;
riverworld's big outdoor cluster 2 lists four.

Other blocks in the cluster seen on the way, not read: +0x3C (20 two-byte entries on riverworld
cluster 2, probably portal indices), +0xC8 (one 48-byte entry), +0xE8 (11 entries; points and
small floats, decorator or seam data?). The instanced geometry's "cubemap 0, cubemap 1, blend
factor" at instance +0x70 (`sbsp-render-geometry.md` 2.2) is 0 on every riverworld instance.

## 3. What a texel holds

`rgb` and `alpha` together: the light is `rgb * alpha`. But every cube is stored at the same
mean: rgb about 0.25 and alpha about 0.06 per face, on Valhalla's sunlit valley and inside
120_halo's dark Forerunner halls alike. So a cube holds the *pattern* of the light round its
point, not its level. In the viewer the level is the lightmap's: the reflection is

```
cube(r) / mean(cube) * mean light of the lightmap's SH at the pixel * CUBE_SCALE
```

with `mean(cube)` the mean of `luminance(rgb as sRGB) * alpha` over the six faces, worked out
at load, and the SH's mean light its constant term (`c0 * 0.2821`, dominant light included).
Objects lit per vertex use their vertex irradiance over pi instead. How Halo itself scales the
cube is not known; this is our reading of the numbers.

## 4. Checks (Valhalla, headless shots)

- `MCC_REFLECTIONS=view` draws, on every world surface, the cube seen straight along the
  view ray. With the camera at a capture point it should look like the world: from cube 8
  (66.67, -60.08, 1.83) looking along +X the tower, the river and the far shore of the cube
  land on the tower, river and shore of the world. That fixed the axes (identity).
- `MCC_REFLECTIONS=mirror` makes every surface a mirror of the cubes: looking straight
  down at a floor shows the sky.
- `CUBE_SCALE` was set by comparing the `view` shot with the normal one, region by region
  (mean display luminance): 0.12. That is about a mean albedo, as it should be: the cube
  holds light reflected off the surfaces round its point, the SH the light falling on them.
  `MCC_CUBE_SCALE` overrides it.
- 2026-11-06: raised to 1.0. At 0.12 surfaces whose look is mostly reflection went near
  black: the Warthog run's metal road in the tunnels measured 7 (display luminance, 0-255)
  where the SH stand-in gave 34. At 1.0 Valhalla's spawn, overview and spawn12 shots come
  out as bright as with `MCC_REFLECTIONS=0` (82 / 111 / 62), so the cubes keep their pattern
  at the level the stand-in had.

## 5. Choosing cubes in the viewer

All cubes of all BSPs go up once at load as one cube array (RGBA8 sRGB, all at the largest
size, full mip chain): 27.5 MiB on Valhalla, 36 MiB on 120_halo. Each draw chunk
(`build.rs`) takes the cubes of the clusters it lies in (failing those its BSP's, failing those
any) and keeps the three nearest its middle; they go in the chunk's `MeshTag` (count in the
top two bits, 10 bits a cube). The pixel blends them by inverse squared distance and reads
them at a mip from the material: Cook-Torrance from its roughness lobe times
`env_roughness_scale`, the phong models at level 1, glass sharp. `per_pixel` environment
maps (the shader's own cube) are drawn with the level's cubes too. Halo 2, Reach and the
test scene have no cubes and keep the SH stand-in (`MCC_REFLECTIONS=0` brings it back
everywhere).

## 6. Parallax

`parallax` (shader category 8) `simple` / `interpolated` / `simple_detail` with `height_map`
and `height_scale` (0.02 to 0.1; the icicles 0.4). Few surfaces use it: 120_halo's
elevator panels, floor tiles and wall bases (`panel_elevator_parallax`, interpolated, 0.03),
The Construct's waste panels and door trim, the Warthog's wheels and hubs, grates and crates.
The viewer moves the texcoord towards the eye by `(height - 0.5) * height_scale` along the
view direction in the tangent frame (the same tangent and binormal as the bump map), once for
`simple` and `simple_detail`, twice for `interpolated` (the second step from where the first
landed, the two heights averaged). Every map of the material reads at the moved texcoord.
