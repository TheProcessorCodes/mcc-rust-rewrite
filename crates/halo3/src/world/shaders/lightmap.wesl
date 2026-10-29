// Halo 3's baked lighting on top of bevy's StandardMaterial (see
// `lighting.rs` and `docs/format/lightmaps.md` section 5).
//
// The StandardMaterial part gives the albedo: base colour texture with its
// uv_transform tiling, tint, alpha mask / blend. Instead of bevy's lights,
// the albedo is multiplied by Halo's diffuse irradiance:
// - meshes with a second UV set (per-pixel lightmapped) decode it from the
//   BSP's two DXT5 arrays at that texcoord, with the (bumped) normal, and
//   add a specular lobe and a reflection from the same light;
// - every other mesh carries the irradiance per vertex in its vertex colour,
//   computed on the CPU, which StandardMaterial already multiplies in.
// Then self-illumination, exposure, and bevy's own fog and tonemapping.
//
// Terrain (`rmtr`, any `terrain_layers` non-zero) has no StandardMaterial
// texture: its albedo is the four-layer blend computed here (see
// `terrain_albedo`), multiplied into the StandardMaterial colour (tint and,
// on vertex-lit batches, the irradiance).
//
// The material model (`docs/format/shaders.md` section 11) is an
// approximation of Halo's: the dominant light of the lightmap is the one
// analytic light; the SH towards the reflection stands in for the area
// specular and for dynamic cube maps.
//
// The same file is the material's prepass fragment shader (`PREPASS_PIPELINE`,
// the depth prepass maps with water get): there it only cuts cut-outs whose
// shader has its own `alpha_test_map`, which bevy's prepass cannot know about.

@if(!PREPASS_PIPELINE)
import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, fog},
};
@if(PREPASS_PIPELINE)
import bevy_pbr::{prepass::io::VertexOutput, render::pbr_prepass_functions::prepass_sample_color_and_alpha_discard};
@if(PREPASS_PIPELINE && PREPASS_FRAGMENT)
import bevy_pbr::prepass::io::FragmentOutput;
@if(PREPASS_PIPELINE && MOTION_VECTOR_PREPASS)
import bevy_pbr::render::pbr_prepass_functions::calculate_motion_vector;
@if(PREPASS_PIPELINE && !BINDLESS)
import bevy_pbr::render::pbr_bindings::material;

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
    // x: bump map on, y: bump detail coefficient (0: none), z: albedo detail
    // map on, w: specular mask (0 none, 1 base alpha, 2 mask texture).
    surface: vec4<f32>,
    // 1 where the map is BC5 signed (-1..1), 0 where it is unorm (0..1):
    // x bump, y bump detail.
    signed: vec4<f32>,
    bump_xform: vec4<f32>,
    bump_detail_xform: vec4<f32>,
    detail_xform: vec4<f32>,
    mask_xform: vec4<f32>,
    illum_xform: vec4<f32>,
    illum_detail_xform: vec4<f32>,
    // x: specular coefficient, y: normal power, z: glancing power,
    // w: fresnel curve steepness.
    spec: vec4<f32>,
    // Normal tint, w: analytical contribution.
    tint_normal: vec4<f32>,
    // Glancing tint, w: area contribution.
    tint_glancing: vec4<f32>,
    // F0, w: model (0 diffuse, 1 phong, 2 cook-torrance, 3 glass).
    f0: vec4<f32>,
    // Reflection colour (tint times contribution).
    env: vec4<f32>,
    // x: self-illumination mode (0 off, 1 map, 2 map x detail,
    // 3 from diffuse, 4 map x diffuse, 5 three channels, 6 plasma: map alpha);
    // y: plasma only, 1 when the mask is read from red instead of alpha.
    illum: vec4<f32>,
    illum_a: vec4<f32>,
    illum_b: vec4<f32>,
    illum_c: vec4<f32>,
    // Terrain: per layer 1 with a bump map; signedness per layer.
    terrain_bump: vec4<f32>,
    terrain_bump_signed: vec4<f32>,
    terrain_bump_xforms: array<vec4<f32>, 4>,
    // Terrain: per layer specular coefficient (0: diffuse only) and power.
    terrain_spec: vec4<f32>,
    terrain_power: vec4<f32>,
    // Terrain: per layer specular tint, analytical contribution in w.
    terrain_tints: array<vec4<f32>, 4>,
    // x: 1 when the alpha test reads `alpha_test_map`; its xform.
    alpha_flags: vec4<f32>,
    alpha_xform: vec4<f32>,
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
@group(constants::MATERIAL_BIND_GROUP) @binding(115) var surface_dummy: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(116) var surface_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(117) var bump_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(118) var bump_detail_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(119) var detail_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(120) var mask_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(121) var illum_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(122) var illum_detail_map: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(123) var terrain_bump_0: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(124) var terrain_bump_1: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(125) var terrain_bump_2: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(126) var terrain_bump_3: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(127) var alpha_map: texture_2d<f32>;

// decode_bpp16_luvw: base and residual layer to a signed RGB value.
fn luvw(a: vec4<f32>, b: vec4<f32>, range: f32) -> vec3<f32> {
    return ((a.rgb + b.rgb) * 2.0 - 2.0) * (a.a * b.a * range);
}

fn layer(t: texture_2d_array<f32>, uv: vec2<f32>, l: i32) -> vec4<f32> {
    return textureSampleLevel(t, lightmap_sampler, uv, l, 0.0);
}

// One lightmap texel, decoded: the SH with the dominant light taken out,
// packed for evaluating, and the dominant light itself.
struct HaloLight {
    k0: vec3<f32>,
    kr: vec3<f32>,
    kg: vec3<f32>,
    kb: vec3<f32>,
    // Dominant light direction (Halo frame, unit or zero) and intensity.
    dir: vec3<f32>,
    d: vec3<f32>,
}

fn halo_light(uv: vec2<f32>) -> HaloLight {
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
    var l: HaloLight;
    l.kr = vec3(c3.r, c1.r, -c2.r) - e * d.r;
    l.kg = vec3(c3.g, c1.g, -c2.g) - e * d.g;
    l.kb = vec3(c3.b, c1.b, -c2.b) - e * d.b;
    l.k0 = c0 - d * 0.2820948;
    l.dir = dir;
    l.d = d;
    return l;
}

// The SH part alone (the light from everywhere but the dominant light).
fn halo_ambient(l: HaloLight, n: vec3<f32>) -> vec3<f32> {
    let x1 = vec3(dot(n, l.kr), dot(n, l.kg), dot(n, l.kb));
    return max((l.k0 * 0.886227 - x1 * (2.0 * 0.511664)) / 3.14159265, vec3(0.0));
}

// Diffuse irradiance for a Halo-frame (Z up) unit normal.
fn halo_irradiance(l: HaloLight, n: vec3<f32>) -> vec3<f32> {
    let direct = l.d * max(dot(l.dir, n), 0.0) * 0.281;
    return halo_ambient(l, n) + direct;
}

fn xform(uv: vec2<f32>, x: vec4<f32>) -> vec2<f32> {
    return uv * x.xy + x.zw;
}

// A tangent-space normal's x and y from a bump map texel.
fn bump_xy(t: vec4<f32>, signed: f32) -> vec2<f32> {
    return select(t.xy * 2.0 - 1.0, t.xy, signed != 0.0);
}

// One terrain layer's colour: its base map, times its detail map scaled so
// that the detail's mid grey leaves the base as it is. Both textures are
// sampled as sRGB, so the product is in linear light. Alpha: the base's,
// the specular mask of `diffuse_plus_specular` layers.
fn terrain_layer(base: texture_2d<f32>, detail: texture_2d<f32>, uv: vec2<f32>, k: i32) -> vec4<f32> {
    let c = textureSample(base, tiling_sampler, xform(uv, halo.base_xforms[k]));
    let d = halo.detail_xforms[k];
    // Sampled either way, keeping control flow uniform.
    let dc = textureSample(detail, tiling_sampler, xform(uv, d)).rgb * halo.terrain_extra.x;
    return vec4(c.rgb * select(vec3(1.0), dc, d.x != 0.0), c.a);
}

// The blend map's red, green, blue and alpha weight layers 0..3; layers
// that are off get no weight; the weights are normalised to sum to one
// (`docs/format/shaders.md` 7.5).
fn terrain_weights(uv: vec2<f32>) -> vec4<f32> {
    var w = textureSample(blend_map, blend_sampler, xform(uv, halo.blend_xform)) * halo.terrain_layers;
    if halo.terrain_extra.y != 0.0 {
        let sum = w.x + w.y + w.z + w.w;
        if sum > 1e-4 {
            w = w / sum;
        } else {
            w = halo.terrain_layers / max(dot(halo.terrain_layers, vec4(1.0)), 1.0);
        }
    }
    return w;
}

// The four-layer terrain blend; alpha is the blended specular mask times
// coefficient.
fn terrain_albedo(uv: vec2<f32>, w: vec4<f32>) -> vec4<f32> {
    // Every layer is sampled (uniform control flow for the derivatives);
    // the weights pick.
    let l0 = terrain_layer(base_0, detail_0, uv, 0);
    let l1 = terrain_layer(base_1, detail_1, uv, 1);
    let l2 = terrain_layer(base_2, detail_2, uv, 2);
    let l3 = terrain_layer(base_3, detail_3, uv, 3);
    let rgb = l0.rgb * w.x + l1.rgb * w.y + l2.rgb * w.z + l3.rgb * w.w;
    let spec = dot(w * vec4(l0.a, l1.a, l2.a, l3.a), halo.terrain_spec);
    return vec4(rgb, spec);
}

fn terrain_bump_xy(uv: vec2<f32>, w: vec4<f32>) -> vec2<f32> {
    let s = halo.terrain_bump_signed;
    let b0 = bump_xy(textureSample(terrain_bump_0, surface_sampler, xform(uv, halo.terrain_bump_xforms[0])), s.x);
    let b1 = bump_xy(textureSample(terrain_bump_1, surface_sampler, xform(uv, halo.terrain_bump_xforms[1])), s.y);
    let b2 = bump_xy(textureSample(terrain_bump_2, surface_sampler, xform(uv, halo.terrain_bump_xforms[2])), s.z);
    let b3 = bump_xy(textureSample(terrain_bump_3, surface_sampler, xform(uv, halo.terrain_bump_xforms[3])), s.w);
    let k = w * halo.terrain_bump;
    return b0 * k.x + b1 * k.y + b2 * k.z + b3 * k.w;
}

// Specular light leaving towards `v` (Halo frame): the dominant light's
// lobe, the SH towards the reflection (area specular) and a reflection of
// the surroundings (environment map stand-in), all times `coefficient`.
// `tint_n`: the normal specular tint, analytical contribution in w.
fn halo_specular(l: HaloLight, n: vec3<f32>, v: vec3<f32>, coefficient: f32, power_n: f32, tint_n: vec4<f32>) -> vec3<f32> {
    let model = halo.f0.w;
    let r = reflect(-v, n);
    let nv = clamp(dot(n, v), 0.0, 1.0);
    let edge = pow(1.0 - nv, max(halo.spec.w, 1.0));
    var power = power_n;
    var tint = tint_n.rgb;
    var env_weight = vec3(1.0);
    if model == 1.0 {
        // Two-lobe phong: the fresnel curve blends normal and glancing lobes.
        power = mix(power_n, halo.spec.z, edge);
        tint = mix(tint_n.rgb, halo.tint_glancing.rgb, edge);
        env_weight = tint;
    } else if model >= 2.0 {
        // Cook-Torrance and glass: Schlick fresnel from F0.
        let f = halo.f0.rgb + (1.0 - halo.f0.rgb) * edge;
        tint = tint * f;
        env_weight = f;
    }
    let rl = max(dot(r, l.dir), 0.0);
    // The lobe times the light falling on the surface (n . l).
    let facing = max(dot(n, l.dir), 0.0);
    let analytic = l.d * pow(rl, max(power, 1.0)) * facing * tint_n.w;
    let area = halo_ambient(l, r) * halo.tint_glancing.w;
    // The light arriving from around the reflection, without the dominant
    // light (that is the analytic lobe's).
    let env = halo_ambient(l, r) * halo.env.rgb * env_weight;
    return ((analytic + area) * tint + env) * coefficient;
}

fn to_halo_dir(r: vec3<f32>) -> vec3<f32> {
    // Renderer frame (Y up) to Halo frame (Z up): (x, -z, y).
    return vec3(r.x, -r.z, r.y);
}

// Prepass: the alpha test, by `alpha_test_map` when the shader has one, else
// bevy's own (the base map's alpha).
@if(PREPASS_PIPELINE)
fn prepass_cut(in: VertexOutput) {
@if(VERTEX_UVS_A) {
    if halo.alpha_flags.x != 0.0 {
@if(!BINDLESS)
        let cutoff = material.alpha_cutoff;
@else
        let cutoff = 0.5;
        let cut = textureSample(alpha_map, surface_sampler, xform(in.uv, halo.alpha_xform)).a;
        if cut < cutoff {
            discard;
        }
        return;
    }
}
    prepass_sample_color_and_alpha_discard(in);
}

@if(PREPASS_PIPELINE && PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    prepass_cut(in);
    var out: FragmentOutput;
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION)
    out.frag_depth = in.unclipped_depth;
@if(NORMAL_PREPASS)
    out.normal = vec4(normalize(in.world_normal) * 0.5 + vec3(0.5), 1.0);
@if(MOTION_VECTOR_PREPASS)
    out.motion_vector = calculate_motion_vector(in.world_position, in.previous_world_position);
    return out;
}

@if(PREPASS_PIPELINE && !PREPASS_FRAGMENT)
@fragment
fn fragment(in: VertexOutput) {
    prepass_cut(in);
}

@if(!PREPASS_PIPELINE)
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    // Before alpha_discard makes opaque alpha 1: the specular mask of
    // `specular_mask_from_diffuse` shaders.
    let base_alpha = pbr_input.material.base_color.a;
@if(VERTEX_UVS_A) {
    // Cut-outs whose shader has its own `alpha_test_map` (grates): the
    // base map's alpha is a specular mask there, not the cut.
    let cut = textureSample(alpha_map, surface_sampler, xform(in.uv, halo.alpha_xform)).a;
    if halo.alpha_flags.x != 0.0 {
        pbr_input.material.base_color.a = cut;
    }
}
    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color,
    );
    var colour = pbr_input.material.base_color;
    var n = pbr_input.N;
    var mask = 1.0;
    var coefficient = halo.spec.x;
    var power = halo.spec.y;
    var tint_n = halo.tint_normal;
    var illum = vec3(0.0);

@if(VERTEX_UVS_A) {
    let uv = in.uv;
    let terrain = dot(halo.terrain_layers, vec4(1.0)) > 0.0;
    var bump = vec2(0.0);
    var bumped = false;
    if terrain {
        let w = terrain_weights(uv);
        let t = terrain_albedo(uv, w);
        colour = vec4(colour.rgb * t.rgb, colour.a);
        coefficient = t.a;
        power = dot(w, halo.terrain_power);
        // Each layer's tint and analytical contribution, blended.
        tint_n = halo.terrain_tints[0] * w.x + halo.terrain_tints[1] * w.y
            + halo.terrain_tints[2] * w.z + halo.terrain_tints[3] * w.w;
        bump = terrain_bump_xy(uv, w);
        bumped = dot(halo.terrain_bump, vec4(1.0)) > 0.0;
    } else {
        // Every map is sampled (uniform control flow); the flags pick.
        let d = textureSample(detail_map, surface_sampler, xform(uv, halo.detail_xform)).rgb;
        colour = vec4(colour.rgb * select(vec3(1.0), d * halo.terrain_extra.x, halo.surface.z != 0.0), colour.a);
        let b = bump_xy(textureSample(bump_map, surface_sampler, xform(uv, halo.bump_xform)), halo.signed.x);
        let bd = bump_xy(textureSample(bump_detail_map, surface_sampler, xform(uv, halo.bump_detail_xform)), halo.signed.y);
        bump = b + bd * halo.surface.y;
        bumped = halo.surface.x != 0.0;
        let m = textureSample(mask_map, surface_sampler, xform(uv, halo.mask_xform)).r;
        mask = select(select(1.0, base_alpha, halo.surface.w == 1.0), m, halo.surface.w == 2.0);

        let im = textureSample(illum_map, surface_sampler, xform(uv, halo.illum_xform));
        let idet = textureSample(illum_detail_map, surface_sampler, xform(uv, halo.illum_detail_xform)).rgb;
        let mode = halo.illum.x;
        if mode == 1.0 {
            illum = im.rgb * halo.illum_a.rgb;
        } else if mode == 2.0 {
            illum = im.rgb * idet * halo.terrain_extra.x * halo.illum_a.rgb;
        } else if mode == 3.0 {
            illum = colour.rgb * halo.illum_a.rgb;
        } else if mode == 4.0 {
            illum = im.rgb * colour.rgb * halo.illum_a.rgb;
        } else if mode == 5.0 {
            illum = im.r * halo.illum_a.rgb + im.g * halo.illum_b.rgb + im.b * halo.illum_c.rgb;
        } else if mode == 6.0 {
            // Plasma: the mask is in the alpha channel, or in red when its
            // alpha is flat white (illum.y = 1: Narrows' plasma_mask).
            let plasma = select(im.a, im.r, halo.illum.y == 1.0);
            illum = plasma * halo.illum_a.rgb;
        }
    }
@if(VERTEX_TANGENTS) {
    if bumped {
        let z = sqrt(max(1.0 - dot(bump, bump), 0.0));
        let t = in.world_tangent.xyz - n * dot(n, in.world_tangent.xyz);
        if dot(t, t) > 1e-8 {
            let tn = normalize(t);
            let b = cross(n, tn) * select(-1.0, 1.0, in.world_tangent.w >= 0.0);
            n = normalize(tn * bump.x + b * bump.y + n * z);
        }
    }
}
}

@if(VERTEX_UVS_B) {
    let l = halo_light(in.uv_b);
    let nh = to_halo_dir(n);
    let vh = to_halo_dir(pbr_input.V);
    let e = halo_irradiance(l, nh);
    var lit = colour.rgb * e;
    if halo.f0.w != 0.0 || dot(halo.terrain_spec, vec4(1.0)) > 0.0 {
        lit = lit + halo_specular(l, nh, vh, coefficient * mask, power, tint_n);
    }
    colour = vec4(lit, colour.a);
}

@if(VERTEX_COLORS && !VERTEX_UVS_B) {
    // Lit per vertex: no light direction here, but the vertex irradiance
    // stands in for the surroundings an environment map would reflect.
    // Halved: the vertex irradiance includes the dominant light, which the
    // per-pixel reflection leaves to the analytic lobe.
    colour = vec4(colour.rgb + in.color.rgb * halo.env.rgb * (0.5 * coefficient * mask), colour.a);
}

    colour = vec4((colour.rgb + illum) * halo.extra.y, colour.a);
@if(BLEND_MULTIPLY) {
    // Multiply decals (`rmd ` blend_mode multiply: High Ground's
    // `medical_decals`) scale the lit surface under them by their own
    // colour: unlit and unexposed, or the white of the texture times the
    // exposure turned them into white sheets. The vertex colour is the
    // irradiance of vertex-lit objects; take it back out.
    var unlit = pbr_input.material.base_color.rgb;
@if(VERTEX_COLORS) {
    unlit = unlit / max(in.color.rgb, vec3(1e-4));
}
    colour = vec4(min(unlit, vec3(1.0)), colour.a);
}
    var out: FragmentOutput;
    pbr_input.world_position = fog_position(pbr_input.world_position);
    out.color = main_pass_post_lighting_processing(pbr_input, colour);
    return out;
}
