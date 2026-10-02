//! The in-game map, in the skin's Map window: the zone's lines from the
//! installation's map files, its labels while they are on, and the player as
//! a dot, with the skin's toolbar to zoom, pan, reset the view and show or
//! hide the labels and layers. The map opens only where the server type
//! offers it.
use crate::{
    online::OnlineState,
    windows::{Shown, WindowId},
};
use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use eq_client_assets::maps::ZoneMap;
use eq_client_core::Capability;

/// The skin's area the map draws in.
#[derive(Component)]
pub(crate) struct MapCanvas;

/// The dot that shows where the player is.
#[derive(Component)]
pub(crate) struct PlayerDot;

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
    /// Shows the whole map again.
    Reset,
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
            "MVW_PanResetButton" => Self::Reset,
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
    zone: String,
    size: UVec2,
    zoom: i32,
    pan: Vec2,
    labels: bool,
    layers: [bool; 3],
}

/// How map units reach the canvas: its scale and the map point at its
/// centre.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Projection {
    scale: f32,
    centre: Vec2,
    size: Vec2,
}

impl Projection {
    /// The whole map fits the canvas at zoom 0, with a margin, and each step
    /// enlarges it; panning moves the centre.
    fn new(map: &ZoneMap, size: Vec2, zoom: i32, pan: Vec2) -> Option<Self> {
        let (min, max) = map.bounds()?;
        let (min, max) = (Vec2::from(min), Vec2::from(max));
        let extent = (max - min).max(Vec2::splat(1.0));
        let fit = (size.x / extent.x).min(size.y / extent.y) * 0.95;
        Some(Self {
            scale: fit * ZOOM_STEP.powi(zoom),
            centre: (min + max) / 2.0 + pan,
            size,
        })
    }

    /// Where a map point falls on the canvas, in its pixels.
    fn place(self, point: Vec2) -> Vec2 {
        (point - self.centre) * self.scale + self.size / 2.0
    }
}

/// Where the player is on the map: the world's x and y negated.
fn map_point(position: eq_client_core::WorldPosition) -> Vec2 {
    Vec2::new(-position.x, -position.y)
}

/// Loads the zone's map when the zone changes, and closes the map where the
/// server type does not offer it.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn load(
    settings: Res<crate::ViewerSettings>,
    online: Res<OnlineState>,
    mut view: ResMut<MapView>,
    mut shown: ResMut<Shown>,
) {
    if !crate::outbox::offered(online.world(), Capability::Map) {
        shown.close(WindowId::Map);
    }
    let zone = online.world().zone();
    if zone.is_empty() || view.zone.as_ref().is_some_and(|(loaded, _)| loaded == zone) {
        return;
    }
    let map = settings
        .0
        .eq_directory
        .as_deref()
        .and_then(|directory| ZoneMap::load(directory, zone));
    view.zone = Some((zone.to_owned(), map));
    view.pan = Vec2::ZERO;
    view.zoom = 0;
    view.drawn = None;
}

/// Carries out the toolbar's buttons.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn buttons(
    mut view: ResMut<MapView>,
    buttons: Query<(&Interaction, &MapButton), Changed<Interaction>>,
    canvases: Query<&ComputedNode, With<MapCanvas>>,
) {
    let size = canvases
        .iter()
        .next()
        .map_or(Vec2::new(472.0, 360.0), |node| {
            node.size() * node.inverse_scale_factor()
        });
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let scale = view
            .zone
            .as_ref()
            .and_then(|(_, map)| map.as_ref())
            .and_then(|map| Projection::new(map, size, view.zoom, view.pan))
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
/// it, and keeps the player's dot where the player is.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn draw(
    mut commands: Commands,
    online: Res<OnlineState>,
    mut view: ResMut<MapView>,
    mut images: ResMut<Assets<Image>>,
    mut canvases: Query<(Entity, &ComputedNode, &mut ImageNode), With<MapCanvas>>,
    mut dots: Query<(&mut Node, &mut Visibility), With<PlayerDot>>,
    labels: Query<Entity, With<LabelText>>,
) {
    let Ok((canvas, node, mut image)) = canvases.single_mut() else {
        view.drawn = None;
        return;
    };
    let size = node.size() * node.inverse_scale_factor();
    if size.x < 1.0 || size.y < 1.0 {
        return;
    }
    let Some((zone, Some(map))) = view.zone.as_ref() else {
        return;
    };
    let Some(projection) = Projection::new(map, size, view.zoom, view.pan) else {
        return;
    };
    let wanted = Drawn {
        zone: zone.clone(),
        size: size.as_uvec2(),
        zoom: view.zoom,
        pan: view.pan,
        labels: view.labels,
        layers: view.layers,
    };
    if view.drawn.as_ref() != Some(&wanted) {
        image.image = images.add(picture(map, projection, view.layers, wanted.size));
        for label in &labels {
            commands.entity(label).despawn();
        }
        if view.labels {
            commands.entity(canvas).with_children(|canvas| {
                for label in &map.labels {
                    if label.layer > 0 && !view.layers[usize::from(label.layer - 1)] {
                        continue;
                    }
                    let at = projection.place(Vec2::new(label.at[0], label.at[1]));
                    if at.x < 0.0 || at.y < 0.0 || at.x > size.x || at.y > size.y {
                        continue;
                    }
                    let [r, g, b] = label.color;
                    canvas.spawn((
                        LabelText,
                        Text::new(label.text.clone()),
                        crate::theme::font(crate::theme::Size::Small),
                        TextColor(Color::srgb_u8(r, g, b)),
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
        view.drawn = Some(wanted);
    }
    // The player's dot, which only exists once the canvas has its first
    // picture.
    let player = online.world().player().map(|player| player.position);
    if dots.is_empty() {
        commands.entity(canvas).with_child((
            PlayerDot,
            Node {
                position_type: PositionType::Absolute,
                width: px(6),
                height: px(6),
                ..default()
            },
            BackgroundColor(Color::srgb(0.85, 0.1, 0.1)),
            Visibility::Hidden,
        ));
        return;
    }
    for (mut dot, mut visibility) in &mut dots {
        let at = player.map(|position| projection.place(map_point(position)));
        let wanted = match at {
            Some(at) if at.x >= 0.0 && at.y >= 0.0 && at.x <= size.x && at.y <= size.y => {
                dot.left = px(at.x - 3.0);
                dot.top = px(at.y - 3.0);
                Visibility::Inherited
            }
            _ => Visibility::Hidden,
        };
        visibility.set_if_neq(wanted);
    }
}

/// The map's lines drawn into a picture of the canvas's size, on a clear
/// background so the skin's parchment shows through.
fn picture(map: &ZoneMap, projection: Projection, layers: [bool; 3], size: UVec2) -> Image {
    let (width, height) = (size.x.max(1), size.y.max(1));
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for line in &map.lines {
        if line.layer > 0 && !layers[usize::from(line.layer - 1)] {
            continue;
        }
        let from = projection.place(Vec2::new(line.from[0], line.from[1]));
        let to = projection.place(Vec2::new(line.to[0], line.to[1]));
        let [r, g, b] = line.color;
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
        let whole = Projection::new(&square(), size, 0, Vec2::ZERO).unwrap();
        // The map's centre is the canvas's, and its width nearly fills it.
        assert_eq!(whole.place(Vec2::ZERO), size / 2.0);
        assert!((whole.place(Vec2::new(100.0, 0.0)).x - 390.0).abs() < 1.0);
        let closer = Projection::new(&square(), size, 1, Vec2::ZERO).unwrap();
        assert!((closer.scale / whole.scale - ZOOM_STEP).abs() < 1e-4);
        let moved = Projection::new(&square(), size, 0, Vec2::new(10.0, 0.0)).unwrap();
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
    fn hidden_layers_leave_their_lines_out() {
        let size = UVec2::new(100, 60);
        let projection = Projection::new(&square(), size.as_vec2(), 0, Vec2::ZERO).unwrap();
        let drawn = |layers| {
            picture(&square(), projection, layers, size)
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
}
