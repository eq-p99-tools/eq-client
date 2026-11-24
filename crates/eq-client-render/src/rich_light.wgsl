// Rich Lighting (rich_light.rs): a zone surface lit per pixel. The zone's
// own light reaches the pixel as the vertex colour (the base), which the
// unlit draw would multiply the texture by. Bevy's point lights (the zone's
// lamps, carried lights) are worked out on a white surface, softly capped,
// and added to the base before the texture multiplies both.
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
}

// The base light's tint, white unless the client asks otherwise.
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> base_tint: vec4<f32>;
// x: the soft ceiling on the lights' own brightness.
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> limits: vec4<f32>;

@fragment
fn fragment(
    vertex_output: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var in = vertex_output;
#ifdef VERTEX_COLORS
    let base = in.color.rgb * base_tint.rgb;
    // The texture alone, as the surface's colour under the lights.
    in.color = vec4<f32>(1.0, 1.0, 1.0, in.color.a);
#else
    let base = base_tint.rgb;
#endif
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let surface = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    // The lights alone, on a white surface; the base holds the zone's
    // ambient, so no ambient or environment light.
    pbr_input.material.base_color = vec4<f32>(1.0, 1.0, 1.0, surface.a);
    pbr_input.material.emissive = vec4<f32>(0.0);
    pbr_input.diffuse_occlusion = vec3<f32>(0.0);
    pbr_input.specular_occlusion = 0.0;
    let lit = apply_pbr_lighting(pbr_input);
    // A soft ceiling by luminance, which keeps the light's hue.
    let luminance = dot(lit.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let light = lit.rgb / (1.0 + luminance / max(limits.x, 0.0001));

    var out: FragmentOutput;
    pbr_input.material.base_color = surface;
    out.color = vec4<f32>(surface.rgb * (base + light), lit.a);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
