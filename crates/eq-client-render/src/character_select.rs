//! Pre-zone entry uses server slots; deletion captures an explicitly named identity.
//! Occupied slots select a character; available empty slots start creation
//! in the installed skin's list or the client's fallback list.
use super::{online::OnlineState, outbox::Outbox, windows::WindowId};
use crate::theme::{self, Size};
use bevy::prelude::*;
use eq_client_core::{CharacterChoice, ClientCommand};

pub(super) struct Selection {
    id: u64,
    connection_id: u64,
    deletion: Option<eq_client_core::deletion::Rules>,
    held: bool,
    entries: Vec<CharacterChoice>,
    selected: Option<u8>,
    submitted: bool,
    message: String,
    creation: Option<eq_client_core::creation::Rules>,
}

impl Selection {
    /// A list as it arrives, with its first character highlighted, so the
    /// screen shows a character at once (inferred, 6.255).
    pub fn new(id: u64, entries: Vec<CharacterChoice>) -> Self {
        Self {
            id,
            connection_id: 0,
            deletion: None,
            held: false,
            selected: entries.first().map(|entry| entry.slot),
            entries,
            submitted: false,
            message: String::new(),
            creation: None,
        }
    }

    /// Identity the server list was published with.
    pub(super) const fn id(&self) -> u64 {
        self.id
    }

    pub(super) fn with_creation(
        mut self,
        creation: Option<eq_client_core::creation::Rules>,
    ) -> Self {
        self.creation = creation;
        self
    }

    pub(super) fn with_deletion(
        mut self,
        scope: eq_client_core::deletion::Scope,
        rules: Option<eq_client_core::deletion::Rules>,
    ) -> Self {
        self.connection_id = scope.connection_id;
        self.deletion = rules;
        self
    }

    pub(super) const fn scope(&self) -> eq_client_core::deletion::Scope {
        eq_client_core::deletion::Scope {
            connection_id: self.connection_id,
            selection_id: self.id,
        }
    }

    pub(super) fn choose_exact(&mut self, name: &str) -> bool {
        if self.submitted || self.held {
            return false;
        }
        let mut entries = self.entries.iter().filter(|entry| entry.name == name);
        let Some(entry) = entries.next() else {
            return false;
        };
        if entries.next().is_some() {
            return false;
        }
        self.selected = Some(entry.slot);
        true
    }

    pub(super) fn can_create(&self) -> Option<eq_client_core::creation::Rules> {
        self.creation
            .filter(|rules| !self.submitted && !self.held && self.entries.len() < rules.slots())
    }

    /// Whether this empty slot can start a character draft under the list's policy.
    fn can_create_at(&self, slot: u8) -> bool {
        self.entry(slot).is_none()
            && self
                .can_create()
                .is_some_and(|rules| usize::from(slot) < rules.slots())
    }

    /// Whether every slot the list's policy offers is occupied.
    fn full(&self) -> bool {
        self.creation
            .is_some_and(|rules| self.entries.len() >= rules.slots())
    }

    pub(super) fn created(&mut self, name: &str) {
        self.choose_named(name);
        self.message = format!("{name} was created. Select Enter World when ready.");
    }

    /// The slot of the character chosen, if any.
    pub(crate) const fn chosen(&self) -> Option<u8> {
        self.selected
    }

    /// The listed character in a slot, if any.
    pub(crate) fn entry(&self, slot: u8) -> Option<&CharacterChoice> {
        self.entries.iter().find(|entry| entry.slot == slot)
    }

    /// The long name of the zone the highlighted character is in; None when
    /// the server's list carries no zone for it (eq-network keeps a list's
    /// zones only where the server type's list is checked to carry them) or
    /// our zone table has no long name for its number. The number is taken
    /// to its low 16 bits first, and the short name is never shown, as the
    /// official client names a zone (from the program, 0x5fa1c0).
    pub(crate) fn chosen_zone(&self) -> Option<&'static str> {
        let zone = self.entry(self.selected?)?.zone_id? & 0xffff;
        eq_client_core::zones::long_name(eq_client_core::zones::short_name(zone)?)
            .filter(|long| !long.is_empty())
    }

    /// Whether Enter World has been asked for with this list.
    pub(crate) const fn entering(&self) -> bool {
        self.submitted
    }

    /// Highlights a listed character by exact server spelling, ignoring case.
    pub(super) fn choose_named(&mut self, name: &str) -> bool {
        if self.held {
            return false;
        }
        let slot = self
            .entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .map(|entry| entry.slot);
        if slot.is_some() && !self.submitted {
            self.selected = slot;
        }
        slot.is_some()
    }

    /// Shows what the session's news says of this list on its guidance
    /// line, where the player looks: a refused character creation, in the
    /// status line's words, until the next key or click on the list or the
    /// world's next answer (inferred, 6.278).
    pub(super) fn hear(&mut self, notice: &eq_client_core::world::Notice) {
        if let eq_client_core::world::Notice::CreationRefused { name, outcome } = notice {
            self.message = crate::notices::creation_refusal(name, *outcome);
        }
    }

    /// Asks once; a refused request leaves the choice available for another
    /// click, and the outbox says why.
    fn enter(&mut self, outbox: &Outbox, world: &eq_client_core::world::ClientWorld) {
        if self.submitted || self.held {
            return;
        }
        let Some(slot) = self.selected else {
            return;
        };
        if !self.entries.iter().any(|entry| entry.slot == slot) {
            return;
        }
        let command = ClientCommand::SelectCharacter {
            selection_id: self.id,
            slot,
        };
        match outbox.send(world, command) {
            Ok(()) => {
                self.submitted = true;
                self.message = "Entering world...".into();
            }
            Err(refusal) => self.message = refusal.text().into(),
        }
    }
}

#[cfg(test)]
impl Selection {
    /// This list with Enter World asked for.
    pub(crate) const fn asked_to_enter(mut self) -> Self {
        self.submitted = true;
        self
    }
}

#[derive(Component)]
pub(super) struct Root;
#[derive(Component)]
pub(super) enum Action {
    Choose(u8),
    Enter,
    Delete,
    /// The skin's Quit: back to the login screen, or out of the game in a
    /// run without one (`login::quit`).
    Quit,
}

/// The words on a skin's character button: the name of the character in its
/// slot, in the skin's colour, or the skin's own words for an empty slot.
#[derive(Component)]
pub(crate) struct SlotName {
    pub(crate) slot: u8,
    pub(crate) empty: String,
    pub(crate) ink: Color,
}

/// Applies selection input, shows the skin's list while the character list
/// is up, and rebuilds the cover and the client's own panel only when their
/// state changes.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
#[allow(
    clippy::too_many_lines,
    reason = "one selection frame orders confirmation, selection input and panel refresh"
)]
pub(super) fn update(
    mut commands: Commands,
    mut online: ResMut<OnlineState>,
    mut editor: ResMut<crate::character_create::Editor>,
    mut asked: ResMut<crate::confirm::Asked>,
    lines: Res<crate::notices::Lines>,
    outbox: Res<Outbox>,
    keys: crate::keys::Keys,
    navigation: Res<super::navigation::NavigationKeys>,
    buttons: crate::clicks::Clicks<Action>,
    roots: Query<Entity, With<Root>>,
    mut previous: Local<String>,
    (mut shown, skinned, front, screen): (
        ResMut<super::windows::Shown>,
        Res<crate::skinned::Skinned>,
        Res<crate::login::FrontEnd>,
        Res<crate::loading::Screen>,
    ),
    (preview, messages): (
        Res<crate::select_preview::Preview>,
        Res<crate::hud::messages::Messages>,
    ),
) {
    let focused = keys.focused();
    let keys = navigation.sample(&keys.input);
    // The world's characters, or the preview's, until a character is in.
    let visible = front.screen(&online) == crate::login::Screen::Characters && !editor.active();
    if visible != shown.is_open(WindowId::CharacterSelect) {
        if visible {
            shown.open(WindowId::CharacterSelect);
        } else {
            shown.close(WindowId::CharacterSelect);
        }
    }
    // Invalidate only this owner's stale dialog; other confirmation questions
    // keep their own answer lifecycle.
    if let Some(question @ crate::confirm::Question::DeleteCharacter { .. }) = asked.question()
        && (!visible
            || online
                .deletion
                .prompt()
                .map(crate::character_delete::question)
                != Some(question))
    {
        asked.withdraw();
    }
    if let Some(request) = online.deletion.prompt().cloned() {
        if !visible {
            online.deletion.cancel(&request);
        } else if let Some(yes) = asked.answer(crate::character_delete::question(&request)) {
            if yes {
                if let Err(error) =
                    online.submit_character_deletion(&request, request.name(), &outbox)
                {
                    online.deletion.cancel(&request);
                    if let Some(selection) = online.selection.as_mut() {
                        selection.message = error;
                    }
                }
            } else {
                online.deletion.cancel(&request);
                if let Some(selection) = online.selection.as_mut() {
                    selection.message.clear();
                }
            }
        }
    }
    let blocked = online.deletion.blocked();
    let deletion_message = online.deletion.message();
    if let Some(selection) = online.selection.as_mut() {
        selection.held = blocked;
        if let Some(message) = deletion_message {
            selection.message = message;
        }
    }
    let mut ask_delete = false;
    let (choosing, world) = online.choosing();
    if visible
        && focused
        && let Some(selection) = choosing
        && !selection.submitted
        && !blocked
    {
        for action in buttons.iter() {
            if editor.active() {
                break;
            }
            match action {
                // An empty slot starts a draft when the session offers creation.
                Action::Choose(slot) => {
                    selection.message.clear();
                    if selection.entries.iter().any(|entry| entry.slot == *slot) {
                        selection.selected = Some(*slot);
                    } else if selection.can_create_at(*slot) {
                        editor.open(selection, world);
                    }
                }
                Action::Enter => selection.enter(&outbox, world),
                Action::Delete => {
                    ask_delete = selection.deletion.is_some() && selection.selected.is_some();
                }
                Action::Quit => (),
            }
        }
        if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::ArrowUp) {
            selection.message.clear();
            let index = selection
                .entries
                .iter()
                .position(|entry| Some(entry.slot) == selection.selected);
            let count = selection.entries.len();
            if count != 0 {
                let next = index.map_or(0, |index| {
                    if keys.just_pressed(KeyCode::ArrowUp) {
                        (index + count - 1) % count
                    } else {
                        (index + 1) % count
                    }
                });
                selection.selected = Some(selection.entries[next].slot);
            }
        }
        // The Delete key as a shortcut to the confirmation is this client's
        // own (inferred, recording checklist 6.383).
        if keys.just_pressed(KeyCode::Delete) && selection.deletion.is_some() {
            ask_delete = true;
        }
        if keys.just_pressed(KeyCode::KeyC) {
            editor.open(selection, world);
        }
        if !editor.active() && keys.just_pressed(KeyCode::Enter) {
            selection.enter(&outbox, world);
        }
    }
    if ask_delete && !editor.active() {
        match online.prepare_character_deletion() {
            Ok(request) => {
                if !asked.ask(crate::character_delete::question(&request)) {
                    online.deletion.cancel(&request);
                }
            }
            Err(error) => {
                if let Some(selection) = online.selection.as_mut() {
                    selection.message = error;
                }
            }
        }
    }
    let status = lines.status.text();
    let fallback_confirmation = !skinned.has(WindowId::Confirmation);
    let skinned = skinned.has(WindowId::CharacterSelect);
    let showing = preview.showing();
    let signature = format!(
        "{visible}:{skinned}:{fallback_confirmation}:{showing}:{status}:{:?}",
        online.selection.as_ref().map(|s| (
            s.id,
            s.selected,
            s.submitted,
            &s.message,
            s.creation,
            s.deletion,
            s.held,
            s.entries.len()
        ))
    );
    if *previous == signature {
        return;
    }
    *previous = signature;
    for root in &roots {
        commands.entity(root).despawn();
    }
    if !visible {
        return;
    }
    if fallback_confirmation && let Some(request) = online.deletion.prompt() {
        deletion_confirmation(&mut commands, request);
    }
    // Once Enter World is asked for, the loading screen covers the list
    // until the zone admits the character and the zone's own cover takes
    // over, so the two read as one (inferred, 6.222). It also covers the
    // screen while the world has sent no list, as after a camp, while a new
    // world connection logs in and the server lists the characters again;
    // the list kept from before is never shown, being one the server has not
    // sent (inferred, 6.331).
    if online.selection.as_ref().is_none_or(Selection::entering) {
        crate::loading::cover(&mut commands, &screen, Root, Display::Flex);
    } else {
        let line = character_line(online.selection.as_ref(), &messages);
        spawn(
            &mut commands,
            online.selection.as_ref(),
            (status, line.as_deref()),
            (skinned, showing),
        );
    }
}

/// Writes each character's name on the skin's character buttons, and the
/// skin's own words on empty slots, dimmed only when creation is unavailable.
/// Hovering names the character's level or explains the empty slot's action.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn names(
    online: Res<OnlineState>,
    mut words: Query<(&SlotName, &mut Text, &mut TextColor)>,
    mut tips: Query<(&Action, &mut crate::tooltip::Tooltip)>,
) {
    let entries = online
        .selection
        .as_ref()
        .map_or(&[][..], |selection| &selection.entries[..]);
    let entry = |slot: u8| entries.iter().find(|entry| entry.slot == slot);
    for (name, mut text, mut color) in &mut words {
        let empty_ink = if online
            .selection
            .as_ref()
            .is_some_and(|selection| selection.can_create_at(name.slot))
        {
            name.ink
        } else {
            theme::INK_DIM
        };
        let (wanted, ink) = entry(name.slot).map_or((name.empty.as_str(), empty_ink), |entry| {
            (entry.name.as_str(), name.ink)
        });
        if text.0 != wanted {
            wanted.clone_into(&mut text.0);
        }
        if color.0 != ink {
            color.0 = ink;
        }
    }
    for (action, mut tip) in &mut tips {
        let Action::Choose(slot) = action else {
            continue;
        };
        let wanted = match (&online.selection, entry(*slot)) {
            (_, Some(entry)) => entry
                .level
                .map_or_else(String::new, |level| format!("Level {level}")),
            (Some(selection), None) if selection.can_create_at(*slot) => {
                "Create a character".to_owned()
            }
            (Some(_), None) => "No character available in this slot".to_owned(),
            (None, None) => String::new(),
        };
        if tip.0 != wanted {
            tip.0 = wanted;
        }
    }
}

/// What the list says under it: the session's state, its messages, or how
/// to use it.
fn guidance<'a>(selection: Option<&'a Selection>, status: &'a str) -> &'a str {
    match selection {
        None => status,
        Some(selection) if !selection.message.is_empty() => &selection.message,
        Some(selection) if selection.entries.is_empty() => {
            if selection.can_create().is_some() {
                "Choose an empty character slot to create a character."
            } else {
                "No characters available on this server."
            }
        }
        Some(_) => "Select a character | Up/Down: browse | Enter: connect",
    }
}

/// The official client's string for a character's line in explore mode:
/// its name, level, class and zone (from the program, 0x5000ba).
const CHARACTER_LINE: u32 = 6681;

/// The highlighted character's line under the list. The installed client's
/// words for it in explore mode, which the official client shows only there
/// (from the program; under the list is this client's place for it, 6.109),
/// with the name, the level, the class's name and the zone's long name in
/// that order, when all four are known; the zone's long name alone when the
/// level or the class is unknown or the installation lacks the string; and
/// nothing when the zone is unknown.
fn character_line(
    selection: Option<&Selection>,
    messages: &crate::hud::messages::Messages,
) -> Option<String> {
    let selection = selection?;
    let entry = selection.entry(selection.chosen()?)?;
    let zone = selection.chosen_zone()?;
    let class = entry.look.and_then(|look| {
        let id = eq_client_core::classes::select_name_string(look.class);
        let name = messages.argument_free(id);
        if name.is_none() {
            debug!(
                "The installed string table lacks string {id} for class {}",
                look.class
            );
        }
        name
    });
    let (Some(level), Some(class)) = (entry.level, class) else {
        return Some(zone.to_owned());
    };
    let arguments = [
        entry.name.clone(),
        level.to_string(),
        class.to_owned(),
        zone.to_owned(),
    ];
    Some(
        messages
            .formatted(CHARACTER_LINE, &arguments)
            .unwrap_or_else(|| {
                debug!("The installed string table lacks string {CHARACTER_LINE}");
                zone.to_owned()
            }),
    )
}

/// Covers the scene behind the screen until a character has completed zone
/// admission: under the skin's list, with the list's guidance at its foot,
/// or with the client's own panel on it. While the highlighted character
/// stands in the scene (`showing`), the cover is clear and lets the pointer
/// through to the camera, and the client's own panel moves to the left.
#[allow(clippy::too_many_lines, reason = "one character-selection panel tree")]
fn spawn(
    commands: &mut Commands,
    selection: Option<&Selection>,
    (status, line): (&str, Option<&str>),
    (skinned, showing): (bool, bool),
) {
    let backdrop = BackgroundColor(if showing { Color::NONE } else { theme::COVER });
    if skinned {
        let mut root = commands.spawn((
            Root,
            // Under the skin's list, which is drawn at the screen's layer.
            GlobalZIndex(super::windows::Layer::Screen.base() - 1),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexEnd,
                row_gap: px(6),
                padding: UiRect::bottom(px(40)),
                ..default()
            },
            backdrop,
        ));
        if !showing {
            root.insert(Button);
        }
        root.with_children(|root| {
            if let Some(line) = line {
                theme::label(root, line, Size::Label);
            }
            theme::label(root, guidance(selection, status), Size::Label);
        });
        return;
    }
    let mut root = commands.spawn((
        Root,
        GlobalZIndex(super::windows::Layer::Screen.base() - 1),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            align_items: AlignItems::Center,
            justify_content: if showing {
                JustifyContent::FlexStart
            } else {
                JustifyContent::Center
            },
            padding: UiRect::left(px(if showing { 40 } else { 0 })),
            ..default()
        },
        backdrop,
    ));
    if !showing {
        root.insert(Button);
    }
    root.with_children(|root| {
        root.spawn((
            Node {
                width: px(440),
                max_width: percent(95),
                padding: UiRect::all(px(24)),
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme::TITLE_BAR),
        ))
        .with_children(|panel| {
            theme::label(panel, "CHARACTER SELECT", Size::Display);
            button(panel, Action::Quit, "Back / Disconnect", false);
            let Some(selection) = selection else {
                theme::label(panel, guidance(selection, status), Size::Large);
                return;
            };
            for entry in &selection.entries {
                let detail = entry
                    .level
                    .map_or_else(String::new, |level| format!("  |  Level {level}"));
                button(
                    panel,
                    Action::Choose(entry.slot),
                    &format!("{}{detail}", entry.name),
                    selection.selected == Some(entry.slot),
                );
            }
            if let Some(rules) = selection.can_create() {
                for slot in (0..=u8::MAX).take(rules.slots()) {
                    if selection.can_create_at(slot) {
                        button(panel, Action::Choose(slot), "Create New Character", false);
                    }
                }
            } else if selection.full() {
                theme::label(panel, "All character slots are occupied.", Size::Label);
            }
            if selection.selected.is_some() && !selection.submitted && !selection.held {
                button(panel, Action::Enter, "Enter World", true);
                if selection.deletion.is_some() {
                    button(panel, Action::Delete, "Delete Character", false);
                }
            }
            if let Some(line) = line {
                theme::label(panel, line, Size::Label);
            }
            theme::label(panel, guidance(Some(selection), status), Size::Label);
        });
    });
}

/// A skin without the confirmation window still offers the same explicit
/// Yes/No question. Both buttons feed the ordinary confirmation owner.
fn deletion_confirmation(commands: &mut Commands, request: &eq_client_core::deletion::Request) {
    commands
        .spawn((
            Root,
            GlobalZIndex(super::windows::Layer::Tool.base() + 100),
            Button,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme::COVER),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: px(440),
                    max_width: percent(95),
                    padding: UiRect::all(px(24)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(12),
                    ..default()
                },
                BackgroundColor(theme::TITLE_BAR),
            ))
            .with_children(|panel| {
                theme::label(
                    panel,
                    &format!(
                        "Delete character {}? This may permanently remove the character.",
                        request.name()
                    ),
                    Size::Large,
                );
                theme::button_with(
                    panel,
                    crate::confirm::AnswerButton(true),
                    "Yes, delete this character",
                    Size::Large,
                );
                theme::button_with(
                    panel,
                    crate::confirm::AnswerButton(false),
                    "No, keep this character",
                    Size::Large,
                );
                button(panel, Action::Quit, "Back / Disconnect", false);
            });
        });
}

fn button(parent: &mut ChildSpawnerCommands, action: Action, text: &str, selected: bool) {
    theme::button_with(parent, action, text, Size::Large).insert((
        Node {
            padding: UiRect::all(px(10)),
            ..default()
        },
        BackgroundColor(theme::button(true, selected, Interaction::None)),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::window::PrimaryWindow;

    fn listed(slot: u8, name: &str, zone_id: Option<u32>) -> CharacterChoice {
        CharacterChoice {
            slot,
            name: name.into(),
            level: Some(12),
            zone_id,
            look: Some(eq_client_core::CharacterLook {
                class: 1,
                ..default()
            }),
        }
    }

    #[test]
    fn the_highlighted_characters_zone_is_its_long_name_and_follows_the_highlight() {
        let mut selection = Selection::new(
            7,
            vec![
                listed(0, "Exampleone", Some(4)),
                listed(2, "Exampletwo", Some(9)),
                listed(5, "Examplethree", None),
                listed(6, "Examplefour", Some(65_000)),
                // The zone's number in the low 16 bits, as the program reads it.
                listed(7, "Examplefive", Some((5 << 16) | 4)),
            ],
        );
        assert_eq!(selection.chosen_zone(), Some("The Qeynos Hills"));
        assert!(selection.choose_named("exampletwo"));
        assert_eq!(selection.chosen_zone(), Some("West Freeport"));
        // A list without the zone, and a number the table doesn't know, name
        // none rather than one made up.
        assert!(selection.choose_named("Examplethree"));
        assert_eq!(selection.chosen_zone(), None);
        assert!(selection.choose_named("Examplefour"));
        assert_eq!(selection.chosen_zone(), None);
        assert!(selection.choose_named("Examplefive"));
        assert_eq!(selection.chosen_zone(), Some("The Qeynos Hills"));
        assert_eq!(Selection::new(7, vec![]).chosen_zone(), None);
    }

    #[test]
    fn the_character_line_is_the_installed_string_with_all_four_parts_or_the_zone_alone() {
        // A test table: the line's string takes its four arguments in order.
        let messages = crate::hud::messages::Messages::parse(
            "EQST0002\n0\n6681 <%1|%2|%3|%4>\n1502 Classone\n",
        );
        let line = |entry: CharacterChoice, messages: &crate::hud::messages::Messages| {
            character_line(Some(&Selection::new(7, vec![entry])), messages)
        };
        assert_eq!(
            line(listed(0, "Exampleone", Some(4)), &messages).as_deref(),
            Some("<Exampleone|12|Classone|The Qeynos Hills>")
        );
        // Without the level, the class, or the class's string: the zone alone.
        let mut unlevelled = listed(0, "Exampleone", Some(4));
        unlevelled.level = None;
        assert_eq!(
            line(unlevelled, &messages).as_deref(),
            Some("The Qeynos Hills")
        );
        let mut unlooked = listed(0, "Exampleone", Some(4));
        unlooked.look = None;
        assert_eq!(
            line(unlooked, &messages).as_deref(),
            Some("The Qeynos Hills")
        );
        let mut unnamed = listed(0, "Exampleone", Some(4));
        unnamed.look = Some(eq_client_core::CharacterLook {
            class: 2,
            ..default()
        });
        assert_eq!(
            line(unnamed, &messages).as_deref(),
            Some("The Qeynos Hills")
        );
        // Without the line's string: the zone alone.
        let lacking = crate::hud::messages::Messages::parse("EQST0002\n0\n1502 Classone\n");
        assert_eq!(
            line(listed(0, "Exampleone", Some(4)), &lacking).as_deref(),
            Some("The Qeynos Hills")
        );
        // Without the zone: nothing, whatever else is known.
        assert_eq!(line(listed(0, "Exampleone", None), &messages), None);
        assert_eq!(line(listed(0, "Exampleone", Some(65_000)), &messages), None);
        assert_eq!(character_line(None, &messages), None);
    }

    /// An app drawing character select over this state and front end.
    fn screen_app(
        state: OnlineState,
        front: crate::login::FrontEnd,
        outbox: crate::outbox::Outbox,
    ) -> App {
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.insert_resource(state)
            .insert_resource(outbox)
            .insert_resource(front)
            .init_resource::<crate::notices::Lines>()
            .init_resource::<crate::character_create::Editor>()
            .init_resource::<crate::confirm::Asked>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<super::super::navigation::NavigationKeys>()
            .init_resource::<crate::windows::Shown>()
            .init_resource::<crate::skinned::Skinned>()
            .init_resource::<crate::loading::Screen>()
            .init_resource::<crate::select_preview::Preview>()
            .init_resource::<crate::hud::messages::Messages>()
            .add_message::<AppExit>()
            .add_systems(Update, update);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        app
    }

    fn deletion_state(connection_id: u64, selection_id: u64) -> OnlineState {
        let mut state = OnlineState::new(true);
        crate::online::testing::news(
            &mut state,
            [eq_client_core::WorldEvent::CharacterSelection {
                connection_id,
                selection_id,
                characters: vec![listed(0, "Disposable", Some(4))],
                creation: Some(eq_client_core::creation::Rules::Titanium),
                deletion: Some(eq_client_core::deletion::Rules::Titanium),
            }],
        );
        let list = state.world().characters().unwrap().clone();
        state.deletion.listed(&list);
        state.selection = Some(
            Selection::new(selection_id, list.characters.clone())
                .with_creation(list.creation)
                .with_deletion(list.scope(), list.deletion),
        );
        state
    }

    #[test]
    fn ordinary_named_confirmation_sends_only_on_yes_and_keeps_back_available() {
        for yes in [false, true] {
            let (tx, rx) = std::sync::mpsc::sync_channel(4);
            let state = deletion_state(7, 9);
            let mut app = screen_app(
                state,
                crate::login::testing::past_login(),
                Outbox::new(Some(tx)),
            );
            crate::clicks::testing::install(&mut app);
            app.init_resource::<crate::ViewerSettings>()
                .init_resource::<crate::chat::ChatState>()
                .add_systems(Update, crate::confirm::buttons.before(update));
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Delete);
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
            app.update();
            assert!(rx.try_recv().is_err());
            let state = app.world().resource::<OnlineState>();
            let request = state.deletion.prompt().unwrap().clone();
            assert_eq!(request.name(), "Disposable");
            assert_eq!(
                request.scope(),
                eq_client_core::deletion::Scope {
                    connection_id: 7,
                    selection_id: 9
                }
            );
            assert!(
                app.world_mut()
                    .query::<&Action>()
                    .iter(app.world())
                    .any(|action| matches!(action, Action::Quit))
            );
            let answer = app
                .world_mut()
                .query::<(Entity, &crate::confirm::AnswerButton)>()
                .iter(app.world())
                .find_map(|(entity, answer)| (answer.0 == yes).then_some(entity))
                .unwrap();
            crate::clicks::testing::click(&mut app, answer);
            if yes {
                assert!(
                    matches!(rx.try_recv().unwrap(), ClientCommand::DeleteCharacter(sent) if sent == request)
                );
                assert!(app.world().resource::<OnlineState>().deletion.blocked());
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::Enter);
                app.update();
                assert!(rx.try_recv().is_err(), "pending deletion must hold entry");
            } else {
                assert!(rx.try_recv().is_err());
                assert!(!app.world().resource::<OnlineState>().deletion.blocked());
            }
        }
    }

    #[test]
    fn script_uses_the_ordinary_identity_sender_and_stale_confirmation_cannot_send() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let outbox = Outbox::new(Some(tx));
        let mut state = deletion_state(7, 9);
        assert!(
            state
                .script_character_deletion("disposable", "disposable", &outbox)
                .is_err()
        );
        assert!(
            state
                .script_character_deletion("Disposable", "Other", &outbox)
                .is_err()
        );
        assert!(rx.try_recv().is_err());
        let old = state.prepare_character_deletion().unwrap();
        let new_list = eq_client_core::world::CharacterList {
            connection_id: 8,
            selection_id: 9,
            characters: vec![listed(0, "Disposable", Some(4))],
            creation: None,
            deletion: Some(eq_client_core::deletion::Rules::Titanium),
        };
        state.deletion.listed(&new_list);
        assert!(
            state
                .submit_character_deletion(&old, "Disposable", &outbox)
                .is_err()
        );
        assert!(rx.try_recv().is_err());
        let mut state = deletion_state(8, 9);
        let request = state
            .script_character_deletion("Disposable", "Disposable", &outbox)
            .unwrap();
        assert!(
            matches!(rx.try_recv().unwrap(), ClientCommand::DeleteCharacter(sent) if sent == request)
        );
        assert!(
            state
                .script_character_deletion("Disposable", "Disposable", &outbox)
                .is_err()
        );
        assert!(rx.try_recv().is_err());
    }

    /// The layers character select's roots are drawn at.
    fn root_layers(app: &mut App) -> Vec<i32> {
        app.world_mut()
            .query_filtered::<&GlobalZIndex, With<Root>>()
            .iter(app.world())
            .map(|layer| layer.0)
            .collect()
    }

    #[test]
    fn the_loading_screen_covers_character_select_until_the_server_lists_again() {
        // Camped: the session is past the login and the world has sent no
        // list yet.
        let mut app = screen_app(
            OnlineState::new(true),
            crate::login::testing::past_login(),
            crate::outbox::Outbox::new(None),
        );
        app.update();
        assert_eq!(root_layers(&mut app), [crate::loading::LAYER]);
        assert_eq!(
            app.world_mut().query::<&Action>().iter(app.world()).count(),
            0
        );
        // The server's list replaces the cover.
        app.world_mut().resource_mut::<OnlineState>().selection = Some(Selection::new(
            8,
            vec![CharacterChoice {
                slot: 0,
                name: "Example".into(),
                level: Some(2),
                zone_id: None,
                look: None,
            }],
        ));
        app.update();
        let layers = root_layers(&mut app);
        assert_ne!(layers, [] as [i32; 0]);
        assert!(!layers.contains(&crate::loading::LAYER), "{layers:?}");
        assert_ne!(
            app.world_mut().query::<&Action>().iter(app.world()).count(),
            0
        );
        // A session that ends meanwhile leaves for the login screen, cover and
        // all.
        app.world_mut().resource_mut::<OnlineState>().selection = None;
        app.update();
        assert_eq!(root_layers(&mut app), [crate::loading::LAYER]);
        app.insert_resource(crate::login::FrontEnd::default());
        app.update();
        assert_eq!(root_layers(&mut app), [] as [i32; 0]);
    }

    #[test]
    fn keyboard_selection_queues_the_server_slot_and_disappears_after_admission() {
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let mut state = OnlineState::new(true);
        state.selection = Some(Selection::new(
            7,
            vec![CharacterChoice {
                slot: 3,
                name: "Example".into(),
                level: Some(1),
                zone_id: None,
                look: None,
            }],
        ));
        let mut app = screen_app(
            state,
            crate::login::FrontEnd::default(),
            crate::outbox::Outbox::new(Some(tx)),
        );
        app.update();
        assert!(rx.try_recv().is_err());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowDown);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert_eq!(
            rx.try_recv().unwrap(),
            ClientCommand::SelectCharacter {
                selection_id: 7,
                slot: 3
            }
        );
        app.update();
        assert!(rx.try_recv().is_err());
        // Entering world, the loading screen covers the list.
        assert_eq!(root_layers(&mut app), [crate::loading::LAYER]);
        crate::online::testing::admit(
            &mut app.world_mut().resource_mut::<OnlineState>(),
            8,
            crate::online::testing::player(1),
        );
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<Root>>()
                .iter(world)
                .count(),
            0
        );
    }

    #[test]
    fn the_skins_character_buttons_name_their_slots_and_say_the_level_or_why_not() {
        let mut state = OnlineState::new(true);
        state.selection = Some(Selection::new(
            7,
            vec![CharacterChoice {
                slot: 0,
                name: "Example".into(),
                level: Some(5),
                zone_id: None,
                look: None,
            }],
        ));
        let mut app = App::new();
        app.insert_resource(state).add_systems(Update, names);
        let ink = Color::WHITE;
        let button_words = |slot| {
            (
                SlotName {
                    slot,
                    empty: "Empty slot".into(),
                    ink,
                },
                Text::new(""),
                TextColor(ink),
            )
        };
        let (taken, empty) = (
            app.world_mut().spawn(button_words(0)).id(),
            app.world_mut().spawn(button_words(1)).id(),
        );
        let tips = [0, 1].map(|slot| {
            app.world_mut()
                .spawn((Action::Choose(slot), crate::tooltip::Tooltip::default()))
                .id()
        });
        app.update();
        let world = app.world();
        assert_eq!(world.get::<Text>(taken).unwrap().0, "Example");
        assert_eq!(world.get::<TextColor>(taken).unwrap().0, ink);
        assert_eq!(world.get::<Text>(empty).unwrap().0, "Empty slot");
        assert_eq!(world.get::<TextColor>(empty).unwrap().0, theme::INK_DIM);
        assert_eq!(
            world.get::<crate::tooltip::Tooltip>(tips[0]).unwrap().0,
            "Level 5"
        );
        // An empty slot gives the reason it does nothing, once the list came.
        assert_eq!(
            world.get::<crate::tooltip::Tooltip>(tips[1]).unwrap().0,
            "No character available in this slot"
        );
        app.world_mut()
            .resource_mut::<OnlineState>()
            .selection
            .as_mut()
            .unwrap()
            .creation = Some(eq_client_core::creation::Rules::Titanium);
        app.update();
        assert_eq!(app.world().get::<TextColor>(empty).unwrap().0, ink);
        assert_eq!(
            app.world()
                .get::<crate::tooltip::Tooltip>(tips[1])
                .unwrap()
                .0,
            "Create a character"
        );
        app.world_mut().resource_mut::<OnlineState>().selection = None;
        app.update();
        assert_eq!(
            app.world()
                .get::<crate::tooltip::Tooltip>(tips[1])
                .unwrap()
                .0,
            ""
        );
    }

    #[test]
    fn empty_slots_open_creation_in_both_lists_without_a_separate_button() {
        use eq_client_core::creation::Rules;
        for rules in [Rules::Titanium, Rules::EqMac] {
            for skinned in [false, true] {
                let (tx, rx) = std::sync::mpsc::sync_channel(4);
                let mut state = OnlineState::new(true);
                state.selection = Some(
                    Selection::new(7, vec![listed(0, "Example", Some(4))])
                        .with_creation(Some(rules)),
                );
                let mut app = screen_app(
                    state,
                    crate::login::testing::past_login(),
                    Outbox::new(Some(tx)),
                );
                crate::clicks::testing::install(&mut app);
                if skinned {
                    app.insert_resource(crate::skinned::Skinned::of(&[WindowId::CharacterSelect]));
                }
                app.update();
                let empty = if skinned {
                    assert_eq!(
                        app.world_mut().query::<&Action>().iter(app.world()).count(),
                        0,
                        "The installed list must not gain a separate create button"
                    );
                    app.world_mut()
                        .spawn((Action::Choose(1), Interaction::None))
                        .id()
                } else {
                    let mut choices = app.world_mut().query::<(Entity, &Action)>();
                    let slots = choices
                        .iter(app.world())
                        .filter(|(_, action)| matches!(action, Action::Choose(_)))
                        .count();
                    assert_eq!(slots, rules.slots());
                    choices
                        .iter(app.world())
                        .find_map(|(entity, action)| {
                            matches!(action, Action::Choose(1)).then_some(entity)
                        })
                        .unwrap()
                };
                crate::clicks::testing::click(&mut app, empty);
                assert!(
                    app.world()
                        .resource::<crate::character_create::Editor>()
                        .active()
                );
                assert!(rx.try_recv().is_err(), "Opening a draft sends no request");
                assert_eq!(
                    app.world()
                        .resource::<OnlineState>()
                        .selection
                        .as_ref()
                        .unwrap()
                        .chosen(),
                    Some(0)
                );
            }
        }
    }

    #[test]
    fn entry_requires_selection_and_never_duplicates_or_discards_a_full_queue() {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let queue = tx.clone();
        let sender = crate::outbox::Outbox::new(Some(tx));
        let world = eq_client_core::world::ClientWorld::default();
        let mut selection = Selection::new(
            7,
            vec![CharacterChoice {
                slot: 3,
                name: "Example".into(),
                level: Some(1),
                zone_id: Some(22),
                look: None,
            }],
        );
        // The list arrives with its first character highlighted.
        assert_eq!(selection.chosen(), Some(3));
        selection.selected = None;
        selection.enter(&sender, &world);
        assert!(rx.try_recv().is_err());
        selection.selected = Some(3);
        queue
            .try_send(ClientCommand::SelectCharacter {
                selection_id: 6,
                slot: 1,
            })
            .unwrap();
        selection.enter(&sender, &world);
        assert!(!selection.submitted);
        rx.try_recv().unwrap();
        selection.enter(&sender, &world);
        selection.enter(&sender, &world);
        assert!(selection.submitted);
        assert_eq!(
            rx.try_recv().unwrap(),
            ClientCommand::SelectCharacter {
                selection_id: 7,
                slot: 3
            }
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_refused_creation_shows_on_the_list_until_the_next_key() {
        use eq_client_core::{creation::CreationOutcome, world::Notice};
        let mut state = OnlineState::new(true);
        let mut selection = Selection::new(
            7,
            vec![CharacterChoice {
                slot: 3,
                name: "Example".into(),
                level: Some(1),
                zone_id: None,
                look: None,
            }],
        );
        let browse = "Select a character | Up/Down: browse | Enter: connect";
        assert_eq!(guidance(Some(&selection), "Connected"), browse);
        // Other news leaves the line alone.
        selection.hear(&Notice::Connection {
            link: eq_client_core::world::Link::Connected,
            dead: false,
        });
        assert_eq!(guidance(Some(&selection), "Connected"), browse);
        selection.hear(&Notice::CreationRefused {
            name: "Newname".into(),
            outcome: CreationOutcome::NameRefused,
        });
        let refused = crate::notices::creation_refusal("Newname", CreationOutcome::NameRefused);
        assert_eq!(guidance(Some(&selection), "Connected"), refused);
        state.selection = Some(selection);
        let (tx, _rx) = std::sync::mpsc::sync_channel(4);
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.insert_resource(state)
            .insert_resource(crate::outbox::Outbox::new(Some(tx)))
            .init_resource::<crate::notices::Lines>()
            .init_resource::<crate::character_create::Editor>()
            .init_resource::<crate::confirm::Asked>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<super::super::navigation::NavigationKeys>()
            .init_resource::<crate::windows::Shown>()
            .init_resource::<crate::skinned::Skinned>()
            .init_resource::<crate::login::FrontEnd>()
            .init_resource::<crate::loading::Screen>()
            .init_resource::<crate::select_preview::Preview>()
            .init_resource::<crate::hud::messages::Messages>()
            .add_message::<AppExit>()
            .add_systems(Update, update);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        app.update();
        let shown = |app: &App| {
            let selection = app.world().resource::<OnlineState>().selection.as_ref();
            guidance(selection, "Connected").to_owned()
        };
        assert_eq!(shown(&app), refused);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ArrowDown);
        app.update();
        assert_eq!(shown(&app), browse);
    }
}
