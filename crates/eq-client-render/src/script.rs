//! Opt-in scripted live checks that press keys through the ordinary input paths.
//!
//! A script never runs unattended: it pauses until the client window is focused,
//! stops if focus is lost while a key is held, and ends after a bounded time.
//! Scripted clicks are only injected while the real pointer is outside the window,
//! so they cannot also press whatever the pointer happens to be over. A followed
//! script keeps reading complete lines appended to its file, under the same limits.
//! `gm` steps send `#` commands only to a local `EQEmu` server, and `chat`
//! steps speak only there.
mod parse;
mod report;

pub use parse::{
    AbilityPage, CharacterClick, ClickTarget, PickButton, RaidClick, SliderClick, Step, TradeClick,
    TrainingClick, parse,
};

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::PrimaryWindow;

use parse::{MAX_STEPS, MAX_WAIT};

const MAX_ONLINE_WAIT: Duration = Duration::from_mins(3);
const MAX_RUNTIME: Duration = Duration::from_mins(15);
const FOLLOW_POLL: Duration = Duration::from_millis(250);
/// Path positions searched per frame, keeping each frame short.
const SEARCH_BUDGET: usize = 1500;
/// A frame at least this long is logged while a script runs: past it, a
/// frame shows as a hitch.
const SLOW_FRAME: Duration = Duration::from_millis(50);
/// A walk's call into its route at least this long is logged, with what the
/// route was doing.
const SLOW_ROUTE_CALL: Duration = Duration::from_millis(5);

/// Remaining steps and the one in progress.
#[derive(Resource)]
pub struct Script {
    steps: VecDeque<Step>,
    current: Option<(Step, Duration)>,
    held: Vec<KeyCode>,
    /// The mouse button a scripted click holds down until the next frame.
    clicked: Option<MouseButton>,
    /// The control a scripted click pressed, which the pointer is over as
    /// the button is let go.
    pressed: Option<Entity>,
    started: Option<Duration>,
    paused: bool,
    /// Newest chat line already included in a report.
    chat_seen: u64,
    /// Script file still being appended to, with the bytes already read.
    follow: Option<Follow>,
    accepted: usize,
    /// The session only reaches servers on this machine's network (a local
    /// `EQEmu` test server): `gm` steps may send `#` commands there, and steps run
    /// without anyone watching the window.
    local: bool,
    /// The preview runs offline: no step can reach a server, so steps run
    /// without anyone watching the window, but `gm` steps stay refused.
    offline: bool,
    /// The window focus winit last reported; None until it reports any.
    focus: Option<bool>,
    /// The route a `walk` step is searching for or following.
    route: Option<eq_client_core::movement::Route>,
    /// The closed door the walk is opening, if any.
    door_wait: Option<DoorWait>,
    /// Where the walk last headed, for the stall line and the follower trace.
    following: Option<Following>,
    /// Since when a following walk has seen no movement sample sent, and
    /// whether it has said so.
    quiet: Option<Quiet>,
    /// While a walk's route searches: when the search began, the last frame,
    /// and the longest frame so far, so the log shows how long the walk
    /// waited and whether the search held frames up.
    search_frames: Option<SearchFrames>,
    /// When the last frame ran, so a slow one is logged with what the script
    /// was doing.
    last_frame: Option<Duration>,
    /// The control a `hover` step rests the pointer on, until the next click
    /// or hover.
    hovering: Option<Entity>,
    /// The spawn a `wait_dead` or `chase` step waits on: the target as it
    /// started.
    awaited: Option<u16>,
    /// The world container an `approach object` or `walk object` step walks
    /// toward, chosen as it started.
    approached: Option<u32>,
    /// A `chase` step's view of its target: the last report it saw and when,
    /// so the aim carries that report forward as the target is drawn.
    chase_seen: Option<(eq_client_core::WorldPosition, Duration)>,
    /// The count of the world's creation answers when a `create` step sent
    /// its request, so the step waits for the next.
    created_before: Option<u64>,
}

/// A walk's frames while its route searches.
#[derive(Clone, Copy)]
struct SearchFrames {
    began: Duration,
    last: Duration,
    slowest: Duration,
    /// Frames so far, counting the one that began the search as 1.
    count: u32,
}

struct Follow {
    path: PathBuf,
    offset: usize,
    next_poll: Duration,
    idle: bool,
}

impl Script {
    /// Queues validated steps.
    #[must_use]
    pub fn new(steps: Vec<Step>) -> Self {
        Self {
            accepted: steps.len(),
            steps: steps.into(),
            current: None,
            held: Vec::new(),
            clicked: None,
            pressed: None,
            started: None,
            paused: false,
            chat_seen: 0,
            follow: None,
            local: false,
            offline: false,
            focus: None,
            route: None,
            door_wait: None,
            following: None,
            quiet: None,
            search_frames: None,
            last_frame: None,
            hovering: None,
            awaited: None,
            approached: None,
            chase_seen: None,
            created_before: None,
        }
    }

    /// Queues validated steps, then keeps reading complete lines appended to `path`
    /// after the first `offset` bytes.
    #[must_use]
    pub fn following(steps: Vec<Step>, path: PathBuf, offset: usize) -> Self {
        Self {
            follow: Some(Follow {
                path,
                offset,
                next_poll: Duration::ZERO,
                idle: false,
            }),
            ..Self::new(steps)
        }
    }

    /// Marks the session local-only (see [`Script::local`]); set only when the
    /// network session refuses servers outside this machine's network.
    #[must_use]
    pub fn local_session(mut self, local: bool) -> Self {
        self.local = local;
        self
    }

    /// Whether the script runs without anyone watching the window: on a
    /// local-only session, or offline.
    #[must_use]
    pub(crate) const fn unattended(&self) -> bool {
        self.local || self.offline
    }

    /// Marks the run offline (see [`Script::offline`]).
    #[must_use]
    pub fn offline_preview(mut self, offline: bool) -> Self {
        self.offline = offline;
        self
    }

    /// Queues complete appended lines; an invalid batch is skipped and logged.
    fn poll(&mut self, now: Duration) {
        let Some(follow) = &mut self.follow else {
            return;
        };
        if now < follow.next_poll {
            return;
        }
        follow.next_poll = now + FOLLOW_POLL;
        let Ok(bytes) = std::fs::read(&follow.path) else {
            return;
        };
        let Some(fresh) = bytes.get(follow.offset..) else {
            return;
        };
        let Some(end) = fresh.iter().rposition(|byte| *byte == b'\n') else {
            if !std::mem::replace(&mut follow.idle, true) {
                info!("Script waiting for appended steps");
            }
            return;
        };
        follow.offset += end + 1;
        follow.idle = false;
        let text = String::from_utf8_lossy(&fresh[..=end]).into_owned();
        let base = follow
            .path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        match parse(&text, &base) {
            Ok(steps) if self.accepted + steps.len() <= MAX_STEPS => {
                self.accepted += steps.len();
                self.steps.extend(steps);
            }
            Ok(_) => warn!("Ignored appended steps beyond the {MAX_STEPS}-step limit"),
            Err(error) => warn!("Ignored appended script lines: {error}"),
        }
    }

    fn release(&mut self, keys: &mut Keyboard, mouse: &mut ButtonInput<MouseButton>) {
        for key in self.held.drain(..) {
            keys.release(key);
        }
        if let Some(button) = self.clicked.take() {
            mouse.release(button);
        }
        self.pressed = None;
    }

    fn stop(&mut self, keys: &mut Keyboard, mouse: &mut ButtonInput<MouseButton>, reason: &str) {
        self.release(keys, mouse);
        self.steps.clear();
        self.current = None;
        self.follow = None;
        self.route = None;
        self.awaited = None;
        self.approached = None;
        info!("Script stopped: {reason}");
    }
}

type Buttons<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Interaction,
        &'static InheritedVisibility,
        Option<&'static super::inventory::SlotButton>,
        Has<super::spellbook::ScribeCursor>,
        Has<super::inventory::StoreCursor>,
        (
            Option<&'static super::spellbook::BookEntry>,
            Option<&'static super::spellbook::BookPlace>,
            Option<&'static super::spellbook::TurnsPages>,
            Option<&'static super::hud::SpellGem>,
            Option<&'static super::hud::hotbar::Slot>,
        ),
        Option<&'static super::spellbook::GemChoice>,
        Option<&'static super::trade::Action>,
        Option<&'static super::inventory::colors::Action>,
        (
            Has<super::give::GiveButton>,
            Option<&'static super::character_select::Action>,
            Option<&'static super::chat::TabButton>,
        ),
        (
            Option<&'static super::coins::CoinBox>,
            Option<&'static super::inventory::SplitAction>,
            Option<&'static super::skinned::scrollbar::ScrollArrow>,
            Has<super::skinned::AmountBox>,
        ),
        (
            Option<&'static super::windows::SelectorButton>,
            Option<&'static super::skinned::SkinTab>,
            Option<&'static super::abilities::AbilityButton>,
            Has<super::skinned::AttackButton>,
            Option<&'static super::skinned::SlashButton>,
            Option<&'static super::options::OptionCheckbox>,
        ),
        (
            Option<&'static super::skinned::SkinSlider>,
            Option<&'static super::skinned::DropDown>,
            Option<&'static super::skinned::DropDownChoice>,
            (
                (
                    Option<&'static super::training::SkillRow>,
                    Has<super::training::TrainButton>,
                    Option<&'static super::skinned::Closes>,
                    Option<&'static super::confirm::AnswerButton>,
                    Option<&'static super::skinned::TitleBox>,
                ),
                Option<&'static super::reading::PageButton>,
                Option<&'static super::tradeskills::CombineButton>,
                Option<&'static super::map::MapButton>,
                (
                    Option<&'static super::raid::RaidRow>,
                    Option<&'static super::raid::RaidAction>,
                    Option<&'static super::skinned::Shows>,
                    Option<&'static super::skinned::SkinId>,
                ),
            ),
        ),
    ),
>;

/// Where the pointer is over each slider, which a scripted press sets.
type Pointers<'w, 's> = Query<'w, 's, &'static mut bevy::ui::RelativeCursorPosition>;

/// Every UI node's display and parent: a closed window is only left out of
/// the layout, so its buttons keep their visibility.
type Layout<'w, 's> = Query<'w, 's, (&'static Node, Option<&'static ChildOf>)>;

type Observed<'w> = (
    Res<'w, super::hud::HudState>,
    Res<'w, crate::outbox::Outbox>,
    Res<'w, super::trade::TradeState>,
    Res<'w, super::combat::CombatState>,
    Res<'w, super::motion::Controls>,
    Option<Res<'w, crate::loading::Loading>>,
    Option<Res<'w, super::doors::Hinges>>,
);

type Input<'w, 's> = (Keyboard<'w, 's>, ResMut<'w, ButtonInput<MouseButton>>);

/// The keyboard as a script presses it: each press and release sets the
/// key's state and sends the typed key the window would, so what reads
/// typed keys (the chat line, the login boxes) takes a scripted key as it
/// takes a real one.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Keyboard<'w, 's> {
    state: ResMut<'w, ButtonInput<KeyCode>>,
    typed: MessageWriter<'w, bevy::input::keyboard::KeyboardInput>,
    window: Query<'w, 's, Entity, With<PrimaryWindow>>,
}

impl Keyboard<'_, '_> {
    fn press(&mut self, key: KeyCode) {
        if !self.state.pressed(key) {
            self.send(key, bevy::input::ButtonState::Pressed);
        }
        self.state.press(key);
    }

    fn release(&mut self, key: KeyCode) {
        if self.state.pressed(key) {
            self.send(key, bevy::input::ButtonState::Released);
        }
        self.state.release(key);
    }

    fn send(&mut self, key_code: KeyCode, state: bevy::input::ButtonState) {
        let shift = self
            .state
            .any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let chorded = self.state.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
        ]);
        let (logical_key, text) = parse::typed(key_code, shift, chorded);
        self.typed.write(bevy::input::keyboard::KeyboardInput {
            key_code,
            logical_key,
            state,
            // A release types nothing.
            text: text.filter(|_| state.is_pressed()).map(Into::into),
            repeat: false,
            window: self.window.single().unwrap_or(Entity::PLACEHOLDER),
        });
    }
}

/// Runs after Bevy's keyboard and UI focus updates so injected input is seen this frame.
#[allow(
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
pub(super) fn drive(
    mut commands: Commands,
    time: Res<Time<Real>>,
    script: Option<ResMut<Script>>,
    input: Input,
    mut online: ResMut<super::online::OnlineState>,
    (mut chat, mut moves, mut front, mut sounds): (
        ResMut<super::chat::ChatState>,
        ResMut<super::windows::Moves>,
        ResMut<super::login::FrontEnd>,
        ResMut<super::sounds::Sounds>,
    ),
    observed: Observed,
    players: Query<&Transform, With<super::Player>>,
    bodies: Query<&super::PlayerBody, With<super::Player>>,
    collision: Option<Res<super::Collision>>,
    mut cameras: Query<&mut super::OrbitCamera>,
    (mut buttons, layout, mut pointers, needs): (
        Buttons,
        Layout,
        Pointers,
        Query<&crate::outbox::Needs>,
    ),
    windows: Query<&Window, With<PrimaryWindow>>,
    mut focus: MessageReader<bevy::window::WindowFocused>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut script) = script else {
        return;
    };
    // Plain borrows of the fields, so a walk can hold its route and note
    // where it heads at once.
    let script = &mut *script;
    if let Some(event) = focus.read().last() {
        script.focus = Some(event.focused);
    }
    let (mut keys, mut mouse) = input;
    let now = time.elapsed();
    if let Some(last) = script.last_frame.replace(now)
        && now.saturating_sub(last) >= SLOW_FRAME
    {
        let ms = now.saturating_sub(last).as_secs_f32() * 1000.0;
        let walking = script.route.is_some();
        let searching = script
            .route
            .as_ref()
            .is_some_and(eq_client_core::movement::Route::is_searching);
        // The frame just ended was this one since the walk's search began;
        // with `searching` false, it was the frame that took the result.
        let search_frame = script.search_frames.map(|frames| frames.count);
        info!(ms, walking, searching, ?search_frame, "Script slow frame");
    }
    let started = *script.started.get_or_insert(now);
    if now.saturating_sub(started) > MAX_RUNTIME {
        script.stop(&mut keys, &mut mouse, "maximum runtime reached");
        commands.remove_resource::<Script>();
        return;
    }
    if script.current.is_none() && script.steps.is_empty() {
        if script.follow.is_none() {
            commands.remove_resource::<Script>();
            info!("Script finished");
            return;
        }
        script.poll(now);
        if script.steps.is_empty() {
            return;
        }
    }
    let window = windows.single().ok();
    if !may_run(
        script.unattended(),
        script.focus,
        window.is_some_and(|window| window.focused),
    ) {
        if !script.held.is_empty() {
            script.stop(
                &mut keys,
                &mut mouse,
                "window lost focus while holding keys",
            );
        } else if !script.paused {
            script.paused = true;
            info!("Script paused until the client window is focused");
        }
        return;
    }
    if std::mem::take(&mut script.paused) {
        info!("Script resumed");
    }
    // The focus system clears a hover no pointer is over; a hovered control
    // stays hovered.
    if let Some(entity) = script.hovering
        && let Ok((_, mut interaction, ..)) = buttons.get_mut(entity)
    {
        interaction.set_if_neq(Interaction::Hovered);
    }
    // One-frame presses and clicks are released on the frame after they were pressed.
    if matches!(
        script.current,
        Some((
            Step::Press(_) | Step::Select(_) | Step::Click(_) | Step::RightClick(_),
            _
        ))
    ) && (!script.held.is_empty() || script.clicked.is_some())
    {
        // Bevy marks the control under the pointer hovered as the button is
        // let go, and the hold rule clicks a control only then.
        if let Some(entity) = script.pressed
            && let Ok((_, mut interaction, ..)) = buttons.get_mut(entity)
        {
            interaction.set_if_neq(Interaction::Hovered);
        }
        script.release(&mut keys, &mut mouse);
        script.current = None;
    }
    if let Some((step, since)) = script.current.clone() {
        let elapsed = now.saturating_sub(since);
        let done = match &step {
            Step::Wait(duration) => elapsed >= *duration,
            Step::Create(character) => {
                match answered(online.world(), script.created_before, &character.name) {
                    Ok(done) => done,
                    Err(reason) => {
                        script.stop(&mut keys, &mut mouse, &reason);
                        return;
                    }
                }
            }
            Step::WaitDead(limit) => {
                match fallen(online.world(), script.awaited, elapsed, *limit) {
                    Ok(done) => done,
                    Err(reason) => {
                        script.stop(&mut keys, &mut mouse, reason);
                        return;
                    }
                }
            }
            Step::Approach(range, duration) => {
                let distance = face(&online, &players, &mut cameras);
                distance.is_none_or(|distance| distance <= *range) || elapsed >= *duration
            }
            Step::Chase(range, limit) => match chase(
                online.world(),
                script.awaited,
                &mut script.chase_seen,
                elapsed,
                *limit,
            ) {
                Ok(Chase::Caught) => true,
                Ok(Chase::OutOfTime) => {
                    info!("Script chase ended without a kill");
                    true
                }
                Ok(Chase::Aim(aim)) => {
                    let running = runs(face_toward(aim, &players, &mut cameras), *range);
                    if running && script.held.is_empty() {
                        keys.press(KeyCode::KeyW);
                        script.held = vec![KeyCode::KeyW];
                    } else if !running && !script.held.is_empty() {
                        script.release(&mut keys, &mut mouse);
                    }
                    false
                }
                Err(reason) => {
                    script.stop(&mut keys, &mut mouse, reason);
                    return;
                }
            },
            Step::ApproachObject {
                range, duration, ..
            } => {
                let Some(distance) =
                    face_object(script.approached, &online, &players, &mut cameras)
                else {
                    script.stop(&mut keys, &mut mouse, "approach object lost its object");
                    return;
                };
                if distance > *range && elapsed >= *duration {
                    // A later step such as `press f` would act on nothing.
                    script.stop(&mut keys, &mut mouse, "approach object ran out of time");
                    return;
                }
                distance <= *range
            }
            Step::Walk(_, duration) | Step::WalkObject { duration, .. } => {
                use eq_client_core::movement::RouteStep;
                // An object walk ends only within its range; a plain walk also
                // ends at the end of a partial route or out of time.
                let object_range = match &step {
                    Step::WalkObject { range, .. } => Some(*range),
                    _ => None,
                };
                // Only an object walk has chosen an object.
                let body = bodies.single().ok().copied();
                let ends = body.and_then(|body| walk_ends(&online, body, script.approached));
                let world = collision
                    .as_ref()
                    .and_then(|collision| collision.0.as_ref());
                let (Some((feet, goal)), Some(world), Some(body)) = (ends, world, body) else {
                    script.stop(&mut keys, &mut mouse, "walk lost its goal or collision");
                    return;
                };
                let height = body.height;
                let (waited, slowest_frame) = if script
                    .route
                    .as_ref()
                    .is_some_and(eq_client_core::movement::Route::is_searching)
                {
                    let frames = script.search_frames.get_or_insert(SearchFrames {
                        began: now,
                        last: now,
                        slowest: Duration::ZERO,
                        count: 0,
                    });
                    frames.slowest = frames.slowest.max(now.saturating_sub(frames.last));
                    frames.last = now;
                    frames.count += 1;
                    (
                        now.saturating_sub(frames.began).as_secs_f32(),
                        frames.slowest.as_secs_f32() * 1000.0,
                    )
                } else {
                    script.search_frames = None;
                    (0.0, 0.0)
                };
                let Some(route) = script.route.as_mut() else {
                    script.stop(&mut keys, &mut mouse, "walk lost its route");
                    return;
                };
                let searching = route.is_searching();
                let call = std::time::Instant::now();
                // Off the ground, the fall brings the walker to its waypoint.
                let falling = observed
                    .4
                    .airborne
                    .as_ref()
                    .is_some_and(|airborne| airborne.peak().is_some());
                let step = route.next_with(world, (feet, falling), goal, SEARCH_BUDGET, now);
                let call = call.elapsed();
                if let RouteStep::Toward(toward) = step {
                    let traced = script.following.and_then(|following| following.traced);
                    let mut following = Following {
                        toward,
                        index: route.waypoints_reached(),
                        ahead: route.remaining(),
                        left: route.distance_left(feet, goal),
                        traced,
                    };
                    if traced.is_none_or(|traced| now.saturating_sub(traced) >= TRACE_EVERY) {
                        following.traced = Some(now);
                        debug!(
                            feet = ?eq_point(feet),
                            toward = ?eq_point(toward),
                            index = following.index,
                            ahead = following.ahead,
                            left = following.left,
                            "Script walk following"
                        );
                    }
                    script.following = Some(following);
                    log_quiet(&mut script.quiet, &observed.4, std::time::Instant::now());
                } else {
                    script.quiet = None;
                }
                if call >= SLOW_ROUTE_CALL {
                    let phase = match (searching, route.is_searching()) {
                        (true, false) => "result",
                        (true, true) => "searching",
                        (false, true) => "search again",
                        (false, false) => "following",
                    };
                    let ms = call.as_secs_f32() * 1000.0;
                    info!(ms, phase, "Script route call slow");
                }
                if searching
                    && !route.is_searching()
                    && let Some(found) = route.detoured()
                {
                    let (expanded, _) = route.searched().unwrap_or_default();
                    let waypoints = route.remaining();
                    let timing = route.search_timing().unwrap_or_default();
                    info!(
                        queued = timing.queued.as_secs_f32(),
                        ran = timing.ran.as_secs_f32(),
                        found,
                        expanded,
                        waypoints,
                        waited,
                        slowest_frame,
                        "Script walk detour ready"
                    );
                } else if searching && !route.is_searching() {
                    let (waypoints, partial) = (route.remaining(), route.partial());
                    let (expanded, open) = route.searched().unwrap_or_default();
                    let (height, drops) = (route.height(), observed.4.airborne.is_some());
                    let reach = route.reach();
                    let timing = route.search_timing().unwrap_or_default();
                    info!(
                        waypoints,
                        waited,
                        queued = timing.queued.as_secs_f32(),
                        ran = timing.ran.as_secs_f32(),
                        obstacles = timing.obstacles,
                        snapshot_ms = timing.snapshot.as_secs_f32() * 1000.0,
                        handoff_ms = timing.handoff.as_secs_f32() * 1000.0,
                        slowest_frame,
                        partial,
                        reach,
                        expanded,
                        open,
                        ?feet,
                        ?goal,
                        height,
                        drops,
                        "Script route ready"
                    );
                    if partial && let Some(end) = route.end() {
                        log_partial_route(&online, end);
                    }
                } else if !searching && route.is_searching() {
                    log_stall(true, (feet, height), script.following, &observed.4, world);
                }
                let distance = Vec2::new(goal.x - feet.x, goal.z - feet.z).length();
                // Where an object walk ends, so a partial route shows in the log.
                let log_end = |online: &super::online::OnlineState| {
                    if object_range.is_some()
                        && let Some(player) = online.world().player()
                    {
                        let at = player.position;
                        info!(
                            x = at.x,
                            y = at.y,
                            z = at.z,
                            distance,
                            "Script walk object ended"
                        );
                    }
                };
                let (arrived, out_of_time) = match step {
                    RouteStep::Unreachable => {
                        log_end(&online);
                        script.stop(&mut keys, &mut mouse, "no walkable path from here");
                        return;
                    }
                    RouteStep::Stalled => {
                        let refused = observed.4.refused.as_deref();
                        log_stall(false, (feet, height), script.following, &observed.4, world);
                        let reason = refused.map_or_else(
                            || "walk made no progress, even after searching again".to_owned(),
                            |refused| {
                                format!("walk made no progress; last step refused: {refused}")
                            },
                        );
                        log_end(&online);
                        script.stop(&mut keys, &mut mouse, &reason);
                        return;
                    }
                    RouteStep::Arrived => (true, false),
                    RouteStep::OpenDoor(id) => {
                        // Stand at the door: click it as the use key would, then wait.
                        script.release(&mut keys, &mut mouse);
                        let door = online.world().doors().entries().get(&id);
                        let within_use =
                            door.zip(online.world().player())
                                .is_some_and(|(door, player)| {
                                    let at =
                                        |p| Vec3::from_array(eq_client_core::render_position(p));
                                    at(door.position).distance(at(player.position))
                                        <= eq_client_core::doors::Doors::USE_DISTANCE
                                });
                        let mut wait = script
                            .door_wait
                            .filter(|wait| wait.id == id)
                            .unwrap_or(DoorWait::new(id));
                        let open = door.and_then(eq_client_core::doors::Door::active_endpoint);
                        let hinge = observed
                            .6
                            .as_ref()
                            .and_then(|hinges| hinges.0.get(&id).copied());
                        match wait.act(open, hinge, within_use, now) {
                            DoorAction::Click => {
                                if let Some(door) = door {
                                    info!(
                                        id,
                                        model = %door.model,
                                        open_type = door.open_type,
                                        x = door.position.x,
                                        y = door.position.y,
                                        z = door.position.z,
                                        "Script walk opening door"
                                    );
                                }
                                super::doors::open(id, &online, &observed.1);
                                script.door_wait = Some(wait);
                            }
                            DoorAction::Wait => script.door_wait = Some(wait),
                            DoorAction::Reroute => {
                                info!(id, ?hinge, "Script walk door open; searching around it");
                                script.door_wait = Some(wait);
                                if let Some(route) = script.route.as_mut() {
                                    route.reroute(feet, goal);
                                }
                            }
                            DoorAction::Refuse(reason) => {
                                info!(id, reason, "Script walk door did not open");
                                script.door_wait = None;
                                if let Some(route) = script.route.as_mut() {
                                    route.refused(id, feet, goal);
                                }
                            }
                        }
                        (false, elapsed >= *duration)
                    }
                    RouteStep::Searching => {
                        // Stand still while a stalled route searches again.
                        script.release(&mut keys, &mut mouse);
                        (false, elapsed >= *duration)
                    }
                    RouteStep::Toward(next) => {
                        if let Some(DoorWait {
                            id,
                            asked: Some(asked),
                            ..
                        }) = script.door_wait.take()
                        {
                            let waited = now.saturating_sub(asked).as_secs_f32();
                            info!(id, waited, "Script walk door opened");
                        }
                        let to = next - feet;
                        for mut camera in &mut cameras {
                            camera.yaw = super::behind(to.x.atan2(to.z));
                        }
                        if script.held.is_empty() {
                            keys.press(KeyCode::KeyW);
                            script.held = vec![KeyCode::KeyW];
                        }
                        (false, elapsed >= *duration)
                    }
                };
                match object_range
                    .map(|range| object_walk_end(distance, range, arrived, out_of_time))
                {
                    None => arrived || out_of_time,
                    Some(Ok(done)) => {
                        if done {
                            log_end(&online);
                        }
                        done
                    }
                    Some(Err(reason)) => {
                        log_end(&online);
                        script.stop(&mut keys, &mut mouse, &reason);
                        return;
                    }
                }
            }
            // Held movement keys are traced too, to measure motion cadence.
            Step::Hold(_, duration) | Step::Trace(duration) => {
                if let Ok(transform) = players.single() {
                    let (x, y, z, heading) = report::placement(transform);
                    debug!(
                        dt_ms = time.delta_secs() * 1000.0,
                        x, y, z, heading, "Script trace"
                    );
                }
                elapsed >= *duration
            }
            Step::WaitServers | Step::WaitSelect | Step::WaitOnline | Step::WaitZone(_) => {
                if elapsed > MAX_ONLINE_WAIT {
                    script.stop(&mut keys, &mut mouse, "server state did not arrive");
                    return;
                }
                if matches!(step, Step::WaitServers) {
                    front.screen(&online) == super::login::Screen::Servers
                        && online
                            .world()
                            .servers()
                            .is_some_and(|list| list.asked.is_none())
                } else {
                    let covered = observed.5.as_ref().is_some_and(|loading| loading.covered());
                    arrived(&step, &online, covered)
                }
            }
            Step::Click(target) | Step::RightClick(target) => {
                if elapsed > MAX_WAIT {
                    script.stop(&mut keys, &mut mouse, "the pointer stayed over the window");
                    return;
                }
                if window.is_some_and(|window| window.cursor_position().is_some()) {
                    return;
                }
                let entity = match reach(*target, &buttons, &layout, &needs, online.world()) {
                    Ok(entity) => entity,
                    Err(reason) => {
                        script.stop(&mut keys, &mut mouse, &reason);
                        return;
                    }
                };
                press(&mut commands, entity, *target, &mut buttons, &mut pointers);
                let button = if matches!(step, Step::RightClick(_)) {
                    MouseButton::Right
                } else {
                    MouseButton::Left
                };
                mouse.press(button);
                script.clicked = Some(button);
                script.pressed = Some(entity);
                return;
            }
            Step::HoldClick(target, duration) => {
                if script.clicked.is_none() {
                    // Pressed as a click presses, the hold is timed from the
                    // press.
                    if elapsed > MAX_WAIT {
                        script.stop(&mut keys, &mut mouse, "the pointer stayed over the window");
                        return;
                    }
                    if window.is_some_and(|window| window.cursor_position().is_some()) {
                        return;
                    }
                    let entity = match reach(*target, &buttons, &layout, &needs, online.world()) {
                        Ok(entity) => entity,
                        Err(reason) => {
                            script.stop(&mut keys, &mut mouse, &reason);
                            return;
                        }
                    };
                    press(&mut commands, entity, *target, &mut buttons, &mut pointers);
                    mouse.press(MouseButton::Left);
                    script.clicked = Some(MouseButton::Left);
                    script.pressed = Some(entity);
                    script.current = Some((step.clone(), now));
                    return;
                }
                if elapsed < *duration {
                    return;
                }
                // Let go over the control it pressed, as a click lets go.
                if let Some(entity) = script.pressed
                    && let Ok((_, mut interaction, ..)) = buttons.get_mut(entity)
                {
                    interaction.set_if_neq(Interaction::Hovered);
                }
                true
            }
            _ => true,
        };
        if !done {
            return;
        }
        script.release(&mut keys, &mut mouse);
        script.current = None;
        script.route = None;
        script.following = None;
        script.quiet = None;
        script.awaited = None;
        script.approached = None;
    }
    let Some(step) = script.steps.pop_front() else {
        return;
    };
    info!(?step, "Script step");
    match &step {
        Step::Create(character) => {
            let sent = online.selection.as_ref().is_some_and(|selection| {
                observed
                    .1
                    .send(
                        online.world(),
                        eq_client_core::ClientCommand::CreateCharacter {
                            selection_id: selection.id(),
                            character: character.clone(),
                        },
                    )
                    .is_ok()
            });
            if !sent {
                script.stop(
                    &mut keys,
                    &mut mouse,
                    "character creation could not be sent",
                );
                return;
            }
            // The step waits for the world's answer to this request.
            script.created_before = online.world().creation().map(|last| last.count);
        }
        Step::Slash(_) | Step::Gm(_) | Step::Chat(_) if !online.in_world() => {
            // The worker discards commands while zoning, dead or disconnected.
            script.stop(
                &mut keys,
                &mut mouse,
                concat!(
                    "slash, gm and chat steps need a character in the world; ",
                    "wait_online or wait_zone first"
                ),
            );
            return;
        }
        Step::Drag(window, to) => {
            if let Some(id) = super::windows::WindowId::named(window) {
                moves.0.push((id, *to));
            }
            return;
        }
        Step::Slash(command) => {
            let queued = super::chat::client_request(command, &mut chat)
                .unwrap_or_else(|| super::chat::submit_game_command(command, &online, &observed.1));
            if let Err(error) = queued {
                script.stop(&mut keys, &mut mouse, &error);
            }
            return;
        }
        Step::Give => {
            let Some(target) = online.world().target().selected else {
                script.stop(&mut keys, &mut mouse, "give needs a target");
                return;
            };
            super::give::offer(target, &mut online, &observed.1);
            return;
        }
        Step::Gm(_) | Step::Chat(_) => {
            let line = match &step {
                Step::Gm(command) => gm_chat(command, script.local),
                Step::Chat(chat) => local_chat(chat, script.local),
                _ => return,
            };
            let sent = line.and_then(|chat| {
                observed
                    .1
                    .send(
                        online.world(),
                        eq_client_core::ClientCommand::SendChat(chat),
                    )
                    .map_err(|refusal| refusal.text().to_owned())
            });
            if let Err(error) = sent {
                script.stop(&mut keys, &mut mouse, &error);
            }
            return;
        }
        Step::Press(chord) | Step::Hold(chord, _) => {
            for key in chord {
                keys.press(*key);
            }
            script.held.clone_from(chord);
        }
        Step::Login => {
            if front.screen(&online) != super::login::Screen::Login || front.running() {
                script.stop(&mut keys, &mut mouse, "login needs the login screen");
            } else if let Err(reason) = front.type_scripted() {
                script.stop(&mut keys, &mut mouse, reason);
            } else {
                front.request = Some(super::login::Request::Connect);
            }
            return;
        }
        Step::Server(name) => {
            let listed = online.world().servers().and_then(|list| {
                list.servers
                    .iter()
                    .position(|server| server.name.eq_ignore_ascii_case(name))
                    .map(|index| (index, list.servers[index].status.open()))
            });
            match listed {
                Some((index, true)) => {
                    front.highlighted = Some(index);
                    front.request = Some(super::login::Request::Play);
                }
                Some((_, false)) => {
                    script.stop(&mut keys, &mut mouse, "world takes no players now");
                }
                None => script.stop(&mut keys, &mut mouse, "world is not listed"),
            }
            return;
        }
        Step::Select(name) => {
            if !online
                .selection
                .as_mut()
                .is_some_and(|selection| selection.choose_named(name))
            {
                script.stop(&mut keys, &mut mouse, "character is not listed");
                return;
            }
            keys.press(KeyCode::Enter);
            script.held = vec![KeyCode::Enter];
        }
        Step::Camera {
            heading,
            relative,
            pitch,
        } => {
            let base = if *relative {
                players
                    .single()
                    .map_or(0.0, |transform| report::placement(transform).3)
            } else {
                0.0
            };
            for mut camera in &mut cameras {
                camera.yaw = super::behind(eq_client_core::render_heading(base + *heading));
                camera.pitch = pitch.to_radians().clamp(-1.45, -0.15);
            }
            return;
        }
        Step::Click(_) | Step::RightClick(_) | Step::HoldClick(..) => {
            script.hovering = None;
            if window.is_some_and(|window| window.cursor_position().is_some()) {
                info!("Scripted click waits until the pointer leaves the client window");
            }
        }
        Step::Hover(target) => {
            match find(*target, &buttons, &layout) {
                Some(entity) => {
                    // A hover rests on the control, even one just clicked.
                    commands
                        .entity(entity)
                        .remove::<super::tooltip::ScriptPressed>();
                    script.hovering = Some(entity);
                }
                None => script.stop(&mut keys, &mut mouse, "hover target is not visible"),
            }
            return;
        }
        Step::Report(label) => {
            report::state(label, &online, &observed, players.single().ok());
            report::surroundings(&online, &observed);
            report::game_messages(&mut script.chat_seen, &chat);
            return;
        }
        Step::Sound(name) => {
            info!(name, "Script plays a sound");
            sounds.play(name, None);
            return;
        }
        Step::Screenshot(path) => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path.clone()));
            return;
        }
        Step::Quit => {
            script.stop(&mut keys, &mut mouse, "quit requested");
            exit.write(AppExit::Success);
            return;
        }
        Step::Face => {
            if face(&online, &players, &mut cameras).is_none() {
                script.stop(&mut keys, &mut mouse, "face needs a visible target");
            }
            return;
        }
        Step::Approach(..) => {
            if face(&online, &players, &mut cameras).is_none() {
                script.stop(&mut keys, &mut mouse, "approach needs a visible target");
                return;
            }
            keys.press(KeyCode::KeyW);
            script.held = vec![KeyCode::KeyW];
        }
        Step::Chase(..) => {
            script.awaited = online.world().target().selected;
            script.chase_seen = None;
            if face(&online, &players, &mut cameras).is_none() {
                script.stop(&mut keys, &mut mouse, "chase needs a visible target");
                return;
            }
        }
        Step::ApproachObject { kind, .. } => {
            script.approached = nearest_station(&online, *kind);
            if face_object(script.approached, &online, &players, &mut cameras).is_none() {
                script.stop(
                    &mut keys,
                    &mut mouse,
                    "approach object found no container of that kind",
                );
                return;
            }
            keys.press(KeyCode::KeyW);
            script.held = vec![KeyCode::KeyW];
        }
        Step::Walk(range, _) | Step::WalkObject { range, .. } => {
            if let Step::WalkObject { kind, .. } = &step {
                script.approached = nearest_station(&online, *kind);
                if script.approached.is_none() {
                    script.stop(
                        &mut keys,
                        &mut mouse,
                        "walk object found no container of that kind",
                    );
                    return;
                }
            }
            let creating = std::time::Instant::now();
            let route = bodies.single().ok().and_then(|body| {
                let (feet, goal) = walk_ends(&online, *body, script.approached)?;
                // Ledges are routes only where the session simulates falls.
                Some(
                    eq_client_core::movement::Route::new(feet, goal, *range, body.height)
                        .with_drops(observed.4.airborne.is_some())
                        .with_doors(openable_doors(&online))
                        .with_background(std::sync::Arc::new(super::background::ComputePool)),
                )
            });
            let creating = creating.elapsed();
            if creating >= SLOW_ROUTE_CALL {
                info!(
                    ms = creating.as_secs_f32() * 1000.0,
                    "Script walk route made slowly"
                );
            }
            let Some(route) = route else {
                script.stop(&mut keys, &mut mouse, "walk needs a visible target");
                return;
            };
            script.route = Some(route);
            script.door_wait = None;
            script.following = None;
            script.quiet = None;
        }
        Step::WaitDead(_) => script.awaited = online.world().target().selected,
        Step::WaitServers
        | Step::WaitSelect
        | Step::WaitOnline
        | Step::WaitZone(_)
        | Step::Wait(_)
        | Step::Trace(_) => (),
    }
    script.current = Some((step, now));
}

/// Whether the world answered a `create` step's request since it was sent:
/// done once it created the character, and the step's failure when it
/// refused it.
fn answered(
    world: &eq_client_core::world::ClientWorld,
    before: Option<u64>,
    name: &str,
) -> Result<bool, String> {
    use eq_client_core::creation::CreationOutcome;
    let Some(answer) = world
        .creation()
        .filter(|answer| Some(answer.count) != before && answer.name == name)
    else {
        return Ok(false);
    };
    match answer.outcome {
        CreationOutcome::Created => Ok(true),
        refused => Err(format!("character creation refused: {refused:?}")),
    }
}

/// Steps run only while someone watches the window, unless the session is
/// local-only or there is none. A window counts as focused before winit
/// reports anything (and a hidden one never gets a report), so only a
/// reported focus counts.
fn may_run(unattended: bool, reported_focus: Option<bool>, window_focused: bool) -> bool {
    unattended || (reported_focus == Some(true) && window_focused)
}

/// The say line carrying a `gm` step's `#` command, refused unless the session is
/// on a local `EQEmu` or TAKP server.
fn gm_chat(command: &str, allowed: bool) -> Result<eq_client_core::OutboundChat, String> {
    if !allowed {
        return Err(
            "gm steps only run on a local EQEmu or TAKP server (EQ_PROTOCOL=eqemu or takp)".into(),
        );
    }
    Ok(eq_client_core::OutboundChat::Say(format!("#{command}")))
}

/// A `chat` step's line, refused unless the session is on a local `EQEmu` or
/// TAKP server, as `gm` steps are.
fn local_chat(
    chat: &eq_client_core::OutboundChat,
    allowed: bool,
) -> Result<eq_client_core::OutboundChat, String> {
    if !allowed {
        return Err(
            "chat steps only run on a local EQEmu or TAKP server (EQ_PROTOCOL=eqemu or takp)"
                .into(),
        );
    }
    Ok(chat.clone())
}

/// Whether a button of the skin's character list or a chat tab is the one
/// a click names.
fn listed_control(
    target: ClickTarget,
    (character, chat_tab): (
        Option<&super::character_select::Action>,
        Option<&super::chat::TabButton>,
    ),
) -> bool {
    use super::character_select::Action;
    match target {
        ClickTarget::ChatTab(tab) => chat_tab.is_some_and(|button| button.0 == tab),
        ClickTarget::CharacterList(click) => {
            character.is_some_and(|action| match (click, action) {
                (CharacterClick::Slot(slot), Action::Choose(chosen)) => slot == *chosen,
                (CharacterClick::Enter, Action::Enter) | (CharacterClick::Quit, Action::Quit) => {
                    true
                }
                _ => false,
            })
        }
        _ => false,
    }
}

/// The Raid window's action a scripted click names.
const fn raid(click: RaidClick) -> super::raid::RaidAction {
    use super::raid::RaidAction;
    match click {
        RaidClick::Disband => RaidAction::Disband,
        RaidClick::Lock(locked) => RaidAction::Lock(locked),
        RaidClick::Move(group) => RaidAction::Move(group),
        RaidClick::MakeLeader => RaidAction::MakeLeader,
    }
}

/// Whether a control of the Training window, the confirmation dialog, a
/// skinned window's title bar or its Done button is the one a click names.
fn dialog_control(
    target: ClickTarget,
    (skill_row, train, closes, answer, title_box): (
        Option<&super::training::SkillRow>,
        bool,
        Option<&super::skinned::Closes>,
        Option<&super::confirm::AnswerButton>,
        Option<&super::skinned::TitleBox>,
    ),
    trade: Option<&super::trade::Action>,
) -> bool {
    match target {
        ClickTarget::Done(key) => done_button(key, closes, trade, title_box),
        ClickTarget::TitleBox(key, close) => title_box
            .is_some_and(|title_box| title_box.window.key() == key && title_box.close == close),
        ClickTarget::Answer(yes) => answer.is_some_and(|answer| answer.0 == yes),
        ClickTarget::Training(TrainingClick::Row(index)) => {
            skill_row.is_some_and(|row| row.index == index)
        }
        ClickTarget::Training(TrainingClick::Train) => train,
        ClickTarget::Training(TrainingClick::Done) => {
            closes.is_some_and(|closes| closes.0 == super::windows::WindowId::Training)
        }
        _ => false,
    }
}

/// The loot or merchant window's action a scripted click names.
const fn trading(click: TradeClick) -> super::trade::Action {
    use super::trade::Action;
    match click {
        TradeClick::Take(place) => Action::Take(place),
        TradeClick::TakeAll => Action::TakeAll,
        TradeClick::EndLoot => Action::EndLoot,
        TradeClick::Buy(slot) => Action::Buy(slot),
        TradeClick::Sell(slot) => Action::Sell(slot),
        TradeClick::EndShop => Action::EndShop,
        TradeClick::Choose(slot) => Action::Choose(slot),
        TradeClick::BuyChosen => Action::BuyChosen,
        TradeClick::SellChosen => Action::SellChosen,
    }
}

/// Whether a control is the button in a window's own body that closes it:
/// one that only closes the window, or the loot or merchant window's Done,
/// which ends the loot or the shopping as it closes. The title bar's close
/// box closes it too, but is `close_box`'s.
fn done_button(
    key: &str,
    closes: Option<&super::skinned::Closes>,
    trade: Option<&super::trade::Action>,
    title_box: Option<&super::skinned::TitleBox>,
) -> bool {
    use super::trade::Action;
    use super::windows::WindowId;
    let window = match (closes, trade) {
        (Some(closes), _) => closes.0,
        (None, Some(Action::EndLoot)) => WindowId::Loot,
        (None, Some(Action::EndShop)) => WindowId::Merchant,
        _ => return false,
    };
    title_box.is_none() && window.key() == key
}

/// The control a click or a hold presses, or why the script stops: none is
/// visible, or a skin's button named by its `ScreenID` is greyed out, as it
/// is for a player, with the reason its hover gives.
fn reach(
    target: ClickTarget,
    buttons: &Buttons,
    layout: &Layout,
    needs: &Query<&crate::outbox::Needs>,
    world: &eq_client_core::world::ClientWorld,
) -> Result<Entity, String> {
    let entity = find(target, buttons, layout).ok_or("click target is not visible")?;
    match (target, needs.get(entity)) {
        (ClickTarget::Button(..), Ok(needs)) if !needs.offered(world) => Err(format!(
            "that button is greyed out: {}",
            needs.reason(world)
        )),
        _ => Ok(entity),
    }
}

/// The first visible matching control in an open window, as a click or a
/// hover reaches it.
fn find(target: ClickTarget, buttons: &Buttons, layout: &Layout) -> Option<Entity> {
    for (
        entity,
        _,
        visibility,
        slot,
        scribe,
        store,
        (row, place, turns, spell_gem, hot_button),
        gem,
        trade,
        tint,
        (give, character, chat_tab),
        (coins, pick, arrow, amount),
        (selector, tab, ability, attack, slash, checkbox),
        (
            slider,
            drop_down,
            choice,
            (dialog, page, combine, map, (raid_row, raid_action, shows, skin_id)),
        ),
    ) in buttons
    {
        let matches = match target {
            ClickTarget::ActionsWindow => selector
                .is_some_and(|selector| selector.0 == super::windows::WindowId::ActionsWindow),
            ClickTarget::Toggle(key) => selector.is_some_and(|selector| selector.0.key() == key),
            // A tab of the window's own tab box, not one on its pages.
            ClickTarget::Tab(index) => tab.is_some_and(|tab| tab.depth == 0 && tab.index == index),
            ClickTarget::Ability(page, index) => ability.is_some_and(|button| {
                use super::abilities::Page;
                button.index == index
                    && button.page
                        == match page {
                            AbilityPage::Combat => Page::Combat,
                            AbilityPage::Abilities => Page::Abilities,
                        }
            }),
            ClickTarget::Attack => attack,
            ClickTarget::Slash(command) => slash.is_some_and(|button| button.0 == command),
            ClickTarget::Option(toggle) => checkbox.is_some_and(|checkbox| checkbox.0 == toggle),
            ClickTarget::Slider(name, _) => slider.is_some_and(|slider| slider.sets == sets(name)),
            ClickTarget::KeyFilter => drop_down.is_some_and(|drop_down| {
                drop_down.choosing() == super::skinned::Choosing::KeyFilter
            }),
            ClickTarget::Choice(index) => choice.is_some_and(|choice| choice.index == index),
            ClickTarget::Answer(_)
            | ClickTarget::Training(_)
            | ClickTarget::TitleBox(..)
            | ClickTarget::Done(_) => dialog_control(target, dialog, trade),
            ClickTarget::Page(forward) => page.is_some_and(|page| page.0 == forward),
            ClickTarget::Combine(slot) => combine.is_some_and(|combine| combine.0.0 == slot),
            ClickTarget::Map(action) => map.is_some_and(|button| *button == action),
            ClickTarget::Raid(click) => raid_action.is_some_and(|action| *action == raid(click)),
            ClickTarget::RaidRow(grouped, index) => raid_row.is_some_and(|row| {
                row.index == index && (row.list == super::raid::RaidList::Grouped) == grouped
            }),
            ClickTarget::Buff(long, index) => effects_button(shows, long, index),
            ClickTarget::Button(key, id) => {
                skin_id.is_some_and(|skin| skin.id == id && skin.window.key() == key)
            }
            ClickTarget::Scroll(key, up) => arrow.is_some_and(|arrow| {
                arrow.window.is_some_and(|window| window.key() == key) && arrow.up == up
            }),
            ClickTarget::Slot(number) => slot.is_some_and(|slot| slot.0.0 == number),
            ClickTarget::Scribe => scribe,
            ClickTarget::Store => store,
            ClickTarget::BookRow(number) => row.is_some_and(|row| row.0 == number),
            ClickTarget::BookPlace(number) => place.is_some_and(|place| place.0 == number),
            ClickTarget::BookPage(forward) => turns.is_some_and(|turns| turns.0 == forward),
            ClickTarget::SpellGem(number) => spell_gem.is_some_and(|gem| gem.0 == number),
            ClickTarget::HotButton(number) => hot_button.is_some_and(|slot| slot.0 == number),
            ClickTarget::MemorizeGem(number) => gem.is_some_and(|gem| gem.0 == number),
            ClickTarget::Trade(click) => trade == Some(&trading(click)),
            ClickTarget::Tint(number, color) => tint.is_some_and(|action| {
                use super::inventory::colors::Action;
                match (action, color) {
                    (Action::Toggle(slot), None) => slot.0 == number,
                    (Action::Set(slot, index), Some(color)) => slot.0 == number && *index == color,
                    _ => false,
                }
            }),
            ClickTarget::Give => give,
            ClickTarget::ChatTab(_) | ClickTarget::CharacterList(_) => {
                listed_control(target, (character, chat_tab))
            }
            ClickTarget::Coins(place, coin) => {
                coins.is_some_and(|coins| coins.place == place && coins.coin == coin)
            }
            ClickTarget::Pick(PickButton::Amount) => amount,
            ClickTarget::Pick(button) => pick.is_some_and(|action| Some(*action) == picks(button)),
        };
        if matches && visibility.get() && displayed(entity, layout) {
            return Some(entity);
        }
    }
    None
}

/// Whether a control is this button of the skin's effects windows: the
/// lasting one, or the short one.
fn effects_button(shows: Option<&super::skinned::Shows>, long: bool, index: u32) -> bool {
    use eq_client_core::buffs::EffectWindow;
    let window = if long {
        EffectWindow::Long
    } else {
        EffectWindow::Short
    };
    shows == Some(&super::skinned::Shows::Buff(window, index))
}

/// What a slider a script names sets.
const fn sets(name: SliderClick) -> super::skinned::Sets {
    use super::skinned::Sets;
    match name {
        SliderClick::Level(level) => Sets::Level(level),
        SliderClick::Quantity => Sets::Quantity,
    }
}

/// What a quantity picker's button a script names does to the amount; the
/// number box does nothing to it.
const fn picks(button: PickButton) -> Option<super::inventory::SplitAction> {
    use super::inventory::SplitAction;
    Some(match button {
        PickButton::Less => SplitAction::Less,
        PickButton::More => SplitAction::More,
        PickButton::Min => SplitAction::Minimum,
        PickButton::Max => SplitAction::Maximum,
        PickButton::Confirm => SplitAction::Confirm,
        PickButton::Cancel => SplitAction::Cancel,
        PickButton::Amount => return None,
    })
}

/// Marks a found control pressed, a slider where the setting is to go; the
/// focus system clears it next frame. No pointer rests on it, so it shows no
/// tooltip.
fn press(
    commands: &mut Commands,
    entity: Entity,
    target: ClickTarget,
    buttons: &mut Buttons,
    pointers: &mut Pointers,
) {
    commands
        .entity(entity)
        .insert(super::tooltip::ScriptPressed);
    let Ok((_, mut interaction, .., (slider, ..))) = buttons.get_mut(entity) else {
        return;
    };
    if let (ClickTarget::Slider(_, percent), Some(slider), Ok(mut pointer)) =
        (target, slider, pointers.get_mut(entity))
    {
        pointer.normalized = Some(Vec2::new(slider.across(f32::from(percent) / 100.0), 0.0));
    }
    *interaction = Interaction::Pressed;
}

/// Whether a node and every node above it are laid out, as a pointer needs
/// to reach it.
fn displayed(entity: Entity, layout: &Layout) -> bool {
    let mut next = Some(entity);
    while let Some((node, parent)) = next.and_then(|entity| layout.get(entity).ok()) {
        if node.display == Display::None {
            return false;
        }
        next = parent.map(ChildOf::parent);
    }
    true
}

/// Logs the doors nearest where a partial route ends, the likeliest reason it
/// ends short.
fn log_partial_route(online: &super::online::OnlineState, end: Vec3) {
    // Renderer (EQ y, EQ z, -EQ x) back to EQ x, y, z.
    let end = Vec3::new(-end.z, end.x, end.y);
    let mut doors: Vec<_> = online
        .world()
        .doors()
        .entries()
        .values()
        .map(|door| {
            let at = Vec3::new(door.position.x, door.position.y, door.position.z);
            (at.distance(end), door)
        })
        .collect();
    doors.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (distance, door) in doors.into_iter().take(5) {
        info!(
            id = door.id,
            model = %door.model,
            open_type = door.open_type,
            incline = door.incline,
            action = ?door.action,
            x = door.position.x,
            y = door.position.y,
            z = door.position.z,
            distance,
            "Script route doors"
        );
    }
}

/// How long a clicked door may take to start opening before the walk treats
/// it as locked: longer than any server's answer to a click.
const DOOR_WAIT: Duration = Duration::from_secs(3);
/// How long an opened door may take to swing open as drawn, and may still
/// block the way after the walk has searched around it once: the swing, a
/// quarter turn at half a turn a second, and a second to spare.
const DOOR_SWING: Duration = Duration::from_millis(1500);
/// Clicks one door may take in one walk: the first, and one more if it shut
/// again before the walker got through.
const DOOR_CLICKS: u8 = 2;

/// How often a walk's follower trace logs where it heads, at debug level.
const TRACE_EVERY: Duration = Duration::from_secs(1);

/// Where a walk last headed: the waypoint it steered toward and that
/// waypoint's index in the last search's waypoints, how many were left, and
/// the route distance still to cover.
#[derive(Clone, Copy, Debug)]
struct Following {
    toward: Vec3,
    index: usize,
    ahead: usize,
    left: f32,
    /// When the follower trace last logged.
    traced: Option<Duration>,
}

/// A renderer point as EQ x, y, z, as the server and the client's own
/// reports give positions.
fn eq_point(point: Vec3) -> [f32; 3] {
    let at = eq_client_core::world_position(point.to_array(), 0.0);
    [at.x, at.y, at.z]
}

/// The line a walk logs when following stops shortening its route: where it
/// stands, where it last headed, and what the movement samples of the last
/// second asked and moved, so one line tells a blocked walker (asked, not
/// moved) from one going round in place (moved, not net) or one not asked to
/// move at all.
fn log_stall(
    again: bool,
    (feet, height): (Vec3, f32),
    following: Option<Following>,
    controls: &super::motion::Controls,
    world: &eq_client_core::movement::CollisionWorld,
) {
    let refused = controls.refused.as_deref();
    let now = std::time::Instant::now();
    let motion = controls.recent.last_second(now);
    let stance = Stance::at(world, controls, (feet, height), now);
    let feet = eq_point(feet);
    let toward = following.map(|following| eq_point(following.toward));
    let index = following.map(|following| following.index);
    let ahead = following.map(|following| following.ahead);
    let left = following.map(|following| following.left);
    let (samples, asked, moved, net) = (motion.samples, motion.asked, motion.moved, motion.net);
    if again {
        info!(
            ?refused,
            ?feet,
            ?toward,
            ?index,
            ?ahead,
            ?left,
            samples,
            asked,
            moved,
            net,
            ?stance,
            "Script walk stalled; searching again from here"
        );
    } else {
        info!(
            ?refused,
            ?feet,
            ?toward,
            ?index,
            ?ahead,
            ?left,
            samples,
            asked,
            moved,
            net,
            ?stance,
            "Script walk stalled after searching again"
        );
    }
}

/// How the player stands as the movement controller sees it, for the stall
/// lines: whether it falls and how far it has, the floor under it, the
/// zone's liquids there, and how long since a sample went out and since the
/// session answered one.
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "the fields are read through Debug, in the stall lines"
)]
struct Stance {
    /// Upward velocity while the session simulates falls; None when it
    /// doesn't.
    velocity: Option<f32>,
    /// How far the feet have come down from the highest they were since
    /// they left the ground; None while they stand on it.
    fallen: Option<f32>,
    /// The floor the next step stands the feet on, in EQ z; None means it
    /// falls.
    support: Option<f32>,
    /// Where the feet come to rest below them, in EQ x, y, z.
    rest: Option<[f32; 3]>,
    /// Whether they are in deep water or lava, and whether coming to rest
    /// stays clear of both.
    submerged: bool,
    dry: bool,
    /// Whether a sample is out awaiting its answer, and why the latest
    /// frame sent none.
    waiting: bool,
    held: Option<super::motion::Held>,
    /// Seconds since the latest sample went out, since the session last
    /// answered one, and since the last sample the controls count as
    /// accepted (an answer, or a prediction paused).
    since_sent: Option<f32>,
    since_answer: Option<f32>,
    since_accepted: f32,
}

impl Stance {
    fn at(
        world: &eq_client_core::movement::CollisionWorld,
        controls: &super::motion::Controls,
        (feet, height): (Vec3, f32),
        now: std::time::Instant,
    ) -> Self {
        let since = |at: std::time::Instant| now.saturating_duration_since(at).as_secs_f32();
        let view = world.guard_view(feet, height);
        let airborne = controls.airborne.as_ref();
        Self {
            velocity: airborne.map(eq_client_core::movement::AirborneController::velocity),
            fallen: airborne
                .and_then(eq_client_core::movement::AirborneController::peak)
                .map(|peak| peak - feet.y),
            support: world
                .support(feet)
                .map(|floor| eq_point(Vec3::new(feet.x, floor, feet.z))[2]),
            rest: view.rest.map(eq_point),
            submerged: view.submerged,
            dry: view.dry,
            waiting: controls.waiting(),
            held: controls.held,
            since_sent: controls.last_sent.map(since),
            since_answer: controls.last_answer.map(since),
            since_accepted: since(controls.last_accepted),
        }
    }
}

/// How long a following walk may see no movement sample sent before it
/// says why.
const QUIET: std::time::Duration = std::time::Duration::from_secs(1);

/// Since when a following walk has seen no sample sent, and whether it has
/// said so for this pause.
#[derive(Clone, Copy)]
struct Quiet {
    since: std::time::Instant,
    said: bool,
}

/// Says once per pause, when a walk following its route has had no movement
/// sample sent for over [`QUIET`], why the latest frame held one back.
fn log_quiet(
    quiet: &mut Option<Quiet>,
    controls: &super::motion::Controls,
    now: std::time::Instant,
) {
    let mark = quiet.get_or_insert(Quiet {
        since: now,
        said: false,
    });
    if let Some(sent) = controls.last_sent.filter(|sent| *sent > mark.since) {
        *mark = Quiet {
            since: sent,
            said: false,
        };
    }
    let quiet_for = now.saturating_duration_since(mark.since);
    if !mark.said && quiet_for > QUIET {
        mark.said = true;
        info!(
            reason = ?controls.held,
            seconds = quiet_for.as_secs_f32(),
            waiting = controls.waiting(),
            speed = ?controls.speed,
            "Script walk samples paused"
        );
    }
}

/// A walk waiting on a closed door across its next leg.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DoorWait {
    id: u8,
    /// When the walk first clicked it.
    asked: Option<Duration>,
    /// When the walk last clicked it, and when it was then seen opening.
    clicked: Option<Duration>,
    opened: Option<Duration>,
    clicks: u8,
    /// Whether the walk has searched again around the door as it stands open.
    rerouted: bool,
}

/// What the walk does at a closed door this frame.
#[derive(Debug, PartialEq)]
enum DoorAction {
    Click,
    Wait,
    /// The door stands open but its leaf is in the way: search again from
    /// here, around it, as a player steps round a door that swung toward them.
    Reroute,
    /// The door is a wall for this route, for this reason.
    Refuse(&'static str),
}

impl DoorWait {
    const fn new(id: u8) -> Self {
        Self {
            id,
            asked: None,
            clicked: None,
            opened: None,
            clicks: 0,
            rerouted: false,
        }
    }

    /// `open` is the door's state as drawn (None when unknown); a click is sent
    /// only to a door shown shut, since a click toggles it. `hinge` is how far
    /// its leaf is drawn swung open, 0 to 1 (None when it is not drawn): the
    /// walk searches round an open door only once the leaf it walks against
    /// stands fully open, however slowly the frames swing it.
    fn act(
        &mut self,
        open: Option<bool>,
        hinge: Option<f32>,
        within_use: bool,
        now: Duration,
    ) -> DoorAction {
        match open {
            None => DoorAction::Refuse("its state is unknown"),
            Some(true) => {
                let since = now.saturating_sub(*self.opened.get_or_insert(now));
                let swung = hinge.is_none_or(|hinge| hinge >= 1.0);
                if swung && !self.rerouted {
                    self.rerouted = true;
                    DoorAction::Reroute
                } else if since < DOOR_SWING {
                    DoorAction::Wait
                } else if self.rerouted {
                    DoorAction::Refuse("it opened but still blocks the way")
                } else {
                    DoorAction::Refuse("it never finished swinging")
                }
            }
            Some(false) => match (self.clicked, self.opened) {
                (Some(clicked), None) if now.saturating_sub(clicked) >= DOOR_WAIT => {
                    DoorAction::Refuse("it did not open")
                }
                (Some(_), None) => DoorAction::Wait,
                _ if self.clicks >= DOOR_CLICKS => DoorAction::Refuse("it shut again"),
                _ if !within_use => DoorAction::Refuse("it is out of reach"),
                _ => {
                    self.clicks += 1;
                    self.asked.get_or_insert(now);
                    self.clicked = Some(now);
                    self.opened = None;
                    // Each opening gets its own step round the leaf.
                    self.rerouted = false;
                    DoorAction::Click
                }
            },
        }
    }
}

/// The doors a walk may open on its way, open or shut now: a route consults
/// them only where one blocks a step, and the walk clicks only a shut one.
fn openable_doors(online: &super::online::OnlineState) -> Vec<u8> {
    online
        .world()
        .doors()
        .entries()
        .values()
        .filter(|door| super::doors::swings(door))
        .map(|door| door.id)
        .collect()
}

/// The player's feet at its accepted position and the goal's position, in
/// render coordinates: the world object `object` names, or else the target.
fn walk_ends(
    online: &super::online::OnlineState,
    body: super::PlayerBody,
    object: Option<u32>,
) -> Option<(Vec3, Vec3)> {
    let world = online.world();
    let player = world.player()?;
    let goal = match object {
        Some(drop_id) => world.objects().entries().get(&drop_id)?.position,
        None => {
            world
                .target()
                .selected
                .and_then(|id| world.spawn(id))?
                .state
                .position
        }
    };
    let origin = Vec3::from_array(eq_client_core::render_position(player.position));
    let goal = Vec3::from_array(eq_client_core::render_position(goal));
    Some((origin - Vec3::Y * body.feet_offset, goal))
}

/// The zone's nearest world container of a tradeskill kind to the player.
fn nearest_station(online: &super::online::OnlineState, kind: u8) -> Option<u32> {
    let world = online.world();
    let player = world.player()?;
    eq_client_core::ground::nearest_container(world.objects(), kind, player.position)
        .map(|object| object.drop_id)
}

/// Whether a `walk object` step is done at a flat `distance` from its object:
/// within `range` it is; short of it, the end of a partial route or running
/// out of time stops the script, since a later step such as `press f` would
/// act on nothing.
fn object_walk_end(
    distance: f32,
    range: f32,
    arrived: bool,
    out_of_time: bool,
) -> Result<bool, String> {
    if distance <= range {
        Ok(true)
    } else if arrived {
        Err(format!(
            "walk object ended {distance:.1} from its object; no walkable path reaches it"
        ))
    } else if out_of_time {
        Err("walk object ran out of time".to_owned())
    } else {
        Ok(false)
    }
}

/// Points the camera from the player toward the target and returns the flat
/// distance between them, or None without a player and a known target.
fn face(
    online: &super::online::OnlineState,
    players: &Query<&Transform, With<super::Player>>,
    cameras: &mut Query<&mut super::OrbitCamera>,
) -> Option<f32> {
    let spawn = online
        .world()
        .target()
        .selected
        .and_then(|id| online.world().spawn(id).map(|spawn| &spawn.state))?;
    face_toward(spawn.position, players, cameras)
}

/// [`face`] toward a world object instead of the target, or None without a
/// player or once the object has left the zone.
fn face_object(
    drop_id: Option<u32>,
    online: &super::online::OnlineState,
    players: &Query<&Transform, With<super::Player>>,
    cameras: &mut Query<&mut super::OrbitCamera>,
) -> Option<f32> {
    let object = online.world().objects().entries().get(&drop_id?)?;
    face_toward(object.position, players, cameras)
}

/// Points the camera from the player toward a position and returns the flat
/// distance between them, or None without a player.
fn face_toward(
    position: eq_client_core::WorldPosition,
    players: &Query<&Transform, With<super::Player>>,
    cameras: &mut Query<&mut super::OrbitCamera>,
) -> Option<f32> {
    let transform = players.single().ok()?;
    let to = Vec3::from_array(eq_client_core::render_position(position)) - transform.translation;
    let flat = Vec2::new(to.x, to.z);
    if flat.length() > f32::EPSILON {
        for mut camera in cameras.iter_mut() {
            camera.yaw = super::behind(flat.x.atan2(flat.y));
        }
    }
    Some(flat.length())
}

/// Whether what a wait step waits for has arrived: the character list, or
/// the player in a zone, the one named if any, with the loading screen
/// lifted so the next step acts on the zone and not on the cover.
fn arrived(step: &Step, online: &super::online::OnlineState, covered: bool) -> bool {
    let world = online.world();
    let in_zone = world.connected() && world.player().is_some() && !covered;
    match step {
        Step::WaitSelect => online.selection.is_some(),
        Step::WaitZone(zone) => in_zone && world.zone() == *zone,
        _ => in_zone,
    }
}

/// Whether the spawn a `wait_dead` step waits on has fallen: an NPC's
/// corpse now, which the servers leave under the dying NPC's own spawn ID,
/// or gone from the zone. With nothing targeted as the step started, or
/// once its time is up, the script stops for the reason given.
fn fallen(
    world: &eq_client_core::world::ClientWorld,
    awaited: Option<u16>,
    elapsed: Duration,
    limit: Duration,
) -> Result<bool, &'static str> {
    let Some(awaited) = awaited else {
        return Err("wait_dead needs a target");
    };
    if world
        .spawn(awaited)
        .is_none_or(|spawn| spawn.state.kind == eq_client_core::SpawnKind::NpcCorpse)
    {
        Ok(true)
    } else if elapsed > limit {
        Err("the target did not die in time")
    } else {
        Ok(false)
    }
}

/// What a `chase` step does on one frame.
#[derive(Debug, PartialEq)]
enum Chase {
    /// The target died or left the zone: the chase is over.
    Caught,
    /// The time ran out with the target still alive: the chase is over.
    OutOfTime,
    /// Keep after the target, facing where it is drawn now.
    Aim(eq_client_core::WorldPosition),
}

/// One frame of a `chase` step. A chase ends only on the target's death,
/// its leaving the zone, or the time limit; otherwise it aims where the
/// target is drawn: its last report carried forward at the reported
/// velocity from when that report was first seen.
fn chase(
    world: &eq_client_core::world::ClientWorld,
    awaited: Option<u16>,
    seen: &mut Option<(eq_client_core::WorldPosition, Duration)>,
    elapsed: Duration,
    limit: Duration,
) -> Result<Chase, &'static str> {
    let Some(id) = awaited else {
        return Err("chase needs a target");
    };
    let spawn = match world.spawn(id) {
        Some(spawn) if spawn.state.kind != eq_client_core::SpawnKind::NpcCorpse => &spawn.state,
        _ => return Ok(Chase::Caught),
    };
    if elapsed >= limit {
        return Ok(Chase::OutOfTime);
    }
    let since = match *seen {
        Some((position, at)) if position == spawn.position => at,
        _ => {
            *seen = Some((spawn.position, elapsed));
            elapsed
        }
    };
    Ok(Chase::Aim(eq_client_core::entities::extrapolate(
        spawn,
        elapsed.saturating_sub(since),
    )))
}

/// Whether a chase holds W: only while its target is beyond the distance.
/// Within it the player stands, auto-attack doing the rest.
fn runs(distance: Option<f32>, range: f32) -> bool {
    distance.is_some_and(|distance| distance > range)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_create_step_waits_for_the_worlds_answer_and_fails_on_a_refusal() {
        use eq_client_core::{
            WorldEvent, WorldUpdate,
            creation::CreationOutcome,
            world::{ClientWorld, NoSpells},
        };
        let mut world = ClientWorld::default();
        let answer = |world: &mut ClientWorld, name: &str, outcome| {
            world.apply(
                &WorldUpdate::Game(WorldEvent::CharacterCreation {
                    name: name.into(),
                    outcome,
                }),
                std::time::Instant::now(),
                &NoSpells,
            );
        };
        // No answer yet, then a refusal: the step fails, naming it.
        assert_eq!(answered(&world, None, "Testcleric"), Ok(false));
        answer(&mut world, "Testcleric", CreationOutcome::CreationRefused);
        assert_eq!(
            answered(&world, None, "Testcleric"),
            Err("character creation refused: CreationRefused".into())
        );
        // A later step waits past the answer it saw sent, and past another
        // name's, then finishes on its own character's creation.
        let before = world.creation().map(|last| last.count);
        assert_eq!(answered(&world, before, "Testcleric"), Ok(false));
        answer(&mut world, "Othername", CreationOutcome::Created);
        assert_eq!(answered(&world, before, "Testcleric"), Ok(false));
        answer(&mut world, "Testcleric", CreationOutcome::Created);
        assert_eq!(answered(&world, before, "Testcleric"), Ok(true));
    }

    #[test]
    fn a_chase_aims_where_its_target_is_drawn_and_ends_only_on_death_leaving_or_time() {
        use eq_client_core::{SpawnKind, WorldEvent, WorldPosition};
        let at = Duration::from_millis;
        let mut online = super::super::online::OnlineState::new(true);
        super::super::online::testing::admit(
            &mut online,
            1,
            super::super::online::testing::player(1),
        );
        let mut gnoll = super::super::online::testing::pet(9, 0);
        gnoll.pet_owner = None;
        gnoll.kind = SpawnKind::Npc;
        gnoll.position = WorldPosition {
            x: 10.0,
            y: 0.0,
            z: 0.0,
            heading: 0.0,
        };
        gnoll.velocity = [4.0, 0.0, 0.0];
        super::super::online::testing::news(&mut online, [WorldEvent::Spawns(vec![gnoll.clone()])]);
        let limit = at(90_000);
        let mut seen = None;
        let aim = |chase: Result<Chase, &'static str>| match chase {
            Ok(Chase::Aim(aim)) => aim.x,
            other => panic!("expected an aim, got {other:?}"),
        };
        // The aim carries the last report forward at its velocity.
        let world = online.world();
        assert!((aim(chase(world, Some(9), &mut seen, at(1_000), limit)) - 10.0).abs() < 1e-3);
        assert!((aim(chase(world, Some(9), &mut seen, at(1_500), limit)) - 12.0).abs() < 1e-3);
        // Beyond the distance the player runs; within it, it stands and the
        // chase goes on, never ending merely on reaching the target.
        assert!(runs(Some(12.0), 10.0));
        assert!(!runs(Some(8.0), 10.0));
        assert!(!runs(None, 10.0));
        assert!(matches!(
            chase(world, Some(9), &mut seen, at(2_000), limit),
            Ok(Chase::Aim(_))
        ));
        // Out of time with the target alive ends the chase.
        assert_eq!(
            chase(world, Some(9), &mut seen, limit, limit),
            Ok(Chase::OutOfTime)
        );
        // Its corpse, or its leaving the zone, ends it before the limit.
        let mut corpse = gnoll.clone();
        corpse.kind = SpawnKind::NpcCorpse;
        super::super::online::testing::news(&mut online, [WorldEvent::Spawns(vec![corpse])]);
        assert_eq!(
            chase(online.world(), Some(9), &mut seen, at(3_000), limit),
            Ok(Chase::Caught)
        );
        super::super::online::testing::news(&mut online, [WorldEvent::Despawn(9)]);
        assert_eq!(
            chase(online.world(), Some(9), &mut seen, at(3_000), limit),
            Ok(Chase::Caught)
        );
        assert_eq!(
            chase(online.world(), None, &mut seen, at(3_000), limit),
            Err("chase needs a target")
        );
    }

    #[test]
    fn a_walk_clicks_a_shut_door_once_then_waits_for_it_to_swing_open() {
        let at = Duration::from_secs;
        let mut wait = DoorWait::new(4);
        assert_eq!(
            wait.act(Some(false), Some(0.0), true, at(10)),
            DoorAction::Click
        );
        // No second click while the server's answer is on its way: a click toggles.
        assert_eq!(
            wait.act(Some(false), Some(0.0), true, at(11)),
            DoorAction::Wait
        );
        // Opening: never clicked, only waited on until the swing ends.
        assert_eq!(
            wait.act(Some(true), Some(0.4), true, at(12)),
            DoorAction::Wait
        );
        assert_eq!(wait.asked, Some(at(10)));
    }

    #[test]
    fn a_door_that_does_not_open_or_still_blocks_is_given_up() {
        let at = Duration::from_millis;
        let mut locked = DoorWait::new(4);
        assert_eq!(
            locked.act(Some(false), Some(0.0), true, at(0)),
            DoorAction::Click
        );
        assert_eq!(
            locked.act(Some(false), Some(0.0), true, at(2_999)),
            DoorAction::Wait
        );
        assert_eq!(
            locked.act(Some(false), Some(0.0), true, at(3_000)),
            DoorAction::Refuse("it did not open")
        );
        let mut stuck = DoorWait::new(4);
        stuck.act(Some(false), Some(0.0), true, at(0));
        assert_eq!(
            stuck.act(Some(true), Some(0.2), true, at(100)),
            DoorAction::Wait
        );
        // Swung open but in the way: one search around it first.
        assert_eq!(
            stuck.act(Some(true), Some(1.0), true, at(600)),
            DoorAction::Reroute
        );
        assert_eq!(
            stuck.act(Some(true), Some(1.0), true, at(900)),
            DoorAction::Wait
        );
        // Still across the way after that: the door is a wall.
        assert_eq!(
            stuck.act(Some(true), Some(1.0), true, at(1_600)),
            DoorAction::Refuse("it opened but still blocks the way")
        );
        let mut slow = DoorWait::new(4);
        slow.act(Some(false), Some(0.0), true, at(0));
        slow.act(Some(true), Some(0.0), true, at(100));
        assert_eq!(
            slow.act(Some(true), Some(0.9), true, at(1_599)),
            DoorAction::Wait
        );
        assert_eq!(
            slow.act(Some(true), Some(0.9), true, at(1_600)),
            DoorAction::Refuse("it never finished swinging")
        );
        assert_eq!(
            DoorWait::new(4).act(Some(false), Some(0.0), false, at(0)),
            DoorAction::Refuse("it is out of reach")
        );
        assert_eq!(
            DoorWait::new(4).act(None, Some(0.0), true, at(0)),
            DoorAction::Refuse("its state is unknown")
        );
    }

    #[test]
    fn a_door_that_shuts_again_is_clicked_once_more_then_given_up() {
        let at = Duration::from_millis;
        let mut wait = DoorWait::new(4);
        assert_eq!(
            wait.act(Some(false), Some(0.0), true, at(0)),
            DoorAction::Click
        );
        assert_eq!(
            wait.act(Some(true), Some(0.8), true, at(500)),
            DoorAction::Wait
        );
        // Shut again before the walker got through.
        assert_eq!(
            wait.act(Some(false), Some(0.6), true, at(900)),
            DoorAction::Click
        );
        assert_eq!(
            wait.act(Some(true), Some(0.8), true, at(1_400)),
            DoorAction::Wait
        );
        assert_eq!(
            wait.act(Some(false), Some(0.6), true, at(1_800)),
            DoorAction::Refuse("it shut again")
        );
    }

    #[test]
    fn a_door_that_shuts_again_gets_its_own_step_round_its_leaf_once_reopened() {
        let at = Duration::from_millis;
        let mut wait = DoorWait::new(4);
        assert_eq!(
            wait.act(Some(false), Some(0.0), true, at(0)),
            DoorAction::Click
        );
        assert_eq!(
            wait.act(Some(true), Some(0.8), true, at(500)),
            DoorAction::Wait
        );
        assert_eq!(
            wait.act(Some(true), Some(1.0), true, at(1_100)),
            DoorAction::Reroute
        );
        assert_eq!(
            wait.act(Some(false), Some(1.0), true, at(1_300)),
            DoorAction::Click
        );
        assert_eq!(
            wait.act(Some(true), Some(0.8), true, at(1_500)),
            DoorAction::Wait
        );
        assert_eq!(
            wait.act(Some(true), Some(1.0), true, at(2_100)),
            DoorAction::Reroute
        );
    }

    #[test]
    fn a_door_drawn_open_slowly_is_searched_round_only_once_its_leaf_stands_open() {
        // Slow frames: each draws at most a tenth of a second of swing, so a
        // quarter turn at half a turn a second takes five of them, whatever
        // time passes between them.
        let frame = Duration::from_millis(250);
        let mut wait = DoorWait::new(4);
        assert_eq!(
            wait.act(Some(false), Some(0.0), true, Duration::ZERO),
            DoorAction::Click
        );
        let mut swing = 0.0;
        let mut now = Duration::ZERO;
        let mut rerouted_at = None;
        // Within the swing's bound: past it, a leaf still across the way
        // after the search round it makes the door a wall.
        for _ in 0..6 {
            now += frame;
            swing = super::super::doors::advance_hinge(
                swing,
                super::super::doors::QUARTER_TURN,
                frame.as_secs_f32(),
            );
            let hinge = swing / super::super::doors::QUARTER_TURN;
            match wait.act(Some(true), Some(hinge), true, now) {
                DoorAction::Reroute => {
                    assert!(rerouted_at.is_none(), "one search round the leaf");
                    assert_eq!(hinge, 1.0, "searched round a leaf still swinging");
                    rerouted_at = Some(now);
                }
                DoorAction::Wait => (),
                other => panic!("{other:?} at {now:?}"),
            }
        }
        assert_eq!(rerouted_at, Some(frame * 5));
    }

    #[test]
    fn an_object_walk_goes_on_only_within_its_range() {
        // Within range it is done, whatever the route says.
        assert_eq!(object_walk_end(12.0, 12.0, false, false), Ok(true));
        assert_eq!(object_walk_end(3.0, 12.0, true, true), Ok(true));
        // Short of it, it walks on until the route ends or time runs out,
        // and either stops the script.
        assert_eq!(object_walk_end(30.0, 12.0, false, false), Ok(false));
        assert_eq!(
            object_walk_end(18.34, 12.0, true, false),
            Err("walk object ended 18.3 from its object; no walkable path reaches it".to_owned())
        );
        assert_eq!(
            object_walk_end(30.0, 12.0, false, true),
            Err("walk object ran out of time".to_owned())
        );
    }

    #[test]
    fn a_zone_wait_ends_once_the_loading_screen_has_lifted() {
        let mut online = crate::online::OnlineState::new(true);
        let zone = Step::WaitZone("qeytoqrg".into());
        assert!(!arrived(&zone, &online, false));
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(7));
        // The zone is admitted, but the cover is still up.
        assert!(!arrived(&zone, &online, true));
        assert!(!arrived(&Step::WaitOnline, &online, true));
        assert!(arrived(&zone, &online, false));
        assert!(arrived(&Step::WaitOnline, &online, false));
        assert!(!arrived(&Step::WaitZone("qeynos2".into()), &online, false));
    }

    #[test]
    fn done_is_a_window_body_button_that_closes_it() {
        use super::super::skinned::{Closes, TitleBox};
        use super::super::trade::Action;
        use super::super::windows::WindowId;
        let bank = Closes(WindowId::Bank);
        let container = Closes(WindowId::WorldContainer);
        assert!(done_button("bank", Some(&bank), None, None));
        assert!(done_button("world-container", Some(&container), None, None));
        // Another window's Done, or a button that does something else.
        assert!(!done_button("bank", Some(&container), None, None));
        assert!(!done_button("bank", None, Some(&Action::BuyChosen), None));
        assert!(!done_button("bank", None, None, None));
        // The loot and merchant windows' Done end the loot or the shopping.
        assert!(done_button("loot", None, Some(&Action::EndLoot), None));
        assert!(done_button("merchant", None, Some(&Action::EndShop), None));
        assert!(!done_button("loot", None, Some(&Action::EndShop), None));
        // The title bar's close box is `close_box`'s.
        let close_box = TitleBox {
            window: WindowId::Bank,
            close: true,
        };
        assert!(!done_button("bank", Some(&bank), None, Some(&close_box)));
        let close_box = TitleBox {
            window: WindowId::Loot,
            close: true,
        };
        assert!(!done_button(
            "loot",
            None,
            Some(&Action::EndLoot),
            Some(&close_box)
        ));
    }

    /// Keys for [`scripted_keyboard`]'s system to press (true) or release.
    #[derive(Resource, Default)]
    struct Queued(Vec<(KeyCode, bool)>);

    /// A chat app whose line is open, with a system pressing and releasing
    /// the queued keys through the keyboard a script's steps press.
    fn scripted_keyboard() -> App {
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
        let mut app = App::new();
        crate::keys::testing::install(&mut app);
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Queued>()
            .init_resource::<crate::chat::ChatState>()
            .init_resource::<crate::windows::pointer::Wheel>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(None))
            .add_systems(
                Update,
                (
                    |mut queued: ResMut<Queued>, mut keyboard: Keyboard| {
                        for (key, down) in queued.0.drain(..) {
                            if down {
                                keyboard.press(key);
                            } else {
                                keyboard.release(key);
                            }
                        }
                    },
                    crate::chat::input,
                )
                    .chain(),
            );
        app.world_mut()
            .resource_mut::<crate::keys::Typing>()
            .composing = true;
        app
    }

    fn run_keys(app: &mut App, keys: &[(KeyCode, bool)]) {
        app.world_mut().resource_mut::<Queued>().0 = keys.to_vec();
        app.update();
    }

    #[test]
    fn a_scripted_escape_closes_the_chat_line_as_a_real_one_does() {
        let mut app = scripted_keyboard();
        run_keys(&mut app, &[(KeyCode::Escape, true)]);
        let typing = app.world().resource::<crate::keys::Typing>();
        assert!(!typing.composing && typing.escape_consumed);
    }

    #[test]
    fn scripted_keys_type_what_a_us_keyboard_types() {
        let mut app = scripted_keyboard();
        let draft = |app: &App| {
            app.world()
                .resource::<crate::chat::ChatState>()
                .draft()
                .to_owned()
        };
        // Shift makes the next letter upper case; a held key types once.
        run_keys(
            &mut app,
            &[
                (KeyCode::ShiftLeft, true),
                (KeyCode::KeyH, true),
                (KeyCode::KeyH, true),
                (KeyCode::ShiftLeft, false),
                (KeyCode::KeyH, false),
                (KeyCode::KeyI, true),
                (KeyCode::KeyI, false),
                (KeyCode::Space, true),
                (KeyCode::Digit2, true),
                (KeyCode::Slash, true),
            ],
        );
        assert_eq!(draft(&app), "Hi 2/");
        // A Ctrl chord types nothing.
        run_keys(
            &mut app,
            &[(KeyCode::ControlLeft, true), (KeyCode::KeyA, true)],
        );
        assert_eq!(draft(&app), "Hi 2/");
        run_keys(
            &mut app,
            &[(KeyCode::ControlLeft, false), (KeyCode::KeyA, false)],
        );
        run_keys(&mut app, &[(KeyCode::Backspace, true)]);
        assert_eq!(draft(&app), "Hi 2");
    }

    #[test]
    fn a_skin_button_is_pressed_by_its_name_unless_it_is_greyed() {
        use crate::outbox::Needs;
        use crate::skinned::SkinId;
        use crate::windows::WindowId;
        #[derive(Resource, Default)]
        struct Reached(Vec<Result<bool, String>>);
        let mut app = App::new();
        app.init_resource::<Reached>();
        for (id, needs) in [("FakeDone", Needs::Nothing), ("FakeGrey", Needs::Missing)] {
            app.world_mut().spawn((
                Node::default(),
                Button,
                InheritedVisibility::VISIBLE,
                SkinId {
                    window: WindowId::Bank,
                    id: id.into(),
                },
                needs,
            ));
        }
        app.add_systems(
            Update,
            |buttons: Buttons,
             layout: Layout,
             needs: Query<&Needs>,
             skins: Query<&SkinId>,
             mut reached: ResMut<Reached>| {
                let world = crate::online::OnlineState::new(true);
                for target in [
                    ClickTarget::Button("bank", "FakeDone"),
                    ClickTarget::Button("bank", "FakeGrey"),
                    ClickTarget::Button("bank", "FakeNone"),
                    ClickTarget::Button("merchant", "FakeDone"),
                ] {
                    let found = reach(target, &buttons, &layout, &needs, world.world());
                    reached.0.push(
                        found.map(|entity| {
                            skins.get(entity).is_ok_and(|skin| skin.id == "FakeDone")
                        }),
                    );
                }
            },
        );
        app.update();
        let reached = &app.world().resource::<Reached>().0;
        assert_eq!(reached[0], Ok(true));
        assert_eq!(
            reached[1],
            Err(format!(
                "that button is greyed out: {}",
                crate::outbox::MISSING
            ))
        );
        // No such button, or none in that window.
        assert_eq!(reached[2], Err("click target is not visible".into()));
        assert_eq!(reached[3], Err("click target is not visible".into()));
    }

    /// A zone with the player (7) and three gnolls (9 to 11) in it, and a
    /// gnoll's death.
    fn gnolls() -> (
        crate::online::OnlineState,
        impl Fn(u32) -> eq_client_core::WorldEvent,
    ) {
        use crate::online::testing;
        let mut online = crate::online::OnlineState::new(true);
        testing::admit(&mut online, 1, testing::player(7));
        for spawn_id in 9..=11 {
            let mut gnoll = testing::pet(spawn_id, 7);
            gnoll.pet_owner = None;
            testing::spawn_entry(&mut online, spawn_id, gnoll);
        }
        let death = |spawn_id| {
            eq_client_core::WorldEvent::Death(eq_client_core::Death {
                spawn_id,
                killer_id: 7,
                corpse_id: spawn_id,
                bind_zone_id: 0,
                corpse_name: None,
            })
        };
        (online, death)
    }

    #[test]
    fn a_death_wait_ends_once_its_target_is_a_corpse_or_gone() {
        use crate::online::testing;
        let (mut online, death) = gnolls();
        let (soon, limit) = (Duration::from_secs(1), Duration::from_secs(90));
        assert_eq!(fallen(online.world(), Some(9), soon, limit), Ok(false));
        // Another spawn's death leaves it waiting.
        testing::news(&mut online, [death(10)]);
        assert_eq!(fallen(online.world(), Some(9), soon, limit), Ok(false));
        // The target's own does not: its corpse keeps its spawn ID.
        testing::news(&mut online, [death(9)]);
        assert_eq!(fallen(online.world(), Some(9), soon, limit), Ok(true));
        // A target that leaves the zone ends it too.
        assert_eq!(fallen(online.world(), Some(11), soon, limit), Ok(false));
        testing::news(&mut online, [eq_client_core::WorldEvent::Despawn(11)]);
        assert_eq!(fallen(online.world(), Some(11), soon, limit), Ok(true));
    }

    #[test]
    fn a_death_wait_stops_without_a_target_or_once_its_time_is_up() {
        let (online, _) = gnolls();
        let limit = Duration::from_secs(90);
        // Nothing targeted as it started: it stops at once.
        assert_eq!(
            fallen(online.world(), None, Duration::ZERO, limit),
            Err("wait_dead needs a target")
        );
        // Still fighting at the limit, it waits; past it, it stops.
        assert_eq!(fallen(online.world(), Some(9), limit, limit), Ok(false));
        assert_eq!(
            fallen(
                online.world(),
                Some(9),
                limit + Duration::from_millis(1),
                limit
            ),
            Err("the target did not die in time")
        );
    }

    #[test]
    fn chat_steps_speak_only_on_a_local_server() {
        let line = eq_client_core::OutboundChat::Say("Hail".into());
        assert!(local_chat(&line, false).is_err());
        assert_eq!(local_chat(&line, true), Ok(line));
    }

    #[test]
    fn gm_commands_only_reach_a_local_eqemu_session() {
        assert!(gm_chat("summon", false).is_err());
        assert_eq!(
            gm_chat("givemoney 0 0 5 0", true).unwrap(),
            eq_client_core::OutboundChat::Say("#givemoney 0 0 5 0".into())
        );
        assert!(!Script::new(vec![Step::Gm("summon".into())]).local);
        assert!(Script::new(Vec::new()).local_session(true).local);
        // Offline, steps run unattended but a gm step is still refused.
        let offline = Script::new(Vec::new()).offline_preview(true);
        assert!(offline.offline && !offline.local);
    }

    #[test]
    fn scripted_clicks_reach_only_buttons_in_open_windows() {
        use bevy::ecs::system::SystemState;
        let mut world = World::new();
        let button = |world: &mut World, display| {
            let frame = world
                .spawn(Node {
                    display,
                    ..default()
                })
                .id();
            world
                .spawn((
                    Node::default(),
                    Interaction::None,
                    InheritedVisibility::VISIBLE,
                    super::super::give::GiveButton,
                    ChildOf(frame),
                ))
                .id()
        };
        let closed = button(&mut world, Display::None);
        let mut state = SystemState::<(Commands, Buttons, Layout, Pointers)>::new(&mut world);
        let (_, buttons, layout, _) = state.get_mut(&mut world).unwrap();
        assert_eq!(find(ClickTarget::Give, &buttons, &layout), None);
        let open = button(&mut world, Display::Flex);
        let (mut commands, mut buttons, layout, mut pointers) = state.get_mut(&mut world).unwrap();
        assert_eq!(find(ClickTarget::Give, &buttons, &layout), Some(open));
        press(
            &mut commands,
            open,
            ClickTarget::Give,
            &mut buttons,
            &mut pointers,
        );
        state.apply(&mut world);
        assert_eq!(world.get::<Interaction>(open), Some(&Interaction::Pressed));
        assert!(
            world
                .get::<super::super::tooltip::ScriptPressed>(open)
                .is_some()
        );
        assert_eq!(world.get::<Interaction>(closed), Some(&Interaction::None));
    }

    #[test]
    fn attended_scripts_wait_for_a_reported_focus() {
        assert!(!may_run(false, None, true));
        assert!(!may_run(false, Some(false), false));
        assert!(may_run(false, Some(true), true));
        assert!(may_run(true, None, true) && may_run(true, Some(false), false));
    }

    #[test]
    fn following_queues_only_complete_appended_lines() {
        let path =
            std::env::temp_dir().join(format!("eq-client-follow-{}.txt", std::process::id()));
        std::fs::write(&path, "wait 10\n").unwrap();
        let mut script = Script::following(Vec::new(), path.clone(), 8);
        script.poll(Duration::ZERO);
        assert!(script.steps.is_empty());
        std::fs::write(&path, "wait 10\nreport a\nwait 2").unwrap();
        script.poll(Duration::from_secs(1));
        assert_eq!(script.steps, [Step::Report("a".into())]);
        // An invalid batch is skipped whole; later batches still run.
        std::fs::write(&path, "wait 10\nreport a\nwait 20\nbogus\n").unwrap();
        script.poll(Duration::from_secs(2));
        assert_eq!(script.steps.len(), 1);
        std::fs::write(&path, "wait 10\nreport a\nwait 20\nbogus\nquit\n").unwrap();
        script.poll(Duration::from_secs(3));
        assert_eq!(script.steps.back(), Some(&Step::Quit));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn camera_heading_makes_w_walk_along_that_heading() {
        for heading in [0.0_f32, 100.0, 256.0, 400.0] {
            let yaw = super::super::behind(eq_client_core::render_heading(heading));
            let walk = super::super::camera_relative_direction(0.0, 1.0, yaw);
            let expected = eq_client_core::world_heading(walk.x.atan2(walk.z));
            let error = (expected - heading).rem_euclid(512.0);
            assert!(error.min(512.0 - error) < 0.01, "{heading} -> {expected}");
        }
    }
}
