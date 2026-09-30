//! Target selection never moves, attacks, casts, or sends chat.
pub(super) mod marker;
mod picking;
use super::{entities::NearbyEntities, online::OnlineState};
use bevy::{prelude::*, window::PrimaryWindow};
use eq_client_core::{ClientCommand, SpawnKind, targeting::cycle};
use std::sync::mpsc::SyncSender;

#[derive(Resource, Default)]
pub(super) struct TargetState {
    pub selected: Option<u16>,
    pub sent: bool,
    session: Option<u64>,
    revision: Option<u64>,
    pub status: String,
}

impl TargetState {
    /// Clears only the rejected selection, preserving the current admission.
    pub(super) fn reject(&mut self, requested: Option<u16>, reason: &str) {
        if self.selected == requested {
            self.selected = None;
            self.revision = None;
            self.sent = false;
            self.status = format!("Target rejected: {reason}");
        }
    }
}
#[derive(Resource)]
pub(super) struct CommandsToServer(pub Option<SyncSender<ClientCommand>>);
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
            GlobalZIndex(15),
            Node {
                position_type: PositionType::Absolute,
                top: px(16),
                left: percent(50),
                margin: UiRect::left(px(-140)),
                width: px(280),
                padding: UiRect::all(px(10)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                ..default()
            },
            BackgroundColor(Color::srgba(0.025, 0.032, 0.04, 0.93)),
            BorderColor::all(Color::srgb(0.4, 0.37, 0.26)),
        ))
        .id();
    super::windows::passive(commands, frame);
    super::windows::identify(commands, frame, "TARGET");
    commands.entity(frame).with_children(|panel| {
        panel.spawn((
            TargetName,
            Text::new("No target"),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            TextColor(Color::srgb(0.9, 0.85, 0.65)),
        ));
        panel
            .spawn((
                Node {
                    height: px(5),
                    width: percent(100),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.13, 0.10, 0.10)),
            ))
            .with_children(|bar| {
                bar.spawn((
                    TargetHp,
                    Node {
                        height: percent(100),
                        width: percent(0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.65, 0.22, 0.22)),
                ));
            });
        panel.spawn((
            TargetDetails,
            Text::new("Click or Tab to select"),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(Color::srgb(0.7, 0.73, 0.77)),
        ));
    });
}

/// Picks visible model triangles or cycles the stable ID order of nearby rendered entities.
#[allow(
    clippy::too_many_arguments,
    clippy::needless_pass_by_value,
    clippy::too_many_lines
)]
pub(super) fn input(
    settings: Option<Res<super::ViewerSettings>>,
    mut attempted: Local<bool>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    picker: picking::Picker,
    collision: Option<Res<super::Collision>>,
    nearby: Res<NearbyEntities>,
    online: Res<OnlineState>,
    mut chat: ResMut<super::chat::ChatState>,
    commands: Res<CommandsToServer>,
    mut target: ResMut<TargetState>,
    ui: super::windows::pointer::PointerUi,
) {
    if target.session != online.session_id {
        *target = TargetState {
            session: online.session_id,
            ..default()
        };
    }
    let requested = chat.requested_target.take();
    if !online.connected || online.death.is_some() {
        target.selected = None;
        target.sent = false;
        return;
    }
    let ids = targetable(nearby.rendered.keys().copied(), &online);
    let own_id = online.player.as_ref().map(|player| player.spawn_id);
    // A target lasts until its spawn despawns, is replaced or turns invisible, however
    // far away it goes; drawing range only limits what can be clicked or cycled.
    let invalid = target.selected.is_some_and(|id| {
        Some(id) != own_id
            && (online.spawns.get(&id).is_none_or(|spawn| spawn.invisible)
                || online.revisions.get(&id).copied() != target.revision)
    });
    let mut proposal = invalid.then_some(None);
    if !*attempted
        && settings
            .as_ref()
            .is_some_and(|s| s.0.validation == Some(super::ValidationAction::TargetNearestPlayer))
        && let Some(player) = &online.player
    {
        let origin = Vec3::from_array(eq_client_core::render_position(player.position));
        let nearest = ids
            .iter()
            .filter_map(|id| {
                let spawn = &online.spawns[id];
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
            eprintln!("One-shot target: selecting nearby player spawn {id}");
        }
    }
    let accepts_input = !chat.composing
        && !chat.escape_consumed
        && windows.single().is_ok_and(|window| window.focused);
    if let Some(name) = requested {
        match named(&ids, &online, &name) {
            Some(id) => proposal = Some(Some(id)),
            None => target.status = format!("No nearby target named {name}"),
        }
    } else if accepts_input && keys.just_pressed(KeyCode::Escape) {
        proposal = Some(None);
    } else if accepts_input && keys.just_pressed(KeyCode::F1) {
        proposal = own_id.map(Some);
    } else if accepts_input && keys.just_pressed(KeyCode::Tab) {
        proposal = Some(cycle(
            &ids,
            target.selected,
            keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        ));
    } else if accepts_input
        && mouse.just_pressed(MouseButton::Left)
        && !chat.hovered
        && let (Ok(window), Ok((camera, camera_transform))) = (windows.single(), cameras.single())
    {
        let over_ui = window
            .physical_cursor_position()
            .is_some_and(|cursor| ui.contains(cursor));
        if !over_ui
            && let Some(cursor) = window.cursor_position()
            && let Ok(ray) = camera.viewport_to_world(camera_transform, cursor)
        {
            let hit_id = ids
                .iter()
                .filter_map(|id| {
                    let distance = picker.distance(nearby.rendered[id], ray)?;
                    let blocked = collision
                        .as_ref()
                        .and_then(|collision| collision.0.as_ref())
                        .and_then(|world| world.ray_distance(ray.origin, *ray.direction, distance))
                        .is_some_and(|obstruction| obstruction + 0.01 < distance);
                    (!blocked).then_some((*id, distance))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id);
            proposal = Some(hit_id);
        }
    }
    let Some(selected) = proposal else {
        return;
    };
    if selected == target.selected && !invalid {
        return;
    }
    if online.enabled {
        let Some(session_id) = online.session_id else {
            return;
        };
        let Some(sender) = &commands.0 else {
            target.status = "Command queue unavailable".into();
            return;
        };
        if sender
            .try_send(ClientCommand::SelectTarget {
                session_id,
                spawn_id: selected,
            })
            .is_err()
        {
            target.status = "Target request could not be queued".into();
            return;
        }
    }
    target.selected = selected;
    target.revision = selected.and_then(|id| online.revisions.get(&id).copied());
    target.sent = false;
    target.status = if online.enabled {
        "Sending selection".into()
    } else {
        "Offline selection".into()
    };
}

/// Drawn spawns the player may target: visible ones within the zone's far clip.
/// The official client targets nothing past its clip plane, and servers log a
/// target beyond it as a possible cheat; drawn spawns may linger a little past it.
fn targetable(rendered: impl Iterator<Item = u16>, online: &OnlineState) -> Vec<u16> {
    let reachable = |spawn: &eq_client_core::SpawnState| {
        online
            .far_clip
            .zip(online.player.as_ref())
            .is_none_or(|(clip, player)| {
                eq_client_core::entities::within(player.position, spawn.position, clip)
            })
    };
    rendered
        .filter(|id| {
            online
                .spawns
                .get(id)
                .is_some_and(|spawn| !spawn.invisible && reachable(spawn))
        })
        .collect()
}

/// The nearest visible spawn whose shown name, or full server name such as
/// `a_whiskered_bat002` (as EQ's `/target` also accepts), starts with `query`,
/// ignoring case; underscores in the query already read as spaces.
fn named(ids: &[u16], online: &OnlineState, query: &str) -> Option<u16> {
    let query = query.to_lowercase();
    let origin = online
        .player
        .as_ref()
        .map(|player| Vec3::from_array(eq_client_core::render_position(player.position)));
    ids.iter()
        .filter_map(|id| {
            let spawn = online.spawns.get(id)?;
            let shown = super::combat::display_name(&spawn.name).to_lowercase();
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

/// Shows server-supplied target identity and HP; unknown HP is never shown as full.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn update(
    online: Res<OnlineState>,
    target: Res<TargetState>,
    combat: Option<Res<super::combat::CombatState>>,
    mut texts: Query<(&mut Text, Option<&TargetName>, Option<&TargetDetails>)>,
    mut bars: Query<&mut Node, With<TargetHp>>,
) {
    let spawn = target.selected.and_then(|id| online.spawns.get(&id));
    let own = online
        .player
        .as_ref()
        .filter(|player| target.selected == Some(player.spawn_id));
    let hp = target
        .selected
        .and_then(|id| online.health.get(&id))
        .copied()
        .or_else(|| own.and_then(|player| player.hp_percent));
    for (mut text, name, details) in &mut texts {
        if name.is_some() {
            text.0 = spawn.map_or_else(
                || {
                    if own.is_some() {
                        "You".into()
                    } else {
                        "No target".into()
                    }
                },
                |s| s.name.replace('_', " "),
            );
        }
        if details.is_some() {
            text.0 = spawn.map_or_else(
                || {
                    own.map_or_else(
                        || {
                            if target.status.is_empty() {
                                "Click / Tab target | F1 self | Esc clear".into()
                            } else {
                                target.status.clone()
                            }
                        },
                        |_| {
                            format!(
                                "You   HP {}   {}",
                                hp.map_or_else(|| "--".into(), |v| format!("{v}%")),
                                if target.sent {
                                    "Request sent"
                                } else {
                                    &target.status
                                }
                            )
                        },
                    )
                },
                |s| {
                    let kind = match s.kind {
                        SpawnKind::Player => "Player",
                        SpawnKind::Npc => "NPC",
                        _ => "Corpse",
                    };
                    let attacking = combat.as_ref().is_some_and(|combat| combat.auto_attack);
                    let keys = match s.kind {
                        _ if attacking => "G stop attacking | K consider",
                        SpawnKind::Npc => "K consider | G attack | H hail | U trade",
                        SpawnKind::Player => "K consider | H hail",
                        _ => "L loot",
                    };
                    format!(
                        "{kind}   HP {}   {}\n{keys}",
                        hp.map_or_else(|| "--".into(), |v| format!("{v}%")),
                        if attacking {
                            "Attacking"
                        } else if target.sent {
                            "Request sent"
                        } else {
                            &target.status
                        },
                    )
                },
            );
        }
    }
    for mut bar in &mut bars {
        bar.width = percent(f32::from(hp.unwrap_or(0)));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rejection_clears_matching_selection_but_preserves_a_newer_target() {
        let mut state = super::TargetState {
            selected: Some(7),
            sent: true,
            session: Some(4),
            revision: Some(2),
            ..Default::default()
        };
        state.reject(Some(8), "Old request");
        assert_eq!(state.selected, Some(7));
        assert!(state.sent);
        state.reject(Some(7), "Unavailable");
        assert_eq!(state.selected, None);
        assert_eq!(state.session, Some(4));
        assert_eq!(state.revision, None);
        assert!(!state.sent);
        assert_eq!(state.status, "Target rejected: Unavailable");
    }
    use super::*;
    use eq_client_core::{SpawnState, WorldPosition};

    #[test]
    fn named_targets_accept_the_shown_or_full_server_name() {
        let mut online = OnlineState::new(false);
        for (id, name) in [(2, "a_whiskered_bat002"), (3, "a_whiskered_bat005")] {
            online.spawns.insert(
                id,
                SpawnState {
                    class: None,
                    spawn_id: id,
                    name: name.into(),
                    kind: SpawnKind::Npc,
                    race: 1,
                    gender: 0,
                    position: WorldPosition::default(),
                    size: 0.0,
                    invisible: false,
                },
            );
        }
        assert!(named(&[2, 3], &online, "a whiskered bat").is_some());
        // `/target a_whiskered_bat005` arrives with its underscores read as spaces.
        assert_eq!(named(&[2, 3], &online, "a whiskered bat005"), Some(3));
        assert_eq!(named(&[2, 3], &online, "a whiskered bat009"), None);
    }

    #[test]
    fn spawns_past_the_zones_far_clip_cannot_be_targeted() {
        let mut online = OnlineState::new(true);
        online.player = Some(eq_client_core::PlayerState {
            name: "Example".into(),
            base_attributes: None,
            deity: None,
            class: Some(1),
            spawn_id: 1,
            race: 1,
            gender: 0,
            level: 1,
            position: WorldPosition::default(),
            mana: 0,
            endurance: None,
            skills: None,
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 0.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: None,
        });
        for (id, x) in [(2, 90.0), (3, 110.0)] {
            online.spawns.insert(
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
                    size: 0.0,
                    invisible: false,
                },
            );
        }
        assert_eq!(targetable([2, 3].into_iter(), &online), [2, 3]);
        online.far_clip = Some(100.0);
        assert_eq!(targetable([2, 3].into_iter(), &online), [2]);
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "Keep the ordered integration scenario and its assertions together"
    )]
    fn keyboard_cycles_only_rendered_entities_and_keeps_targets_until_they_are_gone() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<NearbyEntities>()
            .insert_resource(OnlineState::new(false))
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<TargetState>()
            .insert_resource(CommandsToServer(None))
            .add_systems(Update, input);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        {
            let mut online = app.world_mut().resource_mut::<OnlineState>();
            online.connected = true;
            online.session_id = Some(1);
            for id in [2, 3, 4] {
                online.spawns.insert(
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
                        size: 0.0,
                        invisible: false,
                    },
                );
                online.revisions.insert(id, 1);
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
            app.world().resource::<TargetState>().selected
        };
        assert_eq!(press(&mut app, KeyCode::Tab, false), Some(2));
        app.world_mut()
            .resource_mut::<super::super::chat::ChatState>()
            .composing = true;
        assert_eq!(press(&mut app, KeyCode::Tab, false), Some(2));
        assert_eq!(press(&mut app, KeyCode::Escape, false), Some(2));
        {
            let mut chat = app
                .world_mut()
                .resource_mut::<super::super::chat::ChatState>();
            chat.composing = false;
            chat.escape_consumed = true;
        }
        assert_eq!(press(&mut app, KeyCode::Escape, false), Some(2));
        app.world_mut()
            .resource_mut::<super::super::chat::ChatState>()
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
        app.world_mut()
            .resource_mut::<OnlineState>()
            .revisions
            .insert(2, 2);
        app.update();
        assert_eq!(app.world().resource::<TargetState>().selected, None);
        assert_eq!(press(&mut app, KeyCode::Tab, false), Some(2));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        // A visibility change clears a selected target and removes it from Tab cycling.
        app.world_mut()
            .resource_mut::<OnlineState>()
            .spawns
            .get_mut(&2)
            .unwrap()
            .invisible = true;
        app.update();
        assert_eq!(app.world().resource::<TargetState>().selected, None);
        assert_eq!(press(&mut app, KeyCode::Tab, false), Some(3));
        app.world_mut()
            .resource_mut::<OnlineState>()
            .spawns
            .get_mut(&2)
            .unwrap()
            .invisible = false;
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
        assert_eq!(app.world().resource::<TargetState>().selected, Some(2));
        app.world_mut()
            .resource_mut::<OnlineState>()
            .spawns
            .remove(&2);
        app.update();
        assert_eq!(app.world().resource::<TargetState>().selected, None);
        app.world_mut().resource_mut::<OnlineState>().session_id = Some(2);
        app.update();
        assert_eq!(app.world().resource::<TargetState>().selected, None);
    }

    #[test]
    fn self_target_uses_profile_without_a_nearby_entity_and_survives_culling() {
        let mut app = App::new();
        let mut online = OnlineState::new(true);
        online.connected = true;
        online.session_id = Some(1);
        online.player = Some(eq_client_core::PlayerState {
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
            spell_refresh_ms: None,
            memorized_spells: [None; 8],
            size: 6.0,
            walk_speed: 0.0,
            run_speed: 0.0,
            hp_percent: Some(55),
        });
        let (tx, rx) = std::sync::mpsc::sync_channel(2);
        app.insert_resource(online)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<NearbyEntities>()
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<TargetState>()
            .insert_resource(CommandsToServer(Some(tx)))
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
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::F1);
        app.update();
        assert!(matches!(
            rx.try_recv().unwrap(),
            ClientCommand::SelectTarget {
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
        assert_eq!(app.world().resource::<TargetState>().selected, Some(7));
        assert!(rx.try_recv().is_err());
        app.world_mut()
            .resource_mut::<OnlineState>()
            .health
            .insert(7, 31);
        app.update();
        assert_eq!(app.world().get::<Node>(bar).unwrap().width, percent(31));
    }
}
