use super::*;
use crate::control::StunState;

fn state(session_id: u64, revision: u64, state: StunState) -> WorldEvent {
    WorldEvent::OwnStun {
        session_id,
        revision,
        state,
    }
}

#[test]
fn unknown_clear_and_stunned_are_distinct_and_only_scoped_newer_state_is_applied() {
    let mut world = admitted();
    assert_eq!(world.stun_state(), None);
    assert!(!world.is_stunned()); // False is not known-clear.
    assert_eq!(
        game(&mut world, state(1, 0, StunState::Clear)).notices,
        [] as [Notice; 0]
    );
    assert_eq!(world.stun_state(), Some(StunState::Clear));
    let news = game(&mut world, state(1, 1, StunState::Stunned));
    assert!(world.is_stunned());
    assert!(news.reset.is_none());
    assert!(matches!(
        news.notices[..],
        [Notice::ServerString { id: 12479, .. }]
    ));
    assert!(game(&mut world, state(1, 1, StunState::Stunned)).ignored);
    assert!(game(&mut world, state(1, 0, StunState::Clear)).ignored);
    assert!(game(&mut world, state(2, 9, StunState::Clear)).ignored);
    // A raw zero retains its original value, but does not release host state.
    game(&mut world, WorldEvent::Stun(None));
    assert!(world.is_stunned());
    let news = game(&mut world, state(1, 2, StunState::Clear));
    assert_eq!(world.stun_revision(), Some(2));
    assert!(matches!(
        news.notices[..],
        [Notice::ServerString { id: 12498, .. }]
    ));
    assert_eq!(world.stun_state(), Some(StunState::Clear));
}

#[test]
fn stun_expiry_preserves_death_and_old_release_cannot_change_a_new_admission() {
    let mut world = admitted();
    game(&mut world, state(1, 1, StunState::Stunned));
    game(&mut world, WorldEvent::DeathBind { session_id: 1 });
    assert!(world.is_dead());
    assert!(world.is_stunned());
    game(&mut world, state(1, 2, StunState::Clear));
    assert!(world.is_dead());
    assert_eq!(world.motion(), None);
    game(&mut world, entered(2));
    assert_eq!(world.stun_state(), None);
    assert_eq!(world.stun_revision(), None);
    game(&mut world, state(2, 0, StunState::Stunned));
    assert!(game(&mut world, state(1, 90, StunState::Clear)).ignored);
    assert!(world.is_stunned());
}
