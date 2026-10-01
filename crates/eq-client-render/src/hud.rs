//! Offline character panel and empty action slots, ready for session data.

pub(super) mod action_bar;
pub(crate) mod hotbar;
pub(crate) mod messages;
mod requests;
#[cfg(test)]
mod tests;

use crate::theme::{self, Size};
use bevy::prelude::*;

/// What the HUD shows besides the world: the status line, estimated maxima
/// and timed feedback.
#[derive(Resource, Default)]
pub(super) struct HudState {
    pub status: String,
    /// Calculated, unverified maxima (mana, endurance), never server-reported values.
    pub resource_estimate: Option<(u32, u32)>,
    pub action_feedback: Option<(std::time::Instant, String)>,
}

#[derive(Component)]
pub(super) struct HudRoot;
#[derive(Component)]
pub(super) struct SpellGem(u8);
#[derive(Component)]
pub(super) struct SpellDetails;
#[derive(Component, Clone, Copy)]
pub(super) enum HudLabel {
    Stat(Stat),
    /// The player window's title: the character's name.
    Name,
    Spell(usize),
    Casting,
}

/// The player window's bars.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stat {
    Hp,
    Mana,
    Stamina,
    Experience,
}

impl Stat {
    const ALL: [Self; 4] = [Self::Hp, Self::Mana, Self::Stamina, Self::Experience];

    const fn label(self) -> &'static str {
        match self {
            Self::Hp => "HP",
            Self::Mana => "MANA",
            Self::Stamina => "STAMINA",
            Self::Experience => "EXP",
        }
    }

    const fn tint(self) -> Color {
        match self {
            Self::Hp => theme::HP,
            Self::Mana => theme::MANA,
            Self::Stamina => theme::STAMINA,
            Self::Experience => theme::EXPERIENCE,
        }
    }

    /// The bar's figure: what the server reported, with calculated maxima
    /// marked as such.
    fn text(
        self,
        world: &eq_client_core::world::ClientWorld,
        estimate: Option<(u32, u32)>,
    ) -> Option<String> {
        let vitals = world.vitals();
        match self {
            Self::Hp => world
                .hit_points()
                .map(|(a, b)| format!("{a}/{b}"))
                // Without a report, the health the server gave in percent.
                .or_else(|| {
                    world
                        .player()
                        .and_then(|player| player.hp_percent)
                        .map(|p| format!("{p}%"))
                }),
            Self::Mana => vitals
                .mana
                .map(|v| resource_label(v, estimate.map(|v| v.0))),
            Self::Stamina => vitals
                .endurance
                .map(|v| resource_label(v, estimate.map(|v| v.1))),
            Self::Experience => vitals
                .experience_ratio()
                .map(|ratio| format!("{:.1}%", ratio * 100.0)),
        }
    }

    /// How full the bar is, from 0 to 1, when that is known.
    fn ratio(
        self,
        world: &eq_client_core::world::ClientWorld,
        estimate: Option<(u32, u32)>,
    ) -> Option<f64> {
        let vitals = world.vitals();
        match self {
            Self::Hp => world
                .hit_points()
                .filter(|(_, max)| *max > 0)
                .map(|(value, max)| f64::from(value) / f64::from(max))
                .or_else(|| {
                    world
                        .player()
                        .and_then(|player| player.hp_percent)
                        .map(|v| f64::from(v) / 100.0)
                }),
            Self::Mana => resource_ratio(vitals.mana, estimate.map(|v| v.0)),
            Self::Stamina => resource_ratio(vitals.endurance, estimate.map(|v| v.1)),
            Self::Experience => vitals.experience_ratio(),
        }
    }
}

#[derive(Component)]
pub(super) struct HudFill(Stat);

/// Refreshes actual values without inventing unknown resource maxima.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn update(
    state: Res<HudState>,
    online: Res<super::online::OnlineState>,
    names: Res<super::spellbook::SpellNames>,
    messages: Res<messages::Messages>,
    mut texts: Query<(&mut Text, &HudLabel)>,
    mut fills: Query<(&mut Node, &HudFill)>,
) {
    let now = std::time::Instant::now();
    let world = online.world();
    let casting = world.casting();
    for (mut text, label) in &mut texts {
        let value = match label {
            HudLabel::Casting => casting.cast.map_or_else(
                || {
                    if let Some(spell) = casting.pending {
                        format!("Awaiting cast acknowledgement | {}", names.label(spell))
                    } else if let Some((_, reason)) = casting
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
            // The official player window names the character; where the
            // connection stands shows in the status line instead.
            HudLabel::Name => world
                .player()
                .map_or_else(|| "CHARACTER".into(), |player| player.name.clone()),
            HudLabel::Stat(stat) => stat
                .text(world, state.resource_estimate)
                .unwrap_or_else(|| "--".into()),
            HudLabel::Spell(index) => world.gem(*index).map_or_else(String::new, |id| {
                let remaining = casting.cooldowns.remaining(id, now);
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
        let ratio = stat
            .ratio(world, state.resource_estimate)
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
    super::windows::spawn_selector(commands);
    super::target::spawn(commands);
    super::items::spawn(commands);
    super::inventory::spawn(commands);
    super::spellbook::spawn(commands);
    action_bar::spawn(commands);

    let character = panel(commands, root, 186.0);
    super::windows::identify(commands, character, super::windows::WindowId::Player);
    let name = label(commands, character, "CHARACTER", Size::Small, theme::INK);
    commands.entity(name).insert(HudLabel::Name);
    for stat in Stat::ALL {
        stat_bar(commands, character, stat);
    }

    hotbar::spawn(commands, root);

    let spells = panel(commands, root, 358.0);
    super::windows::titled(commands, spells, super::windows::WindowId::Spells);
    let gems = row(commands, spells, 4.0);
    let casting = label(commands, spells, "", Size::Small, theme::INK);
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
            crate::outbox::Needs(eq_client_core::Capability::Casting),
        ));
        let value = label(commands, slot, "", Size::Caption, theme::INK);
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
    let details = label(commands, spells, "", Size::Small, theme::INK_DIM);
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

/// Writes each gem's and action slot's keys into its tooltip from the key
/// map, so the help follows the bindings.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn key_help(
    map: Res<super::keys::KeyMap>,
    mut commands: Commands,
    gems: Query<(Entity, &SpellGem, Option<&super::tooltip::Tooltip>)>,
    slots: Query<(Entity, &hotbar::Slot, Option<&super::tooltip::Tooltip>)>,
) {
    use super::keys::Act;
    let mut write = |entity: Entity, current: Option<&super::tooltip::Tooltip>, help: String| {
        if current.is_none_or(|tooltip| tooltip.0 != help) {
            commands
                .entity(entity)
                .insert(super::tooltip::Tooltip(help));
        }
    };
    for (entity, SpellGem(gem), tooltip) in &gems {
        let help = format!(
            "{} | Shift-click: forget",
            map.help(&[(Act::Gem(*gem), "cast")])
        );
        write(entity, tooltip, help);
    }
    for (entity, hotbar::Slot(slot), tooltip) in &slots {
        let slot = u8::try_from(*slot).unwrap_or(u8::MAX);
        let help = map.help(&[
            (Act::Slot(slot), "use"),
            (Act::BindSlot(slot), "bind the hovered gem or item"),
            (Act::ClearSlot(slot), "empty"),
        ]);
        write(entity, tooltip, help);
    }
}

/// What an empty gem says: which gem, and where spells are memorized.
pub(super) fn empty_gem(gem: u8, map: &super::keys::KeyMap) -> String {
    format!(
        "Gem {} is empty: {} to memorize a spell",
        gem + 1,
        map.named(
            super::keys::Act::Toggle(super::windows::WindowId::Spellbook),
            "open the spellbook"
        )
    )
}

/// Shows current gem identity and local timing without retaining stale hover state.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn spell_details(
    map: Res<super::keys::KeyMap>,
    state: Res<HudState>,
    online: Res<super::online::OnlineState>,
    names: Res<super::spellbook::SpellNames>,
    mut gems: Query<(&Interaction, &SpellGem, &mut BackgroundColor)>,
    mut labels: Query<&mut Text, With<SpellDetails>>,
) {
    let now = std::time::Instant::now();
    let casting = online.world().casting();
    let held = online.world().gems();
    let mut hovered = None;
    for (interaction, SpellGem(gem), mut background) in &mut gems {
        let spell = held.get(usize::from(*gem)).copied().flatten();
        let waiting = casting.cast.is_some()
            || casting.pending.is_some()
            || spell.is_some_and(|id| !casting.cooldowns.remaining(id, now).is_zero());
        let pointed = *interaction != Interaction::None;
        if pointed {
            hovered = Some((*gem, spell));
        }
        background.0 = theme::readiness(pointed, spell.is_none(), waiting);
    }
    let mut text = match hovered {
        None => String::new(),
        Some((gem, None)) => empty_gem(gem, &map),
        Some((_, Some(spell))) => {
            let mut text = names.label(spell);
            text.push('\n');
            text.push_str(&names.details(spell));
            if let Some(timing) = names.timing(spell) {
                use std::fmt::Write;
                let _ = write!(
                    text,
                    "\nBase reuse {:.1}s | recovery {:.1}s",
                    f64::from(timing.recast_ms) / 1000.0,
                    f64::from(timing.recovery_ms) / 1000.0
                );
            } else {
                text.push_str("\nLocal spell timing unavailable");
            }
            let remaining = casting.cooldowns.remaining(spell, now);
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
    keys: super::keys::Keys,
    mut hud: ResMut<HudState>,
    online: Res<super::online::OnlineState>,
    outbox: Res<crate::outbox::Outbox>,
    clicks: Query<(&Interaction, &SpellGem), Changed<Interaction>>,
    bar_clicks: Query<(&Interaction, &hotbar::Slot), Changed<Interaction>>,
    bindings: Res<hotbar::Bindings>,
    definitions: (Res<super::spellbook::SpellNames>, Res<messages::Messages>),
) {
    use super::keys::Act;
    if !online.world().connected() || online.world().death().is_some() || !keys.focused() {
        return;
    }
    let Some(player) = online.world().player() else {
        return;
    };
    let clicked = clicks
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
        .map(|(_, gem)| gem.0);
    // Shift-click forgets the gem; its key only casts.
    let forgetting = clicked.is_some()
        && keys
            .input
            .any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let gem = clicked.or_else(|| (0..8).find(|gem| keys.pressed(Act::Gem(*gem))));
    let bar_action = hotbar::requested(&keys, &bindings, &bar_clicks);
    let gem = gem.or(match bar_action {
        Some(hotbar::Action::Gem(gem)) => Some(gem),
        _ => None,
    });
    if let Some(gem) = gem {
        let (names, messages) = definitions;
        let mana_cost = online
            .world()
            .gem(usize::from(gem))
            .and_then(|spell| names.mana(spell));
        requests::spell(
            &mut hud,
            online.world(),
            player,
            &outbox,
            &requests::Request {
                gem,
                target_id: online.world().target().selected.unwrap_or(player.spawn_id),
                forgetting,
                mana_cost,
            },
            (&messages, &keys.map),
        );
    }
    let posture = if keys.pressed(Act::Duck) {
        Some(eq_client_core::Posture::Ducking)
    } else if keys.pressed(Act::Sit) {
        Some(eq_client_core::Posture::Sitting)
    } else if keys.pressed(Act::Stand) {
        Some(eq_client_core::Posture::Standing)
    } else {
        match bar_action {
            Some(hotbar::Action::Sit) => Some(eq_client_core::Posture::Sitting),
            Some(hotbar::Action::Stand) => Some(eq_client_core::Posture::Standing),
            _ => None,
        }
    };
    if let Some(posture) = posture {
        // The outbox shows why a stance did not go.
        let _ = outbox.post(online.world(), |stamp| {
            eq_client_core::ClientCommand::SetPosture {
                session_id: stamp.session_id,
                spawn_id: player.spawn_id,
                posture,
                created: stamp.created,
            }
        });
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
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                ..default()
            },
            theme::surface(),
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

fn label(commands: &mut Commands, parent: Entity, text: &str, size: Size, ink: Color) -> Entity {
    let entity = commands.spawn(theme::text(text, size, ink)).id();
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
            BackgroundColor(theme::INSET),
            BorderColor::all(theme::EDGE),
        ))
        .id();
    commands.entity(parent).add_child(entity);
    let key_label = label(commands, entity, key, Size::Small, theme::INK);
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
fn stat_bar(commands: &mut Commands, parent: Entity, stat: Stat) {
    let tint = stat.tint();
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
            BackgroundColor(theme::INSET),
            BorderColor::all(tint),
        ))
        .id();
    commands.entity(parent).add_child(entity);
    let fill = commands
        .spawn((
            HudFill(stat),
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
    label(commands, entity, stat.label(), Size::Small, theme::INK);
    let value = label(commands, entity, "--", Size::Small, theme::INK);
    commands.entity(value).insert(HudLabel::Stat(stat));
}
