// Better Water: the view under the surface once the scene is drawn
// (underwater.rs). A gentle shimmer, and shafts of sunlight that live in
// the world: each pixel's ray is marched through the water up to what is
// drawn there, and every point on it takes the light that reaches it from
// the surface along the sun's bent direction. Both stay light so the view
// stays clear.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import bevy_render::view::View

struct Underwater {
    shafts: vec4<f32>,
    light: vec4<f32>,
    state: vec4<f32>,
}

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> settings: Underwater;
@group(0) @binding(3) var<uniform> view: View;
#ifdef MULTISAMPLED
@group(0) @binding(4) var depth_texture: texture_multisampled_2d<f32>;
#else
@group(0) @binding(4) var depth_texture: texture_2d<f32>;
#endif

// The same lattice hash and smooth noise as water.wgsl.
fn hash(p: vec2<f32>) -> f32 {
    var q = vec2<u32>(bitcast<u32>(i32(p.x)), bitcast<u32>(i32(p.y)));
    q = q * vec2(1597334673u, 3812015801u);
    let n = (q.x ^ q.y) * 1597334673u;
    return f32(n >> 8u) / 16777216.0;
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
        mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x),
        u.y,
    );
}

// How much light comes down through the surface at a level point: bright
// threads where the ripples focus it, drifting and changing as they move.
fn through_surface(p: vec2<f32>, t: f32) -> f32 {
    let a = noise(p * 0.16 + vec2(t * 0.05, t * 0.03));
    let b = noise(p * 0.23 + vec2(-t * 0.04, t * 0.06) + 13.7);
    let c = noise(p * 0.07 + vec2(t * 0.015, -t * 0.01) + 5.1);
    // Ridges where two drifting fields cross make the threads; a slow
    // field makes some stretches of the surface brighter than others.
    let ridge = 1.0 - abs(a + b - 1.0) * 2.0;
    return pow(clamp(ridge, 0.0, 1.0), 4.0) * smoothstep(0.25, 0.75, c);
}

fn scene_depth(coord: vec2<f32>) -> f32 {
    // Sample 0 of a multisampled depth, or mip 0 of a plain one.
    return textureLoad(depth_texture, vec2<i32>(coord), 0).r;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let t = settings.state.x;
    let lava = settings.state.z > 0.5;
    let uv = in.uv;

    // Shimmer: a slow, small sway of the picture, a little stronger at the
    // edges than in the middle where the player looks.
    let edge = 0.35 + 0.65 * smoothstep(0.1, 0.7, length(uv - 0.5));
    let speed = select(1.0, 0.45, lava);
    let sway = vec2(
        sin(uv.y * 23.0 + t * 1.7 * speed) + 0.5 * sin(uv.y * 41.0 - t * 2.3 * speed),
        cos(uv.x * 19.0 + t * 1.3 * speed) + 0.5 * cos(uv.x * 37.0 + t * 2.9 * speed),
    ) * 0.0016 * edge;
    var color = textureSample(screen_texture, screen_sampler, clamp(uv + sway, vec2(0.001), vec2(0.999))).rgb;

    let strength = settings.shafts.a;
    if strength > 0.0 {
        // Where this pixel's ray ends: what is drawn there, or far off.
        let coord = in.position.xy;
        let depth = scene_depth(coord);
        let ndc = vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
        let far_point = view.world_from_clip * vec4(ndc, max(depth, 0.00001), 1.0);
        let eye = view.world_position;
        let toward = far_point.xyz / far_point.w - eye;
        let reach = min(length(toward), 90.0);
        let ray = normalize(toward);

        let down = settings.light.xyz;
        let surface = eye.y + settings.light.w;
        let steps = 20;
        let step = reach / f32(steps);
        // A fixed per-pixel offset hides the steps as soft grain.
        let jitter = fract(52.9829189 * fract(dot(coord, vec2(0.06711056, 0.00583715))));
        var gathered = 0.0;
        for (var i = 0; i < steps; i++) {
            let along = (f32(i) + jitter) * step;
            let point = eye + ray * along;
            let under = surface - point.y;
            if under <= 0.0 {
                continue;
            }
            // Back up the sunlight's path to where it came through.
            let path = under / max(-down.y, 0.2);
            let entry = point.xz - down.xz * path;
            // Light thins on its way down and again on its way to the eye.
            let lost = exp(-path * 0.03 - along * 0.025);
            gathered += through_surface(entry, t) * lost;
        }
        gathered *= step * 1.0;
        color += settings.shafts.rgb * gathered * strength;
    }
    return vec4(color, 1.0);
}
