//! Server dialogs and the read-only equipment reported for a player.
use crate::{
    clicks::Clicks,
    online::OnlineState,
    outbox::Outbox,
    theme::{self, Size},
    windows::{self, Shown, Stack, WindowId},
};
use bevy::prelude::*;
use eq_client_core::{
    Capability, ClientCommand, SpawnKind,
    inspection::PlayerInspection,
    popups::{PopupAction, PopupOffer},
    targets::TargetState,
    world::ClientWorld,
};
use std::time::Instant;

#[derive(Clone, Copy, Component)]
pub(super) struct DialogButton {
    admission: u64,
    serial: u64,
    action: PopupAction,
}

#[derive(Clone, Copy, Component)]
pub(super) struct InspectButton {
    admission: u64,
    target: TargetState,
}

#[derive(Component)]
pub(super) struct ScrollBody;

pub(super) fn respond(
    world: &ClientWorld,
    outbox: &Outbox,
    admission: u64,
    serial: u64,
    action: PopupAction,
) -> Result<(), &'static str> {
    let offer = world
        .popup(Instant::now())
        .ok_or("There is no current server dialog")?;
    if (offer.session_id, offer.serial) != (admission, serial) || !offer.popup.allows(action) {
        return Err("The server dialog or its button changed");
    }
    outbox
        .try_post(world, |stamp| ClientCommand::RespondToPopup {
            session_id: stamp.session_id,
            serial,
            action,
            created: stamp.created,
        })
        .map_err(crate::outbox::Refusal::text)
}

fn inspect_target(world: &ClientWorld) -> Option<(u64, TargetState)> {
    if !world.can(Capability::PlayerInspection) {
        return None;
    }
    let target = world.target_intent()?;
    let id = target.spawn_id?;
    if world.is_player(id)
        || world
            .spawn(id)
            .is_some_and(|spawn| spawn.state.kind == SpawnKind::Player)
    {
        Some((world.session_id()?, target))
    } else {
        None
    }
}

pub(super) fn inspect(
    world: &ClientWorld,
    outbox: &Outbox,
    admission: u64,
    spawn: u16,
    revision: u64,
) -> Result<(), &'static str> {
    let (current, target) = inspect_target(world).ok_or("Select a player to inspect")?;
    if (current, target.spawn_id, target.revision) != (admission, Some(spawn), revision) {
        return Err("The selected player changed");
    }
    outbox
        .try_post(world, |stamp| ClientCommand::InspectPlayer {
            session_id: stamp.session_id,
            target,
            created: stamp.created,
        })
        .map_err(crate::outbox::Refusal::text)
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn buttons(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    mut chat: ResMut<crate::chat::ChatState>,
    dialogs: Clicks<DialogButton>,
    inspect_buttons: Clicks<InspectButton>,
) {
    if let Some(button) = dialogs.iter().next() {
        if let Err(reason) = respond(
            online.world(),
            &outbox,
            button.admission,
            button.serial,
            button.action,
        ) {
            chat.refuse(reason);
        }
    } else if let Some(button) = inspect_buttons.iter().next()
        && let Some(id) = button.target.spawn_id
        && let Err(reason) = inspect(
            online.world(),
            &outbox,
            button.admission,
            id,
            button.target.revision,
        )
    {
        chat.refuse(reason);
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn inspect_button(
    mut commands: Commands,
    online: Res<OnlineState>,
    panels: Query<Entity, With<crate::target::TargetPanel>>,
    buttons: Query<(Entity, &ChildOf, &InspectButton)>,
) {
    let wanted = inspect_target(online.world());
    for panel in &panels {
        let mut exists = false;
        for (entity, parent, button) in &buttons {
            if parent.parent() != panel {
                continue;
            }
            if wanted == Some((button.admission, button.target)) {
                exists = true;
            } else {
                commands.entity(entity).despawn();
            }
        }
        if let Some((admission, target)) = wanted
            && !exists
        {
            commands.entity(panel).with_children(|body| {
                theme::button_with(
                    body,
                    InspectButton { admission, target },
                    "Inspect player",
                    Size::Small,
                );
            });
        }
    }
}

type RenderedPopup = Option<((u64, u64), Entity)>;
type RenderedInspection = Option<(PlayerInspection, Entity)>;

/// Reads the exact installed character's note. Missing or unreadable text
/// leaves automatic replies disabled, since `EQEmu` persists the reply text.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn sync_note(
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    profile: Res<crate::profile_files::Profile>,
    settings: Option<Res<crate::ViewerSettings>>,
    mut loaded: Local<Option<(u64, Option<String>)>>,
) {
    let world = online.world();
    let (Some(admission), Some((character, server))) = (world.session_id(), profile.names()) else {
        return;
    };
    if !world.in_world() || !world.can(Capability::PlayerInspection) {
        return;
    }
    if loaded
        .as_ref()
        .is_none_or(|(previous, _)| *previous != admission)
    {
        let note = settings
            .as_ref()
            .and_then(|settings| settings.0.official_settings())
            .and_then(|official| official.inspect_text(character, server));
        *loaded = Some((admission, note));
    }
    if let Some((_, pending)) = loaded.as_mut()
        && let Some(note) = pending
        && outbox
            .try_post(world, |stamp| ClientCommand::SetInspectNote {
                session_id: stamp.session_id,
                note: Some(note.clone()),
            })
            .is_ok()
    {
        *pending = None;
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn present(
    mut commands: Commands,
    mut online: ResMut<OnlineState>,
    (mut shown, mut stack): (ResMut<Shown>, ResMut<Stack>),
    escape: Res<crate::escape::Escape>,
    mut art: crate::sheets::Art,
    (mut popup, mut inspection): (Local<RenderedPopup>, Local<RenderedInspection>),
) {
    let offer = online.world().popup(Instant::now());
    let key = offer.map(|offer| (offer.session_id, offer.serial));
    if key != popup.as_ref().map(|(key, _)| *key) {
        if let Some((_, entity)) = popup.take() {
            commands.entity(entity).despawn();
        }
        let id = WindowId::ServerDialog;
        if let (Some(key), Some(offer)) = (key, offer) {
            shown.open(id);
            *popup = Some((key, dialog(&mut commands, offer)));
        } else {
            shown.close(id);
        }
    }
    let id = WindowId::PlayerInspection;
    if (!shown.is_open(id) || *escape == crate::escape::Escape::Close(id))
        && inspection
            .as_ref()
            .is_some_and(|(previous, _)| online.world().player_inspection() == Some(previous))
    {
        online.close_player_inspection();
    }
    let report = online.world().player_inspection();
    if report != inspection.as_ref().map(|(report, _)| report) {
        if let Some((_, entity)) = inspection.take() {
            commands.entity(entity).despawn();
        }
        if let Some(report) = report {
            shown.open(id);
            stack.raise(id);
            let panel = equipment(&mut commands, &mut art, report);
            *inspection = Some((report.clone(), panel));
        } else {
            shown.close(id);
        }
    }
}

fn frame(commands: &mut Commands, id: WindowId, width: f32, height: f32) -> Entity {
    windows::frame(
        commands,
        id,
        Node {
            width: px(width),
            max_width: percent(92),
            max_height: percent(75),
            height: px(height),
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            padding: UiRect::all(px(12)),
            ..default()
        },
    )
}

fn scroller() -> impl Bundle {
    (
        ScrollBody,
        windows::pointer::TakesWheel,
        ScrollPosition::default(),
        Node {
            overflow: Overflow::scroll_y(),
            flex_grow: 1.0,
            min_height: px(0),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            padding: UiRect::all(px(6)),
            ..default()
        },
        BackgroundColor(theme::WELL),
    )
}

fn dialog(commands: &mut Commands, offer: &PopupOffer) -> Entity {
    let panel = frame(commands, WindowId::ServerDialog, 460.0, 320.0);
    let popup = &offer.popup;
    commands.entity(panel).with_children(|body| {
        let title = if popup.title.trim().is_empty() {
            "Server dialog"
        } else {
            &popup.title
        };
        body.spawn(theme::text(title, Size::Heading, theme::INK_WARM));
        body.spawn(scroller()).with_child(theme::text(
            stml_text(&popup.text),
            Size::Body,
            theme::INK_BRIGHT,
        ));
        body.spawn(Node {
            column_gap: px(8),
            justify_content: JustifyContent::End,
            ..default()
        })
        .with_children(|buttons| {
            let add = |buttons: &mut ChildSpawnerCommands, action, label| {
                theme::button_with(
                    buttons,
                    DialogButton {
                        admission: offer.session_id,
                        serial: offer.serial,
                        action,
                    },
                    label,
                    Size::Body,
                );
            };
            if popup.asks_question() {
                add(buttons, PopupAction::Positive, "Yes");
                add(buttons, PopupAction::Negative, "No");
            } else if popup.allows(PopupAction::Positive) {
                add(buttons, PopupAction::Positive, "OK");
            }
            add(buttons, PopupAction::Dismiss, "Close");
        });
    });
    panel
}

const SLOT_NAMES: [&str; 22] = [
    "Charm",
    "Left ear",
    "Head",
    "Face",
    "Right ear",
    "Neck",
    "Shoulders",
    "Arms",
    "Back",
    "Left wrist",
    "Right wrist",
    "Range",
    "Hands",
    "Primary",
    "Secondary",
    "Left finger",
    "Right finger",
    "Chest",
    "Legs",
    "Feet",
    "Waist",
    "Ammo",
];

fn equipment(
    commands: &mut Commands,
    art: &mut crate::sheets::Art,
    view: &PlayerInspection,
) -> Entity {
    let panel = frame(commands, WindowId::PlayerInspection, 390.0, 490.0);
    commands.entity(panel).with_children(|body| {
        body.spawn(theme::text(&view.name, Size::Heading, theme::INK_WARM));
        body.spawn(scroller()).with_children(|list| {
            if !view.report.text.is_empty() {
                list.spawn(theme::text(
                    &view.report.text,
                    Size::Body,
                    theme::INK_BRIGHT,
                ));
            }
            let mut any = false;
            for (label, slot) in SLOT_NAMES.into_iter().zip(&view.report.equipment) {
                if slot.name.is_empty() && slot.icon.is_none() {
                    continue;
                }
                any = true;
                list.spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: px(7),
                    ..default()
                })
                .with_children(|row| {
                    if let Some(image) = slot.icon.and_then(|icon| art.item(icon)) {
                        row.spawn((
                            image,
                            Node {
                                width: px(24),
                                height: px(24),
                                flex_shrink: 0.0,
                                ..default()
                            },
                        ));
                    }
                    let name = if slot.name.is_empty() {
                        "Name unavailable"
                    } else {
                        &slot.name
                    };
                    row.spawn(theme::text(
                        format!("{label}  ·  {name}"),
                        Size::Body,
                        theme::INK,
                    ));
                });
            }
            if !any {
                list.spawn(theme::text(
                    "No equipment details received",
                    Size::Body,
                    theme::INK_DIM,
                ));
            }
        });
    });
    panel
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn scroll(
    wheel: Res<windows::pointer::Wheel>,
    mut bodies: Query<(&ComputedNode, &mut ScrollPosition), With<ScrollBody>>,
) {
    if let Some(entity) = wheel.surface
        && let Ok((node, mut position)) = bodies.get_mut(entity)
    {
        windows::scroll_by(&mut position, node, wheel.pixels);
    }
}

/// Renders a conservative STML subset as inert text. Unknown tags remain
/// literal. Recognized anchors keep their labels and never become controls.
fn stml_text(markup: &str) -> String {
    let mut text = String::new();
    let mut rest = markup;
    while !rest.is_empty() {
        if let Some((tag, next)) = rest.strip_prefix('<').and_then(|tail| tail.split_once('>'))
            && let Some(value) = tag_text(tag)
        {
            text.push_str(value);
            rest = next;
        } else if let Some((entity, next)) =
            rest.strip_prefix('&').and_then(|tail| tail.split_once(';'))
            && let Some(value) = entity_text(entity)
        {
            text.push(value);
            rest = next;
        } else {
            let ch = rest.chars().next().expect("nonempty STML");
            if !ch.is_control() || matches!(ch, '\n' | '\t') {
                text.push(ch);
            }
            rest = &rest[ch.len_utf8()..];
        }
    }
    text.trim().to_owned()
}

fn tag_text(tag: &str) -> Option<&'static str> {
    let name = tag.trim().trim_end_matches('/').split_whitespace().next()?;
    match name.to_ascii_lowercase().as_str() {
        "br" | "/p" | "/tr" | "/table" | "hr" => Some("\n"),
        "/td" | "/th" => Some("   "),
        "p" | "table" | "tr" | "td" | "th" | "c" | "/c" | "b" | "/b" | "i" | "/i" | "u" | "/u"
        | "a" | "/a" | "font" | "/font" | "center" | "/center" => Some(""),
        _ => None,
    }
}

fn entity_text(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use eq_client_core::inspection::{Equipment, Inspection};

    fn receive_self_inspection(app: &mut App) {
        let mut online = app.world_mut().resource_mut::<OnlineState>();
        let player = crate::online::testing::player(7);
        let name = player.name.clone();
        crate::online::testing::admit(&mut online, 5, player);
        crate::online::testing::news(
            &mut online,
            [eq_client_core::WorldEvent::PlayerInspection(
                PlayerInspection {
                    session_id: 5,
                    name,
                    report: Inspection {
                        recipient_id: 7,
                        player_id: 7,
                        text: "Synthetic inspection note".into(),
                        equipment: vec![
                            Equipment {
                                name: "Test charm".into(),
                                icon: None,
                            },
                            Equipment {
                                name: "Test earring".into(),
                                icon: Some(500),
                            },
                        ],
                    },
                },
            )],
        );
    }

    #[test]
    fn a_received_player_report_draws_its_contents_above_an_overlapping_item_window() {
        let mut app = crate::testing::app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .eq_directory = Some("synthetic-no-files".into());
        let icon = crate::sheets::testing::sheet(
            &mut app,
            "dragitem1.tga",
            image::RgbaImage::from_pixel(240, 240, image::Rgba([12, 34, 56, 255])),
        );
        // Match the production order. A report arriving in Present needs to
        // raise above an item window already stacked by Route.
        app.add_systems(
            Update,
            (windows::restack, present, windows::restack_popups).chain(),
        );
        let item = app
            .world_mut()
            .run_system_once(|mut commands: Commands| {
                windows::frame(&mut commands, WindowId::Item, Node::default())
            })
            .unwrap();
        let item_layout = app.world_mut().get_mut::<Node>(item).unwrap().into_inner();
        item_layout.left = px(0);
        item_layout.top = px(80);
        item_layout.margin = UiRect::ZERO;
        item_layout.width = px(397);
        item_layout.height = px(190);
        app.world_mut()
            .resource_mut::<Stack>()
            .raise(WindowId::Item);
        app.update();
        receive_self_inspection(&mut app);
        app.update();

        let world = app.world_mut();
        let body = world
            .query_filtered::<Entity, With<ScrollBody>>()
            .single(world)
            .unwrap();
        let panel = world.get::<ChildOf>(body).unwrap().parent();
        assert_eq!(
            world.get::<WindowId>(panel),
            Some(&WindowId::PlayerInspection)
        );
        let player = world.get::<Node>(panel).unwrap();
        let item_node = world.get::<Node>(item).unwrap();
        assert_eq!((player.left, player.top), (px(125), px(105)));
        assert_eq!((item_node.width, item_node.height), (px(397), px(190)));
        assert!(
            world.get::<GlobalZIndex>(panel).unwrap().0
                > world.get::<GlobalZIndex>(item).unwrap().0
        );
        assert!(
            world.get::<GlobalZIndex>(panel).unwrap().0
                < WindowId::ServerDialog.describe().layer.base()
        );
        let rows = world.get::<Children>(body).unwrap();
        let [note, first, second] = &rows[..] else {
            panic!("expected the note followed by two received rows");
        };
        assert_eq!(
            world.get::<Text>(*note).unwrap().0,
            "Synthetic inspection note"
        );
        let row_text = |row| {
            let children = world.get::<Children>(row).unwrap();
            children[..]
                .iter()
                .find_map(|child| world.get::<Text>(*child))
                .unwrap()
                .0
                .as_str()
        };
        assert_eq!(row_text(*first), "Charm  ·  Test charm");
        assert_eq!(row_text(*second), "Left ear  ·  Test earring");
        let mut images = world.query::<&ImageNode>();
        let images: Vec<_> = images.iter(world).collect();
        let [image] = images.as_slice() else {
            panic!("only the row with a received icon should draw one");
        };
        assert_eq!(image.image, icon);

        // Escape dismisses the front inspector and leaves the item window open.
        *world.resource_mut::<crate::escape::Escape>() =
            crate::escape::Escape::Close(WindowId::PlayerInspection);
        app.update();
        assert!(app.world().get_entity(panel).is_err());
        assert_ne!(
            app.world().get::<Node>(item).unwrap().display,
            Display::None
        );
    }

    #[test]
    fn a_new_item_selection_and_received_detail_each_raise_the_open_item_window() {
        use crate::items;
        let mut app = crate::testing::app();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        app.insert_resource(Outbox::new(Some(sender))).add_systems(
            Update,
            (
                windows::raise_opened,
                windows::restack,
                items::input,
                items::raise,
                present,
                windows::restack_popups,
                items::update,
            )
                .chain(),
        );
        app.world_mut()
            .run_system_once(|mut commands: Commands| items::spawn(&mut commands))
            .unwrap();
        app.world_mut()
            .resource_mut::<items::ItemState>()
            .open_received(crate::preview::items()[0].details.clone());
        app.update();
        receive_self_inspection(&mut app);
        app.update();

        let world = app.world_mut();
        let item = world
            .query_filtered::<Entity, With<items::ItemPanel>>()
            .single(world)
            .unwrap();
        let body = world
            .query_filtered::<Entity, With<ScrollBody>>()
            .single(world)
            .unwrap();
        let player = world.get::<ChildOf>(body).unwrap().parent();
        let z = |app: &App, entity| app.world().get::<GlobalZIndex>(entity).unwrap().0;
        assert!(z(&app, player) > z(&app, item));
        assert_ne!(
            app.world().get::<Node>(item).unwrap().display,
            Display::None
        );

        app.world_mut()
            .run_system_once(
                |mut state: ResMut<items::ItemState>,
                 online: Res<OnlineState>,
                 outbox: Res<Outbox>| {
                    let link = eq_client_core::ItemLink {
                        body: format!("00002A{}1234ABCD", "0".repeat(31)),
                        item_id: 42,
                        text: "New linked item".into(),
                        start: 0,
                        end: 0,
                        text_start: 0,
                        text_end: 15,
                    };
                    state.inspect_link(&link, &online, &outbox).unwrap();
                },
            )
            .unwrap();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            eq_client_core::ClientCommand::InspectItem { session_id: 5, .. }
        ));
        app.update();
        assert!(z(&app, item) > z(&app, player));

        app.world_mut()
            .resource_mut::<Stack>()
            .raise(WindowId::PlayerInspection);
        app.update();
        assert!(z(&app, player) > z(&app, item));
        let mut details = crate::preview::items()[0].details.clone();
        details.id = 42;
        details.name = "Received linked item".into();
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            [eq_client_core::WorldEvent::ItemDetails(details)],
        );
        app.update();
        assert!(z(&app, item) > z(&app, player));
        assert!(z(&app, item) < WindowId::ServerDialog.describe().layer.base());
        let world = app.world_mut();
        let mut text = world.query_filtered::<&Text, With<items::ItemText>>();
        assert!(
            text.single(world)
                .unwrap()
                .0
                .starts_with("Received linked item")
        );
    }

    #[test]
    fn stml_layout_keeps_words_and_never_activates_markup() {
        assert_eq!(
            stml_text(
                "<p>Hello<br/> <c \"#FF0000\">traveler</c></p><a href=\"game:do\">Visit</a> &lt;br&gt;<script>text</script>"
            ),
            "Hello\n traveler\nVisit <br><script>text</script>"
        );
        assert_eq!(stml_text("2 < 3 & unknown; <c"), "2 < 3 & unknown; <c");
    }
}
