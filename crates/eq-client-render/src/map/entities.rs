//! Map markers and inspection use the world snapshot, never the nearby 3D
//! model set. Pinning is local; the separate Target button sends a command.
use super::{MapCanvas, MapView, map_point};
use crate::{online::OnlineState, theme, tooltip::Tooltip};
use bevy::prelude::*;
use eq_client_core::{
    SpawnKind, WorldPosition,
    world::{ClientWorld, Spawn},
};
use std::collections::BTreeSet;

#[cfg(test)]
mod tests;

const MARKER_SIZE: f32 = 12.0;
pub(super) const PLAYER: Color = Color::srgb_u8(180, 85, 245);
const CORPSE: Color = Color::srgb_u8(115, 110, 125);

/// A marker's click pins its current entity; Clear releases the inspector.
#[derive(Component, Clone, Copy)]
pub(crate) enum Action {
    Select(u16),
    Clear,
}

#[derive(Component)]
pub(crate) struct Marker(pub(super) u16);

#[derive(Component)]
pub(crate) struct Legend;

#[derive(Component)]
pub(crate) struct Details;

#[derive(Component)]
pub(crate) struct Inspector;

/// An inexpensive borrowed view shared by marker placement and inspection.
#[derive(Clone, Copy)]
pub(super) struct Known<'a> {
    id: u16,
    pub(super) name: &'a str,
    kind: SpawnKind,
    position: WorldPosition,
    level: u8,
    class: Option<u32>,
    invisible: bool,
    own: bool,
    revision: Option<u64>,
}

/// A reused spawn ID must not silently replace the entity being inspected.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Identity {
    pub(super) id: u16,
    revision: Option<u64>,
}

impl Known<'_> {
    pub(super) fn identity(self) -> Identity {
        Identity {
            id: self.id,
            revision: self.revision,
        }
    }

    /// The player's marker shares the displayed pose with their facing arrow.
    fn shown(mut self, world: &ClientWorld, motion: &super::MapMotion) -> Self {
        if self.own
            && let Some(position) = motion.player(world)
        {
            self.position = position;
        }
        self
    }
}

/// All known entities, including the player exactly once. There is no
/// distance, visibility or count filter inherited from the 3D renderer.
fn known(world: &ClientWorld) -> impl Iterator<Item = Known<'_>> {
    let player = world.player();
    player.and_then(known_player).into_iter().chain(
        world
            .spawns()
            .values()
            .filter(move |spawn| {
                player.is_none_or(|player| player.spawn_id != spawn.state.spawn_id)
            })
            .filter_map(known_spawn),
    )
}

fn kind_name(kind: SpawnKind) -> &'static str {
    match kind {
        SpawnKind::Player => "Player",
        SpawnKind::Npc => "NPC",
        SpawnKind::PlayerCorpse => "Player corpse",
        SpawnKind::NpcCorpse => "NPC corpse",
        SpawnKind::Unknown(_) => "Entity",
    }
}

fn color(entity: Known<'_>, world: &ClientWorld, mac: bool) -> Color {
    if entity.own {
        return Color::NONE;
    }
    match entity.kind {
        SpawnKind::Player => PLAYER,
        SpawnKind::Npc => theme::con(world.consider_color(entity.id).or_else(|| {
            eq_client_core::maps::level_con(
                world.player().map_or(0, |player| player.level),
                entity.level,
                mac,
            )
        })),
        SpawnKind::PlayerCorpse | SpawnKind::NpcCorpse | SpawnKind::Unknown(_) => CORPSE,
    }
}

#[derive(Clone, Copy)]
struct Look {
    fill: Color,
    border: Color,
    diameter: f32,
    depth: i32,
}

/// Emphasis keeps the level/PC fill color, using size and outlines instead.
fn look(
    entity: Known<'_>,
    world: &ClientWorld,
    view: &MapView,
    watches: &eq_client_core::maps::Watches,
    mac: bool,
    hovered: bool,
) -> Look {
    let pinned = view.selected == Some(entity.identity());
    let tracked = watches.contains(entity.name);
    let matched = !view.query.is_empty() && eq_client_core::maps::matches(entity.name, &view.query);
    let emphasized = pinned || tracked || matched;
    let dimmed = !view.query.is_empty() && !emphasized && !entity.own;
    let fill = color(entity, world, mac);
    let border = if pinned {
        theme::INK_BRIGHT
    } else if tracked {
        super::TRACKED_INK
    } else if matched || hovered {
        theme::FOCUS
    } else if entity.own {
        Color::NONE
    } else {
        Color::BLACK
    };
    Look {
        fill: if dimmed { fill.with_alpha(0.2) } else { fill },
        border: if dimmed {
            border.with_alpha(0.2)
        } else {
            border
        },
        diameter: if emphasized { 16.0 } else { MARKER_SIZE },
        depth: if pinned {
            5
        } else if emphasized {
            4
        } else {
            2
        },
    }
}

/// Describe the actual snapshot; unknown HP/class/level are not invented.
fn describe(entity: Known<'_>, world: &ClientWorld, motion: &super::MapMotion) -> String {
    let name = eq_client_core::entities::display_name(entity.name);
    let level = if entity.level == 0 {
        "?".into()
    } else {
        entity.level.to_string()
    };
    let class = entity
        .class
        .and_then(eq_client_core::classes::class_name)
        .map_or_else(String::new, |class| format!(" · {class}"));
    let health = world
        .health(entity.id)
        .map_or_else(|| "not reported".into(), |hp| format!("{hp}%"));
    let p = entity.position;
    let distance = motion
        .player(world)
        .filter(|_| !entity.own)
        .map_or_else(String::new, |own| {
            let distance = (p.x - own.x).hypot(p.y - own.y).hypot(p.z - own.z);
            format!("\nDistance {distance:.1}")
        });
    format!(
        "{name}{}\n{} · Level {level}{class} · HP {health}\nX {:.1}   Y {:.1}   Z {:.1}{distance}\nEntity {}{} · {}",
        if entity.own { " (you)" } else { "" },
        kind_name(entity.kind),
        p.x,
        p.y,
        p.z,
        entity.id,
        if entity.invisible {
            " · invisible"
        } else {
            ""
        },
        if entity.own {
            "displayed position"
        } else {
            "last reported position"
        }
    )
}

/// Pins a marker's details. Repeated clicks cycle coincident markers, so a
/// crowded point does not make all but the frontmost entity inaccessible.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn inspect(
    clicks: crate::clicks::Clicks<Action>,
    online: Res<OnlineState>,
    motion: Res<super::MapMotion>,
    mut view: ResMut<MapView>,
) {
    for action in clicks.iter() {
        match *action {
            Action::Clear => view.selected = None,
            Action::Select(id) => {
                view.selected = pick(online.world(), &view, &motion, id);
            }
        }
    }
}

fn pick(
    world: &ClientWorld,
    view: &MapView,
    motion: &super::MapMotion,
    clicked: u16,
) -> Option<Identity> {
    if view.admission != world.session_id() {
        return None;
    }
    let projection = view.projection?;
    let target = entity(world, clicked)?.shown(world, motion);
    let at = projection.place(map_point(target.position));
    let mut nearby: Vec<_> = known(world)
        .map(|entity| entity.shown(world, motion))
        .filter(|entity| {
            projection.place(map_point(entity.position)).distance(at) <= MARKER_SIZE / 2.0
        })
        .map(Known::identity)
        .collect();
    nearby.sort_unstable_by_key(|entity| entity.id);
    view.selected
        .and_then(|selected| nearby.iter().position(|id| *id == selected))
        .map_or(Some(target.identity()), |index| {
            nearby.get((index + 1) % nearby.len()).copied()
        })
}

/// Validate before an input action as well as during rendering: network events
/// can replace an ID or admit a new zone earlier in the same frame as a click.
pub(super) fn selected<'a>(world: &'a ClientWorld, view: &MapView) -> Option<Known<'a>> {
    if view.admission != world.session_id() {
        return None;
    }
    let selected = view.selected?;
    entity(world, selected.id).filter(|entity| entity.identity() == selected)
}

#[cfg(test)]
pub(super) fn select_preview(view: &mut MapView, world: &ClientWorld, id: u16) {
    view.selected = pick(world, view, &super::MapMotion::default(), id);
}

type Markers<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Marker,
        &'static mut Node,
        &'static mut BackgroundColor,
        &'static mut BorderColor,
        &'static mut Tooltip,
        &'static Interaction,
        &'static mut ZIndex,
        &'static mut ImageNode,
        &'static mut UiTransform,
    ),
>;

/// Reconcile lightweight UI dots with all known entities. Stationary dots
/// keep their nodes, and tooltip text is formatted only while inspected.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn update(
    mut commands: Commands,
    online: Res<OnlineState>,
    motion: Res<super::MapMotion>,
    mut view: ResMut<MapView>,
    settings: Res<crate::ViewerSettings>,
    watches: Res<super::watch::WatchState>,
    canvases: Query<Entity, With<MapCanvas>>,
    mut markers: Markers,
    mut legends: Query<&mut Text, (With<Legend>, Without<Details>)>,
    mut details: Query<&mut Text, (With<Details>, Without<Legend>)>,
    mut inspectors: Query<&mut Node, (With<Inspector>, Without<Marker>)>,
    mut images: ResMut<Assets<Image>>,
    mut arrow: Local<Option<Handle<Image>>>,
) {
    let Ok(canvas) = canvases.single() else {
        return;
    };
    let Some(projection) = view.projection else {
        return;
    };
    let world = online.world();
    let arrow =
        arrow.get_or_insert_with(|| images.add(super::arrow_picture([255; 4], [0, 0, 0, 255])));
    let mac = settings.0.installed_client == eq_client_assets::ui::InstalledClient::EqMac;
    let selected = selected(world, &view).map(|entity| entity.shown(world, &motion));
    if selected.is_none() {
        view.selected = None;
    }
    let mut drawn = BTreeSet::new();
    for (
        node_entity,
        marker,
        mut node,
        mut background,
        mut border,
        mut tooltip,
        interaction,
        mut depth,
        mut image,
        mut transform,
    ) in &mut markers
    {
        let Some(entity) = entity(world, marker.0) else {
            commands.entity(node_entity).despawn();
            continue;
        };
        let entity = entity.shown(world, &motion);
        drawn.insert(entity.id);
        let at = projection.place(map_point(entity.position));
        let look = look(
            entity,
            world,
            &view,
            &watches.names,
            mac,
            *interaction != Interaction::None,
        );
        place(&mut node, at, projection.size, look.diameter);
        background.set_if_neq(BackgroundColor(look.fill));
        border.set_if_neq(BorderColor::all(look.border));
        shape(
            &mut node,
            &mut background,
            &mut border,
            &mut image,
            &mut transform,
            entity,
            look,
            arrow,
        );
        depth.set_if_neq(ZIndex(look.depth));
        if *interaction != Interaction::None {
            let text = describe(entity, world, &motion);
            if tooltip.0 != text {
                tooltip.0 = text;
            }
        } else if !tooltip.0.is_empty() {
            tooltip.0.clear();
        }
    }
    let mut counts = [0usize; 3];
    for entity in known(world).map(|entity| entity.shown(world, &motion)) {
        match entity.kind {
            SpawnKind::Player => counts[0] += 1,
            SpawnKind::Npc => counts[1] += 1,
            _ => counts[2] += 1,
        }
        if drawn.contains(&entity.id) {
            continue;
        }
        add_marker(
            &mut commands,
            canvas,
            entity,
            projection,
            look(entity, world, &view, &watches.names, mac, false),
            arrow,
        );
    }
    if legends.is_empty() {
        chrome(&mut commands, canvas);
    }
    let legend = legend_text(&view, counts);
    for mut text in &mut legends {
        if text.0 != legend {
            text.0.clone_from(&legend);
        }
    }
    let shown = selected.map(|selected| describe(selected, world, &motion));
    show_inspector(shown.as_deref(), &mut inspectors, &mut details);
}

/// Keep the pinned details visible only while their entity is still known.
fn show_inspector(
    shown: Option<&str>,
    inspectors: &mut Query<&mut Node, (With<Inspector>, Without<Marker>)>,
    details: &mut Query<&mut Text, (With<Details>, Without<Legend>)>,
) {
    for mut node in inspectors.iter_mut() {
        let display = if shown.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for mut text in details.iter_mut() {
        let wanted = shown.unwrap_or_default();
        if text.0 != wanted {
            wanted.clone_into(&mut text.0);
        }
    }
}

fn add_marker(
    commands: &mut Commands,
    canvas: Entity,
    entity: Known<'_>,
    projection: super::Projection,
    look: Look,
    arrow: &Handle<Image>,
) {
    let at = projection.place(map_point(entity.position));
    let mut node = Node {
        position_type: PositionType::Absolute,
        width: px(MARKER_SIZE),
        height: px(MARKER_SIZE),
        border: UiRect::all(px(1)),
        ..default()
    };
    place(&mut node, at, projection.size, look.diameter);
    let mut background = BackgroundColor(look.fill);
    let mut border = BorderColor::all(look.border);
    let mut image = ImageNode::default();
    let mut transform = UiTransform::default();
    shape(
        &mut node,
        &mut background,
        &mut border,
        &mut image,
        &mut transform,
        entity,
        look,
        arrow,
    );
    commands.entity(canvas).with_children(|parent| {
        parent
            .spawn((
                Button,
                Marker(entity.id),
                Action::Select(entity.id),
                node,
                background,
                border,
                image,
                transform,
                Tooltip::default(),
                ZIndex(look.depth),
            ))
            .with_child(super::motion::indicator());
    });
}

/// Players face their reported heading even while stationary; NPCs are circles.
/// The local player's existing arrow remains responsible for its predicted pose.
#[allow(clippy::too_many_arguments)]
fn shape(
    node: &mut Node,
    background: &mut BackgroundColor,
    border: &mut BorderColor,
    image: &mut ImageNode,
    transform: &mut UiTransform,
    entity: Known<'_>,
    look: Look,
    arrow: &Handle<Image>,
) {
    let unknown_facing = entity.kind == SpawnKind::Player && !entity.position.heading.is_finite();
    node.border_radius = if entity.kind == SpawnKind::Npc || unknown_facing {
        BorderRadius::all(percent(50))
    } else {
        BorderRadius::ZERO
    };
    if entity.kind == SpawnKind::Player && !entity.own && !unknown_facing {
        background.0 = Color::NONE;
        image.image = arrow.clone();
        image.color = look.fill;
        transform.rotation = super::facing(entity.position.heading);
        // Only an emphasized player needs an extra selection outline.
        if look.diameter == MARKER_SIZE {
            *border = BorderColor::all(Color::NONE);
        }
    } else {
        image.color = Color::NONE;
        transform.rotation = Rot2::IDENTITY;
        if entity.own && unknown_facing {
            background.0 = PLAYER;
        }
    }
}

fn legend_text(view: &MapView, counts: [usize; 3]) -> String {
    let geometry = view
        .zone
        .as_ref()
        .is_some_and(|(_, map)| map.as_ref().is_some_and(|map| !map.lines.is_empty()));
    format!(
        "{} players · {} NPCs · {} corpses / other\n{}",
        counts[0],
        counts[1],
        counts[2],
        if geometry {
            "Hover for details · click to pin"
        } else {
            "No local map lines · showing known positions"
        }
    )
}

/// Resolve a single entity without scanning the complete spawn collection.
pub(super) fn entity(world: &ClientWorld, id: u16) -> Option<Known<'_>> {
    if let Some(player) = world.player().filter(|player| player.spawn_id == id) {
        return known_player(player);
    }
    world.spawn(id).and_then(known_spawn)
}

fn known_spawn(spawn: &Spawn) -> Option<Known<'_>> {
    let state = &spawn.state;
    finite(state.position).then_some(Known {
        id: state.spawn_id,
        name: &state.name,
        kind: state.kind,
        position: state.position,
        level: state.level,
        class: state.class.map(u32::from),
        invisible: state.invisible,
        own: false,
        revision: Some(spawn.revision),
    })
}

fn known_player(player: &eq_client_core::PlayerState) -> Option<Known<'_>> {
    finite(player.position).then_some(Known {
        id: player.spawn_id,
        name: &player.name,
        kind: SpawnKind::Player,
        position: player.position,
        level: player.level,
        class: player.class,
        invisible: false,
        own: true,
        revision: None,
    })
}

pub(super) fn finite(position: WorldPosition) -> bool {
    [position.x, position.y, position.z]
        .into_iter()
        .all(f32::is_finite)
}

/// Clip markers to the map; off-screen dots remain
/// tracked and reappear when panned back without reloading a model.
pub(super) fn place(node: &mut Node, at: Vec2, size: Vec2, diameter: f32) {
    if node.width != px(diameter) {
        node.width = px(diameter);
    }
    if node.height != px(diameter) {
        node.height = px(diameter);
    }
    let display = if at.is_finite() && at.cmpge(Vec2::ZERO).all() && at.cmple(size).all() {
        Display::Flex
    } else {
        Display::None
    };
    if node.display != display {
        node.display = display;
    }
    if !at.is_finite() {
        return;
    }
    let left = px(at.x - diameter / 2.0);
    let top = px(at.y - diameter / 2.0);
    if node.left != left {
        node.left = left;
    }
    if node.top != top {
        node.top = top;
    }
}

/// A compact legend and a dismissible inspector over either kind of map.
fn chrome(commands: &mut Commands, canvas: Entity) {
    commands.entity(canvas).with_children(|parent| {
        parent.spawn((
            Legend,
            theme::text("", theme::Size::Small, theme::INK_BRIGHT),
            Node {
                position_type: PositionType::Absolute,
                left: px(6),
                top: px(77),
                padding: UiRect::all(px(5)),
                ..default()
            },
            BackgroundColor(theme::SCRIM),
            ZIndex(10),
            bevy::ui::FocusPolicy::Pass,
        ));
        parent
            .spawn((
                Inspector,
                crate::windows::KeepsPress,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(6),
                    right: px(6),
                    bottom: px(6),
                    padding: UiRect::all(px(8)),
                    column_gap: px(6),
                    align_items: AlignItems::Start,
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(theme::SCRIM),
                ZIndex(12),
            ))
            .with_children(|panel| {
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4),
                        ..default()
                    })
                    .with_children(|actions| {
                        actions.spawn((
                            super::controls::Control::WatchSelected,
                            super::controls::Label::SelectedWatch,
                            super::controls::button("Track"),
                            Tooltip("Toggle name highlighting and appearance alerts.".into()),
                        ));
                        actions.spawn((
                            super::target::TargetButton,
                            super::controls::button("Target"),
                            Tooltip::default(),
                        ));
                    });
                panel.spawn((
                    Details,
                    theme::text("", theme::Size::Small, theme::INK_BRIGHT),
                    Node {
                        flex_grow: 1.0,
                        min_width: px(0),
                        ..default()
                    },
                ));
                panel.spawn((
                    Button,
                    Action::Clear,
                    theme::text("Unpin", theme::Size::Small, theme::INK_BRIGHT),
                    Node {
                        padding: UiRect::all(px(5)),
                        ..default()
                    },
                    BackgroundColor(theme::BUTTON),
                ));
            });
        super::controls::chrome(parent);
    });
}
