//! Offline character panel and empty action slots, ready for session data.

pub(super) mod action_bar;
mod cooldowns;
pub(crate) mod hotbar;
pub(crate) mod messages;
mod requests;
#[cfg(test)]
mod tests;

use bevy::prelude::*;

/// The player's own HP as the server last reported it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ReportedHp {
    pub current: i32,
    pub maximum: i32,
    /// Both leave out what equipped items add, which the client adds back.
    pub without_items: bool,
}

#[derive(Resource, Default)]
pub(super) struct HudState {
    /// Shared gameplay buff state; presentation never assigns server slots.
    pub buff_state: eq_client_core::buffs::BuffTracker,
    pub(super) cooldowns: cooldowns::Cooldowns,
    pub status: String,
    pub hp: Option<(u32, u32)>,
    pub hp_percent: Option<u8>,
    /// The player's last HP report, which `hp` shows.
    pub reported_hp: Option<ReportedHp>,
    /// HP the equipped items add, as last calculated.
    pub item_hp: Option<i64>,
    pub mana: Option<u32>,
    pub endurance: Option<u32>,
    /// Calculated, unverified maxima (mana, endurance), never server-reported values.
    pub resource_estimate: Option<(u32, u32)>,
    pub experience: Option<u32>,
    pub spells: [Option<u32>; 8],
    pub casting: Option<(u16, std::time::Instant, std::time::Duration)>,
    pub pending_cast: Option<u32>,
    pub action_feedback: Option<(std::time::Instant, String)>,
    pub interrupted: Option<(std::time::Instant, u32)>,
    pub spell_book: Option<eq_client_core::SpellBook>,
    pub book_action: Option<eq_client_core::BookActionStatus>,
    /// Distinguishes consecutive worker replies, including identical rejections.
    pub book_action_revision: u64,
}

impl HudState {
    /// Shows the last HP report, adding back what equipped items give when the
    /// report leaves it out (unknown item HP counts as none), and returns the
    /// percentage shown. A dying character shows zero.
    pub(super) fn show_hp(&mut self) -> Option<u8> {
        let report = self.reported_hp?;
        let items = if report.without_items {
            self.item_hp.unwrap_or(0)
        } else {
            0
        };
        let current = i64::from(report.current) + items;
        let maximum = i64::from(report.maximum) + items;
        let shown = |value: i64| u32::try_from(value.max(0)).unwrap_or(u32::MAX);
        self.hp = Some((shown(current), shown(maximum)));
        let percent = (maximum > 0).then(|| {
            u8::try_from(current.clamp(0, maximum) * 100 / maximum)
                .expect("percentage is bounded to 100")
        })?;
        self.hp_percent = Some(percent);
        Some(percent)
    }

    /// Keeps potentially lasting effects; explicit server buff slots remain authoritative.
    pub(super) fn spell_effect(
        &mut self,
        effect: eq_client_core::SpellEffect,
        names: Option<&super::spellbook::SpellNames>,
    ) {
        if effect.effect_flag != 4 || matches!(effect.spell_id, 0 | u16::MAX) {
            return;
        }
        if names.is_some_and(|names| names.instant_effect(u32::from(effect.spell_id))) {
            return;
        }
        self.buff_state.observe_effect(effect);
    }

    /// Reconciles explicit slots and fades without assigning slots to action-only effects.
    pub(super) fn buff_update(&mut self, update: eq_client_core::BuffUpdate) {
        self.buff_state.apply(update);
    }

    /// Restores spell reuse independently of local asset availability.
    pub(super) fn restore_cooldowns(
        &mut self,
        player: &eq_client_core::PlayerState,
        now: std::time::Instant,
    ) {
        self.pending_cast = None;
        self.action_feedback = None;
        self.cooldowns
            .restore(&player.memorized_spells, player.spell_refresh_ms, now);
    }
    /// Discards old-admission reuse timers and unprocessed refreshes.
    pub(super) fn reset_cooldowns(&mut self) {
        self.pending_cast = None;
        self.action_feedback = None;
        self.cooldowns = cooldowns::Cooldowns::default();
    }
    /// Applies own-caster notifications; resource updates alone never claim a successful cast.
    pub(super) fn cast_update(
        &mut self,
        own_id: u16,
        update: &eq_client_core::SpellUpdate,
        now: std::time::Instant,
    ) {
        use eq_client_core::SpellUpdate;
        match *update {
            SpellUpdate::BarRefresh {
                slot,
                spell_id,
                reduction_ms,
            } if usize::try_from(slot)
                .ok()
                .and_then(|slot| self.spells.get(slot))
                == Some(&Some(spell_id)) =>
            {
                self.cooldowns.refresh(spell_id, reduction_ms, now);
                self.action_feedback = None;
                if self
                    .casting
                    .is_some_and(|(active, _, _)| u32::from(active) == spell_id)
                {
                    self.casting = None;
                }
            }
            SpellUpdate::Began {
                caster_id,
                spell_id,
                duration_ms,
            } if caster_id == own_id => {
                self.action_feedback = None;
                self.interrupted = None;
                self.casting = Some((
                    spell_id,
                    now,
                    std::time::Duration::from_millis(u64::from(duration_ms)),
                ));
            }
            SpellUpdate::Interrupted {
                caster_id,
                message_id,
            } if caster_id == u32::from(own_id) => {
                self.action_feedback = None;
                self.casting = None;
                self.interrupted = Some((now, message_id));
            }
            SpellUpdate::Mana {
                spell_id,
                keep_casting: false,
            } if self
                .casting
                .is_some_and(|(active, _, _)| u32::from(active) == spell_id) =>
            {
                self.casting = None;
            }
            _ => (),
        }
    }
}

#[derive(Component)]
pub(super) struct HudRoot;
#[derive(Component)]
pub(super) struct SpellGem(u8);
#[derive(Component)]
pub(super) struct SpellDetails;
#[derive(Component, Clone, Copy)]
pub(super) enum HudLabel {
    Stat(&'static str),
    Status,
    Spell(usize),
    Casting,
}

#[derive(Component)]
pub(super) struct HudFill(&'static str);

/// Refreshes actual values without inventing unknown resource maxima.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn update(
    mut state: ResMut<HudState>,
    names: Res<super::spellbook::SpellNames>,
    messages: Res<messages::Messages>,
    mut texts: Query<(&mut Text, &HudLabel)>,
    mut fills: Query<(&mut Node, &HudFill)>,
) {
    let now = std::time::Instant::now();
    state.cooldowns.resolve(&names, now);
    for (mut text, label) in &mut texts {
        let value = match label {
            HudLabel::Casting => state.casting.map_or_else(
                || {
                    if let Some(spell) = state.pending_cast {
                        format!("Awaiting cast acknowledgement | {}", names.label(spell))
                    } else if let Some((_, reason)) = state
                        .interrupted
                        .filter(|(at, _)| at.elapsed() < std::time::Duration::from_secs(3))
                    {
                        messages.interruption(reason)
                    } else {
                        String::new()
                    }
                },
                |(spell, started, duration)| {
                    let elapsed = started.elapsed();
                    if elapsed < duration {
                        format!(
                            "Casting {} | {:.1}s",
                            names.label(u32::from(spell)),
                            duration.saturating_sub(elapsed).as_secs_f32()
                        )
                    } else {
                        "Awaiting cast result".into()
                    }
                },
            ),
            HudLabel::Status => {
                if state.status.is_empty() {
                    "CHARACTER / OFFLINE".into()
                } else {
                    state.status.to_uppercase()
                }
            }
            HudLabel::Stat(stat) => match *stat {
                "HP" => state
                    .hp
                    .map(|(a, b)| format!("{a}/{b}"))
                    .or_else(|| state.hp_percent.map(|p| format!("{p}%"))),
                "MANA" => state
                    .mana
                    .map(|v| resource_label(v, state.resource_estimate.map(|v| v.0))),
                "STAMINA" => state
                    .endurance
                    .map(|v| resource_label(v, state.resource_estimate.map(|v| v.1))),
                "EXP" => state
                    .experience
                    .map(|v| format!("{:.1}%", f64::from(v) / 3.3)),
                _ => None,
            }
            .unwrap_or_else(|| "--".into()),
            HudLabel::Spell(index) => state.spells[*index].map_or_else(String::new, |id| {
                let remaining = state.cooldowns.remaining(id, now);
                if remaining.is_zero() {
                    format!("{id}")
                } else {
                    format!("{:.1}s", remaining.as_secs_f32())
                }
            }),
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    for (mut node, HudFill(stat)) in &mut fills {
        let ratio = match *stat {
            "MANA" => resource_ratio(state.mana, state.resource_estimate.map(|v| v.0)),
            "STAMINA" => resource_ratio(state.endurance, state.resource_estimate.map(|v| v.1)),
            "HP" => state
                .hp
                .filter(|(_, max)| *max > 0)
                .map(|(value, max)| f64::from(value) / f64::from(max))
                .or_else(|| state.hp_percent.map(|v| f64::from(v) / 100.0)),
            "EXP" => state.experience.map(|v| f64::from(v) / 330.0),
            _ => None,
        }
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
        #[allow(clippy::cast_possible_truncation)] // Ratio is bounded to 0..1.
        let width = (ratio * 100.0) as f32;
        node.width = percent(width);
    }
}

/// Marks calculated maxima in the same place that displays the current server value.
fn resource_label(current: u32, maximum: Option<u32>) -> String {
    maximum.map_or_else(
        || format!("{current} / ?"),
        |max| format!("{current} / ~{max}"),
    )
}

fn resource_ratio(current: Option<u32>, maximum: Option<u32>) -> Option<f64> {
    current
        .zip(maximum.filter(|max| *max != 0))
        .map(|(value, max)| f64::from(value) / f64::from(max))
}

const PANEL: Color = Color::srgba(0.025, 0.032, 0.04, 0.90);
const EDGE: Color = Color::srgb(0.23, 0.25, 0.26);
const INK: Color = Color::srgb(0.72, 0.75, 0.77);

/// Builds the bottom HUD without inventing character statistics or abilities.
pub(super) fn spawn(commands: &mut Commands) {
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                right: px(20),
                bottom: px(16),
                justify_content: JustifyContent::FlexEnd,
                align_items: AlignItems::FlexEnd,
                column_gap: px(8),
                ..default()
            },
            GlobalZIndex(10),
            HudRoot,
        ))
        .id();

    super::chat::spawn(commands);
    super::target::spawn(commands);
    super::items::spawn(commands);
    super::inventory::spawn(commands);
    super::spellbook::spawn(commands);
    action_bar::spawn(commands);

    let character = panel(commands, root, 186.0);
    super::windows::identify(commands, character, "CHARACTER");
    let status = label(commands, character, "CHARACTER / OFFLINE", 10.0, INK);
    commands.entity(status).insert(HudLabel::Status);
    for (name, tint) in [
        ("HP", Color::srgb(0.60, 0.20, 0.20)),
        ("MANA", Color::srgb(0.20, 0.36, 0.70)),
        ("STAMINA", Color::srgb(0.65, 0.49, 0.18)),
        ("EXP", Color::srgb(0.30, 0.52, 0.36)),
    ] {
        stat_bar(commands, character, name, tint);
    }

    hotbar::spawn(commands, root);

    let spells = panel(commands, root, 358.0);
    super::windows::titled(commands, spells, "SPELLS");
    let gems = row(commands, spells, 4.0);
    let casting = label(commands, spells, "", 10.0, INK);
    commands.entity(casting).insert(HudLabel::Casting);
    for number in 1..=8 {
        let slot = slot(commands, gems, &number.to_string(), 38.0, true);
        commands
            .entity(slot)
            .with_child(super::spell_icons::artwork(
                super::spell_icons::Source::Gem(number - 1),
                30.0,
            ));
        commands.entity(slot).insert((
            Button,
            SpellGem(u8::try_from(number - 1).expect("eight gems")),
        ));
        let value = label(commands, slot, "", 8.0, INK);
        commands.entity(value).insert((
            HudLabel::Spell(number - 1),
            Node {
                position_type: PositionType::Absolute,
                bottom: px(1),
                right: px(2),
                ..default()
            },
        ));
    }
    let details = label(
        commands,
        spells,
        "Click: cast | Shift-click: forget\nAlt+1-8 | X sit | C duck | V stand",
        10.0,
        Color::srgb(0.43, 0.48, 0.53),
    );
    commands.entity(details).insert((
        SpellDetails,
        Node {
            // Hover text must not move the gems away from the pointer.
            height: px(64),
            min_height: px(64),
            flex_shrink: 0.0,
            ..default()
        },
    ));
}

/// Shows current gem identity and local timing without retaining stale hover state.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn spell_details(
    state: Res<HudState>,
    names: Res<super::spellbook::SpellNames>,
    mut gems: Query<(&Interaction, &SpellGem, &mut BackgroundColor)>,
    mut labels: Query<&mut Text, With<SpellDetails>>,
) {
    let now = std::time::Instant::now();
    let mut hovered = None;
    for (interaction, SpellGem(gem), mut background) in &mut gems {
        let spell = state.spells.get(usize::from(*gem)).copied().flatten();
        let waiting = state.casting.is_some()
            || state.pending_cast.is_some()
            || spell.is_some_and(|id| !state.cooldowns.remaining(id, now).is_zero());
        background.0 = if *interaction != Interaction::None {
            hovered = Some((*gem, spell));
            Color::srgb(0.18, 0.25, 0.32)
        } else if spell.is_none() {
            Color::srgb(0.04, 0.05, 0.06)
        } else if waiting {
            Color::srgb(0.20, 0.14, 0.08)
        } else {
            Color::srgb(0.09, 0.16, 0.22)
        };
    }
    let mut text = match hovered {
        None => "Hover a gem for spell details\nClick: cast | Shift-click: forget\nAlt+1-8 | X sit | C duck | V stand".into(),
        Some((gem, None)) => format!("Gem {} is empty\nOpen spellbook [B] to memorize", gem + 1),
        Some((gem, Some(spell))) => {
            let mut text = format!("{} | Alt+{}", names.label(spell), gem + 1);
            text.push('\n');
            text.push_str(&names.details(spell));
            if let Some(timing) = names.timing(spell) {
                use std::fmt::Write;
                let _ = write!(text, "\nBase reuse {:.1}s | recovery {:.1}s", f64::from(timing.recast_ms) / 1000.0, f64::from(timing.recovery_ms) / 1000.0);
            } else {
                text.push_str("\nLocal spell timing unavailable");
            }
            let remaining = state.cooldowns.remaining(spell, now);
            if !remaining.is_zero() {
                use std::fmt::Write;
                let _ = write!(text, "\nAvailable in {:.1}s", remaining.as_secs_f32());
            }
            text
        }
    };
    if let Some((created, message)) = &state.action_feedback
        && now.saturating_duration_since(*created) < std::time::Duration::from_secs(3)
    {
        text.clone_from(message);
    }
    for mut label in &mut labels {
        if label.0 != text {
            label.0.clone_from(&text);
        }
    }
}

/// Sends fresh admitted spell/posture requests; server events determine results.
#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub(super) fn actions(
    keys: Res<ButtonInput<KeyCode>>,
    mut hud: ResMut<HudState>,
    online: Res<super::online::OnlineState>,
    chat: Res<super::chat::ChatState>,
    target: Res<super::target::TargetState>,
    sender: Res<super::target::CommandsToServer>,
    clicks: Query<(&Interaction, &SpellGem), Changed<Interaction>>,
    bar_clicks: Query<(&Interaction, &hotbar::Slot), Changed<Interaction>>,
    bindings: Res<hotbar::Bindings>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    definitions: (Res<super::spellbook::SpellNames>, Res<messages::Messages>),
) {
    if !online.world.connected()
        || online.world.death().is_some()
        || chat.composing
        || keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
        || !windows.single().is_ok_and(|window| window.focused)
    {
        return;
    }
    let (Some(session_id), Some(player), Some(sender)) = (
        online.world.session_id(),
        online.world.player(),
        sender.0.as_ref(),
    ) else {
        return;
    };
    let gem = clicks
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
        .map(|(_, gem)| gem.0)
        .or_else(|| {
            if !keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) {
                return None;
            }
            [
                KeyCode::Digit1,
                KeyCode::Digit2,
                KeyCode::Digit3,
                KeyCode::Digit4,
                KeyCode::Digit5,
                KeyCode::Digit6,
                KeyCode::Digit7,
                KeyCode::Digit8,
            ]
            .iter()
            .position(|key| keys.just_pressed(*key))
            .and_then(|index| u8::try_from(index).ok())
        });
    let bar_action = hotbar::requested(&keys, &bindings, &bar_clicks);
    let forgetting = gem.is_some() && keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let gem = gem.or(match bar_action {
        Some(hotbar::Action::Gem(gem)) => Some(gem),
        _ => None,
    });
    if let Some(gem) = gem {
        let (names, messages) = definitions;
        let mana_cost = player
            .memorized_spells
            .get(usize::from(gem))
            .copied()
            .flatten()
            .and_then(|spell| names.mana(spell));
        requests::spell(
            &mut hud,
            player,
            sender,
            &requests::Request {
                session_id,
                gem,
                target_id: target.selected.unwrap_or(player.spawn_id),
                forgetting,
                mana_cost,
            },
            &messages,
        );
    }
    let posture = if keys.just_pressed(KeyCode::KeyC) {
        Some(eq_client_core::Posture::Ducking)
    } else if keys.just_pressed(KeyCode::KeyX) {
        Some(eq_client_core::Posture::Sitting)
    } else if keys.just_pressed(KeyCode::KeyV) {
        Some(eq_client_core::Posture::Standing)
    } else {
        match bar_action {
            Some(hotbar::Action::Sit) => Some(eq_client_core::Posture::Sitting),
            Some(hotbar::Action::Stand) => Some(eq_client_core::Posture::Standing),
            _ => None,
        }
    };
    if let Some(posture) = posture {
        let result = sender.try_send(eq_client_core::ClientCommand::SetPosture {
            session_id,
            spawn_id: player.spawn_id,
            posture,
            created: std::time::Instant::now(),
        });
        if result.is_err() {
            warn!("Posture request queue unavailable");
        }
    }
}

/// Creates a consistently framed HUD group.
fn panel(commands: &mut Commands, parent: Entity, width: f32) -> Entity {
    let entity = commands
        .spawn((
            Node {
                width: px(width),
                padding: UiRect::all(px(10)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(EDGE),
        ))
        .id();
    commands.entity(parent).add_child(entity);
    super::windows::passive(commands, entity);
    entity
}

fn row(commands: &mut Commands, parent: Entity, gap: f32) -> Entity {
    let entity = commands
        .spawn(Node {
            column_gap: px(gap),
            ..default()
        })
        .id();
    commands.entity(parent).add_child(entity);
    entity
}

fn label(commands: &mut Commands, parent: Entity, text: &str, size: f32, color: Color) -> Entity {
    let entity = commands
        .spawn((
            Text::new(text),
            TextFont {
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(color),
        ))
        .id();
    commands.entity(parent).add_child(entity);
    entity
}

/// Empty slots remain visibly unassigned until an ability is supplied.
fn slot(commands: &mut Commands, parent: Entity, key: &str, size: f32, spell: bool) -> Entity {
    let entity = commands
        .spawn((
            Node {
                width: px(size),
                height: px(if spell { 46 } else { 34 }),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                padding: UiRect::all(px(4)),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            },
            BackgroundColor(if spell {
                Color::srgb(0.075, 0.095, 0.14)
            } else {
                Color::srgb(0.09, 0.10, 0.11)
            }),
            BorderColor::all(EDGE),
        ))
        .id();
    commands.entity(parent).add_child(entity);
    let key_label = label(commands, entity, key, 10.0, Color::srgb(0.66, 0.71, 0.76));
    if spell {
        commands.entity(key_label).insert(Node {
            position_type: PositionType::Absolute,
            bottom: px(1),
            left: px(2),
            ..default()
        });
    }
    entity
}

/// Unknown values have no fill, so disconnected state cannot look like full health.
fn stat_bar(commands: &mut Commands, parent: Entity, name: &'static str, tint: Color) {
    let entity = commands
        .spawn((
            Node {
                height: px(14),
                padding: UiRect::horizontal(px(6)),
                border: UiRect::left(px(2)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.085, 0.10, 0.115)),
            BorderColor::all(tint),
        ))
        .id();
    commands.entity(parent).add_child(entity);
    let fill = commands
        .spawn((
            HudFill(name),
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                height: percent(100),
                width: px(0),
                ..default()
            },
            BackgroundColor(tint.with_alpha(0.3)),
        ))
        .id();
    commands.entity(entity).add_child(fill);
    label(commands, entity, name, 10.0, INK);
    let value = label(commands, entity, "--", 10.0, INK);
    commands.entity(value).insert(HudLabel::Stat(name));
}
