#import bevy_ui::ui_vertex_output::UiVertexOutput

@group(1) @binding(0) var mask: texture_2d<f32>;
@group(1) @binding(1) var mask_sampler: sampler;
@group(1) @binding(2) var<uniform> ink: vec4<f32>;

fn coverage(uv: vec2<f32>) -> f32 {
    return textureSample(mask, mask_sampler, uv).a;
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    // The model itself contributes no fill. The outer edge is a few
    // display pixels wide, independent of model distance or mask size.
    let step = 2.0 / max(in.size, vec2<f32>(1.0));
    let across = vec2<f32>(step.x, 0.0);
    let down = vec2<f32>(0.0, step.y);
    let diagonal = step * 0.7071068;
    let opposite = vec2<f32>(diagonal.x, -diagonal.y);
    let axes = max(
        max(coverage(in.uv + across), coverage(in.uv - across)),
        max(coverage(in.uv + down), coverage(in.uv - down))
    );
    let corners = max(
        max(coverage(in.uv + diagonal), coverage(in.uv - diagonal)),
        max(coverage(in.uv + opposite), coverage(in.uv - opposite))
    );
    let edge = max(max(axes, corners) - coverage(in.uv), 0.0);
    return vec4<f32>(ink.rgb, ink.a * edge);
}
