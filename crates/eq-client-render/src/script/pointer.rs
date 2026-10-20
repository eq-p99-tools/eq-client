//! The script pointer: where a running script's clicks and hovers point, so
//! a recording of a script shows where each one lands. A step that clicks or
//! hovers a control glides the script pointer there first, hovering what it
//! crosses, and the click goes to the control under it as the glide ends.
//! The real pointer is never moved: the script pointer is set only while the
//! real one is outside the window, and is drawn as an arrow of its own.
use std::time::Duration;

use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    ui::{ComputedStackIndex, FocusPolicy},
    window::PrimaryWindow,
};

/// How long the script pointer takes to glide to a control.
pub(super) const GLIDE: Duration = Duration::from_millis(250);

/// Where a running script points, in the window's logical pixels. It is set
/// only while the real pointer is outside the window, starts at the
/// window's middle and stays where the last glide left it; it is forgotten
/// once the script ends.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScriptPointer(pub(crate) Vec2);

/// A glide of the script pointer: where it set out from, when, and the
/// control it heads for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Glide {
    pub(super) from: Vec2,
    pub(super) began: Duration,
    pub(super) target: Entity,
}

/// Where a glide from `from` to `to` has reached `elapsed` into it: slow to
/// start and to stop, and at `to` once [`GLIDE`] is over.
pub(super) fn glide_at(from: Vec2, to: Vec2, elapsed: Duration) -> Vec2 {
    if elapsed >= GLIDE {
        return to;
    }
    let t = elapsed.as_secs_f32() / GLIDE.as_secs_f32();
    from.lerp(to, t * t * (3.0 - 2.0 * t))
}

/// The UI nodes the script pointer may rest on, with where they are drawn
/// and how high. Nodes that take no input are left out: the tooltip, the
/// item on the cursor and the script pointer's own arrow.
type Nodes<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static UiGlobalTransform,
        &'static ComputedNode,
        Option<&'static ComputedStackIndex>,
        Option<&'static InheritedVisibility>,
        Option<&'static FocusPolicy>,
        Has<Interaction>,
    ),
    (
        Without<crate::tooltip::TooltipBox>,
        Without<crate::inventory::cursor::Overlay>,
        Without<PointerArt>,
    ),
>;

/// The script pointer as a step glides it, and what lies under it.
#[derive(SystemParam)]
pub(crate) struct Aim<'w, 's> {
    pointer: Option<ResMut<'w, ScriptPointer>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    nodes: Nodes<'w, 's>,
    clipping: Query<
        'w,
        's,
        (
            &'static ComputedNode,
            &'static UiGlobalTransform,
            &'static Node,
        ),
    >,
    parents: Query<'w, 's, &'static ChildOf, Without<bevy::ui::OverrideClip>>,
}

impl Aim<'_, '_> {
    /// Physical pixels to the window's logical pixel.
    fn scale(&self) -> f32 {
        self.windows.single().map_or(1.0, Window::scale_factor)
    }

    /// Where the script pointer is: where the last glide left it, or the
    /// window's middle before the first.
    pub(super) fn at(&self) -> Vec2 {
        self.pointer.as_ref().map_or_else(
            || {
                self.windows
                    .single()
                    .map_or(Vec2::ZERO, |window| window.size() / 2.0)
            },
            |pointer| pointer.0,
        )
    }

    /// Puts the script pointer at `at`.
    pub(super) fn place(&mut self, commands: &mut Commands, at: Vec2) {
        if let Some(pointer) = self.pointer.as_mut() {
            pointer.set_if_neq(ScriptPointer(at));
        } else {
            commands.insert_resource(ScriptPointer(at));
        }
    }

    /// The middle of a laid-out control, in the window's logical pixels.
    pub(super) fn middle(&self, entity: Entity) -> Option<Vec2> {
        let (_, transform, ..) = self.nodes.get(entity).ok()?;
        Some(transform.translation / self.scale())
    }

    /// The topmost control under `at`, in the window's logical pixels, that
    /// takes the pointer as Bevy's UI focus gives it: a node that blocks the
    /// pointer and takes no input itself, as a window's frame, hides what is
    /// under it. The geometry is the one [`crate::windows::pointer::PointerUi`]
    /// tests: visibility, rounded corners and the clipping of the nodes above.
    pub(super) fn under(&self, at: Vec2) -> Option<Entity> {
        let point = at * self.scale();
        self.nodes
            .iter()
            .filter(
                |(entity, transform, node, _, visibility, focus, interactive)| {
                    (*interactive || focus.is_none_or(|focus| *focus == FocusPolicy::Block))
                        && visibility.is_none_or(|visibility| visibility.get())
                        && node.size().cmpgt(Vec2::ZERO).all()
                        && node.contains_point(**transform, point)
                        && bevy::ui::clip_check_recursive(
                            point,
                            *entity,
                            &self.clipping,
                            &self.parents,
                        )
                },
            )
            .max_by_key(|(_, _, _, stack, ..)| stack.map_or(0, |stack| stack.0))
            .filter(|(.., interactive)| *interactive)
            .map(|(entity, ..)| entity)
    }
}

/// The arrow drawn at the script pointer while a script runs.
#[derive(Component)]
pub(crate) struct PointerArt;

/// The arrow's size in logical pixels; its tip, the point it points with,
/// is its top left corner.
const ARROW: Vec2 = Vec2::new(12.0, 19.0);

/// Pixels of the arrow's picture to each of its logical pixels, so it stays
/// sharp on a high-density display.
const ARROW_DENSITY: u8 = 2;

/// Spawns the hidden arrow above every window and the tooltip. It takes no
/// input: it has no `Interaction`, is no pointer surface and lets the
/// pointer pass.
pub(crate) fn spawn(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let image = images.add(crate::sheets::image(arrow()));
    commands.spawn((
        PointerArt,
        ImageNode::new(image),
        FocusPolicy::Pass,
        GlobalZIndex(2000),
        Node {
            position_type: PositionType::Absolute,
            width: px(ARROW.x),
            height: px(ARROW.y),
            display: Display::None,
            ..default()
        },
    ));
}

/// Draws the arrow at the script pointer while a script runs and hides it
/// otherwise, and forgets the script pointer once the script ends, so the
/// next starts at the window's middle.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn draw(
    mut commands: Commands,
    script: Option<Res<super::Script>>,
    pointer: Option<Res<ScriptPointer>>,
    scale: Option<Res<UiScale>>,
    mut arrows: Query<&mut Node, With<PointerArt>>,
) {
    if script.is_none() && pointer.is_some() {
        commands.remove_resource::<ScriptPointer>();
    }
    let factor = scale.as_ref().map_or(1.0, |scale| scale.0);
    let at = pointer
        .filter(|_| script.is_some())
        .map(|pointer| pointer.0 / factor);
    for mut node in &mut arrows {
        let display = if at.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if let Some(at) = at
            && (node.left != px(at.x) || node.top != px(at.y))
        {
            node.left = px(at.x);
            node.top = px(at.y);
        }
    }
}

/// This client's own arrow, drawn in code rather than read from any skin:
/// the installed skin's files name no picture of the pointer that the skin
/// loader reads. It is white with a black edge, its tip at the top left.
fn arrow() -> image::RgbaImage {
    // The outline in logical pixels, from the tip round the left edge, the
    // tail and back up the right edge.
    const OUTLINE: [Vec2; 7] = [
        Vec2::new(0.0, 0.0),
        Vec2::new(0.0, 16.0),
        Vec2::new(4.0, 12.5),
        Vec2::new(7.0, 18.5),
        Vec2::new(9.5, 17.5),
        Vec2::new(6.5, 11.5),
        Vec2::new(11.5, 11.5),
    ];
    let density = f32::from(ARROW_DENSITY);
    let (width, height) = (
        to_pixels(ARROW.x) * u32::from(ARROW_DENSITY),
        to_pixels(ARROW.y) * u32::from(ARROW_DENSITY),
    );
    // Whether a picture pixel's middle lies within the outline.
    let inside = |x: i64, y: i64| {
        let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
            return false;
        };
        let point = (Vec2::new(f32::from(x), f32::from(y)) + 0.5) / density;
        within(&OUTLINE, point)
    };
    image::RgbaImage::from_fn(width, height, |x, y| {
        let (x, y) = (i64::from(x), i64::from(y));
        if !inside(x, y) {
            return image::Rgba([0, 0, 0, 0]);
        }
        let reach = i64::from(ARROW_DENSITY);
        let edge = (-reach..=reach).any(|dx| (-reach..=reach).any(|dy| !inside(x + dx, y + dy)));
        if edge {
            image::Rgba([0, 0, 0, 255])
        } else {
            image::Rgba([255, 255, 255, 255])
        }
    })
}

/// A whole number of pixels, as the arrow's size is.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the arrow's size is small, whole and positive"
)]
const fn to_pixels(length: f32) -> u32 {
    length as u32
}

/// Whether a point lies within a polygon, by the even-odd rule.
fn within(polygon: &[Vec2], point: Vec2) -> bool {
    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];
    for &corner in polygon {
        if (corner.y > point.y) != (previous.y > point.y)
            && point.x
                < (previous.x - corner.x) * (point.y - corner.y) / (previous.y - corner.y)
                    + corner.x
        {
            inside = !inside;
        }
        previous = corner;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glide_eases_from_where_the_pointer_rests_to_the_control() {
        let (from, to) = (Vec2::new(100.0, 100.0), Vec2::new(300.0, 200.0));
        let at = |ms| glide_at(from, to, Duration::from_millis(ms));
        assert_eq!(at(0), from);
        // Halfway through its time it is halfway there, having started slowly.
        assert!((at(125) - Vec2::new(200.0, 150.0)).length() < 0.01);
        assert!(at(25).distance(from) < from.distance(to) * 0.1 / 2.0);
        // It never overshoots, and rests on the control once its time is up.
        let mut last = 0.0;
        for ms in (0..=250).step_by(10) {
            let gone = at(ms).distance(from);
            assert!(gone >= last && gone <= from.distance(to) + 0.01);
            last = gone;
        }
        assert_eq!(at(250), to);
        assert_eq!(at(400), to);
    }

    #[test]
    fn the_arrow_is_white_inside_a_black_edge_with_its_tip_at_the_corner() {
        let arrow = arrow();
        assert_eq!(
            (arrow.width(), arrow.height()),
            (12 * u32::from(ARROW_DENSITY), 19 * u32::from(ARROW_DENSITY))
        );
        // The tip is drawn, as edge, and the far corner is left clear.
        assert_eq!(arrow.get_pixel(0, 1).0, [0, 0, 0, 255]);
        assert_eq!(arrow.get_pixel(23, 0).0[3], 0);
        // The body is white.
        assert_eq!(arrow.get_pixel(6, 16).0, [255, 255, 255, 255]);
    }
}
