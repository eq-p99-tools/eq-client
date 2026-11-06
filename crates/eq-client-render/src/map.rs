//! A local map on every server: installed map lines, the player and all
//! known entities. Entity inspection never sends a game command.
mod controls;
mod entities;
mod items;
mod motion;
mod navigation;
mod presentation;
#[cfg(test)]
mod preview;
mod target;
mod watch;
mod window;

pub(crate) use controls::{
    actions as control_actions, focus as search_focus, scroll as scroll_watches,
};
pub(crate) use controls::{input as search_input, update as update_controls};
pub(crate) use entities::{inspect, update as update_entities};
pub(crate) use items::update as update_items;
pub(crate) use motion::{MapMotion, draw as draw_movement, sample as sample_movement};
pub(crate) use navigation::input as navigate;
pub(crate) use presentation::{
    Backdrop, Presentation, actions as presentation_actions, persist as persist_presentation,
    update as present,
};
pub(crate) use target::{input as target_input, show as show_target};
pub(crate) use watch::{WatchState, alert as watch_alert, persist as persist_watches};
pub(crate) use window::{FallbackMap, ensure_window};

use crate::online::OnlineState;
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use eq_client_assets::maps::ZoneMap;

/// The skin's area the map draws in.
#[derive(Component)]
#[require(Button, crate::windows::pointer::TakesWheel)]
pub(crate) struct MapCanvas;

/// The arrow that shows where the player is and the way they face.
#[derive(Component)]
pub(crate) struct PlayerArrow;

/// How wide and tall the player's arrow is, in pixels.
const ARROW: u32 = 17;
/// Give the local player's facing arrow a modest lift in size and depth.
const OWN_ARROW_SIZE: f32 = 20.0;
const OWN_ARROW_DEPTH: i32 = 7;
/// The arrow's corners, pointing up: its point, a wing, the notch at its
/// back and the other wing. Longer than it is wide and notched at the back,
/// so the point cannot be mistaken once the arrow turns.
const DART: [(f32, f32); 4] = [(8.5, 1.0), (13.0, 15.5), (8.5, 12.0), (4.0, 15.5)];
/// The arrow's colour.
const ARROW_FILL: [u8; 4] = [180, 85, 245, 255];
/// The arrow's darker edge, which keeps it clear of the map's lines.
const ARROW_EDGE: [u8; 4] = [60, 10, 10, 255];

/// A label's words on the map.
#[derive(Component)]
pub(crate) struct LabelText;

/// What a button of the map's toolbar does.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapButton {
    /// Draws the map larger.
    ZoomIn,
    /// Draws the map smaller.
    ZoomOut,
    /// Moves the view a quarter of the way up, down, left or right.
    Pan(i8, i8),
    /// Fits the geometry and every reported position, including remote helpers.
    Reset,
    /// Centers a useful local view on the player.
    Nearby,
    /// Shows or hides the labels.
    Labels,
    /// Shows or hides one of the layers, 1 to 3.
    Layer(u8),
}

impl MapButton {
    /// The skin's button for this action, by screen ID, in the map window.
    pub(crate) fn for_screen(id: &str) -> Option<Self> {
        Some(match id {
            "MVW_ZoomInButton" => Self::ZoomIn,
            "MVW_ZoomOutButton" => Self::ZoomOut,
            "MVW_PanUpButton" => Self::Pan(0, -1),
            "MVW_PanDownButton" => Self::Pan(0, 1),
            "MVW_PanLeftButton" => Self::Pan(-1, 0),
            "MVW_PanRightButton" => Self::Pan(1, 0),
            "MVW_PanResetButton" => Self::Nearby,
            "MVW_LabelsToggleButton" => Self::Labels,
            "MVW_Layer1Button" => Self::Layer(1),
            "MVW_Layer2Button" => Self::Layer(2),
            "MVW_Layer3Button" => Self::Layer(3),
            _ => return None,
        })
    }
}

/// How much each zoom step enlarges the map.
const ZOOM_STEP: f32 = 1.5;
/// How far the view may zoom in or out, in steps.
const ZOOM_STEPS: i32 = 8;

/// The zone's map and how the player looks at it.
#[derive(Resource)]
pub(crate) struct MapView {
    /// The zone whose map is loaded, and its map if the installation has one.
    zone: Option<(String, Option<ZoneMap>)>,
    /// Zone admission, so reconnecting to the same zone drops old selections.
    admission: Option<u64>,
    /// Stable extent chosen by Nearby or Fit all, independent of moving spawns.
    bounds: Option<(Vec2, Vec2)>,
    /// The transform actually used by the current canvas.
    projection: Option<Projection>,
    /// An entity pinned for inspection; scoped to this admission.
    selected: Option<entities::Identity>,
    /// A name substring; unmatched markers stay on the map but become subdued.
    query: String,
    /// Zoom steps from the whole map.
    zoom: i32,
    /// Where the view's centre is moved to, in map units.
    pan: Vec2,
    /// Whether labels show.
    labels: bool,
    /// Which layers show, 1 to 3; the base map always does.
    layers: [bool; 3],
    /// What was last drawn, so the picture is drawn again only on a change.
    drawn: Option<Drawn>,
}

impl Default for MapView {
    fn default() -> Self {
        Self {
            zone: None,
            admission: None,
            bounds: None,
            projection: None,
            selected: None,
            query: String::new(),
            zoom: 0,
            pan: Vec2::ZERO,
            labels: true,
            layers: [true; 3],
            drawn: None,
        }
    }
}

/// What a drawn picture shows.
#[derive(Clone, PartialEq)]
struct Drawn {
    canvas: Entity,
    zone: String,
    size: UVec2,
    zoom: i32,
    pan: Vec2,
    labels: bool,
    layers: [bool; 3],
    bounds: (Vec2, Vec2),
    inspecting: bool,
    plain: bool,
}

/// How map units reach the canvas: its scale and the map point at its
/// centre.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Projection {
    scale: f32,
    centre: Vec2,
    size: Vec2,
    offset: Vec2,
}

impl Projection {
    /// The whole map fits the canvas at zoom 0, with a margin, and each step
    /// enlarges it; panning moves the centre.
    fn new((min, max): (Vec2, Vec2), size: Vec2, zoom: i32, pan: Vec2) -> Option<Self> {
        if !min.is_finite() || !max.is_finite() || !size.is_finite() || size.min_element() <= 0.0 {
            return None;
        }
        let extent = (max - min).max(Vec2::splat(1.0));
        if !extent.is_finite() || !pan.is_finite() {
            return None;
        }
        let fit = (size.x / extent.x).min(size.y / extent.y) * 0.95;
        Some(Self {
            scale: fit * ZOOM_STEP.powi(zoom),
            centre: min + (max - min) / 2.0 + pan,
            size,
            offset: size / 2.0,
        })
    }

    /// Keep the fitted entities clear of search/legend and the pinned inspector.
    fn for_canvas(
        bounds: (Vec2, Vec2),
        size: Vec2,
        zoom: i32,
        pan: Vec2,
        inspecting: bool,
    ) -> Option<Self> {
        let top = 142.0_f32.min(size.y * 0.4);
        let bottom = if inspecting {
            96.0_f32.min(size.y * 0.3)
        } else {
            12.0
        };
        let inset = Vec2::new(10.0_f32.min(size.x * 0.1), top);
        let drawable = size - Vec2::new(inset.x * 2.0, top + bottom);
        let mut projection = Self::new(bounds, drawable, zoom, pan)?;
        projection.offset += inset;
        projection.size = size;
        Some(projection)
    }

    /// Where a map point falls on the canvas, in its pixels.
    fn place(self, point: Vec2) -> Vec2 {
        (point - self.centre) * self.scale + self.offset
    }
}

/// Where the player is on the map: the world's x and y negated.
fn map_point(position: eq_client_core::WorldPosition) -> Vec2 {
    Vec2::new(-position.x, -position.y)
}

/// Loads geometry on admission and starts near the player. Remote server helpers
/// remain inspectable but cannot shrink the default view or rescale it mid-drag.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn load(
    settings: Res<crate::ViewerSettings>,
    online: Res<OnlineState>,
    motion: Res<MapMotion>,
    mut view: ResMut<MapView>,
) {
    let world = online.world();
    let zone = world.zone();
    if zone.is_empty() {
        *view = MapView::default();
        return;
    }
    if view.admission != world.session_id()
        || view.zone.as_ref().is_none_or(|(loaded, _)| loaded != zone)
    {
        let map = settings
            .0
            .eq_directory
            .as_deref()
            .and_then(|directory| ZoneMap::load(directory, zone));
        *view = MapView {
            zone: Some((zone.to_owned(), map)),
            admission: world.session_id(),
            ..default()
        };
    }
    if view.bounds.is_none() {
        view.bounds = motion
            .player(world)
            .map(|position| nearby_bounds(map_point(position)));
    }
}

/// A 1,200-unit neighborhood is readable without depending on zone size.
fn nearby_bounds(centre: Vec2) -> (Vec2, Vec2) {
    (centre - Vec2::splat(600.0), centre + Vec2::splat(600.0))
}

/// Finite map bounds, in the same coordinate frame as entity markers.
fn map_bounds(map: &ZoneMap) -> Option<(Vec2, Vec2)> {
    map.bounds()
        .map(|(min, max)| (Vec2::from(min), Vec2::from(max)))
        .filter(|(min, max)| min.is_finite() && max.is_finite())
}

/// Expand with breathing room so an NPC walking at the edge does not
/// rescale the map every frame. Invalid coordinates never affect the view.
fn expand(bounds: &mut Option<(Vec2, Vec2)>, point: Vec2) {
    if !point.is_finite() {
        return;
    }
    let margin = Vec2::splat(64.0);
    match bounds {
        Some((min, max)) => {
            if point.cmplt(*min).any() || point.cmpgt(*max).any() {
                *min = min.min(point - margin);
                *max = max.max(point + margin);
            }
        }
        None => *bounds = Some((point - margin, point + margin)),
    }
}

/// Carries out the toolbar's buttons.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn buttons(
    mut view: ResMut<MapView>,
    online: Res<OnlineState>,
    motion: Res<MapMotion>,
    buttons: crate::clicks::Clicks<MapButton>,
    canvases: Query<&ComputedNode, With<MapCanvas>>,
) {
    let size = canvases
        .iter()
        .next()
        .map_or(Vec2::new(472.0, 360.0), |node| {
            node.size() * node.inverse_scale_factor()
        });
    for button in buttons.iter() {
        let scale = view
            .bounds
            .and_then(|bounds| {
                Projection::for_canvas(bounds, size, view.zoom, view.pan, view.selected.is_some())
            })
            .map_or(1.0, |projection| projection.scale);
        match *button {
            MapButton::ZoomIn => view.zoom = (view.zoom + 1).min(ZOOM_STEPS),
            MapButton::ZoomOut => view.zoom = (view.zoom - 1).max(-ZOOM_STEPS),
            MapButton::Pan(x, y) => {
                view.pan += Vec2::new(f32::from(x), f32::from(y)) * size / 4.0 / scale;
            }
            MapButton::Reset => {
                view.zoom = 0;
                view.pan = Vec2::ZERO;
                view.bounds = view
                    .zone
                    .as_ref()
                    .and_then(|(_, map)| map.as_ref())
                    .and_then(map_bounds);
                for position in motion.player(online.world()).into_iter().chain(
                    online
                        .world()
                        .spawns()
                        .values()
                        .map(|spawn| spawn.state.position),
                ) {
                    expand(&mut view.bounds, map_point(position));
                }
            }
            MapButton::Nearby => {
                view.zoom = 0;
                view.pan = Vec2::ZERO;
                view.bounds = motion
                    .player(online.world())
                    .map(|position| nearby_bounds(map_point(position)));
            }
            MapButton::Labels => view.labels = !view.labels,
            MapButton::Layer(layer) => {
                if let Some(shows) = view.layers.get_mut(usize::from(layer.saturating_sub(1))) {
                    *shows = !*shows;
                }
            }
        }
    }
}

/// Draws the map in its canvas when what it shows changes, its labels with
/// it, and keeps the player's arrow where the player is, facing their way.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn draw(
    mut commands: Commands,
    (online, motion, presentation): (Res<OnlineState>, Res<MapMotion>, Res<Presentation>),
    mut view: ResMut<MapView>,
    mut images: ResMut<Assets<Image>>,
    mut canvases: Query<(Entity, &ComputedNode, &mut ImageNode), With<MapCanvas>>,
    mut arrows: Query<(&mut Node, &mut UiTransform, &mut Visibility), With<PlayerArrow>>,
    labels: Query<Entity, With<LabelText>>,
) {
    let Ok((canvas, node, mut image)) = canvases.single_mut() else {
        view.drawn = None;
        view.projection = None;
        return;
    };
    let size = node.size() * node.inverse_scale_factor();
    if size.x < 1.0 || size.y < 1.0 {
        view.projection = None;
        return;
    }
    let bounds = view
        .bounds
        .unwrap_or((Vec2::splat(-64.0), Vec2::splat(64.0)));
    let Some(projection) =
        Projection::for_canvas(bounds, size, view.zoom, view.pan, view.selected.is_some())
    else {
        view.projection = None;
        return;
    };
    view.projection = Some(projection);
    let empty = ZoneMap::default();
    let (zone, map) = view.zone.as_ref().map_or(("", &empty), |(zone, map)| {
        (zone.as_str(), map.as_ref().unwrap_or(&empty))
    });
    let wanted = Drawn {
        canvas,
        zone: zone.to_owned(),
        size: size.as_uvec2(),
        zoom: view.zoom,
        pan: view.pan,
        labels: view.labels,
        layers: view.layers,
        bounds,
        inspecting: view.selected.is_some(),
        plain: presentation.plain(),
    };
    if view.drawn.as_ref() != Some(&wanted) {
        image.image = images.add(picture(
            map,
            projection,
            view.layers,
            wanted.size,
            wanted.plain,
        ));
        for label in &labels {
            commands.entity(label).despawn();
        }
        if view.labels {
            draw_labels(
                &mut commands,
                canvas,
                map,
                projection,
                view.layers,
                wanted.plain,
            );
        }
        view.drawn = Some(wanted);
    }
    // The player's arrow, which only exists once the canvas has its first
    // picture.
    let player = motion.player(online.world());
    if arrows.is_empty() {
        commands.entity(canvas).with_child((
            PlayerArrow,
            bevy::ui::FocusPolicy::Pass,
            ImageNode::new(images.add(arrow())),
            // Above every entity and label, below the canvas's controls.
            ZIndex(OWN_ARROW_DEPTH),
            Node {
                position_type: PositionType::Absolute,
                width: px(OWN_ARROW_SIZE),
                height: px(OWN_ARROW_SIZE),
                ..default()
            },
            UiTransform::default(),
            Visibility::Hidden,
        ));
        return;
    }
    for (mut arrow, mut transform, mut visibility) in &mut arrows {
        let at = player.map(|position| (projection.place(map_point(position)), position.heading));
        let wanted = match at {
            Some((at, heading))
                if heading.is_finite() && at.cmpge(Vec2::ZERO).all() && at.cmple(size).all() =>
            {
                arrow.left = px(at.x - OWN_ARROW_SIZE / 2.0);
                arrow.top = px(at.y - OWN_ARROW_SIZE / 2.0);
                let rotation = facing(heading);
                if transform.rotation != rotation {
                    transform.rotation = rotation;
                }
                Visibility::Inherited
            }
            _ => Visibility::Hidden,
        };
        visibility.set_if_neq(wanted);
    }
}

/// Rebuild local geometry labels only when the map image changes.
fn draw_labels(
    commands: &mut Commands,
    canvas: Entity,
    map: &ZoneMap,
    projection: Projection,
    layers: [bool; 3],
    plain: bool,
) {
    commands.entity(canvas).with_children(|canvas| {
        for label in &map.labels {
            if label.layer > 0 && !layers[usize::from(label.layer - 1)] {
                continue;
            }
            let at = projection.place(Vec2::new(label.at[0], label.at[1]));
            if !at.is_finite() || at.cmplt(Vec2::ZERO).any() || at.cmpgt(projection.size).any() {
                continue;
            }
            let [r, g, b] = presentation::ink(label.color, plain);
            canvas.spawn((
                LabelText,
                Text::new(label.text.clone()),
                crate::theme::font(crate::theme::Size::Small),
                TextColor(Color::srgb_u8(r, g, b)),
                bevy::ui::FocusPolicy::Pass,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(at.x + 2.0),
                    top: px(at.y - 6.0),
                    ..default()
                },
            ));
        }
    });
}

/// How far the arrow, drawn pointing north, turns for a heading. EQ headings
/// run 0 to 512 from north towards the west; on the map, north is up and
/// the west to the left, and the UI turns clockwise for a positive angle.
fn facing(heading: f32) -> Rot2 {
    Rot2::radians(-heading.rem_euclid(512.0) / 512.0 * std::f32::consts::TAU)
}

/// Whether a point lies inside the arrow's outline.
fn in_dart(x: f32, y: f32) -> bool {
    let mut inside = false;
    for (index, &(ax, ay)) in DART.iter().enumerate() {
        let (bx, by) = DART[(index + 1) % DART.len()];
        if (ay > y) != (by > y) && x < ax + (y - ay) / (by - ay) * (bx - ax) {
            inside = !inside;
        }
    }
    inside
}

/// The player's arrow: a purple dart pointing up, edged dark, on a clear
/// background.
#[allow(clippy::cast_precision_loss)] // A few pixels.
fn arrow() -> Image {
    arrow_picture(ARROW_FILL, ARROW_EDGE)
}

/// One shared facing silhouette; remote players tint a white copy per entity.
#[allow(clippy::cast_precision_loss)] // A few pixels.
fn arrow_picture(fill: [u8; 4], edge: [u8; 4]) -> Image {
    let filled = |x: u32, y: u32| in_dart(x as f32 + 0.5, y as f32 + 0.5);
    let mut pixels = vec![0u8; (ARROW * ARROW * 4) as usize];
    for y in 0..ARROW {
        for x in 0..ARROW {
            let color = if filled(x, y) {
                fill
            } else if (y.saturating_sub(1)..=(y + 1).min(ARROW - 1)).any(|near| {
                (x.saturating_sub(1)..=(x + 1).min(ARROW - 1)).any(|across| filled(across, near))
            }) {
                edge
            } else {
                continue;
            };
            let at = ((y * ARROW + x) * 4) as usize;
            pixels[at..at + 4].copy_from_slice(&color);
        }
    }
    Image::new(
        Extent3d {
            width: ARROW,
            height: ARROW,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// The map's lines drawn into a picture of the canvas's size, on a clear
/// background so the skin's parchment shows through.
fn picture(
    map: &ZoneMap,
    projection: Projection,
    layers: [bool; 3],
    size: UVec2,
    plain: bool,
) -> Image {
    let (width, height) = (size.x.max(1), size.y.max(1));
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for line in &map.lines {
        if line.layer > 0 && !layers[usize::from(line.layer - 1)] {
            continue;
        }
        let from = projection.place(Vec2::new(line.from[0], line.from[1]));
        let to = projection.place(Vec2::new(line.to[0], line.to[1]));
        let [r, g, b] = presentation::ink(line.color, plain);
        stroke(&mut pixels, (width, height), from, to, [r, g, b, 255]);
    }
    Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// One line, a pixel wide, cut to the picture.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)] // Bounded first, and steps are few.
fn stroke(pixels: &mut [u8], (width, height): (u32, u32), from: Vec2, to: Vec2, color: [u8; 4]) {
    let steps = (to - from).abs().max_element().ceil().min(4096.0) as u32;
    for step in 0..=steps {
        let t = if steps == 0 {
            0.0
        } else {
            step as f32 / steps as f32
        };
        let point = from.lerp(to, t);
        if point.x < 0.0 || point.y < 0.0 {
            continue;
        }
        let (x, y) = (point.x as u32, point.y as u32);
        if x >= width || y >= height {
            continue;
        }
        let at = ((y * width + x) * 4) as usize;
        pixels[at..at + 4].copy_from_slice(&color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_assets::maps::MapLine;

    fn square() -> ZoneMap {
        let line = |from: [f32; 3], to: [f32; 3], layer| MapLine {
            from,
            to,
            color: [0, 0, 0],
            layer,
        };
        ZoneMap {
            lines: vec![
                line([-100.0, -50.0, 0.0], [100.0, -50.0, 0.0], 0),
                line([100.0, 50.0, 0.0], [-100.0, 50.0, 0.0], 2),
            ],
            labels: Vec::new(),
        }
    }

    #[test]
    fn the_whole_map_fits_and_zoom_and_pan_move_it() {
        let size = Vec2::new(400.0, 200.0);
        let whole = Projection::new(map_bounds(&square()).unwrap(), size, 0, Vec2::ZERO).unwrap();
        // The map's centre is the canvas's, and its width nearly fills it.
        assert_eq!(whole.place(Vec2::ZERO), size / 2.0);
        assert!((whole.place(Vec2::new(100.0, 0.0)).x - 390.0).abs() < 1.0);
        let closer = Projection::new(map_bounds(&square()).unwrap(), size, 1, Vec2::ZERO).unwrap();
        assert!((closer.scale / whole.scale - ZOOM_STEP).abs() < 1e-4);
        let moved = Projection::new(
            map_bounds(&square()).unwrap(),
            size,
            0,
            Vec2::new(10.0, 0.0),
        )
        .unwrap();
        assert_eq!(moved.place(Vec2::new(10.0, 0.0)), size / 2.0);
        // The player at world (50, -20) is at map (-50, 20).
        assert_eq!(
            map_point(eq_client_core::WorldPosition {
                x: 50.0,
                y: -20.0,
                ..Default::default()
            }),
            Vec2::new(-50.0, 20.0)
        );
    }

    #[test]
    fn the_arrow_turns_west_at_a_quarter_heading() {
        // North points up, unturned; west, a quarter of the way round
        // anticlockwise, is a quarter turn back for the clockwise UI.
        assert!((facing(0.0).as_radians()).abs() < 1e-6);
        assert!((facing(128.0).as_radians() + std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert!((facing(640.0).as_radians() + std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        // The arrow points up: its point is at the top in the middle, edged
        // above, and the notch leaves the middle of its back clear.
        let picture = arrow();
        let pixels = picture.data.as_ref().unwrap();
        let at = |x: u32, y: u32| &pixels[((y * ARROW + x) * 4) as usize..][..4];
        let middle = ARROW / 2;
        assert_eq!(at(middle, 1), ARROW_FILL);
        assert_eq!(at(middle, 0), ARROW_EDGE);
        assert_eq!(at(middle, ARROW - 2)[3], 0);
        assert_eq!(at(0, 0)[3], 0);
    }

    #[test]
    fn local_facing_stays_above_a_pinned_overlap_inside_the_canvas() {
        use crate::online::testing as session;
        let mut app = crate::testing::app();
        let canvas = app
            .world_mut()
            .spawn((
                MapCanvas,
                ImageNode::default(),
                ComputedNode {
                    size: Vec2::splat(500.0),
                    ..default()
                },
            ))
            .id();
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(1));
        let mut npc = session::pet(2, 1);
        npc.position = online.world().player().unwrap().position;
        session::spawns(&mut online, vec![npc]);
        app.insert_resource(online)
            .add_systems(Update, (draw, update_entities, update_items).chain());
        app.update();
        app.world_mut()
            .resource_scope(|world, mut view: Mut<MapView>| {
                view.admission = Some(1);
                entities::select_preview(&mut view, world.resource::<OnlineState>().world(), 2);
            });
        app.update();
        let scale = app.world().resource::<MapView>().projection.unwrap().scale;
        let mut online = app.world_mut().resource_mut::<OnlineState>();
        let mut position = online.world().player().unwrap().position;
        position.x -= 7.0 / scale;
        session::news(
            &mut online,
            [eq_client_core::WorldEvent::Objects(
                eq_client_core::ground::ObjectUpdate::Spawn(eq_client_core::ground::GroundObject {
                    drop_id: 70_000,
                    model: "IT63_ACTORDEF".into(),
                    position,
                    object_type: 0,
                }),
            )],
        );
        app.update();
        let world = app.world_mut();
        let (arrow, node, transform, visible, facing_depth, parent) = world
            .query_filtered::<(Entity, &Node, &UiTransform, &Visibility, &ZIndex, &ChildOf), With<PlayerArrow>>()
            .single(world)
            .unwrap();
        assert_eq!(*visible, Visibility::Inherited);
        assert_eq!(parent.parent(), canvas);
        assert!(world.get::<GlobalZIndex>(arrow).is_none());
        assert_eq!(
            world.get::<bevy::ui::FocusPolicy>(arrow),
            Some(&bevy::ui::FocusPolicy::Pass)
        );
        let (left, top, width) = (node.left, node.top, node.width);
        let facing_depth = facing_depth.0;
        assert_eq!(transform.rotation, facing(0.0));
        let (_, npc, depth) = world
            .query::<(&entities::Marker, &Node, &ZIndex)>()
            .iter(world)
            .find(|(marker, _, _)| marker.0 == 2)
            .unwrap();
        let centre = |start, width| match (start, width) {
            (Val::Px(start), Val::Px(width)) => start + width / 2.0,
            _ => unreachable!("absolute marker placement uses pixels"),
        };
        assert_eq!(centre(left, width), centre(npc.left, npc.width));
        assert_eq!(centre(top, width), centre(npc.top, npc.height));
        assert!(centre(px(0), width) > centre(px(0), npc.width));
        assert!(facing_depth > depth.0);
        let right = |node: &Node| centre(node.left, node.width) + centre(px(0), node.width);
        let npc_right = right(npc);
        let (_, own, own_depth) = world
            .query::<(&entities::Marker, &Node, &ZIndex)>()
            .iter(world)
            .find(|(marker, _, _)| marker.0 == 1)
            .unwrap();
        let own_right = right(own);
        assert!(facing_depth > own_depth.0);
        let (item, hit_policy) = world
            .query_filtered::<(&Node, &bevy::ui::FocusPolicy), With<items::ItemMarker>>()
            .single(world)
            .unwrap();
        assert_eq!(*hit_policy, bevy::ui::FocusPolicy::Block);
        // The item extends past both blocking markers and is inspectable
        // through the larger, pointer-pass local arrow.
        let hit = centre(item.left, item.width) + 2.0;
        let arrow_right = centre(left, width) + centre(px(0), width);
        assert!(hit > own_right && hit > npc_right && hit < arrow_right && hit < right(item));
        let controls_depth = world
            .query_filtered::<&ZIndex, With<entities::Legend>>()
            .single(world)
            .unwrap();
        assert!(facing_depth < controls_depth.0);
    }

    #[test]
    fn hidden_layers_leave_their_lines_out() {
        let size = UVec2::new(100, 60);
        let projection = Projection::new(
            map_bounds(&square()).unwrap(),
            size.as_vec2(),
            0,
            Vec2::ZERO,
        )
        .unwrap();
        let drawn = |layers| {
            picture(&square(), projection, layers, size, false)
                .data
                .unwrap()
                .chunks(4)
                .filter(|pixel| pixel[3] == 255)
                .count()
        };
        let all = drawn([true; 3]);
        let without_two = drawn([true, false, true]);
        assert!(without_two > 0 && without_two < all);
    }

    #[test]
    fn the_skins_toolbar_buttons_are_known_by_name() {
        assert_eq!(
            MapButton::for_screen("MVW_ZoomInButton"),
            Some(MapButton::ZoomIn)
        );
        assert_eq!(
            MapButton::for_screen("MVW_Layer2Button"),
            Some(MapButton::Layer(2))
        );
        assert_eq!(MapButton::for_screen("MVW_ZFilterButton"), None);
    }

    #[test]
    fn remote_helpers_do_not_shrink_default_view_and_readmission_resets_it() {
        use crate::online::testing as session;
        let mut app = App::new();
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(1));
        let mut spawn = session::pet(2, 1);
        spawn.position.x = 10000.0;
        session::spawns(&mut online, vec![spawn]);
        app.insert_resource(online)
            .init_resource::<crate::ViewerSettings>()
            .init_resource::<MapView>()
            .init_resource::<MapMotion>()
            .add_systems(Update, load);
        app.update();
        let view = app.world().resource::<MapView>();
        let bounds = view.bounds.unwrap();
        assert_eq!(
            bounds,
            nearby_bounds(map_point(session::player(1).position))
        );
        assert!(Projection::new(bounds, Vec2::splat(500.0), 0, Vec2::ZERO).is_some());
        app.world_mut().resource_mut::<MapView>().query = "old zone".into();
        session::admit(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            2,
            session::player(1),
        );
        app.update();
        assert_eq!(app.world().resource::<MapView>().query, "");
        assert_eq!(app.world().resource::<MapView>().bounds.unwrap(), bounds);
    }
}
