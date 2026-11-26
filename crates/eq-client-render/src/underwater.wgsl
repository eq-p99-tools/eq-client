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
    grid: vec4<f32>,
    open: array<vec4<u32>, 32>,
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

// The same lattice hash as water.wgsl, and smooth value noise from it.
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

// How much light comes down through the surface at a level point. The
// surface is split into cells about seven units across, each with one
// thin spot of light wandering about it that brightens and fades on its
// own slow beat, so the shafts are slim, spread evenly rather than in
// clumps, and each comes and goes by itself even while the camera holds
// still. The cells are turned off the world's axes so they never line up.
fn through_surface(p: vec2<f32>, under: f32, t: f32) -> f32 {
    let cell = 7.0;
    let q = mat2x2<f32>(0.8, 0.6, -0.6, 0.8) * p / cell;
    let i = floor(q);
    let f = q - i;
    var light = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let g = vec2(f32(x), f32(y));
            let h1 = hash(i + g);
            let h2 = hash(i + g + vec2(37.0, 17.0));
            let h3 = hash(i + g + vec2(11.0, 53.0));
            // Lit about a fifth of the time, each on its own beat.
            let beat = sin(t * (0.3 + 0.25 * h3) + 6.2831 * h1) * 0.5 + 0.5;
            let lit = smoothstep(0.72, 0.95, beat);
            let centre = g + 0.5 + 0.35 * sin(t * vec2(0.23, 0.31) + 6.2831 * vec2(h1, h2));
            let away = length(centre - f) * cell;
            let width = 0.45 + 0.35 * h2;
            let shimmer = 0.7 + 0.3 * sin(t * 2.3 + 9.0 * h3);
            // Brighter and fainter down its length, so each reads as a
            // wisp rather than a rod.
            let wisp = 0.5 + 0.5 * sin(under * 0.22 + 6.2831 * h3 - t * 0.5);
            light += lit * shimmer * wisp * (1.0 - smoothstep(0.0, width, away));
        }
    }
    return light;
}

// Whether the surface at a level point can see the sun (1) or is shaded by
// something overhead (0), from the grid the client casts around the
// camera, blended between cells; outside the grid counts as open.
fn open_cell(cell: vec2<f32>) -> f32 {
    if any(cell < vec2(0.0)) || any(cell >= vec2(64.0)) {
        return 1.0;
    }
    let index = u32(cell.y) * 64u + u32(cell.x);
    let word = index / 32u;
    let bits = settings.open[word / 4u][word % 4u];
    return f32((bits >> (index % 32u)) & 1u);
}

fn sunlit(p: vec2<f32>) -> f32 {
    if settings.grid.w < 0.5 {
        return 1.0;
    }
    let local = (p - settings.grid.xy) / settings.grid.z - 0.5;
    let base = floor(local);
    let f = local - base;
    return mix(
        mix(open_cell(base), open_cell(base + vec2(1.0, 0.0)), f.x),
        mix(open_cell(base + vec2(0.0, 1.0)), open_cell(base + vec2(1.0, 1.0)), f.x),
        f.y,
    );
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
        let reach = min(length(toward), 50.0);
        let ray = normalize(toward);

        let down = settings.light.xyz;
        let surface = eye.y + settings.light.w;
        let steps = 48;
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
            gathered += through_surface(entry, under, t) * sunlit(entry) * lost;
        }
        gathered *= step * 4.0;
        color += settings.shafts.rgb * gathered * strength;
    }
    return vec4(color, 1.0);
}
