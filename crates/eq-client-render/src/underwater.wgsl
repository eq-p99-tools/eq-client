// Better Water: the view under the surface once the scene is drawn
// (water.rs, UnderwaterView). A gentle shimmer and soft light slanting
// down from the surface; both stay light so the view stays clear.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct Underwater {
    shafts: vec4<f32>,
    state: vec4<f32>,
}

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> settings: Underwater;

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

    // Light shafts: soft bands falling from the top of the view, slanting
    // a little, drifting and flickering; stronger looking up.
    let strength = settings.shafts.a * (0.55 + 0.45 * clamp(settings.state.w + 0.3, 0.0, 1.0));
    if strength > 0.0 {
        let slant = uv.x + (1.0 - uv.y) * 0.18;
        var bands = 0.0;
        bands += pow(0.5 + 0.5 * sin(slant * 17.0 + t * 0.35), 6.0);
        bands += 0.7 * pow(0.5 + 0.5 * sin(slant * 29.0 - t * 0.5 + 1.7), 8.0);
        bands += 0.5 * pow(0.5 + 0.5 * sin(slant * 47.0 + t * 0.8 + 4.1), 10.0);
        let flicker = 0.75 + 0.25 * sin(t * 1.9 + slant * 9.0);
        // Brightest near the surface (the top of the view), gone by the bottom.
        let fall = pow(clamp(1.0 - uv.y, 0.0, 1.0), 1.6);
        color += settings.shafts.rgb * bands * flicker * fall * strength;
    }
    return vec4(color, 1.0);
}
