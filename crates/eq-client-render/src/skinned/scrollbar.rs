//! The skin's vertical scrollbars, beside the boxes that scroll: the chat's
//! output and the lists. Its arrows, its gutter and its thumb scroll the box
//! as the wheel does: each press becomes this frame's wheel for the surface
//! that takes the wheel for the box, which scrolls it its own way.
use super::{Area, SkinButton, WindowId, at, to_f32};
use crate::windows::pointer::{LINE_PIXELS, TakesWheel, Wheel};
use bevy::{prelude::*, ui::RelativeCursorPosition, window::PrimaryWindow};
use eq_client_assets::sidl::{Piece, ScrollbarLook};
use std::time::Duration;

/// How long an arrow is held before it scrolls again, and how often it then
/// scrolls; the official client's timing is not checked yet.
const REPEAT_AFTER: Duration = Duration::from_millis(400);
const REPEAT_EVERY: Duration = Duration::from_millis(60);

/// A scrollbar, and the box whose content it scrolls.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct Scrollbar {
    scrolled: Entity,
}

/// An arrow at an end of a scrollbar.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ScrollArrow {
    bar: Entity,
    /// The window its scrollbar is drawn in.
    pub(crate) window: WindowId,
    /// Whether it scrolls toward the top.
    pub(crate) up: bool,
}

/// The gutter between a scrollbar's arrows, where its thumb runs.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct Gutter {
    bar: Entity,
}

/// A scrollbar's thumb, as long as the part of the box in view and as far
/// down the gutter as the view is down the box.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct Thumb {
    bar: Entity,
    /// The shortest it gets: its two ends.
    ends: f32,
}

/// The scrollbar's width: its arrows'.
pub(super) fn width(look: &ScrollbarLook) -> f32 {
    look.up
        .normal
        .as_ref()
        .or(look.down.normal.as_ref())
        .map_or(12.0, |piece| to_f32(piece.width))
}

/// Draws a scrollbar down the right of a box's inside, for the content of
/// `scrolled`: an arrow at each end and the gutter between them, with the
/// thumb in it.
pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    look: &ScrollbarLook,
    inside: &Area,
    (scrolled, window): (Entity, WindowId),
) {
    let width = width(look);
    let height = |piece: Option<&Piece>| piece.map_or(width, |piece| to_f32(piece.height));
    let up = height(look.up.normal.as_ref());
    let down = height(look.down.normal.as_ref());
    parent
        .spawn((
            Scrollbar { scrolled },
            at(
                inside.x + inside.width - width,
                inside.y,
                width,
                inside.height,
            ),
        ))
        .with_children(|bar| {
            let id = bar.target_entity();
            let arrow = |bar: &mut ChildSpawnerCommands,
                         art: &mut crate::sheets::Art,
                         look: &eq_client_assets::sidl::ButtonLook,
                         (node, up): (Node, bool)| {
                let mut arrow = bar.spawn((
                    Button,
                    SkinButton(look.clone()),
                    ScrollArrow {
                        bar: id,
                        window,
                        up,
                    },
                    node,
                ));
                if let Some(image) = look.normal.as_ref().and_then(|piece| art.cut(piece)) {
                    arrow.insert(image);
                }
            };
            arrow(bar, art, &look.up, (at(0.0, 0.0, width, up), true));
            gutter(
                bar,
                art,
                look,
                (id, width, up, (inside.height - up - down).max(0.0)),
            );
            arrow(
                bar,
                art,
                &look.down,
                (at(0.0, inside.height - down, width, down), false),
            );
        });
}

/// A scrollbar's gutter, this far down it and this long, with its thumb.
fn gutter(
    bar: &mut ChildSpawnerCommands,
    art: &mut crate::sheets::Art,
    look: &ScrollbarLook,
    (id, width, top, length): (Entity, f32, f32, f32),
) {
    let height = |piece: Option<&Piece>| piece.map_or(width, |piece| to_f32(piece.height));
    let mut gutter = bar.spawn((
        Gutter { bar: id },
        Interaction::default(),
        RelativeCursorPosition::default(),
        at(0.0, top, width, length),
    ));
    if let Some(image) = look.gutter.as_deref().and_then(|file| art.texture(file)) {
        gutter.insert(ImageNode {
            image,
            color: look
                .gutter_tint
                .map_or(Color::WHITE, |[r, g, b]| Color::srgb_u8(r, g, b)),
            image_mode: NodeImageMode::Tiled {
                tile_x: true,
                tile_y: true,
                stretch_value: 1.0,
            },
            ..default()
        });
    }
    let [top, middle, bottom] = &look.thumb;
    let ends = height(top.as_ref()) + height(bottom.as_ref());
    gutter.with_children(|gutter| {
        gutter
            .spawn((
                Thumb { bar: id, ends },
                Interaction::default(),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(0),
                    width: px(width),
                    height: px(ends),
                    flex_direction: FlexDirection::Column,
                    display: Display::None,
                    ..default()
                },
            ))
            .with_children(|thumb| {
                for (piece, grows) in [(top, false), (middle, true), (bottom, false)] {
                    let Some(image) = piece.as_ref().and_then(|piece| art.cut(piece)) else {
                        continue;
                    };
                    let node = Node {
                        width: px(width),
                        height: if grows {
                            Val::Auto
                        } else {
                            px(height(piece.as_ref()))
                        },
                        flex_grow: if grows { 1.0 } else { 0.0 },
                        flex_shrink: 0.0,
                        ..default()
                    };
                    thumb.spawn((image, node));
                }
            });
    });
}

/// Where a box's view is: how far it scrolls, how far down it is and how
/// much of it shows, in the UI's pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct View {
    travel: f32,
    down: f32,
    shown: f32,
}

impl View {
    fn of(node: &ComputedNode, position: &ScrollPosition) -> Self {
        let scale = node.inverse_scale_factor();
        let shown = node.size().y * scale;
        let travel = (node.content_size().y * scale - shown).max(0.0);
        Self {
            travel,
            down: position.y.clamp(0.0, travel),
            shown,
        }
    }

    /// The thumb's length and its top in a gutter this long, or None while
    /// all of the box shows.
    fn thumb(self, gutter: f32, ends: f32) -> Option<(f32, f32)> {
        if self.travel <= 0.0 {
            return None;
        }
        let length = (gutter * self.shown / (self.shown + self.travel))
            .max(ends)
            .min(gutter);
        Some((length, (gutter - length) * self.down / self.travel))
    }
}

/// Places each thumb for the part of its box in view, and hides it while all
/// of the box shows.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn place(
    bars: Query<&Scrollbar>,
    boxes: Query<(&ComputedNode, &ScrollPosition)>,
    gutters: Query<(&Gutter, &ComputedNode)>,
    mut thumbs: Query<(&Thumb, &ChildOf, &mut Node)>,
) {
    for (thumb, parent, mut node) in &mut thumbs {
        let Ok((_, gutter)) = gutters.get(parent.parent()) else {
            continue;
        };
        let length = gutter.size().y * gutter.inverse_scale_factor();
        let placed = bars
            .get(thumb.bar)
            .ok()
            .and_then(|bar| boxes.get(bar.scrolled).ok())
            .and_then(|(node, position)| View::of(node, position).thumb(length, thumb.ends));
        let (display, height, top) = match placed {
            Some((height, top)) => (Display::Flex, px(height), px(top)),
            None => (Display::None, node.height, node.top),
        };
        if node.display != display || node.height != height || node.top != top {
            node.display = display;
            node.height = height;
            node.top = top;
        }
    }
}

/// An arrow held down, and when it next scrolls; a thumb being dragged, by
/// where in it the pointer took it.
#[derive(Default)]
pub(crate) struct Held {
    arrow: Option<(Entity, Duration)>,
    thumb: Option<(Entity, f32)>,
}

/// The parts of a gutter a press and its span are read from.
type GutterParts = (
    &'static Gutter,
    Ref<'static, Interaction>,
    &'static RelativeCursorPosition,
    &'static ComputedNode,
    &'static UiGlobalTransform,
);

/// The boxes scrollbars scroll, and how to find what takes the wheel for
/// one: the box, or the nearest of its ancestors that does.
type Boxes<'w, 's> = (
    Query<'w, 's, (&'static ComputedNode, &'static ScrollPosition)>,
    Query<'w, 's, &'static ChildOf>,
    Query<'w, 's, (), With<TakesWheel>>,
);

/// The parts of the scrollbars a press reaches.
type Parts<'w, 's> = (
    Query<'w, 's, (Entity, &'static ScrollArrow, &'static Interaction)>,
    Query<'w, 's, GutterParts>,
    Query<
        'w,
        's,
        (
            Entity,
            &'static Thumb,
            Ref<'static, Interaction>,
            &'static Node,
        ),
    >,
);

/// Turns a press on a scrollbar into this frame's wheel for its box: an
/// arrow scrolls a line, and again while it is held; the gutter pages toward
/// the press; the thumb, dragged, puts the view where it is dragged.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)] // Bevy system parameters are value wrappers.
pub(crate) fn scroll(
    (time, mouse): (Res<Time<Real>>, Res<ButtonInput<MouseButton>>),
    (windows, scale): (Query<&Window, With<PrimaryWindow>>, Option<Res<UiScale>>),
    bars: Query<&Scrollbar>,
    (boxes, parents, surfaces): Boxes,
    (arrows, gutters, thumbs): Parts,
    mut held: Local<Held>,
    mut wheel: ResMut<Wheel>,
) {
    let now = time.elapsed();
    // The box a part of a scrollbar scrolls, and where its view is.
    let view = |bar: Entity| {
        let bar = bars.get(bar).ok()?;
        let (node, position) = boxes.get(bar.scrolled).ok()?;
        Some((bar.scrolled, View::of(node, position)))
    };
    let mut turn = None;
    // An arrow scrolls as it is pressed, then again while it is held.
    let pressed = arrows
        .iter()
        .find(|(_, _, interaction)| **interaction == Interaction::Pressed);
    match (pressed, held.arrow) {
        (Some((entity, arrow, _)), Some((held_arrow, next))) if entity == held_arrow => {
            if now >= next {
                held.arrow = Some((entity, now + REPEAT_EVERY));
                turn = Some((arrow.bar, line(arrow.up)));
            }
        }
        (Some((entity, arrow, _)), _) => {
            held.arrow = Some((entity, now + REPEAT_AFTER));
            turn = Some((arrow.bar, line(arrow.up)));
        }
        (None, _) => held.arrow = None,
    }
    // The gutter pages toward where it is pressed: above the thumb up, below
    // it down.
    for (gutter, interaction, cursor, node, _) in &gutters {
        if !interaction.is_changed() || *interaction != Interaction::Pressed {
            continue;
        }
        let (Some(at), Some((_, view))) = (cursor.normalized, view(gutter.bar)) else {
            continue;
        };
        let length = node.size().y * node.inverse_scale_factor();
        let Some((thumb, top)) = view.thumb(length, 0.0) else {
            continue;
        };
        let pressed = (at.y + 0.5) * length;
        let page = view.shown.max(LINE_PIXELS);
        if pressed < top {
            turn = Some((gutter.bar, page));
        } else if pressed > top + thumb {
            turn = Some((gutter.bar, -page));
        }
    }
    // The thumb follows the pointer while it is dragged.
    let pointer = windows
        .single()
        .ok()
        .and_then(Window::cursor_position)
        .map(|at| at.y / scale.as_ref().map_or(1.0, |scale| scale.0));
    for (entity, thumb, interaction, node) in &thumbs {
        if interaction.is_changed()
            && *interaction == Interaction::Pressed
            && let (Some(pointer), Val::Px(top)) = (pointer, node.top)
            && let Some(gutter_top) = gutter_top(&gutters, thumb.bar)
        {
            held.thumb = Some((entity, pointer - gutter_top - top));
        }
    }
    if !mouse.pressed(MouseButton::Left) {
        held.thumb = None;
    }
    if let Some((entity, grab)) = held.thumb
        && let Ok((_, thumb, _, node)) = thumbs.get(entity)
        && let (Some(pointer), Val::Px(length)) = (pointer, node.height)
        && let Some((gutter_top, gutter_length)) = gutter_span(&gutters, thumb.bar)
        && let Some((_, view)) = view(thumb.bar)
    {
        let room = (gutter_length - length).max(1.0);
        let fraction = ((pointer - gutter_top - grab) / room).clamp(0.0, 1.0);
        let wanted = fraction * view.travel;
        if (wanted - view.down).abs() >= 0.5 {
            turn = Some((thumb.bar, view.down - wanted));
        }
    }
    // The box takes it as it takes the wheel.
    if let Some((bar, pixels)) = turn
        && let Some((scrolled, _)) = view(bar)
        && let Some(surface) = wheel_surface(scrolled, &parents, &surfaces)
    {
        *wheel = Wheel {
            pixels,
            lines: 0.0,
            surface: Some(surface),
        };
    }
}

/// A line's scroll toward the top or the bottom, as the wheel turns it.
const fn line(up: bool) -> f32 {
    if up { LINE_PIXELS } else { -LINE_PIXELS }
}

/// The top of a scrollbar's gutter, in the UI's pixels.
fn gutter_top(gutters: &Query<'_, '_, GutterParts>, bar: Entity) -> Option<f32> {
    gutter_span(gutters, bar).map(|(top, _)| top)
}

/// The top and length of a scrollbar's gutter, in the UI's pixels.
fn gutter_span(gutters: &Query<'_, '_, GutterParts>, bar: Entity) -> Option<(f32, f32)> {
    gutters
        .iter()
        .find(|(gutter, ..)| gutter.bar == bar)
        .map(|(_, _, _, node, transform)| {
            let scale = node.inverse_scale_factor();
            let length = node.size().y * scale;
            (transform.translation.y * scale - length / 2.0, length)
        })
}

/// What takes the wheel for a box: the box itself, or the nearest of its
/// ancestors that does, as the chat's window does for its lines.
fn wheel_surface(
    scrolled: Entity,
    parents: &Query<&ChildOf>,
    surfaces: &Query<(), With<TakesWheel>>,
) -> Option<Entity> {
    let mut next = Some(scrolled);
    while let Some(entity) = next {
        if surfaces.contains(entity) {
            return Some(entity);
        }
        next = parents.get(entity).ok().map(ChildOf::parent);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thumb_is_as_long_as_the_view_and_as_far_down() {
        let view = View {
            travel: 300.0,
            down: 150.0,
            shown: 100.0,
        };
        // A quarter of the box shows, halfway down.
        assert_eq!(view.thumb(200.0, 8.0), Some((50.0, 75.0)));
        // Never shorter than its ends, nor longer than the gutter.
        let long = View {
            travel: 100_000.0,
            ..view
        };
        assert_eq!(long.thumb(200.0, 8.0).map(|(length, _)| length), Some(8.0));
        // All of the box shows: no thumb.
        let all = View {
            travel: 0.0,
            down: 0.0,
            shown: 100.0,
        };
        assert_eq!(all.thumb(200.0, 8.0), None);
    }

    /// An app that runs the scrollbars' input, with the chat's lines showing
    /// a quarter of their content in a box 100 pixels high, the chat's window
    /// taking the wheel for them, and their scrollbar.
    fn scrolling() -> (App, Entity, Entity) {
        let mut app = crate::testing::app();
        app.init_resource::<Wheel>()
            .init_resource::<Time<Real>>()
            .add_systems(Update, scroll);
        let window = app.world_mut().spawn(TakesWheel).id();
        let lines = app
            .world_mut()
            .spawn((
                ComputedNode {
                    size: Vec2::new(100.0, 100.0),
                    content_size: Vec2::new(100.0, 400.0),
                    inverse_scale_factor: 1.0,
                    ..default()
                },
                ScrollPosition::default(),
                ChildOf(window),
            ))
            .id();
        let bar = app.world_mut().spawn(Scrollbar { scrolled: lines }).id();
        (app, window, bar)
    }

    /// A gutter 200 pixels long from the top of the screen, pressed this far
    /// from its middle, as a fraction of its length.
    fn gutter(bar: Entity, pressed: Option<f32>) -> impl Bundle {
        (
            Gutter { bar },
            if pressed.is_some() {
                Interaction::Pressed
            } else {
                Interaction::None
            },
            RelativeCursorPosition {
                cursor_over: pressed.is_some(),
                normalized: pressed.map(|y| Vec2::new(0.0, y)),
            },
            ComputedNode {
                size: Vec2::new(12.0, 200.0),
                inverse_scale_factor: 1.0,
                ..default()
            },
            UiGlobalTransform::from_xy(6.0, 100.0),
        )
    }

    #[test]
    fn the_gutter_pages_toward_where_it_is_pressed() {
        let (mut app, window, bar) = scrolling();
        // Pressed below the thumb, which is at the top: a view down.
        app.world_mut().spawn(gutter(bar, Some(0.4)));
        app.update();
        let wheel = app.world().resource::<Wheel>();
        assert_eq!((wheel.pixels, wheel.surface), (-100.0, Some(window)));
    }

    #[test]
    fn a_dragged_thumb_puts_the_view_where_it_is_dragged() {
        let (mut app, window, bar) = scrolling();
        let gutter = app.world_mut().spawn(gutter(bar, None)).id();
        app.world_mut().spawn((
            Thumb { bar, ends: 8.0 },
            Interaction::Pressed,
            Node {
                top: px(0),
                height: px(50),
                ..default()
            },
            ChildOf(gutter),
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        let point = |app: &mut App, y: f32| {
            let mut windows = app
                .world_mut()
                .query_filtered::<&mut Window, With<PrimaryWindow>>();
            windows
                .single_mut(app.world_mut())
                .unwrap()
                .set_cursor_position(Some(Vec2::new(6.0, y)));
        };
        // Taken 10 pixels down the thumb, then dragged halfway down the
        // gutter's room, it puts the view halfway down.
        point(&mut app, 10.0);
        app.update();
        assert_eq!(app.world().resource::<Wheel>().surface, None);
        point(&mut app, 85.0);
        app.update();
        let wheel = app.world().resource::<Wheel>();
        assert_eq!((wheel.pixels, wheel.surface), (-150.0, Some(window)));
    }

    #[test]
    fn an_arrow_scrolls_its_box_as_the_wheel_does() {
        let (mut app, window, bar) = scrolling();
        app.world_mut().spawn((
            ScrollArrow {
                bar,
                window: WindowId::Chat,
                up: false,
            },
            Interaction::Pressed,
        ));
        app.update();
        let wheel = app.world().resource::<Wheel>();
        assert_eq!((wheel.pixels, wheel.surface), (-LINE_PIXELS, Some(window)));
        // Held down, it waits before it scrolls again.
        *app.world_mut().resource_mut::<Wheel>() = Wheel::default();
        app.update();
        assert_eq!(app.world().resource::<Wheel>().surface, None);
    }
}
