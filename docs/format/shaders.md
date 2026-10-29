# Halo 3 shaders: from a render-method tag to its textures, options and tiling (MCC U13)

What the renderer needs from a material: which bitmap is the colour, how it tiles, whether
pixels are cut out (alpha test), how the surface blends (opaque / additive / alpha blend ...)
and, for terrain, how the four layers are mixed. This document records the tag layouts and
the exact lookup algorithm, resolved to the one cache version we read.

- Target: MCC Halo 3 build `Dec 21 2023 22:31:37` = Reclaimer `CacheType.MccHalo3U13`,
  enum ordinal **22** (counted in `Reclaimer.Blam\Blam\Common\Enums.cs` from `Halo1Xbox = 0`:
  ... `MccHalo3 16`, `MccHalo3U4 17`, `MccHalo3F6 18`, `MccHalo3U6 19`, `MccHalo3U9 20`,
  `MccHalo3U12 21`, `MccHalo3U13 22`, `Halo3ODST 23`). Reclaimer's
  `[Offset(x, MinVersion=A, MaxVersion=B)]` is inclusive A, exclusive B. Every value below is
  already resolved to U13.
- Sources: Gravemind2401's Reclaimer, clone at `D:\reflex-tools\halo-tools\Reclaimer`
  (GPL-3.0; **format facts only, no code copied**): `Reclaimer.Blam\Blam\Halo3\ShaderTag.cs`,
  `Halo3\RenderMethodTemplateTag.cs`, `Halo3\RenderMethodDefinitionTag.cs`,
  `Halo3\Halo3Common.cs` (`GetMaterials`), `Common\Gen3\Gen3MaterialHelper.cs`,
  `Common\Gen3\Gen3Constants.cs`, `Reclaimer.Core\Geometry\Material.cs` (enum values). The
  category rules come from HaloExport (`D:\reflex-tools\halo-tools\HaloExport\Program.cs`,
  methods `Analyse` and `MaterialIndex`, described not copied) and the material table in
  `D:\reflex-tools\halo-tools\README.md`.
- Verified on `riverworld.map` (Valhalla) with `docs\format\scripts\probe_shaders.py`
  (read-only, run with `python -I`; modes: default = the five sample shaders, `--survey` =
  every render-method tag in the map, `--rmdf` / `--rmdf-detail` = the ten definitions,
  `--rmt2` = the five templates). Cross-checked against HaloExport's independent output for the
  same map (`D:\reflex-tools\halo-exports\riverworld\scene.gltf`, `materials[].extras.halo`):
  every bitmap, xform, option and category below agrees.
- Container reminders (`container.md`): tag-data 32-bit pointers are stored `>> 2`;
  TagBlock = 12 bytes `{i32 count, u32 shifted ptr, 4 pad}`; TagReference = 16 bytes
  `{4CC class, 8 skip, i32 tag id}` (tag index = low 16 bits, `-1` = null); StringId = i32
  (19 index bits, 8 namespace bits). All offsets are hex (decimal). Little-endian.

Contents: 1 tag chain, 2 shader tag, 3 rmt2 template, 4 rmdf definition, 5 the ten
definitions in riverworld, 6 the algorithm, 7 classification rules, 8 expected values,
9 survey results, 10 open questions.

---

## 1. The tag chain

```
sbsp Shaders[i] (36-byte entry, TagReference at +0)          sbsp-render-geometry.md 2.4
  or mode Shaders[i] (same 36-byte ShaderBlock)
    -> shader tag, class rmsh | rmtr | rmfl | rmw  | rmd  | rmhg | rmcs | rmct | ...  (section 2)
         +0x00  TagReference -> rmdf  render_method_definition   (section 4: categories, option names)
         +0x10  TagBlock of i16 option indices, one per rmdf category
         +0x28  TagBlock ShaderProperties (always 1 entry)
                  +0x00  TagReference -> rmt2 render_method_template  (section 3: usage and argument names)
                  +0x10  TagBlock ShaderMaps   (28-byte entries: bitmap TagReference ...)  one per rmt2 usage
                  +0x1C  TagBlock TilingData   (16-byte float4)                            one per rmt2 argument
                           -> bitm bitmap tags (decoded elsewhere: bitmaps.md)
```

Render-method classes seen in riverworld (`blam classes riverworld`; all inherit the abstract
class `rm  ` = `render_method`):

| class | name | riverworld count |
|---|---|---|
| `rmsh` | shader | 939 |
| `rmd ` | shader_decal | 22 |
| `rmhg` | shader_halogram | 15 |
| `rmtr` | shader_terrain | 6 |
| `rmfl` | shader_foliage | 5 |
| `rmw ` | shader_water | 1 |
| `rmcs` shader_custom, `rmct` shader_cortana, `rmlv` shader_light_volume, `rmb ` shader_beam, `rmc ` shader_contrail, `rmp ` shader_particle, `rmsk` shader_skin | | 0 here (campaign maps have them) |

Note the trailing space in `rmw ` and `rmd `; `blam tags riverworld --class "rmw "` needs it.
Related classes: `rmdf` render_method_definition (10 tags), `rmt2` render_method_template
(367), `rmop` render_method_option (157; referenced from rmdf options, holds parameter
defaults, not needed), `vtsh`/`pixl` (compiled vertex/pixel shaders referenced from rmt2),
`glvs`/`glps` (shared shaders referenced from rmdf).

---

## 2. The shader tag (render_method), U13 offsets

Source: `Halo3\ShaderTag.cs`. The same struct is read for every `rm**` class (Reclaimer's
`Halo3Common.GetMaterials` calls `ReadMetadata<ShaderTag>()` on whatever the material
references); the class only decides which rmdf it points at.

### 2.1 Main struct

| offset | size | type | name (Reclaimer) | notes |
|---|---|---|---|---|
| `0x00` (0) | 16 | TagReference | `RenderMethodDefinitionReference` | class `rmdf`. riverworld: `shaders\shader` for rmsh, `shaders\terrain` for rmtr, `shaders\foliage` for rmfl, `shaders\water` for rmw, `shaders\decal` for rmd, `shaders\halogram` for rmhg |
| `0x10` (16) | 12 | TagBlock | `ShaderOptions` | entries **2 bytes**: `i16 OptionIndex`. Count equals the rmdf's category count (988 of 988 tags in riverworld) |
| `0x1C` (28) | 12 | ? | (not read) | zero on every sample; probably an empty TagBlock |
| `0x28` (40) | 12 | TagBlock | `ShaderProperties` | entries **132 (0x84) bytes**, see 2.2. Count is **1** on all 988 render-method tags in riverworld; Reclaimer reads `[0]` only |
| `0x34` (52) | 4 | ? | (not read) | `0x00020000` on every sample (flags?) |
| `0x38` (56) | 4 | ? | (not read) | 0 on every sample |
| `0x3C` (60) | 4 | ? | (not read) | `-1` on every sample |

Layout in the file: child blocks precede the main struct (riverworld_rock: ShaderMaps at file
`0x8FD780C`, TilingData `0x8FD787C`, ShaderProperties `0x8FD798C`, main struct `0x8FD7A18`).

### 2.2 ShaderPropertiesBlock, 132 (0x84) bytes

`[FixedSize(132)]` in `ShaderTag.cs`.

| offset | size | type | name | notes |
|---|---|---|---|---|
| `0x00` (0) | 16 | TagReference | `TemplateReference` | class `rmt2` |
| `0x10` (16) | 12 | TagBlock | `ShaderMaps` | 28-byte entries, see 2.3. **Count == rmt2 Usages count** (0 mismatches in 988 tags). Entry `i` belongs to usage `i` |
| `0x1C` (28) | 12 | TagBlock | `TilingData` | 16-byte entries: `float4 {x, y, z, w}`. **Count == rmt2 Arguments count** (0 mismatches). Entry `k` is the value of argument `k` |
| `0x28` (40) | 16 | ? | (not read) | zero on all samples |
| `0x38` (56) | 12 | TagBlock? | (not read) | only non-empty on the water shader (count 16); zero otherwise |
| `0x44` (68) | 12 | TagBlock? | (not read) | water: count 4 |
| `0x50` (80) | 12 | TagBlock? | (not read) | water: count 24 |
| `0x5C` (92) | 12 | TagBlock? | (not read) | water: count 8. These four are probably the animated-parameter ("function") blocks: the water template has `time_warp` arguments |
| `0x68` (104) | 12 | ? | (not read) | glass_wall has `5` at `0x68`, tree_leafa has `2` at `0x6C`; others zero |
| `0x74` (116) | 16 | 8 x i16? | (not read) | rock `-1,0,-1,-1,2,-1,-1,-1`; ground `0,1,4,7,-1,10,-1,-1`; water all `-1`. Looks like a per-entry-point index table. Not needed |

### 2.3 ShaderMapBlock, 28 (0x1C) bytes on U13

`ShaderTag.cs`: `[FixedSize(24, MaxVersion = MccHalo3U4)]`,
**`[FixedSize(28, MinVersion = MccHalo3U4, MaxVersion = Halo3ODST)]`**,
`[FixedSize(24, MinVersion = Halo3ODST)]`. 22 is in [17, 23) so **28 bytes**. Verified: 4
entries x 28 = 112 = `0x8FD787C - 0x8FD780C`, i.e. the ShaderMaps block ends exactly where
TilingData begins on riverworld_rock.

| offset | size | type | name | notes |
|---|---|---|---|---|
| `0x00` (0) | 16 | TagReference | `BitmapReference` | class `bitm`. **Never null** in riverworld (4885 of 4885 maps). Templates fill unused slots with engine defaults: `shaders\default_bitmaps\bitmaps\default_detail`, `default_vector` (flat normal), `color_black`, `color_white`, `gray_50_percent` ... |
| `0x10` (16) | 2 | ? | (not read) | 0 on all samples |
| `0x12` (18) | 1 | ? | (not read) | `0x11` on every cube map (`environment_map`) and on water's `global_shape_texture` / `watercolor_texture`; 0 otherwise. Sampler/addressing flags? |
| `0x13` (19) | 1 | ? | (not read) | `0x04` on some maps (rock `base_map`, ground `detail_map_m_1/2`, `base_map_m_3`, glass `bump_map`); 0 otherwise. Sampler filter flags? |
| `0x14` (20) | 2 | ? | (not read) | 0 |
| **`0x16` (22)** | 1 | u8 | `TilingIndex` | `[Offset(21, Max=U4)]`, **`[Offset(22, Min=U4, Max=ODST)]`**, `[Offset(21, Min=ODST)]`. Index into TilingData of this map's xform; **255 = none** (use scale 1,1 offset 0,0). Verified equal to `Arguments.IndexOf(usage)` for every map in the file (0 mismatches), so either way of finding the xform works |
| `0x17` (23) | 1 | ? | | 0 |
| `0x18` (24) | 4 | ? | (not read) | 0 except water: `01 08`, `04 08`, `06 08` on the array/foam maps |

### 2.4 TilingData entry, 16 bytes

`BlockCollection<RealVector4>`: four f32 `x, y, z, w`. For a texture argument it is the UV
transform: **`uv' = uv * (x, y) + (z, w)`** (scale x, scale y, offset x, offset y; HaloExport
README "xform"; Reclaimer keeps only the scale as `Tiling`). For a non-texture argument it is
the parameter value: colours are RGBA (`albedo_color`, `env_tint_color`, `water_diffuse`),
scalars are replicated into all four lanes (`specular_coefficient (0.2, 0.2, 0.2, 0.2)`).

Offsets do occur: `panel_generic_dark base_map (4, 4, 0.1, 0)`,
`riverworld_cliff_distant base_map (0.3, 0.3, 0, 0.2)`,
`wall_lights_illum self_illum_detail_map (0.5, 0.5, 0, 1.0)`.

---

## 3. `rmt2` render_method_template, U13 offsets

Source: `Halo3\RenderMethodTemplateTag.cs` reads two blocks; the rest is what the bytes show
(flagged). The struct is 0x84 (132) bytes long as far as the file shows (the bytes after
`+0x84` belong to the following tag's data).

| offset | size | type | name | notes |
|---|---|---|---|---|
| `0x00` (0) | 16 | TagReference | (not read) | class `vtsh`, the compiled vertex shaders, same name as the template |
| `0x10` (16) | 16 | TagReference | (not read) | class `pixl`, the compiled pixel shaders |
| `0x20` (32) | 4 | u32 | (not read) | `0x000277FA` on the rmsh templates, `0x000227FA` terrain, `0x05FA` foliage, `0x8038` water: a bit set, probably the valid entry points |
| `0x24` (36) | 12 | TagBlock | (not read) | count 18 (shader, terrain), 11 (foliage), 16 (water); the shader templates share one block. Entry-point table |
| `0x30` (48) | 12 | TagBlock | (not read) | count 13 / 11 / 8 / 4 = the same count as the rmdf's `+0x1C` block (section 4) |
| `0x3C` (60) | 12 | TagBlock | (not read) | 4-byte entries, 42..182 of them: register routing (`{u16, u8, u8}`-looking). Not needed for textures |
| **`0x48` (72)** | 12 | TagBlock | **`Arguments`** | **4-byte StringId entries**: the float4 parameter names, in TilingData order. Texture usages appear here too (their xform slot) |
| `0x54` (84) | 12 | TagBlock | (not read) | empty on all samples |
| `0x60` (96) | 12 | TagBlock | (not read) | 4-byte StringIds: global option names, e.g. `no_dynamic_lights`, `order3_area_specular` |
| **`0x6C` (108)** | 12 | TagBlock | **`Usages`** | **4-byte StringId entries**: the sampler (texture) names, in ShaderMaps order |
| `0x78` (120) | 12 | TagBlock | (not read) | empty on all samples |

Template naming: the tag name is the option indices joined with `_`, so
`shaders\shader_templates\_0_2_0_1_2_0_0_0_0_0_0_0_0` is exactly riverworld_rock's
`ShaderOptions = [0,2,0,1,2,0,0,0,0,0,0,0,0]`; `terrain_templates\_0_0_0_0_0_2`,
`foliage_templates\_0_1_0`, `water_templates\_0_1_1_1_1_0_1_3` likewise (verified on all five
samples). Useful as a sanity check, not as a data source.

Usage names and their meaning (Reclaimer `Gen3Constants.cs` `ShaderParameters.UsageLookup`,
plus names seen in riverworld templates):

| usage StringId | Reclaimer TextureUsage | meaning |
|---|---|---|
| `base_map` | Diffuse | **the colour texture** |
| `detail_map`, `detail_map2`, `detail_map_overlay` | DiffuseDetail | detail multiplied over the base (tiles much tighter) |
| `blend_map` | BlendMap | terrain layer weights, RGBA (7.5) |
| `base_map_m_N`, `detail_map_m_N`, `bump_map_m_N`, `detail_bump_m_N` (N = 0..3) | Diffuse / DiffuseDetail / Normal / NormalDetail with BlendChannel `1 << N` | terrain layer N |
| `bump_map` | Normal | normal map |
| `bump_detail_map` | NormalDetail | |
| `change_color_map` | ColorChange | team/armour colour mask |
| `self_illum_map` (`self_illum_detail_map`) | SelfIllumination | emissive |
| `specular_map`, `specular_mask_texture` | Specular / Other | |
| `alpha_test_map` | Transparency | alpha-test source (foliage template) |
| `alpha_mask_map` | Diffuse | |
| `environment_map` | ReflectionCube | cube map |
| `foam_texture` (`foam_texture_detail`) | Diffuse (water) | foam overlay |
| `watercolor_texture`, `global_shape_texture`, `wave_displacement_array`, `wave_slope_array` | Other | water colour, shape mask, wave arrays |

The usage regex Reclaimer applies to every name: `^(\w+?)(?:_m_(\d))?$`, i.e. strip a trailing
`_m_<digit>`; the digit is the terrain layer (BlendChannel = `1 << digit`:
`Red = 1, Green = 2, Blue = 4, Alpha = 8`, `Reclaimer.Core\Geometry\Material.cs`).

Argument names that are colours/tints (Reclaimer `TintLookup`): `albedo_color` (albedo tint),
`self_illum_color`, `specular_tint`. Terrain uses `global_albedo_tint`; water uses
`water_diffuse` (RGBA, the water colour when `watercolor = pure`) and `env_tint_color` appears
on glass.

---

## 4. `rmdf` render_method_definition, U13 offsets

Source: `Halo3\RenderMethodDefinitionTag.cs` (Categories only); other fields from the bytes.

| offset | size | type | name | notes |
|---|---|---|---|---|
| `0x00` (0) | 16 | TagReference | (not read) | class `rmop`: the global options tag (`shaders\shader_options\global_shader_options` ...) |
| **`0x10` (16)** | 12 | TagBlock | **`Categories`** | **24-byte entries**, see 4.1. Order = the order of the shader's `ShaderOptions` i16s |
| `0x1C` (28) | 12 | TagBlock | (not read) | count 13 (shader), 11 (terrain), 8 (foliage), 4 (water), 1 (decal, particle ...); entries start `{i32 index, TagBlock}`; entry points? |
| `0x28` (40) | 12 | TagBlock | (not read) | count 4 (shader), 3, 6, 2, 1; vertex types? |
| `0x34` (52) | 16 | TagReference | (not read) | class `glps`, e.g. `shaders\shader_shared_pixel_shaders` |
| `0x44` (68) | 16 | TagReference | (not read) | class `glvs`, e.g. `shaders\shader_shared_vertex_shaders` |

### 4.1 ShaderOptionCategoryBlock, 24 (0x18) bytes

| offset | size | type | name | notes |
|---|---|---|---|---|
| `0x00` (0) | 4 | StringId | `Name` | `albedo`, `bump_mapping`, `alpha_test`, `blend_mode` ... |
| `0x04` (4) | 12 | TagBlock | `Options` | **28-byte entries**, see 4.2 |
| `0x10` (16) | 4 | i32 | (not read) | 0 (1701 on `misc_attr_animation`) |
| `0x14` (20) | 4 | StringId | (not read) | the HLSL function/define the category selects: `calc_albedo_ps`, `calc_alpha_test_ps`, `blend_type`, `material_type`, `envmap_type` ... (`<none>` on decal/particle/water) |

### 4.2 ShaderOptionBlock, 28 (0x1C) bytes

| offset | size | type | name | notes |
|---|---|---|---|---|
| `0x00` (0) | 4 | StringId | `Name` | `default`, `detail_blend`, `none`, `simple`, `opaque`, `additive` ... |
| `0x04` (4) | 16 | TagReference | (not read) | class `rmop`, the option's parameter defaults (`shaders\shader_options\albedo_default`) |
| `0x14` (20) | 4 | i32 | (not read) | 0 |
| `0x18` (24) | 4 | StringId | (not read) | the HLSL function name: `calc_albedo_default_ps`, `calc_albedo_detail_blend_ps` ... |

---

## 5. The ten definitions in riverworld (categories and option names)

From `probe_shaders.py --rmdf`. The option index stored in the shader is the position in the
list (0-based). Names Reclaimer's `Gen3Constants.cs` relies on are in bold.

### `shaders\shader` (rmsh), 13 categories

| # | category | options (index 0, 1, 2 ...) |
|---|---|---|
| 0 | **albedo** | default, detail_blend, **constant_color**, two_change_color, four_change_color, three_detail_blend, two_detail_overlay, two_detail, color_mask, two_detail_black_point, two_change_color_anim_overlay, chameleon, two_change_color_chameleon, chameleon_masked, color_mask_hard_light, two_change_color_tex_overlay, chameleon_albedo_masked, custom_cube, two_color, scrolling_cube_mask, scrolling_cube, scrolling_texture_uv, texture_from_misc |
| 1 | bump_mapping | off, standard, detail, detail_masked, detail_plus_detail_masked, detail_unorm |
| 2 | **alpha_test** | **none**, simple |
| 3 | **specular_mask** | no_specular_mask, **specular_mask_from_diffuse**, specular_mask_from_texture, specular_mask_from_color_texture |
| 4 | material_model | diffuse_only, cook_torrance, two_lobe_phong, foliage, none, glass, organism, single_lobe_phong, car_paint, cook_torrance_custom_cube, cook_torrance_pbr_maps, cook_torrance_two_color_spec_tint, two_lobe_phong_tint_map, cook_torrance_scrolling_cube_mask, cook_torrance_rim_fresnel, cook_torrance_scrolling_cube, cook_torrance_from_albedo |
| 5 | environment_mapping | none, per_pixel, dynamic, from_flat_texture, custom_map, from_flat_exture_as_cubemap |
| 6 | self_illumination | off, simple, 3_channel_self_illum, plasma, from_diffuse, illum_detail, meter, self_illum_times_diffuse, simple_with_alpha_mask, simple_four_change_color, illum_detail_world_space_four_cc, illum_change_color |
| 7 | **blend_mode** | **opaque, additive, multiply, alpha_blend, double_multiply, pre_multiplied_alpha** |
| 8 | parallax | off, simple, interpolated, simple_detail |
| 9 | misc | first_person_never, first_person_sometimes, first_person_always, first_person_never_w/rotating_bitmaps |
| 10 | distortion | off, on |
| 11 | soft_fade | off, on |
| 12 | misc_attr_animation | off, scrolling_cube, scrolling_projected |

(The MCC rmdf has more options than the Xbox 360 one: the `*_pbr`, `chameleon`, `scrolling_*`
and `tint_map` entries are 343i additions. Index positions of the original options are
unchanged.)

### `shaders\terrain` (rmtr), 6 categories

| # | category | options |
|---|---|---|
| 0 | blending | morph, dynamic_morph |
| 1 | environment_map | none, per_pixel, dynamic |
| 2 | **material_0** | diffuse_only, **diffuse_plus_specular**, **off**, diffuse_only_plus_self_illum, diffuse_plus_specular_plus_self_illum, diffuse_plus_specular_plus_heightmap, diffuse_plus_two_detail, diffuse_plus_specular_plus_up_vector_plus_heightmap |
| 3 | material_1 | diffuse_only, diffuse_plus_specular, off, diffuse_only_plus_self_illum, diffuse_plus_specular_plus_self_illum, diffuse_plus_specular_plus_heightmap, diffuse_plus_specular_plus_up_vector_plus_heightmap |
| 4 | material_2 | diffuse_only, diffuse_plus_specular, off, diffuse_only_plus_self_illum, diffuse_plus_specular_plus_self_illum |
| 5 | material_3 | off, diffuse_only_(four_material_shaders_disable_detail_bump), diffuse_plus_specular_(four_material_shaders_disable_detail_bump) |

Note `off` is index 2 for materials 0..2 but index **0** for material_3. When a layer is `off`
its `*_m_N` usages are absent from the template (riverworld_ground_snow: `material_0 = off`,
usages start at `base_map_m_1`), so the layer number must be taken from the usage suffix,
never from the position.

### `shaders\foliage` (rmfl), 3 categories

| # | category | options |
|---|---|---|
| 0 | albedo | default |
| 1 | **alpha_test** | none, simple |
| 2 | material_model | default |

### `shaders\water` (rmw), 8 categories

| # | category | options |
|---|---|---|
| 0 | waveshape | default, none, bump |
| 1 | watercolor | pure, texture |
| 2 | reflection | none, static, dynamic, static_ssr |
| 3 | refraction | none, dynamic |
| 4 | bankalpha | none, depth, paint |
| 5 | appearance | default |
| 6 | global_shape | none, paint, depth |
| 7 | foam | none, auto, paint, both |

No `blend_mode` or `alpha_test` category: water is its own pipeline.

### `shaders\decal` (rmd), 6 categories

| # | category | options |
|---|---|---|
| 0 | albedo | diffuse_only, palettized, palettized_plus_alpha, diffuse_plus_alpha, emblem_change_color, change_color, diffuse_plus_alpha_mask, palettized_plus_alpha_mask, vector_alpha, vector_alpha_drop_shadow |
| 1 | **blend_mode** | opaque, additive, multiply, alpha_blend, double_multiply, maximum, multiply_add, add_src_times_dstalpha, add_src_times_srcalpha, inv_alpha_blend, pre_multiplied_alpha |
| 2 | render_pass | pre_lighting, post_lighting, transparent |
| 3 | specular | leave, modulate |
| 4 | bump_mapping | leave, standard, standard_mask |
| 5 | tinting | none, unmodulated, partially_modulated, fully_modulated |

### `shaders\halogram` (rmhg), 9 categories

albedo (default, detail_blend, constant_color, two_change_color, four_change_color,
three_detail_blend, two_detail_overlay, two_detail, color_mask, two_detail_black_point);
self_illumination (off, simple, 3_channel_self_illum, plasma, from_diffuse, illum_detail, meter,
self_illum_times_diffuse, multilayer_additive, scope_blur, ml_add_four_change_color,
ml_add_five_change_color, plasma_wide_and_sharp_five_change_color, self_illum_holograms);
**blend_mode** (opaque, additive, multiply, alpha_blend, double_multiply); misc; warp; overlay;
edge_fade; distortion; soft_fade.

### `shaders\particle` (rmp), `shaders\light_volume` (rmlv), `shaders\contrail` (rmc), `shaders\beam` (rmb)

Effects, not level geometry. Each has `albedo` then the 11-entry `blend_mode` list of the decal
definition, then a few switches (`fog`, `black_point`, `depth_fade`, `frame_blend`,
`specialized_rendering`, `lighting`, `render_targets`, `self_illumination`).

---

## 6. The algorithm: shader tag -> textures, xforms, options

This is Reclaimer's `Halo3Common.GetMaterials` + `Gen3MaterialHelper.PopulateTextureMappings`
in our own words, with the extra pieces HaloExport added (offsets, raw usage names).

```
resolve_shader(shader_tag):
    S     = shader_tag.meta
    rmdf  = tag(TagReference at S+0x00)                 # must be class rmdf
    opts  = block(S+0x10)                               # i16 per category
    props = block(S+0x28)[0]                            # 132-byte entry; count is always 1

    # 1. options: (category name, option name) pairs
    cats = block(rmdf+0x10)                             # 24-byte entries
    options = {}
    for i, optIndex in enumerate(opts):                 # same order as cats
        if i < len(cats) and 0 <= optIndex < count(cats[i].Options):
            options[string(cats[i].Name)] = string(cats[i].Options[optIndex].Name)   # 28-byte entries
        # out-of-range or negative index: skip (Reclaimer does the same)

    # 2. names from the template
    rmt2      = tag(TagReference at props+0x00)
    arguments = [string(sid) for sid in block(rmt2+0x48)]   # 4-byte entries
    usages    = [string(sid) for sid in block(rmt2+0x6C)]

    # 3. textures
    maps   = block(props+0x10)                          # 28-byte entries, count == len(usages)
    tiling = block(props+0x1C)                          # 16-byte float4,  count == len(arguments)
    textures = []
    for i, usage in enumerate(usages):
        bitmap = tag(TagReference at maps[i]+0x00)      # bitm; never null in practice
        k      = arguments.index(usage)                 # == byte maps[i]+0x16, 255 when absent
        xform  = tiling[k] if k >= 0 else (1, 1, 0, 0)  # uv' = uv * xform.xy + xform.zw
        base, layer = split "_m_<digit>" suffix off usage   # layer None when no suffix
        textures.append((usage, base, layer, bitmap, xform))

    # 4. float / colour parameters
    params = { arguments[k]: tiling[k] for k in range(len(arguments)) if arguments[k] not in usages }

    return options, textures, params
```

Notes:

- Reclaimer uses `arguments.IndexOf(usage)`; the byte at `+0x16` holds the same number (checked
  on all 4885 maps in riverworld). Prefer the byte, fall back to the search, warn if they differ.
- One shader is shared by many materials and models; cache by tag index. Reclaimer caches the
  rmdf per tag id and the bitmaps per tag id.
- Reclaimer derives extra mappings we may want too:
  - `specular_mask = specular_mask_from_diffuse` (rmsh) or `material_N = diffuse_plus_specular`
    (rmtr): the **alpha channel of the base map is the specular mask**, not transparency. That
    is why HaloExport writes opaque diffuse PNGs without alpha.
  - when the material is not opaque (6.1 below), the **alpha of the albedo texture** is the
    transparency (`change_color_map` instead of `base_map` when `albedo` is `change_color` or
    `emblem_change_color`).

---

## 7. Classification rules

### 7.1 Alpha test and blend mode (Reclaimer `Gen3MaterialHelper`, HaloExport `Analyse`)

Read `blend_mode` and `alpha_test` from the options dictionary (missing = `opaque` / `none`):

| condition | Reclaimer AlphaMode | meaning (standard D3D blend states) |
|---|---|---|
| `alpha_test != none` (currently only `simple`) | Clip (wins over blend_mode) | discard pixels whose alpha is below the threshold. Source: `alpha_test_map` when the template has it (foliage, `z_invis_lightquad`), else the base map alpha. Threshold not in the tags; 0.5 is the usual engine value (unverified) |
| `blend_mode = opaque` or missing | Opaque | |
| `additive` (and the decal/particle extras `add_src_times_dstalpha`, `add_src_times_srcalpha`, `multiply_add`, `maximum`) | Add | `dst + src` |
| `multiply` | Multiply | `dst * src` |
| `double_multiply` | Opaque in Reclaimer's switch (falls through); HaloExport treats it as multiply | `2 * dst * src` |
| `alpha_blend` | Blend | `src * a + dst * (1 - a)` |
| `pre_multiplied_alpha` | PreMultiplied | `src + dst * (1 - a)` |
| `inv_alpha_blend` (decals only) | (not handled) | `src * (1 - a) + dst * a` |

### 7.2 HaloExport's category (shader class + options), with riverworld results

| class | rule | category |
|---|---|---|
| `rmw ` | always | **water** |
| `rmtr` | always | **terrain** |
| `rmfl` | always | **foliage** (alpha-tested, two-sided) |
| `rmd ` | blend `multiply`/`double_multiply` -> decal_multiply; additive family -> decal_additive; `opaque` -> decal_opaque; anything else (`alpha_blend`, `pre_multiplied_alpha`, `inv_alpha_blend` ...) -> **decal_blend** | |
| `rmsh`, `rmcs`, `rmhg`, `rmtd` | additive family -> **additive**; `multiply`/`double_multiply` -> **multiply**; `alpha_blend`/`pre_multiplied_alpha` -> **alpha_blend**; else `alpha_test != none` -> **alpha_test**, else **opaque** | |
| any other class (`rmct`, `rmzo`, `rmbk`, `rmlv`, beams, contrails, particles) | | **special** (not drawn as geometry) |
| any class, tag path contains a segment `invisible`, `sky_portal`/`skyportal`, `portal`, `z_only`/`zonly`, `black` (regex on the lower-case path: `(^|\\)(invisible|sky_?portal|portal|z_only|zonly|black)(\\|_|$)`) | overrides | **special** |

HaloExport's pipeline treatment: opaque, terrain, decal_opaque drawn opaque; alpha_test and
foliage drawn with alpha mask; alpha_blend and decal_blend blended; multiply, additive, water,
special skipped (README material table). For our viewer: draw water too (flat colour or the
watercolor texture), skip only `special`.

Applying these rules to riverworld's 64 sbsp materials (`--survey`) gives opaque 38, terrain 9,
alpha_blend 5 (glass_wall x2, glass_wall_waterfallbase, glass_wall_lakebase, vahalla_waterfall),
alpha_test 4 (`z_invis_lightquad` x4: albedo constant_color + alpha_test simple against
`color_black`, i.e. an invisible light quad), additive 2 (wall_lights_illum x2), decal_blend 2
(human_military_decals x2), foliage 3, water 1: identical to HaloExport's per-material
categories in `halo_export.json`.

### 7.3 Base colour per class (our rule, built on Reclaimer's UsageLookup and HaloExport's `MaterialIndex`)

| class | colour source | fallback |
|---|---|---|
| `rmsh`, `rmcs`, `rmhg`, `rmct` | `base_map` (Diffuse). Multiply by `albedo_color` RGB when present (1,1,1 on all samples) | no `base_map` (albedo = `constant_color`: `z_null_light`, `ground_lake_deep`): flat `albedo_color`; additive glow shaders without a base map (`wall_lights_illum`): `self_illum_map` (x `self_illum_color`); albedo `change_color`/`emblem_change_color`: `change_color_map` is a mask, use a team colour |
| `rmtr` | `blend_map` + `base_map_m_0..3` composited per 7.5 (HaloExport picks the layer with the largest mean blend weight when it needs a single texture) | a layer whose `material_N = off` has no usages |
| `rmfl` | `base_map`; alpha test from `alpha_test_map` (same bitmap as the base map on all three tree shaders) | |
| `rmd ` | `base_map` (decal sheets, e.g. `decals_weathered`), blended with 7.1 | |
| `rmw ` | `watercolor_texture` when `watercolor = texture`, else the `water_diffuse` RGBA argument (riverworld: (0.067, 0.114, 0.137, 1.0) = dark blue-green). `foam_texture` is an overlay (Reclaimer calls it Diffuse because nothing else maps; HaloExport consequently exported foam as the "base", which is wrong for a colour). `environment_map` is a cube; `wave_*_array` are animated normal/displacement arrays; `global_shape_texture` masks the waves | |
| `rmhg` | as rmsh; usually additive self-illum (spawn_point_plasma2) | |

Detail maps (`detail_map`, `detail_map_m_N`) are multiplied over the base in the real shader
(roughly `base * detail * 2`); they tile 3-10x tighter (rock: base 22, detail 122). Optional for
a first pass; HaloExport bakes terrain detail maps in.

### 7.4 Two-sidedness

There is **no two-sided flag in the shader tag, the rmdf or the rmt2**, and Reclaimer has no
notion of it. HaloExport decides by category: `cull = None` (double sided) for foliage and all
decal categories, back-face culling for everything else. Suggested rule for us: two-sided for
`rmfl`, `rmd `, and `alpha_test` materials (leaf cards, netting, grates are single quads);
back-face culled otherwise; glass (`pre_multiplied_alpha`) is a judgement call. (Collision
surfaces have their own two-sided flag, see `collision-bsp-research.md`; it says nothing about
rendering.)

### 7.5 Terrain: how the blend map weights the four layers

- The layer number N (0..3) comes from the usage suffix `_m_N` (Reclaimer regex
  `^(\w+?)(?:_m_(\d))?$`). Reclaimer records `BlendChannel = 1 << N`:
  **layer 0 = red, 1 = green, 2 = blue, 3 = alpha** of `blend_map`.
- `blend_map` is sampled with the raw UV (its xform is (1, 1, 0, 0) on every terrain shader
  here; HaloExport exports it as TEXCOORD_1 = untransformed UV). Each layer's base/detail/bump
  is sampled with its own xform (ground: layer 0 base (40, 80), layer 1 (6, 12), layer 2 (1, 1),
  layer 3 (15, 30); detail maps (70, 140), layer 3 detail (150, 300)).
- Composite, as an approximation of the `morph` blending:
  `colour = sum_N w_N * base_N(uv * s_N + o_N) [* detail_N(...) * 2]`, with `w = blend_map(uv).rgba`
  and absent layers (`material_N = off`) contributing nothing. HaloExport's mean weights for
  `riverworld_ground`'s blend map `cliff_snow_distant_blend` are R 0.358, G 0.140, B 0.190,
  A 0.349 (sum 1.04), so the channels are already close to normalised; normalise anyway
  (`w /= max(sum w, eps)`) to be safe. What `dynamic_morph` adds is not known.
- `material_N = diffuse_plus_specular`: that layer's base-map alpha is a specular mask, not a
  weight.
- The blend-map bitmap format (which channels exist) is a bitmaps.md matter; HaloExport read it
  as RGBA.

**What the viewer does (2026-10-28, `docs/viewer.md` "Terrain").** It uses the composite
above, with the weights normalised. The blend map is read linear, both colour maps sRGB, and
the detail factor is **2^2.2 = 4.595** instead of 2, because the product is formed in linear
light: the engine default `shaders\default_bitmaps\bitmaps\default_detail` is one texel of
0x80 grey (xRGB curve), and 0.502^2.2 * 4.595 = 1.00, so the default detail leaves the base
unchanged. Measured on riverworld:

| bitmap | curve | mean RGB (bytes / 255) |
|---|---|---|
| `cliff_snow_distant_blend` (blend, ground and snow) | linear | R 0.358 G 0.140 B 0.190 A 0.348 |
| `riverworld_cliff_blend` (layers 0..2; layer 3 off) | linear | 0.571 0.420 0.009 |
| `cliff_snow_distant_cliff_blend` (layer 1 off) | linear | 0.361 (G 0.008) 0.140 A 0.504 |
| `riverworld_ground_grass_overlay` (ground base 0 and 1) | linear | 0.25 grey |
| `riverworld_ground_grass` (detail 1) | xRGB | 0.400 0.453 0.159 |
| `riverworld_ground_dirt` (detail 0) | linear | 0.366 0.300 0.227 |
| `default_detail` | xRGB | 0.502 grey (one texel 0x80) |
| `gray_50_percent` | **linear** | **0.251** grey (one texel 0x40) |

Two things follow. On the main ground, the colour comes from the detail maps (dirt, grass)
and the bases are grey overlays. And the blend maps on Valhalla and The Pit already sum to
about 1 over the layers that are on, so normalising changes very little.

The `gray_50_percent` texel is a clue about the bitmap curve. If "50 percent" is meant at
face value, the engine reads linear-curve bitmaps raw (0x40 = 0.25, the same as an xRGB 0x80
read with gamma 2), not as sRGB. Then `riverworld_distant`'s layers 1 and 2 (base
`gray_50_percent`) would be exactly their detail map. A trial reading linear-curve colour maps
raw with exposure 1 washed the whole of Valhalla out and lost the reference screenshots'
contrast, so the viewer still reads every colour map as sRGB. Under that reading, layers with
a `gray_50_percent` base come out darker than intended. This is still open.

---

## 8. Expected values (riverworld.map), for Rust tests

Tag indices are the low 16 bits of the tag id; "meta" is the file offset of the main struct.

### 8.1 `rmsh levels\multi\riverworld\shaders\riverworld_rock` (tag 11454 = 0x2CBE, meta 0x8FD7A18)

- rmdf `shaders\shader` (tag 15); 13 options `[0, 2, 0, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0]` =
  albedo default, bump_mapping detail, alpha_test none, specular_mask
  specular_mask_from_diffuse, material_model two_lobe_phong, environment_mapping none,
  self_illumination off, blend_mode opaque, parallax off, misc first_person_never,
  distortion off, soft_fade off, misc_attr_animation off. Category **opaque**.
- template `shaders\shader_templates\_0_2_0_1_2_0_0_0_0_0_0_0_0` (tag 6209 = 0x1841);
  ShaderMaps 4 (file 0x8FD780C), TilingData 17 (file 0x8FD787C); arguments 17, usages 4.

| usage | bitmap | tiling idx | xform |
|---|---|---|---|
| base_map | `levels\multi\riverworld\bitmaps\cliff_rock_base_brown` | 0 | (22, 22, 0, 0) |
| detail_map | `levels\multi\riverworld\bitmaps\cliff_rock_detail` | 1 | (122, 122, 0, 0) |
| bump_map | `levels\multi\riverworld\bitmaps\cliff_rock_bump` | 3 | (2, 2, 0, 0) |
| bump_detail_map | `levels\multi\riverworld\bitmaps\cliff_rock_bump` | 4 | (22, 22, 0, 0) |

Arguments in order: base_map, detail_map, albedo_color (1,1,1,1), bump_map, bump_detail_map,
bump_detail_coefficient (1), diffuse_coefficient (1), specular_coefficient (0.2),
normal_specular_power (22), normal_specular_tint (1,1,1,1), glancing_specular_power (10),
glancing_specular_tint (1,1,1,1), fresnel_curve_steepness (5), area_specular_contribution (0.2),
analytical_specular_contribution (0.5), albedo_specular_tint_blend (0),
analytical_anti_shadow_control (0).

### 8.2 `rmtr levels\multi\riverworld\shaders\riverworld_ground` (tag 11594 = 0x2D4A, meta 0x928B9F4)

- rmdf `shaders\terrain` (tag 11524); 6 options `[0, 0, 0, 0, 0, 2]` = blending morph,
  environment_map none, material_0 diffuse_only, material_1 diffuse_only, material_2
  diffuse_only, material_3 diffuse_plus_specular_(four_material_shaders_disable_detail_bump).
  Category **terrain**.
- template `shaders\terrain_templates\_0_0_0_0_0_2` (tag 11595); ShaderMaps 13 (file
  0x928B68C), TilingData 23 (file 0x928B7F8); arguments 23, usages 13.

| usage | layer / channel | bitmap | tiling idx | xform |
|---|---|---|---|---|
| blend_map | | `levels\multi\riverworld\bitmaps\cliff_snow_distant_blend` | 0 | (1, 1, 0, 0) |
| base_map_m_0 | 0 red | `levels\multi\riverworld\bitmaps\riverworld_ground_grass_overlay` | 2 | (40, 80, 0, 0) |
| detail_map_m_0 | 0 | `levels\multi\riverworld\bitmaps\riverworld_ground_dirt` | 3 | (70, 140, 0, 0) |
| bump_map_m_0 | 0 | `levels\multi\riverworld\bitmaps\riverworld_ground_dirt_bump` | 4 | (70, 140, 0, 0) |
| base_map_m_1 | 1 green | `levels\multi\riverworld\bitmaps\riverworld_ground_grass_overlay` | 5 | (6, 12, 0, 0) |
| detail_map_m_1 | 1 | `levels\multi\riverworld\bitmaps\riverworld_ground_grass` | 6 | (70, 140, 0, 0) |
| bump_map_m_1 | 1 | `shaders\default_bitmaps\bitmaps\default_vector` | 7 | (70, 140, 0, 0) |
| base_map_m_2 | 2 blue | `levels\multi\riverworld\bitmaps\cliff_snow_distant_dif` | 8 | (1, 1, 0, 0) |
| detail_map_m_2 | 2 | `levels\multi\riverworld\bitmaps\riverworld_ground_grassrock` | 9 | (70, 140, 0, 0) |
| bump_map_m_2 | 2 | `levels\multi\riverworld\bitmaps\riverworld_ground_grassrock_bump` | 10 | (70, 140, 0, 0) |
| base_map_m_3 | 3 alpha | `levels\multi\riverworld\bitmaps\cliff_rock_base` | 11 | (15, 30, 0, 0) |
| detail_map_m_3 | 3 | `levels\multi\riverworld\bitmaps\cliff_rock_detail` | 12 | (150, 300, 0, 0) |
| bump_map_m_3 | 3 | `levels\multi\riverworld\bitmaps\riverworld_rock_granite_bump` | 13 | (60, 120, 0, 0) |

Non-texture arguments: global_albedo_tint (1,1,1,1) at index 1; diffuse_coefficient_m_3 (1),
specular_coefficient_m_3 (0.2), specular_power_m_3 (22), specular_tint_m_3 (1,1,1,1),
fresnel_curve_steepness_m_3 (5), area_specular_contribution_m_3 (0.8),
analytical_specular_contribution_m_3 (0.5), environment_specular_contribution_m_3 (0),
albedo_specular_tint_blend_m_3 (0) at indices 14..22. HaloExport's mean blend weights:
(0.3583, 0.1398, 0.1904, 0.3486), so layer 0 (grass overlay at 40x80) is the dominant layer.

### 8.3 `rmfl levels\multi\riverworld\shaders\riverworld_tree_leafa` (tag 11652 = 0x2D84, meta 0x935B814)

- rmdf `shaders\foliage` (tag 6698); 3 options `[0, 1, 0]` = albedo default, **alpha_test
  simple**, material_model default. Category **foliage** (alpha-tested, two-sided).
- template `shaders\foliage_templates\_0_1_0` (tag 6702); ShaderMaps 3 (file 0x81B26B0,
  shared with the citadel tree shader), TilingData 5 (file 0x935B738).

| usage | bitmap | tiling idx | xform |
|---|---|---|---|
| base_map | `levels\solo\100_citadel\bitmaps\cit_tree_leafa` | 0 | (1, 1, 0, 0) |
| detail_map | `shaders\default_bitmaps\bitmaps\default_detail` | 1 | (1, 1, 0, 0) |
| alpha_test_map | `levels\solo\100_citadel\bitmaps\cit_tree_leafa` | 3 | (1, 1, 0, 0) |

Arguments: base_map, detail_map, albedo_color (1,1,1,1), alpha_test_map,
animation_amplitude_horizontal (0.02).

### 8.4 `rmw  levels\multi\riverworld\shaders\riverworld_water_rough` (tag 11609 = 0x2D59, meta 0x92D2E00)

- rmdf `shaders\water` (tag 11610); 8 options `[0, 1, 1, 1, 1, 0, 1, 3]` = waveshape default,
  watercolor **texture**, reflection static, refraction dynamic, bankalpha depth, appearance
  default, global_shape paint, foam both. Category **water**.
- template `shaders\water_templates\_0_1_1_1_1_0_1_3` (tag 11632); ShaderMaps 7 (file
  0x92D28A8), TilingData 37 (file 0x92D296C).

| usage | bitmap | tiling idx | xform |
|---|---|---|---|
| wave_displacement_array | `rasterizer\water\wave_test1\wave_test1_displ_water` | 2 | (1, 3, 0, 0) |
| global_shape_texture | `levels\multi\riverworld\bitmaps\riverworld_water_multipurpose` | 255 (none) | (1, 1, 0, 0) |
| wave_slope_array | `rasterizer\water\wave_test1\wave_test1_slope_water` | 4 | (2, 2, 0, 0) |
| watercolor_texture | `levels\multi\riverworld\bitmaps\riverworld_water_diffuse` | 255 | (1, 1, 0, 0) |
| environment_map | `levels\multi\riverworld\bitmaps\z_cubemap_temp` | 255 | (1, 1, 0, 0) |
| foam_texture | `levels\multi\riverworld\bitmaps\riverworld_water_foam_dif` | 22 | (2, 1, 0, 0) |
| foam_texture_detail | `levels\multi\riverworld\bitmaps\riverworld_water_foam_dif` | 23 | (3, 1.5, 0, 0) |

Selected arguments (37): water_diffuse (0.06667, 0.11373, 0.13725, 1.0), water_murkiness (3),
reflection_coefficient (600), fresnel_coefficient (0.1), wave_height (0.5), foam_height (0.5),
foam_pow (2), refraction_texcoord_shift (0.12), time_warp (0), waveshape (0), global_shape (1).

### 8.5 `rmsh levels\multi\riverworld\shaders\forerunner\glass_wall` (tag 11504 = 0x2CF0, meta 0x90624EC)

- rmdf `shaders\shader`; 13 options `[0, 1, 0, 2, 5, 1, 0, 5, 0, 0, 0, 0, 0]` = albedo default,
  bump_mapping standard, alpha_test none, specular_mask specular_mask_from_texture,
  material_model glass, environment_mapping per_pixel, self_illumination off, **blend_mode
  pre_multiplied_alpha**, parallax off, misc first_person_never, distortion off, soft_fade off,
  misc_attr_animation off. Category **alpha_blend** (Reclaimer AlphaMode PreMultiplied;
  transparency = alpha of `waste_glass_dif`).
- template `shaders\shader_templates\_0_1_0_2_5_1_0_5_0_0_0_0_0` (tag 11505); ShaderMaps 5
  (file 0x90622F4), TilingData 14 (file 0x9062380).

| usage | bitmap | tiling idx | xform |
|---|---|---|---|
| base_map | `levels\multi\riverworld\bitmaps\forerunner\waste_glass_dif` | 0 | (1, 1, 0, 0) |
| detail_map | `shaders\default_bitmaps\bitmaps\default_detail` | 1 | (1, 1, 0, 0) |
| bump_map | `levels\multi\riverworld\bitmaps\forerunner\waste_glass_bump` | 3 | (1, 1, 0, 0) |
| specular_mask_texture | `levels\multi\riverworld\bitmaps\forerunner\waste_glass_specmask` | 4 | (2.5, 2.5, 0, 0) |
| environment_map | `levels\multi\riverworld\bitmaps\forerunner\cube_base_blue_int` | 255 | (1, 1, 0, 0) |

Arguments: base_map, detail_map, albedo_color (1,1,1,1), bump_map, specular_mask_texture,
diffuse_coefficient (1), specular_coefficient (1), fresnel_coefficient (0.3),
fresnel_curve_steepness (5), fresnel_curve_bias (0.2), roughness (0.15),
analytical_specular_contribution (10), area_specular_contribution (0), env_tint_color
(0.72157, 0.76863, 1.0, 1.0).

### 8.6 Edge cases worth a test

| shader | options of note | usages |
|---|---|---|
| `levels\multi\snowbound\shaders\z_invis_lightquad` (rmsh, tag 11605) | albedo constant_color, alpha_test simple | only `alpha_test_map` -> `shaders\default_bitmaps\bitmaps\color_black`: everything is cut out (an invisible light-emitting quad). Category alpha_test, effectively invisible |
| `levels\multi\riverworld\shaders\z_null_light` (rmsh, tag 11556) | albedo constant_color, no textures at all | template `_2_0_0_0_0_0_0_0_0_0_0_0_0`; colour = `albedo_color` |
| `levels\multi\riverworld\shaders\ground_lake_deep` (rmsh, tag 11512) | albedo constant_color, bump standard | only `bump_map`; colour = `albedo_color` |
| `levels\multi\riverworld\shaders\forerunner\wall_lights_illum` (rmsh, tag 11641) | constant_color, self_illumination illum_detail, blend_mode additive | `self_illum_map` -> `waste_panel_trim_illum` (1,1,0,0), `self_illum_detail_map` -> `clouds_illum_detail` (0.5, 0.5, 0, 1.0) |
| `levels\multi\riverworld\shaders\vahalla_waterfall` (rmsh, tag 11645) | albedo two_detail, blend_mode alpha_blend | base_map `vahalla_waterfall_mask` (1,1), detail_map `vahalla_waterfall_color` (1,3), detail_map2 same (1,2), bump_map (1,3.5) |
| `levels\multi\riverworld\shaders\riverworld_ground_snow` (rmtr, tag 11583) | material_0 **off** | usages start at `base_map_m_1`; layers 1..3 have `detail_bump_m_N` too; blend map red channel is unused |

---

## 9. Survey results (riverworld, all 988 render-method tags)

From `probe_shaders.py --survey`:

- `ShaderProperties` count is 1 on all 988 tags.
- `ShaderOptions` count equals the rmdf category count on all 988.
- `ShaderMaps` count equals the template's `Usages` count and `TilingData` count equals its
  `Arguments` count on all 988 (0 mismatches).
- The tiling-index byte (`+0x16`) equals `Arguments.IndexOf(usage)` (255 when absent) on all
  4885 shader maps; no shader map has a null bitmap reference.
- `blend_mode` by class: rmsh opaque 809, additive 83, alpha_blend 43, pre_multiplied_alpha 3,
  multiply 1; rmd alpha_blend 20, additive 1, multiply 1; rmhg additive 13, opaque 2;
  rmtr / rmfl / rmw have no blend_mode category.
- `alpha_test` by class: rmsh none 889, simple 50; rmfl simple 5 (all); rmtr, rmw, rmd, rmhg
  have no alpha_test category.

---

## 10. Open questions

- **Ordinal of `MccHalo3U13`.** This document, `sbsp-render-geometry.md` and the task brief use
  22 (counted from `Enums.cs`); `container.md` section 1 says 23. The resolved values here do
  not change either way (the 28-byte ShaderMapBlock range `[MccHalo3U4, Halo3ODST)` covers both
  readings and the byte stride 28 is verified directly), but `container.md` should be corrected.
- The alpha-test threshold is not stored in the tags; 0.5 is assumed.
- `double_multiply`: Reclaimer renders it opaque, HaloExport as multiply. Neither is checked
  against the game.
- The exact terrain `morph` blend (whether the engine normalises the four weights, and what
  `dynamic_morph` does) is not derived; the weighted sum is an approximation.
- How the engine reads "linear"-curve colour maps (raw, or with a gamma). `gray_50_percent`
  points to raw; the viewer reads them as sRGB (7.5).
- The unread fields flagged "?" in sections 2 to 4 (sampler flag bytes at `+0x12`/`+0x13` of a
  shader map, the per-entry-point tables, the water function blocks) are described from the
  bytes only.
- Only riverworld was surveyed. Campaign maps have `rmcs`, `rmct`, `rmsk` (skin) and more
  `rmd` variants; `rmsk` and `rmct` probably follow the same `ShaderTag` layout (Reclaimer reads
  them with it) but their rmdf categories are not listed here.
- The material model of section 11 is our approximation, not Halo's shader code: the lobe
  normalisation, how area specular and the environment map are weighted, and the exact fresnel
  curve are not derived from the game.

---

## 11. The material model: parameters past the colour map (2026-10-28)

What the viewer reads for lighting a surface, from `blam shader` / `blam textures --list` on
the multiplayer maps (all names are template argument and usage names, section 3, so they are
read by name through `Shader::param` and `Shader::map`). No GPL code involved: these are the
names and values in the user's maps.

### 11.1 How common each option is (`blam textures <map>` prints the census)

| map | rmsh materials | bump_mapping off / standard / detail | material_model | environment_mapping none / dynamic / per_pixel | self_illumination not off |
|---|---|---|---|---|---|
| riverworld | 70 | 25 / 28 / 17 | two_lobe_phong 31, diffuse_only 29, cook_torrance 4, glass 4 | 48 / 17 / 5 | 15 |
| construct | 98 | 25 / 36 / 37 | two_lobe_phong 68, diffuse_only 17, glass 5, cook_torrance 3, single_lobe_phong 3 | 32 / 63 / 3 | 32 |

So bump maps and the two-lobe phong are the norm on level geometry; `dynamic` environment
mapping (a cube map the engine renders at runtime, nothing in the tag) is the usual reflection.

### 11.2 Maps

| usage | used when | format seen | meaning |
|---|---|---|---|
| `detail_map` | `albedo` default, detail_blend, two_detail* , three_detail_blend | colour (BC1/BC3) | `albedo = base * detail * 2^2.2` (the same multiplier as terrain, 7.5): mid grey is neutral |
| `bump_map` | `bump_mapping` standard, detail, detail_masked ... | **DXN = BC5 signed (DXGI 84)**, 300 of 436 bump bitmaps on riverworld; U8V8 (R8G8 signed) 96; one A8R8G8B8 | tangent-space normal, x and y only, z = sqrt(1 - x^2 - y^2). Curve byte "linear" |
| `bump_detail_map` | `bump_mapping` detail* | as above | added to the bump's x and y, scaled by `bump_detail_coefficient`, tiling much tighter |
| `specular_mask_texture` | `specular_mask` from_texture / from_color_texture | colour | red is the mask |
| (base map alpha) | `specular_mask` specular_mask_from_diffuse | | the mask |
| `alpha_test_map` | `alpha_test` simple, when the template has it | A8R8G8B8 (cyberdyne grates), alpha replicated | the cut-out; the base map's alpha is then *not* the cut (it is the specular mask) |
| `self_illum_map`, `self_illum_detail_map` | `self_illumination` not off | colour | emissive, see 11.4 |
| `environment_map` | `environment_mapping` per_pixel | cube | not drawn yet (11.5) |
| `bump_map_m_N`, `detail_bump_m_N` | terrain layer N | BC5 | the layer's normal (and its detail normal, not drawn) |

The engine's stand-ins `shaders\default_bitmaps\bitmaps\default_vector` (flat normal),
`default_detail` (0x80 grey) and `gray_50_percent` change nothing and are skipped.

The tangent frame is Halo's own: `s_world_vertex` and `s_rigid_vertex` store a tangent and a
binormal next to the normal (`sbsp-render-geometry.md` 5). The bumped normal is
`normalize(T * x + B * y + N * z)` with `B` the stored binormal; the viewer keeps the tangent
and the sign of `dot(cross(N, T), B)` (+1 or -1 per vertex) and rebuilds `B` from them, flipping
the sign under a mirroring placement.

### 11.3 Specular parameters by `material_model`

| model | parameters (template argument names) |
|---|---|
| `two_lobe_phong` (and the `*_phong*` variants) | `specular_coefficient`, `normal_specular_power`, `normal_specular_tint`, `glancing_specular_power`, `glancing_specular_tint`, `fresnel_curve_steepness`, `area_specular_contribution`, `analytical_specular_contribution`, `environment_map_specular_contribution`, `albedo_specular_tint_blend`, `analytical_anti_shadow_control` |
| `cook_torrance` (and `cook_torrance_*`) | `specular_coefficient`, `roughness`, `specular_tint`, `fresnel_color`, `area_specular_contribution`, `analytical_specular_contribution`, `environment_map_specular_contribution` |
| `glass` | `specular_coefficient`, `fresnel_coefficient`, `fresnel_curve_steepness`, `fresnel_curve_bias`, `roughness`, `analytical_specular_contribution`, `area_specular_contribution` |
| environment (any model, `environment_mapping` not none) | `env_tint_color`, `env_roughness_scale` |
| terrain layer N (`material_N` = `diffuse_plus_specular*`) | `specular_coefficient_m_N`, `specular_power_m_N`, `specular_tint_m_N`, `fresnel_curve_steepness_m_N`, `area_specular_contribution_m_N`, `analytical_specular_contribution_m_N`, `environment_specular_contribution_m_N`; the mask is the layer's base alpha |
| water (`rmw `) | `fresnel_coefficient` (0.1 on Valhalla), `reflection_coefficient`, `water_diffuse` ... |

Typical values: Valhalla rock `specular_coefficient` 0.2, powers 22 / 10; Forerunner panels on
The Construct coefficient 1, powers 10 / 10, `normal_specular_tint` a dull grey-green, env
contribution 0.2-1.0; Last Resort cliffs (terrain) coefficient 1, power 5-10, tint (0.18, 0.14,
0.09): the tints are small, so they matter.

### 11.4 Self-illumination

| option | viewer |
|---|---|
| `simple`, `simple_with_alpha_mask`, `plasma` (approximated), others with a map | `self_illum_map * self_illum_color * self_illum_intensity` |
| `illum_detail` | the same times `self_illum_detail_map * 2^2.2` |
| `from_diffuse` | albedo * colour |
| `self_illum_times_diffuse` | map * albedo * colour |
| `3_channel_self_illum` | `map.r * channel_a + map.g * channel_b + map.b * channel_c` |

The emissive light goes through the same exposure as the lit surface.

**Animated intensity.** `self_illum_intensity` is often driven by a function of time
(`water.md` 3; `blam shader` lists them under `animations`). The value stored with the shader is
usually one end of the function's range: Last Resort's `military_equipment_2` stores 18 and runs
18 -> 30, Narrows' man cannons store 0.5 and run 1.5 -> 0.5, Isolation's `plasma_wall_a` stores 55
and runs 0 -> 55. Its `plasma_wall_b` and `plasma_wall_c` store **0** and run 0 -> 55 (8 s and
10 s), so drawn at the stored value two of Isolation's three kinds of energy-wall slit stayed dark.
The viewer does not animate the value; it keeps the stored one, except that an animated value
stored as 0 takes the top of its range (`textures.rs` `illum_intensity`). Among the level
`rmsh` shaders of Valhalla, The Pit, Guardian, Last Resort, High Ground, Narrows, Blackout and
Isolation, only Isolation's two walls and `020_base`'s `computer_screen_small` (0 -> 1, in
Isolation's tags but not placed) store 0 while animated.

`material_model organism` (Isolation's Flood balls, `060_floodship\shaders\flood_sack`) adds
`subsurface_map`, `transparence_map`, `occlusion_parameter_map`, `height_map` and a
`flat_environment_map`; the viewer draws it as diffuse only.

### 11.5 What the viewer does with it (`world/shaders/lightmap.wesl`)

With the lightmap decoded at the pixel (`lightmaps.md` 5) giving the SH, the dominant direction
`L` and intensity `D`:

```
n      = bumped normal; v = towards the eye; r = reflect(-v, n)
E      = irradiance(n)                                   // diffuse, lightmaps.md 5.3
edge   = (1 - n.v) ^ fresnel_curve_steepness
phong:  power = mix(normal_power, glancing_power, edge); tint = mix(normal_tint, glancing_tint, edge)
cook / glass / water: f = F0 + (1 - F0) * edge;  tint *= f     (F0 = fresnel_color / fresnel_coefficient)
spec   = D * (r.L)^power * max(n.L, 0) * analytical      // the dominant light's lobe
       + ambient(r) * area                               // the SH without the dominant light, towards r
spec   = spec * tint + ambient(r) * env_tint * env_contribution * (tint or f)   // stand-in for the cube map
colour = albedo * E + spec * specular_coefficient * mask + self_illum
```

Only per-pixel lightmapped surfaces (the BSP) get bump and specular; objects lit per vertex keep
their vertex irradiance and flat normals, and get `0.5 * E_vertex * env_tint * env_contribution *
specular_coefficient * mask` as their reflection. `per_pixel` cube maps are not sampled; the SH towards
the reflection stands in for both kinds of environment map.
