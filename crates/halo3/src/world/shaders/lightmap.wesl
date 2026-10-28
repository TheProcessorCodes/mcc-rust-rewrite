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
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> halo: HaloLightmapParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var coefficients: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var lightmap_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var intensity: texture_2d_array<f32>;

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

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color,
    );
    var colour = pbr_input.material.base_color;

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
