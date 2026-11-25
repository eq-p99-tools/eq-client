//! Better Water's view from under the surface (`underwater.wgsl`): after
//! the scene is drawn, a small shimmer and shafts of light falling through
//! the water from the sun.
//!
//! The shafts live in the world, not on the screen: the pass marches each
//! pixel's ray through the water up to what is drawn there (the scene's
//! depth), and at each point asks how much light reaches it from the
//! surface along the sun's direction, bent as it enters the water. So the
//! shafts slant with the sun, stay put as the camera turns, slide past as
//! it swims, end on the floor and walls, and ripple with the surface.
//!
//! Bevy's full-screen material binds only the scene's colour; this pass
//! also needs the view and the depth, so it is set up here the way Bevy's
//! own post effects are.

use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    core_pipeline::{
        FullscreenShader,
        schedule::{Core3d, Core3dSystems},
        tonemapping::tonemapping,
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_component::{
            ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
            UniformComponentPlugin,
        },
        render_resource::{
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            CachedRenderPipelineId, ColorTargetState, ColorWrites, FragmentState, Operations,
            PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            ShaderType, SpecializedRenderPipeline, SpecializedRenderPipelines, TextureFormat,
            TextureSampleType, TextureUsages,
            binding_types::{sampler, texture_2d, texture_2d_multisampled, uniform_buffer},
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        view::{
            ExtractedView, Msaa, ViewDepthTexture, ViewTarget, ViewUniform, ViewUniformOffset,
            ViewUniforms, prepare_view_targets,
        },
    },
    shader::{Shader, ShaderDefVal},
};

/// What the view under the surface adds once the scene is drawn. Mirrors
/// `Underwater` in `underwater.wgsl`.
#[derive(Component, ExtractComponent, Clone, Copy, Default, ShaderType)]
pub(crate) struct UnderwaterView {
    /// The tint of the light shafts (rgb) and how strong they are (a).
    pub(crate) shafts: Vec4,
    /// The way sunlight travels once in the water, bent at the surface
    /// (xyz, pointing down), and the height of the surface above the
    /// camera (w).
    pub(crate) light: Vec4,
    /// Seconds on the wrapped clock (x), depth under the surface (y), and 1
    /// for lava (z).
    pub(crate) state: Vec4,
    /// Where the sunlit grid lies: its corner (x and z in xy), its cell size
    /// (z), and 1 when there is one (w).
    pub(crate) grid: Vec4,
    /// One bit for each cell of the grid, row by row: set where the surface
    /// there can see the sun.
    pub(crate) open: [UVec4; 32],
}

pub(super) fn register(app: &mut App) {
    embedded_asset!(app, "underwater.wgsl");
    app.add_plugins((
        ExtractComponentPlugin::<UnderwaterView>::default(),
        UniformComponentPlugin::<UnderwaterView>::default(),
    ));
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_resource::<SpecializedRenderPipelines<UnderwaterPipeline>>()
        .add_systems(RenderStartup, init_pipeline)
        .add_systems(
            Render,
            (
                prepare_pipelines.in_set(RenderSystems::Prepare),
                depth_readable
                    .in_set(RenderSystems::PrepareViews)
                    .after(prepare_view_targets)
                    .ambiguous_with_all(),
            ),
        )
        .add_systems(
            Core3d,
            draw.in_set(Core3dSystems::PostProcess).before(tonemapping),
        );
}

#[derive(Resource)]
struct UnderwaterPipeline {
    sampler: Sampler,
    layout: BindGroupLayoutDescriptor,
    layout_msaa: BindGroupLayoutDescriptor,
    fullscreen_shader: FullscreenShader,
    fragment_shader: Handle<Shader>,
}

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn init_pipeline(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    asset_server: Res<AssetServer>,
) {
    let layout = |name: &'static str, multisampled: bool| {
        let depth = TextureSampleType::Float { filterable: false };
        BindGroupLayoutDescriptor::new(
            name,
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    uniform_buffer::<UnderwaterView>(true),
                    uniform_buffer::<ViewUniform>(true),
                    if multisampled {
                        texture_2d_multisampled(depth)
                    } else {
                        texture_2d(depth)
                    },
                ),
            ),
        )
    };
    commands.insert_resource(UnderwaterPipeline {
        sampler: render_device.create_sampler(&SamplerDescriptor::default()),
        layout: layout("underwater_layout", false),
        layout_msaa: layout("underwater_layout_msaa", true),
        fullscreen_shader: fullscreen_shader.clone(),
        fragment_shader: load_embedded_asset!(asset_server.as_ref(), "underwater.wgsl"),
    });
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
struct UnderwaterPipelineKey {
    target_format: TextureFormat,
    samples: u32,
}

impl SpecializedRenderPipeline for UnderwaterPipeline {
    type Key = UnderwaterPipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        let multisampled = key.samples > 1;
        let mut shader_defs = Vec::new();
        if multisampled {
            shader_defs.push(ShaderDefVal::from("MULTISAMPLED"));
        }
        RenderPipelineDescriptor {
            label: Some("underwater_pipeline".into()),
            layout: vec![if multisampled {
                self.layout_msaa.clone()
            } else {
                self.layout.clone()
            }],
            vertex: self.fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: self.fragment_shader.clone(),
                shader_defs,
                targets: vec![Some(ColorTargetState {
                    format: key.target_format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        }
    }
}

#[derive(Component)]
struct UnderwaterPipelineId(CachedRenderPipelineId);

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn prepare_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<UnderwaterPipeline>>,
    pipeline: Res<UnderwaterPipeline>,
    views: Query<(Entity, &ExtractedView, &Msaa, Has<UnderwaterView>)>,
) {
    for (entity, view, msaa, under) in &views {
        if !under {
            commands.entity(entity).remove::<UnderwaterPipelineId>();
            continue;
        }
        let id = pipelines.specialize(
            &pipeline_cache,
            &pipeline,
            UnderwaterPipelineKey {
                target_format: view.target_format,
                samples: msaa.samples(),
            },
        );
        commands.entity(entity).insert(UnderwaterPipelineId(id));
    }
}

/// The pass reads the scene's depth, so the depth texture must be bindable
/// while the camera is under the surface.
fn depth_readable(mut cameras: Query<&mut Camera3d, With<UnderwaterView>>) {
    for mut camera in &mut cameras {
        camera.depth_texture_usages.0 |= TextureUsages::TEXTURE_BINDING.bits();
    }
}

/// What [`draw`] reads of the view.
type Drawn = (
    &'static ViewTarget,
    &'static UnderwaterPipelineId,
    &'static DynamicUniformIndex<UnderwaterView>,
    &'static ViewUniformOffset,
    &'static ViewDepthTexture,
    &'static Msaa,
);

#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
fn draw(
    view: ViewQuery<Drawn>,
    pipeline: Res<UnderwaterPipeline>,
    pipeline_cache: Res<PipelineCache>,
    settings: Res<ComponentUniforms<UnderwaterView>>,
    view_uniforms: Res<ViewUniforms>,
    mut ctx: RenderContext,
) {
    let (target, pipeline_id, settings_index, view_offset, depth, msaa) = view.into_inner();
    let Some(render_pipeline) = pipeline_cache.get_render_pipeline(pipeline_id.0) else {
        return;
    };
    let (Some(settings_binding), Some(view_binding)) = (
        settings.uniforms().binding(),
        view_uniforms.uniforms.binding(),
    ) else {
        return;
    };
    let post_process = target.post_process_write();
    let layout = if msaa.samples() > 1 {
        &pipeline.layout_msaa
    } else {
        &pipeline.layout
    };
    let bind_group = ctx.render_device().create_bind_group(
        Some("underwater_bind_group"),
        &pipeline_cache.get_bind_group_layout(layout),
        &BindGroupEntries::sequential((
            post_process.source,
            &pipeline.sampler,
            settings_binding,
            view_binding,
            depth.view(),
        )),
    );
    let mut pass = ctx
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("underwater_pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: post_process.destination,
                depth_slice: None,
                resolve_target: None,
                ops: Operations::default(),
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    pass.set_pipeline(render_pipeline);
    pass.set_bind_group(
        0,
        &bind_group,
        &[settings_index.index(), view_offset.offset],
    );
    pass.draw(0..3, 0..1);
}
