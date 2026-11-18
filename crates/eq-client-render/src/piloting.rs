//! Explicit boat acquire/release, using the existing accepted passenger link.
//! Steering remains unavailable until its native dynamics are established.
use bevy::prelude::*;
use eq_client_core::{
    ClientCommand,
    boats::{ControlState, Intent, Request, Vessel},
    movement::{CarrierSample, CollisionWorld},
};
use std::time::{Duration, Instant};

/// Retry unanswered withdrawals locally; queue acceptance is not settlement.
const WITHDRAW_RETRY: Duration = Duration::from_millis(250);

pub(super) fn typed(input: &str) -> Option<Result<Intent, String>> {
    let mut words = input.split_ascii_whitespace();
    if !words.next()?.eq_ignore_ascii_case("/boat") {
        return None;
    }
    Some(match (words.next(), words.next()) {
        (Some(word), None) if word.eq_ignore_ascii_case("acquire") => Ok(Intent::Acquire),
        (Some(word), None) if word.eq_ignore_ascii_case("release") => Ok(Intent::Release),
        _ => Err("Use /boat acquire or /boat release; steering is not available".into()),
    })
}

/// Both the last accepted rider frame and a fresh floor sweep must identify
/// the same received incarnation. Proximity alone never supplies a link.
fn support(
    online: &crate::online::OnlineState,
    controls: &crate::motion::Controls,
    collision: Option<&CollisionWorld>,
    body: Option<&crate::PlayerBody>,
) -> Option<(CarrierSample, eq_client_core::WorldPosition)> {
    if online.scene_loading() {
        return None;
    }
    let world = online.world();
    let (accepted, _, position) = controls.carrier_support()?;
    let player = world.player()?;
    if player.position != position {
        return None;
    }
    let feet =
        Vec3::from_array(eq_client_core::render_position(position)) - Vec3::Y * body?.feet_offset;
    let floor = collision?.ground_spawn(feet, 0.01, 0.05)?;
    let current = world.carriers().latest(floor)?.sample();
    (current.spawn_id == accepted.spawn_id && current.incarnation == accepted.incarnation)
        .then_some((current, position))
}

/// What the program's boat interaction asks (read R35 (b), (c), (f)): only
/// for the boat the player stands on, taking it, or letting it go when the
/// player already holds it; for any other boat, nothing, said or sent. The
/// program's release resets local control at once; here, as for
/// `/boat release`, control follows the echo.
fn interaction(boat: u16, standing_on: Option<u16>, held: Option<u16>) -> Option<Intent> {
    if standing_on != Some(boat) {
        return None;
    }
    Some(if held == Some(boat) {
        Intent::Release
    } else {
        Intent::Acquire
    })
}

#[derive(Default)]
pub(super) struct LastSupport {
    key: Option<(u64, Option<Vessel>)>,
    sent: Option<Instant>,
}

/// Resolves deferred input after its script-world guard and before new motion.
/// The same floor source also withdraws control when walking off a vessel.
#[allow(
    clippy::needless_pass_by_value,
    reason = "Bevy supplies system parameters by value"
)]
pub(super) fn update(
    online: Res<crate::online::OnlineState>,
    controls: Res<crate::motion::Controls>,
    collision: Option<Res<crate::Collision>>,
    bodies: Query<&crate::PlayerBody>,
    mut chat: Option<ResMut<crate::chat::ChatState>>,
    outbox: Res<crate::outbox::Outbox>,
    mut last: Local<LastSupport>,
) {
    let Some(chat) = chat.as_deref_mut() else {
        return;
    };
    let world = online.world();
    if !world.can(eq_client_core::Capability::BoatControl) || !world.in_world() {
        if chat.boat_asked.take().is_some() {
            chat.refuse("Boat acquire/release is not offered on this session");
        }
        chat.boat_interacted = None;
        *last = LastSupport::default();
        return;
    }
    let Some(stamp) = outbox.peek(world) else {
        return;
    };
    let support = support(
        &online,
        &controls,
        collision
            .as_deref()
            .and_then(|collision| collision.0.as_ref()),
        bodies.single().ok(),
    );
    if let Some(intent) = chat.boat_asked.take() {
        let request = match intent {
            Intent::Acquire => {
                let Some((support, position)) = support else {
                    chat.refuse("Boat acquisition needs a current accepted floor link");
                    return;
                };
                Request::Acquire { support, position }
            }
            Intent::Release => Request::Release,
        };
        let _ = outbox.send(
            world,
            ClientCommand::BoatControl {
                session_id: stamp.session_id,
                created: stamp.created,
                request,
            },
        );
    }
    if let Some(boat) = chat.boat_interacted.take() {
        let held = match world.boat_control() {
            ControlState::Acquired(vessel) => Some(vessel.spawn_id),
            _ => None,
        };
        let request = match interaction(boat, support.map(|(sample, _)| sample.spawn_id), held) {
            Some(Intent::Release) => Some(Request::Release),
            Some(Intent::Acquire) => {
                support.map(|(support, position)| Request::Acquire { support, position })
            }
            None => None,
        };
        if let Some(request) = request {
            let _ = outbox.send(
                world,
                ClientCommand::BoatControl {
                    session_id: stamp.session_id,
                    created: stamp.created,
                    request,
                },
            );
        }
    }
    let selected = match world.boat_control() {
        ControlState::Acquiring { vessel } | ControlState::Acquired(vessel) => Some(vessel),
        _ => None,
    };
    if let Some(selected) = selected {
        let vessel = support.map(|(sample, _)| Vessel {
            spawn_id: sample.spawn_id,
            incarnation: sample.incarnation,
        });
        let key = Some((stamp.session_id, vessel));
        let now = Instant::now();
        let retry = vessel != Some(selected)
            && last
                .sent
                .is_none_or(|sent| now.saturating_duration_since(sent) >= WITHDRAW_RETRY);
        if (last.key != key || retry)
            && outbox
                .send(
                    world,
                    ClientCommand::BoatControl {
                        session_id: stamp.session_id,
                        created: now,
                        request: Request::Support(support.map(|(sample, _)| sample)),
                    },
                )
                .is_ok()
        {
            last.key = key;
            last.sent = Some(now);
        }
    } else {
        *last = LastSupport::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_boat_acquire_and_release_become_intents() {
        assert_eq!(typed("/BOAT acquire"), Some(Ok(Intent::Acquire)));
        assert_eq!(typed("/boat release"), Some(Ok(Intent::Release)));
        assert!(typed("/boat acquire 8").unwrap().is_err());
        assert!(typed("/boat move").unwrap().is_err());
        assert!(typed("/follow").is_none());
    }

    #[test]
    fn interacting_takes_only_the_boat_underfoot_and_lets_go_of_one_held() {
        assert_eq!(interaction(8, Some(8), None), Some(Intent::Acquire));
        assert_eq!(interaction(8, Some(8), Some(9)), Some(Intent::Acquire));
        assert_eq!(interaction(8, Some(8), Some(8)), Some(Intent::Release));
        // Another boat, or none underfoot: nothing at all.
        assert_eq!(interaction(8, Some(9), None), None);
        assert_eq!(interaction(8, None, Some(8)), None);
    }

    #[test]
    fn an_interaction_with_no_boat_underfoot_sends_and_says_nothing() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(8);
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(7));
        let mut app = App::new();
        app.insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .init_resource::<crate::motion::Controls>()
            .init_resource::<crate::chat::ChatState>()
            .add_systems(Update, update);
        app.world_mut()
            .resource_mut::<crate::chat::ChatState>()
            .interact_with_boat(8, None);
        app.update();
        assert!(receiver.try_recv().is_err());
        let chat = app.world().resource::<crate::chat::ChatState>();
        assert!(chat.boat_interacted.is_none());
    }

    #[test]
    fn a_queued_boat_withdrawal_is_retried_until_worker_state_changes() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(8);
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(7));
        let vessel = Vessel {
            spawn_id: 8,
            incarnation: 1,
        };
        crate::online::testing::news(
            &mut online,
            [eq_client_core::WorldEvent::BoatControlState {
                session_id: 1,
                state: ControlState::Acquired(vessel),
            }],
        );
        let mut app = App::new();
        app.insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .init_resource::<crate::motion::Controls>()
            .init_resource::<crate::chat::ChatState>()
            .add_systems(Update, update);
        app.update();
        let first = receiver.try_recv().unwrap();
        assert!(matches!(
            first,
            ClientCommand::BoatControl {
                request: Request::Support(None),
                ..
            }
        ));
        app.update();
        assert!(receiver.try_recv().is_err());
        // The first queued request can now be stale at the worker. It supplied
        // no state result, so its queue acceptance must not suppress withdrawal.
        std::thread::sleep(Duration::from_millis(1050));
        app.update();
        let retry = receiver.try_recv().unwrap();
        assert!(matches!(
            retry,
            ClientCommand::BoatControl {
                request: Request::Support(None),
                ..
            }
        ));
        assert!(retry.created().unwrap() > first.created().unwrap() + Duration::from_secs(1));
        crate::online::testing::news(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            [eq_client_core::WorldEvent::BoatControlState {
                session_id: 1,
                state: ControlState::Releasing(vessel),
            }],
        );
        app.update();
        assert!(receiver.try_recv().is_err());
    }
}
