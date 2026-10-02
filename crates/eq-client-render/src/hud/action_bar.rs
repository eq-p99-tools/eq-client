//! One always-visible place for the action in progress, with a progress bar.
//!
//! Casting, spellbook changes and camping each report elsewhere too; this bar
//! only mirrors their state so timing is visible in the same spot every time.
//! Where the skin is installed, its casting window shows casts instead, and
//! this bar keeps only the spellbook's changes, which no skin window shows
//! that is known yet; the official client says a camp in the chat alone.

use crate::theme::{self, Size};
use bevy::prelude::*;
use eq_client_core::BookActionStatus;
use std::time::{Duration, Instant};

/// Matches the network worker's sit-and-wait interval before a book change is sent.
const BOOK_PREPARATION: Duration = Duration::from_secs(5);
/// Matches the server's camp preparation time.
const CAMP_PREPARATION: Duration = Duration::from_secs(30);
/// How long an interrupted cast stays on the bar once nothing is in progress.
const INTERRUPTED: Duration = Duration::from_secs(3);

/// Labels for requests whose progress is reported only as a generic status.
#[derive(Resource, Default)]
pub(crate) struct ActionRequests {
    /// The latest spellbook request and when it was queued.
    pub(crate) book: Option<(Instant, String)>,
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
            // Rebuilt with the rest of the HUD at each zone entry.
            super::HudRoot,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: px(300),
                    padding: UiRect::all(px(6)),
                    border: UiRect::all(px(1)),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    display: Display::None,
                    ..default()
                },
                theme::surface(),
                ActionBar,
                // Clicks on the bar stay off the world beneath it.
                crate::windows::Frame::default(),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new(""),
                    theme::font(Size::Label),
                    TextColor(theme::INK),
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
                        BackgroundColor(theme::WELL),
                    ))
                    .with_child((
                        Node {
                            width: percent(0),
                            height: percent(100),
                            border_radius: BorderRadius::all(px(2)),
                            ..default()
                        },
                        BackgroundColor(theme::CAST),
                        ActionFill,
                    ));
            });
        });
}

/// How far `since` has come towards `total` by `now`, from 0 to 1.
fn fraction(since: Instant, total: Duration, now: Instant) -> f32 {
    (now.saturating_duration_since(since).as_secs_f32() / total.as_secs_f32()).min(1.0)
}

/// How far the player's cast has come, from 0 to 1: nothing while the
/// server has not begun it.
pub(crate) fn cast_progress(
    world: &eq_client_core::world::ClientWorld,
    now: Instant,
) -> Option<f32> {
    let (_, started, duration) = world.casting().cast?;
    Some(fraction(
        started,
        duration.max(Duration::from_millis(1)),
        now,
    ))
}

/// The spell the player casts, begun or waiting for the server.
pub(crate) fn cast_spell(world: &eq_client_core::world::ClientWorld) -> Option<u32> {
    let casting = world.casting();
    casting
        .cast
        .map(|(spell, ..)| u32::from(spell))
        .or(casting.pending)
}

/// Whether the skin's casting window shows casts: wherever the skin is
/// installed.
pub(crate) fn skin_shows_casts(settings: &crate::ViewerSettings) -> bool {
    settings.0.eq_directory.is_some()
}

/// Chooses the most specific action in progress, then a cast just
/// interrupted. Where the skin's casting window shows casts, only the
/// spellbook's changes stay here.
pub(super) fn current(
    world: &eq_client_core::world::ClientWorld,
    requests: &ActionRequests,
    (names, messages): (&crate::spellbook::SpellNames, &super::messages::Messages),
    now: Instant,
    skin_shows_casts: bool,
) -> Option<Shown> {
    let cast = || {
        if let Some(progress) = cast_progress(world, now) {
            let spell = cast_spell(world).unwrap_or_default();
            return Some(Shown {
                label: format!("Casting {}", names.label(spell)),
                progress: Some(progress),
            });
        }
        world.casting().pending.map(|spell| Shown {
            label: format!("Casting {} (waiting for server)", names.label(spell)),
            progress: None,
        })
    };
    let book = || {
        let book = requests.book.as_ref();
        let book_label = || book.map_or("Changing spells", |(_, label)| label.as_str());
        match world.book_action() {
            Some(BookActionStatus::Preparing) => Some(Shown {
                label: book_label().into(),
                progress: book.map(|(since, _)| fraction(*since, BOOK_PREPARATION, now)),
            }),
            Some(BookActionStatus::Submitted | BookActionStatus::AwaitingReply) => Some(Shown {
                label: format!("{} (waiting for server)", book_label()),
                progress: None,
            }),
            _ => None,
        }
    };
    let camp = || {
        let eq_client_core::world::Camp { since, logging_out } = world.camp()?;
        Some(if logging_out {
            Shown {
                label: "Logging out".into(),
                progress: None,
            }
        } else {
            let left = CAMP_PREPARATION.saturating_sub(now.saturating_duration_since(since));
            Shown {
                label: format!("Camping ({}s)", left.as_secs()),
                progress: Some(fraction(since, CAMP_PREPARATION, now)),
            }
        })
    };
    let interrupted = || {
        let recent = |at: Instant| now.saturating_duration_since(at) < INTERRUPTED;
        world
            .casting()
            .interrupted
            .filter(|(at, _)| recent(*at))
            .map(|(_, reason)| Shown {
                label: messages.interruption(reason),
                progress: Some(0.0),
            })
    };
    if skin_shows_casts {
        return book();
    }
    cast().or_else(book).or_else(camp).or_else(interrupted)
}

/// Opens the skin's casting window while the player casts, and closes it
/// when the cast ends, as the official client shows its own.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn cast_window(
    settings: Res<crate::ViewerSettings>,
    online: Res<crate::online::OnlineState>,
    mut shown: ResMut<crate::windows::Shown>,
) {
    let id = crate::windows::WindowId::CastBar;
    let wanted = skin_shows_casts(&settings) && cast_spell(online.world()).is_some();
    if wanted != shown.is_open(id) {
        if wanted {
            shown.open(id);
        } else {
            shown.close(id);
        }
    }
}

/// Shows or hides the bar and moves its fill; waiting states pulse at full width.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn update(
    state: (
        Res<crate::online::OnlineState>,
        Res<ActionRequests>,
        Res<crate::ViewerSettings>,
    ),
    definitions: (
        Res<crate::spellbook::SpellNames>,
        Res<super::messages::Messages>,
    ),
    time: Res<Time<Real>>,
    mut bars: Query<&mut Node, (With<ActionBar>, Without<ActionFill>)>,
    mut labels: Query<&mut Text, With<ActionLabel>>,
    mut fills: Query<(&mut Node, &mut BackgroundColor), With<ActionFill>>,
) {
    let (online, requests, settings) = state;
    let (names, messages) = definitions;
    let shown = current(
        online.world(),
        &requests,
        (&names, &messages),
        Instant::now(),
        skin_shows_casts(&settings),
    );
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
                (100.0, theme::awaiting(pulse))
            },
            |progress| (progress * 100.0, theme::CAST),
        );
        node.width = percent(width);
        color.0 = tint;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};
    use eq_client_core::SpellUpdate;

    #[test]
    fn with_the_skin_showing_casts_only_the_books_changes_stay() {
        let names = crate::spellbook::SpellNames::default();
        let messages = super::super::messages::Messages::load(None);
        let now = Instant::now();
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        let requests = ActionRequests::default();
        testing::news_at(
            &mut online,
            [eq_client_core::WorldEvent::Spell(SpellUpdate::Began {
                caster_id: 7,
                spell_id: 202,
                duration_ms: 4000,
            })],
            now.checked_sub(Duration::from_secs(1)).unwrap(),
        );
        let shown = |skin| current(online.world(), &requests, (&names, &messages), now, skin);
        assert!(shown(false).is_some());
        assert_eq!(shown(true), None);
        assert!((cast_progress(online.world(), now).unwrap() - 0.25).abs() < 0.01);
        assert_eq!(cast_spell(online.world()), Some(202));
    }

    #[test]
    fn casting_wins_then_book_changes_then_camping() {
        let names = crate::spellbook::SpellNames::default();
        let messages = super::super::messages::Messages::load(None);
        let now = Instant::now();
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        let mut requests = ActionRequests::default();
        assert_eq!(
            current(online.world(), &requests, (&names, &messages), now, false),
            None
        );

        testing::news_at(
            &mut online,
            [eq_client_core::WorldEvent::Camp(
                eq_client_core::CampStatus::Preparing,
            )],
            now.checked_sub(Duration::from_secs(15)).unwrap(),
        );
        let camping = current(online.world(), &requests, (&names, &messages), now, false).unwrap();
        assert_eq!(camping.label, "Camping (15s)");
        assert!((camping.progress.unwrap() - 0.5).abs() < 0.01);

        testing::book_action(&mut online, BookActionStatus::Preparing);
        requests.book = Some((
            now.checked_sub(Duration::from_secs(1)).unwrap(),
            "Memorizing Courage into gem 2".into(),
        ));
        let book = current(online.world(), &requests, (&names, &messages), now, false).unwrap();
        assert_eq!(book.label, "Memorizing Courage into gem 2");
        assert!((book.progress.unwrap() - 0.2).abs() < 0.01);
        testing::book_action(&mut online, BookActionStatus::AwaitingReply);
        assert_eq!(
            current(online.world(), &requests, (&names, &messages), now, false)
                .unwrap()
                .progress,
            None
        );

        // A cast that began a second ago and takes four.
        testing::news_at(
            &mut online,
            [eq_client_core::WorldEvent::Spell(SpellUpdate::Began {
                caster_id: 7,
                spell_id: 202,
                duration_ms: 4000,
            })],
            now.checked_sub(Duration::from_secs(1)).unwrap(),
        );
        let casting = current(online.world(), &requests, (&names, &messages), now, false).unwrap();
        assert!(casting.label.starts_with("Casting "));
        assert!((casting.progress.unwrap() - 0.25).abs() < 0.01);

        testing::spell(
            &mut online,
            SpellUpdate::Mana {
                spell_id: 202,
                keep_casting: false,
            },
        );
        testing::book_action(&mut online, BookActionStatus::Confirmed);
        testing::news(
            &mut online,
            [eq_client_core::WorldEvent::Camp(
                eq_client_core::CampStatus::Abandoned,
            )],
        );
        assert_eq!(
            current(online.world(), &requests, (&names, &messages), now, false),
            None
        );
    }
}
