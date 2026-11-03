//! Converts decoded asset data into Bevy images, meshes and materials.
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat};
use eq_client_assets::{
    BlendOpacity, MaterialMode, ObjectMaterial, TerrainMaterial, ZonePrimitive,
};
use image::{RgbaImage, imageops::FilterType};
use std::collections::HashMap;
pub(super) struct TextureImages {
    opaque: Handle<Image>,
    masked: Handle<Image>,
}

/// Texture swaps belong to the drawn terrain entity, so leaving a zone drops
/// its animation and image handles together with its material.
#[derive(Component)]
pub(super) struct TerrainAnimation {
    frames: Vec<Handle<Image>>,
    frame_millis: std::num::NonZeroU32,
    elapsed: std::time::Duration,
    next_frame_millis: u128,
    skip_frames: bool,
    frame: usize,
}

impl TerrainAnimation {
    fn advance(&mut self, delta: std::time::Duration) -> Option<Handle<Image>> {
        self.elapsed = self.elapsed.saturating_add(delta);
        let now = self.elapsed.as_millis();
        if now <= self.next_frame_millis {
            return None;
        }
        let interval = u128::from(self.frame_millis.get());
        // The terrain material tick at EQGfx_Dx8.dll 0x10040270 (Titanium)
        // and eqgfx_dx8.dll 0x100508d0 (EQMac) advances once unless the
        // texture permits skipping. The deadline comparison is strict.
        let steps = if self.skip_frames {
            let late = now - self.next_frame_millis;
            let steps = if late > interval * 16 {
                late / interval + 1
            } else {
                late.div_ceil(interval)
            };
            self.next_frame_millis = self.next_frame_millis.saturating_add(steps * interval);
            steps
        } else {
            self.next_frame_millis = now.saturating_add(interval);
            1
        };
        let frame = (self.frame as u128 + steps) % self.frames.len() as u128;
        let frame = usize::try_from(frame).ok()?;
        if frame == self.frame {
            return None;
        }
        self.frame = frame;
        self.frames.get(frame).cloned()
    }
}

/// One terrain draw call with the authored material and optional animation.
pub(super) struct TerrainDraw {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    pub animation: Option<TerrainAnimation>,
}

/// Builds terrain using the material's own sidedness. Opposite-wound faces
/// may occupy the same plane: each is visible only from its authored side.
/// Keep the existing behavior for callers without source material metadata.
pub(super) fn create_terrain_primitives(
    primitives: Vec<ZonePrimitive>,
    authored: &[TerrainMaterial],
    texture_images: &[TextureImages],
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Vec<TerrainDraw> {
    primitives
        .into_iter()
        .enumerate()
        .filter(|(_, primitive)| drawn(primitive))
        .map(|(index, primitive)| {
            let texture = primitive
                .texture
                .and_then(|index| texture_images.get(index));
            let material = create_material(primitive.material_mode, texture, materials, true);
            let source = authored.get(index);
            if let Some(source) = source {
                let mut value = materials.get_mut(&material).expect("just added material");
                value.double_sided = source.double_sided;
                value.cull_mode = (!source.double_sided).then_some(Face::Back);
            }
            let animation = source.and_then(|source| {
                let animation = source.animation.as_ref()?;
                let frames: Option<Vec<_>> = animation
                    .frames
                    .iter()
                    .map(|index| {
                        let images = texture_images.get(*index)?;
                        Some(match primitive.material_mode {
                            MaterialMode::Masked | MaterialMode::Additive => images.masked.clone(),
                            MaterialMode::Opaque | MaterialMode::Blended(_) => {
                                images.opaque.clone()
                            }
                        })
                    })
                    .collect();
                let frames = frames.filter(|frames| frames.len() > 1)?;
                materials.get_mut(&material)?.base_color_texture = Some(frames[0].clone());
                Some(TerrainAnimation {
                    frames,
                    frame_millis: animation.frame_millis,
                    elapsed: std::time::Duration::ZERO,
                    next_frame_millis: u128::from(animation.frame_millis.get()),
                    skip_frames: animation.skip_frames,
                    frame: 0,
                })
            });
            TerrainDraw {
                mesh: meshes.add(create_mesh(primitive, RenderAssetUsages::RENDER_WORLD)),
                material,
                animation,
            }
        })
        .collect()
}

/// Swaps only terrain textures whose WLD animation advanced, retaining the
/// material's alpha mode, color, sidedness and depth behavior on every frame.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
pub(super) fn animate_terrain(
    time: Res<Time>,
    mut terrain: Query<(&MeshMaterial3d<StandardMaterial>, &mut TerrainAnimation)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (handle, mut animation) in &mut terrain {
        if let Some(image) = animation.advance(time.delta())
            && let Some(mut material) = materials.get_mut(&handle.0)
        {
            material.base_color_texture = Some(image);
        }
    }
}

pub(super) fn create_texture_images(
    textures: Vec<eq_client_assets::ZoneTexture>,
    images: &mut Assets<Image>,
) -> Vec<TextureImages> {
    textures
        .into_iter()
        .map(|texture| {
            let mut masked_pixels = texture.rgba8.clone();
            let color_key = texture.color_key.unwrap_or([0, 0, 0]);
            for pixel in masked_pixels.as_chunks_mut::<4>().0 {
                if pixel[..3] == color_key {
                    pixel[3] = 0;
                }
            }
            TextureImages {
                opaque: images.add(create_image(texture.width, texture.height, texture.rgba8)),
                masked: images.add(create_image(texture.width, texture.height, masked_pixels)),
            }
        })
        .collect()
}

pub(super) fn create_image(width: u32, height: u32, pixels: Vec<u8>) -> Image {
    let (pixels, mip_level_count) = generate_mip_chain(width, height, pixels);
    let base_level_len = width as usize * height as usize * 4;
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels[..base_level_len].to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(pixels);
    image.texture_descriptor.mip_level_count = mip_level_count;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

pub(super) fn generate_mip_chain(width: u32, height: u32, pixels: Vec<u8>) -> (Vec<u8>, u32) {
    let mut level = RgbaImage::from_raw(width, height, pixels)
        .expect("decoded RGBA texture dimensions match its pixel buffer");
    let mut chain = level.as_raw().clone();
    let mut level_count = 1;
    while level.width() > 1 || level.height() > 1 {
        let next_width = (level.width() / 2).max(1);
        let next_height = (level.height() / 2).max(1);
        level = image::imageops::resize(&level, next_width, next_height, FilterType::Triangle);
        chain.extend_from_slice(level.as_raw());
        level_count += 1;
    }
    (chain, level_count)
}

/// Builds a model's or zone's draw calls. `usage` says where the meshes' data
/// lives: zones draw only, while models hung under a spawn keep theirs in
/// the main world too, where clicks are picked against them.
/// `authored` follows the input primitives before empty geometry is filtered.
/// Callers without object material metadata keep the existing two-sided draw.
pub(super) fn create_render_primitives(
    primitives: Vec<ZonePrimitive>,
    authored: &[ObjectMaterial],
    texture_images: &[TextureImages],
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    unlit: bool,
    usage: RenderAssetUsages,
) -> Vec<(Handle<Mesh>, Handle<StandardMaterial>)> {
    primitives
        .into_iter()
        .enumerate()
        .filter(|(_, primitive)| drawn(primitive))
        .map(|(index, primitive)| {
            let texture = primitive
                .texture
                .and_then(|index| texture_images.get(index));
            let material = create_material(primitive.material_mode, texture, materials, unlit);
            if let Some(source) = authored.get(index) {
                let mut value = materials.get_mut(&material).expect("just added material");
                value.double_sided = source.double_sided;
                value.cull_mode = (!source.double_sided).then_some(Face::Back);
            }
            (meshes.add(create_mesh(primitive, usage)), material)
        })
        .collect()
}

/// A reflected placement reverses raster winding. Unbatched objects keep their
/// transform for transparency sorting, so reverse their cull face in a shared
/// variant rather than changing the source material or duplicating geometry.
pub(super) fn mirrored_material(
    handle: &Handle<StandardMaterial>,
    materials: &mut Assets<StandardMaterial>,
    cache: &mut HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
) -> Handle<StandardMaterial> {
    let Some(source) = materials.get(handle) else {
        return handle.clone();
    };
    let Some(face) = source.cull_mode else {
        return handle.clone();
    };
    if let Some(mirrored) = cache.get(&handle.id()) {
        return mirrored.clone();
    }
    let mut mirrored = source.clone();
    mirrored.cull_mode = Some(match face {
        Face::Back => Face::Front,
        Face::Front => Face::Back,
    });
    let mirrored = materials.add(mirrored);
    cache.insert(handle.id(), mirrored.clone());
    mirrored
}

/// A primitive's geometry as a Bevy mesh; its texture and material are the
/// caller's.
pub(super) fn create_mesh(primitive: ZonePrimitive, usage: RenderAssetUsages) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, usage);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, primitive.positions);
    if !primitive.normals.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, primitive.normals);
    }
    if !primitive.texture_coordinates.is_empty() {
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            bevy_texture_coordinates(primitive.texture_coordinates),
        );
    }
    mesh.insert_indices(Indices::U32(primitive.indices));
    mesh
}

/// Whether a primitive has any geometry to draw.
pub(super) fn drawn(primitive: &ZonePrimitive) -> bool {
    !primitive.indices.is_empty() && !primitive.positions.is_empty()
}

/// Converts classic WLD's downward-growing V axis to Bevy's texture orientation.
pub(super) fn bevy_texture_coordinates(coordinates: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    coordinates.into_iter().map(|[u, v]| [u, -v]).collect()
}

pub(super) fn create_material(
    mode: MaterialMode,
    texture: Option<&TextureImages>,
    materials: &mut Assets<StandardMaterial>,
    unlit: bool,
) -> Handle<StandardMaterial> {
    let (image, color, alpha_mode) = match mode {
        MaterialMode::Opaque => (
            texture.map(|images| images.opaque.clone()),
            Color::WHITE,
            AlphaMode::Opaque,
        ),
        MaterialMode::Masked => (
            texture.map(|images| images.masked.clone()),
            Color::WHITE,
            AlphaMode::Mask(0.5),
        ),
        MaterialMode::Blended(opacity) => {
            let alpha = match opacity {
                BlendOpacity::Quarter => 0.25,
                BlendOpacity::Half => 0.5,
                BlendOpacity::ThreeQuarters => 0.75,
            };
            (
                texture.map(|images| images.opaque.clone()),
                Color::srgba(1.0, 1.0, 1.0, alpha),
                AlphaMode::Blend,
            )
        }
        MaterialMode::Additive => (
            texture.map(|images| images.masked.clone()),
            Color::WHITE,
            AlphaMode::Add,
        ),
    };
    materials.add(StandardMaterial {
        base_color: color,
        base_color_texture: image,
        alpha_mode,
        unlit,
        perceptual_roughness: 0.95,
        double_sided: true,
        cull_mode: None,
        ..default()
    })
}

#[cfg(test)]
mod terrain_tests {
    use super::*;
    use eq_client_assets::TextureAnimation;
    use std::num::NonZeroU32;
    use std::time::Duration;

    fn primitive(mode: MaterialMode) -> ZonePrimitive {
        ZonePrimitive {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            texture_coordinates: vec![[0.0, 0.0]; 3],
            // Both authored sides remain in the mesh; raster culling selects one.
            indices: vec![0, 1, 2, 2, 1, 0],
            texture: Some(0),
            material_mode: mode,
        }
    }

    fn textures(images: &mut Assets<Image>) -> Vec<TextureImages> {
        create_texture_images(
            [[20, 40, 60], [60, 40, 20]]
                .into_iter()
                .map(|rgb| eq_client_assets::ZoneTexture {
                    name: "synthetic.png".to_owned(),
                    width: 1,
                    height: 1,
                    rgba8: vec![rgb[0], rgb[1], rgb[2], 255],
                    color_key: Some(rgb),
                })
                .collect(),
            images,
        )
    }

    fn authored(double_sided: bool) -> TerrainMaterial {
        TerrainMaterial {
            double_sided,
            animation: Some(TextureAnimation {
                frames: vec![0, 1, 0],
                frame_millis: NonZeroU32::new(100).unwrap(),
                skip_frames: false,
            }),
        }
    }

    #[test]
    fn terrain_preserves_authored_sides_geometry_and_blend_modes() {
        let mut images = Assets::<Image>::default();
        let textures = textures(&mut images);
        for texture in &textures {
            assert_eq!(
                images.get(&texture.opaque).unwrap().data.as_ref().unwrap()[3],
                255
            );
            assert_eq!(
                images.get(&texture.masked).unwrap().data.as_ref().unwrap()[3],
                0
            );
        }
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        for (mode, alpha) in [
            (MaterialMode::Opaque, AlphaMode::Opaque),
            (MaterialMode::Masked, AlphaMode::Mask(0.5)),
            (MaterialMode::Blended(BlendOpacity::Half), AlphaMode::Blend),
            (MaterialMode::Additive, AlphaMode::Add),
        ] {
            for double_sided in [false, true] {
                let draws = create_terrain_primitives(
                    vec![primitive(mode)],
                    &[authored(double_sided)],
                    &textures,
                    &mut meshes,
                    &mut materials,
                );
                let draw = &draws[0];
                let material = materials.get(&draw.material).unwrap();
                assert_eq!(material.double_sided, double_sided);
                assert_eq!(material.cull_mode, (!double_sided).then_some(Face::Back));
                assert_eq!(material.alpha_mode, alpha);
                let mesh = meshes.get(&draw.mesh).unwrap();
                assert_eq!(mesh.indices().unwrap().len(), 6);
                let frames = &draw.animation.as_ref().unwrap().frames;
                let masked = matches!(mode, MaterialMode::Masked | MaterialMode::Additive);
                let expected = if masked {
                    &textures[1].masked
                } else {
                    &textures[1].opaque
                };
                assert_eq!(&frames[1], expected);
            }
        }
    }

    #[test]
    fn empty_primitives_do_not_shift_the_source_material_metadata() {
        let mut empty = primitive(MaterialMode::Opaque);
        empty.indices.clear();
        let mut materials = Assets::<StandardMaterial>::default();
        let draws = create_terrain_primitives(
            vec![empty, primitive(MaterialMode::Opaque)],
            &[authored(true), authored(false)],
            &[],
            &mut Assets::<Mesh>::default(),
            &mut materials,
        );
        assert_eq!(draws.len(), 1);
        assert_eq!(
            materials.get(&draws[0].material).unwrap().cull_mode,
            Some(Face::Back)
        );
        // The invalid texture references keep this material still.
        assert!(draws[0].animation.is_none());
    }

    #[test]
    fn delayed_updates_respect_no_skip_keep_material_state_and_end_with_the_entity() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, animate_terrain);
        let mut images = Assets::<Image>::default();
        let textures = textures(&mut images);
        let mut draws = create_terrain_primitives(
            vec![primitive(MaterialMode::Blended(BlendOpacity::Half))],
            &[authored(false)],
            &textures,
            &mut Assets::<Mesh>::default(),
            &mut app.world_mut().resource_mut::<Assets<StandardMaterial>>(),
        );
        let draw = draws.pop().unwrap();
        let handle = draw.material.clone();
        let entity = app
            .world_mut()
            .spawn((MeshMaterial3d(draw.material), draw.animation.unwrap()))
            .id();
        for (delta, expected) in [(100, 0), (1, 1), (250, 0), (100, 0), (1, 0), (1000, 1)] {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(delta));
            app.update();
            let materials = app.world().resource::<Assets<StandardMaterial>>();
            let material = materials.get(&handle).unwrap();
            assert_eq!(
                material.base_color_texture,
                Some(textures[expected].opaque.clone())
            );
            assert_eq!(material.alpha_mode, AlphaMode::Blend);
            assert_eq!(material.cull_mode, Some(Face::Back));
            assert!(!material.double_sided);
            assert!((material.base_color.alpha() - 0.5).abs() < f32::EPSILON);
        }
        app.world_mut().despawn(entity);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(100));
        app.update();
        let mut terrain = app.world_mut().query::<&TerrainAnimation>();
        assert_eq!(terrain.iter(app.world()).count(), 0);
    }

    #[test]
    fn texture_skipping_is_explicit_and_keeps_the_authored_deadline_phase() {
        let mut images = Assets::<Image>::default();
        let textures = textures(&mut images);
        let mut source = authored(false);
        source.animation.as_mut().unwrap().skip_frames = true;
        let mut draws = create_terrain_primitives(
            vec![primitive(MaterialMode::Opaque)],
            &[source],
            &textures,
            &mut Assets::<Mesh>::default(),
            &mut Assets::<StandardMaterial>::default(),
        );
        let mut animation = draws.pop().unwrap().animation.unwrap();
        assert!(animation.advance(Duration::from_millis(100)).is_none());
        assert_eq!(
            animation.advance(Duration::from_millis(1)),
            Some(textures[1].opaque.clone())
        );
        assert_eq!(
            animation.advance(Duration::from_millis(249)),
            Some(textures[0].opaque.clone())
        );
        assert_eq!(animation.next_frame_millis, 400);
        assert!(animation.advance(Duration::from_millis(50)).is_none());
        assert_eq!(
            animation.advance(Duration::from_millis(1)),
            Some(textures[1].opaque.clone())
        );
        // The program's large-delay path uses division above sixteen intervals.
        assert_eq!(
            animation.advance(Duration::from_millis(1700)),
            Some(textures[0].opaque.clone())
        );
        assert_eq!(animation.next_frame_millis, 2200);
    }
}
