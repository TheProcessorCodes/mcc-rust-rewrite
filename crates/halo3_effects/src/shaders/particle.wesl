// Halo 3's particles and contrails: the `shaders\particle` and
// `shaders\contrail` render methods' albedo options (diffuse, palettized,
// with a billboard or sprite alpha map) and black point, coloured by the
// emitter and particle functions in the vertex colour (`draw.rs`,
// `docs/format/effects.md` 9.4).

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::main_pass_post_lighting_processing,
    forward_io::{VertexOutput, FragmentOutput},
};

struct ParticleParams {
    // x: albedo mode, y: 1 when the alpha map has one channel, z: 1 with
    // the black point on, w: exposure.
    mode: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> particle: ParticleParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var base_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var particle_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var alpha_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var palette_map: texture_2d<f32>;

// Alpha below `black` goes to 0; the rest is stretched back up to 1.
fn black_point(black: f32, alpha: f32) -> f32 {
    let mid = (black + 1.0) * 0.5;
    return mid * saturate((alpha - black) / max(mid - black, 1e-4)) + saturate(alpha - mid);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let pbr_input = pbr_input_from_standard_material(in, is_front);
    var tint = vec4(1.0);
    var corner = vec2(0.5);
    var palette_row = 0.0;
    var black = 0.0;
    var uv = vec2(0.5);
@if(VERTEX_COLORS) {
    tint = in.color;
}
@if(VERTEX_UVS_A) {
    uv = in.uv;
}
@if(VERTEX_UVS_B) {
    let k = floor(in.uv_b * 0.5);
    corner = in.uv_b - 2.0 * k;
    palette_row = k.x / 255.0;
    black = k.y / 255.0;
}
    let mode = u32(particle.mode.x + 0.5);
    let base = textureSample(base_map, particle_sampler, uv);
    let sprite_alpha = textureSample(alpha_map, particle_sampler, uv).a;
    let billboard_alpha = textureSample(alpha_map, particle_sampler, corner).a;
    let pal = textureSample(palette_map, particle_sampler, vec2(base.r, palette_row));
    var c = base;
    if mode == 2u || mode == 3u || mode == 5u {
        // The base map's grey picks the colour along the palette's row.
        c = vec4(pal.rgb, pal.a * base.a);
    }
    if mode == 1u || mode == 3u {
        c = vec4(c.rgb, billboard_alpha);
    } else if mode == 4u || mode == 5u {
        c = vec4(c.rgb, sprite_alpha);
    }
    c = c * tint;
    var a = c.a;
    if particle.mode.z > 0.5 {
        a = black_point(black, a);
    }
    let rgb = c.rgb * particle.mode.w;
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, vec4(rgb, saturate(a)));
    return out;
}
