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
        centered_line(
            panel,
            Node::default(),
            (
                TargetName,
                Text::new("No target"),
                theme::bold_font(Size::Heading),
                TextColor(theme::INK_WARM),
            ),
        );
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
        centered_line(
            panel,
            Node::default(),
            (
                TargetDetails,
                theme::text("Click or Tab to select", Size::Small, theme::INK),
            ),
        );
    });
}

/// Centers a line in its box. Auto margins absorb spare room and leave an
/// oversized name at the left edge so clipping keeps the start readable.
pub(super) fn centered_line(parent: &mut ChildSpawnerCommands, bounds: Node, text: impl Bundle) {
    parent
        .spawn(Node {
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            ..bounds
        })
        .with_child((
            text,
            Node {
                flex_shrink: 0.0,
                margin: UiRect::axes(Val::Auto, px(0)),
                ..default()
            },
            TextLayout::new(Justify::Left, LineBreak::NoWrap),
        ));
}

/// Picks visible model triangles or cycles the stable ID order of nearby rendered entities.
#[allow(
    clippy::too_many_arguments,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::type_complexity
)]
pub(super) fn input(
    scene: Scene,
    mut scripted: ResMut<ScenePointer>,
    mut attempted: Local<bool>,
    mut door_clicks: Local<DoorClicks>,
    keys: super::keys::Keys,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut online: ResMut<OnlineState>,
    mut chat: ResMut<super::chat::ChatState>,
    outbox: Res<Outbox>,
    (ui, escape, presses, mut trade, messages, options): (
        super::windows::pointer::PointerUi,
        Res<super::escape::Escape>,
        Res<super::hud::hotbar::carry::Presses>,
        ResMut<super::trade::TradeState>,
        Option<Res<super::hud::messages::Messages>>,
        Option<Res<crate::options::OptionsState>>,
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
    let settings = scene.settings.as_ref();
    let ids = targetable(scene.nearby.rendered.keys().copied(), &online);
    let own_id = online.world().player().map(|player| player.spawn_id);
    // A target lasts until its spawn despawns, is replaced or turns invisible, however
    // far away it goes; drawing range only limits what can be clicked or cycled.
    let invalid = online.world().target_stale() && !online.world().target().server_selected;
    let current = online.world().target().selected;
    let mut proposal = invalid.then_some(None);
    if !*attempted
        && settings
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
    // A script's scene click points with the script's pointer, never the
    // real one; the gates are the same (checklist 6.587).
    let request = scripted.request;
    let cursor = windows.single().ok().and_then(|window| {
        let at = match &request {
            Some(request) => Some(request.at?),
            None => window.cursor_position(),
        }?;
        let physical = at * window.scale_factor();
        // The script's pointer is its own input, so only a window over
        // it blocks it, never the script's ownership of the input.
        let covered = if request.is_some() {
            ui.covers(physical)
        } else {
            ui.contains(physical)
        };
        (keys.escape_free() && !presses.taken() && !chat.hovered && !covered).then_some(at)
    });
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
        match rule {
            // Titanium (read R49-33 b): an empty name says 0x33d3; any other
            // looks over every known spawn, drawn or not, and a name that
            // matches none sends the target naming no one, with no line.
            // Spawns this client can't target are skipped, Adam's call, a
            // labelled departure from the program's look over all of them.
            TargetByName::Nearest if name.is_empty() => {
                let said = messages.as_ref().map_or_else(
                    || super::chat::Said::own(EMPTY_TARGET_NAME),
                    |messages| messages.said_text(EMPTY_TARGET_LINE, EMPTY_TARGET_NAME),
                );
                chat.refuse(said.typed(EMPTY_TARGET_COLOUR));
            }
            TargetByName::Nearest => {
                let known = targetable(
                    online
                        .world()
                        .spawns()
                        .keys()
                        .copied()
                        .collect::<Vec<_>>()
                        .into_iter(),
                    &online,
                );
                proposal = Some(named(rule, &known, &online, &name));
            }
            TargetByName::Shortest if name.is_empty() => chat.refuse(EMPTY_TARGET_NAME.to_owned()),
            TargetByName::Shortest => match named(rule, &ids, &online, &name) {
                Some(id) => proposal = Some(Some(id)),
                None => chat.refuse(format!("No nearby target named {name}")),
            },
        }
    } else if let Some(id) = assisted {
        proposal = Some(Some(id));
    } else if *escape == super::escape::Escape::Target {
        proposal = Some(None);
    } else if keys.pressed(Act::TargetSelf) {
        proposal = own_id.map(Some);
    } else if let Some(act) = keys.first([Act::TargetNext, Act::TargetPrevious]) {
        proposal = Some(cycle(&ids, current, act == Act::TargetPrevious));
    } else if keys.pressed(Act::Use)
        && super::interact::by_ray(&online, &super::interact::qol(options.as_deref()))
    {
        if let Some(chosen) = use_key(&scene, &mut online, &outbox, &mut trade) {
            proposal = Some(Some(chosen));
        }
    } else if let Some(cursor) = cursor
        && (door_release || mouse.any_just_pressed([MouseButton::Left, MouseButton::Right]))
        && let Some(Picked {
            choice,
            spawns,
            door_point,
        }) = scene.pick(&online, cursor, door_cursor.is_some())
    {
        let client = scene.client();
        // A script's click acts only on what it named: anything else the
        // pick finds there now refuses it, and nothing is done.
        if let Some(wanted) = request.as_ref().map(|request| request.thing) {
            if !wanted.is(choice) {
                scripted.refused = Some(wanted.covered().to_owned());
                door_clicks.cancel();
                return;
            }
            scripted.acted = true;
        }
        let picked_door = match choice {
            Choice::Door(door) => Some(door),
            _ => None,
        };
        if door_release
            && let Some(id) = door_clicks.release(picked_door, cursor, now)
            && let Some((_, point)) = door_point.filter(|(picked, _)| *picked == id)
        {
            super::doors::click(id, point, &online, &outbox);
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
                // A spawn the program will not interact with by its body
                // type: nothing at all (R44, 0x4a7356).
                Choice::Spawn(id, _) if !interacts(&online, id) => {}
                Choice::Spawn(id, _) if mouse.just_pressed(MouseButton::Right) => {
                    // Use the clicked entity without changing the current
                    // target. The trade path falls back to consideration.
                    trade.right_click(online.world(), id);
                }
                // One the target setter refuses by body type keeps the
                // current target, with no line and nothing sent (R44).
                Choice::Spawn(id, _) if refused_by_body(&online, id) => {}
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

/// The Use key, as key command 0x10 casts it (read R49-19, checklist
/// 6.609, and its follow-ups): the scene's own click pick through the
/// centre of the 3D view (0x494ac5).
/// - **A spawn** is targeted (0x4a6f0e). Then, if it is an NPC, or no
///   target stands afterwards, it gets what a right click does (0x4a7356).
///   A player or corpse that became the target is only targeted.
/// - **No spawn:** a door on the same ray gets a click at the hit point,
///   whose reach the session judges (0x49812e). Nothing else is reached:
///   not an item on the ground nor a world container, which open by their
///   own clicks.
/// - **Neither:** nothing happens and nothing is said.
///
/// Gives the spawn to target, if any. The program's pick skips its own
/// actor, its mount and "not pickable" actors ([I] the last two); this
/// skips what a mouse click skips.
fn use_key(
    scene: &Scene,
    online: &mut OnlineState,
    outbox: &Outbox,
    trade: &mut super::trade::TradeState,
) -> Option<u16> {
    let centre = scene.centre()?;
    // 0x494ac5: the first actor on the ray, doors aside.
    match scene.pick(online, centre, false)?.choice {
        // A spawn the program will not interact with by its body type:
        // nothing at all, as for a click (R44).
        Choice::Spawn(id, _) if !interacts(online, id) => None,
        Choice::Spawn(id, _) => {
            // One the target setter refuses by body type keeps the current
            // target (R44).
            let refused = refused_by_body(online, id);
            let npc = online
                .world()
                .spawn(id)
                .is_some_and(|spawn| spawn.state.kind == SpawnKind::Npc);
            // 0x4a7356 runs for an NPC, or where no target stands after
            // the targeting.
            if npc || (refused && online.world().target().selected.is_none()) {
                trade.right_click(online.world(), id);
            }
            (!refused).then_some(id)
        }
        // 0x49812e: the same ray, mapped to a door only, with its hit
        // point. An item on the ground is never reached by this key.
        Choice::Door(_) | Choice::Item(_) | Choice::Nothing => {
            let picked = scene.pick(online, centre, true)?;
            if let (Choice::Door(door), Some((id, point))) = (picked.choice, picked.door_point)
                && door.id == id
            {
                super::doors::click(id, point, online, outbox);
            }
            None
        }
    }
}

/// Shared submission for scene, keyboard and map choices. A failed send never
/// changes the local target; choosing the current target sends nothing again.
pub(crate) fn select(online: &mut OnlineState, outbox: &Outbox, selected: Option<u16>) {
    if let Err(refusal) = try_select(online, outbox, selected, None) {
        outbox.refused(refusal);
    }
}

/// Selects a spawn and reports queue refusal to callers with their own status.
pub(super) fn try_select(
    online: &mut OnlineState,
    outbox: &Outbox,
    selected: Option<u16>,
    expected_pet: Option<eq_client_core::pets::PetIdentity>,
) -> Result<(), crate::outbox::Refusal> {
    if selected == online.world().target().selected && !online.world().target_stale() {
        return Ok(());
    }
    let request_id = online.world().next_target_request();
    // Offline, the choice is the viewer's alone; online, the outbox says
    // why a choice did not leave.
    if online.enabled {
        outbox.try_post(online.world(), |stamp| ClientCommand::SelectTarget {
            session_id: stamp.session_id,
            request_id,
            spawn_id: selected,
            expected_pet,
        })?;
    }
    // A choice sent says nothing, as in the official client.
    online.select_target(selected);
    Ok(())
}

/// What the scene's click pick reads besides the cursor: the camera, what
/// is drawn, and what stands in the way.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Scene<'w, 's> {
    settings: Option<Res<'w, super::ViewerSettings>>,
    // The world camera; the paperdoll's camera films the inventory figure.
    cameras: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<super::OrbitCamera>>,
    picker: picking::Picker<'w, 's>,
    doors: Query<
        'w,
        's,
        (
            Entity,
            &'static super::doors::DoorEntity,
            &'static Transform,
            &'static Visibility,
        ),
    >,
    door_models: Option<Res<'w, super::doors::Models>>,
    ground: Query<'w, 's, (&'static super::ground::GroundEntity, &'static Transform)>,
    collision: Option<Res<'w, super::Collision>>,
    nearby: Res<'w, NearbyEntities>,
}

/// What a click at one point of the view picks, and the spawns it weighed.
struct Picked {
    choice: Choice,
    spawns: Vec<Candidate>,
    /// Where the pointer's ray hit the picked door as drawn: the program
    /// measures a click's reach from there (read R16b, R16e; 6.530).
    door_point: Option<(u8, Vec3)>,
}

impl Scene<'_, '_> {
    fn client(&self) -> InstalledClient {
        self.settings
            .as_ref()
            .map_or_else(InstalledClient::default, |s| s.0.installed_client)
    }

    /// The pick a click at this point of the view makes, the player's or a
    /// script's alike: the nearest drawn thing the ray reaches with nothing
    /// solid in front, by the installed generation's rule. None without a
    /// world camera or a ray through the point.
    fn pick(&self, online: &OnlineState, cursor: Vec2, with_doors: bool) -> Option<Picked> {
        let (camera, camera_transform) = self.cameras.single().ok()?;
        let ray = camera.viewport_to_world(camera_transform, cursor).ok()?;
        // A click meets every drawn spawn it could target, and also the ones
        // the program's target setter refuses by body type, which then keep
        // the current target (program read R44: the pick 0x494ac5 takes
        // them, and 0x4a6f0e puts the previous target back).
        let clickable = clickable(self.nearby.rendered.keys().copied(), online);
        let blocked = |distance: f32, ignore_door: Option<u8>| {
            self.collision
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
        let rule = self.client().spawn_pick();
        let shown = |point: Vec3| camera.world_to_viewport(camera_transform, point).ok();
        let spawns: Vec<Candidate> = clickable
            .iter()
            .map(|id| {
                let pick = self.picker.pick(self.nearby.rendered[id], ray);
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
            self.ground.iter(),
            crate::outbox::offered(online.world(), eq_client_core::Capability::WorldContainers),
        )
        .filter(|hit| !blocked(hit.1, None));
        let door = if let Some(models) = self.door_models.as_ref().filter(|_| with_doors) {
            self.doors
                .iter()
                .filter_map(|(entity, door, transform, visibility)| {
                    let (distance, point) =
                        super::doors::hit_point(ray, door, transform, *visibility, models)
                            .filter(|(d, _)| !blocked(*d, Some(door.id)))?;
                    Some((
                        Door {
                            entity,
                            id: door.id,
                        },
                        distance,
                        point,
                    ))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.id.cmp(&b.0.id)))
        } else {
            None
        };
        Some(Picked {
            choice: choose(
                rule,
                &spawns,
                item,
                door.map(|(door, distance, _)| (door, distance)),
            ),
            spawns,
            door_point: door.map(|(door, _, point)| (door.id, point)),
        })
    }

    /// The centre of the world camera's 3D view rectangle, in the window's
    /// logical pixels, where the Use key casts its ray (read R49-19: the
    /// middle of the view's left, right, top and bottom).
    fn centre(&self) -> Option<Vec2> {
        let (camera, _) = self.cameras.single().ok()?;
        Some(camera.logical_viewport_rect()?.center())
    }

    /// Where on the view a click may name this thing, best first: the
    /// middle of what the pick tests of it (a spawn's posed box, an
    /// object's pick box, a door's pick mesh), and for a door then points
    /// on its drawn faces ([`super::doors::aim_points`]), projected. Empty
    /// when it isn't drawn; a point behind the camera is left out.
    fn aim(&self, thing: Thing) -> Vec<Vec2> {
        let Ok((camera, camera_transform)) = self.cameras.single() else {
            return Vec::new();
        };
        let points = match thing {
            Thing::Spawn(id) => self
                .nearby
                .rendered
                .get(&id)
                .and_then(|root| {
                    let ray = Ray3d::new(Vec3::ZERO, Dir3::Y);
                    self.picker.pick(*root, ray).posed
                })
                .map(|posed| posed.middle())
                .into_iter()
                .collect(),
            Thing::Object(drop_id) => self
                .ground
                .iter()
                .find_map(|(entity, transform)| super::ground::middle(entity, transform, drop_id))
                .into_iter()
                .collect(),
            Thing::Door(id) => self
                .door_models
                .as_ref()
                .and_then(|models| {
                    self.doors
                        .iter()
                        .find_map(|(_, door, transform, visibility)| {
                            (door.id == id).then(|| {
                                super::doors::aim_points(door, transform, *visibility, models)
                            })
                        })
                })
                .unwrap_or_default(),
        };
        points
            .into_iter()
            .filter_map(|point| camera.world_to_viewport(camera_transform, point).ok())
            .collect()
    }
}

impl Scene<'_, '_> {
    /// Why a click at `at`, a thing's middle on the view, picks something
    /// else: what it picks, and for a door what its ray met of the door and
    /// what solid stands nearer. For a script's report only.
    fn missed(&self, thing: Thing, at: Vec2, choice: Choice) -> String {
        let picks = match choice {
            Choice::Spawn(id, _) => format!("spawn {id}"),
            Choice::Item(id) => format!("object {id}"),
            Choice::Door(door) => format!("door {}", door.id),
            Choice::Nothing => "nothing".to_owned(),
        };
        let Thing::Door(id) = thing else {
            return format!("a click at its middle picks {picks}");
        };
        let ray = self
            .cameras
            .single()
            .ok()
            .and_then(|(camera, transform)| camera.viewport_to_world(transform, at).ok());
        let models = self.door_models.as_ref();
        let met = ray.zip(models).and_then(|(ray, models)| {
            self.doors
                .iter()
                .find_map(|(_, door, transform, visibility)| {
                    (door.id == id)
                        .then(|| super::doors::hit_point(ray, door, transform, *visibility, models))
                        .flatten()
                })
        });
        let Some((distance, _)) = met else {
            return format!(
                "a click at its middle picks {picks}; the ray there misses the door's drawn faces"
            );
        };
        let solid = ray.and_then(|ray| {
            self.collision
                .as_ref()
                .and_then(|collision| collision.0.as_ref())
                .and_then(|world| {
                    world
                        .without_doors(&[id])
                        .ray_distance(ray.origin, *ray.direction, distance)
                })
        });
        match solid {
            Some(nearer) if nearer + 0.01 < distance => format!(
                "a click at its middle picks {picks}; the ray meets the door at {distance:.1} \
                 but something solid at {nearer:.1}"
            ),
            _ => format!(
                "a click at its middle picks {picks}; the ray meets the door at {distance:.1}"
            ),
        }
    }
}

/// A thing in the scene a script names for a click (checklist 6.587).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Thing {
    Spawn(u16),
    /// An object on the ground, by its drop id.
    Object(u32),
    Door(u8),
}

impl Thing {
    /// Whether a click's pick names this thing.
    const fn is(self, choice: Choice) -> bool {
        match (self, choice) {
            (Self::Spawn(wanted), Choice::Spawn(id, _)) => wanted == id,
            (Self::Object(wanted), Choice::Item(id)) => wanted == id,
            (Self::Door(wanted), Choice::Door(door)) => wanted == door.id,
            _ => false,
        }
    }

    /// Why a click can't name it from here.
    pub(crate) const fn covered(self) -> &'static str {
        match self {
            Self::Spawn(_) => "that spawn is covered or off screen from here",
            Self::Object(_) => "that object is covered or off screen from here",
            Self::Door(_) => "that door is covered or off screen from here",
        }
    }
}

/// A script's scene click this frame: what it names and where its pointer
/// is. The script sets it every frame of the click and the scene takes it,
/// so a run that stops leaves the scene its real pointer at once.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SceneRequest {
    pub(crate) thing: Thing,
    /// The script pointer, once it is on its way; None while aiming.
    pub(crate) at: Option<Vec2>,
}

/// The scene's side of a script's scene click (checklist 6.587).
#[derive(Resource, Default, Debug)]
pub(crate) struct ScenePointer {
    /// This frame's request, which the scene takes.
    pub(crate) request: Option<SceneRequest>,
    /// Where the click would land, checked with the scene's own pick, or
    /// why it can't: written each frame a request names a thing.
    pub(crate) aim: Option<Result<Vec2, String>>,
    /// A press whose pick named something else: nothing was done.
    pub(crate) refused: Option<String>,
    /// A press whose pick named the thing and acted.
    pub(crate) acted: bool,
}

/// Checks where a script's scene click would land: the thing's middle on
/// the view, under no window, and the pick there naming it. Runs after the
/// scene's input, which read the pointer, and takes the request.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn aim(
    scene: Scene,
    online: Res<OnlineState>,
    mut scripted: ResMut<ScenePointer>,
    windows: Query<&Window, With<PrimaryWindow>>,
    ui: super::windows::pointer::PointerUi,
) {
    let Some(thing) = scripted.request.take().map(|request| request.thing) else {
        return;
    };
    let refuse = |why: String| format!("{}: {why}", thing.covered());
    let points = scene.aim(thing);
    let doors = matches!(thing, Thing::Door(_));
    // Why a click at one point can't name it, or None where it can.
    let check = |at: Vec2| -> Option<String> {
        let Ok(window) = windows.single() else {
            return Some("there is no window".to_owned());
        };
        let size = window.size();
        if at.x < 0.0 || at.y < 0.0 || at.x >= size.x || at.y >= size.y {
            return Some(format!(
                "its middle shows at ({:.0}, {:.0}), off the window",
                at.x, at.y
            ));
        }
        if ui.covers(at * window.scale_factor()) {
            return Some(format!(
                "a window covers its middle at ({:.0}, {:.0})",
                at.x, at.y
            ));
        }
        match scene.pick(&online, at, doors) {
            None => Some("the view casts no ray there".to_owned()),
            Some(picked) if thing.is(picked.choice) => None,
            Some(picked) => Some(scene.missed(thing, at, picked.choice)),
        }
    };
    let mut first = None;
    let mut verdict = None;
    for at in &points {
        match check(*at) {
            None => {
                verdict = Some(Ok(*at));
                break;
            }
            Some(why) => {
                first.get_or_insert(why);
            }
        }
    }
    let verdict = verdict.unwrap_or_else(|| {
        Err(refuse(match first {
            None => "it isn't drawn, or is behind the camera".to_owned(),
            Some(why) if points.len() > 1 => format!(
                "{why}; nor does a click at any of {} points on its drawn faces",
                points.len() - 1
            ),
            Some(why) => why,
        }))
    });
    scripted.aim = Some(verdict);
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

/// Drawn spawns a click may meet: the targetable ones and the ones the
/// program's target setter refuses by body type.
fn clickable(rendered: impl Iterator<Item = u16>, online: &OnlineState) -> Vec<u16> {
    rendered
        .filter(|id| match unavailable(online, *id) {
            None => true,
            Some(reason) => reason == REFUSED_BY_BODY,
        })
        .collect()
}

/// Whether the program's target setter refuses this spawn by its body type.
fn refused_by_body(online: &OnlineState, id: u16) -> bool {
    online
        .world()
        .spawn(id)
        .is_some_and(|spawn| online.world().refuses_target(&spawn.state))
}

/// Whether the program interacts with this spawn at all, by its body type.
fn interacts(online: &OnlineState, id: u16) -> bool {
    online
        .world()
        .spawn(id)
        .is_none_or(|spawn| online.world().interacts_with(&spawn.state))
}

/// Why a spawn the program's target setter refuses by its body type cannot
/// be chosen: only this client's own windows, such as the map's Target
/// button, say it. The program's paths refuse in silence (program read R44),
/// and so do this client's click, cycle and `/target` (the last two
/// inferred until their handlers are read, recording checklist 6.486).
pub(crate) const REFUSED_BY_BODY: &str = "This entity cannot be targeted";

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
    if !world.can_see(&spawn.state) {
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
    // Last, so that a refused spawn a click may still meet is otherwise
    // targetable ([`clickable`]).
    if world.refuses_target(&spawn.state) {
        return Some(REFUSED_BY_BODY);
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

/// Titanium's `/target` with no name: line 0x33d3, colour 0x111 (read
/// R49-33 b), in these words where the installation lacks it.
const EMPTY_TARGET_LINE: u32 = 0x33d3;
const EMPTY_TARGET_COLOUR: u32 = 0x111;
const EMPTY_TARGET_NAME: &str = "Use /target Name";

/// Titanium (read R49-33 b): a spawn whose full server name is exactly
/// `query` (underscores read as spaces) first; otherwise the nearest whose
/// shown name, or full server name such as `a_whiskered_bat002`, starts
/// with `query`, ignoring case. `ids` are the spawns it may pick.
fn nearest_named(ids: &[u16], online: &OnlineState, query: &str) -> Option<u16> {
    if let Some(exact) = ids.iter().copied().find(|id| {
        online
            .world()
            .spawn(*id)
            .is_some_and(|spawn| spawn.state.name.replace('_', " ") == query)
    }) {
        return Some(exact);
    }
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
                    body_type: None,
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
                    posture: None,
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
                    body_type: None,
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
                    posture: None,
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
        // Titanium's rule picks only among the spawns it is given.
        assert_eq!(named(TargetByName::Nearest, &[], &online, "a bat"), None);
        // An exact full name is picked first; otherwise the nearest of the
        // prefix matches, the first of equals (read R49-33 b).
        assert_eq!(
            named(TargetByName::Nearest, &[2, 3, 4], &online, "a bat003"),
            Some(4)
        );
        assert_eq!(
            named(TargetByName::Nearest, &[2, 3, 4], &online, "a bat"),
            Some(2)
        );
        assert_eq!(
            named(TargetByName::Nearest, &[3, 4], &online, "A BAT"),
            Some(3)
        );
        // Exact is exact: another case falls back to the nearest prefix.
        assert_eq!(
            named(TargetByName::Nearest, &[3, 4], &online, "A BAT003"),
            Some(4)
        );
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
                        body_type: None,
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
                        posture: None,
                    },
                );
            }
            online
        };
        assert_eq!(targetable([2, 3].into_iter(), &zone(None)), [2, 3]);
        assert_eq!(targetable([2, 3].into_iter(), &zone(Some(100.0))), [2]);
    }

    /// A zone of the given generation holding NPCs 2 to 8 with no body
    /// type, then body types 1, 11, 60, 65, 67 and 64.
    fn body_type_zone(generation: eq_client_core::GameDialect) -> OnlineState {
        use crate::online::testing as session;
        let mut online = OnlineState::new(true);
        session::generation(&mut online, generation);
        session::enter(&mut online, 1, session::player(1), None);
        for (id, body_type) in [
            (2, None),
            (3, Some(1)),
            (4, Some(11)),
            (5, Some(60)),
            (6, Some(65)),
            (7, Some(67)),
            (8, Some(64)),
        ] {
            let mut spawn = session::pet(id, 99);
            spawn.pet_owner = None;
            spawn.body_type = body_type;
            session::spawn_entry(&mut online, id, spawn);
        }
        online
    }

    #[test]
    fn titanium_refuses_the_body_types_its_target_setter_does_and_a_click_still_meets_them() {
        let online = body_type_zone(eq_client_core::GameDialect::Titanium);
        assert_eq!(targetable(2..=8, &online), [2, 3, 8]);
        assert_eq!(clickable(2..=8, &online), [2, 3, 4, 5, 6, 7, 8]);
        for id in 4..=7 {
            assert_eq!(unavailable(&online, id), Some(REFUSED_BY_BODY), "{id}");
            assert!(refused_by_body(&online, id), "{id}");
        }
        assert!(!refused_by_body(&online, 2) && !refused_by_body(&online, 8));
        // Interaction 0x4a7356 passes over 11, 65 and 67 only.
        assert_eq!(
            (2..=8)
                .filter(|id| !interacts(&online, *id))
                .collect::<Vec<_>>(),
            [4, 6, 7]
        );
    }

    #[test]
    fn eqmac_and_an_unknown_body_type_target_as_before() {
        let online = body_type_zone(eq_client_core::GameDialect::EqMac);
        assert_eq!(targetable(2..=8, &online), [2, 3, 4, 5, 6, 7, 8]);
        assert!((2..=8).all(|id| interacts(&online, id) && !refused_by_body(&online, id)));
        let titanium = body_type_zone(eq_client_core::GameDialect::Titanium);
        assert_eq!(unavailable(&titanium, 2), None);
    }

    #[test]
    fn a_refused_spawn_keeps_the_target_by_click_or_cycle_and_a_name_matching_none_clears_it() {
        use crate::online::testing as session;
        const GUARD: u16 = 41;
        const CONTROLLER: u16 = 42;
        let (mut app, commands, ..) = target_app();
        scene_camera(&mut app);
        let mut meshes = Assets::<Mesh>::default();
        // The controller is drawn under the cursor; the guard, far to the
        // side, has nothing a click could meet.
        let body = app
            .world_mut()
            .spawn((
                Transform::default(),
                Mesh3d(meshes.add(Cuboid::new(2.0, 2.0, 2.0))),
            ))
            .id();
        let guard = app
            .world_mut()
            .spawn(Transform::from_xyz(500.0, 0.0, 0.0))
            .id();
        app.insert_resource(meshes);
        {
            let mut nearby = app.world_mut().resource_mut::<NearbyEntities>();
            nearby.rendered.insert(CONTROLLER, body);
            nearby.rendered.insert(GUARD, guard);
        }
        {
            let mut online = app.world_mut().resource_mut::<OnlineState>();
            let mut controller = session::pet(CONTROLLER, 99);
            controller.pet_owner = None;
            controller.name = "zone_controller".into();
            controller.body_type = Some(11);
            let mut npc = session::pet(GUARD, 99);
            npc.pet_owner = None;
            npc.name = "Guard_Synthetic".into();
            npc.body_type = Some(1);
            session::spawns(&mut online, vec![controller, npc]);
            online.select_target(Some(GUARD));
        }
        let mut windows = app
            .world_mut()
            .query_filtered::<&mut Window, With<PrimaryWindow>>();
        windows
            .single_mut(app.world_mut())
            .unwrap()
            .set_cursor_position(Some(Vec2::new(400.0, 300.0)));
        let selected = |app: &App| {
            app.world()
                .resource::<OnlineState>()
                .world()
                .target()
                .selected
        };
        let said = |app: &App| {
            app.world()
                .resource::<super::super::chat::ChatState>()
                .history
                .lines(eq_client_core::chat::ChatTab::All)
                .len()
        };
        let lines = said(&app);

        // A left or right click on the controller in front: the target stays,
        // nothing is sent, nothing is said.
        for button in [MouseButton::Left, MouseButton::Right] {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.reset_all();
            mouse.press(button);
            app.update();
            assert_eq!(selected(&app), Some(GUARD), "{button:?}");
        }
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset_all();
        assert!(commands.try_recv().is_err());
        assert_eq!(
            app.world().resource::<Outbox>().take_refused(),
            Vec::<crate::outbox::Refusal>::new()
        );
        assert_eq!(said(&app), lines);

        // Target Next passes over it: from the guard it stays on the guard,
        // so nothing is sent again.
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(KeyCode::Tab);
        }
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        assert_eq!(selected(&app), Some(GUARD));
        assert!(commands.try_recv().is_err());

        // `/target` by its name finds no target, as for a spawn not there:
        // Titanium then sends the target naming no one, with no line (read
        // R49-33 b).
        app.world_mut()
            .resource_mut::<super::super::chat::ChatState>()
            .requested_target = Some("zone controller".into());
        app.update();
        assert_eq!(selected(&app), None);
        assert!(matches!(
            commands.try_recv(),
            Ok(ClientCommand::SelectTarget { spawn_id: None, .. })
        ));
        assert!(commands.try_recv().is_err());
        assert_eq!(said(&app), lines);
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
                        body_type: None,
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
                        posture: None,
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
                state: eq_client_core::vision::Invisibility::Titanium(1),
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
                state: eq_client_core::vision::Invisibility::Titanium(0),
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
                titanium_environment: None,
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
                intoxication: None,
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
                posture: None,
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
            .init_resource::<super::ScenePointer>()
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
                look: 0.0,
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

    /// A door with id 0 in the middle of the view, drawn with its pick mesh.
    fn door_scene(app: &mut App) {
        door_scene_of(
            app,
            vec![[-2.0, -2.0, 0.0], [2.0, -2.0, 0.0], [0.0, 2.0, 0.0]],
            vec![0, 1, 2],
        );
    }

    /// Door 0 of model TEST, these faces, before the test camera.
    fn door_scene_of(app: &mut App, positions: Vec<[f32; 3]>, indices: Vec<u32>) {
        app.init_resource::<Time>()
            .init_resource::<super::super::doors::Hinges>()
            .init_resource::<super::super::doors::LiftReach>();
        let source = eq_client_assets::ZoneModel {
            name: "TEST".into(),
            primitives: vec![eq_client_assets::ZonePrimitive {
                positions,
                indices,
                normals: Vec::new(),
                texture_coordinates: Vec::new(),
                texture: None,
                material_mode: eq_client_assets::MaterialMode::Opaque,
                light: Vec::new(),
            }],
            materials: vec![eq_client_assets::ObjectMaterial { double_sided: true }],
            collision: Vec::new(),
            no_hit_collision: Vec::new(),
            vertex_count: None,
            radius: None,
        };
        let mut models = super::super::doors::Models::default();
        models.0.insert(
            "TEST".into(),
            super::super::doors::Model {
                // Simulate render-world extraction. No CPU mesh remains.
                primitives: vec![(Handle::default(), Handle::default())],
                collision: Vec::new(),
                pick: super::super::doors::PickMesh::from_model(&source),
                radius: None,
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
    }

    #[test]
    #[allow(clippy::too_many_lines)] // Exercises the scene system and its real picking inputs.
    fn a_door_uses_two_completed_scene_clicks_and_ignores_drags_and_ui_clicks() {
        let (mut app, commands, ..) = target_app();
        let center = Vec2::new(400.0, 300.0);
        scene_camera(&mut app);
        door_scene(&mut app);
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
        // The click names where the pointer's ray met the door as drawn, in
        // the server's frame: the middle of the screen falls on its origin.
        let ClientCommand::ClickDoor {
            session_id: 1,
            door_id: 0,
            point: Some(point),
            ..
        } = commands.try_recv().unwrap()
        else {
            panic!("a door click with its point");
        };
        assert!(point.x.hypot(point.y).hypot(point.z) < 1e-3, "{point:?}");
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

    /// One scene frame with a script's request standing in for the
    /// pointer (checklist 6.587).
    fn scripted_frame(app: &mut App, thing: Thing, at: Option<Vec2>, pressed: bool) {
        app.world_mut().resource_mut::<ScenePointer>().request = Some(SceneRequest { thing, at });
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.clear();
        if pressed {
            mouse.reset(MouseButton::Left);
            mouse.press(MouseButton::Left);
        } else {
            mouse.release(MouseButton::Left);
        }
        app.update();
    }

    #[test]
    fn a_script_click_points_with_its_own_pointer_and_acts_only_on_what_it_names() {
        use crate::online::testing as session;

        const NPC: u16 = 42;
        let center = Vec2::new(400.0, 300.0);
        let (mut app, commands, ..) = target_app();
        scene_camera(&mut app);
        app.add_systems(Update, aim.after(input));
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
        session::spawns(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            vec![session::pet(NPC, 99)],
        );
        // The real pointer rests over nothing, off in a corner.
        let mut windows = app
            .world_mut()
            .query_filtered::<&mut Window, With<PrimaryWindow>>();
        windows
            .single_mut(app.world_mut())
            .unwrap()
            .set_cursor_position(Some(Vec2::new(5.0, 5.0)));

        // Aiming: the scene's own pick names the spawn at its middle.
        scripted_frame(&mut app, Thing::Spawn(NPC), None, false);
        let aimed = app.world_mut().resource_mut::<ScenePointer>().aim.take();
        assert!(
            matches!(aimed, Some(Ok(at)) if at.distance(center) < 1.0),
            "{aimed:?}"
        );
        assert!(app.world().resource::<ScenePointer>().request.is_none());
        // A spawn the pick doesn't name there is refused, and nothing goes.
        scripted_frame(&mut app, Thing::Spawn(NPC + 1), None, false);
        assert_eq!(
            app.world_mut().resource_mut::<ScenePointer>().aim.take(),
            Some(Err(
                "that spawn is covered or off screen from here: it isn't drawn, or is behind \
                 the camera"
                    .to_owned()
            ))
        );
        scripted_frame(&mut app, Thing::Spawn(NPC + 1), Some(center), true);
        assert_eq!(
            app.world_mut()
                .resource_mut::<ScenePointer>()
                .refused
                .take()
                .as_deref(),
            Some("that spawn is covered or off screen from here")
        );
        assert!(!app.world().resource::<ScenePointer>().acted);
        assert!(commands.try_recv().is_err());
        // Pressed where it shows, the click targets it as a person's does,
        // though the real pointer is elsewhere.
        scripted_frame(&mut app, Thing::Spawn(NPC), Some(center), true);
        assert!(app.world().resource::<ScenePointer>().acted);
        assert!(matches!(
            commands.try_recv().unwrap(),
            ClientCommand::SelectTarget {
                spawn_id: Some(NPC),
                ..
            }
        ));

        // Behind a wall the pick names nothing, so the aim refuses.
        let wall = eq_client_core::movement::CollisionWorld::new([[
            [-4.0, -4.0, 5.0],
            [4.0, -4.0, 5.0],
            [0.0, 4.0, 5.0],
        ]])
        .unwrap();
        app.insert_resource(super::super::Collision(Some(wall)));
        scripted_frame(&mut app, Thing::Spawn(NPC), None, false);
        assert!(matches!(
            app.world_mut().resource_mut::<ScenePointer>().aim.take(),
            Some(Err(_))
        ));
        // Behind the camera it has no point on the view.
        app.insert_resource(super::super::Collision(None));
        app.world_mut()
            .get_mut::<Transform>(body)
            .unwrap()
            .translation = Vec3::new(0.0, 0.0, 20.0);
        scripted_frame(&mut app, Thing::Spawn(NPC), None, false);
        assert!(matches!(
            app.world_mut().resource_mut::<ScenePointer>().aim.take(),
            Some(Err(_))
        ));
    }

    #[test]
    fn a_script_door_click_is_two_real_clicks_on_one_point() {
        let center = Vec2::new(400.0, 300.0);
        let (mut app, commands, ..) = target_app();
        scene_camera(&mut app);
        door_scene(&mut app);
        app.add_systems(Update, aim.after(input));
        app.update();
        scripted_frame(&mut app, Thing::Door(0), None, false);
        assert!(matches!(
            app.world_mut().resource_mut::<ScenePointer>().aim.take(),
            Some(Ok(_))
        ));
        // One completed click opens nothing; the second does, once.
        scripted_frame(&mut app, Thing::Door(0), Some(center), true);
        scripted_frame(&mut app, Thing::Door(0), Some(center), false);
        assert!(commands.try_recv().is_err());
        scripted_frame(&mut app, Thing::Door(0), Some(center), true);
        scripted_frame(&mut app, Thing::Door(0), Some(center), false);
        assert!(matches!(
            commands.try_recv().unwrap(),
            ClientCommand::ClickDoor { door_id: 0, .. }
        ));
        assert!(commands.try_recv().is_err());
        assert!(app.world().resource::<ScenePointer>().refused.is_none());
        // A door the pick doesn't name there is refused on the press.
        scripted_frame(&mut app, Thing::Door(1), Some(center), true);
        assert!(app.world().resource::<ScenePointer>().refused.is_some());
    }

    #[test]
    fn a_script_clicks_a_door_drawn_as_bars_on_a_bar() {
        // Two bars with a gap between, as the Neriak cells' doors are
        // drawn: the middle of the door's mesh is in the gap, where a click
        // meets nothing of it.
        let bar = |left: f32| {
            [
                [left, -2.0, 0.0],
                [left + 0.5, -2.0, 0.0],
                [left + 0.5, 2.0, 0.0],
                [left, 2.0, 0.0],
            ]
        };
        let positions = [bar(-3.0), bar(2.5)].concat();
        let indices = vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
        let center = Vec2::new(400.0, 300.0);
        let (mut app, commands, ..) = target_app();
        scene_camera(&mut app);
        door_scene_of(&mut app, positions, indices);
        app.add_systems(Update, aim.after(input));
        app.update();
        scripted_frame(&mut app, Thing::Door(0), None, false);
        let aimed = app.world_mut().resource_mut::<ScenePointer>().aim.take();
        let Some(Ok(at)) = aimed else {
            panic!("a bar's point is clickable: {aimed:?}");
        };
        assert!(at.distance(center) > 10.0, "aimed at the gap: {at:?}");
        for pressed in [true, false, true, false] {
            scripted_frame(&mut app, Thing::Door(0), Some(at), pressed);
        }
        assert!(matches!(
            commands.try_recv().unwrap(),
            ClientCommand::ClickDoor { door_id: 0, .. }
        ));
        assert!(app.world().resource::<ScenePointer>().refused.is_none());
    }

    /// A spawn's body at the scene's origin, which the test camera sees at
    /// the middle of its view.
    fn body_at_origin(app: &mut App, id: u16, kind: SpawnKind) {
        use crate::online::testing as session;
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
            .insert(id, body);
        let mut spawn = session::pet(id, 99);
        spawn.kind = kind;
        spawn.pet_owner = None;
        session::spawns(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            vec![spawn],
        );
    }

    /// One frame with the Use key freshly pressed and the real pointer off
    /// in a corner, so only the view's centre can name anything.
    fn press_use(app: &mut App) {
        let mut windows = app
            .world_mut()
            .query_filtered::<&mut Window, With<PrimaryWindow>>();
        windows
            .single_mut(app.world_mut())
            .unwrap()
            .set_cursor_position(Some(Vec2::new(5.0, 5.0)));
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset(KeyCode::KeyU);
        keys.press(KeyCode::KeyU);
        app.update();
    }

    fn right_clicked(app: &App) -> Option<u16> {
        app.world()
            .resource::<super::super::trade::TradeState>()
            .right_clicked()
    }

    #[test]
    fn the_use_key_targets_the_npc_at_the_views_centre_and_uses_it_as_a_right_click() {
        const NPC: u16 = 42;
        let (mut app, commands, ..) = target_app();
        scene_camera(&mut app);
        body_at_origin(&mut app, NPC, SpawnKind::Npc);
        press_use(&mut app);
        assert!(matches!(
            commands.try_recv().unwrap(),
            ClientCommand::SelectTarget {
                spawn_id: Some(NPC),
                ..
            }
        ));
        assert!(commands.try_recv().is_err());
        assert_eq!(right_clicked(&app), Some(NPC));
    }

    #[test]
    fn the_use_key_only_targets_a_player_or_corpse_at_the_views_centre() {
        // 0x462519 uses the spawn only when it is an NPC or no target
        // stands afterwards; these become the target.
        for (id, kind) in [
            (43, SpawnKind::Player),
            (44, SpawnKind::NpcCorpse),
            (45, SpawnKind::PlayerCorpse),
        ] {
            let (mut app, commands, ..) = target_app();
            scene_camera(&mut app);
            body_at_origin(&mut app, id, kind);
            press_use(&mut app);
            assert!(matches!(
                commands.try_recv().unwrap(),
                ClientCommand::SelectTarget {
                    spawn_id: Some(target),
                    ..
                } if target == id
            ));
            assert_eq!(right_clicked(&app), None, "{kind:?}");
        }
    }

    #[test]
    fn the_use_key_never_reaches_an_item_on_the_ground_at_the_views_centre() {
        let (mut app, commands, ..) = target_app();
        scene_camera(&mut app);
        app.world_mut()
            .spawn((super::super::ground::testing_item(7), Transform::default()));
        press_use(&mut app);
        // No pickup, and nothing else: only a door is reached (0x49812e).
        assert!(commands.try_recv().is_err());
        assert_eq!(right_clicked(&app), None);
    }

    #[test]
    fn the_use_key_clicks_the_door_at_the_views_centre_at_its_hit_point() {
        let (mut app, commands, ..) = target_app();
        scene_camera(&mut app);
        door_scene(&mut app);
        app.update();
        press_use(&mut app);
        // One press is one click, at the point the ray met the door, in the
        // server's frame: the middle of the view falls on its origin.
        let ClientCommand::ClickDoor {
            session_id: 1,
            door_id: 0,
            point: Some(point),
            ..
        } = commands.try_recv().unwrap()
        else {
            panic!("a door click with its point");
        };
        assert!(point.x.hypot(point.y).hypot(point.z) < 1e-3, "{point:?}");
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn the_use_key_with_nothing_at_the_views_centre_sends_and_says_nothing() {
        let (mut app, commands, ..) = target_app();
        scene_camera(&mut app);
        let lines = app
            .world()
            .resource::<super::super::chat::ChatState>()
            .history
            .lines(eq_client_core::chat::ChatTab::All)
            .len();
        press_use(&mut app);
        assert!(commands.try_recv().is_err());
        assert_eq!(
            app.world()
                .resource::<super::super::chat::ChatState>()
                .history
                .lines(eq_client_core::chat::ChatTab::All)
                .len(),
            lines
        );
        assert_eq!(
            app.world().resource::<Outbox>().take_refused(),
            Vec::<crate::outbox::Refusal>::new()
        );
    }

    #[test]
    fn the_use_key_casts_through_the_3d_views_centre_not_the_windows() {
        const NPC: u16 = 42;
        let (mut app, commands, ..) = target_app();
        scene_camera(&mut app);
        body_at_origin(&mut app, NPC, SpawnKind::Npc);
        // The view takes only the window's top left quarter, so the window's
        // middle is the view's bottom right corner, well off the body.
        let mut cameras = app.world_mut().query::<&mut Camera>();
        let mut camera = cameras.single_mut(app.world_mut()).unwrap();
        camera.viewport = Some(bevy::camera::Viewport {
            physical_position: UVec2::ZERO,
            physical_size: UVec2::new(400, 300),
            ..default()
        });
        press_use(&mut app);
        assert!(matches!(
            commands.try_recv().unwrap(),
            ClientCommand::SelectTarget {
                spawn_id: Some(NPC),
                ..
            }
        ));
        // A pick at the window's middle would have found nothing there.
        let mut state =
            bevy::ecs::system::SystemState::<(Scene, Res<OnlineState>)>::new(app.world_mut());
        let (scene, online) = state.get(app.world()).unwrap();
        assert_eq!(scene.centre(), Some(Vec2::new(200.0, 150.0)));
        let missed = scene.pick(&online, Vec2::new(400.0, 300.0), false).unwrap();
        assert_eq!(missed.choice, Choice::Nothing);
    }

    #[test]
    fn use_picks_nearest_leaves_the_use_key_to_the_nearest_rule() {
        let (mut app, commands, ..) = target_app();
        scene_camera(&mut app);
        door_scene(&mut app);
        let mut options = crate::options::OptionsState::default();
        options
            .options
            .qol
            .set(eq_client_core::qol::Fix::UsePicksNearest, true);
        app.insert_resource(options);
        app.update();
        press_use(&mut app);
        // The scene's pick sends nothing; the nearest rule's own system,
        // not in this app, answers the key.
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn a_door_clicked_out_of_reach_goes_to_the_session_to_judge() {
        let (mut app, commands, ..) = target_app();
        let mut bytes = [0u8; 80];
        bytes[..4].copy_from_slice(b"TEST");
        bytes[52] = 100;
        {
            let mut online = app.world_mut().resource_mut::<OnlineState>();
            crate::online::testing::doors(
                &mut online,
                &eq_client_core::doors::decode(0x4c24, &bytes)
                    .unwrap()
                    .unwrap(),
                std::time::Instant::now(),
            );
            // Far past any reach: the session says 1462, so the click goes
            // to it rather than being dropped here without a word.
            crate::online::testing::place_axis(&mut online, |position| position.x = 100.0);
        }
        let online = app.world().resource::<OnlineState>();
        let outbox = app.world().resource::<Outbox>();
        let drawn = Vec3::new(0.5, 1.0, -0.25);
        super::super::doors::click(0, drawn, online, outbox);
        let ClientCommand::ClickDoor {
            door_id: 0,
            point: Some(point),
            ..
        } = commands.try_recv().unwrap()
        else {
            panic!("a door click with its point");
        };
        assert_eq!(point, eq_client_core::world_position(drawn.to_array(), 0.0));
        // The Use key's and a script's use name no point.
        super::super::doors::open(0, online, outbox);
        assert!(matches!(
            commands.try_recv().unwrap(),
            ClientCommand::ClickDoor { point: None, .. }
        ));
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
                spawn_id: Some(7),
                expected_pet: None,
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
