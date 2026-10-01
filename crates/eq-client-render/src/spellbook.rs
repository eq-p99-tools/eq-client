//! Paged view of the server's indexed spellbook.
use bevy::{prelude::*, window::PrimaryWindow};
mod scribe;
pub(super) use scribe::presentation as scribe_presentation;

/// Keeps an open book above the bottom HUD at the default viewport size.
pub(super) const ROWS_PER_PAGE: usize = 8;

#[derive(Component)]
pub(super) struct BookFrame;
#[derive(Component)]
pub(super) struct BookText;
#[derive(Component)]
pub(super) struct GemHint;
#[derive(Component)]
pub(super) struct PageButton(bool);
#[derive(Component)]
pub(super) struct BookEntry(pub(super) usize);
#[derive(Component)]
pub(super) struct GemChoice(pub(super) u8);
#[derive(Component)]
pub(super) struct ScribeCursor;

#[derive(Resource, Default)]
pub(super) struct BookView {
    pub(super) page: usize,
}

/// Populates a clearly offline book through normalized spell-slot updates.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn demo(
    settings: Res<super::ViewerSettings>,
    mut online: ResMut<super::online::OnlineState>,
    mut shown: ResMut<super::windows::Shown>,
) {
    if !settings.0.demo_spellbook || online.enabled {
        return;
    }
    if online.world.spell_book().is_some() {
        return;
    }
    let mut book = eq_client_core::SpellBook::default();
    for slot in 0..14 {
        book.apply(&eq_client_core::SpellUpdate::Slot {
            slot,
            spell_id: slot + 1,
            mode: 0,
        });
    }
    let gems: [Option<u32>; 8] = std::array::from_fn(|index| u32::try_from(index + 1).ok());
    let buff = |spell_id| eq_client_core::Buff {
        spell_id,
        caster_level: 1,
        effect_type: 2,
        bard_modifier: 10,
        duration_ticks: 5,
        counters: 0,
        caster_id: 0,
    };
    let mut news = vec![
        eq_client_core::WorldEvent::SpellBook(book),
        eq_client_core::WorldEvent::BuffSnapshot(vec![Some(buff(202)), None, Some(buff(200))]),
    ];
    // The preview player, admitted at startup, memorizes the first spells.
    news.extend(gems.iter().zip(0..).filter_map(|(spell, slot)| {
        spell.map(|spell_id| {
            eq_client_core::WorldEvent::Spell(eq_client_core::SpellUpdate::Slot {
                slot,
                spell_id,
                mode: 1,
            })
        })
    }));
    let now = std::time::Instant::now();
    for event in news {
        online.world.apply(
            &eq_client_core::WorldUpdate::Game(event),
            now,
            &eq_client_core::world::NoSpells,
        );
    }
    shown.open(super::windows::WindowId::Spellbook);
}

type BookRows<'w, 's> = Query<
    'w,
    's,
    (
        Ref<'static, Interaction>,
        &'static BookEntry,
        &'static mut Text,
        &'static mut Node,
        &'static mut BackgroundColor,
    ),
    (Without<BookText>, Without<BookFrame>),
>;

/// The spell chosen in the book and what the book says about it; a new
/// admission or a camp starts it over.
#[derive(Resource, Default)]
pub(super) struct BookSelection {
    deletion: super::book_delete::Confirmation,
    selected: Option<u32>,
    location: Option<(u32, usize)>,
    message: String,
    reply_revision: u64,
}

impl BookSelection {
    /// Follows a selected spell only when a book update changes its displayed position.
    fn relocated_page(&mut self, entries: &[(usize, u32)]) -> Option<usize> {
        let location = self.selected.and_then(|id| {
            entries
                .iter()
                .position(|(_, spell)| *spell == id)
                .map(|index| (id, index))
        });
        let page = match (self.location, location) {
            (Some((old_id, old_index)), Some((id, index)))
                if old_id == id && old_index != index =>
            {
                Some(index / ROWS_PER_PAGE)
            }
            _ => None,
        };
        self.location = location;
        page
    }
    /// Releases queued UI state when the worker or server publishes action progress.
    fn receive_reply(&mut self, revision: u64) {
        if self.reply_revision != revision {
            self.deletion = super::book_delete::Confirmation::default();
            self.message.clear();
            self.reply_revision = revision;
        }
    }
}

use eq_client_core::world::{ClientWorld, SpellTiming};

/// The installed spell data answers the world's questions about spells.
impl eq_client_core::world::SpellCatalog for SpellNames {
    fn timing(&self, spell: u32) -> Option<SpellTiming> {
        Self::timing(self, spell)
    }

    fn instant_effect(&self, spell: u32) -> bool {
        Self::instant_effect(self, spell)
    }
}

#[derive(Resource, Default)]
pub(super) struct SpellNames {
    spells: std::collections::BTreeMap<u32, SpellDefinition>,
}

#[derive(Default)]
struct SpellDefinition {
    mechanics: Option<eq_client_assets::spells::Mechanics>,
    name: Option<String>,
    timing: Option<SpellTiming>,
    icon: Option<u32>,
    mana: Option<u32>,
    cast_ms: Option<u32>,
    range: Option<f32>,
}

impl SpellNames {
    /// Loads labels and timing from the user's installation; assets are never bundled.
    pub fn load(directory: Option<&std::path::Path>) -> Self {
        directory
            .and_then(|path| std::fs::read(path.join("spells_us.txt")).ok())
            .map_or_else(Self::default, |bytes| {
                Self::parse(&String::from_utf8_lossy(&bytes))
            })
    }

    /// Keeps valid names even when a line has unavailable or malformed timing fields.
    pub(super) fn parse(text: &str) -> Self {
        let mut result = Self::default();
        for line in text.lines() {
            let fields: Vec<_> = line.split('^').take(183).collect();
            // EQEmu SPDat field 144 indexes the default UI's spell artwork grid.
            let Some(id) = fields.first().and_then(|field| field.parse::<u32>().ok()) else {
                continue;
            };
            let mut definition = SpellDefinition {
                mechanics: eq_client_assets::spells::Mechanics::from_fields(&fields),
                icon: fields.get(144).and_then(|field| field.parse().ok()),
                mana: fields.get(19).and_then(|field| field.parse().ok()),
                cast_ms: fields.get(13).and_then(|field| field.parse().ok()),
                range: fields
                    .get(9)
                    .and_then(|field| field.parse::<f32>().ok())
                    .filter(|range| range.is_finite() && *range >= 0.0),
                ..Default::default()
            };
            definition.name = fields
                .get(1)
                .map(|name| name.trim())
                .filter(|name| !name.is_empty())
                .map(str::to_owned);
            // EQ spells_us.txt fields 14/15 are recovery and same-spell reuse milliseconds.
            if let (Some(recovery_ms), Some(recast_ms)) = (
                fields.get(14).and_then(|s| s.parse().ok()),
                fields.get(15).and_then(|s| s.parse().ok()),
            ) {
                definition.timing = Some(SpellTiming {
                    recovery_ms,
                    recast_ms,
                });
            }
            result.spells.insert(id, definition);
        }
        result
    }

    pub(super) fn timing(&self, id: u32) -> Option<SpellTiming> {
        self.spells.get(&id).and_then(|spell| spell.timing)
    }

    /// Looks up local mechanics by the server's exact spell ID, never by display name.
    pub(super) fn mechanics(&self, id: u32) -> Option<&eq_client_assets::spells::Mechanics> {
        self.spells.get(&id)?.mechanics.as_ref()
    }

    /// Only an explicit zero duration is an instant effect; unknown rules stay unresolved.
    pub(super) fn instant_effect(&self, id: u32) -> bool {
        self.mechanics(id).is_some_and(|mechanics| {
            mechanics.duration_formula == 0
                && mechanics.duration_cap == 0
                && mechanics
                    .alternate_duration
                    .is_some_and(|alternate| alternate.formula == 0 && alternate.duration == 0)
        })
    }

    /// Unmodified installation mana cost.
    pub(super) fn mana(&self, id: u32) -> Option<u32> {
        self.spells.get(&id).and_then(|spell| spell.mana)
    }

    pub(super) fn icon(&self, id: u32) -> Option<u32> {
        self.spells.get(&id).and_then(|spell| spell.icon)
    }

    pub fn label(&self, id: u32) -> String {
        self.spells
            .get(&id)
            .and_then(|spell| spell.name.clone())
            .unwrap_or_else(|| format!("Spell {id}"))
    }

    /// Unmodified installation values, not server-adjusted costs or cast times.
    pub(super) fn details(&self, id: u32) -> String {
        let Some(spell) = self.spells.get(&id) else {
            return "Local spell details unavailable".into();
        };
        format!(
            "Base: {} mana | {}s cast | {} range",
            spell
                .mana
                .map_or_else(|| "--".into(), |value| value.to_string()),
            spell.cast_ms.map_or_else(
                || "--".into(),
                |value| format!("{:.1}", f64::from(value) / 1000.0)
            ),
            spell
                .range
                .map_or_else(|| "--".into(), |value| format!("{value:.0}"))
        )
    }
}

/// Builds a compact movable book; empty and unavailable books remain distinct.
pub(super) fn spawn(commands: &mut Commands) {
    let frame = super::windows::frame(
        commands,
        super::windows::WindowId::Spellbook,
        Node {
            width: px(310),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(8)),
            row_gap: px(4),
            ..default()
        },
        Color::srgba(0.025, 0.032, 0.04, 0.96),
    );
    commands
        .entity(frame)
        .insert((BookFrame, super::hud::HudRoot));
    commands.entity(frame).with_children(|parent| {
        parent
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            })
            .with_children(spawn_body);
    });
}

/// Keeps refreshed rows inside one collapsible container beneath the title bar.
fn spawn_body(parent: &mut ChildSpawnerCommands) {
    super::book_delete::spawn(parent);
    parent.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        TextColor(Color::srgb(0.78, 0.80, 0.84)),
        BookText,
    ));
    parent
        .spawn((
            Button,
            ScribeCursor,
            crate::outbox::Needs(eq_client_core::Capability::Spellbook),
            Node {
                padding: UiRect::all(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.10, 0.12, 0.16)),
        ))
        .with_child((
            scribe::Label,
            Text::new("Scribe cursor scroll"),
            TextColor(Color::WHITE),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
        ));
    for index in 0..ROWS_PER_PAGE {
        parent
            .spawn((
                Button,
                BookEntry(index),
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                Node {
                    min_height: px(26),
                    padding: UiRect {
                        left: px(32),
                        ..UiRect::all(px(4))
                    },
                    ..default()
                },
                BackgroundColor(Color::srgb(0.07, 0.09, 0.12)),
            ))
            .with_child(super::spell_icons::artwork(
                super::spell_icons::Source::Book(index),
                22.0,
            ));
    }
    gem_choices(parent);
    parent
        .spawn(Node {
            column_gap: px(8),
            ..default()
        })
        .with_children(|row| {
            for (next, label) in [(false, "Previous"), (true, "Next")] {
                row.spawn((
                    Button,
                    PageButton(next),
                    Node {
                        padding: UiRect::axes(px(12), px(6)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.10, 0.12, 0.16)),
                ))
                .with_child((
                    Text::new(label),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                ));
            }
        });
}

/// Browses known spells and requests gem assignments without predicting server state.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn update(
    shown: Res<super::windows::Shown>,
    chat: Res<super::chat::ChatState>,
    names: Res<SpellNames>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut frames: Query<&mut Node, With<BookFrame>>,
    mut text: BookLabels,
    buttons: Query<(&Interaction, &PageButton), Changed<Interaction>>,
    mut state: ResMut<BookView>,
    mut rows: BookRows,
    mut gems: GemChoices,
    online: Option<Res<super::online::OnlineState>>,
    outbox: Option<Res<crate::outbox::Outbox>>,
    mut selection: ResMut<BookSelection>,
    actions: BookActions,
) {
    let (scribe, mut deletion, mut requests) = actions;
    let world = super::online::world(online.as_deref());
    selection.receive_reply(world.book_action_revision());
    let accepts_input = !chat.composing && windows.single().is_ok_and(|window| window.focused);
    let open = shown.is_open(super::windows::WindowId::Spellbook);
    for mut node in &mut frames {
        node.display = if open { Display::Flex } else { Display::None };
    }
    if !open {
        selection.deletion.cancel();
        return;
    }
    let pending = action_pending(world) || selection.deletion.queued;
    if !pending
        && accepts_input
        && scribe
            .iter()
            .any(|interaction| *interaction == Interaction::Pressed)
    {
        selection.message =
            match request_scribe(online.as_deref(), world.spell_book(), outbox.as_deref()) {
                Ok(spell) => {
                    let _ = note(&mut requests, format!("Scribing {}", names.label(spell)));
                    String::new()
                }
                Err(error) => crate::outbox::window_line(&error),
            };
    }
    let entries = known_entries(world.spell_book());
    if selection
        .selected
        .is_some_and(|selected| !entries.iter().any(|(_, id)| *id == selected))
    {
        selection.selected = None;
    }
    let pages = entries.len().div_ceil(ROWS_PER_PAGE).max(1);
    if let Some(page) = selection.relocated_page(&entries) {
        state.page = page;
    }
    turn_page(&buttons, &mut state.page, accepts_input);
    state.page = state.page.min(pages - 1);
    refresh_rows(
        &mut rows,
        &entries,
        state.page,
        &mut selection,
        &names,
        accepts_input,
    );
    delete_input(
        &deletion,
        &mut selection,
        world,
        outbox.as_deref(),
        &names,
        accepts_input && !pending,
    );
    refresh_actions(&mut deletion, &selection.deletion);
    let (requested, gem_hint) = refresh_gems(
        &mut gems,
        world,
        selection.selected,
        &names,
        pending || selection.deletion.queued,
        accepts_input,
    );
    if let Some(gem) = requested {
        let queued = request_memorize(
            online.as_deref(),
            world.spell_book(),
            outbox.as_deref(),
            selection.selected,
            gem,
        );
        selection.message = match queued {
            Ok(spell) => {
                let label = format!("Memorizing {} into gem {}", names.label(spell), gem + 1);
                let _ = note(&mut requests, label);
                String::new()
            }
            Err(error) => crate::outbox::window_line(&error),
        };
    }
    let label = book_label(world, &names, &selection, &entries, state.page, pages);
    refresh_labels(&mut text, &label, gem_hint.as_deref());
}

/// Names the queued book change for the action bar.
fn note(
    requests: &mut Option<ResMut<super::hud::action_bar::ActionRequests>>,
    label: String,
) -> Option<()> {
    requests.as_mut()?.book = Some((std::time::Instant::now(), label));
    Some(())
}

/// Updates book status and hover details without rebuilding the controls.
fn refresh_labels(text: &mut BookLabels, label: &str, gem_hint: Option<&str>) {
    for (mut value, hint) in text.iter_mut() {
        let content = if hint.is_some() {
            gem_hint.unwrap_or("Hover a gem to inspect its current spell")
        } else {
            label
        };
        if value.0 != content {
            value.0 = content.into();
        }
    }
}

/// Applies fresh page clicks only while the book accepts keyboard and pointer input.
fn turn_page(
    buttons: &Query<(&Interaction, &PageButton), Changed<Interaction>>,
    page: &mut usize,
    enabled: bool,
) {
    for (interaction, PageButton(next)) in buttons {
        if enabled && *interaction == Interaction::Pressed {
            *page = if *next {
                page.saturating_add(1)
            } else {
                page.saturating_sub(1)
            };
        }
    }
}

type BookActions<'w, 's> = (
    Query<'w, 's, &'static Interaction, (With<ScribeCursor>, Changed<Interaction>)>,
    DeleteControls<'w, 's>,
    Option<ResMut<'w, super::hud::action_bar::ActionRequests>>,
);

type DeleteControls<'w, 's> = Query<
    'w,
    's,
    (
        Ref<'static, Interaction>,
        &'static super::book_delete::Action,
        &'static mut Node,
    ),
    (Without<BookFrame>, Without<BookEntry>),
>;

/// Replaces editing controls with confirmation controls for the pending selection.
fn refresh_actions(buttons: &mut DeleteControls, confirmation: &super::book_delete::Confirmation) {
    for (_, action, mut node) in buttons.iter_mut() {
        node.display = if confirmation.visible(*action) {
            Display::Flex
        } else {
            Display::None
        };
    }
}

/// Processes deletion confirmation separately from gem assignment and scribing.
fn delete_input(
    buttons: &DeleteControls,
    selection: &mut BookSelection,
    world: &ClientWorld,
    outbox: Option<&crate::outbox::Outbox>,
    names: &SpellNames,
    enabled: bool,
) {
    let session = outbox
        .and_then(|outbox| outbox.peek(world))
        .map(|stamp| stamp.session_id);
    selection
        .deletion
        .validate(session, selection.selected, world.spell_book());
    if !enabled {
        if !selection.deletion.queued {
            selection.deletion = super::book_delete::Confirmation::default();
        }
        return;
    }
    let Some((_, action, _)) = buttons.iter().find(|(interaction, action, _)| {
        interaction.is_changed()
            && **interaction == Interaction::Pressed
            && selection.deletion.visible(**action)
    }) else {
        return;
    };
    let result = selection
        .deletion
        .act(*action, session, selection.selected, world.spell_book());
    selection.message = match result {
        Err(error) => error.to_string(),
        // A change sent says nothing until the server answers; the outbox
        // says why one did not leave.
        Ok(Some(command)) => {
            if outbox.is_some_and(|outbox| outbox.send(world, command).is_ok()) {
                selection.deletion.queued = true;
            }
            String::new()
        }
        Ok(None) => match action {
            super::book_delete::Action::Select => format!(
                "Delete {} from the book? Confirm or Cancel.",
                names.label(selection.selected.unwrap_or(0))
            ),
            _ => "Deletion cancelled".into(),
        },
    };
}

/// Book actions wait for outstanding casts or server assignment confirmation.
fn action_pending(world: &ClientWorld) -> bool {
    world.casting().cast.is_some()
        || world.casting().pending.is_some()
        || matches!(
            world.book_action(),
            Some(
                eq_client_core::BookActionStatus::Preparing
                    | eq_client_core::BookActionStatus::Submitted
                    | eq_client_core::BookActionStatus::AwaitingReply
            )
        )
}

type BookLabels<'w, 's> = Query<
    'w,
    's,
    (&'static mut Text, Option<&'static GemHint>),
    (Or<(With<BookText>, With<GemHint>)>, Without<BookEntry>),
>;

type GemChoices<'w, 's> = Query<
    'w,
    's,
    (
        Ref<'static, Interaction>,
        &'static GemChoice,
        &'static mut BackgroundColor,
    ),
    Without<BookEntry>,
>;

/// Styles current gem occupants and collects at most one fresh assignment click.
fn refresh_gems(
    gems: &mut GemChoices,
    world: &ClientWorld,
    selected: Option<u32>,
    names: &SpellNames,
    pending: bool,
    accepts_input: bool,
) -> (Option<u8>, Option<String>) {
    let mut gem_hint = None;
    let mut requested = None;
    for (interaction, GemChoice(gem), mut background) in gems.iter_mut() {
        let available = !pending
            && selected.is_some()
            && world
                .player()
                .map_or([None; 8], |player| player.memorized_spells)
                .get(usize::from(*gem))
                .copied()
                .flatten()
                != selected;
        background.0 = if !available {
            Color::srgb(0.055, 0.065, 0.08)
        } else if *interaction != Interaction::None {
            Color::srgb(0.20, 0.29, 0.40)
        } else {
            Color::srgb(0.12, 0.16, 0.22)
        };
        if *interaction != Interaction::None {
            gem_hint = Some(gem_description(
                *gem,
                &world
                    .player()
                    .map_or([None; 8], |player| player.memorized_spells),
                selected,
                names,
                pending,
            ));
        }
        if available
            && accepts_input
            && interaction.is_changed()
            && *interaction == Interaction::Pressed
        {
            requested = Some(*gem);
        }
    }
    (requested, gem_hint)
}

/// Applies selection before styling, and removes empty rows from layout and hit testing.
fn refresh_rows(
    rows: &mut BookRows,
    entries: &[(usize, u32)],
    page: usize,
    selection: &mut BookSelection,
    names: &SpellNames,
    accepts_input: bool,
) {
    for (interaction, row, _, _, _) in rows.iter() {
        if accepts_input && interaction.is_changed() && *interaction == Interaction::Pressed {
            selection.selected = entries.get(page * ROWS_PER_PAGE + row.0).map(|(_, id)| *id);
        }
    }
    for (interaction, row, mut value, mut node, mut background) in rows.iter_mut() {
        if let Some((slot, id)) = entries.get(page * ROWS_PER_PAGE + row.0) {
            node.display = Display::Flex;
            value.0 = format!("{} | {}", slot + 1, names.label(*id));
            background.0 = if selection.selected == Some(*id) {
                Color::srgb(0.16, 0.23, 0.32)
            } else if *interaction == Interaction::Hovered {
                Color::srgb(0.10, 0.13, 0.18)
            } else {
                Color::srgb(0.07, 0.09, 0.12)
            };
        } else {
            value.0.clear();
            node.display = Display::None;
        }
    }
}

/// Lists populated slots while preserving their original book indexes.
fn known_entries(book: Option<&eq_client_core::SpellBook>) -> Vec<(usize, u32)> {
    book.map(|book| {
        book.slots()
            .iter()
            .enumerate()
            .filter_map(|(slot, id)| id.map(|id| (slot, id)))
            .collect()
    })
    .unwrap_or_default()
}

fn book_label(
    world: &ClientWorld,
    names: &SpellNames,
    selection: &BookSelection,
    entries: &[(usize, u32)],
    page: usize,
    pages: usize,
) -> String {
    let mut label = if world.spell_book().is_none() {
        "Spellbook unavailable".into()
    } else if entries.is_empty() {
        "No scribed spells".into()
    } else {
        format!("Page {} / {}", page + 1, pages)
    };
    if let Some(id) = selection.selected {
        use std::fmt::Write;
        let _ = write!(label, "\n{} | choose gem 1-8", names.label(id));
        let _ = write!(label, "\n{}", names.details(id));
    }
    if world.casting().pending.is_some() {
        label.push_str("\nWaiting for cast acknowledgement");
    } else if world.casting().cast.is_some() {
        label.push_str("\nFinish or interrupt casting before changing spells");
    } else if !selection.message.is_empty() {
        label.push('\n');
        label.push_str(&selection.message);
    } else if let Some(status) = &world.book_action() {
        use eq_client_core::BookActionStatus;
        use std::fmt::Write;
        label.push('\n');
        match status {
            BookActionStatus::Preparing => label.push_str("Preparing spellbook action..."),
            BookActionStatus::Submitted => label.push_str("Submitted; waiting for server update"),
            BookActionStatus::AwaitingReply => {
                label.push_str("Waiting for matching server reply...");
            }
            BookActionStatus::Confirmed => label.push_str("Server confirmed spellbook action"),
            BookActionStatus::Cancelled(reason) => {
                let _ = write!(label, "Cancelled: {reason}");
            }
            BookActionStatus::Rejected(reason) => {
                let _ = write!(label, "Rejected: {reason}");
            }
        }
    }
    label
}

/// Reserves hover space so buttons never shift beneath the pointer.
fn gem_hint() -> impl Bundle {
    (
        GemHint,
        Text::new("Hover a gem to inspect its current spell"),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(Color::srgb(0.70, 0.76, 0.84)),
        Node {
            height: px(44),
            flex_shrink: 0.0,
            overflow: Overflow::clip(),
            ..default()
        },
    )
}

fn gem_choices(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
            column_gap: px(4),
            ..default()
        })
        .with_children(|row| {
            for gem in 0..8u8 {
                row.spawn((
                    Button,
                    GemChoice(gem),
                    crate::outbox::Needs(eq_client_core::Capability::Spellbook),
                    Text::new(format!("{}", gem + 1)),
                    TextFont {
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    Node {
                        width: px(32),
                        height: px(42),
                        padding: UiRect::top(px(26)),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.12, 0.16, 0.22)),
                ))
                .with_child(super::spell_icons::artwork(
                    super::spell_icons::Source::Gem(usize::from(gem)),
                    24.0,
                ));
            }
        });
    parent.spawn(gem_hint());
}

/// Identifies the authoritative current occupant before requesting a replacement.
fn gem_description(
    gem: u8,
    spells: &[Option<u32>; 8],
    selected: Option<u32>,
    names: &SpellNames,
    pending: bool,
) -> String {
    let current = spells.get(usize::from(gem)).copied().flatten();
    let occupant = current.map_or_else(|| "Empty".into(), |id| names.label(id));
    let action = match (selected, current) {
        _ if pending => "Wait for the current spell action",
        (Some(selected), Some(current)) if selected == current => "Already memorized here",
        (Some(_), Some(_)) => "Click to replace this spell",
        (Some(_), None) => "Click to memorize here",
        (None, _) => "Select a spell first",
    };
    format!("Gem {}: {occupant}\n{action}", gem + 1)
}

/// Validates a gem click and sends it without predicting the resulting spell state.
fn request_memorize(
    online: Option<&super::online::OnlineState>,
    book: Option<&eq_client_core::SpellBook>,
    outbox: Option<&crate::outbox::Outbox>,
    selected: Option<u32>,
    gem: u8,
) -> anyhow::Result<u32> {
    use anyhow::{Context, ensure};
    let spell_id = selected.context("Select a spell first")?;
    ensure!(gem < 8, "Choose a gem from 1 to 8");
    let (online, outbox) = online.zip(outbox).context("Connect to memorize a spell")?;
    ensure!(
        book.context("Spellbook unavailable")?
            .slots()
            .contains(&Some(spell_id)),
        "The selected spell is no longer in the spellbook"
    );
    outbox.post(&online.world, |stamp| {
        eq_client_core::ClientCommand::MemorizeSpell {
            session_id: stamp.session_id,
            gem,
            spell_id,
            created: stamp.created,
        }
    })?;
    Ok(spell_id)
}

/// Sends the current cursor scroll without consuming or inserting it locally.
fn request_scribe(
    online: Option<&super::online::OnlineState>,
    book: Option<&eq_client_core::SpellBook>,
    outbox: Option<&crate::outbox::Outbox>,
) -> anyhow::Result<u32> {
    use anyhow::Context;
    let (state, outbox) = online.zip(outbox).context("Connect to scribe a scroll")?;
    let stamp = outbox.stamp(&state.world)?;
    let command = prepare_scribe(online, book, Some(stamp))?;
    let eq_client_core::ClientCommand::ScribeSpell { spell_id, .. } = command else {
        anyhow::bail!("Scribe request was not a scribe");
    };
    outbox.send(&state.world, command)?;
    Ok(spell_id)
}

/// Shares admission, cursor and book validation between presentation and
/// submission; the stamp is the one the outbox gives a command made now.
fn prepare_scribe(
    online: Option<&super::online::OnlineState>,
    book: Option<&eq_client_core::SpellBook>,
    stamp: Option<crate::outbox::Stamp>,
) -> anyhow::Result<eq_client_core::ClientCommand> {
    use anyhow::{Context, ensure};
    let online = online.context("Connect to scribe a scroll")?;
    let stamp = stamp.context("Connect to scribe a scroll")?;
    let inventory = online.world.inventory();
    ensure!(
        inventory.received() && !inventory.stale(),
        "Inventory awaiting refresh"
    );
    let book = book.context("Spellbook unavailable")?;
    let spell_id = inventory
        .items()
        .get(&eq_client_core::inventory::InventorySlot::CURSOR)
        .context("Put a scroll on the cursor first")?
        .scroll_spell
        .context("The cursor item is not a spell scroll")?;
    let slot = u16::try_from(
        book.slots()
            .iter()
            .position(Option::is_none)
            .context("Spellbook is full")?,
    )?;
    book.scribe_packet(inventory, inventory.revision(), slot, spell_id)?;
    Ok(eq_client_core::ClientCommand::ScribeSpell {
        session_id: stamp.session_id,
        revision: inventory.revision(),
        slot,
        spell_id,
        created: stamp.created,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn same_name_variants_keep_distinct_mechanics_by_spell_id() {
        let mut fields = vec!["0"; 183];
        fields[0] = "42";
        fields[1] = "Synthetic shared name";
        fields[16] = "11";
        fields[17] = "270";
        fields[181] = "7";
        fields[182] = "13";
        let first = fields.join("^");
        fields[0] = "84";
        fields[16] = "5";
        fields[182] = "21";
        let names = super::SpellNames::parse(&format!("{first}\n{}", fields.join("^")));
        assert_eq!(names.label(42), names.label(84));
        assert_eq!(
            names.mechanics(42).unwrap().base_duration(1),
            Some(eq_client_assets::spells::BaseDuration::Ticks(120))
        );
        assert_eq!(
            names.mechanics(84).unwrap().base_duration(1),
            Some(eq_client_assets::spells::BaseDuration::Ticks(2))
        );
        assert!(names.mechanics(123).is_none());
        assert_eq!(
            names
                .mechanics(42)
                .unwrap()
                .alternate_duration
                .unwrap()
                .duration,
            13
        );
        assert_eq!(
            names
                .mechanics(84)
                .unwrap()
                .alternate_duration
                .unwrap()
                .duration,
            21
        );
    }
    use super::*;

    #[test]
    fn gem_hints_show_current_occupant_replacement_and_busy_state() {
        let names = SpellNames::parse("42^Example spell\n43^Replacement spell");
        let mut spells = [None; 8];
        assert_eq!(
            gem_description(2, &spells, Some(42), &names, false),
            "Gem 3: Empty\nClick to memorize here"
        );
        spells[2] = Some(42);
        assert_eq!(
            gem_description(2, &spells, Some(43), &names, false),
            "Gem 3: Example spell\nClick to replace this spell"
        );
        assert!(gem_description(2, &spells, Some(42), &names, false).contains("Already memorized"));
        assert!(
            gem_description(2, &spells, Some(43), &names, true)
                .contains("Wait for the current spell action")
        );
        spells[2] = Some(999);
        assert!(gem_description(2, &spells, None, &names, false).contains("Spell 999"));
    }

    #[test]
    fn spell_details_distinguish_missing_values_from_zero_and_replace_whole_records() {
        let mut fields = vec!["0"; 145];
        fields[0] = "73";
        fields[1] = "Synthetic spell";
        fields[9] = "100";
        fields[13] = "2500";
        fields[19] = "20";
        fields[144] = "36";
        let valid = fields.join("^");
        let data = SpellNames::parse(&valid);
        assert_eq!(data.details(73), "Base: 20 mana | 2.5s cast | 100 range");
        fields[9] = "NaN";
        fields[13] = "-1";
        fields[19] = "bad";
        let data = SpellNames::parse(&fields.join("^"));
        assert_eq!(data.details(73), "Base: -- mana | --s cast | -- range");
        assert_eq!(data.label(73), "Synthetic spell");
        fields[9] = "0";
        fields[13] = "0";
        fields[19] = "0";
        assert_eq!(
            SpellNames::parse(&fields.join("^")).details(73),
            "Base: 0 mana | 0.0s cast | 0 range"
        );
        let replaced = SpellNames::parse(&format!("{valid}\n73^Replacement"));
        assert_eq!(replaced.label(73), "Replacement");
        assert!(replaced.icon(73).is_none());
        assert!(replaced.timing(73).is_none());
        assert!(replaced.details(73).contains("-- mana"));
    }

    #[test]
    fn scribe_control_queues_current_scroll_without_consuming_it() {
        use eq_client_core::inventory::{
            InventoryItem, InventorySlot, InventoryUpdate, ItemPlacement,
        };
        let details = eq_client_core::ItemDetails {
            equipment: None,
            bonuses: None,
            id: 301,
            name: "Synthetic scroll".into(),
            lore: String::new(),
            weight_tenths: 1,
            slots: 0,
            classes: 2,
            races: 1,
            flags: Vec::new(),
            stats: Vec::new(),
        };
        let book = eq_client_core::SpellBook::titanium_profile(&vec![0; 19592]).unwrap();
        let mut online = super::super::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 7, crate::online::testing::player(1));
        crate::online::testing::inventory(
            &mut online,
            InventoryUpdate::Snapshot(vec![InventoryItem {
                activation: eq_client_core::inventory::ItemActivation::default(),
                scroll_spell: Some(73),
                rules: ItemPlacement::default(),
                slot: InventorySlot::CURSOR,
                details,
                icon: 0,
                stack_count: None,
                charges: 1,
                bag_slots: 0,
            }]),
        );
        let before = online.world.inventory().clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let sender = crate::outbox::Outbox::new(Some(tx));
        request_scribe(Some(&online), Some(&book), Some(&sender)).unwrap();
        assert!(
            matches!(rx.try_recv().unwrap(), eq_client_core::ClientCommand::ScribeSpell { session_id: 7, slot: 0, spell_id: 73, revision, .. } if revision == before.revision())
        );
        assert_eq!(online.world.inventory(), &before);
        crate::online::testing::connect(&mut online, false);
        assert!(request_scribe(Some(&online), Some(&book), Some(&sender)).is_err());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn selected_spell_follows_confirmed_reordering_but_not_manual_browsing() {
        let mut entries: Vec<_> = (0..16)
            .map(|slot| (slot, u32::try_from(slot + 1).unwrap()))
            .collect();
        let mut selection = BookSelection {
            selected: Some(8),
            ..default()
        };
        assert_eq!(selection.relocated_page(&entries), None);
        // A request alone does not change entries or force a page change.
        assert_eq!(selection.relocated_page(&entries), None);
        entries[7].1 = 9;
        entries[8].1 = 8;
        assert_eq!(selection.relocated_page(&entries), Some(1));
        assert_eq!(selection.relocated_page(&entries), None);
        entries[7].1 = 8;
        entries[8].1 = 9;
        assert_eq!(selection.relocated_page(&entries), Some(0));
        selection.selected = Some(16);
        assert_eq!(selection.relocated_page(&entries), None);
        entries.retain(|(_, id)| *id != 16);
        assert_eq!(selection.relocated_page(&entries), None);
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "Keep the ordered integration scenario and its assertions together"
    )]
    fn deletion_buttons_require_confirmation_and_keep_queued_state_when_closed() {
        use crate::book_delete::Action;
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 7, crate::online::testing::player(1));
        let mut book = eq_client_core::SpellBook::default();
        book.apply(&eq_client_core::SpellUpdate::Slot {
            slot: 3,
            spell_id: 42,
            mode: 0,
        });
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::chat::ChatState>()
            .init_resource::<SpellNames>()
            .insert_resource({
                let mut online = online;
                crate::online::testing::book(&mut online, book.clone());
                online
            })
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .init_resource::<crate::hud::HudState>()
            .init_resource::<BookSelection>()
            .insert_resource(BookView { page: 0 })
            .insert_resource({
                let mut shown = crate::windows::Shown::default();
                shown.open(crate::windows::WindowId::Spellbook);
                shown
            })
            .add_systems(Startup, |mut commands: Commands| spawn(&mut commands))
            .add_systems(Update, update);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    focused: true,
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.update();
        let world = app.world_mut();
        let row = world
            .query::<(Entity, &BookEntry)>()
            .iter(world)
            .find(|(_, entry)| entry.0 == 0)
            .unwrap()
            .0;
        let click = |app: &mut App, entity: Entity| {
            app.world_mut()
                .entity_mut(entity)
                .insert(Interaction::Pressed);
            app.update();
            app.world_mut().entity_mut(entity).insert(Interaction::None);
            app.update();
        };
        let buttons: Vec<_> = world
            .query::<(Entity, &Action)>()
            .iter(world)
            .map(|(entity, action)| (entity, *action))
            .collect();
        let button = |action| {
            buttons
                .iter()
                .find(|(_, value)| *value == action)
                .unwrap()
                .0
        };
        click(&mut app, row);
        assert_eq!(
            app.world()
                .get::<Node>(button(Action::Confirm))
                .unwrap()
                .display,
            Display::None
        );
        click(&mut app, button(Action::Confirm));
        assert!(receiver.try_recv().is_err());
        click(&mut app, button(Action::Select));
        assert_eq!(
            app.world()
                .get::<Node>(button(Action::Confirm))
                .unwrap()
                .display,
            Display::Flex
        );
        assert_eq!(
            app.world()
                .get::<Node>(button(Action::Earlier))
                .unwrap()
                .display,
            Display::None
        );
        click(&mut app, button(Action::Cancel));
        assert_eq!(
            app.world()
                .get::<Node>(button(Action::Select))
                .unwrap()
                .display,
            Display::Flex
        );
        click(&mut app, button(Action::Confirm));
        assert!(receiver.try_recv().is_err());
        click(&mut app, button(Action::Select));
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        click(&mut app, button(Action::Confirm));
        assert!(receiver.try_recv().is_err());
        click(&mut app, button(Action::Select));
        click(&mut app, button(Action::Confirm));
        assert!(matches!(
            receiver.try_recv().unwrap(),
            eq_client_core::ClientCommand::DeleteSpell {
                session_id: 7,
                slot: 3,
                spell_id: 42,
                ..
            }
        ));
        app.world_mut()
            .resource_mut::<crate::windows::Shown>()
            .close(crate::windows::WindowId::Spellbook);
        app.update();
        app.world_mut()
            .resource_mut::<crate::windows::Shown>()
            .open(crate::windows::WindowId::Spellbook);
        click(&mut app, button(Action::Select));
        click(&mut app, button(Action::Confirm));
        assert!(receiver.try_recv().is_err());
        assert_eq!(
            app.world()
                .resource::<crate::online::OnlineState>()
                .world
                .spell_book(),
            Some(&book)
        );
    }

    #[test]
    fn minimized_book_stays_collapsed_while_spell_rows_refresh() {
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<super::super::windows::DragState>()
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<super::super::hud::HudState>()
            .insert_resource(crate::online::OnlineState::new(false))
            .init_resource::<SpellNames>()
            .init_resource::<BookSelection>()
            .insert_resource(BookView { page: 0 })
            .insert_resource({
                let mut shown = crate::windows::Shown::default();
                shown.open(crate::windows::WindowId::Spellbook);
                shown
            })
            .add_systems(Startup, |mut commands: Commands| spawn(&mut commands))
            .add_systems(Update, (super::super::windows::input, update).chain());
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        app.update();
        let world = app.world_mut();
        let button = world
            .query_filtered::<Entity, With<super::super::windows::Minimize>>()
            .single(world)
            .unwrap();
        let row = world
            .query::<(Entity, &BookEntry)>()
            .iter(world)
            .find(|(_, entry)| entry.0 == 0)
            .unwrap()
            .0;
        let body = world.get::<ChildOf>(row).unwrap().parent();
        world.entity_mut(button).insert(Interaction::Pressed);
        app.update();
        let mut book = eq_client_core::SpellBook::default();
        book.apply(&eq_client_core::SpellUpdate::Slot {
            slot: 0,
            spell_id: 42,
            mode: 0,
        });
        crate::online::testing::book(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            book,
        );
        app.update();
        assert_eq!(app.world().get::<Node>(row).unwrap().display, Display::Flex);
        assert_eq!(
            app.world().get::<Node>(body).unwrap().display,
            Display::None
        );
        app.world_mut().entity_mut(button).insert(Interaction::None);
        app.update();
        app.world_mut()
            .entity_mut(button)
            .insert(Interaction::Pressed);
        app.update();
        assert_eq!(
            app.world().get::<Node>(body).unwrap().display,
            Display::Flex
        );
        assert_eq!(app.world().get::<Text>(row).unwrap().0, "1 | Spell 42");
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "Keep the ordered integration scenario and its assertions together"
    )]
    fn opening_book_shows_server_slots_and_chat_typing_does_not_toggle_it() {
        let mut profile = vec![0; 19592];
        profile[2312..2316].copy_from_slice(&42u32.to_le_bytes());
        let book = eq_client_core::SpellBook::titanium_profile(&profile).unwrap();
        let mut app = App::new();
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<BookView>()
            .init_resource::<BookSelection>()
            .init_resource::<super::super::chat::ChatState>()
            .init_resource::<super::super::hud::HudState>()
            .insert_resource({
                let mut online = crate::online::OnlineState::new(false);
                crate::online::testing::book(&mut online, book.clone());
                online
            })
            .insert_resource(SpellNames::parse("42^Example spell"))
            .init_resource::<crate::windows::Shown>()
            .init_resource::<crate::windows::Stack>()
            .init_resource::<crate::escape::Escape>()
            .add_systems(Update, (crate::windows::toggle, update).chain());
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            PrimaryWindow,
        ));
        let frame = app.world_mut().spawn((Node::default(), BookFrame)).id();
        let label = app.world_mut().spawn((Text::new(""), BookText)).id();
        let hint = app.world_mut().spawn((Text::new(""), GemHint)).id();
        let empty = app
            .world_mut()
            .spawn((
                Text::new(""),
                BookEntry(1),
                Interaction::None,
                Node::default(),
                BackgroundColor::default(),
            ))
            .id();
        let text = app
            .world_mut()
            .spawn((
                Text::new(""),
                BookEntry(0),
                Interaction::None,
                Node::default(),
                BackgroundColor::default(),
            ))
            .id();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyB);
        app.update();
        assert_eq!(
            app.world().get::<Node>(frame).unwrap().display,
            Display::Flex
        );
        assert!(
            app.world()
                .get::<Text>(text)
                .unwrap()
                .0
                .contains("Example spell")
        );
        assert_eq!(
            app.world().get::<Node>(empty).unwrap().display,
            Display::None
        );
        app.world_mut()
            .resource_mut::<super::super::chat::ChatState>()
            .composing = true;
        app.update();
        assert_eq!(
            app.world().get::<Node>(frame).unwrap().display,
            Display::Flex
        );
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.world_mut()
            .resource_mut::<super::super::chat::ChatState>()
            .composing = false;
        let mut online = super::super::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 7, crate::online::testing::player(1));
        // The admission brings its book along.
        crate::online::testing::book(&mut online, book);
        let (tx, rx) = std::sync::mpsc::sync_channel(2);
        app.insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(Some(tx)));
        let gem = app
            .world_mut()
            .spawn((GemChoice(2), Interaction::None, BackgroundColor::default()))
            .id();
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::Pressed;
        app.update();
        assert!(
            app.world()
                .get::<Text>(hint)
                .unwrap()
                .0
                .contains("Select a spell first")
        );
        assert!(rx.try_recv().is_err());
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::None;
        crate::online::testing::book_action(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            eq_client_core::BookActionStatus::Rejected("Example rejection".into()),
        );
        *app.world_mut().get_mut::<Interaction>(text).unwrap() = Interaction::Pressed;
        app.update();
        assert_eq!(
            app.world().get::<BackgroundColor>(text).unwrap().0,
            Color::srgb(0.16, 0.23, 0.32)
        );
        app.world_mut()
            .resource_mut::<super::super::chat::ChatState>()
            .composing = true;
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::Pressed;
        app.update();
        assert!(rx.try_recv().is_err());
        app.world_mut()
            .resource_mut::<super::super::chat::ChatState>()
            .composing = false;
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::None;
        app.update();
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::Pressed;
        app.update();
        assert!(rx.try_recv().is_err());
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        // Returning focus does not replay a previously held click.
        app.update();
        assert!(rx.try_recv().is_err());
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::None;
        app.update();
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::Pressed;
        app.update();
        assert!(matches!(
            rx.try_recv().unwrap(),
            eq_client_core::ClientCommand::MemorizeSpell {
                session_id: 7,
                gem: 2,
                spell_id: 42,
                ..
            }
        ));
        // A request sent says nothing until the server answers.
        assert!(app.world().resource::<BookSelection>().message.is_empty());
        // Even an identical rejection is a new reply, and shows.
        crate::online::testing::book_action(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            eq_client_core::BookActionStatus::Rejected("Example rejection".into()),
        );
        app.update();
        assert!(
            app.world()
                .get::<Text>(label)
                .unwrap()
                .0
                .contains("Rejected: Example rejection")
        );
        crate::online::testing::book_action(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            eq_client_core::BookActionStatus::Preparing,
        );
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::None;
        app.update();
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::Pressed;
        app.update();
        assert!(rx.try_recv().is_err());
        crate::online::testing::book_action(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            eq_client_core::BookActionStatus::Cancelled("Server relocated character".into()),
        );
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::None;
        app.update();
        assert!(
            app.world()
                .get::<Text>(label)
                .unwrap()
                .0
                .contains("Server relocated character")
        );
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::Pressed;
        app.update();
        assert!(matches!(
            rx.try_recv().unwrap(),
            eq_client_core::ClientCommand::MemorizeSpell {
                session_id: 7,
                gem: 2,
                spell_id: 42,
                ..
            }
        ));
        {
            let mut online = app.world_mut().resource_mut::<crate::online::OnlineState>();
            crate::online::testing::book_action(
                &mut online,
                eq_client_core::BookActionStatus::Confirmed,
            );
            crate::online::testing::spell(
                &mut online,
                eq_client_core::SpellUpdate::Slot {
                    slot: 2,
                    spell_id: 42,
                    mode: 1,
                },
            );
        }
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::None;
        app.update();
        *app.world_mut().get_mut::<Interaction>(gem).unwrap() = Interaction::Pressed;
        app.update();
        assert!(rx.try_recv().is_err());
        assert!(
            app.world()
                .get::<Text>(hint)
                .unwrap()
                .0
                .contains("Already memorized here")
        );
    }
}
