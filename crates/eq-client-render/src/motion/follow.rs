//! Following a group member on foot, as the Titanium client does (program
//! read R6: `/follow` 0x472313, each frame 0x47c4fc, the end 0x479482).
//!
//! `/follow` follows the target, who must be another player in the group,
//! while the player may move and stands. Each frame the player turns toward
//! them, by the program's steps at 60 frames a second, and goes forward
//! until within both bodies' stand heights and 15 units, walking until 30;
//! following ends when the one followed dies, goes further than 200 units or
//! leaves, or when a movement key is pressed.
//!
//! Inferred (recording checklist 6.412): the program's clear-path test
//! before it turns (0x476167) is not read, so this one always turns; the
//! distance is measured in three dimensions; the start's posture test other
//! than standing (0x33ac) is not made; every key that moves the player ends
//! it, as the five the program tests are not told apart; and a client
//! whose wire carries no absolute heading turns no faster than its turn keys.
use crate::chat::{ChatState, Said, system_line};
use crate::hud::messages::Messages;
use bevy::prelude::*;
use eq_client_core::{SpawnKind, WorldPosition, world::ClientWorld};

/// The official client's lines for following, by `eqstr_us.txt` number.
mod lines {
    /// The target is the player.
    pub(super) const SELF: u32 = 0x33aa;
    /// The target is not in the player's group.
    pub(super) const NOT_GROUPED: u32 = 0x33a9;
    /// The player cannot move.
    pub(super) const ROOTED: u32 = 0x33ab;
    /// The player is not standing.
    pub(super) const NOT_STANDING: u32 = 0x33ad;
    /// Following begins, and how it ends.
    pub(super) const STARTED: [u32; 3] = [0x33ae, 0x33af, 0x33b0];
    /// The one followed died.
    pub(super) const DIED: u32 = 0x3194;
    /// The one followed is too far away.
    pub(super) const TOO_FAR: u32 = 0x3195;
    /// Following stopped.
    pub(super) const STOPPED: u32 = 0x3196;
}

/// How far the one followed may get before following ends.
const LEAVE_DISTANCE: f32 = 200.0;
/// Within this distance the heading snaps to the one followed.
const SNAP_DISTANCE: f32 = 20.0;
/// Beyond both stand heights and this, the player goes forward.
const CLOSE: f32 = 15.0;
/// Beyond both stand heights and this, the player runs.
const RUN: f32 = 30.0;
/// The program steps its turn once a frame, at this rate.
const FRAMES_PER_SECOND: f32 = 60.0;

/// Who the player follows.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Following {
    /// The admission it began in; another zone's spawn of the same ID is
    /// someone else.
    session_id: Option<u64>,
    spawn_id: u16,
    name: String,
    /// How fast the player went at the last step, in units a second.
    speed: f32,
}

/// What following does in a sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Step {
    /// The heading the player turns to.
    pub heading: f32,
    /// Whether the player goes forward.
    pub forward: bool,
    /// Whether the player runs rather than walks.
    pub run: bool,
    /// How far the player is beyond where they stop, so that a sample
    /// stops there as the program's frames do rather than going past.
    pub beyond: f32,
}

/// What the player and the one followed are now.
pub(super) struct Bodies {
    /// The player's position.
    pub own: WorldPosition,
    /// The player's stand height.
    pub own_height: f32,
    /// How fast the player goes running, and walking, in units a second.
    pub speeds: (f32, f32),
}

/// Why following ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum End {
    /// The one followed died.
    Died,
    /// They got too far away.
    TooFar,
    /// They left, or the player moved.
    Stopped,
}

/// Whether a movement key moves the player, which ends following.
pub(super) fn moves(
    map: &crate::keys::KeyMap,
    keys: &ButtonInput<KeyCode>,
    scripted: &[crate::keys::Act],
) -> bool {
    use crate::keys::Act;
    let held = map.holding(keys, scripted);
    [
        Act::Forward,
        Act::Back,
        Act::StrafeLeft,
        Act::StrafeRight,
        Act::CameraForward,
        Act::CameraBack,
        Act::CameraLeft,
        Act::CameraRight,
        Act::TurnLeft,
        Act::TurnRight,
    ]
    .into_iter()
    .any(|act| held.held(act) || map.held(keys, act) || scripted.contains(&act))
}

/// The line that says following ended.
pub(super) fn ended(end: End, name: &str, messages: &Messages) -> Said {
    let (id, fallback) = match end {
        End::Died => (lines::DIED, format!("{name} has died; you stop following.")),
        End::TooFar => (
            lines::TOO_FAR,
            format!("{name} is too far away; you stop following."),
        ),
        End::Stopped => (lines::STOPPED, format!("You stop following {name}.")),
    };
    messages.said_or(id, &[name.to_owned()], &fallback)
}

/// Starts following the target, as `/follow` does, or says why not. The
/// program says nothing for no target or one that is not a player.
pub(super) fn start(
    world: &ClientWorld,
    can_move: bool,
    messages: &Messages,
) -> Result<Following, Option<Said>> {
    let id = world.target().selected.ok_or(None)?;
    if world.is_player(id) {
        return Err(Some(
            messages.said_text(lines::SELF, "You cannot follow yourself."),
        ));
    }
    let spawn = world
        .spawn(id)
        .filter(|spawn| spawn.state.kind == SpawnKind::Player)
        .ok_or(None)?;
    let name = spawn.state.name.clone();
    let grouped = world.group().is_some_and(|group| {
        group
            .members
            .iter()
            .any(|member| member.eq_ignore_ascii_case(&name))
    });
    if !grouped {
        return Err(Some(messages.said_text(
            lines::NOT_GROUPED,
            "You can only follow a member of your group.",
        )));
    }
    if !can_move {
        return Err(Some(messages.said_text(lines::ROOTED, "You cannot move.")));
    }
    let standing = world
        .player()
        .and_then(|player| world.posture(player.spawn_id))
        .is_none_or(|posture| posture == eq_client_core::PostureState::Standing);
    if !standing {
        return Err(Some(
            messages.said_text(lines::NOT_STANDING, "You must be standing to follow."),
        ));
    }
    Ok(Following {
        session_id: world.session_id(),
        spawn_id: id,
        name,
        speed: 0.0,
    })
}

/// Says that following began.
pub(super) fn started(following: &Following, messages: &Messages, chat: &mut ChatState) {
    let name = std::slice::from_ref(&following.name);
    let fallback = [
        format!("You begin to follow {}.", following.name),
        String::new(),
        String::new(),
    ];
    for (id, fallback) in lines::STARTED.into_iter().zip(fallback) {
        // Only the first has words of this client's own where the
        // installation lacks the line.
        let said = match messages.formatted(id, name) {
            Some(text) => Said::official(text),
            None if !fallback.is_empty() => Said::own(fallback),
            None => continue,
        };
        chat.history.push(system_line(said));
    }
}

/// The heading the program turns to over a number of frames, from where
/// it faces toward where it wants to: a step of the turn left if under 4,
/// else 4 a frame up to 128 left and 9 beyond, the short way, and at once
/// within [`SNAP_DISTANCE`].
pub(super) fn turned(current: f32, desired: f32, distance: f32, frames: u32) -> f32 {
    if distance < SNAP_DISTANCE {
        return desired.rem_euclid(512.0);
    }
    let mut heading = current;
    for _ in 0..frames {
        let left = super::arc(heading, desired);
        if left.abs() < 4.0 {
            // What is left of the turn, which ends it.
            return desired.rem_euclid(512.0);
        }
        let step: f32 = if left.abs() <= 128.0 { 4.0 } else { 9.0 };
        heading = (heading + step.copysign(left)).rem_euclid(512.0);
    }
    heading
}

impl Following {
    /// Whether it began in this admission.
    pub(super) fn here(&self, world: &ClientWorld) -> bool {
        self.session_id == world.session_id()
    }

    /// The name of the one followed.
    pub(super) fn name(&self) -> &str {
        &self.name
    }

    /// What following does over this many seconds, or why it ends. `turn_limit`
    /// bounds the turn where the wire takes only a paced one.
    pub(super) fn step(
        &mut self,
        world: &ClientWorld,
        bodies: &Bodies,
        (seconds, turn_limit, now): (f32, Option<f32>, std::time::Instant),
    ) -> Result<Step, End> {
        let spawn = world.spawn(self.spawn_id).ok_or(End::Stopped)?;
        match spawn.state.kind {
            SpawnKind::Player => (),
            SpawnKind::PlayerCorpse | SpawnKind::NpcCorpse => return Err(End::Died),
            _ => return Err(End::Stopped),
        }
        let generation = world.generation();
        let there = spawn.carried(generation, now);
        let own = bodies.own;
        let offset = Vec3::new(there.x - own.x, there.y - own.y, there.z - own.z);
        let distance = offset.length();
        if distance > LEAVE_DISTANCE {
            return Err(End::TooFar);
        }
        let height =
            bodies.own_height + eq_client_core::z_offset(spawn.state.race, spawn.state.size);
        let from = Vec3::from_array(eq_client_core::render_position(own));
        let to = Vec3::from_array(eq_client_core::render_position(there));
        let flat = Vec2::new(to.x - from.x, to.z - from.z);
        let desired = if flat == Vec2::ZERO {
            own.heading
        } else {
            eq_client_core::world_heading(flat.x.atan2(flat.y))
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let frames = (seconds * FRAMES_PER_SECOND).round().max(1.0) as u32;
        let mut heading = turned(own.heading, desired, distance, frames);
        if let Some(limit) = turn_limit {
            heading = super::turn_toward(own.heading, heading, limit);
        }
        let forward = distance >= height + CLOSE;
        // The program also runs while the player already moves no faster
        // than the one followed.
        let theirs = their_speed(spawn, generation);
        let run = distance >= height + RUN || (self.speed > 0.0 && self.speed <= theirs);
        self.speed = match (forward, run) {
            (false, _) => 0.0,
            (true, true) => bodies.speeds.0,
            (true, false) => bodies.speeds.1,
        };
        Ok(Step {
            heading,
            forward,
            run,
            beyond: (distance - height - CLOSE).max(0.0),
        })
    }
}

/// How fast a spawn goes over the ground, in units a second, as the client
/// carries it on from its latest report.
///
/// Measured over a quarter second from the report, by the motion the
/// report carries, so a report just received counts as much as an old one.
fn their_speed(
    spawn: &eq_client_core::world::Spawn,
    generation: eq_client_core::GameDialect,
) -> f32 {
    const OVER: std::time::Duration = std::time::Duration::from_millis(250);
    let at = |elapsed| eq_client_core::entities::carried(generation, &spawn.state, elapsed);
    let (a, b) = (at(std::time::Duration::ZERO), at(OVER));
    Vec2::new(b.x - a.x, b.y - a.y).length() / OVER.as_secs_f32()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::{OnlineState, testing};
    use eq_client_core::{SpawnState, WorldEvent, group::GroupUpdate};

    /// A player named Friend with this spawn ID, this far north of the
    /// origin.
    fn friend(spawn_id: u16, y: f32) -> SpawnState {
        SpawnState {
            name: "Friend".into(),
            kind: SpawnKind::Player,
            race: 1,
            size: 6.0,
            pet_owner: None,
            position: WorldPosition {
                y,
                ..WorldPosition::default()
            },
            ..testing::pet(spawn_id, 0)
        }
    }

    /// The player at the origin with Friend as spawn 5, `y` away, grouped
    /// or not, and targeted.
    fn world(y: f32, grouped: bool) -> OnlineState {
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(1));
        testing::spawn_entry(&mut online, 5, friend(5, y));
        if grouped {
            testing::news(
                &mut online,
                [WorldEvent::Group(GroupUpdate::Members {
                    leader: "Friend".into(),
                    members: vec!["Friend".into()],
                })],
            );
        }
        online.select_target(Some(5));
        online
    }

    #[test]
    fn follow_starts_only_on_a_standing_player_with_a_group_member_targeted() {
        let messages = Messages::default();
        let grouped = world(50.0, true);
        let following = start(grouped.world(), true, &messages).expect("follows");
        assert_eq!(following.name(), "Friend");
        // Not in the group, the player cannot move, or the player targeted.
        let refused = |online: &OnlineState, can_move| {
            start(online.world(), can_move, &messages)
                .err()
                .flatten()
                .map(|said| said.words.text)
        };
        assert_eq!(
            refused(&world(50.0, false), true).as_deref(),
            Some("You can only follow a member of your group.")
        );
        assert_eq!(
            refused(&grouped, false).as_deref(),
            Some("You cannot move.")
        );
        let mut alone = world(50.0, true);
        alone.select_target(Some(1));
        assert_eq!(
            refused(&alone, true).as_deref(),
            Some("You cannot follow yourself.")
        );
        // No target says nothing.
        alone.select_target(None);
        assert_eq!(start(alone.world(), true, &messages).err(), Some(None));
    }

    #[test]
    fn a_correction_or_a_servers_move_keeps_following() {
        let online = world(50.0, true);
        let mut controls = super::super::Controls {
            following: start(online.world(), true, &Messages::default()).ok(),
            ..super::super::Controls::default()
        };
        controls.reset(None);
        assert!(
            controls
                .following
                .as_ref()
                .is_some_and(|following| following.here(online.world()))
        );
    }

    #[test]
    fn a_follower_already_walking_runs_while_the_one_followed_goes_faster() {
        let now = std::time::Instant::now();
        let bodies = Bodies {
            own: WorldPosition::default(),
            own_height: 3.75,
            speeds: (30.0, 10.0),
        };
        let mut online = world(25.0, true);
        let mut following = start(online.world(), true, &Messages::default()).unwrap();
        // Standing still, they are walked to.
        let first = following.step(online.world(), &bodies, (0.1, None, now));
        assert!(first.is_ok_and(|step| step.forward && !step.run));
        // Running away, faster than the player walks: the player runs.
        testing::spawn_entry(
            &mut online,
            5,
            SpawnState {
                velocity: [0.0, 40.0, 0.0],
                forward: 40.0,
                ..friend(5, 25.0)
            },
        );
        let spawn = online.world().spawn(5).unwrap();
        let theirs = their_speed(spawn, online.world().generation());
        assert!(theirs > 10.0, "{theirs}");
        let next = following.step(online.world(), &bodies, (0.1, None, now));
        assert!(next.is_ok_and(|step| step.forward && step.run));
    }

    #[test]
    fn following_closes_in_walks_near_and_ends_far_away_or_on_death() {
        let now = std::time::Instant::now();
        let bodies = Bodies {
            own: WorldPosition::default(),
            own_height: 3.75,
            speeds: (30.0, 10.0),
        };
        let messages = Messages::default();
        let step = |y: f32| {
            let online = world(y, true);
            let mut following = start(online.world(), true, &messages).unwrap();
            following.step(online.world(), &bodies, (0.1, None, now))
        };
        // Both stand heights are 3.75: under 18.75 it stops, under 33.75
        // it walks, beyond it runs.
        let close = step(15.0).unwrap();
        assert!(!close.forward);
        let near = step(25.0).unwrap();
        assert!(near.forward && !near.run);
        let far = step(60.0).unwrap();
        assert!(far.forward && far.run);
        // Within 20 units the heading snaps to them, due north.
        let north = eq_client_core::world_heading({
            let to = Vec3::from_array(eq_client_core::render_position(WorldPosition {
                y: 1.0,
                ..WorldPosition::default()
            }));
            to.x.atan2(to.z)
        });
        assert!(super::super::arc(close.heading, north).abs() < 0.01);
        assert_eq!(step(201.0), Err(End::TooFar));
        let mut online = world(50.0, true);
        let mut following = start(online.world(), true, &messages).unwrap();
        testing::spawn_entry(
            &mut online,
            5,
            SpawnState {
                kind: SpawnKind::PlayerCorpse,
                ..friend(5, 50.0)
            },
        );
        assert_eq!(
            following.step(online.world(), &bodies, (0.1, None, now)),
            Err(End::Died)
        );
        testing::news(&mut online, [WorldEvent::Despawn(5)]);
        assert_eq!(
            following.step(online.world(), &bodies, (0.1, None, now)),
            Err(End::Stopped)
        );
    }

    #[test]
    fn the_turn_steps_as_the_program_steps_it() {
        // Within 20 units the heading snaps.
        assert_eq!(turned(0.0, 300.0, 19.0, 1), 300.0);
        // Up to 128 left, 4 a frame; beyond, 9; the last step is what is left.
        assert_eq!(turned(0.0, 100.0, 50.0, 1), 4.0);
        assert_eq!(turned(0.0, 200.0, 50.0, 1), 9.0);
        assert_eq!(turned(0.0, 6.0, 50.0, 2), 6.0);
        assert_eq!(turned(0.0, 2.5, 50.0, 1), 2.5);
        // The short way round, across the wrap.
        assert_eq!(turned(10.0, 500.0, 50.0, 1), 6.0);
        // Six frames from 140 left: 9, then 4s.
        assert_eq!(
            turned(0.0, 140.0, 50.0, 6),
            9.0 + 9.0 + 4.0 + 4.0 + 4.0 + 4.0
        );
    }
}
