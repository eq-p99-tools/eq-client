// Better Water: the look of a water or lava surface (water.rs).
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::PbrInput,
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::{view, globals, view_transmission_texture, view_transmission_sampler},
    view_transformations::{depth_ndc_to_view_z, frag_coord_to_uv, position_world_to_ndc, ndc_to_uv},
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
    rich: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> water: Water;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var frame_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var frame_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var next_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var next_sampler: sampler;

// A hash of a lattice point, 0 to 1, that holds up at zone-sized coordinates.
fn hash(p: vec2<f32>) -> f32 {
    var q = vec2<u32>(bitcast<u32>(i32(p.x)), bitcast<u32>(i32(p.y)));
    q = q * vec2(1597334673u, 3812015801u);
    let n = (q.x ^ q.y) * 1597334673u;
    return f32(n >> 8u) / 16777216.0;
}

// Smooth value noise and its slope (x: value, yz: d/dx, d/dy).
fn noised(p: vec2<f32>) -> vec3<f32> {
    let i = floor(p);
    let f = p - i;
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let du = 30.0 * f * f * (f * (f - 2.0) + 1.0);
    let a = hash(i);
    let b = hash(i + vec2(1.0, 0.0));
    let c = hash(i + vec2(0.0, 1.0));
    let d = hash(i + vec2(1.0, 1.0));
    let k = vec3(b - a, c - a, a - b - c + d);
    return vec3(
        a + k.x * u.x + k.y * u.y + k.z * u.x * u.y,
        du * vec2(k.x + k.z * u.y, k.y + k.z * u.x),
    );
}

// Plain fractal noise, 0 to 1, for slow patches across a surface.
fn patches(p: vec2<f32>) -> f32 {
    return noised(p).x * 0.6 + noised(p * 2.13 + vec2(17.3, 4.1)).x * 0.4;
}

// The water at a level point, in world units (a person is about six
// tall): its height, -1 to 1 (x), and its slope (yz). A long swell, then
// octaves of drifting noise, each turned and moving its own way so no
// pattern repeats.
fn waves(p: vec2<f32>, t: f32) -> vec3<f32> {
    var height = 0.0;
    var slope = vec2(0.0);
    // Swell: two long, slow, crossing waves.
    let s1 = vec2(0.80, 0.60);
    let s2 = vec2(-0.45, 0.89);
    let p1 = dot(s1, p) * 0.13 - t * 0.75;
    let p2 = dot(s2, p) * 0.19 - t * 0.9;
    height += 0.3 * sin(p1) + 0.2 * sin(p2);
    slope += s1 * 0.07 * cos(p1) + s2 * 0.055 * cos(p2);
    // Chop: noise octaves with their own drift, the first ones drawn out
    // across the wind into long wave crests.
    let across = vec2(-s1.y, s1.x);
    var q = vec2(dot(p, s1) * 0.11, dot(p, across) * 0.035);
    var amplitude = 0.5;
    let turn = mat2x2<f32>(0.8, 0.6, -0.6, 0.8);
    var drift = vec2(0.3, 0.18);
    var scale = 0.09;
    for (var i = 0; i < 5; i++) {
        let n = noised(q + drift * t);
        height += (n.x - 0.5) * amplitude;
        // The noise's slope is per unit of q; bring it back to world units.
        // Turned back to world axes, the stretch undone in the slope.
        let stretched = n.yz * amplitude * select(vec2(0.11, 0.035), vec2(scale), i > 0);
        slope += select(s1 * stretched.x + across * stretched.y, n.yz * amplitude * scale, i > 0) * 1.25;
        q = select(turn * q * 2.03 + vec2(3.7, 1.3), turn * (p * 0.09 * 2.03) + vec2(3.7, 1.3), i == 0);
        scale *= 2.03;
        amplitude *= 0.48;
        drift = turn * drift * 1.35;
    }
    return vec3(height, slope);
}

// A texture sampled with its repeat hidden: a slow noise picks one of many
// offsets of the same texture and blends between neighbours (after Inigo
// Quilez's "texture repetition", technique 3).
fn untiled(texture: texture_2d<f32>, smp: sampler, uv: vec2<f32>, pick: f32) -> vec4<f32> {
    let dx = dpdx(uv);
    let dy = dpdy(uv);
    let l = pick * 8.0;
    let ia = floor(l);
    let f = l - ia;
    let ca = textureSampleGrad(texture, smp, uv + sin(vec2(3.0, 7.0) * ia), dx, dy);
    let cb = textureSampleGrad(texture, smp, uv + sin(vec2(3.0, 7.0) * (ia + 1.0)), dx, dy);
    let difference = ca.rgb - cb.rgb;
    return mix(ca, cb, smoothstep(0.2, 0.8, f - 0.1 * (difference.x + difference.y + difference.z)));
}

// What Rich Lighting's lights (the zone's lamps, carried lights and the
// sun, with their shadows) add here, as they light the ground beside the
// water (rich_light.wgsl): worked out on a white surface with no ambient,
// softly capped by luminance. No specular: the water makes its own glints.
fn rich_light(input: PbrInput) -> vec3<f32> {
    var lit_input = input;
    lit_input.material.base_color = vec4(1.0);
    lit_input.material.emissive = vec4(0.0);
    lit_input.material.reflectance = vec3(0.0);
    lit_input.material.perceptual_roughness = 1.0;
    lit_input.diffuse_occlusion = vec3(0.0);
    lit_input.specular_occlusion = 0.0;
    let lit = apply_pbr_lighting(lit_input).rgb;
    let brightness = dot(lit, vec3(0.2126, 0.7152, 0.0722));
    // Outdoors the night's grading darkens the frame afterwards, so the
    // ceiling rises to hold once graded.
    let night = min(view.color_grading.exposure, 0.0);
    let cap = water.rich.y * select(1.0, exp2(-night), water.rich.z > 0.5);
    return lit / (1.0 + brightness / max(cap, 0.0001));
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
    // Each layer picks its offsets from its own slow noise across the zone.
    let pick1 = patches(world.xz * 0.011);
    let pick2 = patches(world.xz * 0.009 + vec2(41.0, 13.0));
    let a = mix(untiled(frame_texture, frame_sampler, uv1, pick1), untiled(next_texture, next_sampler, uv1, pick1), blend);
    let b = mix(untiled(frame_texture, frame_sampler, uv2, pick2), untiled(next_texture, next_sampler, uv2, pick2), blend);
    // Wind: broad patches of rougher and calmer water drift across it.
    let wind = smoothstep(0.2, 0.8, patches(world.xz * 0.0045 + vec2(t * 0.012, t * 0.007)));
    let albedo = mix(a.rgb, b.rgb, 0.45) * (0.88 + 0.24 * wind);
    var light = vec3(1.0);
#ifdef VERTEX_COLORS
    light = in.color.rgb;
#endif
    if water.rich.x > 0.5 && !lava {
        light += rich_light(pbr_input);
    }

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
    // Far ripples are finer than a pixel: calm them rather than shimmer.
    let distance = length(view.world_position.xyz - world);
    let calm = 1.0 - 0.75 * smoothstep(80.0, 400.0, distance);
    let wave = waves(world.xz, t);
    let rough = calm * (0.55 + 0.9 * wind);
    let slope = wave.yz * rough;
    // Crests stand out and troughs sink back, more so where it is rough.
    let crest = clamp(0.5 + wave.x * 0.6 * rough, 0.0, 1.0);
    var normal = normalize(vec3(-slope.x, 1.0, -slope.y));
    if !from_above {
        normal = -normal;
    }
    let facing = clamp(dot(normal, to_eye), 0.0, 1.0);
    let fresnel = min(0.02 + 0.98 * pow(1.0 - facing, 5.0), 0.6);
    let mirrored = reflect(-to_eye, normal);
    let screen = frag_coord_to_uv(in.position.xy);
    let here_z = depth_ndc_to_view_z(in.position.z);

    // What lies behind the surface, bent by the ripples. A bend that would
    // land on something in front of the water is not taken.
    var bend = slope * vec2(1.0, -1.0) * 0.12 / max(distance * 0.02, 1.0);
    var thickness = 12.0;
    var behind_at = world - to_eye * 12.0;
#ifdef DEPTH_PREPASS
    if water.timing.w > 0.5 {
        let straight = depth_ndc_to_view_z(prepass_depth(in.position, 0u));
        let bent_coord = in.position.xy + bend * view.viewport.zw;
        let bent_depth = prepass_depth(vec4(clamp(bent_coord, view.viewport.xy, view.viewport.xy + view.viewport.zw - 1.0), 0.0, 0.0), 0u);
        let bent = depth_ndc_to_view_z(bent_depth);
        if bent > here_z {
            bend = vec2(0.0);
            thickness = max(here_z - straight, 0.0);
        } else {
            thickness = max(here_z - bent, 0.0);
        }
        behind_at = world - to_eye * thickness;
    }
#endif
    let behind = textureSampleLevel(view_transmission_texture, view_transmission_sampler, screen + bend, 0.0).rgb;

    var color: vec3<f32>;
    var water_layer: vec3<f32>;
    var opacity: f32;
    let sun = water.sun_direction;
    if from_above {
        // Light dancing on whatever lies under shallow water.
        let under = max(world.y - behind_at.y, 0.0);
        let caustic_depth = smoothstep(0.3, 2.5, under) * (1.0 - smoothstep(12.0, 45.0, under));
        let c1 = noised(behind_at.xz * 0.35 + vec2(t * 0.21, t * 0.13)).yz;
        let c2 = noised(behind_at.xz * 0.47 + vec2(-t * 0.17, t * 0.19) + 7.3).yz;
        let caustic = pow(clamp(1.0 - length(c1 + c2) * 0.42, 0.0, 1.0), 6.0);
        var seen = behind + behind * caustic * caustic_depth * 0.9 * (0.3 + 0.7 * sun.w);

        // The water between: clear when thin, tinted and then dark when deep.
        let absorbed = 1.0 - exp(-thickness * water.deep.a);
        let deep_body = water.deep.rgb * max(light, vec3(0.35)) * (0.6 + 0.8 * luminance(albedo));
        var body = mix(albedo * water.shallow.rgb * light, deep_body, absorbed * 0.85);
        body *= 0.85 + 0.3 * crest;
        opacity = mix(0.18, 0.92, absorbed);
        water_layer = body;

        // Reflections: trace the mirrored ray through what is on screen,
        // falling back to the sky where it leaves the screen or finds
        // nothing.
        let horizon = water.sky.rgb;
        let zenith = horizon * vec3(0.45, 0.6, 0.9);
        var reflected = mix(horizon, zenith, smoothstep(0.0, 0.35, mirrored.y)) * 0.85;
#ifdef DEPTH_PREPASS
        if water.timing.w > 0.5 {
            let found = trace(world, mirrored, distance);
            reflected = mix(reflected, found.rgb, found.a);
        }
#endif
        // Calm patches mirror more than rough ones.
        let mirror = fresnel * (0.62 - 0.2 * wind);
        water_layer = mix(water_layer, reflected, mirror / max(opacity + mirror * (1.0 - opacity), 0.001));
        opacity = opacity + mirror * (1.0 - opacity);

        // Light passing through the thin tops of waves facing away from
        // the sun turns them a brighter green-blue.
        let back = pow(clamp(dot(-to_eye.xz, normalize(sun.xz + vec2(0.0001))) * 0.5 + 0.5, 0.0, 1.0), 2.0);
        let through = smoothstep(0.55, 1.0, crest) * (0.35 + 0.65 * back) * (0.3 + 0.7 * sun.w) * 0.5;
        water_layer += vec3(0.05, 0.16, 0.14) * through * (1.0 - fresnel) * max(light, vec3(0.5));

        // Foam where the water thins out against the shore.
        let shore = thickness * max(abs(to_eye.y), 0.2);
        let broken = smoothstep(0.35, 0.8, luminance(b.rgb) + 0.25 * sin(t * 1.3 + world.x * 0.3 + world.z * 0.23));
        let foam = (1.0 - smoothstep(0.0, 1.2, shore)) * (0.2 + 0.8 * broken);
        water_layer = mix(water_layer, vec3(0.9, 0.95, 0.95) * max(light, vec3(0.6)), foam * 0.75 / max(opacity, 0.05));
        opacity = max(opacity, foam * 0.85);

        // The sun glints in the ripples.
        let toward = max(dot(mirrored, normalize(sun.xyz)), 0.0);
        let glint = (pow(toward, 600.0) * 5.0 + pow(toward, 60.0) * 0.18) * sun.w;
        water_layer += glint * water.sun_color.rgb / max(opacity, 0.2);
        opacity = min(1.0, opacity + glint);

        // The water's own colour takes the fog; what is behind already has.
        let fogged = main_pass_post_lighting_processing(pbr_input, vec4(water_layer, 1.0)).rgb;
        color = mix(seen, fogged, clamp(opacity, 0.0, 1.0));
    } else {
        // Seen from below: the world above through a bright window
        // overhead, darkening toward the slant where the surface mirrors
        // the deep.
        let window = smoothstep(0.45, 0.9, facing);
        let deep = water.deep.rgb * 1.6;
        let above = behind * vec3(0.8, 0.95, 1.0) + albedo * 0.05;
        color = mix(deep, above, window);
        color = main_pass_post_lighting_processing(pbr_input, vec4(color, 1.0)).rgb;
    }
    out.color = vec4(color, 1.0);
    return out;
}

// The colour (rgb) the mirrored ray from `start` finds on screen and how
// sure the find is (a), by marching it against the scene's depth.
#ifdef DEPTH_PREPASS
fn trace(start: vec3<f32>, direction: vec3<f32>, distance: f32) -> vec4<f32> {
    let steps = 24;
    let reach = clamp(distance * 1.5, 40.0, 400.0);
    var travelled = 0.5;
    var step = reach / f32(steps) * 0.25;
    for (var i = 0; i < steps; i++) {
        travelled += step;
        step *= 1.12;
        let point = start + direction * travelled;
        let ndc = position_world_to_ndc(point);
        if any(abs(ndc.xy) > vec2(1.0)) || ndc.z <= 0.0 {
            return vec4(0.0);
        }
        let uv = ndc_to_uv(ndc.xy);
        let coord = uv * view.viewport.zw + view.viewport.xy;
        let scene = depth_ndc_to_view_z(prepass_depth(vec4(coord, 0.0, 0.0), 0u));
        let ray = depth_ndc_to_view_z(ndc.z);
        // The ray has gone behind something drawn, not far behind it.
        let gap = scene - ray;
        if gap > 0.0 && gap < max(step * 2.0, 3.0) {
            let edge = smoothstep(0.0, 0.12, min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y)));
            let far = 1.0 - smoothstep(0.6, 1.0, travelled / reach);
            let colour = textureSampleLevel(view_transmission_texture, view_transmission_sampler, uv, 0.0).rgb;
            return vec4(colour, edge * far);
        }
    }
    return vec4(0.0);
}
#endif
