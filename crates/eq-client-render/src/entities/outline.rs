//! An isolated view of the meshes already drawn for tracked names. The
//! compositor uses its alpha edge over the gameplay camera; scenery and
//! unrelated entities never participate in the mask.
use super::NearbyEntities;
use crate::{
    Player,
    camera::OrbitCamera,
    character::AnimatedCharacter,
    loading::Loading,
    map::{Presentation, TRACKED_INK, WatchState},
    online::OnlineState,
};
use bevy::{
    asset::embedded_asset,
    camera::{RenderTarget, visibility::RenderLayers},
    core_pipeline::tonemapping::Tonemapping,
    image::ImageSampler,
    picking::Pickable,
    prelude::*,
    render::render_resource::{AsBindGroup, TextureFormat},
    shader::ShaderRef,
    ui::{FocusPolicy, UiTargetCamera},
    ui_render::prelude::{MaterialNode, UiMaterial, UiMaterialPlugin},
    window::PrimaryWindow,
};
use eq_client_core::world::ClientWorld;
use std::collections::{BTreeMap, HashSet};

const MASK_LAYER: usize = 3;
const MAX_MASK_EDGE: u32 = 1536;

#[cfg(test)]
mod preview;

#[derive(Asset, AsBindGroup, TypePath, Clone)]
struct EdgeMaterial {
    #[texture(0)]
    #[sampler(1)]
    mask: Handle<Image>,
    #[uniform(2)]
    color: Vec4,
}

impl UiMaterial for EdgeMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://eq_client_render/entities/tracked_outline.wgsl".into()
    }
}

#[derive(Component)]
struct MaskCamera;

#[derive(Component)]
struct Composite;

#[derive(Resource, Default)]
struct Mask {
    size: UVec2,
    selected: HashSet<Entity>,
}

/// Draw after the normal scene updates, including the model's new pose and gear.
pub(crate) fn register(app: &mut App) {
    install_shader(app);
    app.add_systems(
        Update,
        (select, sync_layers, composite)
            .chain()
            .in_set(crate::Stage::Present)
            .after(crate::map::control_actions)
            .after(crate::map::watch_alert)
            .after(crate::camera::orbit_camera)
            .after(crate::camera::fly_camera)
            .after(crate::camera::show_player)
            .after(crate::character::animate),
    );
}

fn install_shader(app: &mut App) {
    embedded_asset!(app, "tracked_outline.wgsl");
    app.add_plugins(UiMaterialPlugin::<EdgeMaterial>::default())
        .init_resource::<Mask>();
}

/// Only an actual draw root for the current incarnation can contribute.
fn remote_roots<'a>(
    world: &'a ClientWorld,
    nearby: &'a NearbyEntities,
    tracks: impl Fn(&str) -> bool + 'a,
) -> impl Iterator<Item = Entity> + 'a {
    nearby.rendered.iter().filter_map(move |(id, &root)| {
        let spawn = world.spawn(*id)?;
        (nearby.revisions.get(id) == Some(&spawn.revision) && tracks(&spawn.state.name))
            .then_some(root)
    })
}

#[allow(clippy::needless_pass_by_value)]
fn select(
    (online, nearby, options, watches): (
        Res<OnlineState>,
        Res<NearbyEntities>,
        Res<Presentation>,
        Res<WatchState>,
    ),
    loading: Option<Res<Loading>>,
    models: Query<(&AnimatedCharacter, &Visibility)>,
    own: Query<Entity, With<Player>>,
    children: Query<(&Children, &Visibility)>,
    parts: Query<&Visibility, With<Mesh3d>>,
    mut mask: ResMut<Mask>,
) {
    let wanted = &mut mask.selected;
    wanted.clear();
    let world = online.world();
    if !options.outlines()
        || !world.can(eq_client_core::Capability::GameplayDepartures)
        || !watches.has_names()
        || !world.in_world()
        || online.scene_loading()
        || loading.is_some_and(|loading| loading.covered())
    {
        return;
    }
    let player = world.player().filter(|player| watches.tracks(&player.name));
    let own = player.and_then(|_| own.single().ok());
    let remotes = remote_roots(world, &nearby, |name| watches.tracks(name));
    for root in remotes.chain(own) {
        let Ok((model, visibility)) = models.get(root) else {
            // The ordinary scene can use a box when a model is missing.
            // Only real model geometry defines the tracking silhouette.
            continue;
        };
        if *visibility == Visibility::Hidden {
            continue;
        }
        let shown = |part| {
            parts
                .get(part)
                .is_ok_and(|visibility| *visibility != Visibility::Hidden)
        };
        wanted.extend(
            model
                .parts
                .iter()
                .map(|part| part.entity)
                .filter(|&part| shown(part)),
        );
        for held in model.held.iter().flatten() {
            if let Ok((parts, visibility)) = children.get(held.entity)
                && *visibility != Visibility::Hidden
            {
                wanted.extend(parts.iter().filter(|&part| shown(part)));
            }
        }
    }
}

/// Add a camera layer to the actual meshes. They retain the same entity,
/// material, skin, visibility, and gameplay layer. Removal edits only this
/// layer, preserving unrelated layer changes made during tracking.
#[allow(clippy::needless_pass_by_value)]
fn sync_layers(
    mut commands: Commands,
    mask: Res<Mask>,
    layers: Query<Option<&RenderLayers>, With<Mesh3d>>,
    mut previous: Local<BTreeMap<Entity, bool>>,
) {
    let gameplay = RenderLayers::default();
    let layer = RenderLayers::layer(MASK_LAYER);
    previous.retain(|&entity, implicit| {
        let Ok(Some(current)) = layers.get(entity) else {
            return false;
        };
        if mask.selected.contains(&entity) && current.intersects(&gameplay) {
            return true;
        }
        if current.intersects(&layer) {
            let remaining = current.clone().without(MASK_LAYER);
            if *implicit && remaining == gameplay {
                commands.entity(entity).remove::<RenderLayers>();
            } else {
                commands.entity(entity).insert(remaining);
            }
        }
        false
    });
    for &entity in &mask.selected {
        let Ok(current) = layers.get(entity) else {
            continue;
        };
        let current_or_default = current.unwrap_or(&gameplay);
        if current_or_default.intersects(&gameplay) {
            previous.entry(entity).or_insert(current.is_none());
            if !current_or_default.intersects(&layer) {
                commands
                    .entity(entity)
                    .insert(current_or_default.clone().with(MASK_LAYER));
            }
        }
    }
}

fn texture_size(size: UVec2) -> Option<UVec2> {
    if size.min_element() == 0 {
        return None;
    }
    let longest = size.max_element();
    if longest <= MAX_MASK_EDGE {
        return Some(size);
    }
    let scale = |side: u32| {
        let scaled = u64::from(side) * u64::from(MAX_MASK_EDGE) / u64::from(longest);
        u32::try_from(scaled.max(1)).expect("bounded by MAX_MASK_EDGE")
    };
    Some(UVec2::new(scale(size.x), scale(size.y)))
}

type GameplayCamera<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Camera,
        &'static Transform,
        &'static Projection,
    ),
    (With<OrbitCamera>, Without<MaskCamera>),
>;
type OutlineCamera<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Camera,
        &'static mut Transform,
        &'static mut Projection,
        &'static mut RenderTarget,
    ),
    With<MaskCamera>,
>;

/// Keep the mask camera at the gameplay camera's pose and clip range.
/// Its output exists only when a tracked model needs a silhouette.
#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
fn composite(
    mut commands: Commands,
    main: GameplayCamera,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut mask: ResMut<Mask>,
    mut cameras: OutlineCamera,
    mut nodes: Query<
        (
            &mut Visibility,
            &mut UiTargetCamera,
            &mut MaterialNode<EdgeMaterial>,
        ),
        With<Composite>,
    >,
    (mut images, mut materials): (ResMut<Assets<Image>>, ResMut<Assets<EdgeMaterial>>),
) {
    let view = main
        .single()
        .ok()
        .filter(|(_, camera, ..)| camera.is_active && !mask.selected.is_empty());
    let active = view.and_then(|(entity, camera, pose, projection)| {
        let size = camera
            .viewport
            .as_ref()
            .map(|viewport| viewport.physical_size)
            .or_else(|| {
                windows
                    .single()
                    .ok()
                    .map(|window| UVec2::new(window.physical_width(), window.physical_height()))
            })
            .or_else(|| camera.physical_viewport_size())?;
        Some((entity, pose, projection, texture_size(size)?))
    });
    let Some((main, pose, projection, size)) = active else {
        for (mut camera, ..) in &mut cameras {
            camera.is_active = false;
        }
        for (mut visibility, ..) in &mut nodes {
            visibility.set_if_neq(Visibility::Hidden);
        }
        return;
    };
    if let Ok((mut camera, mut mask_pose, mut mask_projection, mut target)) = cameras.single_mut()
        && let Ok((mut visibility, mut ui_camera, mut material)) = nodes.single_mut()
    {
        if mask.size != size {
            let (image, edge) = target_for(size, &mut images, &mut materials);
            *target = RenderTarget::Image(image.into());
            material.0 = edge;
            mask.size = size;
        }
        camera.is_active = true;
        mask_pose.set_if_neq(*pose);
        mask_projection.clone_from(projection);
        ui_camera.0 = main;
        visibility.set_if_neq(Visibility::Inherited);
        return;
    }

    let (image, edge) = target_for(size, &mut images, &mut materials);
    mask.size = size;
    commands.spawn((
        MaskCamera,
        Camera3d::default(),
        Camera {
            order: -3,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        RenderTarget::Image(image.into()),
        RenderLayers::layer(MASK_LAYER),
        Tonemapping::None,
        Msaa::Off,
        *pose,
        projection.clone(),
    ));
    commands.spawn((
        Composite,
        UiTargetCamera(main),
        MaterialNode(edge),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        GlobalZIndex(-10),
        FocusPolicy::Pass,
        Pickable::IGNORE,
    ));
}

fn target_for(
    size: UVec2,
    images: &mut Assets<Image>,
    materials: &mut Assets<EdgeMaterial>,
) -> (Handle<Image>, Handle<EdgeMaterial>) {
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    image.sampler = ImageSampler::linear();
    let mask = images.add(image);
    let color = TRACKED_INK.to_linear();
    let material = materials.add(EdgeMaterial {
        mask: mask.clone(),
        color: Vec4::new(color.red, color.green, color.blue, 0.82),
    });
    (mask, material)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing as session;
    use eq_client_core::{WorldEvent, maps::Watches};

    #[test]
    fn all_rendered_matches_follow_the_current_spawn_incarnation_and_watch() {
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(1));
        let mut other = session::pet(4, 1);
        other.name = "Other".into();
        session::spawns(
            &mut online,
            vec![
                session::pet(2, 1),
                session::pet(3, 1),
                other,
                session::pet(5, 1),
            ],
        );
        let mut nearby = NearbyEntities::default();
        let mut entities = World::new();
        for id in [2, 3, 4] {
            nearby.rendered.insert(id, entities.spawn_empty().id());
            nearby
                .revisions
                .insert(id, online.world().spawn(id).unwrap().revision);
        }
        let mut watches = Watches::default();
        watches.add("gABOBER");
        let matches = |online: &OnlineState, watches: &Watches| {
            remote_roots(online.world(), &nearby, |name| watches.contains(name)).collect::<Vec<_>>()
        };
        assert_eq!(
            matches(&online, &watches),
            [nearby.rendered[&2], nearby.rendered[&3]]
        );
        session::news(&mut online, [WorldEvent::Despawn(2)]);
        assert_eq!(matches(&online, &watches), [nearby.rendered[&3]]);
        session::spawns(&mut online, vec![session::pet(2, 1)]);
        assert_eq!(matches(&online, &watches), [nearby.rendered[&3]]);
        watches.remove("Gabober");
        assert_eq!(matches(&online, &watches), []);
        watches.add("Gabober");
        session::admit(&mut online, 2, session::player(1));
        assert_eq!(matches(&online, &watches), []);
    }

    #[test]
    fn mask_does_not_change_normal_meshes_and_keeps_later_layer_edits() {
        let mut app = App::new();
        app.init_resource::<Mask>().add_systems(Update, sync_layers);
        let selected = app.world_mut().spawn(Mesh3d::default()).id();
        let edited = app
            .world_mut()
            .spawn((Mesh3d::default(), RenderLayers::default().with(5)))
            .id();
        let hidden_layer = app
            .world_mut()
            .spawn((Mesh3d::default(), RenderLayers::layer(1)))
            .id();
        let unselected = app.world_mut().spawn(Mesh3d::default()).id();
        app.world_mut()
            .resource_mut::<Mask>()
            .selected
            .extend([selected, edited, hidden_layer]);
        app.update();
        let world = app.world();
        assert_eq!(
            world.get::<RenderLayers>(selected),
            Some(&RenderLayers::default().with(MASK_LAYER))
        );
        assert_eq!(
            world.get::<RenderLayers>(edited),
            Some(&RenderLayers::default().with(5).with(MASK_LAYER))
        );
        assert_eq!(
            world.get::<RenderLayers>(hidden_layer),
            Some(&RenderLayers::layer(1))
        );
        assert!(world.get::<RenderLayers>(unselected).is_none());
        app.world_mut()
            .entity_mut(edited)
            .insert(RenderLayers::default().with(6));
        app.update();
        assert_eq!(
            app.world().get::<RenderLayers>(edited),
            Some(&RenderLayers::default().with(6).with(MASK_LAYER))
        );
        app.world_mut().resource_mut::<Mask>().selected.clear();
        app.update();
        assert!(app.world().get::<RenderLayers>(selected).is_none());
        assert_eq!(
            app.world().get::<RenderLayers>(edited),
            Some(&RenderLayers::default().with(6))
        );
    }

    #[test]
    fn mask_size_has_a_hard_pixel_budget_and_follows_both_aspects() {
        assert_eq!(
            texture_size(UVec2::new(1280, 720)),
            Some(UVec2::new(1280, 720))
        );
        assert_eq!(
            texture_size(UVec2::new(3840, 2160)),
            Some(UVec2::new(1536, 864))
        );
        assert_eq!(
            texture_size(UVec2::new(2160, 3840)),
            Some(UVec2::new(864, 1536))
        );
        assert_eq!(texture_size(UVec2::new(640, 0)), None);
    }
}
