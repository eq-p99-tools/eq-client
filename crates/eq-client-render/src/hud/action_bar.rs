//! One always-visible place for the action in progress, with a progress bar.
//!
//! Casting, spellbook changes and camping each report elsewhere too; this bar
//! only mirrors their state so timing is visible in the same spot every time.

use super::{EDGE, HudState, INK, PANEL};
use bevy::prelude::*;
use eq_client_core::BookActionStatus;
use std::time::{Duration, Instant};

/// Matches the network worker's sit-and-wait interval before a book change is sent.
const BOOK_PREPARATION: Duration = Duration::from_secs(5);
/// Matches the server's camp preparation time.
const CAMP_PREPARATION: Duration = Duration::from_secs(30);
/// How long a refusal or interruption stays visible once nothing is in progress.
const FEEDBACK: Duration = Duration::from_secs(3);

/// Labels for requests whose progress is reported only as a generic status.
#[derive(Resource, Default)]
pub(crate) struct ActionRequests {
    /// The latest spellbook request and when it was queued.
    pub(crate) book: Option<(Instant, String)>,
    /// When camping began, and whether the logout itself has started.
    pub(crate) camp: Option<(Instant, bool)>,
}

#[derive(Component)]
pub(crate) struct ActionBar;
#[derive(Component)]
pub(crate) struct ActionLabel;
#[derive(Component)]
pub(crate) struct ActionFill;

/// What the bar shows: a label and a completed fraction, or None while waiting.
#[derive(Debug, PartialEq)]
pub(super) struct Shown {
    pub(super) label: String,
    pub(super) progress: Option<f32>,
}

/// Builds the hidden bar, centered above the bottom HUD.
pub(crate) fn spawn(commands: &mut Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(232),
                justify_content: JustifyContent::Center,
                ..default()
            },
            GlobalZIndex(12),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: px(300),
                    padding: UiRect::all(px(6)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(4)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(PANEL),
                BorderColor::all(EDGE),
                ActionBar,
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(INK),
                    ActionLabel,
                ));
                panel
                    .spawn((
                        Node {
                            width: percent(100),
                            height: px(8),
                            border_radius: BorderRadius::all(px(2)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.06, 0.07, 0.09)),
                    ))
                    .with_child((
                        Node {
                            width: percent(0),
                            height: percent(100),
                            border_radius: BorderRadius::all(px(2)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.30, 0.50, 0.78)),
                        ActionFill,
                    ));
            });
        });
}

/// Chooses the most specific action in progress, then recent feedback.
pub(super) fn current(
    hud: &HudState,
    requests: &ActionRequests,
    names: &crate::spellbook::SpellNames,
    messages: &super::messages::Messages,
    now: Instant,
) -> Option<Shown> {
    let fraction = |since: Instant, total: Duration| {
        Some((now.saturating_duration_since(since).as_secs_f32() / total.as_secs_f32()).min(1.0))
    };
    if let Some((spell, started, duration)) = hud.casting {
        return Some(Shown {
            label: format!("Casting {}", names.label(u32::from(spell))),
            progress: fraction(started, duration.max(Duration::from_millis(1))),
        });
    }
    if let Some(spell) = hud.pending_cast {
        return Some(Shown {
            label: format!("Casting {} (waiting for server)", names.label(spell)),
            progress: None,
        });
    }
    let book = requests.book.as_ref();
    let book_label = || book.map_or("Changing spells", |(_, label)| label.as_str());
    match hud.book_action {
        Some(BookActionStatus::Preparing) => {
            return Some(Shown {
                label: book_label().into(),
                progress: book.and_then(|(since, _)| fraction(*since, BOOK_PREPARATION)),
            });
        }
        Some(BookActionStatus::Submitted | BookActionStatus::AwaitingReply) => {
            return Some(Shown {
                label: format!("{} (waiting for server)", book_label()),
                progress: None,
            });
        }
        _ => (),
    }
    if let Some((since, logging_out)) = requests.camp {
        return Some(if logging_out {
            Shown {
                label: "Logging out".into(),
                progress: None,
            }
        } else {
            let left = CAMP_PREPARATION.saturating_sub(now.saturating_duration_since(since));
            Shown {
                label: format!("Camping ({}s)", left.as_secs()),
                progress: fraction(since, CAMP_PREPARATION),
            }
        });
    }
    let recent = |at: Instant| now.saturating_duration_since(at) < FEEDBACK;
    if let Some((_, reason)) = hud.interrupted.filter(|(at, _)| recent(*at)) {
        return Some(Shown {
            label: messages.interruption(reason),
            progress: Some(0.0),
        });
    }
    hud.action_feedback
        .as_ref()
        .filter(|(at, _)| recent(*at))
        .map(|(_, message)| Shown {
            label: message.clone(),
            progress: Some(0.0),
        })
}

/// Shows or hides the bar and moves its fill; waiting states pulse at full width.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn update(
    hud: Res<HudState>,
    requests: Res<ActionRequests>,
    definitions: (
        Res<crate::spellbook::SpellNames>,
        Res<super::messages::Messages>,
    ),
    time: Res<Time<Real>>,
    mut bars: Query<&mut Node, (With<ActionBar>, Without<ActionFill>)>,
    mut labels: Query<&mut Text, With<ActionLabel>>,
    mut fills: Query<(&mut Node, &mut BackgroundColor), With<ActionFill>>,
) {
    let (names, messages) = definitions;
    let shown = current(&hud, &requests, &names, &messages, Instant::now());
    for mut node in &mut bars {
        let display = if shown.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    let Some(shown) = shown else {
        return;
    };
    for mut text in &mut labels {
        if text.0 != shown.label {
            text.0.clone_from(&shown.label);
        }
    }
    for (mut node, mut color) in &mut fills {
        let (width, tint) = shown.progress.map_or_else(
            || {
                let pulse = 0.35 + 0.25 * (time.elapsed_secs() * 4.0).sin().abs();
                (100.0, Color::srgb(pulse * 0.6, pulse * 0.8, pulse))
            },
            |progress| (progress * 100.0, Color::srgb(0.30, 0.50, 0.78)),
        );
        node.width = percent(width);
        color.0 = tint;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn casting_wins_then_book_changes_then_camping_then_feedback() {
        let names = crate::spellbook::SpellNames::default();
        let messages = super::super::messages::Messages::load(None);
        let now = Instant::now();
        let mut hud = HudState::default();
        let mut requests = ActionRequests::default();
        assert_eq!(current(&hud, &requests, &names, &messages, now), None);

        requests.camp = Some((now.checked_sub(Duration::from_secs(15)).unwrap(), false));
        let camping = current(&hud, &requests, &names, &messages, now).unwrap();
        assert_eq!(camping.label, "Camping (15s)");
        assert!((camping.progress.unwrap() - 0.5).abs() < 0.01);

        hud.book_action = Some(BookActionStatus::Preparing);
        requests.book = Some((
            now.checked_sub(Duration::from_secs(1)).unwrap(),
            "Memorizing Courage into gem 2".into(),
        ));
        let book = current(&hud, &requests, &names, &messages, now).unwrap();
        assert_eq!(book.label, "Memorizing Courage into gem 2");
        assert!((book.progress.unwrap() - 0.2).abs() < 0.01);
        hud.book_action = Some(BookActionStatus::AwaitingReply);
        assert_eq!(
            current(&hud, &requests, &names, &messages, now)
                .unwrap()
                .progress,
            None
        );

        hud.casting = Some((
            202,
            now.checked_sub(Duration::from_secs(1)).unwrap(),
            Duration::from_secs(4),
        ));
        let casting = current(&hud, &requests, &names, &messages, now).unwrap();
        assert!(casting.label.starts_with("Casting "));
        assert!((casting.progress.unwrap() - 0.25).abs() < 0.01);

        hud.casting = None;
        hud.book_action = None;
        requests.camp = None;
        hud.action_feedback = Some((now, "Spell available in 2.0s".into()));
        assert_eq!(
            current(&hud, &requests, &names, &messages, now)
                .unwrap()
                .label,
            "Spell available in 2.0s"
        );
        let later = now + FEEDBACK;
        assert_eq!(current(&hud, &requests, &names, &messages, later), None);
    }
}
