// Halo 3's baked lighting on top of bevy's StandardMaterial (see
// `lighting.rs` and `docs/format/lightmaps.md` section 5).
//
// The StandardMaterial part gives the albedo: base colour texture with its
// uv_transform tiling, tint, alpha mask / blend. Instead of bevy's lights,
// the albedo is multiplied by Halo's diffuse irradiance:
// - meshes with a second UV set (per-pixel lightmapped) decode it from the
//   BSP's two DXT5 arrays at that texcoord, with the surface normal;
// - every other mesh carries the irradiance per vertex in its vertex colour,
//   computed on the CPU, which StandardMaterial already multiplies in.
// Then exposure, and bevy's own fog and tonemapping.
//
// Terrain (`rmtr`, any `terrain_layers` non-zero) has no StandardMaterial
// texture: its albedo is the four-layer blend computed here (see
// `terrain_albedo`), multiplied into the StandardMaterial colour (tint and,
// on vertex-lit batches, the irradiance).

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
};

struct HaloLightmapParams {
    // Ranges of SH coefficients c0..c3.
    scales: vec4<f32>,
    // x: dominant light intensity range, y: exposure.
    extra: vec4<f32>,
    // Terrain: 1 per layer drawn, 0 per layer off; all 0 on non-terrain.
    terrain_layers: vec4<f32>,
    // Terrain: x: detail multiplier, y: 1 to normalise the blend weights.
    terrain_extra: vec4<f32>,
    // Terrain xforms: scale in xy, offset in zw.
    blend_xform: vec4<f32>,
    base_xforms: array<vec4<f32>, 4>,
    // A zero scale means the layer has no detail map.
    detail_xforms: array<vec4<f32>, 4>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> halo: HaloLightmapParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var coefficients: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var lightmap_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var intensity: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var blend_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var blend_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var base_0: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var tiling_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var base_1: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(109) var base_2: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(110) var base_3: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(111) var detail_0: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(112) var detail_1: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(113) var detail_2: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(114) var detail_3: texture_2d<f32>;

// decode_bpp16_luvw: base and residual layer to a signed RGB value.
fn luvw(a: vec4<f32>, b: vec4<f32>, range: f32) -> vec3<f32> {
    return ((a.rgb + b.rgb) * 2.0 - 2.0) * (a.a * b.a * range);
}

fn layer(t: texture_2d_array<f32>, uv: vec2<f32>, l: i32) -> vec4<f32> {
    return textureSampleLevel(t, lightmap_sampler, uv, l, 0.0);
}

// Diffuse irradiance at `uv` for a Halo-frame (Z up) unit normal.
fn halo_irradiance(uv: vec2<f32>, n: vec3<f32>) -> vec3<f32> {
    let c0 = luvw(layer(coefficients, uv, 0), layer(coefficients, uv, 1), halo.scales.x);
    let c1 = luvw(layer(coefficients, uv, 2), layer(coefficients, uv, 3), halo.scales.y);
    let c2 = luvw(layer(coefficients, uv, 4), layer(coefficients, uv, 5), halo.scales.z);
    let c3 = luvw(layer(coefficients, uv, 6), layer(coefficients, uv, 7), halo.scales.w);
    let d = luvw(layer(intensity, uv, 0), layer(intensity, uv, 1), halo.extra.x);

    let w = vec3(0.212656, 0.715158, 0.0721856);
    let raw_dir = vec3(-dot(c3, w), -dot(c1, w), dot(c2, w));
    let len = length(raw_dir);
    var dir = vec3(0.0);
    if len > 1e-6 {
        dir = raw_dir / len;
    }

    // ravi_order_2_with_dominant_light.
    let e = -0.4886025 * dir;
    let kr = vec3(c3.r, c1.r, -c2.r) - e * d.r;
    let kg = vec3(c3.g, c1.g, -c2.g) - e * d.g;
    let kb = vec3(c3.b, c1.b, -c2.b) - e * d.b;
    let k0 = c0 - d * 0.2820948;
    let x1 = vec3(dot(n, kr), dot(n, kg), dot(n, kb));
    let ambient = (k0 * 0.886227 - x1 * (2.0 * 0.511664)) / 3.14159265;
    let direct = d * max(dot(dir, n), 0.0) * 0.281;
    return max(ambient + direct, vec3(0.0));
}

fn xform(uv: vec2<f32>, x: vec4<f32>) -> vec2<f32> {
    return uv * x.xy + x.zw;
}

// One terrain layer's colour: its base map, times its detail map scaled so
// that the detail's mid grey leaves the base as it is. Both textures are
// sampled as sRGB, so the product is in linear light.
fn terrain_layer(base: texture_2d<f32>, detail: texture_2d<f32>, uv: vec2<f32>, k: i32) -> vec3<f32> {
    let c = textureSample(base, tiling_sampler, xform(uv, halo.base_xforms[k])).rgb;
    let d = halo.detail_xforms[k];
    // Sampled either way, keeping control flow uniform.
    let dc = textureSample(detail, tiling_sampler, xform(uv, d)).rgb * halo.terrain_extra.x;
    return c * select(vec3(1.0), dc, d.x != 0.0);
}

// The four-layer terrain blend (`docs/format/shaders.md` 7.5): the blend
// map's red, green, blue and alpha weight layers 0..3; layers that are off
// get no weight; the weights are normalised to sum to one.
fn terrain_albedo(uv: vec2<f32>) -> vec3<f32> {
    var w = textureSample(blend_map, blend_sampler, xform(uv, halo.blend_xform)) * halo.terrain_layers;
    if halo.terrain_extra.y != 0.0 {
        let sum = w.x + w.y + w.z + w.w;
        if sum > 1e-4 {
            w = w / sum;
        } else {
            w = halo.terrain_layers / max(dot(halo.terrain_layers, vec4(1.0)), 1.0);
        }
    }
    // Every layer is sampled (uniform control flow for the derivatives);
    // the weights pick.
    return terrain_layer(base_0, detail_0, uv, 0) * w.x
        + terrain_layer(base_1, detail_1, uv, 1) * w.y
        + terrain_layer(base_2, detail_2, uv, 2) * w.z
        + terrain_layer(base_3, detail_3, uv, 3) * w.w;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color,
    );
    var colour = pbr_input.material.base_color;

@if(VERTEX_UVS_A) {
    if dot(halo.terrain_layers, vec4(1.0)) > 0.0 {
        colour = vec4(colour.rgb * terrain_albedo(in.uv), colour.a);
    }
}

@if(VERTEX_UVS_B) {
    // Renderer frame (Y up) to Halo frame (Z up): (x, -z, y).
    let n = pbr_input.N;
    let e = halo_irradiance(in.uv_b, normalize(vec3(n.x, -n.z, n.y)));
    colour = vec4(colour.rgb * e, colour.a);
}

    colour = vec4(colour.rgb * halo.extra.y, colour.a);
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, colour);
    return out;
}
