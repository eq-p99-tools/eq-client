//! Landmark controls, navigation guidance and map markers.
use super::{Action, Landmarks, map_at};
use crate::{
    keys::Typing,
    map::{
        MapCanvas, MapMotion, MapView, Projection,
        controls::{self, SearchBox},
    },
    online::OnlineState,
    theme, windows,
};
use bevy::{picking::Pickable, prelude::*};
use eq_client_core::WorldPosition;

const INK: Color = Color::srgb_u8(135, 235, 226);
const PROMPT: &str = "Name or Name | X Y Z";

#[derive(Component)]
pub(crate) struct Panel;

#[derive(Component)]
pub(crate) struct List;

#[derive(Component)]
pub(crate) enum Label {
    Entry,
    Status,
}

/// Landmark editor below the map toolbar, independent of the entity watch list.
pub(in crate::map) fn chrome(parent: &mut ChildSpawnerCommands) {
    use controls::button;
    parent
        .spawn((
            Panel,
            windows::KeepsPress,
            Node {
                position_type: PositionType::Absolute,
                top: px(74),
                left: px(6),
                right: px(6),
                max_width: px(380),
                max_height: percent(65),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(6)),
                row_gap: px(4),
                display: Display::None,
                ..default()
            },
            BackgroundColor(theme::SCRIM),
            ZIndex(16),
        ))
        .with_children(|panel| {
            panel
                .spawn((Label::Entry, SearchBox::Landmark, button(PROMPT)))
                .insert(Node {
                    padding: UiRect::all(px(4)),
                    overflow: Overflow::clip(),
                    ..default()
                });
            panel
                .spawn(Node {
                    column_gap: px(4),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((Action::Save, button("Save landmark")));
                    row.spawn((Action::Remove, button("Remove selected")));
                });
            panel.spawn((
                Label::Status,
                theme::text("", theme::Size::Small, theme::INK),
            ));
            panel.spawn((
                List,
                windows::pointer::TakesWheel,
                ScrollPosition::default(),
                Node {
                    max_height: px(140),
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::scroll_y(),
                    row_gap: px(3),
                    ..default()
                },
            ));
        });
}

/// Range and clockwise bearing from map north, in the horizontal plane.
pub(super) fn guidance(player: WorldPosition, point: [f32; 3]) -> String {
    let delta = Vec2::new(player.x - point[0], point[1] - player.y);
    let range = delta.length();
    if !range.is_finite() {
        return String::new();
    }
    if range == 0.0 {
        return " · Here on map".into();
    }
    let bearing = delta.x.atan2(delta.y).to_degrees().rem_euclid(360.0);
    format!(
        " · {range:.0} units at {:.0}°",
        bearing.round().rem_euclid(360.0)
    )
}

/// Updates the editor and its list. Selecting a list entry centers the map.
#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub(crate) fn present(
    mut commands: Commands,
    state: Res<Landmarks>,
    (view, typing): (Res<MapView>, Res<Typing>),
    online: Res<OnlineState>,
    motion: Res<MapMotion>,
    mut panels: Query<&mut Node, With<Panel>>,
    lists: Query<Entity, With<List>>,
    mut labels: Query<(&Label, &mut Text)>,
    mut drawn: Local<Option<(Entity, u64)>>,
) {
    for mut panel in &mut panels {
        panel.display = if state.open {
            Display::Flex
        } else {
            Display::None
        };
    }
    if !state.open {
        return;
    }
    for (label, mut text) in &mut labels {
        let wanted = match label {
            Label::Entry => {
                let focused = typing.map_search && view.field == SearchBox::Landmark;
                if state.draft.is_empty() && !focused {
                    PROMPT.into()
                } else {
                    format!("{}{}", state.draft, if focused { "|" } else { "" })
                }
            }
            Label::Status => {
                if !state.writable {
                    "Landmarks need a character profile and readable settings".into()
                } else if state
                    .scope
                    .as_ref()
                    .is_some_and(|scope| state.store.pending(&scope.file))
                {
                    "Saving landmarks…".into()
                } else if let Some((name, point)) = state
                    .selected
                    .as_ref()
                    .and_then(|name| Some((name, state.places.get(name)?)))
                {
                    let guide = motion
                        .player(online.world())
                        .map_or_else(String::new, |p| guidance(p, *point));
                    if state.status.is_empty() {
                        format!("{name}{guide}")
                    } else {
                        format!("{} · {name}{guide}", state.status)
                    }
                } else if state.status.is_empty() {
                    "A name alone saves your current position.".into()
                } else {
                    state.status.clone()
                }
            }
        };
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    let Ok(list) = lists.single() else { return };
    if *drawn == Some((list, state.revision)) {
        return;
    }
    commands
        .entity(list)
        .despawn_children()
        .with_children(|panel| {
            for (name, &[x, y, z]) in &state.places {
                panel.spawn((
                    Action::Locate(name.clone()),
                    controls::button(&format!("{name}  ·  X {x:.1}  Y {y:.1}  Z {z:.1}")),
                ));
            }
        });
    *drawn = Some((list, state.revision));
}

#[derive(Component)]
pub(crate) struct Dot;

#[derive(PartialEq)]
pub(crate) struct Drawn {
    canvas: Entity,
    projection: Projection,
    revision: u64,
    labels: bool,
}

/// Put personal landmarks on the same projection as the map geometry and
/// entity markers. These decorations never intercept a canvas click.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn draw(
    mut commands: Commands,
    view: Res<MapView>,
    state: Res<Landmarks>,
    canvases: Query<Entity, With<MapCanvas>>,
    dots: Query<Entity, With<Dot>>,
    mut drawn: Local<Option<Drawn>>,
) {
    let Some((canvas, projection)) = canvases.single().ok().zip(view.projection) else {
        return;
    };
    let wanted = Drawn {
        canvas,
        projection,
        revision: state.revision,
        labels: view.labels,
    };
    if drawn.as_ref() == Some(&wanted) {
        return;
    }
    for dot in &dots {
        commands.entity(dot).despawn();
    }
    commands.entity(canvas).with_children(|canvas| {
        for (label, &point) in &state.places {
            let at = projection.place(map_at(point));
            if !at.is_finite() || at.cmplt(Vec2::ZERO).any() || at.cmpgt(projection.size).any() {
                continue;
            }
            let text = if view.labels {
                format!("◆ {label}")
            } else {
                "◆".into()
            };
            canvas.spawn((
                Dot,
                theme::text(text, theme::Size::Small, INK),
                BackgroundColor(theme::SCRIM),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(at.x - 4.0),
                    top: px(at.y - 6.0),
                    ..default()
                },
                ZIndex(3),
                bevy::ui::FocusPolicy::Pass,
                Pickable::IGNORE,
            ));
        }
    });
    *drawn = Some(wanted);
}
