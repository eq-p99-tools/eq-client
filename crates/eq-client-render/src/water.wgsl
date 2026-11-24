// Better Water: the look of a water or lava surface (water.rs).
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::main_pass_post_lighting_processing,
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals},
    view_transformations::depth_ndc_to_view_z,
}
#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils::prepass_depth
#endif

struct Water {
    sky: vec4<f32>,
    sun_direction: vec4<f32>,
    sun_color: vec4<f32>,
    shallow: vec4<f32>,
    deep: vec4<f32>,
    timing: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> water: Water;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var frame_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var frame_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var next_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var next_sampler: sampler;

// The slope of a few travelling waves at a level point, in world units
// (a person is about six tall).
fn ripples(p: vec2<f32>, t: f32) -> vec2<f32> {
    var directions = array<vec2<f32>, 6>(
        vec2(0.80, 0.60), vec2(-0.45, 0.89), vec2(0.97, -0.24),
        vec2(-0.71, -0.70), vec2(0.20, 0.98), vec2(-0.93, 0.37),
    );
    var lengths = array<f32, 6>(29.0, 19.0, 12.5, 8.1, 5.3, 3.4);
    var slopes = array<f32, 6>(0.035, 0.035, 0.04, 0.035, 0.03, 0.025);
    var slope = vec2(0.0);
    for (var i = 0; i < 6; i++) {
        let k = 6.2831853 / lengths[i];
        let phase = dot(directions[i], p) * k - t * sqrt(9.8 * k) * 0.55;
        slope += directions[i] * slopes[i] * cos(phase);
    }
    return slope;
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3(0.299, 0.587, 0.114));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let t = globals.time;
    let world = in.world_position.xyz;
    let lava = water.timing.z > 0.5;

    // Frames blend into each other, and two layers flow apart.
    let blend = smoothstep(0.0, 1.0, clamp((t - water.timing.x) / max(water.timing.y, 0.001), 0.0, 1.0));
    var uv = vec2(0.0);
#ifdef VERTEX_UVS_A
    uv = in.uv;
#endif
    let flow = select(0.03, 0.008, lava);
    let uv1 = uv + vec2(t * flow, t * flow * 0.6);
    let uv2 = uv * 0.71 + vec2(-t * flow * 0.8, t * flow * 0.45) + vec2(0.37, 0.11);
    let a = mix(textureSample(frame_texture, frame_sampler, uv1), textureSample(next_texture, next_sampler, uv1), blend);
    let b = mix(textureSample(frame_texture, frame_sampler, uv2), textureSample(next_texture, next_sampler, uv2), blend);
    let albedo = mix(a.rgb, b.rgb, 0.45);
    var light = vec3(1.0);
#ifdef VERTEX_COLORS
    light = in.color.rgb;
#endif

    var out: FragmentOutput;
    if lava {
        // Molten rock glows on its own, brighter where the texture is hot.
        let heat = luminance(albedo);
        let pulse = 0.85 + 0.15 * sin(t * 1.7 + world.x * 0.05 + world.z * 0.07);
        // Cooler crust darkens and the hottest seams run yellow.
        var glow = albedo * (0.7 + 0.6 * smoothstep(0.15, 0.7, heat) * pulse);
        glow = mix(glow, vec3(1.0, 0.78, 0.35), smoothstep(0.55, 0.95, heat) * 0.45 * pulse);
        out.color = main_pass_post_lighting_processing(pbr_input, vec4(glow, 1.0));
        return out;
    }

    let to_eye = normalize(view.world_position.xyz - world);
    let from_above = to_eye.y >= 0.0;
    // The texture's own pattern, as bumps, breaks up the waves' regularity.
    let e = 0.02;
    let bump = vec2(
        luminance(textureSample(frame_texture, frame_sampler, uv1 + vec2(e, 0.0)).rgb) - luminance(a.rgb)
            + luminance(textureSample(frame_texture, frame_sampler, uv2 + vec2(e, 0.0)).rgb) - luminance(b.rgb),
        luminance(textureSample(frame_texture, frame_sampler, uv1 + vec2(0.0, e)).rgb) - luminance(a.rgb)
            + luminance(textureSample(frame_texture, frame_sampler, uv2 + vec2(0.0, e)).rgb) - luminance(b.rgb),
    );
    // Far ripples are finer than a pixel: calm them rather than shimmer.
    let distance = length(view.world_position.xyz - world);
    let calm = 1.0 - 0.75 * smoothstep(80.0, 400.0, distance);
    let slope = (ripples(world.xz, t) + bump * 1.6) * calm;
    var normal = normalize(vec3(-slope.x, 1.0, -slope.y));
    if !from_above {
        normal = -normal;
    }
    let facing = clamp(dot(normal, to_eye), 0.0, 1.0);
    let fresnel = min(0.02 + 0.98 * pow(1.0 - facing, 5.0), 0.6);
    let mirrored = reflect(-to_eye, normal);

    // How much water lies between the surface and what is behind it.
    var thickness = 12.0;
#ifdef DEPTH_PREPASS
    if water.timing.w > 0.5 {
        let behind = depth_ndc_to_view_z(prepass_depth(in.position, 0u));
        let here = depth_ndc_to_view_z(in.position.z);
        thickness = max(here - behind, 0.0);
    }
#endif
    let absorbed = 1.0 - exp(-thickness * water.deep.a);
    // The texture's own detail stays faintly visible however deep it is.
    let deep_body = water.deep.rgb * max(light, vec3(0.35)) * (0.6 + 0.8 * luminance(albedo));
    let body = mix(albedo * water.shallow.rgb * light, deep_body, absorbed * 0.85);

    let sun = water.sun_direction;
    var color: vec3<f32>;
    var alpha: f32;
    if from_above {
        let horizon = water.sky.rgb;
        let zenith = horizon * vec3(0.45, 0.6, 0.9);
        let sky = mix(horizon, zenith, smoothstep(0.0, 0.35, mirrored.y)) * 0.85;
        // Only the sky is mirrored, not the shore, walls and trees around
        // it, so it is held back rather than shown whole.
        color = mix(body, sky, fresnel * 0.5);
        alpha = max(mix(0.12, 0.9, absorbed), fresnel * 0.8);

        // Foam where the water thins out against the shore.
        let shore = thickness * max(abs(to_eye.y), 0.2);
        let broken = smoothstep(0.35, 0.8, luminance(b.rgb) + 0.25 * sin(t * 1.3 + world.x * 0.3 + world.z * 0.23));
        let foam = (1.0 - smoothstep(0.0, 1.2, shore)) * (0.2 + 0.8 * broken);
        color = mix(color, vec3(0.9, 0.95, 0.95) * max(light, vec3(0.6)), foam * 0.75);
        alpha = max(alpha, foam * 0.85);

        // The sun glints in the ripples.
        let toward = max(dot(mirrored, normalize(sun.xyz)), 0.0);
        let glint = (pow(toward, 600.0) * 5.0 + pow(toward, 60.0) * 0.18) * sun.w;
        color += glint * water.sun_color.rgb;
        alpha = min(1.0, alpha + glint);
    } else {
        // Seen from below: the bright window overhead, darkening toward
        // the slant where the surface mirrors the deep.
        let window = smoothstep(0.55, 0.95, facing);
        color = mix(water.deep.rgb * 1.5, water.sky.rgb * 1.1 + albedo * 0.2, window);
        alpha = mix(0.92, 0.55, window);
    }
    out.color = main_pass_post_lighting_processing(pbr_input, vec4(color, alpha));
    return out;
}
