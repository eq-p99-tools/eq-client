//! Converts decoded asset data into Bevy images, meshes and materials.
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use eq_client_assets::{BlendOpacity, MaterialMode, ZonePrimitive};
use image::{RgbaImage, imageops::FilterType};
pub(super) struct TextureImages {
    opaque: Handle<Image>,
    masked: Handle<Image>,
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
pub(super) fn create_render_primitives(
    primitives: Vec<ZonePrimitive>,
    texture_images: &[TextureImages],
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    unlit: bool,
    usage: RenderAssetUsages,
) -> Vec<(Handle<Mesh>, Handle<StandardMaterial>)> {
    primitives
        .into_iter()
        .filter(drawn)
        .map(|primitive| {
            let texture = primitive
                .texture
                .and_then(|index| texture_images.get(index));
            let material = create_material(primitive.material_mode, texture, materials, unlit);
            (meshes.add(create_mesh(primitive, usage)), material)
        })
        .collect()
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
