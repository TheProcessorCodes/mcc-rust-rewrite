# Uploading Halo's textures in bevy 0.20 and applying them to `StandardMaterial`

Written 2026-10-27 by reading the bevy 0.20.0 / wgpu 30.0.1 sources in the local cargo registry
(`C:\Users\gibso\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\`) and the IW4L material
code (`D:\GAME ISO AND ROMS\PC - AI MASHUP\mw2-skate-3-src\src\mashup\...`). Everything here is
about **our own code** in `crates\halo3\src\world.rs`; it does not depend on any map file and
contains no Reclaimer facts. The bitmap tag / resource page layout (how to get the BCn bytes out
of `shared.map`) is another reader's topic; this document starts where that one ends: you hold
`width`, `height`, a format, and the mip levels as byte slices.

Line numbers are given as `file:line` into the crates named above so the next person can check.

---

## 1. One-paragraph answer

Build a `bevy::image::Image` with `Image::new_uninit(...)`, set
`texture_descriptor.mip_level_count`, put the mips into `image.data` **concatenated largest
first with no padding** (each mip is exactly `ceil(w/4) * ceil(h/4) * block_bytes` bytes), pick
the `TextureFormat` (`Bc1RgbaUnormSrgb` / `Bc3RgbaUnormSrgb` for colour, `Bc5RgUnorm` for bump
maps, no-`Srgb` variants for data), give it an `ImageSampler::Descriptor` with `Repeat` address
modes and linear mip filtering, add it to `Assets<Image>`, and hand the `Handle<Image>` to
`StandardMaterial::base_color_texture` (and `normal_map_texture`). BC compression is already
enabled: bevy's `RenderPlugin` requests every feature the adapter supports, which on a desktop
DX12/Vulkan adapter includes `TEXTURE_COMPRESSION_BC`. Tiling goes through
`StandardMaterial::uv_transform: Affine2`; alpha-tested shaders use `AlphaMode::Mask(0.5)`,
translucent ones `AlphaMode::Blend`. Terrain blend maps cannot be expressed with
`StandardMaterial` at all and need a custom material (IW4L's stand-in shader is the prior art,
section 10).

---

## 2. Is BC texture compression on? (yes, by default on desktop)

| fact | where |
|---|---|
| `WgpuSettings::default()` sets `priority: WgpuSettingsPriority::Functionality` (overridable by env `WGPU_SETTINGS_PRIO`), `features: Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES`, `limits: wgpu::Limits::default()`, `backends: Backends::all()` (env `WGPU_BACKEND`) | `bevy_render-0.20.0\src\settings.rs:71-156` |
| With `Functionality` priority the renderer asks for **`features = adapter.features()`** (everything the adapter offers, minus `MAPPABLE_PRIMARY_BUFFERS` on discrete GPUs) and **`limits = adapter.limits()`**, then ORs in `options.features` | `bevy_render-0.20.0\src\renderer\mod.rs:320-340` |
| `Features::TEXTURE_COMPRESSION_BC` ("texture-compression-bc"): BC1-BC7, 4x4 blocks of 8 or 16 bytes, supported on all desktops; guarantees `COPY_SRC | COPY_DST | TEXTURE_BINDING` usage for BC formats | `wgpu-types-30.0.1\src\features.rs:1540-1558` |
| After device creation bevy inserts the resource `CompressedImageFormatSupport(CompressedImageFormats::from_features(device.features()))`; `CompressedImageFormats::BC` is set when the feature is present, and `.supports(TextureFormat)` answers per format | `bevy_render-0.20.0\src\settings.rs:193`, `bevy_image-0.20.0\src\image.rs:2390-2477` |

So on the user's Windows machine (DX12 or Vulkan backend) nothing has to be added to
`Cargo.toml` or `RenderPlugin` for BC1/BC3/BC5/BC7 textures to work. The only way it would be
off is a software adapter or `WGPU_SETTINGS_PRIO=webgl2`. A defensive check at startup, in a
system that runs after the render device exists:

```rust
use bevy::render::renderer::RenderDevice;
use bevy::render::settings::WgpuFeatures;

fn log_bc_support(device: Res<RenderDevice>) {
    let bc = device.features().contains(WgpuFeatures::TEXTURE_COMPRESSION_BC);
    let max_2d = device.limits().max_texture_dimension_2d; // 16384 on modern desktop GPUs
    info!("BC texture compression: {bc}, max 2D texture {max_2d}");
}
```

(bevy inserts clones of `RenderDevice`, `RenderQueue` and `RenderAdapterInfo` into the main
world as well as the render world, `bevy_render-0.20.0\src\settings.rs:195-198`, so an ordinary
system can read them. If `bc` is false, decode the blocks to RGBA8 on the CPU before upload;
IW4L carries such a decoder in `asset_material\src\material_images\decode.rs`.)

---

## 3. The `Image` asset

### 3.1 Fields (`bevy_image-0.20.0\src\image.rs:634-667`)

| field | type | meaning |
|---|---|---|
| `data` | `Option<Vec<u8>>` | all mips (and layers) back to back; `None` only for GPU-written textures |
| `data_order` | `TextureDataOrder` | how layers and mips interleave in `data`; default `LayerMajor` = "Layer0Mip0 Layer0Mip1 ... Layer1Mip0 ..." = DDS order (`wgpu-types-30.0.1\src\texture.rs:41-63`). For a plain 2D texture with one layer this is simply mip 0, mip 1, ... |
| `texture_descriptor` | `TextureDescriptor<Option<&'static str>, &'static [TextureFormat]>` | `size: Extent3d`, `mip_level_count`, `sample_count`, `dimension`, `format`, `usage`, `view_formats`, `label` |
| `sampler` | `ImageSampler` | `Default` (the `ImagePlugin` sampler) or `Descriptor(ImageSamplerDescriptor)` |
| `texture_view_descriptor` | `Option<TextureViewDescriptor<..>>` | only needed for cube maps / arrays |
| `asset_usage` | `RenderAssetUsages` | see 3.4 |
| `copy_on_resize` | `bool` | leave `false` |

### 3.2 Constructors (`image.rs:1123-1170`)

- `Image::new(size, dimension, data, format, asset_usage)`: in debug builds asserts
  `pixel_count(size) * pixel_size == data.len()` **but only when `format.pixel_size()` is
  `Ok`**, and `pixel_size()` returns `Err` for every block-compressed format
  (`image.rs:2377-2383`, it only answers for `block_dimensions() == (1, 1)`). So for BCn data
  the length is never checked; for RGBA8 with appended mips the assert **fires** because it only
  counts mip 0. Use `new_uninit` + `data = Some(..)` whenever there is more than one mip.
- `Image::new_uninit(size, dimension, format, asset_usage)`: sets `mip_level_count: 1`,
  `sample_count: 1`, `usage: TEXTURE_BINDING | COPY_DST | COPY_SRC`, `view_formats: &[]`,
  `sampler: ImageSampler::Default`, `data: None`. This is what IW4L uses
  (`material_images.rs:315-326`, `materialization.rs:265-283`).

### 3.3 The texture descriptor fields we set

| field | value for a Halo 2D bitmap |
|---|---|
| `size` | `Extent3d { width, height, depth_or_array_layers: 1 }` of mip 0 |
| `dimension` | `TextureDimension::D2` (also for 1-pixel-high strips: bevy's DDS loader only picks `D1` for uncompressed data, `dds.rs:81-88`) |
| `format` | section 4 |
| `mip_level_count` | number of mips you actually put in `data` (1 if only the top level); must be `<= Extent3d::max_mips(D2)` = `32 - leading_zeros(max(w, h))` (`origin_extent.rs:151-163`) |
| `usage` | leave the `new_uninit` default (`TEXTURE_BINDING | COPY_DST | COPY_SRC`); `create_texture_with_data` adds `COPY_DST` itself anyway (`wgpu-30.0.1\src\util\device.rs:90`). Do not add `RENDER_ATTACHMENT` or `STORAGE_BINDING`: BC formats cannot have them |
| `label` | `Some("...")` needs a `&'static str`; leave `None` (it is only a debug aid for assets, `image.rs:647-650`) |

### 3.4 `RenderAssetUsages` (`bevy_asset-0.20.0\src\render_asset.rs`)

Bitflags `MAIN_WORLD = 1`, `RENDER_WORLD = 2`, default both. With **`RENDER_WORLD` only** the
CPU copy (`data`) is dropped once the image has been extracted and uploaded
(`bevy_render-0.20.0\src\render_asset.rs:348-359`); the handle stays valid. Use `RENDER_WORLD`
for every map texture (IW4L does) so a 1.5 GB map does not stay resident twice. Keep
`MAIN_WORLD | RENDER_WORLD` only for images you want to read back (none so far).

### 3.5 How the data reaches the GPU (why the layout rule is what it is)

`GpuImage::prepare_asset` (`bevy_render-0.20.0\src\texture\gpu_image.rs:63-147`) calls
`render_device.create_texture_with_data(queue, &texture_descriptor, data_order, data)`. That
helper (`wgpu-30.0.1\src\util\device.rs:81-165`) walks layers and mips in `data_order`, and for
each mip:

```
mip_size      = (max(1, width >> mip), max(1, height >> mip))          // logical size
mip_physical  = round each of those up to a multiple of the block size  // 4 for BCn
width_blocks  = mip_physical.width / 4
height_blocks = mip_physical.height / 4
bytes_per_row = width_blocks * block_bytes                              // 8 for BC1/BC4, 16 otherwise
data_size     = bytes_per_row * height_blocks
queue.write_texture(mip, &data[offset .. offset + data_size],
                    TexelCopyBufferLayout { offset: 0, bytes_per_row, rows_per_image: height_blocks },
                    mip_physical)
offset += data_size
```

Consequences:

- Mips are **densely packed, largest first, no padding or alignment between them**. The
  256-byte `COPY_BYTES_PER_ROW_ALIGNMENT` rule does *not* apply to `queue.write_texture`
  (wgpu-core only enforces it for buffer-to-texture copies, `wgpu-core-30.0.1\src\command\transfer.rs:484-487`).
- Each mip's rows are block rows: `ceil(w/4)` blocks wide, `ceil(h/4)` block rows, block bytes
  8 (BC1, BC4) or 16 (BC2, BC3, BC5, BC6H, BC7) (`wgpu-types-30.0.1\src\texture\format.rs:1215-1275`).
  This is the natural DDS/Xbox layout, so Halo mips that come out of the page as a plain
  linear block array can be copied straight in.
- The 2x2 and 1x1 mips of a BC texture are one 8- or 16-byte block each (physical size 4x4).
- If `data` is shorter than the sum of the mip sizes, the slice `data[offset..end]` **panics**
  on the render thread. Compute the expected length and refuse the image if it does not match.
- The upload happens for every image on the frame it is extracted; hundreds of large textures
  in one frame mean one long frame, not a crash. There is a soft per-frame upload budget:
  insert the main-world resource `RenderAssetBytesPerFrame::new(bytes)`
  (`bevy_render-0.20.0\src\render_asset.rs:505-526`); images take part because
  `GpuImage::byte_len` reports `data.len()` (`texture\gpu_image.rs:58-60`). Whole assets are
  uploaded, so one 5 MiB texture past the limit still goes through; the rest wait a frame.

### 3.6 Expected byte count of a mip chain

```rust
/// Bytes a BCn mip chain must occupy in `Image::data`, largest mip first.
pub fn bc_chain_len(width: u32, height: u32, levels: u32, block_bytes: u32) -> usize {
    (0..levels)
        .map(|l| {
            let w = (width >> l).max(1).div_ceil(4) as usize;
            let h = (height >> l).max(1).div_ceil(4) as usize;
            w * h * block_bytes as usize
        })
        .sum()
}
```

(IW4L's equivalent: `material_images\decode.rs:90-96`.)

---

## 4. Which `TextureFormat` (sRGB or linear)

`wgpu-types-30.0.1\src\texture\format.rs:291-393`:

| Halo / DDS name | `TextureFormat` for colour (sRGB decode in the sampler) | for data (linear) | bytes per 4x4 block |
|---|---|---|---|
| DXT1 | `Bc1RgbaUnormSrgb` | `Bc1RgbaUnorm` | 8 |
| DXT3 | `Bc2RgbaUnormSrgb` | `Bc2RgbaUnorm` | 16 |
| DXT5 | `Bc3RgbaUnormSrgb` | `Bc3RgbaUnorm` | 16 |
| ATI1 / BC4 | - | `Bc4RUnorm` (one channel) | 8 |
| DXN / ATI2 / BC5 | - | `Bc5RgUnorm` (two channels; bump maps) | 16 |
| BC7 | `Bc7RgbaUnormSrgb` | `Bc7RgbaUnorm` | 16 |
| A8R8G8B8 (uncompressed) | `Bgra8UnormSrgb` or `Rgba8UnormSrgb` | `Bgra8Unorm` / `Rgba8Unorm` | 4 per pixel |

Helpers: `format.add_srgb_suffix()`, `remove_srgb_suffix()`, `is_srgb()`, `block_dimensions()`,
`block_copy_size(None)`, `required_features()`, `is_compressed()`.

Rule: anything the shader multiplies into the surface colour (base/diffuse map, detail map,
change-colour map, self-illumination colour) is authored in sRGB and wants the `Srgb` variant
so the sampler converts it to linear light for lighting. Anything that is **numbers** (bump /
normal maps, specular / roughness / mask maps, terrain blend weights, height maps) must be the
plain `Unorm` variant, otherwise the gamma curve bends the values. bevy's own documentation on
`normal_map_texture` says the same ("The normal map texture is in linear color space",
`bevy_pbr-0.20.0\src\pbr_material.rs:404-413`). IW4L encodes this as one bool per use
(`WrapRecipe::linear()` = `is_normal || !use_srgb_reads || force_linear`,
`materialization.rs:49-51`, format table `materialization.rs:234-247`).

Bump maps in Halo 3 are usually DXN = BC5 two-channel. bevy recognises `Bc5RgUnorm` as a
two-component normal map automatically (sets `TWO_COMPONENT_NORMAL_MAP` and reconstructs Z in
the shader, `pbr_material.rs:1168-1182`) so no CPU conversion is needed; IW4L converts BC5 to
"DXT5nm" only because its MW2 shaders expect that.

---

## 5. Samplers

### 5.1 Types (`bevy_image-0.20.0\src\image.rs:697-900`)

- `ImageSampler::Default` uses the `ImagePlugin` sampler: `ImageSamplerDescriptor::linear()`
  = linear mag/min/mipmap filters but **`ClampToEdge`** on all axes (`image.rs:178-183, 903-912`).
  Halo world textures tile, so `Default` is wrong for them.
- `ImageSampler::Descriptor(ImageSamplerDescriptor { .. })` per image, fields:
  `label: Option<String>`, `address_mode_u/v/w: ImageAddressMode` (`ClampToEdge` default,
  `Repeat`, `MirrorRepeat`, `ClampToBorder` needs a wgpu feature), `mag_filter`, `min_filter`,
  `mipmap_filter: ImageFilterMode` (`Nearest` default, `Linear`), `lod_min_clamp: 0.0`,
  `lod_max_clamp: 32.0`, `compare: None`, `anisotropy_clamp: u16 = 1`, `border_color`.
- wgpu rule (`wgpu-types-30.0.1\src\texture.rs:679`): `anisotropy_clamp` must be >= 1 and
  **if it is not 1 all three filters must be linear**. `set_anisotropic_filter(n)` sets exactly
  that (`image.rs:945-950`). Values up to 16 are plain WebGPU, no feature needed.

### 5.2 The sampler for a tiling world texture

```rust
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};

fn tiling_sampler(levels: u32) -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        // Trilinear only when there is a chain to filter between.
        mipmap_filter: if levels > 1 { ImageFilterMode::Linear } else { ImageFilterMode::Nearest },
        // Needs all three filters linear; 8 is a sensible default for ground seen at grazing angles.
        anisotropy_clamp: if levels > 1 { 8 } else { 1 },
        ..Default::default()
    })
}
```

Clamp-to-edge is right for things that must not wrap: lightmaps, UI, cube maps, decals whose
UVs stay inside 0..1. When the bitmap tag carries its own "clamp u / clamp v" style flags
(another reader's topic), map them to `ClampToEdge` per axis the way IW4L maps its sampler
state bits (`material_images.rs:676-724`). Each distinct descriptor creates one wgpu sampler at
upload time (`gpu_image.rs:165-170`); a handful of variants is fine.

A global alternative: build the app with
`ImagePlugin { default_sampler: ImageSamplerDescriptor { address_mode_u: Repeat, .. } }` so
`ImageSampler::Default` tiles. Per-image descriptors are clearer and are what IW4L does.

---

## 6. The complete builder

```rust
use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// What the bitmap reader hands over: mips largest first, already linear block arrays.
pub struct DecodedBitmap<'a> {
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat, // already the Srgb / linear variant you want
    pub mips: Vec<&'a [u8]>,   // mips[0] is the largest; may be just one
}

pub fn bitmap_to_image(b: &DecodedBitmap, sampler: ImageSampler) -> Result<Image, String> {
    let (bw, bh) = b.format.block_dimensions();
    if b.width % bw != 0 || b.height % bh != 0 {
        // wgpu refuses the texture outright (section 8.1); decompress or pad first.
        return Err(format!("{}x{} is not a multiple of the {bw}x{bh} block", b.width, b.height));
    }
    let levels = b.mips.len() as u32;
    let max_mips = Extent3d { width: b.width, height: b.height, depth_or_array_layers: 1 }
        .max_mips(TextureDimension::D2);
    if levels == 0 || levels > max_mips {
        return Err(format!("{levels} mips for {}x{}", b.width, b.height));
    }
    let block_bytes = b.format.block_copy_size(None).ok_or("odd format")?;
    let mut data = Vec::with_capacity(b.mips.iter().map(|m| m.len()).sum());
    for (level, mip) in b.mips.iter().enumerate() {
        let w = (b.width >> level).max(1).div_ceil(bw) as usize;
        let h = (b.height >> level).max(1).div_ceil(bh) as usize;
        let expect = w * h * block_bytes as usize;
        if mip.len() != expect {
            return Err(format!("mip {level}: {} bytes, expected {expect}", mip.len()));
        }
        data.extend_from_slice(mip);
    }

    let mut image = Image::new_uninit(
        Extent3d { width: b.width, height: b.height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        b.format,
        RenderAssetUsages::RENDER_WORLD, // CPU copy freed after upload
    );
    image.texture_descriptor.mip_level_count = levels;
    image.data = Some(data);
    image.sampler = sampler;
    Ok(image)
}
```

Then in the startup system (`world.rs::spawn_world` already has `ResMut<Assets<StandardMaterial>>`;
add `mut images: ResMut<Assets<Image>>`):

```rust
let base: Handle<Image> = images.add(bitmap_to_image(&decoded, tiling_sampler(levels))?);
```

Keep one `Handle<Image>` per bitmap tag in a `HashMap<u32 /*tag index*/, Handle<Image>>` so the
sixty-odd shaders of a map that share `shared.map` bitmaps upload each texture once.

Cube maps (sky, reflection probes), for later: stack the six faces vertically in one 2D image
(`height = 6 * size`), call `image.reinterpret_stacked_2d_as_array(6)` and set
`image.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() })`
(`image.rs:1416-1441`; IW4L `materialization.rs:285-313`). With mips, `data_order`
`LayerMajor` means face 0 all mips, then face 1 all mips (the DDS order).

---

## 7. Applying it to `StandardMaterial` (`bevy_pbr-0.20.0\src\pbr_material.rs`)

### 7.1 The fields that matter

| field | type / default | notes (file:line) |
|---|---|---|
| `base_color` | `Color`, `WHITE` | multiplied with the texture: leave white to show the texture as is (`:35-58`) |
| `base_color_texture` | `Option<Handle<Image>>`, `None` | texture binding 1, sampler 2; sampled with `base_color_channel` (`UvChannel::Uv0` default) |
| `uv_transform` | `Affine2`, `IDENTITY` | applied to **both** UV channels and to **every** texture of the material: `uv = (uv_transform * vec3(in.uv, 1.0)).xy`, `uv_b` likewise (`render\pbr_fragment.wesl:123-135`) |
| `normal_map_texture` | `Option<Handle<Image>>` | binding 9/10; needs vertex UVs, normals **and tangents** (`:378-396`, section 7.3) |
| `flip_normal_map_y` | `bool`, `false` | "Normal map textures authored for DirectX have their y-component flipped. Set this to flip it" (`:419-421`). Halo 3 is a D3D/Xbox title, so expect `true`; confirm visually on a known bump (bumps should look raised, not dented) |
| `alpha_mode` | `AlphaMode`, `Opaque` | "How to apply the alpha channel of the base_color_texture" (`:682-685`), section 7.2 |
| `double_sided` | `bool`, `false` | only flips the lighting normal on back faces; does **not** disable culling (`:644-650`) |
| `cull_mode` | `Option<Face>`, `Some(Face::Back)` | `None` draws both sides; front face = counter-clockwise (`:652-671`), consistent with `docs\format\sbsp-render-geometry.md` "no flip" rule |
| `unlit` | `bool`, `false` | ignores lighting, shadows, alpha mode... use only for debugging (`:673-677`) |
| `perceptual_roughness`, `metallic`, `reflectance` | `0.5`, `0.0`, `0.5` | world.rs already overrides these; with real textures 0.9 / 0.0 / 0.2 still reads better than bevy's plastic defaults |
| `emissive`, `emissive_texture` | | for Halo self-illumination maps, later |

Minimal textured material for an opaque `rmsh`:

```rust
let material = StandardMaterial {
    base_color: Color::WHITE,
    base_color_texture: Some(base.clone()),
    normal_map_texture: bump.clone(),   // Option<Handle<Image>>; None is fine
    flip_normal_map_y: true,            // D3D convention; verify
    uv_transform: tiling,               // section 7.4
    perceptual_roughness: 0.9,
    metallic: 0.0,
    reflectance: 0.2,
    cull_mode: if two_sided { None } else { Some(Face::Back) },
    double_sided: two_sided,
    alpha_mode,
    ..default()
};
```

### 7.2 `AlphaMode` (`bevy_material-0.20.0\src\alpha.rs:9-62`)

| variant | effect | Halo use |
|---|---|---|
| `Opaque` | alpha forced to 1 | `rmsh`, `rmtr`, most things |
| `Mask(f32)` | alpha below the cutoff -> discarded, else opaque; still in the opaque pass, writes depth, casts shadows | `rmfl` foliage and other alpha-tested shaders (our `Shading::AlphaTest`): `Mask(0.5)` to start, Halo's reference is usually around there |
| `AlphaToCoverage` | hardware alpha-to-coverage; needs MSAA on the camera, otherwise identical to `Mask(0.5)` | nicer edges on foliage if the camera keeps `Msaa::Sample4` |
| `Blend` | standard alpha blending in the transparent pass, sorted per **entity** by distance, no depth write | `rmw ` water, `rmct`, glass, `rmd ` decals (`Shading::Translucent`); because sorting is per entity, do not merge all translucent parts of a material into one giant batch, keep one entity per placement or per cluster |
| `Premultiplied`, `Add`, `Multiply` | blend variants | `Add` is right for Halo "additive" shaders (plasma, glows) |

`alpha_cutoff` defaults to 0.5 when the mode is not `Mask` (`pbr_material.rs:1190-1196`). The
alpha that is tested or blended is `base_color.alpha * base_color_texture.a`, so a `Bc1`
texture (1-bit alpha) works for cut-outs and `Bc3` for soft ones.

### 7.3 Normal maps need tangents (and how to get them)

- The mesh pipeline adds the `VERTEX_TANGENTS` shader define only if the mesh layout contains
  `Mesh::ATTRIBUTE_TANGENT` (`bevy_pbr-0.20.0\src\render\mesh.rs:3453-3462`, shader location 4,
  format `Float32x4`: tangent xyz plus handedness sign in w).
- The fragment shader samples the normal map only under
  `VERTEX_UVS && VERTEX_TANGENTS && STANDARD_MATERIAL_NORMAL_MAP` (`render\pbr_fragment.wesl:556-583`).
  **Without tangents a normal-mapped material silently renders flat; there is no warning.**
- Two ways to get tangents:
  1. `Mesh::generate_tangents()` / `with_generated_tangents()` (mikktspace) need a
     `TriangleList`, `ATTRIBUTE_POSITION` `Float32x3`, `ATTRIBUTE_NORMAL` `Float32x3`,
     `ATTRIBUTE_UV_0` `Float32x2`; indices are optional (`bevy_mesh-0.20.0\src\mesh.rs:2170-2191`,
     `mikktspace.rs:80-137`). They are behind the cargo feature **`bevy_mikktspace`**
     (`bevy_mesh\Cargo.toml` optional dependency; bevy feature `bevy_mikktspace`), which our
     workspace `Cargo.toml` does **not** enable: add `"bevy_mikktspace"` to the bevy feature
     list, or
  2. use Halo's own tangents: `s_world_vertex` and `s_rigid_vertex` carry `tangent` and
     `binormal` as `Int16_N4` (`docs\format\sbsp-render-geometry.md` section 5; `blam_geometry`
     does not decode them yet). Insert them as
     `[t.x, t.y, t.z, w]` with `w = if n.cross(t).dot(b) < 0.0 { -1.0 } else { 1.0 }`, rotated
     into the renderer frame with `to_render_dir` like the normals in `world.rs`. This is cheaper
     than mikktspace on a 125k-triangle map and matches the game's bump maps exactly.

  ```rust
  mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, batch.tangents); // Vec<[f32; 4]>
  // or, with the bevy_mikktspace feature on:
  if let Err(e) = mesh.generate_tangents() { warn!("{name}: no tangents: {e}"); }
  ```

### 7.4 Tiling with `uv_transform` (`Affine2`)

Halo's tiling data is a float4 per shader map: `(scale u, scale v, offset u, offset v)` applied
as `uv * scale + offset` (`docs\iw4l-notes.md` section 8 "xform = [scale x, scale y, offset x,
offset y]"; IW4L shader `at(slot, uv) = uv * x.xy + x.zw`). bevy applies
`uv' = matrix2 * uv + translation`, so:

```rust
use bevy::math::{Affine2, Mat2, Vec2};

fn tiling(scale_u: f32, scale_v: f32, offset_u: f32, offset_v: f32) -> Affine2 {
    Affine2 {
        matrix2: Mat2::from_diagonal(Vec2::new(scale_u, scale_v)),
        translation: Vec2::new(offset_u, offset_v),
    }
    // equivalent: Affine2::from_scale_angle_translation(Vec2::new(scale_u, scale_v), 0.0, Vec2::new(offset_u, offset_v))
}
```

Gotchas: one `uv_transform` per material, applied to every texture and both UV sets
(`pbr_fragment.wesl:131-135`), so if a shader tiles its bump map differently from its base map
only one can be right with `StandardMaterial`; pick the base map's. The composition order
matters (`Affine2 * Affine2` is not commutative; bevy's own `flip()` left-multiplies,
`pbr_material.rs:835-843`). No V flip is needed: Halo (D3D, top-left origin) and wgpu agree,
as the "no flip" rows in `sbsp-render-geometry.md` (lines 786-788) already establish for
winding; HaloExport's glTF passes Halo UVs through unchanged and IW4L renders them without a
flip.

### 7.5 Bindless, for information

`StandardMaterial` is declared `#[bindless(index_table(range(0..31)))]`
(`pbr_material.rs:25`). On a device with `TEXTURE_BINDING_ARRAY` (DX12 and Vulkan desktop have
it) bevy packs up to `AUTO_BINDLESS_SLAB_RESOURCE_LIMIT = 2048` resources per slab on Windows
(`bevy_render-0.20.0\src\render_resource\bindless.rs:32`) and indexes textures and samplers in
the shader (`bindless_textures_2d[...]`, `bindless_samplers_filtering[...]`,
`pbr_fragment.wesl:561-565`). Per-image samplers (Repeat vs Clamp) are honoured in that mode,
and hundreds of distinct textures do not mean hundreds of bind groups. Nothing to configure.

---

## 8. Gotchas

### 8.1 Block-multiple rule for compressed textures

`wgpu-core-30.0.1\src\device\resource.rs:1436-1458`: creating a texture with a compressed
format whose **mip-0 width or height is not a multiple of the block size (4)** fails with
`CreateTextureError::InvalidDimension(NotMultipleOfBlockWidth/Height)`; bevy's render thread
will log/panic on it. Smaller mips are fine: they are addressed by their physical (rounded-up)
size (`command\transfer.rs:509-518`), so a 2x2 or 1x1 BC mip is one block. Halo bitmaps are
almost all powers of two >= 4; the rare 1x1 or 2x2 BC bitmap must be decompressed to RGBA8
(or skipped).

### 8.2 Dimension limits

`Limits::defaults()` has `max_texture_dimension_2d: 8192`, `max_texture_array_layers: 256`
(`wgpu-types-30.0.1\src\limits.rs:424-426`), but bevy asks for `adapter.limits()` (section 2),
so on real GPUs it is 16384. Halo 3's largest bitmaps are 2048 (some 4096 cube map faces);
no issue.

### 8.3 Mip count and lod clamps

`mip_level_count` must be <= `max_mips`; `lod_max_clamp` 32 is harmless. If the resource page
only yields the top mip (or the reader skips the small mips in the primary page), upload what
you have with `mip_level_count = levels` and `mipmap_filter: Nearest`; wgpu clamps sampling to
the last available level.

### 8.4 Data length

Nothing checks `data.len()` for compressed formats before the render thread slices it
(section 3.5). Validate with `bc_chain_len` yourself.

### 8.5 Memory

Per 2048 x 2048 texture with a full mip chain (the chain adds one third):

| format | bits/pixel | mip 0 | full chain |
|---|---|---|---|
| BC1 | 4 | 2.0 MiB | 2.67 MiB |
| BC3 / BC5 / BC7 | 8 | 4.0 MiB | 5.33 MiB |
| RGBA8 (decompressed) | 32 | 16.0 MiB | 21.3 MiB |

300 such textures: about 0.8 GB (BC1), 1.6 GB (BC3), 6.4 GB (RGBA8). That is why uploading
the BCn blocks as they are, not decoded, is the only workable path for whole maps, and why
`RenderAssetUsages::RENDER_WORLD` matters (otherwise the same bytes also sit in `Assets<Image>`
on the CPU for the life of the map). Only upload bitmaps referenced by drawn materials; IW4L
tracks its resident bytes (`materialization.rs:13-16`) and that is worth copying as a debug
line (`info!`) at load time. The upload itself goes through staging memory during the frame
the asset is extracted; consider loading a map's textures over a few frames
(`RenderAssetBytesPerFrame`) if the first frame hitch is bad.

### 8.6 Translucent sorting

`AlphaMode::Blend` entities are sorted back to front per entity. One mesh per translucent
material covering the whole map (today's batching in `world.rs`) will sort wrongly against
other translucent materials. Keep translucent batches per placement/cluster.

### 8.7 Which one is a normal map

Pass the plain `Bc5RgUnorm` (not `Srgb`, there is no sRGB BC5 anyway); bevy flags it as
two-component automatically (section 4). For old DXT5nm-style bump maps (`Bc3RgbaUnorm`, normal
in A and G) bevy has no decoder; those would need a CPU swizzle to RG/BC5 or a custom shader.

---

## 9. Prior art: how IW4L (`asset_material`) uploads MW2's textures

`D:\GAME ISO AND ROMS\PC - AI MASHUP\mw2-skate-3-src\src\mashup\crates\asset_material\src\`:

- **Image build** (`material_images.rs:313-327`, `material_images\materialization.rs:265-283`):
  `Image::new_uninit(Extent3d{w,h,1}, TextureDimension::D2, format, RenderAssetUsages::RENDER_WORLD)`;
  then `texture_descriptor.mip_level_count = levels`; `data = Some(packed_mips)`;
  `sampler = ImageSampler::Descriptor(sampler_from_iw4(..))`. Usage flags are left at bevy's
  defaults.
- **Mip packing** (`material_images\decode.rs:41-62, 90-96`): levels concatenated largest
  first, each `div_ceil(w,4) * div_ceil(h,4) * (8 | 16)` bytes for BC1 / BC2-3-5; RGBA8 levels
  `w * h * 4`. The IWI files store mips smallest first, so IW4L reverses them (`decode.rs:278-299`);
  check which way the Halo page stores them before copying.
- **Format choice** (`materialization.rs:234-247`): `Bc1`/`Bc2`/`Bc3` -> `Srgb` variant for colour,
  plain for linear; `Bc5` -> `Bc5RgUnorm` always; linear = `is_normal || !use_srgb_reads || force_linear`.
- **Sampler** (`material_images.rs:676-724`): `Repeat` on each axis unless the material's
  sampler-state bits say clamp; `Linear` mag/min; `mipmap_filter: Linear` when the state asks for
  trilinear or anisotropy > 1, else `Nearest`; `anisotropy_clamp` 2 or 4 from the filter bits,
  forced to 1 when `levels <= 1` or the texture is alpha-tested (MW2 quirk: alpha-tested
  textures use `Nearest` everywhere).
- **Cube maps** (`materialization.rs:285-313`): six RGBA8 faces stacked, `reinterpret_stacked_2d_as_array(6)`,
  view dimension `Cube`, `ClampToEdge` sampler.
- **Sharing**: one decoded payload per image hash, wrapped once per (format, sampler) recipe so a
  texture used both as sRGB colour and as linear data is uploaded twice at most
  (`materialization.rs:168-231`); resident bytes counted in a static.
- **Fallbacks**: `solid_texture(rgba, srgb)` 1x1 images for missing maps (`material_images.rs:1102`).

---

## 10. Prior art: IW4L's Halo stand-in terrain shader

The Valhalla stand-in does not use bevy's PBR at all; it has its own wgpu pass
(`D:\GAME ISO AND ROMS\PC - AI MASHUP\mw2-skate-3-src\src\mashup-halo3-valhalla\crates\render_gpu\src\drawsurf\halo_world.rs`,
WGSL at lines 430-545; textures there are RGBA8 sRGB from HaloExport PNGs with CPU box-filtered
mips, `docs\iw4l-notes.md` section 8). What its fragment shader does, in words:

- Material uniform: `xforms: array<vec4, 9>` (per slot `uv * xy + zw`), `layers: vec4` (1.0 for
  each enabled terrain layer), `params = (is_terrain, is_alpha_test, opacity, has_detail)`; nine
  `texture_2d` slots and one shared `Repeat` sampler.
- Slot meaning (from `halo3_world`): 0 = base map (terrain: the **blend map**), 1..=4 = the four
  terrain layer base maps, 5..=8 = the four detail maps (non-terrain: 5 = `detail_map`).
- **Terrain** (`params.x > 0.5`): `blend = sample(slot 0, uv1 transformed by xform 0)`;
  `w = blend.rgba * layers`; if `sum(w) > 1e-4` normalise `w /= sum(w)`, else fall back to equal
  weights over the enabled layers; colour = `sum_k w[k] * layer(base_k at uv1 with xform k,
  detail_(k+4) at uv1 with xform k+4)` where `layer(base, detail) = min(base.rgb * detail.rgb * 4.59479, 1)`
  (the constant is Halo 3's detail multiplier: a mid-grey detail map leaves the base alone;
  neutral fallback texture is `[128,128,128,255]`). Alpha is 1.
- **Everything else**: `base = sample(slot 0, uv0)` (no transform on the base map), and if a
  detail map exists `base.rgb = layer(base, sample(slot 5, uv1 with xform 5))`.
- Alpha test: `if params.y > 0.5 && c.a < 0.5 { discard; }` (= `AlphaMode::Mask(0.5)`).
- Translucent pass uses a fixed opacity by material name (water 0.6, glass 0.3, else 0.75)
  because the real alpha is not available from the glTF.
- Lighting is a hemisphere ambient + one sun + distance haze, with the normal flipped toward
  the viewer for back faces of two-sided surfaces.

What this means for us: `StandardMaterial` covers `rmsh`-style shaders (base + bump + optional
alpha), but the terrain blend (four layers weighted by a blend map, each with its own tiling,
times detail maps) needs either a `Material` of our own or an
`ExtendedMaterial<StandardMaterial, TerrainExt>` carrying the blend map, the four layer
textures and nine `vec4` transforms, with a fragment shader that computes the albedo the way
above and then hands it to bevy's PBR functions. The detail-map multiply on ordinary shaders
is the same story in miniature; without it base maps look washed out next to the game. Those
are the next steps after plain base-map texturing works.

---

## 11. Checklist for the first textured build of `world.rs`

1. Add `mut images: ResMut<Assets<Image>>` to `spawn_world`; a `HashMap<bitmap tag index, Handle<Image>>`.
2. For each material, resolve its base map (another reader's chain: rmsh -> rmt2 usage names -> bitmap),
   decode the page, build the `Image` with `bitmap_to_image` + `tiling_sampler`, `RENDER_WORLD`.
3. `StandardMaterial { base_color: WHITE, base_color_texture, uv_transform: tiling(..), alpha_mode, .. }`.
4. Keep `material_colour(name)` as the fallback `base_color` when a material has no base map.
5. Bump maps: decode Halo tangents in `blam_geometry` (or enable `bevy_mikktspace`), insert
   `ATTRIBUTE_TANGENT`, set `normal_map_texture` + `flip_normal_map_y: true`, check one wall.
6. Split translucent batches per placement; add `NotShadowCaster` as today.
7. Log `TEXTURE_COMPRESSION_BC` presence and total uploaded bytes at startup.
