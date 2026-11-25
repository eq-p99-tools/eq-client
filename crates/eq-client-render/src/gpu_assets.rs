//! Converts decoded asset data into Bevy images, meshes and materials.
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat};
use eq_client_assets::{
    BlendOpacity, MaterialMode, ObjectMaterial, TerrainMaterial, TextureAnimation, ZonePrimitive,
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
    /// A terrain surface's material for each frame, made with the zone: the
    /// surface swaps between them, and none of them changes. The renderer
    /// prepares a changed material again, and hundreds of surfaces animate
    /// in a zone like Qeynos. Empty for an object, whose shared material
    /// takes each frame's image instead.
    materials: Vec<Handle<StandardMaterial>>,
    frame_millis: std::num::NonZeroU32,
    elapsed: std::time::Duration,
    next_frame_millis: u128,
    skip_frames: bool,
    frame: usize,
}

impl TerrainAnimation {
    /// An animation through `frames`, a frame each second.
    #[cfg(test)]
    pub(super) fn testing(frames: Vec<Handle<Image>>) -> Self {
        Self {
            frames,
            materials: Vec::new(),
            frame_millis: std::num::NonZeroU32::new(1000).unwrap(),
            elapsed: std::time::Duration::ZERO,
            next_frame_millis: 1000,
            skip_frames: false,
            frame: 0,
        }
    }

    /// Steps the clock, giving the frame it moves to.
    fn advance(&mut self, delta: std::time::Duration) -> Option<usize> {
        self.elapsed = self.elapsed.saturating_add(delta);
        let now = self.elapsed.as_millis();
        if now <= self.next_frame_millis {
            return None;
        }
        let interval = u128::from(self.frame_millis.get());
        // The Titanium and EQMac graphics libraries' terrain material tick
        // advances once unless the texture permits skipping (program read).
        // The deadline comparison is strict.
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
        Some(frame)
    }
}

/// A zone-owned timer drives the shared object material and its reflected copy.
/// Sharing their phase is an inferred presentation choice until native object
/// phase ownership is read.
#[derive(Component)]
pub(super) struct ObjectAnimation {
    animation: TerrainAnimation,
    targets: Vec<Handle<StandardMaterial>>,
}

impl ObjectAnimation {
    /// Reflection changes culling alone; both materials take each frame together.
    pub(super) fn include_mirrored(
        &mut self,
        mirrored: &HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    ) {
        if let Some(material) = mirrored.get(&self.targets[0].id()) {
            self.targets.push(material.clone());
        }
    }
}

/// The shared meshes/materials and their timers for one zone object model.
pub(super) struct ObjectDraws {
    pub primitives: crate::doors::Primitives,
    pub animations: Vec<ObjectAnimation>,
}

/// One terrain primitive's draw: the authored material and optional
/// animation, and the mesh it is drawn in, which surfaces merged with it
/// share.
pub(super) struct TerrainDraw {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    pub animation: Option<TerrainAnimation>,
    /// Where its vertices start in the mesh: 0 for a mesh of its own.
    pub offset: usize,
    /// Whether it spawns the mesh's entity: the first of a merged mesh's
    /// surfaces does, the rest are drawn by it.
    pub spawn: bool,
}

/// The side of a square cell on the ground in which still terrain surfaces
/// of one material merge into one mesh, in world units. Each merged mesh is
/// one entity the renderer culls, prepares and draws, instead of one per
/// surface; a smaller cell keeps the mesh a carried light rewrites small.
pub(super) const TERRAIN_CELL: f32 = 256.0;

/// The surfaces a merged mesh holds: one material in one cell.
type Group = (AssetId<StandardMaterial>, i32, i32);

/// Terrain materials by what makes one: the blend, the texture and, when
/// authored, the sides. Surfaces alike share a material, as do the frames
/// of animated surfaces alike.
type TerrainMaterials =
    HashMap<(MaterialMode, Option<usize>, Option<bool>), Handle<StandardMaterial>>;

/// Builds terrain using the material's own sidedness. Opposite-wound faces
/// may occupy the same plane: each is visible only from its authored side.
/// Keep the existing behavior for callers without source material metadata.
#[cfg(test)]
pub(super) fn create_terrain_primitives(
    primitives: Vec<ZonePrimitive>,
    authored: &[TerrainMaterial],
    texture_images: &[TextureImages],
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Vec<TerrainDraw> {
    create_merged_terrain(
        primitives,
        authored,
        (&[], None),
        texture_images,
        (meshes, materials),
    )
}

/// What a terrain primitive is drawn in: a mesh of its own, or a part of
/// its group's, from a vertex on.
enum Geometry {
    Own(ZonePrimitive),
    Part(Group, usize),
}

/// Builds terrain using the material's own sidedness, merging the still,
/// opaque or masked surfaces of one material in each `cell` into one mesh.
/// `apart[i]` keeps primitive `i` in a mesh of its own, as Better Water
/// dresses a water surface alone. Blended and additive surfaces stay apart
/// too: they are sorted by distance, and a merged mesh would sort as one.
/// The draws come back one per drawn primitive, in order.
pub(super) fn create_merged_terrain(
    primitives: Vec<ZonePrimitive>,
    authored: &[TerrainMaterial],
    (apart, cell): (&[bool], Option<f32>),
    texture_images: &[TextureImages],
    (meshes, materials): (&mut Assets<Mesh>, &mut Assets<StandardMaterial>),
) -> Vec<TerrainDraw> {
    let mut shared = TerrainMaterials::new();
    let mut material = |mode, texture: Option<usize>, sides| {
        let texture = texture.filter(|index| texture_images.get(*index).is_some());
        shared
            .entry((mode, texture, sides))
            .or_insert_with(|| {
                let texture = texture.and_then(|index| texture_images.get(index));
                let material = create_material(mode, texture, materials, true);
                if let Some(double_sided) = sides {
                    let mut value = materials.get_mut(&material).expect("just added material");
                    value.double_sided = double_sided;
                    value.cull_mode = (!double_sided).then_some(Face::Back);
                }
                material
            })
            .clone()
    };
    let drawn: Vec<Unmerged> = primitives
        .into_iter()
        .enumerate()
        .filter(|(_, primitive)| drawn(primitive))
        .map(|(index, primitive)| {
            let (mode, texture) = (primitive.material_mode, primitive.texture);
            let source = authored.get(index);
            let sides = source.map(|source| source.double_sided);
            let animation = source
                .and_then(|source| source.animation.as_ref())
                .and_then(|source| {
                    let frames = frame_images(source, mode, texture_images)?;
                    Some(TerrainAnimation {
                        frames,
                        materials: source
                            .frames
                            .iter()
                            .map(|frame| material(mode, Some(*frame), sides))
                            .collect(),
                        ..TerrainAnimation::starting(source)
                    })
                });
            let material = animation.as_ref().map_or_else(
                || material(mode, texture, sides),
                |animation| animation.materials[0].clone(),
            );
            let group = cell
                .filter(|_| {
                    animation.is_none()
                        && !apart.get(index).copied().unwrap_or(false)
                        && matches!(mode, MaterialMode::Opaque | MaterialMode::Masked)
                        && primitive.normals.len() == primitive.positions.len()
                        && primitive.texture_coordinates.len() == primitive.positions.len()
                })
                .map(|cell| {
                    let (x, z) = centre(&primitive.positions);
                    #[allow(clippy::cast_possible_truncation)] // Zone coordinates.
                    let at = ((x / cell).floor() as i32, (z / cell).floor() as i32);
                    (material.id(), at.0, at.1)
                });
            (primitive, material, animation, group)
        })
        .collect();
    merge(drawn, meshes)
}

/// A drawn terrain primitive before it is merged: its geometry, material,
/// animation, and the group it may merge in.
type Unmerged = (
    ZonePrimitive,
    Handle<StandardMaterial>,
    Option<TerrainAnimation>,
    Option<Group>,
);

/// Merges each group of more than one surface into one mesh, and gives the
/// rest meshes of their own; one draw per surface, in order.
fn merge(drawn: Vec<Unmerged>, meshes: &mut Assets<Mesh>) -> Vec<TerrainDraw> {
    let mut members: HashMap<Group, usize> = HashMap::new();
    for (_, _, _, group) in &drawn {
        if let Some(group) = group {
            *members.entry(*group).or_default() += 1;
        }
    }
    let mut merged: HashMap<Group, ZonePrimitive> = HashMap::new();
    let placed: Vec<_> = drawn
        .into_iter()
        .map(|(primitive, material, animation, group)| {
            let geometry = match group.filter(|group| members[group] > 1) {
                Some(group) => {
                    let whole = merged.entry(group).or_insert_with(|| ZonePrimitive {
                        positions: Vec::new(),
                        normals: Vec::new(),
                        texture_coordinates: Vec::new(),
                        indices: Vec::new(),
                        texture: primitive.texture,
                        material_mode: primitive.material_mode,
                        light: Vec::new(),
                    });
                    let offset = whole.positions.len();
                    let base = u32::try_from(offset).expect("a zone's vertices fit a u32 index");
                    whole
                        .indices
                        .extend(primitive.indices.iter().map(|index| index + base));
                    whole.positions.extend(primitive.positions);
                    whole.normals.extend(primitive.normals);
                    whole
                        .texture_coordinates
                        .extend(primitive.texture_coordinates);
                    Geometry::Part(group, offset)
                }
                None => Geometry::Own(primitive),
            };
            (geometry, material, animation)
        })
        .collect();
    // Kept in the main world too, where the zone light colours their
    // vertices.
    let merged: HashMap<Group, Handle<Mesh>> = merged
        .into_iter()
        .map(|(group, whole)| {
            (
                group,
                meshes.add(create_mesh(whole, RenderAssetUsages::default())),
            )
        })
        .collect();
    placed
        .into_iter()
        .map(|(geometry, material, animation)| {
            let (mesh, offset) = match geometry {
                Geometry::Own(primitive) => (
                    meshes.add(create_mesh(primitive, RenderAssetUsages::default())),
                    0,
                ),
                Geometry::Part(group, offset) => (merged[&group].clone(), offset),
            };
            TerrainDraw {
                mesh,
                material,
                animation,
                offset,
                spawn: offset == 0,
            }
        })
        .collect()
}

/// The middle of a surface's box on the ground.
fn centre(positions: &[[f32; 3]]) -> (f32, f32) {
    let (low, high) = positions.iter().fold(
        ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]),
        |(low, high), position| {
            (
                [low[0].min(position[0]), low[1].min(position[2])],
                [high[0].max(position[0]), high[1].max(position[2])],
            )
        },
    );
    (
        f32::midpoint(low[0], high[0]),
        f32::midpoint(low[1], high[1]),
    )
}

/// An animated surface's plain material: drawn, or parked while Rich
/// Lighting draws the surface.
type AnimatedSurface = AnyOf<(
    &'static mut MeshMaterial3d<StandardMaterial>,
    &'static mut crate::rich_light::Parked,
)>;

/// The sequence's images in the draw's blend mode, when it has more than
/// one frame and every image is there.
fn frame_images(
    source: &TextureAnimation,
    mode: MaterialMode,
    texture_images: &[TextureImages],
) -> Option<Vec<Handle<Image>>> {
    let frames: Option<Vec<_>> = source
        .frames
        .iter()
        .map(|index| {
            let images = texture_images.get(*index)?;
            Some(match mode {
                MaterialMode::Masked | MaterialMode::Additive => images.masked.clone(),
                MaterialMode::Opaque | MaterialMode::Blended(_) => images.opaque.clone(),
            })
        })
        .collect();
    frames.filter(|frames| frames.len() > 1)
}

impl TerrainAnimation {
    /// The sequence's clock at its first frame, with no frames yet.
    fn starting(source: &TextureAnimation) -> Self {
        Self {
            frames: Vec::new(),
            materials: Vec::new(),
            frame_millis: source.frame_millis,
            elapsed: std::time::Duration::ZERO,
            next_frame_millis: u128::from(source.frame_millis.get()),
            skip_frames: source.skip_frames,
            frame: 0,
        }
    }
}

/// Realizes the complete sequence in the draw's blend mode and starts the
/// object's shared material at its first frame. Missing images keep the
/// existing still material.
fn material_animation(
    source: &TextureAnimation,
    mode: MaterialMode,
    texture_images: &[TextureImages],
    material: &Handle<StandardMaterial>,
    materials: &mut Assets<StandardMaterial>,
) -> Option<TerrainAnimation> {
    let frames = frame_images(source, mode, texture_images)?;
    materials.get_mut(material)?.base_color_texture = Some(frames[0].clone());
    Some(TerrainAnimation {
        frames,
        ..TerrainAnimation::starting(source)
    })
}

/// A terrain primitive's full-colour texture frames, in animation order,
/// and how long each shows: the still texture alone, and no length, when
/// it doesn't animate. Better Water draws from these whatever the
/// primitive's own blend, so water cut out by a colour key shows whole.
pub(super) fn opaque_frames(
    texture: Option<usize>,
    authored: Option<&TerrainMaterial>,
    texture_images: &[TextureImages],
) -> (Vec<Handle<Image>>, Option<std::num::NonZeroU32>) {
    let animated = authored
        .and_then(|source| source.animation.as_ref())
        .and_then(|animation| {
            let frames: Option<Vec<_>> = animation
                .frames
                .iter()
                .map(|index| {
                    texture_images
                        .get(*index)
                        .map(|images| images.opaque.clone())
                })
                .collect();
            frames
                .filter(|frames| frames.len() > 1)
                .map(|frames| (frames, Some(animation.frame_millis)))
        });
    animated.unwrap_or_else(|| {
        (
            texture
                .and_then(|index| texture_images.get(index))
                .map(|images| images.opaque.clone())
                .into_iter()
                .collect(),
            None,
        )
    })
}

/// Moves each animated terrain surface to its material for the frame its
/// WLD animation advanced to; the materials keep the alpha mode, color,
/// sidedness and depth behavior of the surface.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters.
pub(super) fn animate_terrain(
    time: Res<Time>,
    mut terrain: Query<(AnimatedSurface, &mut TerrainAnimation)>,
) {
    for ((drawn, parked), mut animation) in &mut terrain {
        let Some(material) = animation
            .advance(time.delta())
            .and_then(|frame| animation.materials.get(frame).cloned())
        else {
            continue;
        };
        // A surface Rich Lighting draws keeps its plain material parked,
        // which its rich one follows.
        let mut current = match (drawn, parked) {
            (Some(drawn), _) => drawn.map_unchanged(|drawn| &mut drawn.0),
            (None, Some(parked)) => parked.map_unchanged(|parked| &mut parked.0),
            (None, None) => continue,
        };
        current.set_if_neq(material);
    }
}

/// Advances each shared object material once, whatever its placement count.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn animate_objects(
    time: Res<Time>,
    mut objects: Query<&mut ObjectAnimation>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for mut object in &mut objects {
        if let Some(frame) = object.animation.advance(time.delta()) {
            let image = object.animation.frames[frame].clone();
            for handle in &object.targets {
                if let Some(mut material) = materials.get_mut(handle) {
                    material.base_color_texture = Some(image.clone());
                }
            }
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

/// Builds static zone object draws and one timer per animated shared material.
/// Other actor renderers retain their existing still-image behavior.
pub(super) fn create_zone_object_primitives(
    primitives: Vec<ZonePrimitive>,
    authored: &[ObjectMaterial],
    texture_images: &[TextureImages],
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> ObjectDraws {
    let sources: Vec<_> = primitives
        .iter()
        .enumerate()
        .filter(|(_, primitive)| drawn(primitive))
        .map(|(index, primitive)| {
            (
                primitive.material_mode,
                authored
                    .get(index)
                    .and_then(|source| source.animation.as_ref()),
            )
        })
        .collect();
    let primitives = create_render_primitives(
        primitives,
        authored,
        texture_images,
        meshes,
        materials,
        true,
        RenderAssetUsages::default(),
    );
    let animations = primitives
        .iter()
        .zip(sources)
        .filter_map(|((_, material), (mode, source))| {
            Some(ObjectAnimation {
                animation: material_animation(source?, mode, texture_images, material, materials)?,
                targets: vec![material.clone()],
            })
        })
        .collect();
    ObjectDraws {
        primitives,
        animations,
    }
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
            light: Vec::new(),
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
        let entity = app
            .world_mut()
            .spawn((MeshMaterial3d(draw.material), draw.animation.unwrap()))
            .id();
        let mut seen = Vec::new();
        for (delta, expected) in [(100, 0), (1, 1), (250, 0), (100, 0), (1, 0), (1000, 1)] {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(Duration::from_millis(delta));
            app.update();
            let handle = app
                .world()
                .get::<MeshMaterial3d<StandardMaterial>>(entity)
                .unwrap()
                .0
                .clone();
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
            seen.push(handle);
        }
        // The surface swaps between materials, and no material changes:
        // the renderer would prepare a changed one again.
        let materials = app.world().resource::<Assets<StandardMaterial>>();
        assert_eq!(materials.len(), 2);
        assert_ne!(seen[0], seen[1]);
        assert_eq!(
            materials.get(&seen[0]).unwrap().base_color_texture,
            Some(textures[0].opaque.clone())
        );
        app.world_mut().despawn(entity);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(100));
        app.update();
        let mut terrain = app.world_mut().query::<&TerrainAnimation>();
        assert_eq!(terrain.iter(app.world()).count(), 0);
    }

    #[test]
    fn still_surfaces_of_a_material_merge_in_their_cell_and_keep_their_place() {
        let mut images = Assets::<Image>::default();
        let textures = textures(&mut images);
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let moved = |mode, x: f32| {
            let mut primitive = primitive(mode);
            for position in &mut primitive.positions {
                position[0] += x;
            }
            primitive
        };
        let still = TerrainMaterial {
            double_sided: false,
            animation: None,
        };
        let draws = create_merged_terrain(
            vec![
                moved(MaterialMode::Opaque, 0.0),
                moved(MaterialMode::Blended(BlendOpacity::Half), 0.0),
                moved(MaterialMode::Opaque, 10.0),
                // Water, kept apart.
                moved(MaterialMode::Opaque, 20.0),
                // Another cell.
                moved(MaterialMode::Opaque, 300.0),
                // Animated.
                moved(MaterialMode::Opaque, 30.0),
                moved(MaterialMode::Opaque, 40.0),
            ],
            &[
                still.clone(),
                still.clone(),
                still.clone(),
                still.clone(),
                still.clone(),
                authored(false),
                still,
            ],
            (&[false, false, false, true], Some(TERRAIN_CELL)),
            &textures,
            (&mut meshes, &mut materials),
        );
        assert_eq!(draws.len(), 7);
        let merged = &draws[0].mesh;
        let parts: Vec<_> = draws
            .iter()
            .map(|draw| (&draw.mesh == merged, draw.offset, draw.spawn))
            .collect();
        assert_eq!(
            parts,
            [
                (true, 0, true),
                (false, 0, true),
                (true, 3, false),
                (false, 0, true),
                (false, 0, true),
                (false, 0, true),
                (true, 6, false),
            ]
        );
        let mesh = meshes.get(merged).unwrap();
        assert_eq!(mesh.count_vertices(), 9);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        // The third surface's first vertex, where its draw says it starts.
        assert_eq!(positions[3], [10.0, 0.0, 0.0]);
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("indices");
        };
        assert_eq!(indices[6..12], [3, 4, 5, 5, 4, 3]);
        // Five meshes for seven surfaces.
        assert_eq!(meshes.len(), 5);
    }

    #[test]
    fn surfaces_alike_share_one_material() {
        let mut images = Assets::<Image>::default();
        let textures = textures(&mut images);
        let mut materials = Assets::<StandardMaterial>::default();
        let still = TerrainMaterial {
            double_sided: false,
            animation: None,
        };
        let draws = create_terrain_primitives(
            vec![
                primitive(MaterialMode::Opaque),
                primitive(MaterialMode::Opaque),
                primitive(MaterialMode::Masked),
                primitive(MaterialMode::Opaque),
            ],
            &[still.clone(), still.clone(), still, authored(true)],
            &textures,
            &mut Assets::<Mesh>::default(),
            &mut materials,
        );
        assert_eq!(draws[0].material, draws[1].material);
        assert_ne!(draws[0].material, draws[2].material);
        assert_ne!(draws[0].material, draws[3].material);
        // Opaque one-sided, masked one-sided, and the two frames two-sided.
        assert_eq!(materials.len(), 4);
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
        let advance = |animation: &mut TerrainAnimation, millis| {
            animation
                .advance(Duration::from_millis(millis))
                .map(|frame| animation.frames[frame].clone())
        };
        assert!(advance(&mut animation, 100).is_none());
        assert_eq!(advance(&mut animation, 1), Some(textures[1].opaque.clone()));
        assert_eq!(
            advance(&mut animation, 249),
            Some(textures[0].opaque.clone())
        );
        assert_eq!(animation.next_frame_millis, 400);
        assert!(advance(&mut animation, 50).is_none());
        assert_eq!(advance(&mut animation, 1), Some(textures[1].opaque.clone()));
        // The program's large-delay path uses division above sixteen intervals.
        assert_eq!(
            advance(&mut animation, 1700),
            Some(textures[0].opaque.clone())
        );
        assert_eq!(animation.next_frame_millis, 2200);
    }
}
