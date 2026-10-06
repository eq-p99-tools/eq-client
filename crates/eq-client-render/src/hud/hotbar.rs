//! The hotbar's bindings, kept per character; activating a binding uses the
//! normal command validators.
use crate::theme::{self, Size};
use bevy::prelude::*;
use eq_client_core::{hotbar::Hotbar, qol::Fix};
use std::path::PathBuf;
pub(crate) mod carry;
mod item_art;
#[cfg(test)]
mod item_tests;
pub(crate) use carry::{Clicks, Pickable, Source};
pub(super) use eq_client_core::hotbar::Binding as Action;
pub(crate) use item_art::update as item_artwork;

#[derive(Resource)]
pub(crate) struct Bindings(pub(super) [Option<Action>; 10]);

impl Default for Bindings {
    fn default() -> Self {
        Self(Hotbar::default().0)
    }
}

/// What the hotbar's saving knows between frames.
#[derive(Default)]
pub(crate) struct Store {
    /// Whether the settings directory was taken yet.
    loaded: bool,
    directory: Option<PathBuf>,
    /// The world and character whose hotbar is in use; None before one enters.
    profile: Option<(String, String)>,
    /// The text last read or written for this profile, to skip unchanged writes.
    written: String,
}

/// Loads the hotbar of whoever is playing, and saves a change to their own
/// file. A character with no file of their own starts from the official
/// client's hotbuttons, read from their ini in the installation, or from
/// the defaults; before a character enters, the defaults stand.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn persist(
    settings: Res<crate::ViewerSettings>,
    profile: Res<crate::profile_files::Profile>,
    mut bindings: ResMut<Bindings>,
    mut store: Local<Store>,
) {
    if store.loaded && !profile.is_changed() {
        if bindings.is_changed() {
            save(&mut store, &bindings);
        }
        return;
    }
    if store.loaded {
        save(&mut store, &bindings);
    } else {
        store.directory.clone_from(&settings.0.settings_directory);
        store.loaded = true;
    }
    store.profile = profile.key();
    let Some((world, character)) = store.profile.clone() else {
        bindings.0 = Hotbar::default().0;
        store.written.clear();
        return;
    };
    let saved = store.directory.as_ref().and_then(|directory| {
        std::fs::read_to_string(directory.join(file_name(&world, &character))).ok()
    });
    let hotbar = saved.map_or_else(
        || {
            settings
                .0
                .official_settings()
                .and_then(|official| Hotbar::official(&official.hotbuttons(&character, &world)))
                .unwrap_or_default()
        },
        |text| Hotbar::read(&text, Hotbar::default()),
    );
    bindings.0 = hotbar.0;
    store.written = hotbar.text();
}

/// Writes the hotbar to the profile's file when it changed since last time.
fn save(store: &mut Store, bindings: &Bindings) {
    let (Some(directory), Some((world, character))) = (&store.directory, &store.profile) else {
        return;
    };
    let text = Hotbar(bindings.0).text();
    if text == store.written {
        return;
    }
    let path = directory.join(file_name(world, character));
    if let Err(error) = crate::profile_files::write(directory, &path, &text) {
        warn!("Could not save the hotbar to {}: {error}", path.display());
    }
    // A failed write waits for the next change rather than retrying each frame.
    store.written = text;
}

/// The character's own hotbar file.
fn file_name(world: &str, character: &str) -> String {
    let profile = (world.to_owned(), character.to_owned());
    crate::profile_files::name("hotbar", "hotbar.txt", Some(&profile))
}

impl Bindings {
    /// Resolves the current gem binding without retaining a previously memorized spell.
    pub(crate) fn gem(&self, slot: usize) -> Option<usize> {
        match self.0.get(slot).copied().flatten()? {
            Action::Gem(gem) => Some(usize::from(gem)),
            Action::Sit
            | Action::Stand
            | Action::Item { .. }
            | Action::Ability(_)
            | Action::Attack
            | Action::Camp
            | Action::Invite
            | Action::Follow
            | Action::Disband => None,
        }
    }
}

#[derive(Component)]
pub(crate) struct Slot(pub usize);
#[derive(Component)]
pub(crate) struct Caption(pub usize);
#[derive(Component)]
pub(crate) struct Hint;

/// Builds ten bound action buttons in the requested five-by-two layout.
pub(super) fn spawn(commands: &mut Commands, root: Entity) {
    let frame = super::panel(commands, root, 244.0);
    crate::windows::titled(commands, frame, crate::windows::WindowId::Actions);
    for (row_index, keys) in [["1", "2", "3", "4", "5"], ["6", "7", "8", "9", "0"]]
        .into_iter()
        .enumerate()
    {
        let row = super::row(commands, frame, 4.0);
        for (column, key) in keys.into_iter().enumerate() {
            let index = row_index * 5 + column;
            let button = super::slot(commands, row, key, 40.0, true);
            commands
                .entity(button)
                .insert((
                    Button,
                    Slot(index),
                    Pickable(Source::Slot(index)),
                    crate::outbox::Needs::Nothing,
                ))
                .with_children(|button| contents(button, index));
        }
    }
    let hint = super::label(commands, frame, "", Size::Caption, theme::INK);
    commands.entity(hint).insert((
        Hint,
        Node {
            height: px(52),
            overflow: Overflow::clip(),
            ..default()
        },
    ));
}

/// What a slot's button shows of what it holds: the spell's icon, the
/// item's picture and a caption, in the client's own window and in the
/// skin's Hot Button window alike.
pub(crate) fn contents(button: &mut ChildSpawnerCommands, index: usize) {
    button.spawn(crate::spell_icons::artwork(
        crate::spell_icons::Source::Action(index),
        30.0,
    ));
    button.spawn(item_art::artwork(index));
    button.spawn((
        Caption(index),
        theme::text("", Size::Caption, theme::INK),
        Node {
            position_type: PositionType::Absolute,
            bottom: px(1),
            right: px(2),
            ..default()
        },
        bevy::ui::FocusPolicy::Pass,
    ));
}

/// The first of the ten slots whose action the keys freshly pressed.
fn slot_pressed(keys: &crate::keys::Keys, act: fn(u8) -> crate::keys::Act) -> Option<usize> {
    (0..10u8)
        .find(|slot| keys.pressed(act(*slot)))
        .map(usize::from)
}

/// Resolves a slot's key or a click on its button into a typed action.
pub(super) fn requested(
    keys: &crate::keys::Keys,
    bindings: &Bindings,
    clicks: &Clicks<Slot>,
) -> Option<Action> {
    let slot = clicks
        .iter()
        .next()
        .map(|slot| slot.0)
        .or_else(|| slot_pressed(keys, crate::keys::Act::Slot))?;
    bindings.0.get(slot).copied().flatten()
}

/// The slash command a binding runs, as its Actions window button runs it:
/// the one table for both. None for a kind that sends its own request (a
/// gem's spell, an item, an ability, melee attack).
pub(crate) const fn command(action: Action) -> Option<&'static str> {
    match action {
        Action::Sit => Some("/sit"),
        Action::Stand => Some("/stand"),
        Action::Camp => Some("/camp"),
        Action::Invite => Some("/invite"),
        Action::Follow => Some("/follow"),
        Action::Disband => Some("/disband"),
        Action::Gem(_) | Action::Item { .. } | Action::Ability(_) | Action::Attack => None,
    }
}

/// Ctrl+number binds what the control under the pointer gives the action
/// bar, as a hold would pick it up ([`Source::binding`]); Ctrl+Shift+number
/// empties the slot.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn update(
    keys: crate::keys::Keys,
    controls: Query<(&Interaction, &Pickable)>,
    online: Res<crate::online::OnlineState>,
    mut bindings: ResMut<Bindings>,
) {
    if let Some(index) = slot_pressed(&keys, crate::keys::Act::ClearSlot) {
        bindings.0[index] = None;
    } else if let Some(index) = slot_pressed(&keys, crate::keys::Act::BindSlot)
        && let Some(binding) = controls
            .iter()
            .filter(|(interaction, _)| **interaction != Interaction::None)
            .find_map(|(_, pickable)| pickable.0.binding(online.world(), &bindings))
    {
        bindings.0[index] = Some(binding);
    }
}

/// A slot's button, as the client draws it or as the skin does: the
/// client's own shows readiness in its fill, the skin's in its disabled
/// look.
type SlotButtons<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Interaction,
        &'static Slot,
        Option<&'static mut BackgroundColor>,
        Has<crate::skinned::SkinButton>,
        Has<crate::skinned::Greyed>,
    ),
>;

/// Presents current bindings, spell identity and cooldowns independently of keyboard focus.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn presentation(
    mut commands: Commands,
    bindings: Res<Bindings>,
    (online, options): (
        Res<crate::online::OnlineState>,
        Res<crate::options::OptionsState>,
    ),
    (names, map): (Res<crate::spellbook::SpellNames>, Res<crate::keys::KeyMap>),
    mut slots: SlotButtons,
    mut labels: Query<(&mut Text, Option<&Caption>, Option<&Hint>)>,
) {
    let now = std::time::Instant::now();
    let inventory = online.world().inventory();
    let qol = &options.options.qol;
    let hovered = slots
        .iter()
        .find(|(_, interaction, ..)| **interaction != Interaction::None)
        .map(|(_, _, slot, ..)| slot.0);
    for (entity, interaction, slot, color, skinned, greyed) in &mut slots {
        let spell = bindings.gem(slot.0).and_then(|gem| online.world().gem(gem));
        let missing_item = match bindings.0[slot.0] {
            Some(Action::Item { slot, id }) => bound_item(inventory, (slot, id), qol).is_none(),
            _ => false,
        };
        let empty = missing_item
            || bindings.0[slot.0].is_none()
            || (bindings.gem(slot.0).is_some() && spell.is_none());
        let ability_waits = match bindings.0[slot.0] {
            Some(Action::Ability(ability)) => online.world().ability_wait(ability, now).is_some(),
            _ => false,
        };
        let waiting = ability_waits
            || spell.is_some_and(|id| {
                online.world().casting().pending.is_some()
                    || online.world().casting().cast.is_some()
                    || !online
                        .world()
                        .casting()
                        .cooldowns
                        .remaining(id, now)
                        .is_zero()
            });
        if skinned {
            // The skin's button lights itself under the pointer; it shows a
            // running timer in its disabled look.
            if waiting && !greyed {
                commands.entity(entity).insert(crate::skinned::Greyed);
            } else if !waiting && greyed {
                commands.entity(entity).remove::<crate::skinned::Greyed>();
            }
        } else if let Some(mut color) = color {
            color.0 = theme::readiness(*interaction != Interaction::None, empty, waiting);
        }
    }
    for (mut text, caption, hint) in &mut labels {
        if let Some(caption) = caption {
            text.0 = match bindings.0[caption.0] {
                Some(Action::Gem(gem)) => {
                    if online.world().gem(usize::from(gem)).is_some() {
                        format!("G{}", gem + 1)
                    } else {
                        "-".into()
                    }
                }
                Some(Action::Sit) => "Sit".into(),
                Some(Action::Stand) => "Stand".into(),
                Some(Action::Item { .. }) => "Item".into(),
                Some(Action::Ability(ability)) => ability.name().into(),
                Some(Action::Attack) => "Attack".into(),
                Some(Action::Camp) => "Camp".into(),
                Some(Action::Invite) => "Invite".into(),
                Some(Action::Follow) => "Follow".into(),
                Some(Action::Disband) => "Disband".into(),
                None => "-".into(),
            };
        }
        if hint.is_some() {
            text.0 = hovered_detail(
                &bindings,
                hovered,
                (online.world(), qol),
                (&names, &map),
                now,
            );
        }
    }
}

/// What the hovered slot holds, for the window's detail line.
fn hovered_detail(
    bindings: &Bindings,
    hovered: Option<usize>,
    (world, qol): (
        &eq_client_core::world::ClientWorld,
        &eq_client_core::qol::Settings,
    ),
    (names, map): (&crate::spellbook::SpellNames, &crate::keys::KeyMap),
    now: std::time::Instant,
) -> String {
    let inventory = world.inventory();
    match hovered.and_then(|index| bindings.0[index]) {
        Some(Action::Gem(gem)) => match world.gem(usize::from(gem)) {
            Some(spell) => {
                let status = if world.casting().pending.is_some() {
                    "Awaiting cast acknowledgement".into()
                } else if world.casting().cast.is_some() {
                    "Casting".into()
                } else {
                    let remaining = world.casting().cooldowns.remaining(spell, now);
                    if remaining.is_zero() {
                        "Uses current target".into()
                    } else {
                        format!("Available in {:.1}s", remaining.as_secs_f32())
                    }
                };
                format!("{}\n{}\n{status}", names.label(spell), names.details(spell))
            }
            None => super::empty_gem(gem, map),
        },
        Some(Action::Sit) => "Sit down".into(),
        Some(Action::Stand) => "Stand up".into(),
        Some(Action::Attack) => "Melee attack\nTurns auto attack on or off".into(),
        Some(Action::Camp) => "Camp\nSits down and leaves the world".into(),
        Some(Action::Invite) => "Invite\nInvites the target into your group".into(),
        Some(Action::Follow) => "Follow\nJoins the group you were invited to".into(),
        Some(Action::Disband) => "Disband\nLeaves your group, or declines an invitation".into(),
        Some(Action::Ability(ability)) => match world.ability_wait(ability, now) {
            Some(wait) => format!(
                "{}\nAvailable in {:.0}s",
                ability.name(),
                wait.as_secs_f32().ceil()
            ),
            None => format!("{}\nReady", ability.name()),
        },
        Some(Action::Item { slot, id }) => bound_item(inventory, (slot, id), qol).map_or_else(
            || {
                format!(
                    "Bound item unavailable\n{}; rebind after moving it",
                    slot.label()
                )
            },
            |item| {
                format!(
                    "{}\n{}; uses the current target or yourself",
                    item.details.name,
                    slot.label()
                )
            },
        ),
        None if hovered.is_some() => "Unassigned".into(),
        None => String::new(),
    }
}

/// The item a slot binding uses: what its place holds, if that has a click
/// effect, and never a different item later put in that place
/// ([`Fix::HotbarItemGuard`]).
fn bound_item<'a>(
    inventory: &'a eq_client_core::inventory::Inventory,
    (slot, id): (eq_client_core::inventory::InventorySlot, u32),
    qol: &eq_client_core::qol::Settings,
) -> Option<&'a eq_client_core::inventory::InventoryItem> {
    let guards = qol.on(Fix::HotbarItemGuard);
    inventory
        .items()
        .get(&slot)
        .filter(|item| (!guards || item.details.id == id) && item.activation.effect.is_some())
}

/// What a slot needs of the session for what it holds, as the command it
/// sends needs it (`ClientCommand::capability`): sitting and standing are
/// moves, an ability is one the server type lists, a gem's spell or an
/// item's click effect is a cast, melee attack is combat, camping is
/// camping, and inviting, following and disbanding are grouping. An empty
/// slot needs nothing.
pub(crate) fn need(action: Option<Action>) -> crate::outbox::Needs {
    use crate::outbox::Needs;
    use eq_client_core::Capability;
    match action {
        None => Needs::Nothing,
        Some(Action::Sit | Action::Stand) => Needs::Capability(Capability::Moving),
        Some(Action::Ability(ability)) => Needs::Ability(ability),
        Some(Action::Gem(_) | Action::Item { .. }) => Needs::Capability(Capability::Casting),
        Some(Action::Attack) => Needs::Capability(Capability::Combat),
        Some(Action::Camp) => Needs::Capability(Capability::Camping),
        Some(Action::Invite | Action::Follow | Action::Disband) => {
            Needs::Capability(Capability::Grouping)
        }
    }
}

/// Keeps what each slot needs of the session in step with its binding, in
/// the client's own bar and in the skin's alike ([`need`]).
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn needs(bindings: Res<Bindings>, mut slots: Query<(&Slot, &mut crate::outbox::Needs)>) {
    for (slot, mut needs) in &mut slots {
        let wanted = need(bindings.0[slot.0]);
        if *needs != wanted {
            *needs = wanted;
        }
    }
}

/// Item shortcuts share inventory validation, request IDs, cursor rules and worker feedback.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(crate) fn item_actions(
    keys: crate::keys::Keys,
    bindings: Res<Bindings>,
    clicks: Clicks<Slot>,
    (online, options): (
        Res<crate::online::OnlineState>,
        Res<crate::options::OptionsState>,
    ),
    sender: Res<crate::outbox::Outbox>,
    mut chat: ResMut<crate::chat::ChatState>,
    mut inventory: ResMut<crate::inventory::InventoryState>,
) {
    if !keys.focused() {
        return;
    }
    let Some(Action::Item { slot, id }) = requested(&keys, &bindings, &clicks) else {
        return;
    };
    // The inventory says its own refusals.
    if bound_item(online.world().inventory(), (slot, id), &options.options.qol).is_none() {
        chat.refuse("Bound item unavailable; rebind after moving it");
    } else {
        inventory.activate_shortcut(
            slot,
            &online,
            &sender,
            online.world().target().selected,
            online.world().casting().cast.is_some() || online.world().casting().pending.is_some(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_needs_what_its_binding_sends_and_an_empty_one_nothing() {
        use crate::outbox::Needs;
        use eq_client_core::{Capability, ClientCommand, Posture};
        assert_eq!(need(None), Needs::Nothing);
        for action in [Action::Sit, Action::Stand] {
            assert_eq!(need(Some(action)), Needs::Capability(Capability::Moving));
        }
        // Sitting and standing go as a posture, which the session takes with
        // moving.
        let posture = ClientCommand::SetPosture {
            session_id: 1,
            spawn_id: 1,
            posture: Posture::Sitting,
            created: std::time::Instant::now(),
        };
        assert_eq!(posture.capability(), Some(Capability::Moving));
        let item = Action::Item {
            slot: eq_client_core::inventory::InventorySlot(22),
            id: 9,
        };
        for action in [Action::Gem(4), item] {
            assert_eq!(need(Some(action)), Needs::Capability(Capability::Casting));
        }
        let bash = eq_client_core::abilities::Ability::Bash;
        assert_eq!(need(Some(Action::Ability(bash))), Needs::Ability(bash));
    }

    #[test]
    fn the_actions_windows_kinds_need_what_their_commands_need() {
        use crate::outbox::Needs;
        use eq_client_core::ClientCommand;
        let created = std::time::Instant::now();
        let sent = [
            (
                Action::Attack,
                ClientCommand::AutoAttack {
                    session_id: 1,
                    enabled: true,
                    created,
                },
            ),
            (
                Action::Camp,
                ClientCommand::Camp {
                    session_id: 1,
                    created,
                },
            ),
            (
                Action::Invite,
                ClientCommand::InviteToGroup {
                    session_id: 1,
                    name: "Example".into(),
                },
            ),
            (Action::Follow, ClientCommand::FollowGroup { session_id: 1 }),
            (Action::Disband, ClientCommand::Disband { session_id: 1 }),
        ];
        for (action, command) in sent {
            let capability = command.capability().unwrap();
            assert_eq!(
                need(Some(action)),
                Needs::Capability(capability),
                "{action:?}"
            );
        }
    }

    #[test]
    fn hover_follows_current_spell_and_distinguishes_empty_from_unassigned() {
        let mut fields = vec!["0"; 145];
        fields[0] = "73";
        fields[1] = "Synthetic spell";
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
        app.init_resource::<Bindings>()
            .init_resource::<crate::options::OptionsState>()
            .insert_resource(online)
            .insert_resource(crate::spellbook::SpellNames::parse(&fields.join("^")))
            .add_systems(Update, presentation);
        app.world_mut()
            .spawn((Slot(0), Interaction::Hovered, BackgroundColor::default()));
        let hint = app.world_mut().spawn((Hint, Text::default())).id();
        app.update();
        assert!(
            app.world()
                .get::<Text>(hint)
                .unwrap()
                .0
                .contains("Gem 1 is empty")
        );
        crate::online::testing::spell(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            eq_client_core::SpellUpdate::Slot {
                slot: 0,
                spell_id: 73,
                mode: 1,
            },
        );
        app.update();
        assert!(
            app.world()
                .get::<Text>(hint)
                .unwrap()
                .0
                .contains("Synthetic spell")
        );
        crate::online::testing::pending_cast(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            Some(73),
        );
        app.update();
        assert!(
            app.world()
                .get::<Text>(hint)
                .unwrap()
                .0
                .contains("Awaiting cast acknowledgement")
        );
        app.world_mut().resource_mut::<Bindings>().0[0] = Some(Action::Gem(1));
        app.update();
        assert_eq!(app.world().resource::<Bindings>().gem(0), Some(1));
        assert!(
            app.world()
                .get::<Text>(hint)
                .unwrap()
                .0
                .contains("Gem 2 is empty")
        );
        app.world_mut().resource_mut::<Bindings>().0[0] = None;
        app.update();
        assert!(
            app.world()
                .get::<Text>(hint)
                .unwrap()
                .0
                .starts_with("Unassigned")
        );
    }
    #[test]
    fn binding_and_clearing_requires_focused_non_chat_input() {
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
        crate::online::testing::spell(
            &mut online,
            eq_client_core::SpellUpdate::Slot {
                slot: 4,
                spell_id: 73,
                mode: 1,
            },
        );
        app.init_resource::<Bindings>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::chat::ChatState>()
            .insert_resource(online)
            .add_systems(Update, update);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    ..default()
                },
                bevy::window::PrimaryWindow,
            ))
            .id();
        let gem = app
            .world_mut()
            .spawn((Interaction::Hovered, Pickable(Source::Gem(4))))
            .id();
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ControlLeft);
            keys.press(KeyCode::Digit1);
        }
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().0[0],
            Some(Action::Gem(4))
        );
        // An Actions window button binds what it does; an empty gem binds
        // nothing, as a hold picks nothing up from it.
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::None;
        let attack = app
            .world_mut()
            .spawn((
                Interaction::Hovered,
                Pickable(Source::Fixed(Action::Attack)),
            ))
            .id();
        // Only the digit pressed last is fresh, with Ctrl still held.
        let press = |app: &mut App, digit| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            for key in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3] {
                keys.release(key);
            }
            keys.clear();
            keys.press(digit);
        };
        press(&mut app, KeyCode::Digit2);
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().0[1],
            Some(Action::Attack)
        );
        app.world_mut().entity_mut(attack).despawn();
        app.world_mut()
            .spawn((Interaction::Hovered, Pickable(Source::Gem(5))));
        press(&mut app, KeyCode::Digit3);
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().0[2],
            Some(Action::Gem(2))
        );
        press(&mut app, KeyCode::Digit1);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ShiftLeft);
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().0[0],
            Some(Action::Gem(4))
        );
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = false;
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        assert_eq!(
            app.world().resource::<Bindings>().0[0],
            Some(Action::Gem(4))
        );
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        assert_eq!(app.world().resource::<Bindings>().0[0], None);
    }
}

#[cfg(test)]
mod persist_tests {
    use super::*;
    use crate::online::{OnlineState, testing};

    fn enter(app: &mut App, name: &str) {
        let mut online = OnlineState::new(true);
        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::WorldName {
                short_name: "ExampleWorld".into(),
            }],
        );
        let mut player = testing::player(7);
        player.name = name.into();
        testing::admit(&mut online, 1, player);
        app.insert_resource(online);
    }

    #[test]
    fn each_character_keeps_their_own_hotbar() {
        let directory = std::env::temp_dir().join(format!(
            "eq-client-hotbar-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        let mut app = crate::testing::app();
        app.world_mut()
            .resource_mut::<crate::ViewerSettings>()
            .0
            .settings_directory = Some(directory.clone());
        app.init_resource::<Bindings>()
            .add_systems(Update, (crate::profile_files::follow, persist).chain());
        enter(&mut app, "Example");
        app.update();
        // Nothing is written until the player changes something.
        assert!(std::fs::read_dir(&directory).is_err());
        app.world_mut().resource_mut::<Bindings>().0[2] = None;
        app.update();
        let file = directory.join("hotbar-ExampleWorld-Example.txt");
        assert!(
            std::fs::read_to_string(&file)
                .unwrap()
                .contains("slot_3 = none")
        );
        // Another character starts from the defaults, and the first comes
        // back as they left it.
        enter(&mut app, "Another");
        app.update();
        assert_eq!(app.world().resource::<Bindings>().0, Hotbar::default().0);
        enter(&mut app, "Example");
        app.update();
        assert_eq!(app.world().resource::<Bindings>().0[2], None);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
