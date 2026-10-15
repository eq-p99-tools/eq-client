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
    /// The control a `hover` step rests the pointer on, until the next click
    /// or hover.
    hovering: Option<Entity>,
    /// The spawn a `wait_dead` step waits on: the target as it started.
    awaited: Option<u16>,
    /// The world container an `approach object` or `walk object` step walks
    /// toward, chosen as it started.
    approached: Option<u32>,
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
            hovering: None,
            awaited: None,
            approached: None,
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

    fn release(&mut self, keys: &mut ButtonInput<KeyCode>, mouse: &mut ButtonInput<MouseButton>) {
        for key in self.held.drain(..) {
            keys.release(key);
        }
        if let Some(button) = self.clicked.take() {
            mouse.release(button);
        }
        self.pressed = None;
    }

    fn stop(
        &mut self,
        keys: &mut ButtonInput<KeyCode>,
        mouse: &mut ButtonInput<MouseButton>,
        reason: &str,
    ) {
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
);

type Input<'w> = (
    ResMut<'w, ButtonInput<KeyCode>>,
    ResMut<'w, ButtonInput<MouseButton>>,
);

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
    (mut chat, mut moves, mut front): (
        ResMut<super::chat::ChatState>,
        ResMut<super::windows::Moves>,
        ResMut<super::login::FrontEnd>,
    ),
    observed: Observed,
    players: Query<&Transform, With<super::Player>>,
    bodies: Query<&super::PlayerBody, With<super::Player>>,
    mut collision: Option<ResMut<super::Collision>>,
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
    if let Some(event) = focus.read().last() {
        script.focus = Some(event.focused);
    }
    let (mut keys, mut mouse) = input;
    let now = time.elapsed();
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
                let ends = bodies
                    .single()
                    .ok()
                    .and_then(|body| walk_ends(&online, *body, script.approached));
                let world = collision
                    .as_ref()
                    .and_then(|collision| collision.0.as_ref());
                let (Some((feet, goal)), Some(world)) = (ends, world) else {
                    script.stop(&mut keys, &mut mouse, "walk lost its goal or collision");
                    return;
                };
                let Some(route) = script.route.as_mut() else {
                    script.stop(&mut keys, &mut mouse, "walk lost its route");
                    return;
                };
                let searching = route.is_searching();
                let step = route.next(world, feet, goal, SEARCH_BUDGET, now);
                if searching && !route.is_searching() {
                    let (waypoints, partial) = (route.remaining(), route.partial());
                    let (expanded, open) = route.searched().unwrap_or_default();
                    let (height, drops) = (route.height(), observed.4.airborne.is_some());
                    let reach = route.reach();
                    info!(
                        waypoints,
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
                    if partial
                        && let Some(end) = route.end()
                        && let Some(world) = collision
                            .as_mut()
                            .and_then(|collision| collision.bypass_change_detection().0.as_mut())
                    {
                        log_partial_route(&online, world, [feet, goal, end], reach, height, drops);
                    }
                } else if !searching && route.is_searching() {
                    let refused = observed.4.refused.as_deref();
                    info!(?refused, "Script walk stalled; searching again from here");
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
                    RouteStep::Searching => {
                        // Stand still while a stalled route searches again.
                        script.release(&mut keys, &mut mouse);
                        (false, elapsed >= *duration)
                    }
                    RouteStep::Toward(next) => {
                        let to = next - feet;
                        // W walks along the camera's forward direction, opposite its orbit offset.
                        for mut camera in &mut cameras {
                            camera.yaw = to.x.atan2(to.z) + std::f32::consts::PI;
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
            }
            return;
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
                // W walks along the camera's forward direction, opposite its orbit offset.
                camera.yaw = eq_client_core::render_heading(base + *heading) + std::f32::consts::PI;
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
            let route = bodies.single().ok().and_then(|body| {
                let (feet, goal) = walk_ends(&online, *body, script.approached)?;
                // Ledges are routes only where the session simulates falls.
                Some(
                    eq_client_core::movement::Route::new(feet, goal, *range, body.height)
                        .with_drops(observed.4.airborne.is_some()),
                )
            });
            let Some(route) = route else {
                script.stop(&mut keys, &mut mouse, "walk needs a visible target");
                return;
            };
            script.route = Some(route);
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

/// The player's feet at its accepted position and the goal's position, in
/// render coordinates: the world object `object` names, or else the target.
/// Diagnoses a partial route: the doors nearest where it ends, and whether the
/// same search finds the goal with every door left out, once per partial route
/// at the usual cap. Logging only; the walk keeps its own route. Diagnostic:
/// the door-less repeat goes before the fix is marked.
fn log_partial_route(
    online: &super::online::OnlineState,
    world: &mut eq_client_core::movement::CollisionWorld,
    [feet, goal, end]: [Vec3; 3],
    reach: f32,
    height: f32,
    drops: bool,
) {
    use eq_client_core::movement::{PathProgress, PathSearch};
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
    let obstacles = world.take_obstacles();
    let mut search = PathSearch::new(feet, goal, reach, height).with_drops(drops);
    let found = loop {
        match search.advance(world, 5000) {
            PathProgress::Searching => (),
            done => break matches!(done, PathProgress::Found(_)),
        }
    };
    world.set_obstacles(obstacles);
    info!(
        found,
        expanded = search.expanded(),
        open = search.has_open(),
        "Script route without doors"
    );
}

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
            // W walks along the camera's forward direction, opposite its orbit offset.
            camera.yaw = flat.x.atan2(flat.y) + std::f32::consts::PI;
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

#[cfg(test)]
mod tests {
    use super::*;

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
            let yaw = eq_client_core::render_heading(heading) + std::f32::consts::PI;
            let walk = super::super::camera_relative_direction(0.0, 1.0, yaw);
            let expected = eq_client_core::world_heading(walk.x.atan2(walk.z));
            let error = (expected - heading).rem_euclid(512.0);
            assert!(error.min(512.0 - error) < 0.01, "{heading} -> {expected}");
        }
    }
}
