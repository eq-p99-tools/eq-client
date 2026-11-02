//! Character creation is a local draft until the final Create button sends
//! one typed request. The session owns name approval and the packet exchange.
use crate::{
    character_select::Selection,
    login,
    online::OnlineState,
    outbox::Outbox,
    theme::{self, Size},
};
use bevy::{
    clipboard::{Clipboard, ClipboardRead},
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
};

mod name;
use eq_client_core::{
    ClientCommand,
    character_creation::{Draft, STATS},
    creation::{CLASSES, CreationOutcome, DEITIES, RACES, appearance::Feature, names},
    world::ClientWorld,
};

#[derive(Resource, Default)]
pub(super) struct Editor {
    form: Option<Form>,
    request_id: u64,
}

#[derive(Debug)]
struct Form {
    selection_id: u64,
    draft: Draft,
    page: u8,
    typing: bool,
    select_all: bool,
    paste: Option<ClipboardRead>,
    pending: Option<(String, Option<u64>)>,
    generating: Option<u64>,
    generator_enabled: bool,
    message: String,
}

impl Editor {
    pub(super) fn preview(&self) -> Option<(&eq_client_core::creation::NewCharacter, bool)> {
        self.form
            .as_ref()
            .map(|form| (&form.draft.character, form.draft.rules.classic_appearance()))
    }

    pub(super) fn active(&self) -> bool {
        self.form.is_some()
    }

    pub(super) fn open(&mut self, selection: &Selection, world: &ClientWorld) {
        if self.active() {
            return;
        }
        let Some(rules) = selection.can_create() else {
            return;
        };
        self.form = Some(Form {
            selection_id: selection.id(),
            draft: Draft::new(rules, world.expansions(), world.pvp_rule()),
            page: 0,
            typing: true,
            select_all: false,
            paste: None,
            pending: None,
            generating: None,
            generator_enabled: world.name_generator(),
            message: String::new(),
        });
    }

    pub(super) fn open_preview(
        &mut self,
        selection: &Selection,
        world: &ClientWorld,
        draft: Draft,
    ) {
        self.open(selection, world);
        if let Some(form) = self.form.as_mut() {
            form.draft = draft;
        }
    }

    /// A refreshed list invalidates the draft. Success selects the new
    /// character without entering it; a refusal keeps every editable field.
    fn refresh(&mut self, online: &mut OnlineState) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let (selection, world) = online.choosing();
        let Some(selection) = selection.filter(|selection| !selection.entering()) else {
            self.form = None;
            return;
        };
        form.generator_enabled = world.name_generator();
        if let Some(answer) = world.name_suggestion().filter(|answer| {
            answer.selection_id == form.selection_id && Some(answer.request_id) == form.generating
        }) {
            form.generating = None;
            if let Some(name) = &answer.name {
                form.draft.name.clone_from(name);
                form.typing = form.page == 0;
                form.select_all = true;
                form.message.clear();
            } else {
                form.message =
                    "No name suggestion received. You can type a name or try again.".into();
            }
        }
        if let Some((name, before)) = &form.pending
            && let Some(answer) = world
                .creation()
                .filter(|answer| Some(answer.count) != *before && answer.name == *name)
        {
            if answer.outcome == CreationOutcome::Created {
                if selection.id() != form.selection_id && selection.choose_named(name) {
                    selection.created(name);
                    self.form = None;
                    return;
                }
            } else {
                form.message = crate::notices::creation_refusal(name, answer.outcome);
                form.pending = None;
            }
        }
        if selection.id() != form.selection_id {
            self.form = None;
            return;
        }
        if form.pending.is_none()
            && (form.draft.expansions != world.expansions() || form.draft.pvp != world.pvp_rule())
        {
            form.draft.expansions = world.expansions();
            form.draft.pvp = world.pvp_rule();
            let before = (form.draft.character.race, form.draft.character.class);
            form.draft.reconcile();
            if before != (form.draft.character.race, form.draft.character.class) {
                form.draft.reset_stats();
            }
        }
    }
}

impl Form {
    fn generate(&mut self, id: u64, online: &OnlineState, outbox: &Outbox) {
        if self.generating.is_some() || self.pending.is_some() || !self.generator_enabled {
            return;
        }
        let c = &self.draft.character;
        match self.draft.rules.name_source() {
            names::Source::Local => match names::local(c.race, c.gender) {
                Ok(name) => {
                    self.draft.name = name;
                    self.select_all = true;
                    self.typing = true;
                    self.message.clear();
                }
                Err(error) => self.message = error.to_string(),
            },
            names::Source::World => match outbox.send(
                online.world(),
                ClientCommand::GenerateName {
                    selection_id: self.selection_id,
                    request_id: id,
                    race: c.race,
                    gender: c.gender,
                },
            ) {
                Ok(()) => {
                    self.generating = Some(id);
                    self.message = "Asking the server for a name...".into();
                }
                Err(error) => self.message = error.text().into(),
            },
        }
    }
    fn submit(&mut self, online: &OnlineState, outbox: &Outbox) {
        if self.pending.is_some() {
            return;
        }
        if online.selection.as_ref().is_none_or(|selection| {
            selection.id() != self.selection_id || selection.can_create().is_none()
        }) {
            self.message = "The character list changed. Return to the list and try again.".into();
            return;
        }
        match self.draft.request() {
            Ok(character) => {
                let name = character.name.clone();
                match outbox.send(
                    online.world(),
                    ClientCommand::CreateCharacter {
                        selection_id: self.selection_id,
                        character,
                    },
                ) {
                    Ok(()) => {
                        self.generating = None;
                        self.pending =
                            Some((name, online.world().creation().map(|answer| answer.count)));
                        self.message = "Waiting for the server to approve the name and create your character...".into();
                        self.typing = false;
                    }
                    Err(error) => self.message = error.text().into(),
                }
            }
            Err(reason) => self.message = reason,
        }
    }
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Name,
    Race(u32),
    Class(u32),
    Gender(u32),
    Feature(Feature, bool),
    Generate,
    RandomizeAppearance,
    Turn(bool),
    Zoom,
    Deity(u32),
    City(u32),
    Tutorial(bool),
    Point(usize, bool),
    Allocate,
    Reset,
    Page(u8),
    Submit,
    Cancel,
}

#[derive(Component)]
pub(super) struct Root;

#[derive(Component)]
pub(super) struct Panel;

#[allow(clippy::needless_pass_by_value)]
pub(super) fn scroll(
    wheel: Res<crate::windows::pointer::Wheel>,
    mut panels: Query<(Entity, &ComputedNode, &mut ScrollPosition), With<Panel>>,
) {
    for (entity, node, mut position) in &mut panels {
        if wheel.surface == Some(entity) {
            crate::windows::scroll_by(&mut position, node, wheel.pixels);
        }
    }
}

/// Claim before chat, navigation, Escape routing and gameplay input. Keep
/// the claim for a closing frame so Escape cannot also leave character select.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn claim(editor: Res<Editor>, mut typing: ResMut<crate::keys::Typing>) {
    typing.creating_character = editor.active();
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn input(
    mut editor: ResMut<Editor>,
    mut online: ResMut<OnlineState>,
    front: Res<login::FrontEnd>,
    outbox: Res<Outbox>,
    mut preview: ResMut<crate::creation_preview::Preview>,
    keys: crate::keys::Keys,
    buttons: crate::clicks::Clicks<Action>,
    mut typed: MessageReader<KeyboardInput>,
    logical: Res<ButtonInput<Key>>,
    mut clipboard: Option<ResMut<Clipboard>>,
) {
    let events = typed.read().cloned().collect::<Vec<_>>();
    editor.refresh(&mut online);
    if front.screen(&online) != login::Screen::Characters {
        editor.form = None;
    }
    // Unique across closing/reopening drafts; late answers cannot edit a new form.
    let actions = buttons.iter().copied().collect::<Vec<_>>();
    if actions.contains(&Action::Generate) {
        editor.request_id = editor.request_id.wrapping_add(1);
    }
    let request_id = editor.request_id;
    let Some(form) = editor.form.as_mut() else {
        return;
    };
    if !keys.window_focused() || keys.editing_script() || form.pending.is_some() {
        form.paste = None;
        return;
    }
    if actions.contains(&Action::Cancel) || keys.input.just_pressed(KeyCode::Escape) {
        editor.form = None;
        return;
    }
    for action in actions {
        act(form, action, request_id, &online, &outbox, &mut preview);
        if form.pending.is_some() {
            break;
        }
    }
    if form.pending.is_some() {
        return;
    }
    if keys.input.just_pressed(KeyCode::Tab) {
        form.typing = form.page == 0 && !form.typing;
    }
    form.edit_name(&events, &logical, clipboard.as_deref_mut());
    if keys.input.just_pressed(KeyCode::Enter) || keys.input.just_pressed(KeyCode::NumpadEnter) {
        if form.page < 3 {
            form.page += 1;
            form.typing = false;
        } else {
            form.submit(&online, &outbox);
        }
    }
}

fn act(
    form: &mut Form,
    action: Action,
    request_id: u64,
    online: &OnlineState,
    outbox: &Outbox,
    preview: &mut crate::creation_preview::Preview,
) {
    match action {
        Action::Name => form.typing = true,
        Action::Race(id) => {
            form.generating = None;
            form.draft.race(id);
        }
        Action::Class(id) => form.draft.class(id),
        Action::Gender(id) => {
            form.generating = None;
            form.draft.character.gender = id;
            form.draft.reconcile();
        }
        Action::Feature(feature, next) => {
            let c = &mut form.draft.character;
            form.draft
                .rules
                .appearance(c.race, c.gender)
                .step(c, feature, next);
        }
        Action::Generate => form.generate(request_id, online, outbox),
        Action::RandomizeAppearance => {
            let c = &mut form.draft.character;
            form.draft.rules.appearance(c.race, c.gender).randomize(c);
        }
        Action::Turn(right) => preview.angle += if right { 0.35 } else { -0.35 },
        Action::Zoom => preview.close = !preview.close,
        Action::Deity(id) => form.draft.deity(id),
        Action::City(id) => form.draft.character.start_zone = id,
        Action::Tutorial(start) => {
            form.draft.character.tutorial = start && form.draft.rules.offers_tutorial();
        }
        Action::Point(index, add) => form.draft.point(index, add),
        Action::Allocate => form.draft.allocate(),
        Action::Reset => form.draft.reset_stats(),
        Action::Page(page) => {
            form.page = page;
            form.typing = false;
        }
        Action::Submit => form.submit(online, outbox),
        Action::Cancel => (),
    }
    if !matches!(action, Action::Submit | Action::Generate) {
        form.message.clear();
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(super) fn present(
    mut commands: Commands,
    editor: Res<Editor>,
    preview: Res<crate::creation_preview::Preview>,
    roots: Query<Entity, With<Root>>,
    panels: Query<&ScrollPosition, With<Panel>>,
    mut previous: Local<String>,
    mut previous_page: Local<Option<u8>>,
) {
    let signature = format!("{:?} {:?} {}", editor.form, preview.status, preview.close);
    if *previous == signature {
        return;
    }
    *previous = signature;
    for root in &roots {
        commands.entity(root).despawn();
    }
    let Some(form) = &editor.form else {
        return;
    };
    let offset = if *previous_page == Some(form.page) {
        panels.single().map_or(Vec2::ZERO, |position| position.0)
    } else {
        Vec2::ZERO
    };
    *previous_page = Some(form.page);
    login::cover(&mut commands, Root)
        .insert(GlobalZIndex(crate::windows::Layer::Screen.base() + 20))
        .with_children(|root| {
            root.spawn((Panel, crate::windows::pointer::TakesWheel, ScrollPosition(offset), Node {
                width: px(780), max_width: percent(96), max_height: percent(96), overflow: Overflow::scroll_y(),
                padding: UiRect::all(px(20)), row_gap: px(10), flex_direction: FlexDirection::Column,
                border_radius: BorderRadius::all(px(8)), ..default()
            }, BackgroundColor(theme::TITLE_BAR))).with_children(|panel| {
                theme::label(panel, "CREATE CHARACTER", Size::Display);
                row(panel, |row| {
                    for (page, title) in [(0, "1  Identity"), (1, "2  Appearance"), (2, "3  Attributes"), (3, "4  Home & review")] {
                        button(row, Action::Page(page), title, form.pending.is_none(), form.page == page);
                    }
                });
                if form.pending.is_some() {
                    theme::label(panel, &form.message, Size::Large);
                    theme::label(panel, "You will return to character selection when the server confirms creation.", Size::Label);
                    login::lit_button(panel, crate::character_select::Action::Quit, "Disconnect", login::Lit { usable: true, on: false });
                    return;
                }
                match form.page {
                    0 => identity(panel, form),
                    1 => appearance(panel, form, &preview),
                    2 => attributes(panel, form),
                    _ => home(panel, form),
                }
                let reason = form.draft.request().err();
                let message = if form.message.is_empty() { reason.as_deref().unwrap_or("Ready to create. The server makes the final decision on your name and choices.") } else { form.message.as_str() };
                theme::label(panel, message, Size::Label);
                row(panel, |row| {
                    button(row, Action::Cancel, "Cancel", true, false);
                    if form.page > 0 { button(row, Action::Page(form.page - 1), "Back", true, false); }
                    if form.page < 3 { button(row, Action::Page(form.page + 1), "Next", true, true); }
                    else { button(row, Action::Submit, "Create Character", reason.is_none(), true); }
                });
            });
        });
}

fn row(parent: &mut ChildSpawnerCommands, build: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn(Node {
            flex_shrink: 0.0,
            flex_wrap: FlexWrap::Wrap,
            column_gap: px(6),
            row_gap: px(5),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(build);
}

fn button(parent: &mut ChildSpawnerCommands, action: Action, text: &str, usable: bool, on: bool) {
    let mut button = login::lit_button(parent, action, text, login::Lit { usable, on });
    if !usable {
        button.remove::<Action>();
    }
}

fn identity(panel: &mut ChildSpawnerCommands, form: &Form) {
    let draft = &form.draft;
    theme::label(panel, "Name (4–15 letters)", Size::Label);
    let name = if draft.name.is_empty() {
        "Click here and type a name".into()
    } else {
        format!("{}{}", draft.name, if form.typing { " |" } else { "" })
    };
    row(panel, |row| {
        button(row, Action::Name, &name, true, form.typing);
        button(
            row,
            Action::Generate,
            if form.generating.is_some() {
                "Generating..."
            } else {
                "Generate name"
            },
            form.generator_enabled && form.generating.is_none(),
            false,
        );
    });
    if !form.generator_enabled {
        theme::label(
            panel,
            "This server has disabled name generation.",
            Size::Label,
        );
    }
    theme::label(panel, "Race", Size::Label);
    row(panel, |row| {
        for (id, name) in draft.rules.races(draft.expansions) {
            button(
                row,
                Action::Race(id),
                name,
                true,
                id == draft.character.race,
            );
        }
    });
    theme::label(panel, "Class", Size::Label);
    row(panel, |row| {
        for (id, name) in draft.rules.classes(draft.character.race, draft.expansions) {
            button(
                row,
                Action::Class(id),
                name,
                true,
                id == draft.character.class,
            );
        }
    });
    row(panel, |row| {
        for (id, name) in [(0, "Male"), (1, "Female")] {
            button(
                row,
                Action::Gender(id),
                name,
                true,
                draft.character.gender == id,
            );
        }
    });
}

fn appearance(
    panel: &mut ChildSpawnerCommands,
    form: &Form,
    preview: &crate::creation_preview::Preview,
) {
    row(panel, |columns| {
        columns
            .spawn(Node {
                width: px(340),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                ..default()
            })
            .with_children(|panel| {
                appearance_controls(panel, form);
                button(
                    panel,
                    Action::RandomizeAppearance,
                    "Randomize appearance",
                    true,
                    false,
                );
            });
        columns
            .spawn(Node {
                width: px(300),
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                ..default()
            })
            .with_children(|panel| {
                // Loading a new appearance must not move the form's controls.
                panel
                    .spawn(Node {
                        width: px(280),
                        height: px(345),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    })
                    .with_children(|preview_panel| {
                        if preview.status.is_empty() {
                            preview_panel.spawn((
                                ImageNode::new(preview.image.clone()),
                                Node {
                                    width: percent(100),
                                    height: percent(100),
                                    ..default()
                                },
                            ));
                        } else {
                            theme::label(preview_panel, &preview.status, Size::Label);
                        }
                    });
                row(panel, |row| {
                    button(row, Action::Turn(false), "Turn left", true, false);
                    button(row, Action::Turn(true), "Turn right", true, false);
                    button(
                        row,
                        Action::Zoom,
                        if preview.close {
                            "Full body"
                        } else {
                            "Face close-up"
                        },
                        true,
                        false,
                    );
                });
            });
    });
}

fn appearance_controls(panel: &mut ChildSpawnerCommands, form: &Form) {
    let c = &form.draft.character;
    for (feature, values) in form.draft.rules.appearance(c.race, c.gender).fields() {
        row(panel, |row| {
            row.spawn(Node {
                width: px(170),
                ..default()
            })
            .with_children(|row| {
                theme::label(row, feature.label(), Size::Label);
            });
            button(row, Action::Feature(feature, false), "<", true, false);
            theme::label(
                row,
                &if feature.get(c) == 255 {
                    "None".to_owned()
                } else {
                    let index = values
                        .iter()
                        .position(|value| *value == feature.get(c))
                        .unwrap_or(0);
                    format!("{} / {}", index + 1, values.len())
                },
                Size::Label,
            );
            button(row, Action::Feature(feature, true), ">", true, false);
        });
    }
}

fn attributes(panel: &mut ChildSpawnerCommands, form: &Form) {
    let draft = &form.draft;
    theme::label(
        panel,
        &format!("{} points remaining", draft.remaining()),
        Size::Large,
    );
    for (index, name) in STATS.iter().enumerate() {
        let value = draft.character.stats[index];
        let base = draft.base().0[index];
        row(panel, |row| {
            row.spawn(Node {
                width: px(210),
                ..default()
            })
            .with_children(|label| {
                theme::label(label, name, Size::Large);
            });
            button(row, Action::Point(index, false), "−", value > base, false);
            row.spawn(Node {
                width: px(50),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|label| {
                theme::label(label, &value.to_string(), Size::Large);
            });
            button(
                row,
                Action::Point(index, true),
                "+",
                draft.remaining() > 0 && value < (base + 25).min(150),
                false,
            );
            theme::label(row, &format!("base {base}"), Size::Label);
        });
    }
    row(panel, |row| {
        button(row, Action::Allocate, "Suggested allocation", true, false);
        button(row, Action::Reset, "Reset points", true, false);
    });
}

fn home(panel: &mut ChildSpawnerCommands, form: &Form) {
    let draft = &form.draft;
    let c = &draft.character;
    theme::label(panel, "Deity", Size::Label);
    row(panel, |row| {
        for (id, name) in draft.rules.deities(c.race, c.class, draft.pvp) {
            button(row, Action::Deity(id), name, true, id == c.deity);
        }
    });
    theme::label(panel, "Starting city", Size::Label);
    row(panel, |row| {
        for city in draft.rules.cities(c.race, c.class, c.deity, draft.pvp) {
            button(
                row,
                Action::City(city.start_zone),
                city.name,
                true,
                city.start_zone == c.start_zone,
            );
        }
    });
    if draft.rules.offers_tutorial() {
        row(panel, |row| {
            button(
                row,
                Action::Tutorial(false),
                "Start in home city",
                true,
                !c.tutorial,
            );
            button(
                row,
                Action::Tutorial(true),
                "Start in tutorial",
                true,
                c.tutorial,
            );
        });
    }
    let label = |choices: &[(u32, &'static str)], id| {
        choices
            .iter()
            .find(|(key, _)| *key == id)
            .map_or("Unknown", |(_, name)| *name)
    };
    theme::label(
        panel,
        &format!(
            "{} · {} {} · {}",
            draft.name,
            label(RACES, c.race),
            label(CLASSES, c.class),
            label(DEITIES, c.deity)
        ),
        Size::Large,
    );
    theme::label(
        panel,
        &STATS
            .iter()
            .zip(c.stats)
            .map(|(name, value)| format!("{name} {value}"))
            .collect::<Vec<_>>()
            .join("  ·  "),
        Size::Label,
    );
    theme::label(
        panel,
        &format!(
            "{} · {}",
            if c.gender == 0 { "Male" } else { "Female" },
            draft
                .rules
                .appearance(c.race, c.gender)
                .fields()
                .map(|(feature, values)| {
                    let value = feature.get(c);
                    let choice = if value == 255 {
                        "None".into()
                    } else {
                        (values.iter().position(|v| *v == value).unwrap_or(0) + 1).to_string()
                    };
                    format!("{} {choice}", feature.label())
                })
                .collect::<Vec<_>>()
                .join(" · ")
        ),
        Size::Label,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::{CharacterChoice, WorldEvent, creation::Rules};
    use std::sync::mpsc::{Receiver, sync_channel};

    pub(super) fn setup() -> (Editor, OnlineState, Outbox, Receiver<ClientCommand>) {
        let mut online = OnlineState::new(true);
        online.selection = Some(Selection::new(7, vec![]).with_creation(Some(Rules::EqMac)));
        let mut editor = Editor::default();
        editor.open(online.selection.as_ref().unwrap(), online.world());
        let form = editor.form.as_mut().unwrap();
        form.draft.name = "testhero".into();
        form.draft.allocate();
        let (tx, rx) = sync_channel(1);
        (editor, online, Outbox::new(Some(tx)), rx)
    }

    #[test]
    fn name_generation_obeys_policy_and_never_creates_a_character() {
        let (mut editor, mut online, outbox, rx) = setup();
        let form = editor.form.as_mut().unwrap();
        form.generate(1, &online, &outbox);
        assert_ne!(form.draft.name, "testhero");
        assert!(form.generating.is_none());
        assert!(rx.try_recv().is_err());
        form.draft.rules = Rules::Titanium;
        form.generate(2, &online, &outbox);
        form.generate(3, &online, &outbox);
        assert!(matches!(
            rx.try_recv().unwrap(),
            ClientCommand::GenerateName {
                selection_id: 7,
                request_id: 2,
                race: 1,
                gender: 0
            }
        ));
        assert!(rx.try_recv().is_err());
        crate::online::testing::news(
            &mut online,
            [WorldEvent::NameSuggestion {
                selection_id: 7,
                request_id: 1,
                name: Some("Stalehero".into()),
            }],
        );
        editor.refresh(&mut online);
        assert_eq!(editor.form.as_ref().unwrap().generating, Some(2));
        crate::online::testing::news(
            &mut online,
            [WorldEvent::NameSuggestion {
                selection_id: 7,
                request_id: 2,
                name: Some("Freshhero".into()),
            }],
        );
        editor.refresh(&mut online);
        let form = editor.form.as_mut().unwrap();
        assert_eq!(form.draft.name, "Freshhero");
        assert!(form.generating.is_none());
        form.generate(4, &online, &outbox);
        rx.try_recv().unwrap();
        form.submit(&online, &outbox);
        assert!(
            matches!(rx.try_recv().unwrap(), ClientCommand::CreateCharacter { character, .. } if character.name == "Freshhero")
        );
        crate::online::testing::news(
            &mut online,
            [WorldEvent::NameSuggestion {
                selection_id: 7,
                request_id: 4,
                name: Some("Latehero".into()),
            }],
        );
        editor.refresh(&mut online);
        assert_eq!(editor.form.as_ref().unwrap().draft.name, "Freshhero");
    }

    #[test]
    fn a_disabled_generator_and_full_queue_preserve_the_players_name() {
        let (mut editor, online, outbox, rx) = setup();
        let form = editor.form.as_mut().unwrap();
        form.generator_enabled = false;
        form.generate(1, &online, &outbox);
        assert_eq!(form.draft.name, "testhero");
        form.generator_enabled = true;
        form.draft.rules = Rules::Classic;
        form.generate(2, &online, &outbox);
        form.generating = None; // Manual editing cancels attribution of the reply.
        form.draft.name = "Chosenhero".into();
        form.generate(3, &online, &outbox);
        assert!(form.generating.is_none());
        assert_eq!(form.draft.name, "Chosenhero");
        assert!(rx.try_recv().is_ok());
        form.generate(4, &online, &outbox);
        assert_eq!(form.generating, Some(4));
    }

    #[test]
    fn submit_once_refusal_keeps_the_draft_and_retry_uses_the_same_list() {
        let (mut editor, mut online, outbox, rx) = setup();
        let form = editor.form.as_mut().unwrap();
        let draft = form.draft.clone();
        form.submit(&online, &outbox);
        form.submit(&online, &outbox);
        let sent = rx.try_recv().unwrap();
        assert!(
            matches!(&sent, ClientCommand::CreateCharacter { selection_id: 7, character } if character.name == "Testhero")
        );
        assert!(rx.try_recv().is_err());
        crate::online::testing::news(
            &mut online,
            [WorldEvent::CharacterCreation {
                name: "Testhero".into(),
                outcome: CreationOutcome::NameRefused,
            }],
        );
        editor.refresh(&mut online);
        let form = editor.form.as_mut().unwrap();
        assert!(form.pending.is_none());
        assert_eq!(form.draft, draft);
        assert_ne!(form.message, "");
        form.submit(&online, &outbox);
        assert_eq!(rx.try_recv().unwrap(), sent);
        // The old refusal cannot reject the newly submitted attempt.
        editor.refresh(&mut online);
        assert!(editor.form.as_ref().unwrap().pending.is_some());
    }

    #[test]
    fn a_full_queue_leaves_the_form_editable_and_retryable() {
        let (mut editor, online, _, _) = setup();
        let (tx, rx) = sync_channel(1);
        tx.try_send(ClientCommand::SelectCharacter {
            selection_id: 6,
            slot: 0,
        })
        .unwrap();
        let outbox = Outbox::new(Some(tx));
        let form = editor.form.as_mut().unwrap();
        form.submit(&online, &outbox);
        assert!(form.pending.is_none());
        assert_ne!(form.message, "");
        rx.try_recv().unwrap();
        form.submit(&online, &outbox);
        assert!(form.pending.is_some());
        assert!(matches!(
            rx.try_recv().unwrap(),
            ClientCommand::CreateCharacter {
                selection_id: 7,
                ..
            }
        ));
    }

    #[test]
    fn success_requires_the_new_list_and_selects_without_entering() {
        let (mut editor, mut online, outbox, rx) = setup();
        editor.form.as_mut().unwrap().submit(&online, &outbox);
        rx.try_recv().unwrap();
        crate::online::testing::news(
            &mut online,
            [WorldEvent::CharacterCreation {
                name: "Testhero".into(),
                outcome: CreationOutcome::Created,
            }],
        );
        editor.refresh(&mut online);
        assert!(editor.active());
        online.selection = Some(
            Selection::new(
                8,
                vec![CharacterChoice {
                    slot: 4,
                    name: "Testhero".into(),
                    level: Some(1),
                    zone_id: Some(1),
                    look: None,
                }],
            )
            .with_creation(Some(Rules::EqMac)),
        );
        editor.refresh(&mut online);
        assert!(!editor.active());
        let selection = online.selection.as_ref().unwrap();
        assert_eq!(selection.chosen(), Some(4));
        assert!(!selection.entering());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn stale_list_and_disconnection_close_the_form_without_sending() {
        let (mut editor, mut online, outbox, rx) = setup();
        online.selection = Some(Selection::new(8, vec![]).with_creation(Some(Rules::EqMac)));
        editor.form.as_mut().unwrap().submit(&online, &outbox);
        assert!(rx.try_recv().is_err());
        editor.refresh(&mut online);
        assert!(!editor.active());
        editor.open(online.selection.as_ref().unwrap(), online.world());
        online.selection = None;
        editor.refresh(&mut online);
        assert!(!editor.active());
    }

    #[test]
    fn full_or_unsupported_character_lists_cannot_open_creation() {
        let mut editor = Editor::default();
        let world = ClientWorld::default();
        editor.open(&Selection::new(1, vec![]), &world);
        assert!(!editor.active());
        let characters = (0..8)
            .map(|slot| CharacterChoice {
                slot,
                name: format!("Example{slot}"),
                level: Some(1),
                zone_id: None,
                look: None,
            })
            .collect();
        editor.open(
            &Selection::new(1, characters).with_creation(Some(Rules::Titanium)),
            &world,
        );
        assert!(!editor.active());
    }

    #[test]
    fn typing_is_claimed_and_enter_visits_review_before_submitting() {
        let (mut editor, online, outbox, rx) = setup();
        editor.form.as_mut().unwrap().draft.name.clear();
        let mut app = crate::testing::app();
        app.init_resource::<crate::creation_preview::Preview>();
        app.insert_resource(editor)
            .insert_resource(online)
            .insert_resource(outbox)
            .add_message::<KeyboardInput>()
            .add_systems(Update, (claim, input, present).chain());
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .editing_script = true;
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyX,
            logical_key: Key::Character("x".into()),
            state: bevy::input::ButtonState::Pressed,
            text: Some("x".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert_eq!(
            app.world()
                .resource::<Editor>()
                .form
                .as_ref()
                .unwrap()
                .draft
                .name,
            ""
        );
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .editing_script = false;
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyN,
            logical_key: Key::Character("newhero".into()),
            state: bevy::input::ButtonState::Pressed,
            text: Some("newhero".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert_eq!(
            app.world()
                .resource::<Editor>()
                .form
                .as_ref()
                .unwrap()
                .draft
                .name,
            "newhero"
        );
        assert!(
            app.world()
                .resource::<crate::keys::Typing>()
                .creating_character
        );
        for page in [1, 2, 3] {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Enter);
            app.update();
            assert_eq!(
                app.world().resource::<Editor>().form.as_ref().unwrap().page,
                page
            );
            assert!(rx.try_recv().is_err());
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert!(
            matches!(rx.try_recv().unwrap(), ClientCommand::CreateCharacter { character, .. } if character.name == "Newhero")
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(
            app.world().resource::<Editor>().active(),
            "a sent creation cannot be cancelled locally"
        );
    }

    #[test]
    fn escape_cancels_locally_and_keeps_the_frame_claimed() {
        let (editor, online, outbox, rx) = setup();
        let mut app = crate::testing::app();
        app.init_resource::<crate::creation_preview::Preview>();
        app.insert_resource(editor)
            .insert_resource(online)
            .insert_resource(outbox)
            .add_message::<KeyboardInput>()
            .add_systems(Update, (claim, input, present).chain());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(!app.world().resource::<Editor>().active());
        assert!(
            app.world()
                .resource::<crate::keys::Typing>()
                .creating_character
        );
        assert!(rx.try_recv().is_err());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.update();
        assert!(
            !app.world()
                .resource::<crate::keys::Typing>()
                .creating_character
        );
    }
}
