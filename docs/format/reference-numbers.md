# Reference numbers: what our Rust reader must reproduce

Written 2026-10-27. Two MCC Halo 3 maps were exported with HaloExport (Reclaimer-based, see
`docs/haloexport-notes.md`): **riverworld** (Valhalla, the primary test file) and **cyberdyne** (The Pit).
Every number below comes from one of three places, marked in the tables:

* **map** = read straight from the `.map` bytes with the scripts in `docs/format/scripts/`
  (`check_scnr_sbsp_counts.py`, `check_lbsp_resource.py`, `check_triangle_sums.py`, `check_placement_matrix.py`;
  run as `python -I <script> <map path> [...]`, read-only). These are the authoritative values.
* **sidecar** = `<HaloExport output>\<map>\halo_export.json` written by HaloExport.
* **gltf** = computed with python from `scene.gltf` + `scene.bin` next to the sidecar.

Where a number appears in more than one source they agree exactly unless a note says otherwise.
Maps: `<MCC>\halo3\maps\{riverworld,cyberdyne}.map`
(read-only). Units: Halo world units (wu) unless marked m (metres); 1 wu = 3.048 m; glTF (x, y, z) = 3.048 * Halo (x, z, -y),
so Halo (x, y, z) = (gx, -gz, gy) / 3.048.

Related: `docs/format/verify/` holds another reader's scenario checks (`scnr_riverworld.py`, `rotcheck/`); the
scripts here are independent of them.

---

## 1. Cache level

| item | riverworld (map) | cyberdyne (map) |
|---|---|---|
| file size | 0x0952C000 = 156,418,048 bytes (corrected: was written as 156,286,976, off by 131,072; the file on disk and the header `0x8` field both give 156,418,048) | (not re-derived) |
| build string (header 0xA0) | `Dec 21 2023 22:31:37` | same |
| cache_type (sidecar) | `MccHalo3U13` | `MccHalo3U13` |
| scenario path (header 0xE0, sidecar `scenario`) | `levels\multi\riverworld\riverworld` | `levels\multi\cyberdyne\cyberdyne` |
| tag count (header 0x20) | 11,658 (0x2D8A) | 11,358 (0x2C5E) |
| tag class count | 183 | 183 |
| `scnr` tag index / meta file offset | 8 / 0x5B29158 | 8 / 0x4C7490C |
| `sbsp` tag (class, index) / meta file offset | (`sbsp`, 9757) / 0x8C9FCF4 | (`sbsp`, 9417) / 0x7C1E1C0 |
| `sLdT` lightmap tag | 6878 `levels\multi\riverworld\riverworld_faux_lightmap` | 6530 `levels\multi\cyberdyne\cyberdyne_faux_lightmap` |
| `Lbsp` tag (from sLdT.LightmapRefs[0]) | 6879 `levels\multi\riverworld\riverworld`, BspIndex 0 | 6531 `levels\multi\cyberdyne\cyberdyne`, BspIndex 0 |
| sky `scen` tag | 11 `levels\multi\riverworld\sky\riverworld` | 9 `levels\multi\cyberdyne\sky\sky` |
| `play` SharedCaches | `maps\english.map`, `maps\mainmenu.map`, `maps\shared.map` | same |
| data table address for local pages (u32 at file 1232 = sectionOffset[1]) | 0x114000 | 0x111000 |

---

## 2. Scenario (`scnr`) block counts, U13 offsets (map)

| block | offset | riverworld | cyberdyne |
|---|---|---|---|
| StructureBsps (108-byte) | 24 (0x18) | 1 | 1 |
| Skies (20-byte) | 52 (0x34) | 1 | 1 |
| ObjectNames (36-byte) | 168 (0xA8) | 1 | 49 |
| Scenery (180-byte) | 180 (0xB4) | **142** | **132** |
| SceneryPalette (16-byte) | 192 (0xC0) | 26 | 37 |
| Machines (136-byte) | 312 (0x138) | 0 | **8** |
| MachinePalette | 324 (0x144) | 0 | 3 |
| Controls (120-byte) | 360 (0x168) | 0 | 0 |
| ControlPalette | 372 (0x174) | 0 | 0 |
| Crates (176-byte) | 1468 (0x5BC) | **102** | **131** |
| CratePalette | 1480 (0x5C8) | 43 | 52 |
| ScenarioLightmapReference (TagReference) | 1708 (0x6AC) | `sLdT` 6878 | `sLdT` 6530 |

Every scenery / crate / machine placement was exported (sidecar node counts equal these block counts), so
no palette entry failed on either map.

Palette heads (first entries, map): riverworld SceneryPalette[0] `objects\multi\spawning\respawn_point`,
CratePalette[0] `objects\levels\multi\riverworld\man_cannon_river\man_cannon_river`, [2]
`objects\multi\koth\koth_hill_static`; cyberdyne SceneryPalette[6] = `respawn_point`, MachinePalette =
[`objects\levels\multi\cyberdyne\jump_fan\jump_fan`, `...\brute_target\brute_target`, `...\jackal_target\jackal_target`].

---

## 3. Structure BSP (`sbsp`) and lightmap data (`Lbsp`) (map)

| item | riverworld | cyberdyne |
|---|---|---|
| sbsp XBounds @+64 | (-40.85219, 206.53394) | (-56.94171, 56.94174) |
| sbsp YBounds @+72 | (-285.42664, 92.48665) | (-50.12860, 85.33836) |
| sbsp ZBounds @+80 | (-15.51741, 1892.30896) | (-3.00005, 27.05882) |
| Clusters @+196 (280-byte) | **26** | **48** |
| Shaders @+208 (36-byte), null refs | 64, 0 | 85, 0 |
| GeometryInstances @+448 (120-byte) | **262** | **834** |
| sbsp.Sections @+756 (unused by Reclaimer for MCC) | 102 | 359 |
| sbsp.BoundingBoxes @+768 (44-byte) | 76 | 311 |
| cluster SectionIndex values (@+216 in each cluster) | 76..101 in order | 311..358 in order |
| clusters whose section has no vertex data (cluster -> section) | 3: 0->76, 1->77, 23->99 | 4: 4->315, 5->316, 7->318, 38->349 |
| instances with TransformScale != 1 | 95 | 56 |
| instances whose section has no vertex data (index, section) | 1: (71, 34) | 4: (99, 47), (110, 56), (832, 309), (833, 310) |
| instance[0]: scale / section / translation row | 0.99999988 / 0 / (81.35999, -148.49684, 9.85206) | 1.0 / 0 / (-6.50145, -0.51614, 0.31232) |
| ResourcePointer1 / 2 / 3 (@+596 / 876 / 908) | 0xFDE91C86 / 0xFDEA1C87 / 0xFE651D02 | 0xFD681C05 / 0xFD691C06 / 0xFE1F1CBC |
| Lbsp.Sections @+308 (76-byte) | 102 | 359 |
| Lbsp.ResourcePointer @+428 (resource index = low 16 bits) | 0xEE3D0CDA (3290) | 0xEDD00C6D (3181) |
| Lbsp section IndexFormat @+48 | all 102 = 3 (TriangleList) | all 359 = 3 |
| Lbsp section VertexFormat @+46 | 1 x 76 (sections 0..75), 0 x 26 (76..101) | 1 x 311, 0 x 48 |
| unindexed sections (IB -1 or flag 0x10) | 4 | 8 |
| section 0 submesh (ShaderIndex, IndexStart, IndexLength, VertexCount) | (40, 0, 1266, 702); Subsets 10; VB 0 IB 0; Flags 0x8 | (27, 0, 174, 112); Subsets 5; Flags 0x8 |
| sum of submesh IndexLength / 3 over all Lbsp sections (unplaced) | 61,500 | 91,032 |
| **World triangles** = that sum over cluster sections with a material, non-empty | **37,260** | **12,240** |
| **instance triangles** = per-section sum added once per placed instance | **87,367** | **229,319** |
| distinct instance sections placed | 75 | 307 |

Resource plumbing for the Lbsp (map):

| item | riverworld | cyberdyne |
|---|---|---|
| zone.ResourceEntries @+100 count | 7,428 | 7,358 |
| zone.FixupDataSize @+328 / FixupDataPointer @+340 (shifted) -> file | 666,384 / 0x7316D590 -> 0x5988640 | 730,764 / 0x731E1630 -> 0x4AB28C0 |
| entry[Lbsp]: FixupOffset / FixupSize / LocationType / SegmentIndex | 298,232 / 19,732 / 1 / 3254 | 282,596 / 56,164 / 1 / 3132 |
| entry ResourceFixups / ResourceDefinitionFixups | 1008 / 503 | 2878 / 1438 |
| fixup tail: vertexBufferCount / indexBufferCount | 394 / 109 | 1087 / 351 |
| VertexBufferInfo[0..4] (VertexCount, DataLength) | (702, 25272) (650, 23400) (428, 15408) (368, 13248) (44, 1584) | (112, 4032) (608, 21888) (2460, 88560) (2451, 88236) (1475, 53100) |
| total vertices / vertex bytes | 319,934 / 11,892,976 | 443,873 / 10,152,204 |
| IndexBufferInfo[0..4] (IndexFormat, DataLength) | (3, 2532) (3, 2592) (3, 1524) (3, 1296) (3, 144) | (3, 348) (3, 2160) (3, 10464) (3, 10428) (3, 5298) |
| total index bytes | 400,284 | 546,192 |
| ResourceFixups masked offsets [0..3]; index buffer 0 = fixups[2*vb] | 0x0, 0x62B8, 0xBE20, 0xFA50; 0xB578F0 | 0x0, 0xFC0, 0x6540, 0x1BF30; 0x9AE90C |
| play: Pages / Segments | 1581 / 7175 | 1632 / 6945 |
| segment: PrimaryPage / SecondaryPage / PrimaryOffset | 1420 / -1 / 25,733,816 | 1420 / -1 / 38,121,656 |
| page 1420: CacheIndex / DataOffset / CompressedSize / DecompressedSize / DataChunkCount | -1 / 0 / 45,416,454 / 92,027,152 / 1870 | -1 / 0 / 42,724,060 / 94,883,840 / 2002 |

Note: 36 bytes per vertex for VertexFormat 1 (25272 / 702) on riverworld; 36 bytes also for cyberdyne (4032 / 112).

---

## 4. HaloExport sidecar: counts, bounds, categories

### 4.1 bsps[]

| field | riverworld | cyberdyne |
|---|---|---|
| index / tag | 0 / `levels\multi\riverworld\riverworld` | 0 / `levels\multi\cyberdyne\cyberdyne` |
| clusters / instances | 26 / 262 | 48 / 834 |
| cluster_meshes / instance_placements | 23 / 261 | 44 / 830 |
| bounds_wu x / y / z (= sbsp bounds fields) | [-40.85219, 206.53394] / [-285.42664, 92.48665] / [-15.5174055, 1892.309] | [-56.941708, 56.94174] / [-50.128597, 85.338356] / [-3.0000455, 27.058815] |

### 4.2 counts

| | riverworld | cyberdyne |
|---|---|---|
| nodes: world / instance / scenery / crate / machine / control | 1 / 261 / 142 / 102 / - / - | 1 / 830 / 132 / 131 / 8 / - |
| triangles: world / instance / scenery / crate / machine | 37,260 / 87,367 / 146,845 / 39,625 / - | 12,240 / 229,319 / 139,652 / 50,382 / 3,200 |
| vertices: world / instance / scenery / crate / machine | 35,256 / 98,270 / 238,975 / 50,103 / - | 18,839 / 320,314 / 224,356 / 68,739 / 4,162 |
| triangles_total / vertices_total | 311,097 / 422,604 | 434,793 / 636,410 |
| unique_meshes (glTF meshes) | 132 | 380 |
| materials | 72 | 134 |
| textures written / missing | 62 / 0 | 108 / 0 |
| winding_agrees_with_normals | 1.0 | 0.9996 |
| skies | [`levels\multi\riverworld\sky\riverworld`] | [`levels\multi\cyberdyne\sky\sky`] |
| sky_exported | false | false |
| backdrop_excluded | [] | [] |
| collision_bsp | "not exported (render geometry only); see README gaps" | same |
| warnings | [] | 1: `image objects\levels\multi\salvation\bitmaps\plasma_sm_red_mask -> plasma_sm_red_mask-00175.png: all black (512x512, max RGB 0, mean alpha 0.09)` |
| unit_probe | tag `objects\characters\masterchief\mp_masterchief\fp\fp`, height_wu 0.0901, 0.901 ft (first-person arms; meaningless on MP maps) | same |
| materials_without_base_color | ground_lake_deep, z_null_light, wall_lights_illum, z_invis_lightquad, invisible, fusion_coil_plasma, nutblocker | fusion_coil_plasma |
| pipeline_hints.terrain_regex | `^(ground_lake_deep\|riverworld_cliff_blend\|riverworld_cliff_distant\|riverworld_distant\|riverworld_ground\|riverworld_ground_cliff\|riverworld_ground_snow\|riverworld_ground_water)$` | `^(cyberdyne_ground_concrete\|cyberdyne_ground_outer)$` |
| pipeline_hints.ground_regex | `^(riverworld_distant\|riverworld_ground\|riverworld_ground_snow\|riverworld_ground_water)$` | same as terrain |
| pipeline_hints.foliage_models_renamed | 28 | 14 |

### 4.3 bounds (sidecar, glTF metres; mesh-AABB corners through the node matrix)

| kind | riverworld min_m / max_m / size_ft | cyberdyne min_m / max_m / size_ft |
|---|---|---|
| world | [-124.52, -28.31, -281.9] / [629.52, 276.99, 869.98] / [2473.9, 1001.6, 3779.1] | [-152.4, -8.23, -260.11] / [152.4, 25.91, 65.33] / [1000, 112, 1067.7] |
| instance | [103.78, -11.62, 30.88] / [398.5, 173.36, 718.27] / [966.9, 606.9, 2255.2] | [-143.03, -26.28, -229.59] / [103.88, 68.87, 166.51] / [810.1, 312.2, 1299.5] |
| scenery | [147.4, -0.22, 139.97] / [314.14, 60.55, 487.51] / [547.1, 199.4, 1140.2] | [-65.23, -8.39, -95.88] / [65.46, 14.23, 49.18] / [428.8, 74.2, 475.9] |
| crate | [155.39, -19.96, 21.34] / [312.38, 41.12, 470.06] / [515.1, 200.4, 1472.2] | [-74.69, -12.42, -111.31] / [65.14, 37.73, 49.25] / [458.8, 164.5, 526.8] |
| machine | - | [-53.95, -4.65, -34.44] / [53.95, 19.68, 49.97] / [354, 79.8, 277] |
| all | = world | [-152.4, -26.28, -260.11] / [152.4, 68.87, 166.51] / [1000, 312.2, 1399.7] |

### 4.4 materials_by_category (materials / triangles)

| category | riverworld | cyberdyne |
|---|---|---|
| opaque (h3_r0) | 42 / 212,528 | 102 / 395,138 |
| terrain (h3_r0) | 6 / 25,241 | 2 / 782 |
| foliage (h3_t0) | 5 / 53,912 | 1 / 21,920 |
| alpha_test (h3_t0) | 3 / 1,904 | 7 / 1,124 |
| alpha_blend (h3_b0) | 6 / 858 | 4 / 2,312 |
| decal_blend (h3_b0) | 3 / 214 | 4 / 728 |
| additive (h3_ua) | 4 / 13,804 | 13 / 12,769 |
| water (h3_ua) | 1 / 2,607 | - |
| special (h3_ua) | 2 / 29 | 1 / 20 |
| total | 72 / 311,097 | 134 / 434,793 |

---

## 5. glTF file facts (gltf)

| item | riverworld | cyberdyne |
|---|---|---|
| scene.bin bytes = buffers[0].byteLength | 5,022,288 | 8,826,320 |
| nodes (all in scene 0) | 506 | 1,102 |
| meshes | 132 | 380 |
| materials | 72 | 134 |
| accessors = bufferViews | 1,195 | 3,290 |
| images / textures | 62 / 65 | 108 / 133 |
| primitives in all meshes | 239 | 658 |
| extras.halo | map `riverworld.map`, scenario, units "metres (1 Halo world unit = 10 ft = 3.048 m)", up "+Y (Halo +Z)", axis_map "glTF (x, y, z) = 3.048 * Halo (x, z, -y)" | same shape |
| node kinds (extras.halo.kind) | bsp_clusters 1, bsp_instance 261, scenery 142, crate 102 | bsp_clusters 1, bsp_instance 830, scenery 132, crate 131, machine 8 |

### 5.1 World mesh (node `World`, mesh 0, no matrix)

| item | riverworld | cyberdyne |
|---|---|---|
| primitives | **28** | **23** |
| triangles / vertices | **37,260 / 35,256** | **12,240 / 18,839** |
| glTF min (m) | (-124.51747, -28.311525, -281.89932) | (-152.39998, -8.229649, -260.11133) |
| glTF max (m) | (629.51544, 276.98703, 869.9804) | (152.40025, 25.907997, 65.32853) |
| **Halo wu X** | (-40.85219, 206.53394) | (-49.99999, 50.00008) |
| **Halo wu Y** | (-285.42664, 92.48665) | (-21.43324, 85.33836) |
| **Halo wu Z** | (-9.28856, 90.87501) | (-2.70002, 8.50000) |

Riverworld World X/Y equal the sbsp bounds exactly; Z does not (sbsp says -15.52..1892.31). Cyberdyne
differs in X and Y too: the sbsp bounds are an envelope, not the geometry box.

World primitives in file order (material | class | category | triangles | vertices):

**riverworld** (mesh 0, 28 primitives):

| # | material | class | category | triangles | vertices |
|---|---|---|---|---|---|
| 0 | `riverworld_water_rough` | rmw | water | 2,607 | 1,831 |
| 1 | `panel_generic_simple` | rmsh | opaque | 3,655 | 5,717 |
| 2 | `waste_panel_wall_diagonal` | rmsh | opaque | 144 | 236 |
| 3 | `ground_lake_deep` | rmsh | opaque | 238 | 359 |
| 4 | `riverworld_cliff_distant` | rmsh | opaque | 914 | 800 |
| 5 | `riverworld_distant` | rmtr | terrain | 642 | 512 |
| 6 | `z_null_light` | rmsh | opaque | 101 | 85 |
| 7 | `riverworld_ground_cliff` | rmtr | terrain | 1,804 | 1,199 |
| 8 | `riverworld_cliff_blend` | rmtr | terrain | 11,545 | 9,734 |
| 9 | `riverworld_ground` | rmtr | terrain | 8,130 | 5,828 |
| 10 | `wall_lights_illum` | rmsh | additive | 44 | 86 |
| 11 | `riverworld_ground_water` | rmtr | terrain | 1,929 | 1,395 |
| 12 | `riverworld_ground_snow` | rmtr | terrain | 1,191 | 820 |
| 13 | `cust_base_floor` | rmsh | opaque | 1,516 | 2,208 |
| 14 | `panel_tech_vert` | rmsh | opaque | 1,000 | 1,784 |
| 15 | `panel_generic_dark` | rmsh | opaque | 64 | 104 |
| 16 | `waste_panel_trim_illum` | rmsh | opaque | 248 | 416 |
| 17 | `riverworld_metal` | rmsh | opaque | 76 | 146 |
| 18 | `panel_tech_vert_red` | rmsh | opaque | 132 | 228 |
| 19 | `glass_wall` | rmsh | alpha_blend | 288 | 456 |
| 20 | `z_invis_lightquad` | rmsh | alpha_test | 136 | 240 |
| 21 | `panel_tech_vert_blue` | rmsh | opaque | 132 | 228 |
| 22 | `military_equipment` | rmsh | opaque | 576 | 636 |
| 23 | `generator_turbine` | rmsh | opaque | 32 | 48 |
| 24 | `human_military_decals` | rmd | decal_blend | 4 | 8 |
| 25 | `waste_panel_trim` | rmsh | opaque | 72 | 104 |
| 26 | `glass_wall_waterfallbase` | rmsh | alpha_blend | 20 | 24 |
| 27 | `glass_wall_lakebase` | rmsh | alpha_blend | 20 | 24 |

**cyberdyne** (mesh 0, 23 primitives):

| # | material | class | category | triangles | vertices |
|---|---|---|---|---|---|
| 0 | `cyberdyne_ground_concrete` | rmtr | terrain | 654 | 757 |
| 1 | `cyber_boilerplate` | rmsh | opaque | 382 | 456 |
| 2 | `cyberdyne_wall_concrete_d` | rmsh | opaque | 1,898 | 2,772 |
| 3 | `cyberdyne_wall_metal_a` | rmsh | opaque | 440 | 740 |
| 4 | `cyberdyne_wall_concrete_sections_universal_trim` | rmsh | opaque | 942 | 1,714 |
| 5 | `cyberdyne_wall_concrete_sections_universal` | rmsh | opaque | 1,122 | 1,334 |
| 6 | `cyberdyne_modular_piece_trim` | rmsh | opaque | 3,674 | 6,192 |
| 7 | `cyberdyne_metal_trim_a` | rmsh | opaque | 956 | 1,405 |
| 8 | `cyberdyne_grate_a` | rmsh | alpha_test | 36 | 60 |
| 9 | `cyberdyne_wall_metal_c` | rmsh | opaque | 86 | 134 |
| 10 | `cyberdyne_wall_wood_a` | rmsh | opaque | 316 | 588 |
| 11 | `cyberdyne_floor_ramp_a` | rmsh | opaque | 64 | 92 |
| 12 | `cyberdyne_modular_piece_solid` | rmsh | opaque | 126 | 170 |
| 13 | `cyberdyne_wall_concrete_interior_a` | rmsh | opaque | 1,105 | 1,767 |
| 14 | `cyberdyne_tower_floor` | rmsh | opaque | 64 | 100 |
| 15 | `cyberdyne_tower_ramp` | rmsh | opaque | 16 | 32 |
| 16 | `cyberdyne_grate_drain` | rmsh | alpha_test | 8 | 16 |
| 17 | `cyberdyne_floor_metal` | rmsh | opaque | 95 | 126 |
| 18 | `cyberdyne_metal_painted_tower_a` | rmsh | opaque | 88 | 152 |
| 19 | `cyberdyne_roof` | rmsh | opaque | 120 | 146 |
| 20 | `metal_out_fence` | rmsh | alpha_blend | 8 | 16 |
| 21 | `cyberdyne_wall_concrete_a` | rmsh | opaque | 32 | 60 |
| 22 | `cyberdyne_ground_outer` | rmtr | terrain | 8 | 10 |

### 5.2 Placed nodes (gltf)

"corner" = mesh AABB corners through the node matrix (what the sidecar reports); "exact" = every vertex
transformed.

| kind | map | glTF corner min / max (m) | Halo wu X / Y / Z (corner) | Halo wu X / Y / Z (exact) | distinct meshes | nodes with non-unit scale |
|---|---|---|---|---|---|---|
| bsp_instance | riverworld | [103.776, -11.619, 30.88] / [398.497, 173.359, 718.267] | (34.0472, 130.7405) / (-235.6518, -10.1313) / (-3.8121, 56.8764) | (34.3013, 117.8482) / (-235.1054, -10.1313) / (-3.8121, 55.9326) | 75 | 95 |
| scenery | riverworld | [147.398, -0.221, 139.973] / [314.143, 60.553, 487.507] | (48.359, 103.0652) / (-159.9433, -45.9229) / (-0.0724, 19.8665) | (48.4379, 103.0424) / (-159.9136, -46.0314) / (-0.0603, 19.8665) | 21 | 0 |
| crate | riverworld | [155.388, -19.962, 21.34] / [312.381, 41.116, 470.059] | (50.9804, 102.4872) / (-154.2187, -7.0014) / (-6.5492, 13.4894) | (50.9805, 102.4811) / (-154.2131, -7.0014) / (-6.5492, 13.4894) | 35 | 0 |
| bsp_instance | cyberdyne | [-143.03, -26.282, -229.586] / [103.877, 68.875, 166.509] | (-46.9257, 34.0805) / (-54.629, 75.3235) / (-8.6226, 22.5966) | (-44.1573, 31.4719) / (-54.629, 75.3034) / (-8.6226, 22.5966) | 307 | 56 |
| scenery | cyberdyne | [-65.226, -8.392, -95.884] / [65.465, 14.231, 49.18] | (-21.3998, 21.478) / (-16.1351, 31.4579) / (-2.7531, 4.6688) | (-21.3998, 21.4471) / (-16.131, 31.4253) / (-2.7405, 4.6688) | 23 | 2 |
| crate | cyberdyne | [-74.688, -12.417, -111.306] / [65.145, 37.729, 49.255] | (-24.5038, 21.3729) / (-16.1597, 36.5176) / (-4.0737, 12.3783) | (-24.5038, 21.3606) / (-16.1591, 36.4786) / (-4.0737, 12.3783) | 46 | 0 |
| machine | cyberdyne | [-53.95, -4.648, -34.442] / [53.95, 19.675, 49.972] | (-17.7, 17.7) / (-16.395, 11.3) / (-1.525, 6.4552) | same | 3 | 0 |

Instance groups (gltf, `extras.iw4l.model`): riverworld 45 distinct, largest `rock` 84, `base_int_thing` 20,
`basespiresmall` 20, `wallpiecebig` 16, `wallpiece_sm` 15, `root` 9, `base_thingie` 8,
`foliage_tree_pine_small` 8. Cyberdyne 104 distinct, largest `fence_bracket_duo` 32, `bracketandbar` 30,
`lightfix_f_blue` 30, `fence_mesh` 26, `fencepole` 24, `lightcover_combined` 24, `rubberplates` 24,
`catwalk_cables` 22.

Object tags (gltf): riverworld 56 distinct; `objects\multi\spawning\respawn_point` x 99 (scenery),
`objects\multi\territories\territory_static` x 10, `objects\levels\solo\100_citadel\foliage\plant_tree_pine\plant_tree_pine`
x 10 (crates), `respawn_point_invisible` x 8, `vip_destination_static` x 8, `nutblocker_1x1x2` x 6,
`drum_12gal` x 5, `juggernaut_destination_static` x 5, `koth_hill_static` x 3, `oddball_ball_spawn_point`
x 3, `resupply_capsule_panel` x 3, `assault_bomb_spawn_point` x 3. Cyberdyne 72 distinct; `respawn_point`
x 84, `street_cone` x 11, `territory_static` x 10, `fusion_coil` x 8, `respawn_point_invisible` x 6,
`juggernaut_destination_static` x 6, `box_metal_small` x 6, `vip_destination_static` x 5.

First nodes (gltf; translation converted back to Halo wu):

| node | section / palette | instance_name or tag | Halo translation (wu) |
|---|---|---|---|
| riverworld `backet_bottom#bsp0-00000` | section 19 | `backet_bottom` | (97.3284, -86.2064, 3.5768) |
| riverworld `backet_bottom#bsp0-00001` | section 19 | `backet_bottom01` | (94.258, -118.0157, 3.7693) |
| riverworld `base_int_thing#bsp0-00002` | section 10 | `!base_int_thing00` | (76.6378, -57.4485, 0.781) |
| riverworld `respawn_point#scenery-0000` | palette 0 | `objects\multi\spawning\respawn_point` | (79.7632, -56.7296, 1.5002) |
| riverworld `man_cannon_river#crate-0000` | palette 0 | `objects\levels\multi\riverworld\man_cannon_river\man_cannon_river` | (81.5977, -141.485, 2.7501) |
| riverworld `koth_hill_static#crate-0000` | palette 2 | `objects\multi\koth\koth_hill_static` | (81.9019, -104.075, 4.8328) |
| cyberdyne `bay_glass#bsp0-00000` | section 208 | `?*bay_glass01` | (-7.3487, 36.0002, -2.2915) |
| cyberdyne `respawn_point#scenery-0000` | palette 6 | `respawn_point` | (19.4969, -7.1675, 3.6995) |
| cyberdyne `oddball_ball_spawn_point#crate-0000` | palette 1 | `objects\multi\oddball\oddball_ball_spawn_point` | (0.0483, -5.2622, 1.8238) |
| cyberdyne `jump_fan#machine-0000` | palette 0 | `objects\levels\multi\cyberdyne\jump_fan\jump_fan` | (17.2, 10.8, -1.35) |

### 5.3 Matrix test vectors (map -> gltf, full precision)

Rebuild rule (verified on every object and instance node of both maps, worst difference 5.2e-5 m):
placement `M = S(scale or 1) * R(q) * T(pos)` with `q = Qx(roll) * Q(-Y)(pitch) * Qz(yaw)` (Hamilton product);
instance `M = S(TransformScale) * [raw 4x3 with M44 = 1]`; glTF 16 floats = `C^-1 * M * C` in M11..M44 order
with C = {M11 = 3.048, M23 = -3.048, M32 = 3.048, M44 = 1}. See `docs/haloexport-notes.md` section 5.

**riverworld `respawn_point#scenery-0000`** (kind scenery, mesh 76)

* raw placement [0] (palette 0, tag `objects\multi\spawning\respawn_point`): Position `[79.7631989, -56.7296181, 1.50024343]`, Rotation yaw/pitch/roll `[-1.52768326, 8.517545e-10, 2.98194232e-06]`, Scale `0.0`
* glTF `matrix`: `[0.04309964, -2.9791345e-06, 0.9990707, 0.0, -8.5151436e-10, 0.99999994, 2.981942e-06, 0.0, -0.9990707, -1.293713e-07, 0.04309964, 0.0, 243.11824, 4.572742, 172.91188, 1.0]`

**riverworld `man_cannon_river#crate-0001`** (kind crate, mesh 97)

* raw placement [1] (palette 0, tag `objects\levels\multi\riverworld\man_cannon_river\man_cannon_river`): Position `[81.6011963, -62.8223, 1.79815996]`, Rotation yaw/pitch/roll `[-3.14159274, 0.0, 0.0]`, Scale `0.0`
* glTF `matrix`: `[-0.99999994, 0.0, -8.742278e-08, 0.0, 0.0, 0.99999994, 0.0, 0.0, 8.742278e-08, 0.0, -0.99999994, 0.0, 248.72046, 5.4807916, 191.48238, 1.0]`

**riverworld `koth_hill_static#crate-0001`** (kind crate, mesh 98)

* raw placement [8] (palette 2, tag `objects\multi\koth\koth_hill_static`): Position `[58.4949989, -99.4875031, 2.28512001]`, Rotation yaw/pitch/roll `[-0.0, 0.0155740269, -0.0]`, Scale `0.0`
* glTF `matrix`: `[0.9998787, 0.015573396, 0.0, 0.0, -0.015573396, 0.9998787, 0.0, 0.0, 0.0, 0.0, 0.99999994, 0.0, 178.29276, 6.965046, 303.2379, 1.0]`

**riverworld `backet_bottom#bsp0-00000`** (kind bsp_instance, mesh 1)

* raw GeometryInstances[53]: TransformScale `1.0`, SectionIndex 19, instance_name `backet_bottom`
* raw 4x3 rows: `[-0.965925932, 0.258818924, 0.0]`, `[0.0, -3.7252903e-08, 1.0]`, `[0.258818924, 0.965925872, -3.7252903e-08]`, translation `[97.3283539, -86.2063522, 3.57684326]`
* glTF `matrix`: `[-0.9659259, -1.490116e-08, -0.25881892, 0.0, 0.25881892, -1.1920928e-07, -0.9659258, 0.0, -1.490116e-08, -0.99999994, 0.0, 0.0, 296.65683, 10.902219, 262.75696, 1.0]`

**riverworld `rock#bsp0-00092`** (kind bsp_instance, mesh 41)

* raw GeometryInstances[161]: TransformScale `0.491010338`, SectionIndex 55, instance_name `!rock`
* raw 4x3 rows: `[0.0924310833, 0.863281786, 0.496186614]`, `[0.177133739, 0.476115286, -0.861358166]`, `[-0.979836822, 0.167507648, -0.108908474]`, translation `[61.4030609, -148.868866, 3.50683117]`
* glTF `matrix`: `[0.04538461, 0.24363276, -0.42388022, 0.0, -0.48111, -0.053475153, -0.08224799, 0.0, -0.08697446, 0.42293572, 0.23377755, 0.0, 187.15654, 10.688822, 453.75232, 1.0]`

**riverworld `rock#bsp0-00093`** (kind bsp_instance, mesh 42)

* raw GeometryInstances[144]: TransformScale `1.07740045`, SectionIndex 53, instance_name `!rock88`
* raw 4x3 rows: `[0.966473639, 0.0585553683, 0.24999997]`, `[-0.00842931494, -0.965889215, 0.258818835]`, `[0.25662747, -0.252248913, -0.933012903]`, translation `[94.1973572, -157.919083, 2.27787399]`
* glTF `matrix`: `[1.0412791, 0.2693501, -0.06308758, 0.0, 0.27649057, -1.0052282, 0.27177313, 0.0, 0.00908175, -0.27885157, -1.0406493, 0.0, 287.11356, 6.9429603, 481.33737, 1.0]`

**cyberdyne `jump_fan#machine-0000`** (kind machine, mesh 377)

* raw placement [0] (palette 0, tag `objects\levels\multi\cyberdyne\jump_fan\jump_fan`): Position `[17.2000008, 10.8000002, -1.35000002]`, Rotation yaw/pitch/roll `[3.14159274, 0.0, 1.57079637]`, Scale `0.0`
* glTF `matrix`: `[-0.9999999, -8.742278e-08, 0.0, 0.0, 0.0, 5.960464e-08, 0.9999999, 0.0, -8.742278e-08, 0.9999999, 5.960464e-08, 0.0, 52.425606, -4.1148, -32.9184, 1.0]`

**cyberdyne `respawn_point#scenery-0000`** (kind scenery, mesh 308)

* raw placement [0] (palette 6, tag `objects\multi\spawning\respawn_point`): Position `[19.4969006, -7.16745996, 3.69947743]`, Rotation yaw/pitch/roll `[-3.13148737, -4.61287758e-10, 0.00025885395]`, Scale `0.0`
* glTF `matrix`: `[-0.9999489, -2.6152868e-06, 0.010105112, 0.0, 4.6134116e-10, 0.9999999, 0.00025885395, 0.0, -0.010105112, 0.00025884077, -0.9999489, 0.0, 59.426556, 11.276008, 21.846418, 1.0]`

**cyberdyne `lightfix_i_blue#bsp0-00461`** (kind bsp_instance, mesh 180)

* raw GeometryInstances[215]: TransformScale `0.661036849`, SectionIndex 102, instance_name `lightfix_i_blue00`
* raw 4x3 rows: `[1.49011612e-07, 0.707110524, 0.707103014]`, `[1.16229057e-06, 0.707103074, -0.707110524]`, `[-1.0, 8.64267349e-07, -6.85453415e-07]`, translation `[-1.63565707, -14.3409786, 2.01441312]`
* glTF `matrix`: `[1.5760345e-07, 0.46742114, -0.467426, 0.0, -0.66103673, -3.9400868e-07, -6.107134e-07, 0.0, -7.28916e-07, 0.467426, 0.4674212, 0.0, -4.9854827, 6.139931, 43.711304, 1.0]`


---

## 6. Full material lists (sidecar `materials[]`, sorted by triangles descending)

Columns: glTF material name | shader class | category | technique_set | triangles | shader tag path | has base colour.

### 6.1 riverworld (72)

| name | class | category | technique_set | triangles | tag | base colour |
|---|---|---|---|---|---|---|
| `spawn_point_disc` | rmsh | opaque | `h3_r0_opaque_rmsh` | 79,968 | `objects\multi\shaders\spawn_point_disc` | yes |
| `spawn_point_disc_pointer` | rmsh | opaque | `h3_r0_opaque_rmsh` | 47,362 | `objects\multi\shaders\spawn_point_disc_pointer` | yes |
| `riverworld_tree_leafa` | rmfl | foliage | `h3_t0_foliage_rmfl` | 33,432 | `levels\multi\riverworld\shaders\riverworld_tree_leafa` | yes |
| `multiplayer_return_plate` | rmsh | opaque | `h3_r0_opaque_rmsh` | 19,760 | `objects\multi\models\shaders\multiplayer_return_plate` | yes |
| `riverworld_rock` | rmsh | opaque | `h3_r0_opaque_rmsh` | 14,677 | `levels\multi\riverworld\shaders\riverworld_rock` | yes |
| `spawn_point_plasma2` | rmhg | additive | `h3_ua_additive_rmhg` | 13,328 | `objects\multi\shaders\spawn_point_plasma2` | yes |
| `riverworld_tree_leafb` | rmfl | foliage | `h3_t0_foliage_rmfl` | 11,920 | `levels\multi\riverworld\shaders\riverworld_tree_leafb` | yes |
| `riverworld_cliff_blend` | rmtr | terrain | `h3_r0_terrain_rmtr` | 11,545 | `levels\multi\riverworld\shaders\riverworld_cliff_blend` | yes |
| `panel_generic_simple` | rmsh | opaque | `h3_r0_opaque_rmsh` | 11,396 | `levels\multi\riverworld\shaders\forerunner\panel_generic_simple` | yes |
| `riverworld_ground` | rmtr | terrain | `h3_r0_terrain_rmtr` | 8,130 | `levels\multi\riverworld\shaders\riverworld_ground` | yes |
| `koth_shield_generator` | rmsh | opaque | `h3_r0_opaque_rmsh` | 5,993 | `objects\multi\shaders\koth_shield_generator` | yes |
| `military_equipment` | rmsh | opaque | `h3_r0_opaque_rmsh` | 5,952 | `objects\gear\human\military\shaders\military_equipment` | yes |
| `multiplayer_flag_return` | rmsh | opaque | `h3_r0_opaque_rmsh` | 4,320 | `objects\multi\models\shaders\multiplayer_flag_return` | yes |
| `riverworld_tree_leafc` | rmfl | foliage | `h3_t0_foliage_rmfl` | 3,808 | `levels\multi\riverworld\shaders\riverworld_tree_leafc` | yes |
| `riverworld_tree_bark` | rmsh | opaque | `h3_r0_opaque_rmsh` | 3,138 | `levels\multi\riverworld\shaders\riverworld_tree_bark` | yes |
| `riverworld_water_rough` | rmw | water | `h3_ua_water_rmw` | 2,607 | `levels\multi\riverworld\shaders\riverworld_water_rough` | yes |
| `cr_pelican_hull` | rmsh | opaque | `h3_r0_opaque_rmsh` | 2,586 | `objects\levels\shared\pelican_crashed_h3\shaders\cr_pelican_hull` | yes |
| `cust_base_floor` | rmsh | opaque | `h3_r0_opaque_rmsh` | 2,504 | `levels\multi\riverworld\shaders\forerunner\cust_base_floor` | yes |
| `cit_tree_leafb` | rmfl | foliage | `h3_t0_foliage_rmfl` | 2,400 | `levels\solo\100_citadel\shaders\cit_tree_leafb` | yes |
| `cit_tree_leafa` | rmfl | foliage | `h3_t0_foliage_rmfl` | 2,352 | `levels\solo\100_citadel\shaders\cit_tree_leafa` | yes |
| `riverworld_ground_water` | rmtr | terrain | `h3_r0_terrain_rmtr` | 1,929 | `levels\multi\riverworld\shaders\riverworld_ground_water` | yes |
| `panel_generic_simple_horiz` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,878 | `levels\multi\riverworld\shaders\forerunner\panel_generic_simple_horiz` | yes |
| `riverworld_ground_cliff` | rmtr | terrain | `h3_r0_terrain_rmtr` | 1,804 | `levels\multi\riverworld\shaders\riverworld_ground_cliff` | yes |
| `military_equipment_2` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,716 | `objects\gear\human\military\shaders\military_equipment_2` | yes |
| `cit_tree_leafc` | rmsh | alpha_test | `h3_t0_alpha_test_rmsh` | 1,680 | `levels\solo\100_citadel\shaders\cit_tree_leafc` | yes |
| `riverworld_ground_snow` | rmtr | terrain | `h3_r0_terrain_rmtr` | 1,191 | `levels\multi\riverworld\shaders\riverworld_ground_snow` | yes |
| `resupply_capsule` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,076 | `objects\gear\human\military\shaders\resupply_capsule` | yes |
| `panel_tech_vert` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,000 | `levels\multi\riverworld\shaders\forerunner\panel_tech_vert` | yes |
| `military_radio` | rmsh | opaque | `h3_r0_opaque_rmsh` | 982 | `objects\gear\human\military\shaders\military_radio` | yes |
| `riverworld_cliff_distant` | rmsh | opaque | `h3_r0_opaque_rmsh` | 914 | `levels\multi\riverworld\shaders\riverworld_cliff_distant` | yes |
| `forerunner_grill` | rmsh | opaque | `h3_r0_opaque_rmsh` | 864 | `levels\multi\riverworld\shaders\forerunner_grill` | yes |
| `waste_panel_trim_illum` | rmsh | opaque | `h3_r0_opaque_rmsh` | 836 | `levels\multi\riverworld\shaders\forerunner\waste_panel_trim_illum` | yes |
| `cr_pelican_instances` | rmsh | opaque | `h3_r0_opaque_rmsh` | 692 | `objects\levels\shared\pelican_crashed_h3\shaders\cr_pelican_instances` | yes |
| `riverworld_distant` | rmtr | terrain | `h3_r0_terrain_rmtr` | 642 | `levels\multi\riverworld\shaders\riverworld_distant` | yes |
| `cit_tree_bark` | rmsh | opaque | `h3_r0_opaque_rmsh` | 588 | `levels\solo\100_citadel\shaders\cit_tree_bark` | yes |
| `mongoose_palette` | rmsh | opaque | `h3_r0_opaque_rmsh` | 536 | `objects\scenery\human\military\shaders\mongoose_palette` | yes |
| `vahalla_waterfall` | rmsh | alpha_blend | `h3_b0_alpha_blend_rmsh` | 510 | `levels\multi\riverworld\shaders\vahalla_waterfall` | yes |
| `longsword_burn_unlit` | rmsh | opaque | `h3_r0_opaque_rmsh` | 447 | `objects\levels\shared\pelican_crashed_h3\shaders\longsword_burn_unlit` | yes |
| `waste_panel_wall_diagonal` | rmsh | opaque | `h3_r0_opaque_rmsh` | 428 | `levels\multi\riverworld\shaders\forerunner\waste_panel_wall_diagonal` | yes |
| `cr_pelican_interior` | rmsh | opaque | `h3_r0_opaque_rmsh` | 421 | `objects\levels\shared\pelican_crashed_h3\shaders\cr_pelican_interior` | yes |
| `waste_panel_trim` | rmsh | opaque | `h3_r0_opaque_rmsh` | 392 | `levels\multi\riverworld\shaders\forerunner\waste_panel_trim` | yes |
| `panel_cust_wall_dark` | rmsh | opaque | `h3_r0_opaque_rmsh` | 339 | `levels\multi\riverworld\shaders\forerunner\panel_cust_wall_dark` | yes |
| `camping_stool_legs` | rmsh | opaque | `h3_r0_opaque_rmsh` | 308 | `objects\gear\human\military\shaders\camping_stool_legs` | yes |
| `glass_wall` | rmsh | alpha_blend | `h3_b0_alpha_blend_rmsh` | 288 | `levels\multi\riverworld\shaders\forerunner\glass_wall` | yes |
| `nutblocker` | rmsh | additive | `h3_ua_additive_rmsh` | 288 | `objects\physics\shaders\nutblocker` | no |
| `ground_lake_deep` | rmsh | opaque | `h3_r0_opaque_rmsh` | 238 | `levels\multi\riverworld\shaders\ground_lake_deep` | no |
| `riverworld_metal` | rmsh | opaque | `h3_r0_opaque_rmsh` | 220 | `levels\multi\riverworld\shaders\riverworld_metal` | yes |
| `military_decals` | rmd | decal_blend | `h3_b0_decal_blend_rmd` | 162 | `objects\gear\human\military\shaders\military_decals` | yes |
| `panel_generic_dark` | rmsh | opaque | `h3_r0_opaque_rmsh` | 152 | `levels\multi\riverworld\shaders\forerunner\panel_generic_dark` | yes |
| `panel_cust_wall` | rmsh | opaque | `h3_r0_opaque_rmsh` | 146 | `levels\multi\riverworld\shaders\forerunner\panel_cust_wall` | yes |
| `fusion_coil_plasma` | rmsh | additive | `h3_ua_additive_rmsh` | 144 | `objects\gear\human\military\shaders\fusion_coil_plasma` | no |
| `z_invis_lightquad` | rmsh | alpha_test | `h3_t0_alpha_test_rmsh` | 136 | `levels\multi\snowbound\shaders\z_invis_lightquad` | no |
| `panel_tech_vert_red` | rmsh | opaque | `h3_r0_opaque_rmsh` | 132 | `levels\multi\riverworld\shaders\forerunner\panel_tech_vert_red` | yes |
| `panel_tech_vert_blue` | rmsh | opaque | `h3_r0_opaque_rmsh` | 132 | `levels\multi\riverworld\shaders\forerunner\panel_tech_vert_blue` | yes |
| `z_null_light` | rmsh | opaque | `h3_r0_opaque_rmsh` | 101 | `levels\multi\riverworld\shaders\z_null_light` | no |
| `netting` | rmsh | alpha_test | `h3_t0_alpha_test_rmsh` | 88 | `objects\gear\human\military\shaders\netting` | yes |
| `cr_broken_glass` | rmsh | opaque | `h3_r0_opaque_rmsh` | 78 | `objects\levels\shared\pelican_crashed_h3\shaders\cr_broken_glass` | yes |
| `egg_crate` | rmsh | opaque | `h3_r0_opaque_rmsh` | 72 | `objects\gear\human\military\shaders\egg_crate` | yes |
| `generator_turbine` | rmsh | opaque | `h3_r0_opaque_rmsh` | 64 | `objects\gear\human\military\shaders\generator_turbine` | yes |
| `forerunner_grill__2` | rmsh | opaque | `h3_r0_opaque_rmsh` | 48 | `objects\levels\multi\riverworld\shaders\forerunner_grill` | yes |
| `camping_stool_fabric` | rmsh | opaque | `h3_r0_opaque_rmsh` | 48 | `objects\gear\human\military\shaders\camping_stool_fabric` | yes |
| `wall_lights_illum` | rmsh | additive | `h3_ua_additive_rmsh` | 44 | `levels\multi\riverworld\shaders\forerunner\wall_lights_illum` | no |
| `human_military_decals_2` | rmd | decal_blend | `h3_b0_decal_blend_rmd` | 36 | `objects\gear\human\military\shaders\human_military_decals_2` | yes |
| `glass_wall_waterfallbase` | rmsh | alpha_blend | `h3_b0_alpha_blend_rmsh` | 20 | `levels\multi\riverworld\shaders\forerunner\glass_wall_waterfallbase` | yes |
| `glass_wall_lakebase` | rmsh | alpha_blend | `h3_b0_alpha_blend_rmsh` | 20 | `levels\multi\riverworld\shaders\forerunner\glass_wall_lakebase` | yes |
| `invisible__2` | rmsh | special | `h3_ua_special_rmsh` | 20 | `objects\multi\shaders\invisible` | yes |
| `human_military_decals` | rmd | decal_blend | `h3_b0_decal_blend_rmd` | 16 | `objects\gear\human\military\shaders\human_military_decals` | yes |
| `light_florescent_a_illum` | rmsh | opaque | `h3_r0_opaque_rmsh` | 14 | `levels\shared\shaders\multi\light_florescent_a_illum` | yes |
| `mongoose_palette_grate` | rmsh | alpha_blend | `h3_b0_alpha_blend_rmsh` | 12 | `objects\scenery\human\military\shaders\mongoose_palette_grate` | yes |
| `military_radio_display` | rmsh | opaque | `h3_r0_opaque_rmsh` | 10 | `objects\gear\human\military\shaders\military_radio_display` | yes |
| `invisible` | rmsh | special | `h3_ua_special_rmsh` | 9 | `fx\null_object\shaders\invisible` | no |
| `human_military_decals__2` | rmsh | alpha_blend | `h3_b0_alpha_blend_rmsh` | 8 | `objects\gear\human\military\shaders\human_military_decals` | yes |

### 6.2 cyberdyne (134)

| name | class | category | technique_set | triangles | tag | base colour |
|---|---|---|---|---|---|---|
| `spawn_point_disc` | rmsh | opaque | `h3_r0_opaque_rmsh` | 71,232 | `objects\multi\shaders\spawn_point_disc` | yes |
| `cyberdyne_modular_piece_brace` | rmsh | opaque | `h3_r0_opaque_rmsh` | 47,394 | `levels\multi\cyberdyne\shaders\cyberdyne_modular_piece_brace` | yes |
| `spawn_point_disc_pointer` | rmsh | opaque | `h3_r0_opaque_rmsh` | 42,188 | `objects\multi\shaders\spawn_point_disc_pointer` | yes |
| `cyberdyne_modular_piece_cables` | rmsh | opaque | `h3_r0_opaque_rmsh` | 24,394 | `levels\multi\cyberdyne\shaders\cyberdyne_modular_piece_cables` | yes |
| `tree_leaves_oval` | rmfl | foliage | `h3_t0_foliage_rmfl` | 21,920 | `levels\solo\010_jungle\shaders\foliage\tree_leaves_oval` | yes |
| `multiplayer_return_plate` | rmsh | opaque | `h3_r0_opaque_rmsh` | 18,772 | `objects\multi\models\shaders\multiplayer_return_plate` | yes |
| `pelican_hull` | rmsh | opaque | `h3_r0_opaque_rmsh` | 16,779 | `objects\vehicles\pelican\shaders\pelican_hull` | yes |
| `cyberdyne_wall_metal_a` | rmsh | opaque | `h3_r0_opaque_rmsh` | 14,368 | `levels\multi\cyberdyne\shaders\cyberdyne_wall_metal_a` | yes |
| `cyberdyne_metal_trim_a` | rmsh | opaque | `h3_r0_opaque_rmsh` | 12,798 | `levels\multi\cyberdyne\shaders\cyberdyne_metal_trim_a` | yes |
| `spawn_point_plasma2` | rmhg | additive | `h3_ua_additive_rmhg` | 11,872 | `objects\multi\shaders\spawn_point_plasma2` | yes |
| `tree_bark_smooth` | rmsh | opaque | `h3_r0_opaque_rmsh` | 10,158 | `levels\solo\010_jungle\shaders\foliage\tree_bark_smooth` | yes |
| `pelican_instances` | rmsh | opaque | `h3_r0_opaque_rmsh` | 8,582 | `objects\vehicles\pelican\shaders\pelican_instances` | yes |
| `cyberdyne_modular_piece_trim` | rmsh | opaque | `h3_r0_opaque_rmsh` | 8,304 | `levels\multi\cyberdyne\shaders\cyberdyne_modular_piece_trim` | yes |
| `metal_trim_d` | rmsh | opaque | `h3_r0_opaque_rmsh` | 7,174 | `levels\solo\040_voi\shaders\metals\metal_trim_d` | yes |
| `cyberdyne_bracket_big` | rmsh | opaque | `h3_r0_opaque_rmsh` | 6,332 | `levels\multi\cyberdyne\shaders\cyberdyne_bracket_big` | yes |
| `cyber_cables` | rmsh | opaque | `h3_r0_opaque_rmsh` | 6,148 | `levels\multi\cyberdyne\shaders\cyber_cables` | yes |
| `metal_rustbucket_b` | rmsh | opaque | `h3_r0_opaque_rmsh` | 6,096 | `levels\multi\cyberdyne\shaders\metal_rustbucket_b` | yes |
| `cyberdyne_metal_bracket` | rmsh | opaque | `h3_r0_opaque_rmsh` | 5,392 | `levels\multi\cyberdyne\shaders\cyberdyne_metal_bracket` | yes |
| `koth_shield_generator` | rmsh | opaque | `h3_r0_opaque_rmsh` | 5,071 | `objects\multi\shaders\koth_shield_generator` | yes |
| `military_equipment` | rmsh | opaque | `h3_r0_opaque_rmsh` | 4,922 | `objects\gear\human\military\shaders\military_equipment` | yes |
| `metal_d_painted_yellow` | rmsh | opaque | `h3_r0_opaque_rmsh` | 4,832 | `levels\solo\040_voi\shaders\metals\metal_d_painted_yellow` | yes |
| `military_equipment_2` | rmsh | opaque | `h3_r0_opaque_rmsh` | 4,638 | `objects\gear\human\military\shaders\military_equipment_2` | yes |
| `multiplayer_flag_return` | rmsh | opaque | `h3_r0_opaque_rmsh` | 4,320 | `objects\multi\models\shaders\multiplayer_flag_return` | yes |
| `missle_rack` | rmsh | opaque | `h3_r0_opaque_rmsh` | 3,608 | `objects\gear\human\military\shaders\missle_rack` | yes |
| `pelican_interior` | rmsh | opaque | `h3_r0_opaque_rmsh` | 3,514 | `objects\vehicles\pelican\shaders\pelican_interior` | yes |
| `cyberdyne_bracket` | rmsh | opaque | `h3_r0_opaque_rmsh` | 3,200 | `levels\multi\cyberdyne\shaders\cyberdyne_bracket` | yes |
| `metal_rustbucket_b__2` | rmsh | opaque | `h3_r0_opaque_rmsh` | 3,098 | `levels\solo\040_voi\shaders\metals\metal_rustbucket_b` | yes |
| `pallet` | rmsh | opaque | `h3_r0_opaque_rmsh` | 3,040 | `objects\gear\human\industrial\shaders\pallet` | yes |
| `out_girder_trim` | rmsh | opaque | `h3_r0_opaque_rmsh` | 2,616 | `levels\solo\030_outskirts\shaders\out_girder_trim` | yes |
| `cyberdyne_wall_concrete_d` | rmsh | opaque | `h3_r0_opaque_rmsh` | 2,493 | `levels\multi\cyberdyne\shaders\cyberdyne_wall_concrete_d` | yes |
| `metal_doodad_a` | rmsh | opaque | `h3_r0_opaque_rmsh` | 2,484 | `levels\solo\040_voi\shaders\lights\metal_doodad_a` | yes |
| `cyberdyne_modular_piece_panel` | rmsh | opaque | `h3_r0_opaque_rmsh` | 2,376 | `levels\multi\cyberdyne\shaders\cyberdyne_modular_piece_panel` | yes |
| `h_barrel_rusty` | rmsh | opaque | `h3_r0_opaque_rmsh` | 2,048 | `objects\gear\human\industrial\shaders\h_barrel_rusty` | yes |
| `cyberdyne_bulletpanels_a` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,892 | `levels\multi\cyberdyne\shaders\cyberdyne_bulletpanels_a` | yes |
| `cyberdyne_railing_panel_a` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,840 | `levels\multi\cyberdyne\shaders\cyberdyne_railing_panel_a` | yes |
| `generator_heavy_kettle` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,806 | `objects\gear\human\industrial\shaders\generator_heavy_kettle` | yes |
| `metal_fan` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,792 | `levels\solo\040_voi\shaders\metals\metal_fan` | yes |
| `out_big_pipe` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,720 | `levels\solo\030_outskirts\shaders\out_big_pipe` | yes |
| `street_cone` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,584 | `objects\gear\human\industrial\shaders\street_cone` | yes |
| `monitor_shader` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,270 | `objects\levels\solo\020_base\shaders\monitor_shader` | yes |
| `forklift_hull` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,214 | `objects\vehicles\civilian\forklift\shaders\forklift_hull` | yes |
| `metal_out_fence__2` | rmsh | alpha_blend | `h3_b0_alpha_blend_rmsh` | 1,152 | `levels\solo\040_voi\shaders\metals\metal_out_fence` | yes |
| `bathroom_items` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,140 | `objects\levels\solo\020_base\shaders\bathroom_items` | yes |
| `cyberdyne_wall_concrete_sections_universal` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,122 | `levels\multi\cyberdyne\shaders\cyberdyne_wall_concrete_sections_universal` | yes |
| `locker_closed` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,120 | `objects\gear\human\residential\shaders\locker_closed` | yes |
| `cyberdyne_wall_concrete_interior_a` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,105 | `levels\multi\cyberdyne\shaders\cyberdyne_wall_concrete_interior_a` | yes |
| `metal_doodad_a_illum_cool` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,104 | `levels\solo\040_voi\shaders\lights\metal_doodad_a_illum_cool` | yes |
| `office_wood` | rmsh | opaque | `h3_r0_opaque_rmsh` | 1,084 | `objects\gear\human\residential\shaders\office_wood` | yes |
| `jersey_barrier` | rmsh | opaque | `h3_r0_opaque_rmsh` | 960 | `objects\gear\human\industrial\shaders\jersey_barrier` | yes |
| `cyberdyne_wall_concrete_sections_universal_trim` | rmsh | opaque | `h3_r0_opaque_rmsh` | 942 | `levels\multi\cyberdyne\shaders\cyberdyne_wall_concrete_sections_universal_trim` | yes |
| `metal_out_fence` | rmsh | alpha_blend | `h3_b0_alpha_blend_rmsh` | 896 | `levels\multi\cyberdyne\shaders\metal_out_fence` | yes |
| `hb_metal_steel_grey` | rmsh | opaque | `h3_r0_opaque_rmsh` | 893 | `levels\solo\020_base\shaders\hb_metal_steel_grey` | yes |
| `out_big_girder` | rmsh | opaque | `h3_r0_opaque_rmsh` | 832 | `levels\solo\030_outskirts\shaders\out_big_girder` | yes |
| `cyberdyne_bulletpanels_b` | rmsh | opaque | `h3_r0_opaque_rmsh` | 800 | `levels\multi\cyberdyne\shaders\cyberdyne_bulletpanels_b` | yes |
| `sawhorse` | rmsh | opaque | `h3_r0_opaque_rmsh` | 800 | `objects\gear\human\industrial\shaders\sawhorse` | yes |
| `box_metal_small` | rmsh | opaque | `h3_r0_opaque_rmsh` | 792 | `objects\gear\human\industrial\shaders\box_metal_small` | yes |
| `cyberdyne_ground_concrete` | rmtr | terrain | `h3_r0_terrain_rmtr` | 774 | `levels\multi\cyberdyne\shaders\cyberdyne_ground_concrete` | yes |
| `metal_doodad_a_illum_cool__2` | rmsh | opaque | `h3_r0_opaque_rmsh` | 766 | `levels\multi\cyberdyne\shaders\metal_doodad_a_illum_cool` | yes |
| `brute_target_trim` | rmsh | opaque | `h3_r0_opaque_rmsh` | 700 | `objects\levels\multi\cyberdyne\shaders\brute_target_trim` | yes |
| `gun_rack` | rmsh | opaque | `h3_r0_opaque_rmsh` | 696 | `objects\levels\solo\020_base\shaders\gun_rack` | yes |
| `camping_stool_legs` | rmsh | opaque | `h3_r0_opaque_rmsh` | 616 | `objects\gear\human\military\shaders\camping_stool_legs` | yes |
| `fusion_coil_plasma` | rmsh | additive | `h3_ua_additive_rmsh` | 576 | `objects\gear\human\military\shaders\fusion_coil_plasma` | no |
| `tarp_crate_lg_a` | rmsh | opaque | `h3_r0_opaque_rmsh` | 550 | `objects\gear\human\industrial\shaders\tarp_crate_lg_a` | yes |
| `tarp_box_stack` | rmsh | opaque | `h3_r0_opaque_rmsh` | 544 | `objects\gear\human\industrial\shaders\tarp_box_stack` | yes |
| `human_military_decals_2` | rmd | decal_blend | `h3_b0_decal_blend_rmd` | 528 | `objects\gear\human\military\shaders\human_military_decals_2` | yes |
| `metal_i_generic_cruddy` | rmsh | opaque | `h3_r0_opaque_rmsh` | 528 | `levels\solo\040_voi\shaders\metals\metal_i_generic_cruddy` | yes |
| `metal_trim_c` | rmsh | opaque | `h3_r0_opaque_rmsh` | 520 | `levels\solo\040_voi\shaders\metals\metal_trim_c` | yes |
| `hb_metal_trim_01` | rmsh | opaque | `h3_r0_opaque_rmsh` | 490 | `levels\solo\020_base\shaders\hb_metal_trim_01` | yes |
| `cyberdyne_grate_a` | rmsh | alpha_test | `h3_t0_alpha_test_rmsh` | 472 | `levels\multi\cyberdyne\shaders\cyberdyne_grate_a` | yes |
| `cyberdyne_modular_piece_paneldark` | rmsh | opaque | `h3_r0_opaque_rmsh` | 456 | `levels\multi\cyberdyne\shaders\cyberdyne_modular_piece_paneldark` | yes |
| `speaker` | rmsh | opaque | `h3_r0_opaque_rmsh` | 416 | `objects\levels\solo\020_base\shaders\speaker` | yes |
| `cov_holo_generator` | rmsh | opaque | `h3_r0_opaque_rmsh` | 404 | `objects\gear\covenant\military\shaders\cov_holo_generator` | yes |
| `cyber_boilerplate` | rmsh | opaque | `h3_r0_opaque_rmsh` | 396 | `levels\multi\cyberdyne\shaders\cyber_boilerplate` | yes |
| `metal_k_wireholder_inset_overframe` | rmsh | alpha_test | `h3_t0_alpha_test_rmsh` | 372 | `levels\solo\040_voi\shaders\metals\metal_k_wireholder_inset_overframe` | yes |
| `rucksack` | rmsh | opaque | `h3_r0_opaque_rmsh` | 328 | `objects\gear\human\military\shaders\rucksack` | yes |
| `cyberdyne_wall_wood_a` | rmsh | opaque | `h3_r0_opaque_rmsh` | 316 | `levels\multi\cyberdyne\shaders\cyberdyne_wall_wood_a` | yes |
| `cyberdyne_roof` | rmsh | opaque | `h3_r0_opaque_rmsh` | 302 | `levels\multi\cyberdyne\shaders\cyberdyne_roof` | yes |
| `office_metal` | rmsh | opaque | `h3_r0_opaque_rmsh` | 300 | `objects\gear\human\residential\shaders\office_metal` | yes |
| `telephone_wall_box` | rmsh | opaque | `h3_r0_opaque_rmsh` | 253 | `objects\gear\human\residential\shaders\telephone_wall_box` | yes |
| `out_voi_corrug` | rmsh | opaque | `h3_r0_opaque_rmsh` | 216 | `levels\solo\030_outskirts\shaders\out_voi_corrug` | yes |
| `cyberdyne_roof_glass_a` | rmsh | alpha_blend | `h3_b0_alpha_blend_rmsh` | 200 | `levels\multi\cyberdyne\shaders\cyberdyne_roof_glass_a` | yes |
| `rucksack_metal` | rmsh | opaque | `h3_r0_opaque_rmsh` | 186 | `objects\gear\human\military\shaders\rucksack_metal` | yes |
| `cyberdyne_catwalk_railings` | rmsh | alpha_test | `h3_t0_alpha_test_rmsh` | 152 | `levels\multi\cyberdyne\shaders\cyberdyne_catwalk_railings` | yes |
| `military_decals` | rmd | decal_blend | `h3_b0_decal_blend_rmd` | 136 | `objects\gear\human\military\shaders\military_decals` | yes |
| `office_metalb` | rmsh | opaque | `h3_r0_opaque_rmsh` | 136 | `objects\gear\human\residential\shaders\office_metalb` | yes |
| `cyberdyne_modular_piece_solid` | rmsh | opaque | `h3_r0_opaque_rmsh` | 126 | `levels\multi\cyberdyne\shaders\cyberdyne_modular_piece_solid` | yes |
| `cyberdyne_floor_ramp_a` | rmsh | opaque | `h3_r0_opaque_rmsh` | 116 | `levels\multi\cyberdyne\shaders\cyberdyne_floor_ramp_a` | yes |
| `metal_factory_outerpanel_i` | rmsh | opaque | `h3_r0_opaque_rmsh` | 112 | `levels\solo\040_voi\shaders\metals\metal_factory_outerpanel_i` | yes |
| `missle_caution` | rmsh | opaque | `h3_r0_opaque_rmsh` | 110 | `objects\gear\human\military\shaders\missle_caution` | yes |
| `cyberdyne_bulletpanels_c` | rmsh | opaque | `h3_r0_opaque_rmsh` | 108 | `levels\multi\cyberdyne\shaders\cyberdyne_bulletpanels_c` | yes |
| `opaq_cyber_glass` | rmsh | opaque | `h3_r0_opaque_rmsh` | 104 | `levels\multi\cyberdyne\shaders\opaq_cyber_glass` | yes |
| `toolbox` | rmsh | opaque | `h3_r0_opaque_rmsh` | 100 | `objects\gear\human\industrial\shaders\toolbox` | yes |
| `cyberdyne_metal_painted_tower_a` | rmsh | opaque | `h3_r0_opaque_rmsh` | 96 | `levels\multi\cyberdyne\shaders\cyberdyne_metal_painted_tower_a` | yes |
| `camping_stool_fabric` | rmsh | opaque | `h3_r0_opaque_rmsh` | 96 | `objects\gear\human\military\shaders\camping_stool_fabric` | yes |
| `cyberdyne_floor_metal` | rmsh | opaque | `h3_r0_opaque_rmsh` | 95 | `levels\multi\cyberdyne\shaders\cyberdyne_floor_metal` | yes |
| `cyberdyne_wall_metal_c` | rmsh | opaque | `h3_r0_opaque_rmsh` | 86 | `levels\multi\cyberdyne\shaders\cyberdyne_wall_metal_c` | yes |
| `pelican_glass` | rmsh | additive | `h3_ua_additive_rmsh` | 84 | `objects\vehicles\pelican\shaders\pelican_glass` | yes |
| `monitor_screen_shader` | rmsh | opaque | `h3_r0_opaque_rmsh` | 75 | `objects\levels\solo\020_base\shaders\monitor_screen_shader` | yes |
| `monitor_screen_glass` | rmsh | additive | `h3_ua_additive_rmsh` | 75 | `objects\levels\solo\020_base\shaders\monitor_screen_glass` | yes |
| `cyberdyne_tower_floor` | rmsh | opaque | `h3_r0_opaque_rmsh` | 64 | `levels\multi\cyberdyne\shaders\cyberdyne_tower_floor` | yes |
| `rubber_wire` | rmsh | opaque | `h3_r0_opaque_rmsh` | 64 | `levels\solo\040_voi\shaders\plastic\rubber_wire` | yes |
| `human_military_decals` | rmsh | alpha_blend | `h3_b0_alpha_blend_rmsh` | 64 | `objects\gear\human\military\shaders\human_military_decals` | yes |
| `human_industrial_decals` | rmd | decal_blend | `h3_b0_decal_blend_rmd` | 60 | `objects\gear\human\industrial\shaders\human_industrial_decals` | yes |
| `target_illum` | rmsh | opaque | `h3_r0_opaque_rmsh` | 60 | `objects\levels\multi\cyberdyne\shaders\target_illum` | yes |
| `metal_doodad_a_illum_warm` | rmsh | opaque | `h3_r0_opaque_rmsh` | 56 | `levels\solo\040_voi\shaders\lights\metal_doodad_a_illum_warm` | yes |
| `rusted_industrial_grating` | rmsh | alpha_test | `h3_t0_alpha_test_rmsh` | 56 | `objects\levels\solo\020_base\shaders\rusted_industrial_grating` | yes |
| `brute_target` | rmsh | opaque | `h3_r0_opaque_rmsh` | 56 | `objects\levels\multi\cyberdyne\shaders\brute_target` | yes |
| `cyberdyne_roof_solid_a` | rmsh | opaque | `h3_r0_opaque_rmsh` | 48 | `levels\multi\cyberdyne\shaders\cyberdyne_roof_solid_a` | yes |
| `nylon_strap` | rmsh | opaque | `h3_r0_opaque_rmsh` | 48 | `objects\gear\human\military\shaders\nylon_strap` | yes |
| `flare_a_red` | rmsh | additive | `h3_ua_additive_rmsh` | 40 | `levels\solo\040_voi\bitmaps\lights\flare_a_red` | yes |
| `quartz_light` | rmsh | alpha_test | `h3_t0_alpha_test_rmsh` | 40 | `objects\gear\human\military\shaders\quartz_light` | yes |
| `flare_a_red_blinky` | rmsh | additive | `h3_ua_additive_rmsh` | 36 | `levels\solo\040_voi\bitmaps\lights\flare_a_red_blinky` | yes |
| `metal_factory_outerpanel_a` | rmsh | opaque | `h3_r0_opaque_rmsh` | 34 | `levels\solo\040_voi\shaders\metals\metal_factory_outerpanel_a` | yes |
| `cyberdyne_wall_concrete_a` | rmsh | opaque | `h3_r0_opaque_rmsh` | 32 | `levels\multi\cyberdyne\shaders\cyberdyne_wall_concrete_a` | yes |
| `generator_turbine` | rmsh | opaque | `h3_r0_opaque_rmsh` | 32 | `objects\gear\human\military\shaders\generator_turbine` | yes |
| `cov_camp_stool_antigrav` | rmhg | additive | `h3_ua_additive_rmhg` | 32 | `objects\gear\covenant\military\shaders\cov_camp_stool_antigrav` | yes |
| `hb_metal_painted_red_pillar_unwrap_a` | rmsh | opaque | `h3_r0_opaque_rmsh` | 28 | `levels\solo\020_base\shaders\hb_metal_painted_red_pillar_unwrap_a` | yes |
| `jackal_target` | rmsh | opaque | `h3_r0_opaque_rmsh` | 28 | `objects\levels\multi\cyberdyne\shaders\jackal_target` | yes |
| `forklift_grate` | rmsh | alpha_test | `h3_t0_alpha_test_rmsh` | 24 | `objects\vehicles\civilian\forklift\shaders\forklift_grate` | yes |
| `grid_zone` | rmsh | additive | `h3_ua_additive_rmsh` | 24 | `objects\levels\multi\cyberdyne\shaders\grid_zone` | yes |
| `invisible` | rmsh | special | `h3_ua_special_rmsh` | 20 | `objects\multi\shaders\invisible` | yes |
| `office_monitor_screen` | rmsh | opaque | `h3_r0_opaque_rmsh` | 18 | `objects\gear\human\residential\shaders\office_monitor_screen` | yes |
| `cyberdyne_tower_ramp` | rmsh | opaque | `h3_r0_opaque_rmsh` | 16 | `levels\multi\cyberdyne\shaders\cyberdyne_tower_ramp` | yes |
| `mp_floor_rubber` | rmsh | opaque | `h3_r0_opaque_rmsh` | 16 | `levels\shared\shaders\multi\mp_floor_rubber` | yes |
| `jackal_grid` | rmsh | additive | `h3_ua_additive_rmsh` | 12 | `objects\levels\multi\cyberdyne\shaders\jackal_grid` | yes |
| `cyberdyne_grate_drain` | rmsh | alpha_test | `h3_t0_alpha_test_rmsh` | 8 | `levels\multi\cyberdyne\shaders\cyberdyne_grate_drain` | yes |
| `cyberdyne_ground_outer` | rmtr | terrain | `h3_r0_terrain_rmtr` | 8 | `levels\multi\cyberdyne\shaders\cyberdyne_ground_outer` | yes |
| `box_labels` | rmsh | opaque | `h3_r0_opaque_rmsh` | 8 | `objects\gear\human\industrial\shaders\box_labels` | yes |
| `telephone_wall_animated` | rmsh | additive | `h3_ua_additive_rmsh` | 6 | `objects\gear\human\residential\shaders\telephone_wall_animated` | yes |
| `bathroom_mirror` | rmsh | opaque | `h3_r0_opaque_rmsh` | 4 | `objects\levels\solo\020_base\shaders\bathroom_mirror` | yes |
| `panel_logo_decal` | rmd | decal_blend | `h3_b0_decal_blend_rmd` | 4 | `objects\gear\human\industrial\shaders\panel_logo_decal` | yes |
| `light_glass` | rmsh | additive | `h3_ua_additive_rmsh` | 4 | `objects\gear\human\military\shaders\light_glass` | yes |
| `cyber_plasma_sm_red` | rmsh | additive | `h3_ua_additive_rmsh` | 4 | `objects\levels\multi\cyberdyne\shaders\cyber_plasma_sm_red` | yes |
| `forklift_glass` | rmsh | additive | `h3_ua_additive_rmsh` | 4 | `objects\vehicles\civilian\forklift\shaders\forklift_glass` | yes |

---

## 7. Sidecars next to the riverworld export

* `analysis.json` / `analysis.png` are **not** HaloExport outputs: they come from the mw2-to-reflex
  `analyse_gltf.py` run on `scene.gltf` with the sidecar terrain regex (README "Then analyse"). They hold:
  `scene_bounds_m` = `bsp_bounds_m` = [-124.5, -28.3, -281.9, 629.5, 277.0, 870.0]; `triangles` 311,097;
  `bsp_triangles` 37,260; `placed_models` 505; `playable_box_m` [145.7, 325.7, 127.7, 477.7] (x min, x
  max, z min, z max); `focus_m` [250.7, 442.7]; `triangles_in_box` 295,045; `floor` {floor_m -1.1259,
  source "water", water_m -1.1259}; `ground_materials[]` (up-facing area per material: `riverworld_ground`
  26,399 m2 share 0.373 median y 6.41 m; `riverworld_ground_cliff` 11,533 / 0.163 / 54.42; `riverworld_water_rough`
  9,345 / 0.132 / 2.3 (mode skip); `riverworld_ground_water` 7,885 / 0.111 / 1.69; `riverworld_cliff_blend`
  6,952 / 0.098 / 13.71; `riverworld_ground_snow` 3,026 / 0.043 / 10.03; `panel_generic_simple` 2,254;
  `cust_base_floor` 1,580; ...); `terrain_regex_coverage` 0.788; `start` {x 275.48, z 467.4, heading 315,
  clear_m 8, slope_deg 1.7, ground_m 4.27, row 679, col 259}; `suggest` {box [131, 341, 113, 493],
  mesh_box_args, playable_half_width_ft 623, whole_map_args}. Useful only as a pipeline reference.
* `textures/` holds the 62 PNGs (`<bitmap>-<tagid>.png` / `_rgb.png`).
* `<HaloExport output>\riverworld-sky\` is the same export with `--sky`: nodes world 1 + sky 1;
  sky 8,462 triangles, 5,040 vertices, 13 primitives (materials `skydome`, `clouds_hazy_wisps`,
  `clouds_cumulus`, `clouds_fore_frontlit`, `mountains_snow`, `mountains_terrain`, `clouds_patchy`,
  `clouds_horizon`, `matte_waypoint_reverse`, `sun_halves`, `sun_ring`, `sun_lines`, `sundisk`); sky bounds
  min_m [-23036.86, -1522.06, -23046.26] max_m [23036.86, 22543.64, 23046.26] (151,160 x 78,956 x 151,222 ft);
  41 materials total; no matrix on the sky node.
* `riverworld-warncheck` / `riverworld-sky-warncheck` are byte-identical re-exports (README "Image check").
  `riverworld-creekfix` is an older variant; not examined.

---

## 8. Checklist: our reader must reproduce

Container and index (riverworld.map):

- [ ] build `Dec 21 2023 22:31:37`, 11,658 tags, 183 classes, scenario `levels\multi\riverworld\riverworld`
      found as the `scnr` whose name equals the header scenario path (tag index 8).
- [ ] `scnr` block counts: StructureBsps 1, Skies 1, Scenery 142 (palette 26), Crates 102 (palette 43),
      Machines 0, Controls 0, ObjectNames 1; lightmap ref -> `sLdT` 6878.
- [ ] `sbsp` 9757: 26 clusters (sections 76..101), 262 instances (95 with scale != 1), 64 shaders, 76
      bounding boxes, bounds X (-40.85219, 206.53394) Y (-285.42664, 92.48665) Z (-15.51741, 1892.30896).
- [ ] `Lbsp` 6879 found through `sLdT.LightmapRefs` with BspIndex 0; 102 sections, all TriangleList;
      VertexFormat 1 for sections 0..75 and 0 for 76..101; resource index 3290.
- [ ] zone entry 3290: segment 3254 -> local page 1420 (45,416,454 -> 92,027,152 bytes deflate), segment
      offset 25,733,816; fixups: 394 vertex buffers (319,934 vertices), 109 index buffers; first vertex buffer
      702 vertices / 25,272 bytes; vertex data for section 0 at resource offset 0x0; index buffer 0 at 0xB578F0.
- [ ] shared caches named `maps\english.map`, `maps\mainmenu.map`, `maps\shared.map`; local data table
      address 0x114000.

Geometry (riverworld):

- [ ] World (all non-empty clusters, untransformed): **37,260 triangles, 35,256 vertices**, 28 material
      groups as in 5.1, AABB in wu X (-40.85219, 206.53394), Y (-285.42664, 92.48665), Z (-9.28856, 90.87501).
- [ ] 3 clusters (0, 1, 23) have no mesh data and must be skipped without error.
- [ ] 261 instance placements from 262 instances (instance 71 -> section 34 has no data), 75 distinct
      sections, **87,367 placed triangles, 98,270 placed vertices**; placed AABB (corner method) wu
      X (34.0472, 130.7405) Y (-235.6518, -10.1313) Z (-3.8121, 56.8764).
- [ ] Instance transform = S(TransformScale) * raw 4x3 (translation row last); the `rock#bsp0-00092` and
      `rock#bsp0-00093` vectors in 5.3 match to 1e-4.
- [ ] Per-material triangle totals equal the riverworld list in 6.1 (top: `spawn_point_disc` 79,968,
      `spawn_point_disc_pointer` 47,362, `riverworld_tree_leafa` 33,432; World-only: `riverworld_cliff_blend`
      11,545, `riverworld_ground` 8,130, `riverworld_water_rough` 2,607).
- [ ] Material classification reproduces the category column of 6.1 (42 opaque, 6 terrain, 5 foliage, 3
      alpha_test, 6 alpha_blend, 3 decal_blend, 4 additive, 1 water, 2 special).

Objects (riverworld):

- [ ] 142 scenery + 102 crate placements, all with a valid palette tag and a render model; object -> `hlmt`
      (+52) -> `mode` (+0); first permutation of each region plus instanced permutations.
- [ ] Placement matrix S * R(yaw, pitch, roll) * T reproduces the `respawn_point#scenery-0000`,
      `man_cannon_river#crate-0001`, `koth_hill_static#crate-0001` vectors in 5.3.
- [ ] Totals: scenery 146,845 triangles / 238,975 vertices; crates 39,625 / 50,103; 21 and 35 distinct
      models; bounds as in 5.2.
- [ ] `skies` = [`levels\multi\riverworld\sky\riverworld`] (scen tag 11); sky model 8,462 triangles, 13
      materials if loaded.

Cross-check on cyberdyne.map: 48 clusters (44 with data), 834 instances (830 placed, 56 scaled, 307
distinct sections), World 12,240 triangles / 18,839 vertices / 23 materials, instances 229,319 / 320,314,
scenery 132 nodes 139,652 / 224,356, crates 131 nodes 50,382 / 68,739, machines 8 nodes 3,200 / 4,162
(tags `jump_fan` x 2, `brute_target`, `jackal_target`), 134 materials, World wu X (-50.0, 50.0) Y (-21.433,
85.338) Z (-2.700, 8.500); `jump_fan#machine-0000` matrix in 5.3.

---

## 9. Last Resort (`zanzibar`), added 2026-10-28

HaloExport sidecar `<HaloExport output>\zanzibar\halo_export.json` (same build, `MccHalo3U13`)
against `blam geometry zanzibar`. Every count matches exactly; `last_resort_matches_haloexport` in
`crates/blam_geometry/tests/maps.rs` pins them.

| item | sidecar | ours |
|---|---|---|
| bsps | 1, `levels\multi\zanzibar\zanzibar` | same |
| clusters / with a mesh | 40 / 39 | 40 / 39 |
| instances / placed | 1,130 / 1,123 | 1,130 / 1,123 |
| bounds_wu x / y / z | [-99.67684, 44.225243] / [-68.49561, 160.14279] / [-8.990257, 20.045654] | same (stated sbsp bounds) |
| triangles: world / instance | 38,991 / 316,212 | same |
| triangles: scenery / crate / machine / control | 183,060 / 82,432 / 3,192 / 764 | same |
| nodes: scenery / crate / machine / control | 178 / 168 / 3 / 1 | 312 / 273 / 3 / 2 placements (one per mesh of a multi-mesh object) |
| materials | 141 | 155 scene materials (14 more from the sky model, which HaloExport left out) |
| colour bitmaps | 146 written, 0 missing | 146 decoded, 0 failed (`blam textures`) |
| water | `zanzibar_water_awesome`, 1,846 triangles | same; a flat sheet at z -3.0 from x -74.2 to 34.9, y 16.1 to 112.6 |
| skies | [`levels\multi\zanzibar\sky\sky`] | 12 sky placements, 11,026 triangles |

Ours only (no sidecar equivalent):

| item | value |
|---|---|
| shaders in the sbsp | 112 |
| sections / with geometry | 376 / 368 |
| collision | 31,784 BSP triangles, 86,394 instanced from 790 of 1,130 instances (340 have no collision), 65 materials; 1,908 invisible, 2,621 slip, 3,158 two-sided |
| placed geometry bounds (sky left out) | x -74.224..41.792, y -68.496..112.634, z -11.563..16.904 |
| spawn points | 106 (`respawn_point` and `respawn_point_invisible`); each stands on collision less than 0.5 wu over it, with nothing through the body (checked by the test) |
| start spawn | 0: [14.96, 23.92, -0.86] facing -168 deg, on the beach |
| camera fx | exposure -4.47 stops, mean irradiance 0.317 -> x4.65 |
| rails | 45,973 walkable edges, 11,517 lips, 10,094 runs, 3,954 rails; 3,828 outside the kill volumes; 3,824 out of deep water (a 108 wu rail along the sea bed off the beach, z -5.2 to -3.5, ran up to 2 wu under the surface) |

Shaders borrowed from other levels resolve like the map's own: `levels\solo\040_voi\...` (metals, concrete,
signs, `light_volume_a`), `levels\solo\020_base\...`, `levels\solo\010_jungle\...` (palms, bark),
`levels\multi\deadlock\...` (chain-link fence, rubble edge). Decals (`panel_logo_decal`,
`human_military_decals*`, `military_decals`, `panel_crate_shipping`) are `rmd ` alpha blend;
`forklift_glass` is a glass `rmsh` with additive blending; `deadlock_screen` is opaque with a scrolling
`self_illum_map`; the 21 `box_*` / `wall_*` scenery pieces (Forge blocks) use `objects\multi\shaders\invisible` and are not drawn.

## 10. High Ground (`deadlock`), added 2026-10-28

HaloExport sidecar `<HaloExport output>\deadlock\halo_export.json` (same build, `MccHalo3U13`)
against `blam geometry deadlock`. Every count matches exactly; `high_ground_matches_haloexport` in
`crates/blam_geometry/tests/maps.rs` pins them.

| item | sidecar | ours |
|---|---|---|
| bsps | 1, `levels\multi\deadlock\deadlock` | same |
| clusters / with a mesh | 43 / 43 | 43 / 43 |
| instances / placed | 857 / 849 | 857 / 849 |
| bounds_wu x / y / z | [-62.073673, 48.44192] / [-72.087036, 83.86857] / [2.225637, 41.033073] | same (stated sbsp bounds) |
| triangles: world / instance | 38,754 / 313,216 | same |
| triangles: scenery / crate / machine / control | 141,099 / 68,677 / 2,276 / 72 | same |
| nodes: scenery / crate / machine / control | 154 / 196 / 2 / 2 | 277 / 271 / 2 / 2 placements (one per mesh of a multi-mesh object) |
| machine bounds (m) | min (-26.14, 35.36, -26.45), max (-14.27, 41.91, -10.37) | the gate `wall_door` at (-6.153, 4.273, 11.588) wu and `switch_lever` at (-8.544, 8.745, 13.270): together x -8.58..-4.69, y 3.40..8.68, z 11.60..13.75 wu, the same box |
| materials | 110 | 122 scene materials (the sky model's, which HaloExport left out, and the spawn markers) |
| colour bitmaps | 100 written, 0 missing | 101 decoded, 0 failed (`blam textures`) |
| water | `deadlock_water_awesome`, 635 triangles | same; a flat sheet at z 4.41 over x -37.0..15.1, y -67.8..-25.5 (the cove) |
| skies | [`levels\multi\deadlock\sky\deadlock`] | 9 sky placements, 8,649 triangles |

Ours only (no sidecar equivalent):

| item | value |
|---|---|
| shaders in the sbsp | 79 |
| sections / with geometry | 318 / 310 |
| collision | 35,890 BSP triangles, 89,094 instanced from 857 instances, 40 materials; 5,124 invisible, 6,510 two-sided. Every up-facing opaque render floor (clusters and instances, sampled every 0.25 wu) has collision within 0.3 wu but 124 scattered samples, none in a cluster: no holes to fall through |
| placed geometry bounds (sky left out) | x -59.063..42.328, y -67.846..83.869, z -0.907..29.620 |
| spawn points | 85 (79 `respawn_point`, 6 `respawn_point_invisible`); each stands on collision less than 0.5 wu over it, with nothing through the body (checked by the test). Seven on the beach (spawns 8, 16, 17, 46, 71-73, z 4.17-4.30) stand in 0.1-0.25 wu of water |
| start spawn | 0: [1.70, 14.15, 11.75] facing 177 deg, inside the base |
| kill volume | `kill_water2`, under the sea bed past the cove (corner (-12.9, -100.2, -4.3), 56 x 60 x 5.6 wu); no spawn in it. `safe_zone` and `flock0` are other trigger volumes |
| camera fx | exposure -4.64 stops (auto range 0.5..3.0), mean irradiance 0.456 -> x4.33 |
| rails | 52,662 walkable edges, 10,085 lips, 9,850 runs, 3,306 rails; 3,305 out of deep water |
| lit placements (viewer) | 368 nearest surface, 192 per-pixel, 693 per-vertex, 9 sky; 191 spawn-marker pieces not drawn |

The sky model's far sea, `lake` (63 triangles, z 2.86..3.67 out to x -190..196, y -361..37), is an opaque
cook-torrance `rmsh` whose base map is the sky's `terrain_africa`; it also runs under the cove's bed. Round
the cove lie `water_edge_breakup` cards (17 triangles, z 3.0..3.8), alpha-tested by their own map (alpha
mean 0.14). The two controls `wall_switch` and `wall_switch_on` are the same 36-triangle switch box placed
at the same spot. Shaders shared with Last Resort (`deadlock_screen`, industrial and medical gear) resolve the
same way; `medical_decals` is a `rmd ` with `blend_mode multiply`.

## 11. Isolation (`isolation`), added 2026-10-29

HaloExport sidecar `<HaloExport output>\isolation\halo_export.json` (same build, `MccHalo3U13`)
against `blam geometry isolation`. Every count matches exactly; `isolation_matches_haloexport` in
`crates/blam_geometry/tests/maps.rs` pins them.

| item | sidecar | ours |
|---|---|---|
| bsps | 1, `levels\multi\isolation\isolation` | same |
| clusters / with a mesh | 49 / 47 | 49 / 47 |
| instances / placed | 391 / 391 | 391 / 391 |
| bounds_wu x / y / z | [-112.74447, 112.74448] / [-39.311527, 43.637577] / [-14.824824, 47.826878] | same (stated sbsp bounds) |
| triangles: world / instance | 18,987 / 191,321 | same |
| triangles: scenery / crate / machine / control | 148,112 / 19,108 / 3,134 / none | same |
| nodes: scenery / crate / machine | 189 / 51 / 1 | 299 / 51 / 9 placements (one per mesh of a multi-mesh object) |
| machine | `flood_door_fast` (mesh 137), glTF translation (0.102, -29.917, -29.608) m, turned 15 degrees about x | the same three pieces; computed from the glTF primitives they span x -2.671..2.699, y 5.874..11.289, z -10.932..-5.720 wu, and ours span the same. The sidecar's `bounds.machine` box (z -11.11..-5.39) is wider: it seems to box the rotated box, not the vertices |
| materials | 47 | 61 scene materials: the 47, the sky model's 12 (HaloExport left the sky out) and 2 no placed triangle uses |
| colour bitmaps | 46 written, 0 missing (`z_null_dark`, `z_invis_lightquad` have none) | 49 decoded, 0 failed (`blam textures`) |
| water | none | none: no `rmw ` shader |
| skies | [`levels\multi\isolation\sky\sky`] | 6 sky placements, 5,748 triangles |

Ours only (no sidecar equivalent):

| item | value |
|---|---|
| shaders in the sbsp | 43 |
| sections / with geometry | 149 / 147 |
| collision | 16,280 BSP triangles, 79,954 instanced from 391 instances, 27 materials; 704 invisible, 832 two-sided. Bounds x -44.2..44.2, y -39.3..43.6, z -11.34..43.5, reaching well past the yard's walls. Up-facing opaque render floors (about 52,000 random samples, one per 0.0625 square wu, up to 50 per triangle) all have collision within 0.3 wu in every cluster; the misses are tree branches, ledges high on the outer wall's fins, the Flood balls and the Flood door (objects have no collision here) and a few scattered samples on the Flood mounds and plates |
| placed geometry bounds (sky left out) | x -25.567..38.975, y -16.208..19.440, z -11.343..21.325 |
| spawn points | 91 (84 `respawn_point`, 7 `respawn_point_invisible`), z -11.18..-1.76; each stands on collision less than 0.5 wu over it, with nothing through the body (checked by the test) |
| start spawn | 0: [18.82, 1.18, -7.46] facing 178 deg, on the lawn at the +x end |
| trigger volumes | `safe_zone` (corner (-25.68, -15.50, -11.84), 51.8 x 33.3 x 15.5 wu: the yard; every spawn is inside), `flock0`, `flock2`, `flock3` (birds, beyond the walls). No kill volume: the walls keep everyone in, and anything that did get out is caught 20 wu under the lowest collision (z -31.3) |
| camera fx | exposure -5.06 stops (auto range 0..3), mean irradiance 0.310 -> x3.65 |
| atmosphere | `levels\multi\isolation\sky\isolation`: world and sky setting `flood_fog` (the green haze in the cave and tunnels) |
| rails | 39,368 walkable edges, 8,830 lips, 7,907 runs, 3,904 rails (3,902 on another run); none dropped (no kill volume, no water) |
| lit placements (viewer) | 99 nearest surface, 160 per-pixel, 277 per-vertex, 6 sky; 261 spawn-marker pieces not drawn |

Shaders of note. The Flood growths borrow campaign shaders: `flood_sack` (the pulsing Flood balls,
`objects\levels\solo\060_floodship\...`) is `material_model organism` with a `subsurface_map`,
`transparence_map` and `flat_environment_map`; the viewer draws `organism` as diffuse only. The Flood
door uses `cook_torrance`; `cust_flood_bag` (the mounds) and the `110_hc` flood plates `two_lobe_phong`.
The energy walls, the glowing slits round the outer wall, are three additive `plasma` shaders
(`plasma_wall_a/b/c`, sharp band only, thinness 512, colour (0.39, 1, 0.93)) whose
`self_illum_intensity` pulses 0 -> 55 over 5, 8 and 10 s; `a` stores 55, `b` and `c` store 0 (see
`docs/format/shaders.md` 11.4). The tunnel lights are `light_crusty_warm/cool` (`simple` self-illum x8)
with additive `light_cone_warm/cool` beams (`material_model none`, `illum_detail`, base
`gray_50_percent`) and `z_invis_lightquad` (alpha-tested, no colour map, not drawn). The sky borrows The
Ark's (`070_waste`) sun, galaxy and nebula bitmaps; its dome (`skydome`) is opaque `gray_50_percent`
times a scrolling detail map, lit, with an `illum_detail` glow whose map (`skydome_detail2`) is black (mean 0.000), so the
viewer's sky shader drawing `illum_detail` as a plain map loses nothing there. No decals (`rmd `).
