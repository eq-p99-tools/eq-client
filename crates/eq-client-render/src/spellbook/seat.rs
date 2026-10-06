//! The book and the player's seat go together, as in the official client:
//! the player must be sitting to keep the book open.
use crate::windows::{Shown, WindowId};
use bevy::prelude::*;
use eq_client_core::{PostureState, world::ClientWorld};

/// What the seat saw last frame: whether the book was open, and the
/// player's reported posture.
#[derive(Default)]
pub(crate) struct Seen {
    open: bool,
    posture: Option<PostureState>,
}

/// Sits a standing player down as the book opens, with the command their
/// sit key sends, and closes the book once their reported posture changes
/// to anything but sitting: standing, walking (the session stands them
/// first), ducking, lying or looting, or the reset when they zone or camp.
/// The sit it sends cannot close the book, since a change to sitting never
/// does. Closing the book leaves the player sitting (inferred). Every
/// official client does this (the Mac client's is inferred), so no server
/// type changes it.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn seat(
    mut shown: ResMut<Shown>,
    online: Option<Res<crate::online::OnlineState>>,
    outbox: Option<Res<crate::outbox::Outbox>>,
    mut seen: Local<Seen>,
) {
    let world = crate::online::world(online.as_deref());
    let posture = world
        .player()
        .and_then(|player| world.posture(player.spawn_id));
    let mut open = shown.is_open(WindowId::Spellbook);
    if open && seen.open && posture != seen.posture && posture != Some(PostureState::Sitting) {
        shown.close(WindowId::Spellbook);
        open = false;
    } else if open && !seen.open {
        sit(world, posture, outbox.as_deref());
    }
    *seen = Seen { open, posture };
}

/// Sits the player in a session, alive, and standing, ducking or not yet
/// reported (as after zoning in, which the session takes for standing).
/// Nothing goes while a memorize, scribe or cast waits, since any posture
/// command cancels a waiting memorize, nor from lying, which a sit would
/// end for a player feigning death (what the official client does then is
/// inferred).
fn sit(world: &ClientWorld, posture: Option<PostureState>, outbox: Option<&crate::outbox::Outbox>) {
    let Some((player, outbox)) = world.player().zip(outbox) else {
        return;
    };
    let standing = matches!(
        posture,
        None | Some(PostureState::Standing | PostureState::Ducking)
    );
    if !standing || !world.connected() || world.death().is_some() || super::action_pending(world) {
        return;
    }
    // The outbox shows why a sit did not go.
    let _ = outbox.post(world, |stamp| eq_client_core::ClientCommand::SetPosture {
        session_id: stamp.session_id,
        spawn_id: player.spawn_id,
        posture: eq_client_core::Posture::Sitting,
        created: stamp.created,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::testing;
    use eq_client_core::{ClientCommand, WorldEvent};
    use std::sync::mpsc::Receiver;

    /// An app with the seat, a player (spawn 7) admitted in session 9, and
    /// the commands it sends.
    fn seated_app() -> (App, Receiver<ClientCommand>) {
        let mut online = crate::online::OnlineState::new(true);
        testing::admit(&mut online, 9, testing::player(7));
        let (sender, receiver) = std::sync::mpsc::sync_channel(8);
        let mut app = App::new();
        app.init_resource::<Shown>()
            .insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .add_systems(Update, seat);
        (app, receiver)
    }

    fn report(app: &mut App, posture: PostureState) {
        let mut online = app.world_mut().resource_mut::<crate::online::OnlineState>();
        testing::news(
            &mut online,
            [WorldEvent::Posture {
                spawn_id: 7,
                posture,
            }],
        );
    }

    fn open(app: &mut App, open: bool) {
        let mut shown = app.world_mut().resource_mut::<Shown>();
        if open {
            shown.open(WindowId::Spellbook);
        } else {
            shown.close(WindowId::Spellbook);
        }
    }

    fn is_open(app: &App) -> bool {
        app.world().resource::<Shown>().is_open(WindowId::Spellbook)
    }

    /// The postures the seat sent, in order.
    fn sent(receiver: &Receiver<ClientCommand>) -> Vec<(u16, eq_client_core::Posture)> {
        receiver
            .try_iter()
            .map(|command| match command {
                ClientCommand::SetPosture {
                    session_id: 9,
                    spawn_id,
                    posture,
                    ..
                } => (spawn_id, posture),
                other => panic!("unexpected {other:?}"),
            })
            .collect()
    }

    #[test]
    fn opening_the_book_sits_a_standing_player_once() {
        use eq_client_core::Posture;
        let (mut app, receiver) = seated_app();
        // Not yet reported, as after zoning in.
        app.update();
        open(&mut app, true);
        app.update();
        assert_eq!(sent(&receiver), [(7, Posture::Sitting)]);
        app.update();
        app.update();
        assert_eq!(sent(&receiver), [] as [(u16, Posture); 0]);
        assert!(is_open(&app));
        // Reported standing, and ducking: each opening sits them once.
        for posture in [PostureState::Standing, PostureState::Ducking] {
            open(&mut app, false);
            report(&mut app, posture);
            app.update();
            open(&mut app, true);
            app.update();
            app.update();
            assert_eq!(sent(&receiver), [(7, Posture::Sitting)]);
        }
    }

    #[test]
    fn the_book_sits_no_one_sitting_lying_frozen_looting_dead_offline_or_memorizing() {
        use eq_client_core::Posture;
        let (mut app, receiver) = seated_app();
        for posture in [
            PostureState::Sitting,
            PostureState::Lying,
            PostureState::Frozen,
            PostureState::Looting,
        ] {
            report(&mut app, posture);
            app.update();
            open(&mut app, true);
            app.update();
            assert_eq!(sent(&receiver), [] as [(u16, Posture); 0], "{posture:?}");
            // The book opens all the same.
            assert!(is_open(&app));
            open(&mut app, false);
            app.update();
        }
        // A memorize the session prepares, which a posture command would
        // cancel.
        report(&mut app, PostureState::Standing);
        testing::book_action(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            eq_client_core::BookActionStatus::Preparing,
        );
        app.update();
        open(&mut app, true);
        app.update();
        assert_eq!(sent(&receiver), [] as [(u16, Posture); 0]);
        open(&mut app, false);
        app.update();
        // Dead.
        let (mut app, receiver) = seated_app();
        testing::news(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            [WorldEvent::Death(eq_client_core::Death {
                spawn_id: 7,
                killer_id: 0,
                corpse_id: 0,
                bind_zone_id: 0,
                corpse_name: None,
            })],
        );
        app.update();
        open(&mut app, true);
        app.update();
        assert_eq!(sent(&receiver), [] as [(u16, Posture); 0]);
        // Offline, as the preview runs: the book opens with nothing to send.
        let mut app = App::new();
        app.init_resource::<Shown>().add_systems(Update, seat);
        open(&mut app, true);
        app.update();
        assert!(is_open(&app));
    }

    #[test]
    fn the_sits_own_report_keeps_the_book_open_and_standing_closes_it() {
        let (mut app, receiver) = seated_app();
        report(&mut app, PostureState::Standing);
        app.update();
        open(&mut app, true);
        app.update();
        assert_eq!(sent(&receiver).len(), 1);
        // The session reports the sit it sent.
        report(&mut app, PostureState::Sitting);
        app.update();
        assert!(is_open(&app));
        app.update();
        assert!(is_open(&app));
        // The stand key, `/stand` or a step: the session reports standing.
        report(&mut app, PostureState::Standing);
        app.update();
        assert!(!is_open(&app));
        // Closing sends nothing; the player stays as they are.
        assert_eq!(sent(&receiver).len(), 0);
    }

    #[test]
    fn ducking_lying_looting_or_a_zone_change_closes_the_book() {
        let (mut app, receiver) = seated_app();
        for posture in [
            PostureState::Ducking,
            PostureState::Lying,
            PostureState::Looting,
        ] {
            report(&mut app, PostureState::Sitting);
            app.update();
            open(&mut app, true);
            app.update();
            assert!(is_open(&app), "{posture:?}");
            report(&mut app, posture);
            app.update();
            assert!(!is_open(&app), "{posture:?}");
        }
        // Opened while lying, it closes once the player stands.
        report(&mut app, PostureState::Lying);
        app.update();
        open(&mut app, true);
        app.update();
        report(&mut app, PostureState::Standing);
        app.update();
        assert!(!is_open(&app));
        // A new admission forgets the posture.
        report(&mut app, PostureState::Sitting);
        app.update();
        open(&mut app, true);
        app.update();
        testing::admit(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            9,
            testing::player(7),
        );
        app.update();
        assert!(!is_open(&app));
        assert_eq!(sent(&receiver).len(), 0);
    }

    #[test]
    fn the_skins_spellbook_button_and_its_key_take_the_same_path() {
        use eq_client_core::Posture;
        let (mut app, receiver) = seated_app();
        crate::keys::testing::install(&mut app);
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::windows::Stack>()
            .init_resource::<crate::escape::Escape>()
            .init_resource::<crate::chat::ChatState>()
            .add_systems(Update, crate::windows::toggle.before(seat));
        app.world_mut().spawn((
            Window {
                focused: true,
                ..default()
            },
            bevy::window::PrimaryWindow,
        ));
        report(&mut app, PostureState::Standing);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyB);
        app.update();
        assert!(is_open(&app));
        assert_eq!(sent(&receiver), [(7, Posture::Sitting)]);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::KeyB);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        open(&mut app, false);
        app.update();
        // The skin's button is a selector button for the book.
        let button = app
            .world_mut()
            .spawn((
                crate::windows::SelectorButton(WindowId::Spellbook),
                Interaction::Pressed,
            ))
            .id();
        app.update();
        assert!(is_open(&app));
        assert_eq!(sent(&receiver), [(7, Posture::Sitting)]);
        app.world_mut().despawn(button);
    }
}
