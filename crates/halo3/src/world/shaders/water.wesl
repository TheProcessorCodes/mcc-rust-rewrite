// Halo 3 water and flowing waterfalls (see `water.rs` and
// `docs/format/water.md`).
//
// Water mode:
//   normal   from the shader's wave slope array: two samples (main and
//            detail), each between two of the 16 animated slices picked by
//            `time_warp`, scrolled by the shader's functions, in world space;
//   colour   = (1 - F) * (1 - T) * watercolor * irradiance   (murk, lit)
//            + F * sky along the reflected ray + sun glint     (reflection)
//            over the bed seen through the water with weight (1 - F) * T,
//            T = exp(-murkiness * path length in the water), the path taken
//            from the depth prepass; the banks fade out over
//            `bankalpha_infuence_depth`; foam laid over where painted
//            (shape map blue) and on steep waves.
// Flow mode: `two_detail` albedo (base * detail * detail2) with every map's
// xform offset moving, lit by the lightmap, blended by its alpha.

import bevy_pbr::{
    render::{
        pbr_fragment::pbr_input_from_standard_material,
        pbr_functions::main_pass_post_lighting_processing,
        forward_io::{VertexOutput, FragmentOutput},
        mesh_view_bindings::{view, globals, fog},
        view_transformations::depth_ndc_to_view_z,
    },
    prepass::utils as prepass_utils,
};

// Halo's fog starts `distance bias` world units from the eye (`skya`
// setting +0x3C; -100 wu on 120_halo's Warthog run): bevy's fog has no
// start, so `sky.rs` passes the start in metres in the fog's unused
// directional-light exponent, and the fragment is moved that much closer
// for the fog only.
fn fog_position(world: vec4<f32>) -> vec4<f32> {
    let start = fog.directional_light_exponent;
    if start <= 0.0 {
        return world;
    }
    let v = world.xyz - view.world_position;
    let d = length(v);
    return vec4(view.world_position + v * (max(d - start, 0.0) / max(d, 1e-4)), world.w);
}

struct WaterParams {
    params0: vec4<f32>,
    params1: vec4<f32>,
    params2: vec4<f32>,
    params3: vec4<f32>,
    colour: vec4<f32>,
    slope_xform: vec4<f32>,
    slope_scroll: vec4<f32>,
    xform_a: vec4<f32>,
    scroll_a: vec4<f32>,
    xform_b: vec4<f32>,
    scroll_b: vec4<f32>,
    xform_c: vec4<f32>,
    scroll_c: vec4<f32>,
    xform_d: vec4<f32>,
    scroll_d: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_colour: vec4<f32>,
    beta_m: vec4<f32>,
    beta_p: vec4<f32>,
    beta_m_theta: vec4<f32>,
    beta_p_theta: vec4<f32>,
    heights: vec4<f32>,
    atmosphere_extra: vec4<f32>,
    lightmap_scales: vec4<f32>,
    lightmap_extra: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> water: WaterParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var slope_array: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var slope_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var map_a: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var map_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var map_b: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var map_c: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var coefficients: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var lightmap_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(109) var intensity: texture_2d_array<f32>;

const WORLD_UNIT: f32 = 3.048;
const DETAIL: f32 = 4.59479;

fn has(bit: u32) -> bool {
    return (u32(water.params3.w + 0.5) & bit) != 0u;
}

fn to_halo(r: vec3<f32>) -> vec3<f32> {
    return vec3(r.x, -r.z, r.y) / WORLD_UNIT;
}

fn to_render_dir(h: vec3<f32>) -> vec3<f32> {
    return vec3(h.x, h.z, -h.y);
}

// The scrolling offset wrapped to one tile, so long runs keep precision.
fn moved(uv: vec2<f32>, x: vec4<f32>, scroll: vec4<f32>) -> vec2<f32> {
    return uv * x.xy + x.zw + fract(scroll.xy * globals.time);
}

// ---- Halo's lightmap (as in lightmap.wesl)

fn luvw(a: vec4<f32>, b: vec4<f32>, range: f32) -> vec3<f32> {
    return ((a.rgb + b.rgb) * 2.0 - 2.0) * (a.a * b.a * range);
}

fn lm(t: texture_2d_array<f32>, uv: vec2<f32>, l: i32) -> vec4<f32> {
    return textureSampleLevel(t, lightmap_sampler, uv, l, 0.0);
}

fn halo_irradiance(uv: vec2<f32>, n: vec3<f32>) -> vec3<f32> {
    let s = water.lightmap_scales;
    let c0 = luvw(lm(coefficients, uv, 0), lm(coefficients, uv, 1), s.x);
    let c1 = luvw(lm(coefficients, uv, 2), lm(coefficients, uv, 3), s.y);
    let c2 = luvw(lm(coefficients, uv, 4), lm(coefficients, uv, 5), s.z);
    let c3 = luvw(lm(coefficients, uv, 6), lm(coefficients, uv, 7), s.w);
    let d = luvw(lm(intensity, uv, 0), lm(intensity, uv, 1), water.lightmap_extra.x);
    let w = vec3(0.212656, 0.715158, 0.0721856);
    let raw_dir = vec3(-dot(c3, w), -dot(c1, w), dot(c2, w));
    let len = length(raw_dir);
    var dir = vec3(0.0);
    if len > 1e-6 {
        dir = raw_dir / len;
    }
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

// Diffuse light at this fragment: the lightmap when there is one, else the
// vertex colour (irradiance baked per vertex), else a plain sunny day.
fn irradiance(in: VertexOutput, n_halo: vec3<f32>) -> vec3<f32> {
    var e = vec3(1.0);
@if(VERTEX_COLORS) {
    e = in.color.rgb;
}
@if(VERTEX_UVS_B) {
    if has(64u) {
        e = halo_irradiance(in.uv_b, n_halo);
    }
}
    return e;
}

// ---- the sky along a ray (as in sky.wesl)

fn mean_density(h0: f32, h1: f32, scale: f32) -> f32 {
    let s = max(scale, 1e-3);
    let dh = h1 - h0;
    if abs(dh) < 1e-2 {
        return exp(-h0 / s);
    }
    return s * (exp(-h0 / s) - exp(-h1 / s)) / dh;
}

// The light scattered toward the eye along `dir` (Halo frame) from `eye`
// out through the whole atmosphere: what a mirror shows of the sky.
fn sky_along(eye: vec3<f32>, dir: vec3<f32>) -> vec3<f32> {
    if !has(128u) {
        return vec3(0.35, 0.45, 0.6);
    }
    let dist = water.atmosphere_extra.x;
    let h0 = eye.z - water.heights.x;
    let h1 = h0 + dir.z * dist;
    let dm = dist * mean_density(h0, h1, water.heights.y);
    let dp = dist * mean_density(h0, h1, water.heights.z);
    let tau = water.beta_m.rgb * dm + water.beta_p.rgb * dp;
    let mu = dot(dir, water.sun_dir.xyz);
    let g = water.sun_dir.w;
    let rayleigh = water.beta_m_theta.rgb * (1.0 + mu * mu);
    let mie = water.beta_p_theta.rgb * (1.0 - g) * (1.0 - g) / pow(max(1.0 + g * g - 2.0 * g * mu, 1e-4), 1.5);
    let ratio = (rayleigh * dm + mie * dp) / max(tau, vec3(1e-6));
    return ratio * water.sun_colour.rgb * (1.0 - exp(-tau));
}

// ---- waves

// One slope sample: between the two slices `warp` (0..1 over the cycle)
// falls on, as a slope (dz/dx, dz/dy).
fn slope_at(uv: vec2<f32>, warp: f32) -> vec2<f32> {
    let n = max(water.params3.z, 1.0);
    let f = warp * n;
    let i0 = i32(floor(f)) % i32(n);
    let i1 = (i0 + 1) % i32(n);
    let a = textureSample(slope_array, slope_sampler, uv, i0).xy;
    let b = textureSample(slope_array, slope_sampler, uv, i1).xy;
    return mix(a, b, fract(f));
}

fn warp(period: f32) -> f32 {
    if period <= 1e-4 {
        return 0.0;
    }
    return fract(globals.time / period);
}

struct Waves {
    // Halo-frame unit normal.
    normal: vec3<f32>,
    // How steep the waves are here (for foam).
    steepness: f32,
}

fn waves(halo_pos: vec3<f32>, distance: f32, shape: f32) -> Waves {
    var w: Waves;
    w.normal = vec3(0.0, 0.0, 1.0);
    w.steepness = 0.0;
    if !has(1u) {
        return w;
    }
    let uv = moved(halo_pos.xy, water.slope_xform, water.slope_scroll);
    let main = slope_at(uv, warp(water.params3.x)) * water.params1.xy;
    let detail_uv = uv * water.slope_scroll.z;
    let detail = slope_at(detail_uv, warp(water.params3.y)) * water.params1.xy * water.params1.z;
    // Waves calm down with distance (Halo's wave_visual_damping_distance),
    // which also keeps the far water from shimmering.
    let damp = 1.0 / (1.0 + max(distance - water.params1.w, 0.0) / max(water.params1.w * 4.0, 1.0));
    let s = (main + detail) * damp * shape;
    w.normal = normalize(vec3(-s.x, -s.y, 1.0));
    w.steepness = length(main + detail) * shape;
    return w;
}

// ---- flow mode

fn flow(in: VertexOutput, is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let base = textureSample(map_a, map_sampler, moved(in.uv, water.xform_a, water.scroll_a));
    var d1 = vec4(vec3(1.0 / DETAIL), 1.0);
    var d2 = vec4(vec3(1.0 / DETAIL), 1.0);
    let s1 = textureSample(map_b, map_sampler, moved(in.uv, water.xform_b, water.scroll_b));
    let s2 = textureSample(map_c, map_sampler, moved(in.uv, water.xform_c, water.scroll_c));
    if has(4u) {
        d1 = s1;
    }
    if has(8u) {
        d2 = s2;
    }
    let albedo = base.rgb * d1.rgb * d2.rgb * DETAIL * DETAIL;
    let alpha = saturate(base.a * d1.a * d2.a);
    let n = pbr_input.N;
    let e = irradiance(in, normalize(vec3(n.x, -n.z, n.y)));
    var out: FragmentOutput;
    pbr_input.world_position = fog_position(pbr_input.world_position);
    out.color = main_pass_post_lighting_processing(pbr_input, vec4(albedo * e * water.params0.w, alpha));
    return out;
}

// ---- water mode

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    if water.params2.w > 0.5 {
        return flow(in, is_front);
    }
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let exposure = water.params0.w;
    let eye = to_halo(view.world_position);
    let p = to_halo(in.world_position.xyz);
    let to_eye = eye - p;
    let distance = length(to_eye);
    let v = to_eye / max(distance, 1e-4);

    // Painted masks on the mesh's own UV: green shapes the waves, blue
    // paints foam.
    var shape = vec4(0.0, 1.0, 0.0, 0.0);
    let shape_sample = textureSample(map_b, map_sampler, in.uv * water.xform_b.xy + water.xform_b.zw);
    if has(4u) {
        shape = shape_sample;
    }
    let wave_shape = mix(0.35, 1.0, saturate(shape.g));
    let w = waves(p, distance, wave_shape);
    var n = w.normal;
    // Seen from below (rare): mirror the normal.
    if v.z < 0.0 {
        n = vec3(n.xy, -n.z);
    }

    // Fresnel (Schlick) with Halo's fresnel_coefficient as F0.
    let f0 = water.params0.y;
    let ndv = saturate(dot(n, v));
    let fresnel = f0 + (1.0 - f0) * pow(1.0 - ndv, 5.0);

    // Reflection: the sky along the mirrored ray, and the sun's glint.
    var r = reflect(-v, n);
    r = vec3(r.xy, abs(r.z));
    var reflection = sky_along(eye, r) * exposure;
    if has(128u) {
        let sun = pow(saturate(dot(r, water.sun_dir.xyz)), 600.0);
        reflection = reflection + water.sun_colour.rgb * sun * exposure * 0.6;
    }

    // Lit water colour (the murk).
    let flat_n = vec3(0.0, 0.0, 1.0);
    let light = irradiance(in, flat_n);
    var colour = water.colour.rgb;
    let colour_sample = textureSample(map_c, map_sampler, in.uv * water.xform_c.xy + water.xform_c.zw);
    if has(8u) {
        colour = colour_sample.rgb;
    }
    let murk_colour = colour * water.params2.z * light * exposure;

    // How much of the bed shows: the path through the water to the opaque
    // surface behind, from the depth prepass.
    var transmit = 0.35;
    var depth_wu = 10.0;
@if(DEPTH_PREPASS) {
    let behind = depth_ndc_to_view_z(prepass_utils::prepass_depth(in.position, 0u));
    let here = depth_ndc_to_view_z(in.position.z);
    // View z is negative ahead; the path length scales with the ray.
    let along = max(here - behind, 0.0) / max(abs(here), 1e-4) * distance;
    depth_wu = along * max(v.z, 0.05);
    transmit = exp(-along * water.params0.x * 0.5);
}

    // Foam: painted (blue) and on steep waves.
    var foam = 0.0;
    let foam_a = textureSample(map_a, map_sampler, moved(p.xy, water.xform_a, water.scroll_a));
    let foam_d = textureSample(map_a, map_sampler, moved(p.xy, water.xform_d, water.scroll_d));
    if has(2u) {
        let foam_tex = saturate(dot(foam_a.rgb * foam_d.rgb, vec3(0.333)) * 2.2);
        var mask = 0.0;
        if has(16u) {
            mask = mask + shape.b;
        }
        if has(32u) {
            let h = water.params2.x;
            mask = mask + pow(saturate((w.steepness - h) / max(1.0 - h, 1e-3)), water.params2.y);
        }
        foam = saturate(mask * foam_tex * 1.5);
    }
    let foam_colour = vec3(0.85, 0.9, 0.95) * light * exposure;

    // Straight-alpha result over the bed.
    let opaque = 1.0 - (1.0 - fresnel) * transmit;
    var premul = (1.0 - fresnel) * (1.0 - transmit) * murk_colour + fresnel * reflection;
    premul = mix(premul, foam_colour, foam);
    var alpha = mix(opaque, 1.0, foam);
    // Banks: fade out where the water is shallow.
    if water.params0.z > 0.0 {
        let bank = saturate(depth_wu / water.params0.z);
        premul = premul * bank;
        alpha = alpha * bank;
    }
    let rgb = premul / max(alpha, 1e-3);
    var out: FragmentOutput;
    pbr_input.world_position = fog_position(pbr_input.world_position);
    out.color = main_pass_post_lighting_processing(pbr_input, vec4(rgb, saturate(alpha)));
    return out;
}
