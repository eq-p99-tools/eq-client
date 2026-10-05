//! The loading screen. While the player moves between zones, a cover hides
//! the scene and every window: from the moment the server offers a transfer
//! (Gate, a zone line, the trip back to the bind after a death) until the
//! new zone is drawn with the spawns around the player in it. At Enter World,
//! character select's own cover waits for the admission and this one takes
//! over until the zone has settled. Offline there is nothing to wait for.
//!
//! So the player never sees the old zone hang on while the transfer runs,
//! the frame freeze while the new zone loads, or the new zone's windows and
//! spawns pop in. What the official client shows while it loads a zone is
//! unrecorded, so this look is inferred: character select's cover with one
//! word on it.
use crate::theme::{self, Size};
use bevy::prelude::*;

/// The least time the cover stays once the zone is admitted, so its windows
/// and the chat have laid out before they show.
const SETTLE: f32 = 0.25;
/// The most it stays once the zone is admitted, whatever is still to come.
const MOST: f32 = 4.0;
/// The most it stays while a transfer runs. A transfer that hangs this long
/// is stuck, and the player sees the old zone again, with the status box
/// saying where the connection stands.
const STUCK: f32 = 30.0;
/// Over every window and screen. The cover takes the pointer, so nothing
/// under it reacts or shows a tooltip.
const LAYER: i32 = 900;

/// Where the player stands between zones.
#[derive(Resource, Default)]
pub(crate) struct Loading {
    phase: Phase,
    /// The admission last seen, to tell a new one.
    admission: Option<u64>,
}

/// How far a move between zones has come.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Phase {
    /// In a zone, at character select or offline: nothing to cover.
    #[default]
    Clear,
    /// The server offered a transfer this many seconds into the run, and no
    /// zone has admitted the player since.
    Moving(f32),
    /// The transfer ran longer than [`STUCK`]: the cover is down until a zone
    /// admits the player.
    Stuck,
    /// A zone admitted the player this many seconds into the run, and the
    /// spawns around them are coming in.
    Arriving(f32),
}

impl Loading {
    /// Whether the cover is up.
    pub(crate) fn covered(&self) -> bool {
        matches!(self.phase, Phase::Moving(_) | Phase::Arriving(_))
    }
}

/// The loading screen's cover.
#[derive(Component)]
struct Cover;

/// Spawns the cover, hidden, and keeps it up while the player moves between
/// zones.
pub(crate) fn install(app: &mut App) {
    app.init_resource::<Loading>()
        .add_systems(Startup, spawn)
        .add_systems(
            Update,
            (track, show)
                .chain()
                .after(crate::Stage::Scene)
                .before(crate::Stage::Typing),
        );
}

fn spawn(mut commands: Commands) {
    commands
        .spawn((
            Cover,
            Button,
            GlobalZIndex(LAYER),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            BackgroundColor(theme::COVER),
        ))
        .with_children(|cover| theme::label(cover, "Loading", Size::Large));
}

/// Follows the move between zones: a transfer the server offered puts the
/// cover up for up to [`STUCK`] seconds, and a new admission keeps it up
/// until the spawns around the player are drawn, at least [`SETTLE`] and at
/// most [`MOST`] seconds.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn track(
    time: Res<Time>,
    online: Res<crate::online::OnlineState>,
    nearby: Option<Res<crate::entities::NearbyEntities>>,
    mut loading: ResMut<Loading>,
) {
    let world = online.world();
    let now = time.elapsed_secs();
    let admission = world.session_id();
    let phase = if !online.enabled || world.ended() {
        // Offline, or the session is over and the screen says why.
        Phase::Clear
    } else if admission != loading.admission {
        // A new admission brings its zone. None means the player camped or
        // the connection dropped, and character select has its own cover.
        admission.map_or(Phase::Clear, |_| Phase::Arriving(now))
    } else if world.pending_transfer().is_some() {
        match loading.phase {
            Phase::Moving(since) if now - since >= STUCK => Phase::Stuck,
            Phase::Moving(since) => Phase::Moving(since),
            Phase::Stuck => Phase::Stuck,
            Phase::Clear | Phase::Arriving(_) => Phase::Moving(now),
        }
    } else if let Phase::Arriving(since) = loading.phase {
        let settled = world.connected() && nearby.is_none_or(|nearby| nearby.settled());
        let waited = now - since;
        if waited < MOST && (waited < SETTLE || !settled) {
            Phase::Arriving(since)
        } else {
            Phase::Clear
        }
    } else {
        // In the zone, or the server refused the transfer.
        Phase::Clear
    };
    if loading.phase != phase || loading.admission != admission {
        loading.phase = phase;
        loading.admission = admission;
    }
}

/// Shows the cover while the player moves between zones, and hides it
/// otherwise.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
fn show(loading: Res<Loading>, mut covers: Query<&mut Node, With<Cover>>) {
    let display = if loading.covered() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut covers {
        if node.display != display {
            node.display = display;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};
    use eq_client_core::WorldEvent;
    use std::time::Duration;

    fn app(online: bool) -> App {
        let mut app = App::new();
        app.insert_resource(OnlineState::new(online))
            .init_resource::<Time>()
            .init_resource::<Loading>()
            .add_systems(Update, track);
        app
    }

    /// Runs a frame this many seconds after the last, and says whether the
    /// cover is up.
    fn after(app: &mut App, seconds: f32) -> bool {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(seconds));
        app.update();
        app.world().resource::<Loading>().covered()
    }

    fn online(app: &mut App) -> Mut<'_, OnlineState> {
        app.world_mut().resource_mut::<OnlineState>()
    }

    fn transfer(app: &mut App) {
        testing::news(
            &mut online(app),
            [WorldEvent::ZoneTransfer(eq_client_core::ZoneOffer {
                zone_id: 2,
                instance_id: 0,
                position: eq_client_core::WorldPosition::default(),
                reason: 0,
                to_bind: true,
                solicited: true,
            })],
        );
    }

    #[test]
    fn a_transfer_covers_the_old_zone_until_the_new_one_has_settled() {
        let mut app = app(true);
        assert!(!after(&mut app, 0.0));
        // Enter World: the zone's first moments stay covered.
        testing::admit(&mut online(&mut app), 1, testing::player(7));
        assert!(after(&mut app, 0.0));
        assert!(after(&mut app, 0.1));
        assert!(!after(&mut app, SETTLE));
        // Gate: the old zone is covered for the whole transfer.
        transfer(&mut app);
        assert!(after(&mut app, 0.0));
        assert!(after(&mut app, MOST + 10.0));
        // One that hangs shows the old zone again until a zone admits the
        // player.
        assert!(!after(&mut app, STUCK));
        assert!(!after(&mut app, 60.0));
        // The new zone's admission keeps it up until it has settled.
        testing::admit(&mut online(&mut app), 2, testing::player(7));
        assert!(after(&mut app, 0.1));
        assert!(!after(&mut app, SETTLE));
        assert!(!after(&mut app, 1.0));
    }

    #[test]
    fn the_cover_waits_for_the_spawns_around_the_player_but_not_for_ever() {
        let mut app = app(true);
        app.init_resource::<crate::entities::NearbyEntities>();
        testing::admit(&mut online(&mut app), 1, testing::player(7));
        assert!(after(&mut app, 0.0));
        assert!(after(&mut app, SETTLE));
        assert!(after(&mut app, 1.0));
        app.world_mut()
            .resource_mut::<crate::entities::NearbyEntities>()
            .set_settled(true);
        assert!(!after(&mut app, 0.0));
        // Spawns that never all come in lift it once it has waited its most.
        app.world_mut()
            .resource_mut::<crate::entities::NearbyEntities>()
            .set_settled(false);
        transfer(&mut app);
        testing::admit(&mut online(&mut app), 2, testing::player(7));
        assert!(after(&mut app, 0.0));
        assert!(after(&mut app, MOST - 0.5));
        assert!(!after(&mut app, 0.5));
    }

    #[test]
    fn a_refused_transfer_a_camp_or_an_ended_session_lifts_the_cover() {
        let mut app = app(true);
        testing::admit(&mut online(&mut app), 1, testing::player(7));
        after(&mut app, 0.0);
        assert!(!after(&mut app, SETTLE));
        transfer(&mut app);
        assert!(after(&mut app, 0.0));
        testing::news(
            &mut online(&mut app),
            [WorldEvent::ZoneTransferRejected {
                session_id: 1,
                reason: eq_client_core::ZoneRejection::Server(0),
            }],
        );
        assert!(!after(&mut app, 0.0));
        // Camped, character select has its own cover.
        transfer(&mut app);
        assert!(after(&mut app, 0.0));
        testing::news(
            &mut online(&mut app),
            [WorldEvent::Camp(eq_client_core::CampStatus::Camped)],
        );
        assert!(!after(&mut app, 0.0));
        // An ended session shows why it ended.
        testing::admit(&mut online(&mut app), 3, testing::player(7));
        transfer(&mut app);
        assert!(after(&mut app, 0.0));
        testing::link(&mut online(&mut app), eq_client_core::world::Link::Ended);
        assert!(!after(&mut app, 0.0));
    }

    #[test]
    fn offline_nothing_is_covered() {
        let mut app = app(false);
        testing::admit(&mut online(&mut app), 1, testing::player(7));
        assert!(!after(&mut app, 0.0));
        transfer(&mut app);
        assert!(!after(&mut app, 0.0));
    }

    #[test]
    fn the_cover_shows_while_the_player_moves_between_zones() {
        let mut app = App::new();
        app.init_resource::<Loading>()
            .add_systems(Startup, spawn)
            .add_systems(Update, show);
        app.update();
        let display = |app: &mut App| {
            app.world_mut()
                .query_filtered::<&Node, With<Cover>>()
                .single(app.world())
                .unwrap()
                .display
        };
        assert_eq!(display(&mut app), Display::None);
        app.world_mut().resource_mut::<Loading>().phase = Phase::Moving(0.0);
        app.update();
        assert_eq!(display(&mut app), Display::Flex);
        app.world_mut().resource_mut::<Loading>().phase = Phase::Clear;
        app.update();
        assert_eq!(display(&mut app), Display::None);
    }
}
