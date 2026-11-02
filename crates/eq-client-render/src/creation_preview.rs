//! A creation draft rendered into its own image, independent of the selected
//! character and its zone. Archive work runs off the frame thread.
use super::{background::Background, character};
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    mesh::skinning::SkinnedMeshInverseBindposes,
    prelude::*,
    render::render_resource::TextureFormat,
};
use eq_client_assets::characters::{
    CharacterAsset, CreationAppearance, STANDING_CLIP, load_creation_character,
};
use eq_client_core::creation::{Cosmetics, NewCharacter};

const LAYER: usize = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Look {
    race: u32,
    gender: u32,
    face: u8,
    cosmetics: Cosmetics,
    classic: bool,
}

#[derive(Resource, Default)]
pub(super) struct Preview {
    pub image: Handle<Image>,
    pub status: String,
    wanted: Option<Look>,
    job: u64,
    loading: Background<Result<CharacterAsset, String>>,
    pub angle: f32,
    pub close: bool,
    frames: [Frame; 2],
}

// Fit the posed geometry, including a head that projects ahead of its feet.
// A bounding sphere keeps the camera outside the model at every orbit angle.
#[derive(Clone, Copy, Default)]
struct Frame {
    center: Vec3,
    radius: f32,
}

impl Frame {
    fn points(points: impl Iterator<Item = Vec3>) -> Self {
        let (min, max) = points.fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(min, max), point| (min.min(point), max.max(point)),
        );
        if !min.is_finite() || !max.is_finite() {
            return Self::default();
        }
        Self {
            center: (min + max) * 0.5,
            radius: (max - min).length() * 0.5,
        }
    }

    fn of(asset: &CharacterAsset) -> [Self; 2] {
        let pose = asset.pose(STANDING_CLIP, 0.0);
        let rotation = Quat::from_rotation_y(eq_client_core::model_yaw());
        let lift = Vec3::Y * -asset.standing_bottom().unwrap_or_default();
        let points: Vec<_> = pose
            .iter()
            .zip(&asset.pieces)
            .filter(|(_, piece)| piece.base())
            .flat_map(|(pose, piece)| {
                pose.positions
                    .iter()
                    .map(move |position| (rotation * Vec3::from_array(*position) + lift, *piece))
            })
            .collect();
        let body = Self::points(points.iter().map(|(point, _)| *point));
        let head = asset.head_points(&asset.joints(STANDING_CLIP, 0.0, true));
        let face = if head.is_empty() {
            body
        } else {
            Self::points(head.into_iter().map(|point| rotation * point + lift))
        };
        [body, face]
    }
}

#[derive(Component)]
pub(super) struct Figure;
#[derive(Component)]
struct PortraitCamera;

pub(super) fn register(app: &mut App) {
    app.init_resource::<Preview>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            sync.in_set(super::Stage::Input)
                .after(super::character_create::input)
                .before(super::character_create::present),
        );
}

fn setup(mut commands: Commands, mut preview: ResMut<Preview>, mut images: ResMut<Assets<Image>>) {
    preview.image = images.add(Image::new_target_texture(
        420,
        520,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    commands.spawn((
        PortraitCamera,
        Camera3d::default(),
        Camera {
            order: -2,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.035, 0.045, 0.06)),
            ..default()
        },
        RenderTarget::Image(preview.image.clone().into()),
        RenderLayers::layer(LAYER),
        Transform::default(),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 6500.0,
            ..default()
        },
        RenderLayers::layer(LAYER),
        Transform::from_xyz(3.0, 8.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn look(character: &NewCharacter, classic: bool) -> Look {
    Look {
        race: character.race,
        gender: character.gender,
        face: character.face,
        cosmetics: character.cosmetics,
        classic,
    }
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn sync(
    mut commands: Commands,
    editor: Res<super::character_create::Editor>,
    settings: Res<super::ViewerSettings>,
    mut preview: ResMut<Preview>,
    figures: Query<Entity, With<Figure>>,
    mut cameras: Query<(&mut Camera, &mut Transform), With<PortraitCamera>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut binds: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let wanted = editor.preview().map(|(c, classic)| look(c, classic));
    if preview.wanted != wanted {
        preview.wanted.clone_from(&wanted);
        preview.loading.clear();
        preview.job += 1;
        preview.status = "Loading appearance...".into();
        for entity in &figures {
            commands.entity(entity).despawn();
        }
        if let Some(wanted) = wanted {
            let directory = settings.0.eq_directory.clone();
            let job = preview.job;
            preview.loading.start(job, move || {
                let directory = directory
                    .ok_or("Choose an EverQuest installation to preview your character.")?;
                let model = eq_client_core::races::model(wanted.race, wanted.gender)
                    .ok_or("No player model for this race")?;
                load_creation_character(
                    &directory,
                    model,
                    CreationAppearance {
                        race: wanted.race,
                        gender: wanted.gender,
                        face: wanted.face,
                        hair: wanted.cosmetics.hair_style,
                        hair_color: wanted.cosmetics.hair_color,
                        beard: wanted.cosmetics.beard,
                        beard_color: wanted.cosmetics.beard_color,
                        eyes: [wanted.cosmetics.eye_color_1, wanted.cosmetics.eye_color_2],
                        tattoo: wanted.cosmetics.woad,
                        classic: wanted.classic,
                    },
                )
                .map_err(|error| error.to_string())
            });
        }
    }
    let job = preview.job;
    if let Some(result) = preview.loading.poll(Some(job)) {
        match result {
            Ok(asset) => {
                let frames = Frame::of(&asset);
                if let Some(model) = character::prepare(
                    asset,
                    (&mut images, &mut meshes, &mut materials, &mut binds),
                ) {
                    preview.frames = frames;
                    let root = commands
                        .spawn((Figure, Transform::IDENTITY, Visibility::Visible))
                        .id();
                    character::spawn_on_layers(
                        &mut commands,
                        root,
                        &model,
                        0.0,
                        &RenderLayers::layer(LAYER),
                    );
                    preview.status.clear();
                } else {
                    preview.status = "The installed model could not be prepared.".into();
                }
            }
            Err(error) => preview.status = error,
        }
    }
    for (mut camera, mut transform) in &mut cameras {
        camera.is_active = editor.active() && preview.status.is_empty();
        let frame = preview.frames[usize::from(preview.close)];
        // Match the default 45-degree vertical field of view and image aspect.
        let half_fov = ((std::f32::consts::FRAC_PI_8).tan() * 420.0 / 520.0).atan();
        let distance = frame.radius.max(0.1) * 1.15 / half_fov.sin();
        let target = frame.center;
        let offset = Vec3::new(preview.angle.sin(), 0.03, preview.angle.cos()) * distance;
        *transform = Transform::from_translation(target + offset).looking_at(target, Vec3::Y);
    }
}
