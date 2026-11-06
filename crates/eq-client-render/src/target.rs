//! Target selection never moves, attacks, casts, or sends chat.
mod door_click;
pub(super) mod marker;
pub(super) mod picking;
use super::{entities::NearbyEntities, online::OnlineState};
use crate::theme::{self, Size};
use crate::{keys::Act, outbox::Outbox};
use bevy::{prelude::*, window::PrimaryWindow};
use door_click::{Door, DoorClicks};
use eq_client_assets::ui::{ClickMiss, InstalledClient, SpawnPick, TargetByName};
use eq_client_core::{ClientCommand, SpawnKind, targeting::cycle};
use picking::PosedBox;

#[derive(Component)]
pub(super) struct TargetPanel;
#[derive(Component)]
pub(super) struct TargetName;
#[derive(Component)]
pub(super) struct TargetDetails;
#[derive(Component)]
pub(super) struct TargetHp;

/// Creates an always-visible compact target panel.
pub(super) fn spawn(commands: &mut Commands) {
    let frame = commands
        .spawn((
            super::hud::HudRoot,
            TargetPanel,
            GlobalZIndex(super::windows::Layer::Hud.base()),
            super::windows::placed(
                super::windows::WindowId::Target,
                Node {
                    width: px(280),
                    padding: UiRect::all(px(10)),
                    border: UiRect::all(px(1)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(5),
                    ..default()
                },
            ),
            theme::surface(),
        ))
        .id();
    super::windows::passive(commands, frame);
    super::windows::identify(commands, frame, super::windows::WindowId::Target);
    commands.entity(frame).with_children(|panel| {
        panel.spawn((
            TargetName,
            Text::new("No target"),
            theme::font(Size::Heading),
            TextColor(theme::INK_WARM),
        ));
        panel
            .spawn((
                Node {
                    height: px(5),
                    width: percent(100),
                    ..default()
                },
                BackgroundColor(theme::WELL),
            ))
            .with_children(|bar| {
                bar.spawn((
                    TargetHp,
                    Node {
                        height: percent(100),
                        width: percent(0),
                        ..default()
                    },
                    BackgroundColor(theme::HP),
                ));
            });
        panel.spawn((
            TargetDetails,
            Text::new("Click or Tab to select"),
            theme::font(Size::Small),
            TextColor(theme::INK),
        ));
    });
}

/// Picks visible model triangles or cycles the stable ID order of nearby rendered entities.
#[allow(
    clippy::too_many_arguments,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::type_complexity
)]
pub(super) fn input(
    settings: Option<Res<super::ViewerSettings>>,
    mut attempted: Local<bool>,
    mut door_clicks: Local<DoorClicks>,
    keys: super::keys::Keys,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    // The world camera; the paperdoll's camera films the inventory figure.
    cameras: Query<(&Camera, &GlobalTransform), With<super::OrbitCamera>>,
    picker: picking::Picker,
    (doors, door_models): (
        Query<(Entity, &super::doors::DoorEntity, &Transform, &Visibility)>,
        Option<Res<super::doors::Models>>,
    ),
    ground: Query<(&super::ground::GroundEntity, &Transform)>,
    collision: Option<Res<super::Collision>>,
    nearby: Res<NearbyEntities>,
    mut online: ResMut<OnlineState>,
    mut chat: ResMut<super::chat::ChatState>,
    outbox: Res<Outbox>,
    (ui, escape, presses, mut trade): (
        super::windows::pointer::PointerUi,
        Res<super::escape::Escape>,
        Res<super::hud::hotbar::carry::Presses>,
        ResMut<super::trade::TradeState>,
    ),
) {
    let requested = chat.requested_target.take();
    // The server's answer to an assist names a spawn it knows near the one
    // assisted, which may lie past the drawn ones.
    let assisted = online
        .take_assisted()
        .filter(|id| online.world().spawn(*id).is_some() || online.world().is_player(*id));
    // The world forgets the target when the connection drops or the player dies.
    if !online.world().connected() || online.world().is_dead() {
        door_clicks.cancel();
        return;
    }
    let ids = targetable(nearby.rendered.keys().copied(), &online);
    let own_id = online.world().player().map(|player| player.spawn_id);
    // A target lasts until its spawn despawns, is replaced or turns invisible, however
    // far away it goes; drawing range only limits what can be clicked or cycled.
    let invalid = online.world().target_stale() && !online.world().target().server_selected;
    let current = online.world().target().selected;
    let mut proposal = invalid.then_some(None);
    if !*attempted
        && settings
            .as_ref()
            .is_some_and(|s| s.0.validation == Some(super::ValidationAction::TargetNearestPlayer))
        && let Some(player) = online.world().player()
    {
        let origin = Vec3::from_array(eq_client_core::render_position(player.position));
        let nearest = ids
            .iter()
            .filter_map(|id| {
                let spawn = &online.world().spawns()[id].state;
                if spawn.kind != SpawnKind::Player || *id == player.spawn_id {
                    return None;
                }
                let position = Vec3::from_array(eq_client_core::render_position(spawn.position));
                Some((*id, position.distance_squared(origin)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id);
        if let Some(id) = nearest {
            *attempted = true;
            proposal = Some(Some(id));
            info!("One-shot target: selecting nearby player spawn {id}");
        }
    }
    let cursor = windows
        .single()
        .ok()
        .filter(|window| {
            keys.escape_free()
                && !presses.taken()
                && !chat.hovered
                && window
                    .physical_cursor_position()
                    .is_none_or(|cursor| !ui.contains(cursor))
        })
        .and_then(Window::cursor_position);
    let door_cursor = cursor.filter(|_| {
        !mouse.pressed(MouseButton::Right)
            && !mouse.just_pressed(MouseButton::Right)
            && !keys.pressed(Act::Use)
    });
    let now = std::time::Instant::now();
    let door_release = door_clicks.track(online.world().session_id(), door_cursor, &mouse, now);
    if let Some(name) = requested {
        let rule = settings.as_ref().map_or(TargetByName::Nearest, |s| {
            s.0.installed_client.target_by_name()
        });
        match named(rule, &ids, &online, &name) {
            Some(id) => proposal = Some(Some(id)),
            None => chat.refuse(format!("No nearby target named {name}")),
        }
    } else if let Some(id) = assisted {
        proposal = Some(Some(id));
    } else if *escape == super::escape::Escape::Target {
        proposal = Some(None);
    } else if keys.pressed(Act::TargetSelf) {
        proposal = own_id.map(Some);
    } else if let Some(act) = keys.first([Act::TargetNext, Act::TargetPrevious]) {
        proposal = Some(cycle(&ids, current, act == Act::TargetPrevious));
    } else if let Some(cursor) = cursor
        && (door_release || mouse.any_just_pressed([MouseButton::Left, MouseButton::Right]))
        && let Ok((camera, camera_transform)) = cameras.single()
        && let Ok(ray) = camera.viewport_to_world(camera_transform, cursor)
    {
        let blocked = |distance: f32, ignore_door: Option<u8>| {
            collision
                .as_ref()
                .and_then(|collision| collision.0.as_ref())
                .and_then(|world| match ignore_door {
                    Some(door) => world.without_doors(&[door]).ray_distance(
                        ray.origin,
                        *ray.direction,
                        distance,
                    ),
                    None => world.ray_distance(ray.origin, *ray.direction, distance),
                })
                .is_some_and(|obstruction| obstruction + 0.01 < distance)
        };
        let client = settings
            .as_ref()
            .map_or_else(InstalledClient::default, |s| s.0.installed_client);
        let rule = client.spawn_pick();
        let shown = |point: Vec3| camera.world_to_viewport(camera_transform, point).ok();
        let spawns: Vec<Candidate> = ids
            .iter()
            .map(|id| {
                let pick = picker.pick(nearby.rendered[id], ray);
                Candidate {
                    id: *id,
                    hit: pick.hit.filter(|distance| !blocked(*distance, None)),
                    posed: pick.posed.and_then(|posed| {
                        let distance = posed.entry(ray).filter(|d| !blocked(*d, None))?;
                        let offset = shown(posed.middle())?.distance(cursor);
                        Some(PosedHit { distance, offset })
                    }),
                    shown: pick.posed.and_then(|posed| shown_rect(&posed, shown)),
                }
            })
            .collect();
        let item = super::ground::hit(
            ray,
            ground.iter(),
            crate::outbox::offered(online.world(), eq_client_core::Capability::WorldContainers),
        )
        .filter(|hit| !blocked(hit.1, None));
        let door = if let Some(models) = door_models.as_ref().filter(|_| door_cursor.is_some()) {
            doors
                .iter()
                .filter_map(|(entity, door, transform, visibility)| {
                    let distance = super::doors::hit(ray, door, transform, *visibility, models)
                        .filter(|d| !blocked(*d, Some(door.id)))?;
                    Some((
                        Door {
                            entity,
                            id: door.id,
                        },
                        distance,
                    ))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.id.cmp(&b.0.id)))
        } else {
            None
        };
        let choice = choose(rule, &spawns, item, door);
        let picked_door = match choice {
            Choice::Door(door) => Some(door),
            _ => None,
        };
        if door_release && let Some(id) = door_clicks.release(picked_door, cursor, now) {
            super::doors::click(id, &online, &outbox);
        }
        if mouse.just_pressed(MouseButton::Left) {
            door_clicks.press(picked_door, cursor, now);
        }
        if mouse.any_just_pressed([MouseButton::Left, MouseButton::Right]) {
            report_click(cursor, &spawns, choice);
            match choice {
                // Clicking an item on the ground picks it up and keeps the target.
                Choice::Item(drop_id) if mouse.just_pressed(MouseButton::Left) => {
                    if let Some(line) = super::ground::use_object(drop_id, &mut online, &outbox) {
                        chat.history.push(super::chat::system_line(line));
                    }
                }
                Choice::Spawn(id, _) if mouse.just_pressed(MouseButton::Right) => {
                    // Use the clicked entity without changing the current
                    // target. The trade path falls back to consideration.
                    trade.right_click(online.world(), id);
                }
                Choice::Spawn(id, _) => {
                    // An NPC clicked with an item on the cursor is asked to
                    // take it, as well as targeted.
                    if let Err(super::give::OfferFailure::Refused(refusal)) =
                        super::give::offer(id, &mut online, &outbox)
                    {
                        outbox.refused(refusal);
                    }
                    proposal = Some(Some(id));
                }
                Choice::Nothing if mouse.just_pressed(MouseButton::Left) => {
                    match client.click_miss() {
                        // Nothing is sent; a stale target is still dropped.
                        ClickMiss::KeepTarget => {}
                        ClickMiss::ClearTarget => proposal = Some(None),
                    }
                }
                Choice::Door(_) | Choice::Item(_) | Choice::Nothing => (),
            }
        }
    } else if door_release || mouse.just_pressed(MouseButton::Left) {
        door_clicks.cancel();
    }
    let Some(selected) = proposal else {
        return;
    };
    select(&mut online, &outbox, selected);
}

/// Shared submission for scene, keyboard and map choices. A failed send never
/// changes the local target; choosing the current target sends nothing again.
pub(crate) fn select(online: &mut OnlineState, outbox: &Outbox, selected: Option<u16>) {
    if selected == online.world().target().selected && !online.world().target_stale() {
        return;
    }
    let request_id = online.world().next_target_request();
    // Offline, the choice is the viewer's alone; online, the outbox says
    // why a choice did not leave.
    if online.enabled
        && outbox
            .post(online.world(), |stamp| ClientCommand::SelectTarget {
                session_id: stamp.session_id,
                request_id,
                spawn_id: selected,
            })
            .is_err()
    {
        return;
    }
    // A choice sent says nothing, as in the official client.
    online.select_target(selected);
}

/// What a click's ray meets of one targetable spawn, line of sight
/// already checked.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Candidate {
    id: u16,
    /// World distance to its nearest drawn triangle hit, if seen.
    hit: Option<f32>,
    /// Its posed box, if the ray enters it where it is seen.
    posed: Option<PosedHit>,
    /// The screen rectangle its posed box covers, for the debug line only.
    shown: Option<Rect>,
}

/// A ray entering a spawn's posed box.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PosedHit {
    /// World distance to where the ray enters the box.
    distance: f32,
    /// Screen distance from the cursor to the box's middle, in logical pixels.
    offset: f32,
}

/// How a spawn was found by a click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Found {
    Triangles,
    PosedBox,
}

/// What a click in the 3D view picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    Spawn(u16, Found),
    /// An object on the ground, by its drop id.
    Item(u32),
    Door(Door),
    Nothing,
}

/// The click's pick by the installed generation's rule: the nearest drawn
/// triangle, a ground item or door nearer than it; then, by
/// [`SpawnPick::TrianglesThenPosedBox`], the posed box whose middle shows
/// nearest the cursor, a tie going to the nearer. A ground item the ray
/// hits beats a spawn found only by its box.
fn choose(
    rule: SpawnPick,
    spawns: &[Candidate],
    item: Option<(u32, f32)>,
    door: Option<(Door, f32)>,
) -> Choice {
    let exact = spawns
        .iter()
        .filter_map(|spawn| Some((spawn.id, spawn.hit?)))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((door, distance)) = door
        && exact.is_none_or(|(_, spawn)| distance < spawn)
        && item.is_none_or(|(_, item)| distance < item)
    {
        return Choice::Door(door);
    }
    match (exact, item) {
        (exact, Some((drop_id, distance))) if exact.is_none_or(|(_, nearer)| distance < nearer) => {
            return Choice::Item(drop_id);
        }
        (Some((id, _)), _) => return Choice::Spawn(id, Found::Triangles),
        (None, _) => {}
    }
    if rule != SpawnPick::TrianglesThenPosedBox {
        return Choice::Nothing;
    }
    spawns
        .iter()
        .filter_map(|spawn| Some((spawn.id, spawn.posed?)))
        .min_by(|(_, a), (_, b)| {
            a.offset
                .total_cmp(&b.offset)
                .then(a.distance.total_cmp(&b.distance))
        })
        .map_or(Choice::Nothing, |(id, _)| {
            Choice::Spawn(id, Found::PosedBox)
        })
}

/// The screen rectangle a posed box covers; None when a corner lies off the
/// view's front.
fn shown_rect(posed: &PosedBox, shown: impl Fn(Vec3) -> Option<Vec2>) -> Option<Rect> {
    posed
        .corners()
        .into_iter()
        .map(shown)
        .try_fold(None::<Rect>, |rect, point| {
            let point = point?;
            Some(Some(
                rect.map_or(Rect::from_corners(point, point), |rect| {
                    rect.union_point(point)
                }),
            ))
        })
        .flatten()
}

/// Logical pixels from a point to a screen rectangle, 0 inside it.
fn outside(rect: Rect, point: Vec2) -> f32 {
    (point.clamp(rect.min, rect.max) - point).length()
}

/// A debug line for each click in the 3D view, to measure misses: what it
/// picked and how, and, for the spawn whose shown box lies nearest the
/// cursor, how far outside that box the click landed and the box's size on
/// screen. Spawn ids and screen pixels only. Off unless the log filter asks
/// for the entities module at debug level.
fn report_click(cursor: Vec2, spawns: &[Candidate], choice: Choice) {
    let nearest = spawns
        .iter()
        .filter_map(|spawn| {
            let rect = spawn.shown?;
            Some((spawn.id, outside(rect, cursor), rect.size()))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let triangles = spawns.iter().filter(|spawn| spawn.hit.is_some()).count();
    let boxes = spawns.iter().filter(|spawn| spawn.posed.is_some()).count();
    if let Some((id, off, size)) = nearest {
        debug!(
            target: "eq_client_render::entities",
            "click at ({:.0}, {:.0}) picked {choice:?}; triangles hit {triangles}, boxes entered \
             {boxes}; nearest shown box spawn {id}, {off:.1} px outside, {:.0}x{:.0} px",
            cursor.x, cursor.y, size.x, size.y,
        );
    } else {
        debug!(
            target: "eq_client_render::entities",
            "click at ({:.0}, {:.0}) picked {choice:?}; triangles hit {triangles}, boxes entered \
             {boxes}; no box shown",
            cursor.x, cursor.y,
        );
    }
}

/// Drawn spawns the player may target: visible ones within the zone's far clip.
/// The official client targets nothing past its clip plane, and servers log a
/// target beyond it as a possible cheat; drawn spawns may linger a little past it.
fn targetable(rendered: impl Iterator<Item = u16>, online: &OnlineState) -> Vec<u16> {
    rendered
        .filter(|id| unavailable(online, *id).is_none())
        .collect()
}

/// Why a known entity cannot be newly selected from a local interface. Map
/// visibility does not grant targeting through invisibility or past the clip.
pub(crate) fn unavailable(online: &OnlineState, id: u16) -> Option<&'static str> {
    let world = online.world();
    if world.is_dead() {
        return Some(crate::outbox::Refusal::Dead.text());
    }
    if !world.in_world() {
        return Some(crate::outbox::Refusal::Away.text());
    }
    if online.enabled && !world.can(eq_client_core::Capability::Targeting) {
        return Some(crate::outbox::UNAVAILABLE);
    }
    if world.is_player(id) {
        return None;
    }
    let Some(spawn) = world.spawn(id) else {
        return Some("This entity is no longer present");
    };
    if spawn.state.invisible {
        return Some("This entity is invisible");
    }
    if matches!(spawn.state.kind, SpawnKind::Unknown(_)) {
        return Some("This entity type cannot be targeted");
    }
    if world
        .far_clip()
        .zip(world.player())
        .is_some_and(|(clip, player)| {
            !eq_client_core::entities::within(player.position, spawn.state.position, clip)
        })
    {
        return Some("Beyond the zone's targeting range");
    }
    None
}

/// The spawn `/target <query>` picks, by the installed generation's rule;
/// underscores in the query already read as spaces.
fn named(rule: TargetByName, ids: &[u16], online: &OnlineState, query: &str) -> Option<u16> {
    match rule {
        TargetByName::Nearest => nearest_named(ids, online, query),
        TargetByName::Shortest => shortest_named(online, query),
    }
}

/// Titanium: the nearest drawn visible spawn whose shown name, or full
/// server name such as `a_whiskered_bat002` (as `/target` also accepts),
/// starts with `query`, ignoring case.
fn nearest_named(ids: &[u16], online: &OnlineState, query: &str) -> Option<u16> {
    let query = query.to_lowercase();
    let origin = online
        .world()
        .player()
        .map(|player| Vec3::from_array(eq_client_core::render_position(player.position)));
    ids.iter()
        .filter_map(|id| {
            let spawn = &online.world().spawn(*id)?.state;
            let shown = eq_client_core::entities::display_name(&spawn.name).to_lowercase();
            let full = spawn.name.replace('_', " ").to_lowercase();
            (shown.starts_with(&query) || full.starts_with(&query)).then(|| {
                let position = Vec3::from_array(eq_client_core::render_position(spawn.position));
                (
                    *id,
                    origin.map_or(0.0, |origin| position.distance_squared(origin)),
                )
            })
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// A name as the TAKP program compares it: digits and '#' dropped, '_' read
/// as a space, and case ignored (TAKP read T6).
fn cleaned(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_ascii_digit() && *c != '#')
        .map(|c| {
            if c == '_' {
                ' '
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}

/// `EQMac` (TAKP read T6): any known visible spawn at any range whose cleaned
/// name starts with the cleaned text; the shortest name wins, and a tie goes
/// to the lowest spawn ID (inferred, checklist 6.313).
fn shortest_named(online: &OnlineState, query: &str) -> Option<u16> {
    let query = cleaned(query);
    online
        .world()
        .spawns()
        .iter()
        .filter(|(_, spawn)| !spawn.state.invisible)
        .filter_map(|(id, spawn)| {
            let name = cleaned(&spawn.state.name);
            name.starts_with(&query).then_some((*id, name.len()))
        })
        // `min_by_key` keeps the first of equals, and the map runs by ID.
        .min_by_key(|(_, length)| *length)
        .map(|(id, _)| id)
}

/// Shows server-supplied target identity and HP; unknown HP is never shown as full.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn update(
    online: Res<OnlineState>,
    combat: Option<Res<super::combat::CombatState>>,
    mut texts: Query<(&mut Text, Option<&TargetName>, Option<&TargetDetails>)>,
    mut bars: Query<&mut Node, With<TargetHp>>,
) {
    let chosen = online.world().target();
    let spawn = chosen
        .selected
        .filter(|_| !online.world().target_stale())
        .and_then(|id| online.world().spawn(id).map(|spawn| &spawn.state));
    let own = online
        .world()
        .player()
        .filter(|player| chosen.selected == Some(player.spawn_id));
    let hp = chosen
        .selected
        .filter(|_| !online.world().target_stale())
        .and_then(|id| online.world().health(id))
        .or_else(|| own.and_then(|player| player.hp_percent));
    let health = format!("HP {}", hp.map_or_else(|| "--".into(), |v| format!("{v}%")));
    for (mut text, name, details) in &mut texts {
        if name.is_some() {
            text.0 = spawn.map_or_else(
                || {
                    if own.is_some() {
                        "You".into()
                    } else if chosen.selected.is_some() {
                        "Target unavailable".into()
                    } else {
                        "No target".into()
                    }
                },
                |s| eq_client_core::entities::display_name(&s.name),
            );
        }
        if details.is_some() {
            text.0 = spawn.map_or_else(
                || own.map_or_else(String::new, |_| joined(&["You", &health])),
                |s| {
                    let kind = match s.kind {
                        SpawnKind::Player => "Player",
                        SpawnKind::Npc => "NPC",
                        _ => "Corpse",
                    };
                    let attacking = combat.as_ref().is_some_and(|combat| combat.auto_attack);
                    let doing = if attacking { "Attacking" } else { "" };
                    let pending = if online.enabled && !chosen.sent {
                        "Target pending"
                    } else {
                        ""
                    };
                    joined(&[kind, &health, doing, pending])
                },
            );
        }
    }
    for mut bar in &mut bars {
        bar.width = percent(f32::from(hp.unwrap_or(0)));
    }
}

/// Writes the target keys into the target window's tooltip from the key map,
/// so the help follows the bindings; the window itself shows only facts.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn key_help(
    map: Res<crate::keys::KeyMap>,
    mut commands: Commands,
    panels: Query<(Entity, Option<&crate::tooltip::Tooltip>), With<TargetPanel>>,
) {
    let help = map.help(&[
        (Act::TargetNext, "next target"),
        (Act::TargetPrevious, "previous"),
        (Act::TargetSelf, "yourself"),
        (Act::Consider, "consider"),
        (Act::Attack, "attack"),
        (Act::Hail, "hail"),
        (Act::Trade, "trade"),
        (Act::Loot, "loot"),
    ]);
    let help = format!("Click: target | {help} | Esc: clear");
    for (panel, tooltip) in &panels {
        if tooltip.is_none_or(|tooltip| tooltip.0 != help) {
            commands
                .entity(panel)
                .insert(crate::tooltip::Tooltip(help.clone()));
        }
    }
}

/// The target window's facts on one line, leaving out the empty ones.
fn joined(parts: &[&str]) -> String {
    parts
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("   ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::{SpawnState, WorldPosition};

    #[test]
    fn named_targets_accept_the_shown_or_full_server_name() {
        let mut online = OnlineState::new(false);
        for (id, name) in [(2, "a_whiskered_bat002"), (3, "a_whiskered_bat005")] {
            crate::online::testing::spawn_entry(
                &mut online,
                id,
                SpawnState {
                    class: None,
                    spawn_id: id,
                    name: name.into(),
                    kind: SpawnKind::Npc,
                    race: 1,
                    gender: 0,
                    position: WorldPosition::default(),
                    velocity: [0.0; 3],
                    forward: 0.0,
                    walk_speed: 0.0,
                    run_speed: 0.0,
                    size: 0.0,
                    invisible: false,
                    appearance: eq_client_core::outfit::Appearance::default(),
                    level: 0,
                    listing: eq_client_core::listing::Listing::default(),
                    name_parts: eq_client_core::names::NameParts::default(),
                    pet_owner: None,
                    hp_percent: None,
                },
            );
        }
        assert!(named(TargetByName::Nearest, &[2, 3], &online, "a whiskered bat").is_some());
        // `/target a_whiskered_bat005` arrives with its underscores read as spaces.
        assert_eq!(
            named(
                TargetByName::Nearest,
                &[2, 3],
                &online,
                "a whiskered bat005"
            ),
            Some(3)
        );
        assert_eq!(
            named(
                TargetByName::Nearest,
                &[2, 3],
                &online,
                "a whiskered bat009"
            ),
            None
        );
    }

    #[test]
    fn takp_targets_the_shortest_cleaned_name_at_any_range() {
        let mut online = OnlineState::new(false);
        for (id, name, invisible) in [
            (2, "a_bat_wing001", false),
            (3, "a_bat002", false),
            (4, "a_bat003", false),
            (5, "a_ba#004", true),
        ] {
            crate::online::testing::spawn_entry(
                &mut online,
                id,
                SpawnState {
                    class: None,
                    spawn_id: id,
                    name: name.into(),
                    kind: SpawnKind::Npc,
                    race: 1,
                    gender: 0,
                    position: WorldPosition {
                        x: f32::from(id) * 1000.0,
                        ..WorldPosition::default()
                    },
                    velocity: [0.0; 3],
                    forward: 0.0,
                    walk_speed: 0.0,
                    run_speed: 0.0,
                    size: 0.0,
                    invisible,
                    appearance: eq_client_core::outfit::Appearance::default(),
                    level: 0,
                    listing: eq_client_core::listing::Listing::default(),
                    name_parts: eq_client_core::names::NameParts::default(),
                    pet_owner: None,
                    hp_percent: None,
                },
            );
        }
        // None of them is drawn; the rule looks at every known spawn.
        let pick = |query| named(TargetByName::Shortest, &[], &online, query);
        // Two names tie at the shortest; the lower spawn ID wins, and the
        // invisible, shorter one is passed over.
        assert_eq!(pick("A BA"), Some(3));
        assert_eq!(pick("a bat wing"), Some(2));
        // Digits in the text are dropped as they are from the names.
        assert_eq!(pick("a bat003"), Some(3));
        assert_eq!(pick("a cat"), None);
        // Titanium's rule only looks at drawn spawns.
        assert_eq!(named(TargetByName::Nearest, &[], &online, "a bat"), None);
    }

    #[test]
    fn spawns_past_the_zones_far_clip_cannot_be_targeted() {
        let zone = |far_clip| {
            let mut online = OnlineState::new(true);
            crate::online::testing::enter(
                &mut online,
                1,
                crate::online::testing::player(1),
                far_clip,
            );
            for (id, x) in [(2, 90.0), (3, 110.0)] {
                crate::online::testing::spawn_entry(
                    &mut online,
                    id,
                    SpawnState {
                        class: None,
                        spawn_id: id,
                        name: "a_bat".into(),
                        kind: SpawnKind::Npc,
                        race: 1,
                        gender: 0,
                        position: WorldPosition {
                            x,
                            ..WorldPosition::default()
                        },
                        velocity: [0.0; 3],
                        forward: 0.0,
                        walk_speed: 0.0,
                        run_speed: 0.0,
                        size: 0.0,
                        invisible: false,
                        appearance: eq_client_core::outfit::Appearance::default(),
                        level: 0,
                        listing: eq_client_core::listing::Listing::default(),
                        name_parts: eq_client_core::names::NameParts::default(),
                        pet_owner: None,
                        hp_percent: None,
                    },
                );
            }
            online
        };
        assert_eq!(targetable([2, 3].into_iter(), &zone(None)), [2, 3]);
        assert_eq!(targetable([2, 3].into_iter(), &zone(Some(100.0))), [2]);
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "Keep the ordered integration scenario and its assertions together"
    )]
    fn keyboard_cycles_only_rendered_entities_and_keeps_targets_until_they_are_gone() {
        let mut app = crate::testing::app();
        app.add_systems(Update, (super::super::escape::route, input).chain());
        // The program's key table binds only Target Next; cycling backwards
        // is an explicit user assignment.
        crate::keys::testing::bind(
            &mut app,
            crate::keys::Act::TargetPrevious,
            crate::keys::Chord::shift(KeyCode::Tab),
        );
        {
            let mut online = app.world_mut().resource_mut::<OnlineState>();
            crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
            for id in [2, 3, 4] {
                crate::online::testing::spawn_entry(
                    &mut online,
                    id,
                    SpawnState {
                        class: None,
                        spawn_id: id,
                        name: format!("Synthetic {id}"),
                        kind: SpawnKind::Player,
                        race: 1,
                        gender: 0,
                        position: WorldPosition {
                            x: 0.0,
                            y: 0.0,
                            z: 0.0,
                            heading: 0.0,
                        },
                        velocity: [0.0; 3],
                        forward: 0.0,
                        walk_speed: 0.0,
                        run_speed: 0.0,
                        size: 0.0,
                        invisible: false,
                        appearance: eq_client_core::outfit::Appearance::default(),
                        level: 0,
                        listing: eq_client_core::listing::Listing::default(),
                        name_parts: eq_client_core::names::NameParts::default(),
                        pet_owner: None,
                        hp_percent: None,
                    },
                );
            }
        }
        for id in [2, 3] {
            let entity = app.world_mut().spawn(Transform::default()).id();
            app.world_mut()
                .resource_mut::<NearbyEntities>()
                .rendered
                .insert(id, entity);
        }
        let press = |app: &mut App, key, shift| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            if shift {
                keys.press(KeyCode::ShiftLeft);
            }
            keys.press(key);
            app.update();
            app.world()
                .resource::<OnlineState>()
                .world()
                .target()
                .selected
        };
        assert_eq!(press(&mut app, KeyCode::Tab, false), Some(2));
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        assert_eq!(press(&mut app, KeyCode::Tab, false), Some(2));
        assert_eq!(press(&mut app, KeyCode::Escape, false), Some(2));
        {
            let mut typing = app.world_mut().resource_mut::<crate::keys::Typing>();
            typing.composing = false;
            typing.escape_consumed = true;
        }
        assert_eq!(press(&mut app, KeyCode::Escape, false), Some(2));
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .escape_consumed = false;
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        assert_eq!(press(&mut app, KeyCode::Tab, false), Some(2));
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        assert_eq!(press(&mut app, KeyCode::Tab, false), Some(3));
        assert_eq!(press(&mut app, KeyCode::Tab, false), Some(2));
        assert_eq!(press(&mut app, KeyCode::Tab, true), Some(3));
        assert_eq!(press(&mut app, KeyCode::Escape, false), None);
        assert_eq!(press(&mut app, KeyCode::Tab, false), Some(2));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        let again = app
            .world()
            .resource::<OnlineState>()
            .world()
            .spawn(2)
            .unwrap()
            .state
            .clone();
        crate::online::testing::spawns(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            vec![again],
        );
        app.update();
        assert_eq!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .target()
                .selected,
            None
        );
        assert_eq!(press(&mut app, KeyCode::Tab, false), Some(2));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        // A visibility change clears a selected target and removes it from Tab cycling.
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::Visibility {
                spawn_id: 2,
                invisible: true,
            }],
        );
        app.update();
        assert_eq!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .target()
                .selected,
            None
        );
        assert_eq!(press(&mut app, KeyCode::Tab, false), Some(3));
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::Visibility {
                spawn_id: 2,
                invisible: false,
            }],
        );
        assert_eq!(press(&mut app, KeyCode::Tab, false), Some(2));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        // Leaving the drawing range keeps the target, as in EQ; despawning clears it.
        app.world_mut()
            .resource_mut::<NearbyEntities>()
            .rendered
            .remove(&2);
        app.update();
        assert_eq!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .target()
                .selected,
            Some(2)
        );
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::Despawn(2)],
        );
        app.update();
        assert_eq!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .target()
                .selected,
            None
        );
        crate::online::testing::admit(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            2,
            crate::online::testing::player(1),
        );
        app.update();
        assert_eq!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .target()
                .selected,
            None
        );
    }

    fn target_app() -> (
        App,
        std::sync::mpsc::Receiver<ClientCommand>,
        Entity,
        Entity,
    ) {
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        let mut online = OnlineState::new(true);
        crate::online::testing::admit(
            &mut online,
            1,
            eq_client_core::PlayerState {
                name: "Example".into(),
                base_attributes: None,
                spawn_id: 7,
                race: 1,
                gender: 0,
                class: Some(2),
                deity: None,
                level: 1,
                position: WorldPosition::default(),
                mana: 0,
                endurance: None,
                skills: None,
                practice_points: None,
                languages: None,
                disciplines: None,
                spell_refresh_ms: None,
                memorized_spells: [None; 8],
                size: 6.0,
                walk_speed: 0.0,
                run_speed: 0.0,
                hp_percent: Some(55),
                appearance: eq_client_core::outfit::Appearance::default(),
                listing: eq_client_core::listing::Listing::default(),
                name_parts: eq_client_core::names::NameParts::default(),
            },
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(2);
        app.insert_resource(online)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<NearbyEntities>()
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<super::super::trade::TradeState>()
            .init_resource::<super::super::escape::Escape>()
            .init_resource::<super::super::hud::hotbar::carry::Presses>()
            .insert_resource(crate::outbox::Outbox::new(Some(tx)))
            .add_systems(Update, (input, update).chain());
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        let label = app.world_mut().spawn((TargetName, Text::default())).id();
        let bar = app.world_mut().spawn((TargetHp, Node::default())).id();
        (app, rx, label, bar)
    }

    fn scene_camera(app: &mut App) {
        let mut camera = Camera::default();
        camera.computed.clip_from_view =
            Mat4::perspective_infinite_reverse_rh(std::f32::consts::FRAC_PI_4, 4.0 / 3.0, 0.1);
        camera.computed.target_info = Some(bevy::camera::RenderTargetInfo {
            physical_size: UVec2::new(800, 600),
            scale_factor: 1.0,
        });
        app.world_mut().spawn((
            camera,
            GlobalTransform::from_translation(Vec3::new(0.0, 0.0, 10.0)),
            crate::OrbitCamera {
                focus: Vec3::ZERO,
                radius: 10.0,
                yaw: 0.0,
                pitch: 0.0,
                pulled: None,
                view: default(),
            },
        ));
    }

    #[test]
    fn a_native_spawn_click_queues_one_offer_and_keeps_the_outbox_feedback() {
        use crate::online::testing as session;
        use eq_client_core::inventory::{InventorySlot, InventoryUpdate};

        const NPC: u16 = 42;
        let (mut app, commands, ..) = target_app();
        scene_camera(&mut app);
        let mut meshes = Assets::<Mesh>::default();
        let body = app
            .world_mut()
            .spawn((
                Transform::default(),
                Mesh3d(meshes.add(Cuboid::new(2.0, 2.0, 2.0))),
            ))
            .id();
        app.insert_resource(meshes);
        app.world_mut()
            .resource_mut::<NearbyEntities>()
            .rendered
            .insert(NPC, body);
        {
            let mut online = app.world_mut().resource_mut::<OnlineState>();
            session::spawns(&mut online, vec![session::pet(NPC, 99)]);
            online.select_target(Some(NPC));
            let mut item = crate::preview::items().into_iter().next().unwrap();
            item.slot = InventorySlot::CURSOR;
            session::inventory(&mut online, InventoryUpdate::Snapshot(vec![item]));
        }
        let mut windows = app
            .world_mut()
            .query_filtered::<&mut Window, With<PrimaryWindow>>();
        windows
            .single_mut(app.world_mut())
            .unwrap()
            .set_cursor_position(Some(Vec2::new(400.0, 300.0)));
        let click = |app: &mut App| {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.reset(MouseButton::Left);
            mouse.press(MouseButton::Left);
            app.update();
        };

        // The existing target is unchanged, so only the offer enters the queue.
        click(&mut app);
        assert!(matches!(
            commands.try_recv().unwrap(),
            ClientCommand::OfferTrade {
                session_id: 1,
                with_id: NPC,
                ..
            }
        ));
        // Clicking again while the offer is pending is a silent no-op.
        click(&mut app);
        assert!(commands.try_recv().is_err());
        assert_eq!(
            app.world().resource::<Outbox>().take_refused(),
            Vec::<crate::outbox::Refusal>::new()
        );

        app.world_mut().resource_mut::<OnlineState>().close_trade();
        app.world_mut().resource_mut::<Outbox>().connect(None);
        click(&mut app);
        assert_eq!(
            app.world().resource::<Outbox>().take_refused(),
            [crate::outbox::Refusal::Offline]
        );
        assert!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .exchange()
                .is_none()
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)] // Exercises the scene system and its real picking inputs.
    fn a_door_uses_two_completed_scene_clicks_and_ignores_drags_and_ui_clicks() {
        let (mut app, commands, ..) = target_app();
        let center = Vec2::new(400.0, 300.0);
        scene_camera(&mut app);
        app.init_resource::<Time>()
            .init_resource::<super::super::doors::Hinges>();
        let source = eq_client_assets::ZoneModel {
            name: "TEST".into(),
            primitives: vec![eq_client_assets::ZonePrimitive {
                positions: vec![[-2.0, -2.0, 0.0], [2.0, -2.0, 0.0], [0.0, 2.0, 0.0]],
                indices: vec![0, 1, 2],
                normals: Vec::new(),
                texture_coordinates: Vec::new(),
                texture: None,
                material_mode: eq_client_assets::MaterialMode::Opaque,
            }],
            materials: vec![eq_client_assets::ObjectMaterial { double_sided: true }],
            collision: Vec::new(),
        };
        let mut models = super::super::doors::Models::default();
        models.0.insert(
            "TEST".into(),
            super::super::doors::Model {
                // Simulate render-world extraction. No CPU mesh remains.
                primitives: vec![(Handle::default(), Handle::default())],
                collision: Vec::new(),
                pick: super::super::doors::PickMesh::from_model(&source),
            },
        );
        app.insert_resource(models)
            .add_systems(Update, super::super::doors::reconcile.before(input));
        let mut bytes = [0u8; 80];
        bytes[..4].copy_from_slice(b"TEST");
        bytes[52] = 100;
        crate::online::testing::doors(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            &eq_client_core::doors::decode(0x4c24, &bytes)
                .unwrap()
                .unwrap(),
            std::time::Instant::now(),
        );
        app.update();

        let frame = |app: &mut App, cursor, pressed| {
            let mut windows = app
                .world_mut()
                .query_filtered::<&mut Window, With<PrimaryWindow>>();
            windows
                .single_mut(app.world_mut())
                .unwrap()
                .set_cursor_position(Some(cursor));
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.clear();
            if pressed {
                mouse.press(MouseButton::Left);
            } else {
                mouse.release(MouseButton::Left);
            }
            app.update();
        };
        let click = |app: &mut App| {
            frame(app, center, true);
            frame(app, center, false);
        };
        click(&mut app);
        assert!(commands.try_recv().is_err());

        // The second press drags away and comes back. Neither its release nor
        // the following single click can complete the initial pair.
        frame(&mut app, center, true);
        frame(&mut app, center + Vec2::X * 12.0, true);
        frame(&mut app, center, false);
        click(&mut app);
        assert!(commands.try_recv().is_err());

        app.world_mut()
            .resource_mut::<super::super::chat::ChatState>()
            .hovered = true;
        click(&mut app);
        assert!(commands.try_recv().is_err());
        app.world_mut()
            .resource_mut::<super::super::chat::ChatState>()
            .hovered = false;

        click(&mut app);
        frame(&mut app, center, true);
        assert!(commands.try_recv().is_err());
        frame(&mut app, center, false);
        assert!(matches!(
            commands.try_recv().unwrap(),
            ClientCommand::ClickDoor {
                session_id: 1,
                door_id: 0,
                ..
            }
        ));
        click(&mut app);
        assert!(commands.try_recv().is_err());
        assert_eq!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .doors()
                .entries()[&0]
                .action,
            None
        );

        let wall = eq_client_core::movement::CollisionWorld::new([[
            [-2.0, -2.0, 5.0],
            [2.0, -2.0, 5.0],
            [0.0, 2.0, 5.0],
        ]])
        .unwrap();
        app.insert_resource(super::super::Collision(Some(wall)));
        click(&mut app);
        click(&mut app);
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn self_target_uses_profile_without_a_nearby_entity_and_survives_culling() {
        let (mut app, rx, label, bar) = target_app();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F1);
        app.update();
        assert!(matches!(
            rx.try_recv().unwrap(),
            ClientCommand::SelectTarget {
                request_id: 1,
                session_id: 1,
                spawn_id: Some(7)
            }
        ));
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "You");
        assert_eq!(app.world().get::<Node>(bar).unwrap().width, percent(55));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.update();
        assert_eq!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .target()
                .selected,
            Some(7)
        );
        assert!(rx.try_recv().is_err());
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::HealthPercent {
                spawn_id: 7,
                percent: 31,
            }],
        );
        app.update();
        assert_eq!(app.world().get::<Node>(bar).unwrap().width, percent(31));
    }

    #[test]
    fn unresolved_server_targets_and_clears_never_echo_or_borrow_health() {
        let (mut app, rx, label, bar) = target_app();
        // An unresolved server selection must stay explicit, with no
        // automatic clear packet from input and no borrowed entity HP.
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::TargetChanged {
                session_id: 1,
                request_id: 0,
                state: eq_client_core::targets::TargetState {
                    revision: 2,
                    spawn_id: Some(99),
                    available: false,
                },
            }],
        );
        app.update();
        assert_eq!(
            app.world().get::<Text>(label).unwrap().0,
            "Target unavailable"
        );
        assert_eq!(app.world().get::<Node>(bar).unwrap().width, percent(0));
        assert_eq!(
            app.world()
                .resource::<OnlineState>()
                .world()
                .target()
                .selected,
            Some(99)
        );
        assert!(rx.try_recv().is_err());
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::TargetChanged {
                session_id: 1,
                request_id: 0,
                state: eq_client_core::targets::TargetState {
                    revision: 3,
                    spawn_id: None,
                    available: true,
                },
            }],
        );
        app.update();
        assert_eq!(app.world().get::<Text>(label).unwrap().0, "No target");
        assert!(rx.try_recv().is_err());
    }

    fn candidate(id: u16, hit: Option<f32>, posed: Option<(f32, f32)>) -> Candidate {
        Candidate {
            id,
            hit,
            posed: posed.map(|(distance, offset)| PosedHit { distance, offset }),
            shown: None,
        }
    }

    #[test]
    fn a_drawn_triangle_beats_any_posed_box() {
        let rule = SpawnPick::TrianglesThenPosedBox;
        // The nearer triangle hit wins, whatever the boxes say.
        let spawns = [
            candidate(2, Some(30.0), Some((25.0, 40.0))),
            candidate(3, Some(20.0), Some((18.0, 9.0))),
            candidate(4, None, Some((5.0, 0.0))),
        ];
        assert_eq!(
            choose(rule, &spawns, None, None),
            Choice::Spawn(3, Found::Triangles)
        );
    }

    #[test]
    fn with_no_triangle_hit_the_box_shown_nearest_the_cursor_wins() {
        let rule = SpawnPick::TrianglesThenPosedBox;
        // A big model's box swallowing a small one in front of it: the
        // small one's middle is nearer the cursor, though its box is farther.
        let spawns = [
            candidate(2, None, Some((10.0, 60.0))),
            candidate(3, None, Some((40.0, 4.0))),
            candidate(4, None, None),
        ];
        assert_eq!(
            choose(rule, &spawns, None, None),
            Choice::Spawn(3, Found::PosedBox)
        );
        // A tie on screen goes to the nearer.
        let tied = [
            candidate(2, None, Some((40.0, 4.0))),
            candidate(3, None, Some((10.0, 4.0))),
        ];
        assert_eq!(
            choose(rule, &tied, None, None),
            Choice::Spawn(3, Found::PosedBox)
        );
        // Nothing entered, nothing picked.
        assert_eq!(choose(rule, &spawns[2..], None, None), Choice::Nothing);
        assert_eq!(choose(rule, &[], None, None), Choice::Nothing);
    }

    #[test]
    fn ground_items_rank_with_the_triangles_and_beat_a_box() {
        let rule = SpawnPick::TrianglesThenPosedBox;
        let hit = [candidate(2, Some(20.0), None)];
        assert_eq!(choose(rule, &hit, Some((7, 15.0)), None), Choice::Item(7));
        assert_eq!(
            choose(rule, &hit, Some((7, 25.0)), None),
            Choice::Spawn(2, Found::Triangles)
        );
        let boxed = [candidate(2, None, Some((5.0, 0.0)))];
        assert_eq!(choose(rule, &boxed, Some((7, 25.0)), None), Choice::Item(7));
    }

    #[test]
    fn visible_door_competes_with_exact_hits_and_precedes_spawn_box_fallback() {
        let rule = SpawnPick::TrianglesThenPosedBox;
        let door = Door {
            entity: Entity::PLACEHOLDER,
            id: 7,
        };
        let exact = [candidate(1, Some(20.0), None)];
        let fallback = [candidate(1, None, Some((5.0, 0.0)))];
        assert_eq!(
            choose(rule, &exact, None, Some((door, 19.0))),
            Choice::Door(door)
        );
        assert_eq!(
            choose(rule, &exact, None, Some((door, 20.0))),
            Choice::Spawn(1, Found::Triangles)
        );
        assert_eq!(
            choose(rule, &fallback, None, Some((door, 19.0))),
            Choice::Door(door)
        );
        assert_eq!(
            choose(rule, &fallback, Some((8, 18.0)), Some((door, 19.0))),
            Choice::Item(8)
        );
    }

    #[test]
    fn the_triangles_rule_ignores_boxes() {
        let spawns = [candidate(2, None, Some((5.0, 0.0)))];
        assert_eq!(
            choose(SpawnPick::Triangles, &spawns, None, None),
            Choice::Nothing
        );
        let hit = [candidate(2, Some(5.0), None)];
        assert_eq!(
            choose(SpawnPick::Triangles, &hit, None, None),
            Choice::Spawn(2, Found::Triangles)
        );
    }

    #[test]
    fn both_generations_pick_by_triangles_then_the_posed_box_until_read() {
        for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
            assert_eq!(client.spawn_pick(), SpawnPick::TrianglesThenPosedBox);
        }
    }

    #[test]
    fn a_click_on_nothing_keeps_the_target_on_both_generations() {
        for client in [InstalledClient::Titanium, InstalledClient::EqMac] {
            assert_eq!(client.click_miss(), ClickMiss::KeepTarget);
        }
        // A click that enters no box and hits no triangle picks nothing.
        let away = [candidate(2, None, None)];
        assert_eq!(
            choose(SpawnPick::TrianglesThenPosedBox, &away, None, None),
            Choice::Nothing
        );
    }

    #[test]
    fn a_shown_box_is_measured_in_screen_pixels() {
        let posed = PosedBox {
            frame: Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)),
            min: Vec3::new(-1.0, 0.0, -1.0),
            max: Vec3::new(1.0, 4.0, 1.0),
        };
        // A flat projection dropping depth, ten pixels to a unit.
        let rect = shown_rect(&posed, |p| Some(Vec2::new(p.x, p.y) * 10.0)).unwrap();
        assert_eq!(rect, Rect::new(90.0, 0.0, 110.0, 40.0));
        assert!(outside(rect, Vec2::new(100.0, 20.0)).abs() < 1e-4);
        assert!((outside(rect, Vec2::new(113.0, 44.0)) - 5.0).abs() < 1e-4);
        // A corner off the view's front shows no box.
        assert!(shown_rect(&posed, |p| (p.z > 0.0).then_some(Vec2::ZERO)).is_none());
    }
}
