// Halo 3's sky: the scenario's sky model drawn with its shaders' albedo
// combinations, and coloured by the map's atmosphere (see `sky.rs` and
// `docs/format/sky-and-atmosphere.md`).
//
// Every sky surface gets
//     colour = surface * extinction + inscatter
// along the ray from the camera, with single scattering of the sun's light
// by molecules (Rayleigh, beta_m) and haze (Mie, beta_p) whose densities
// fall off exponentially with height above sea level. The near-black sky
// dome turns blue that way, as in the game.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::main_pass_post_lighting_processing,
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
};

struct SkyParams {
    // Texture transforms: scale.xy, offset.zw.
    xform_base: vec4<f32>,
    xform_detail: vec4<f32>,
    xform_detail2: vec4<f32>,
    xform_detail3: vec4<f32>,
    xform_illum: vec4<f32>,
    albedo_colour: vec4<f32>,
    // Self-illumination colour times intensity; w: 0 none, 1 from the
    // illum map, 2 from the albedo (`from_diffuse`), 3 plasma (the illum
    // slot holds the alpha mask), 4 plasma times the overlay in detail3.
    illum_colour: vec4<f32>,
    // x: albedo mode, y: 1 for additive blending (no inscatter),
    // z: 1 when lit (diffuse), w: exposure.
    mode: vec4<f32>,
    // Diffuse light for lit sky surfaces (already exposed).
    light: vec4<f32>,
    // Toward the sun (Halo frame); w: Mie asymmetry g.
    sun_dir: vec4<f32>,
    // Sun colour times intensity.
    sun_colour: vec4<f32>,
    beta_m: vec4<f32>,
    beta_p: vec4<f32>,
    beta_m_theta: vec4<f32>,
    beta_p_theta: vec4<f32>,
    // x: sea level, y: Rayleigh height, z: Mie height, w: distance bias.
    heights: vec4<f32>,
    // x: max fog thickness, y: atmosphere on (1) or off (0),
    // z: which of detail, detail2, detail3 are present (bits 0, 1, 2).
    extra: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> sky: SkyParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var base_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var sky_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var detail_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var detail_map2: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var detail_map3: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var illum_map: texture_2d<f32>;

// Detail maps are authored around mid grey: 0.5 in gamma space is 1 here.
const DETAIL: f32 = 4.59479;
const WORLD_UNIT: f32 = 3.048;

fn tex(t: texture_2d<f32>, xf: vec4<f32>, uv: vec2<f32>) -> vec4<f32> {
    return textureSample(t, sky_sampler, uv * xf.xy + xf.zw);
}

// Alpha below `black` goes to 0; the rest is stretched back up to 1.
fn black_point(black: f32, alpha: f32) -> f32 {
    let mid = (black + 1.0) * 0.5;
    return mid * saturate((alpha - black) / max(mid - black, 1e-4)) + saturate(alpha - mid);
}

// A detail map, or the neutral grey Halo's default detail bitmap gives
// when the shader leaves the slot empty.
fn detail(t: texture_2d<f32>, xf: vec4<f32>, uv: vec2<f32>, bit: u32) -> vec4<f32> {
    let v = tex(t, xf, uv);
    if (u32(sky.extra.z + 0.5) & bit) == 0u {
        return vec4(vec3(1.0 / DETAIL), 1.0);
    }
    return v;
}

fn albedo(uv: vec2<f32>) -> vec4<f32> {
    let mode = u32(sky.mode.x + 0.5);
    let base = tex(base_map, sky.xform_base, uv);
    let d1 = detail(detail_map, sky.xform_detail, uv, 1u);
    let d2 = detail(detail_map2, sky.xform_detail2, uv, 2u);
    let d3 = tex(detail_map3, sky.xform_detail3, uv);
    var out = vec4(1.0);
    switch mode {
        // constant_color
        case 0u: {
            out = vec4(1.0);
        }
        // default: base times detail.
        case 1u: {
            out = vec4(base.rgb * d1.rgb * DETAIL, base.a * d1.a);
        }
        // detail_blend: base alpha picks between the two details.
        case 2u: {
            let d = mix(d1, d2, base.a);
            out = vec4(base.rgb * d.rgb * DETAIL, d.a);
        }
        // three_detail_blend: base alpha runs detail -> detail2 -> detail3.
        case 3u: {
            let a = mix(d1, d2, saturate(base.a * 2.0));
            let d = mix(a, d3, saturate(base.a * 2.0 - 1.0));
            out = vec4(base.rgb * d.rgb * DETAIL, d.a);
        }
        // two_detail_overlay: detail_blend, then the overlay (detail3 slot)
        // laid over by its own alpha.
        case 4u: {
            let d = mix(d1, d2, base.a);
            let c = base.rgb * d.rgb * DETAIL;
            out = vec4(mix(c, d3.rgb, d3.a), d.a);
        }
        // two_detail: base times both details.
        case 5u: {
            out = vec4(base.rgb * d1.rgb * d2.rgb * DETAIL * DETAIL, base.a * d1.a * d2.a);
        }
        // two_detail_black_point: as two_detail, with the base alpha as the
        // black point of the details' alpha.
        case 6u: {
            out = vec4(base.rgb * d1.rgb * d2.rgb * DETAIL * DETAIL, black_point(base.a, d1.a * d2.a));
        }
        default: {
            out = base;
        }
    }
    return out * sky.albedo_colour;
}

// Mean density between heights h0 and h1 for a scale height `scale`.
fn mean_density(h0: f32, h1: f32, scale: f32) -> f32 {
    let s = max(scale, 1e-3);
    let dh = h1 - h0;
    if abs(dh) < 1e-2 {
        return exp(-h0 / s);
    }
    return s * (exp(-h0 / s) - exp(-h1 / s)) / dh;
}

struct Scatter {
    extinction: vec3<f32>,
    inscatter: vec3<f32>,
}

// Single scattering along the ray from `eye` to `p` (Halo frame, world units).
fn scatter(eye: vec3<f32>, p: vec3<f32>) -> Scatter {
    let v = p - eye;
    let d = length(v);
    let dir = v / max(d, 1e-4);
    let dist = clamp(d + sky.heights.w, 0.0, sky.extra.x);
    // Heights above sea level, the far end shortened with the distance.
    // No thicker than at sea level below it (Sandtrap plays 20 wu under).
    let h0 = max(eye.z - sky.heights.x, 0.0);
    let h1 = max(h0 + dir.z * dist, 0.0);
    let dm = dist * mean_density(h0, h1, sky.heights.y);
    let dp = dist * mean_density(h0, h1, sky.heights.z);
    let tau = sky.beta_m.rgb * dm + sky.beta_p.rgb * dp;
    let extinction = exp(-tau);

    let mu = dot(dir, sky.sun_dir.xyz);
    let g = sky.sun_dir.w;
    let rayleigh = sky.beta_m_theta.rgb * (1.0 + mu * mu);
    let mie = sky.beta_p_theta.rgb * (1.0 - g) * (1.0 - g) / pow(max(1.0 + g * g - 2.0 * g * mu, 1e-4), 1.5);
    let ratio = (rayleigh * dm + mie * dp) / max(tau, vec3(1e-6));
    var s: Scatter;
    s.extinction = extinction;
    s.inscatter = ratio * sky.sun_colour.rgb * (1.0 - extinction);
    return s;
}

fn to_halo(r: vec3<f32>) -> vec3<f32> {
    return vec3(r.x, -r.z, r.y) / WORLD_UNIT;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let pbr_input = pbr_input_from_standard_material(in, is_front);
    let exposure = sky.mode.w;
    let a = albedo(in.uv);
    var rgb = a.rgb;
    if sky.mode.z > 0.5 {
        rgb = rgb * sky.light.rgb;
    }
    let illum_mode = u32(sky.illum_colour.w + 0.5);
    if illum_mode == 1u {
        rgb = rgb + tex(illum_map, sky.xform_illum, in.uv).rgb * sky.illum_colour.rgb * exposure;
    } else if illum_mode == 2u {
        rgb = rgb + a.rgb * sky.illum_colour.rgb * exposure;
    } else if illum_mode >= 3u {
        // Plasma: the mask's alpha (its colour channels are black), through
        // the multiply overlay when there is one.
        var glow = tex(illum_map, sky.xform_illum, in.uv).a * sky.illum_colour.rgb;
        if illum_mode == 4u {
            glow = glow * tex(detail_map3, sky.xform_detail3, in.uv).rgb;
        }
        rgb = rgb + glow * exposure;
    }
    if sky.extra.y > 0.5 {
        let s = scatter(to_halo(view.world_position), to_halo(in.world_position.xyz));
        rgb = rgb * s.extinction;
        // Added light would pile up under every additive layer.
        if sky.mode.y < 0.5 {
            rgb = rgb + s.inscatter * exposure;
        }
    }
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, vec4(rgb, a.a));
    return out;
}
