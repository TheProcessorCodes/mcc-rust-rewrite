# Sky and atmosphere (MCC Halo 3, U13)

How a Halo 3 map's sky gets its colour and how its distance haze works, as far as the bytes of the
user's `riverworld.map` (Valhalla) and `cyberdyne.map` (The Pit) show. Written 2026-10-28 from our
own probes (`blam dump`, `blam shader`, small Python readers); no GPL code or plugin XML was used.
Field names are ours, chosen for what the data does; where a meaning is a guess it says so.

Code: `crates/blam_geometry/src/atmosphere.rs` (reader, tests pin the numbers below),
`crates/halo3/src/world/sky.rs` and `world/shaders/sky.wesl` (drawing).

## 1. Where things are

The scenario (`scnr`) points at four per-level tags in a row:

| scnr offset | class | riverworld | meaning |
|---|---|---|---|
| `0x660` | `cfxs` | #6875 `levels\multi\riverworld\riverworld` | camera fx: the exposure (section 5); bloom and the rest not decoded |
| `0x670` | (null reference) | | unknown |
| `0x680` | `skya` | #6876 `levels\multi\riverworld\sky\riverworld` | **sky_atm_parameters: the atmosphere** |
| `0x690` | `chmt` | #6877 `levels\multi\riverworld\riverworld` | lighting globals (not decoded) |
| `0x6AC` | `sLdT` | #6878 | the lightmap (`lightmaps.md`) |

The sky model itself is the `scen` in `scnr + 0x34` (Skies block, 20-byte entries:
`scenario-and-tag-reading.md` 2.2). On riverworld the entry's last four bytes are `FF FF 01 00`
(an i16 -1 and an i16 1; perhaps a name index and an active-BSP mask; not used).

The sky is ordinary geometry at its true size around the origin: riverworld's dome is a
7,400-7,560 wu shell (bottom rim at z -165), sun pieces at r ~7,000, cloud layers at r 570-7,200,
the near snowy mountain (`mountains_snow`) 500-1,150 wu out. The Pit's sky is similar.

No per-cluster atmosphere palette was found in the riverworld `sbsp`: no sbsp field holds the
setting names' string ids, and the clusters' bytes `+0x18..+0x1F` (`ff ff ff 00 | i16 | i16`) match
the acoustics palette (`sbsp + 0x118`, 3 entries `halo_exterior`, `interior`, ...) rather than
atmosphere. Which setting applies where is therefore taken from the setting flags (3.3).

## 2. `skya` header

`skya` tag data, riverworld (0x50 bytes; cyberdyne the same layout):

| offset | type | riverworld | notes |
|---|---|---|---|
| `0x00` | u32 | 0 | flags |
| `0x04` | tag ref (16) | null | a bitmap reference by its slot (patchy fog?) |
| `0x14` | f32 | 0.1 | |
| `0x18` | f32 | 0.1 | |
| `0x1C` | f32 | 1e-5 | |
| `0x20` | f32 | 25 | |
| `0x24` | f32 | 5 | |
| `0x28` | f32 | 2 | |
| `0x2C` | f32 | 100 | |
| `0x30` | u32 | 0 | |
| `0x34` | block | 4 entries | **atmosphere settings**, 0xA4 bytes each (section 3) |
| `0x40` | block | 14 entries | underwater settings, 20 bytes each: string id name (`water_physics00`, `water_physics01`, ...), f32 (1.0), linear RGB (0.318 0.459 0.659). Stored just before the header |
| `0x4C` | f32 | 0.15 | |

The 0x14-0x2C floats are probably the patchy-fog sheet settings (repeat rates, distances); unused.

## 3. Atmosphere setting, 0xA4 (164) bytes

| offset | type | name (ours) | haze_level | haze_skydome | not_used | haze_skydome | cyberdyne_sky_fog |
|---|---|---|---|---|---|---|---|
| `0x00` | u16 | flags | 3 | 7 | 3 | 7 | 3 |
| `0x02` | u16 | pad | 0 | 0 | 0 | 0 | 0 |
| `0x04` | string id | name | `haze_level` | `haze_skydome` | `not_used` | `haze_skydome` | `cyberdyne_sky_fog` |
| `0x08` | f32 | sun elevation, deg | 45 | 45 | 45 | 45 | 40 |
| `0x0C` | f32 | sun heading, deg (about +Z from +X) | 25 | 25 | 25 | 25 | 120 |
| `0x10` | 3 x f32 | sun colour (linear RGB) | 0.541 0.584 0.965 | same | same | same | 0.820 0.867 0.855 |
| `0x1C` | f32 | sun intensity | 4 | 5 | 5 | 5 | 2.5 |
| `0x20` | f32 | sea level (wu) | 0 | 0 | 0 | 0 | 0 |
| `0x24` | f32 | Rayleigh scale height (wu) | 150 | 150 | 150 | 158 | 150 |
| `0x28` | f32 | Mie scale height (wu) | 6 | 5 | 5 | 5 | 2 |
| `0x2C` | f32 | Rayleigh multiplier | 0.05 | 0.14 | 0.12 | 0.14 | 0.075 |
| `0x30` | f32 | Mie multiplier | 0.025 | 0.03 | 0.2 | 0.03 | 0.05 |
| `0x34` | f32 | Mie asymmetry g | 0.2 | 0.1 | 0 | 0.1 | 0.6 |
| `0x38` | f32 | desaturation | 0 | 0 | 0 | 0 | 0.2 |
| `0x3C` | f32 | distance bias (wu) | -15 | 0 | 0 | 0 | -25 |
| `0x40` | f32 | max fog thickness (wu) | 65536 | 65536 | 65536 | 65536 | 65535 |
| `0x44` | 3 x f32 | beta_m (Rayleigh, per wu) | 2.95e-4 5.02e-4 1.06e-3 | 8.25e-4 1.41e-3 2.96e-3 | | | 5.39e-4 7.88e-4 1.46e-3 |
| `0x50` | 3 x f32 | beta_p (Mie, per wu) | 1.43e-3 1.85e-3 2.63e-3 | 1.72e-3 2.22e-3 3.15e-3 | 1.15e-2 1.48e-2 2.10e-2 | | 3.08e-3 3.75e-3 4.99e-3 |
| `0x5C` | 3 x f32 | beta_m theta prefix | 1.79e-5 3.05e-5 6.43e-5 | 5.01e-5 8.54e-5 1.80e-4 | | | 3.27e-5 4.79e-5 8.84e-5 |
| `0x68` | 3 x f32 | beta_p theta prefix | 3.33e-4 4.34e-4 6.24e-4 | 4.00e-4 5.20e-4 7.49e-4 | | | 7.19e-4 8.79e-4 1.18e-3 |
| `0x74` | 6 x f32 | | 0 | 0 | 0 | 0 | 0 |
| `0x8C` | tag ref (16) | (a weather effect?) | null | null | null | null | null |
| `0x9C` | 8 bytes | | 0 | 0 | 0 | 0 | 0 |

### 3.1 How the meanings were pinned (all checked on the bytes; `atmosphere.rs` tests)

- **Sun angles.** The `sundisk` pieces of riverworld's sky model sit in direction
  (0.645, 0.298, 0.703); `0x08` = 45 as elevation and `0x0C` = 25 as heading give
  (0.641, 0.299, 0.707): **0.3 degrees apart** (`riverworld_sundisk_matches_sun_angles`). The
  lightmap's dominant light (`lightmaps.md` 5.4, (0.64, 0.34, 0.69)) agrees too.
- **Multipliers.** The stored coefficients are one base set times the multipliers: haze_skydome's
  beta_m is exactly 0.14 / 0.05 = 2.8x haze_level's, its beta_p 0.03 / 0.025 = 1.2x, not_used's
  beta_p 0.2 / 0.025 = 8x. So `0x2C` scales `0x44`/`0x5C` and `0x30` scales `0x50`/`0x68`.
- **Which is Rayleigh.** `0x44` grows 3.6x from red to blue (Rayleigh-like, wavelength^-4); `0x50`
  only 1.8x (haze). Halo names them `beta_m` (molecules) and `beta_p` (particles).
- **Phase prefixes.** `0x5C / 0x44` is 0.0607 in every channel and every setting: a constant
  (close to Rayleigh's 3/(16 pi) = 0.0597). `0x68 / 0x50` is 0.2325 / 0.2345 / 0.2376 per channel:
  wavelength-dependent, as Preetham's Mie term is.
- **Heights.** Rayleigh 150 wu against Mie 5-6 wu is the usual ordering (air thins slowly, haze
  hugs the ground). Valhalla's lake is at about z 0 (the sea level) and its playable ground at
  z 0-90, so haze is thick over the water and thin up on the bases, as in the game.
- **Flag bit 2 (`0x04`)** is set only on the two `haze_skydome` settings. Read here as "for the sky".
  Not proven; the sky drawn with them matches the reference screenshots.

### 3.2 The scattering model used

Single scattering of sunlight, after Hoffman and Preetham (2002), with exponential densities over
the sea level. For a ray from the eye `e` to a point `p` (Halo frame, wu), `d = clamp(|p - e| +
distance_bias, 0, max_fog_thickness)`, heights `h = z - sea_level`:

```
D_m = d * mean(exp(-h / H_m)) over the ray      # mean density times length, Rayleigh
D_p = d * mean(exp(-h / H_p)) over the ray      # the same for haze
tau = beta_m * D_m + beta_p * D_p               # per channel
extinction = exp(-tau)
mu = dot(ray direction, sun direction)
phase_m = beta_m_theta * (1 + mu^2)
phase_p = beta_p_theta * (1 - g)^2 / (1 + g^2 - 2 g mu)^1.5
inscatter = (phase_m * D_m + phase_p * D_p) / tau * sun_colour * sun_intensity * (1 - extinction)
colour = surface * extinction + inscatter
```

`mean(exp(-h/H))` between heights `h0` and `h1` is `H (exp(-h0/H) - exp(-h1/H)) / (h1 - h0)`.
With the viewer's exposure (x4, `lightmaps.md` 8) this gives riverworld a zenith of about
(0.08, 0.15, 0.47) linear and a pale horizon, the colours of the in-game screenshots, from a
sky dome whose own shader colour is almost black (`albedo_color` (0.004, 0.004, 0.063)). So the
blue of Halo 3's sky is the atmosphere, not a texture.

Not confirmed against the game's shader code: the exact phase-function forms, how `desaturation`
is applied (zero on riverworld), and whether Halo integrates the height falloff the same way.

### 3.3 Which setting goes where (viewer's rule)

- World geometry: the first setting without flag bit 2 (`haze_level`; cyberdyne's only one).
- Sky geometry: the first setting with bit 2 (`haze_skydome`), else the world's.

## 4. Sky shaders

Riverworld's sky materials (`blam shader riverworld <tag>`), all `rmsh` with
`misc first_person_never`:

| shader | albedo | blend | maps |
|---|---|---|---|
| `skydome` | constant_color (0.004 0.004 0.063) | opaque | self_illum `stars` x7 (simple, intensity 1) |
| `clouds_patchy` | two_detail_black_point | alpha_blend | base `clouds_patchy_mask`, detail and detail2 `clouds_altocumulus` x8 / x10 |
| `clouds_horizon` | default | alpha_blend | base `clouds_horizon_mask`, detail `clouds_horizon` x4 x1 |
| `clouds_cumulus` | detail_blend | alpha_blend | base `clouds_cumulus`, detail `clouds_cumulus_overlay`, detail2 `smoke_billow` x3.5 x3 |
| `clouds_fore_frontlit` | two_detail_overlay | alpha_blend | base, detail, detail2 x4 x1, detail_map_overlay |
| `clouds_hazy_wisps` | two_detail_overlay | additive | base, detail and detail2 x1 x0.25, overlay |
| `mountains_terrain` | three_detail_blend | opaque | base, detail x16 x12, detail2 x10, detail3 x16 x12 (+ bump) |
| `mountains_snow` | default | opaque | base `mountains_snow`, detail `default_detail` (+ bump) |
| `matte_waypoint_reverse` | default | alpha_blend | base, detail `default_detail` |
| `sundisk` | default | additive | base and self_illum `sundisk` (self-illum intensity 50, colour 0.93 0.85 0.93) |
| `sun_ring` | default | additive | base and detail `sun_ring_base` |
| `sun_halves` | two_detail | additive | base, detail and detail2 `clouds_high_detail_1` (x1 x0.25, x1) |
| `sun_lines` | two_detail | additive | base, detail; **detail2 empty** |

An empty detail slot behaves as Halo's grey `default_detail` (neutral after `M` below); bevy binds
white for a missing texture, so the shader is told which slots are filled.

Parameters that equal the template default are not stored, so a shader without `albedo_color`
uses white.

The combinations as drawn (`sky.wesl`; `M` = 4.59479, which maps a detail value of 0.5 sRGB to 1):

| albedo | rgb | alpha |
|---|---|---|
| default | base * detail * M | base.a * detail.a |
| detail_blend | base * lerp(detail, detail2, base.a) * M | lerp(...).a |
| three_detail_blend | base * lerp(lerp(detail, detail2, sat(2 base.a)), detail3, sat(2 base.a - 1)) * M | that blend's alpha |
| two_detail_overlay | lerp(base * lerp(detail, detail2, base.a) * M, overlay, overlay.a) | lerp(detail, detail2, base.a).a |
| two_detail | base * detail * detail2 * M^2 | base.a * detail.a * detail2.a |
| two_detail_black_point | as two_detail | black_point(base.a, detail.a * detail2.a) |
| constant_color | albedo_color | 1 |

`black_point(b, a) = m * sat((a - b) / (m - b)) + sat(a - m)` with `m = (b + 1) / 2`. On
`clouds_patchy` the base mask's alpha is low (about 0.1) in the middle of the cloud disc and 1 at
its rim, so clouds show overhead and fade out toward the horizon, as in the game; using
`1 - base.a` instead put the clouds at the horizon and left the zenith empty.

Two facts the pictures depended on:

- **Sample each bitmap the way its DXGI format says.** The cloud masks and most cloud colour maps
  are `BC3_UNORM` (DXGI 77, "linear" curve) and must be read linearly; the details such as
  `clouds_altocumulus` are `BC3_UNORM_SRGB` (78). Reading the masks as sRGB made the patchy clouds
  dark grey. (`Texture::srgb` carries the flag.)
- Lit sky surfaces (`material_model` diffuse_only) need light: the viewer uses the irradiance of a
  sunlit Valhalla lightmap texel times the exposure for every map. Halo's own source of sky
  lighting was not found.

## 5. Camera fx (`cfxs`): the exposure

Riverworld `cfxs` #6875, 0x288 bytes; every multiplayer map has one level `cfxs` besides
`globals\default` (scnr `+0x660`). The first 0x30 bytes over the 27 multiplayer maps
(`blam_geometry::camera_fx` reads `+0x00`, `+0x10`, `+0x14`, `+0x18`):

| map | `+0x00` | `+0x08` | `+0x0C` | `+0x10` | `+0x14` | **`+0x18`** | `+0x1C` | mean E (lightmap) |
|---|---|---|---|---|---|---|---|---|
| riverworld | 0x14 | 0.05 | 0.01 | 0 | 3 | **-4.837** | 0.033 | 0.22 |
| bunkerworld | 0x14 | 0.1 | 0.025 | 2 | 3.5 | **-2.515** | 0.033 | 0.05 |
| ghosttown | 0x14 | 0.1 | 0.04 | 1.5 | 3.5 | **-2.515** | 0.033 | 0.03 |
| sidewinder | 0x14 | 0.05 | 0.025 | -0.25 | 2 | **-2.737** | 0.033 | 0.28 |
| cyberdyne, docks | 0x14 | 0.2 | 0.01 / 0.035 | 0.5 / 1 | 2.5 / 2 | **-3.322** | 0.033 | 0.20 / 0.04 |
| sandbox | 0x10 | 0.05 | 0.005 | 0 | 0.75 | **-3.322** | 0.5 | 1.35 |
| shrine, deadlock | 0x14 | 0.2 | 0.014 | 0.25 / 0.5 | 2.5 / 3 | **-4.644** | 0.033 | 0.82 / 0.20 |
| chill, isolation | 0x14 | 0.2 | 0.014 | -0.5 / 0 | 3 | **-5.059** | 0.033 | 0.70 / 0.14 |
| chillout | 0x14 | 0.1 | 0.04 | 0.5 | 1.5 | **-16.61** | 0.033 | 0.13 |
| s3d_waterfall | 0x16 | 0.05 | 0.01 | 0 | 3 | -4.837 | 0.1 | 0.22 |

(Other maps: armory, fortress, midship -4.322; construct, guardian, salvation, zanzibar
-4.474; descent, warehouse -4.059; lockout -3.837; s3d_edge -4.238; s3d_turf, snowbound
-4.837; spacecamp -4.184. "mean E" is `blam lightmap`'s mean diffuse irradiance over sampled
cluster vertices.)

- `+0x18` is an **exposure in stops**: the values are round logs (-3.322 = log2(1/10),
  -4.322 = log2(1/20), -4.644 = log2(1/25)), and darker-lit maps have less negative values
  (Standoff -2.5 with mean E 0.05, Valhalla -4.8 with 0.22): `2^stops * E` varies about 10x over
  the maps where E alone varies 45x. So the game scales its light by `2^stops`.
- `+0x10` / `+0x14` look like an auto-exposure range in stops (min <= max on every map);
  `+0x08` / `+0x0C` like adaptation speeds. Not used. Chillout's -16.6 (= log2(1e-5)) is an
  outlier, probably meaning "auto exposure only".

The viewer: `exposure = 4 * 2^(0.6 * (stops + 4.8365))`, i.e. Valhalla keeps the hand-picked x4
and other maps move from it by 60 % of their stop difference (the full difference over-exposes
Avalanche at x17 and Standoff at x20, plausibly because the game's auto exposure pulls them back).
Values outside -8..0 stops (Chillout's -16.6) are ignored and get the plain x4. Sandbox's
lightmap is so bright (mean E 1.35) that its -3.32 stops still blow it out; the viewer caps the
exposure so that the mean E (luminance) comes out at most 3.0 (`docs/viewer.md`, "Lighting").
`HALO3_EXPOSURE` (a fixed multiplier) and `HALO3_EXPOSURE_SLOPE` override.

## 6. The Pit (cyberdyne)

`skya` #6528 `levels\multi\cyberdyne\sky\cyberdyne`, one setting `cyberdyne_sky_fog` (table in 3):
a whiter, weaker sun (2.5) at elevation 40, heading 120, strong forward haze (g 0.6), 2 wu haze
height. Its sky (`levels\multi\cyberdyne\sky\sky`: skydome, Kilimanjaro, puffy and wispy clouds,
treeline, smoke) draws as a hazy savanna under cumulus.
