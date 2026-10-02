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

pub use parse::{AbilityPage, ClickTarget, PickButton, Step, TradeClick, TrainingClick, parse};

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
            started: None,
            paused: false,
            chat_seen: 0,
            follow: None,
            local: false,
            offline: false,
            focus: None,
            route: None,
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
        Option<&'static super::spellbook::BookEntry>,
        Option<&'static super::spellbook::GemChoice>,
        Option<&'static super::trade::Action>,
        Option<&'static super::inventory::colors::Action>,
        Has<super::give::GiveButton>,
        (
            Option<&'static super::coins::CoinBox>,
            Option<&'static super::inventory::SplitAction>,
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
            Option<&'static super::skinned::LevelSlider>,
            Option<&'static super::skinned::DropDown>,
            Option<&'static super::skinned::DropDownChoice>,
            (
                Option<&'static super::training::SkillRow>,
                Has<super::training::TrainButton>,
                Option<&'static super::skinned::Closes>,
                Option<&'static super::resurrection::AnswerButton>,
                Option<&'static super::reading::PageButton>,
                Option<&'static super::tradeskills::CombineButton>,
                Option<&'static super::map::MapButton>,
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
    (
        Res<'w, super::hud::HudState>,
        Res<'w, super::notices::Lines>,
    ),
    Res<'w, crate::outbox::Outbox>,
    Res<'w, super::trade::TradeState>,
    Res<'w, super::combat::CombatState>,
    Res<'w, super::motion::Controls>,
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
    mut chat: ResMut<super::chat::ChatState>,
    observed: Observed,
    players: Query<&Transform, With<super::Player>>,
    bodies: Query<&super::PlayerBody, With<super::Player>>,
    collision: Option<Res<super::Collision>>,
    mut cameras: Query<&mut super::OrbitCamera>,
    (mut buttons, layout, mut pointers): (Buttons, Layout, Pointers),
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
        script.local || script.offline,
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
    // One-frame presses and clicks are released on the frame after they were pressed.
    if matches!(
        script.current,
        Some((
            Step::Press(_) | Step::Select(_) | Step::Click(_) | Step::RightClick(_),
            _
        ))
    ) && (!script.held.is_empty() || script.clicked.is_some())
    {
        script.release(&mut keys, &mut mouse);
        script.current = None;
    }
    if let Some((step, since)) = script.current.clone() {
        let elapsed = now.saturating_sub(since);
        let done = match &step {
            Step::Wait(duration) => elapsed >= *duration,
            Step::Approach(range, duration) => {
                let distance = face(&online, &players, &mut cameras);
                distance.is_none_or(|distance| distance <= *range) || elapsed >= *duration
            }
            Step::Walk(_, duration) => {
                use eq_client_core::movement::RouteStep;
                let ends = bodies
                    .single()
                    .ok()
                    .and_then(|body| walk_ends(&online, *body));
                let world = collision
                    .as_ref()
                    .and_then(|collision| collision.0.as_ref());
                let (Some((feet, goal)), Some(world)) = (ends, world) else {
                    script.stop(&mut keys, &mut mouse, "walk lost its target or collision");
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
                    info!(waypoints, partial, "Script route ready");
                } else if !searching && route.is_searching() {
                    let refused = observed.4.refused.as_deref();
                    info!(?refused, "Script walk stalled; searching again from here");
                }
                match step {
                    RouteStep::Unreachable => {
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
                        script.stop(&mut keys, &mut mouse, &reason);
                        return;
                    }
                    RouteStep::Arrived => true,
                    RouteStep::Searching => {
                        // Stand still while a stalled route searches again.
                        script.release(&mut keys, &mut mouse);
                        elapsed >= *duration
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
                        elapsed >= *duration
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
            Step::WaitSelect | Step::WaitOnline | Step::WaitZone(_) => {
                if elapsed > MAX_ONLINE_WAIT {
                    script.stop(&mut keys, &mut mouse, "server state did not arrive");
                    return;
                }
                match &step {
                    Step::WaitSelect => online.selection.is_some(),
                    Step::WaitZone(zone) => {
                        online.world().connected()
                            && online.world().player().is_some()
                            && online.world().zone() == *zone
                    }
                    _ => online.world().connected() && online.world().player().is_some(),
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
                if !click(*target, &mut buttons, &layout, &mut pointers) {
                    script.stop(&mut keys, &mut mouse, "click target is not visible");
                    return;
                }
                let button = if matches!(step, Step::RightClick(_)) {
                    MouseButton::Right
                } else {
                    MouseButton::Left
                };
                mouse.press(button);
                script.clicked = Some(button);
                return;
            }
            _ => true,
        };
        if !done {
            return;
        }
        script.release(&mut keys, &mut mouse);
        script.current = None;
        script.route = None;
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
        Step::Click(_) | Step::RightClick(_) => {
            if window.is_some_and(|window| window.cursor_position().is_some()) {
                info!("Scripted click waits until the pointer leaves the client window");
            }
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
        Step::Walk(range, _) => {
            let route = bodies.single().ok().and_then(|body| {
                let (feet, goal) = walk_ends(&online, *body)?;
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
        Step::WaitSelect
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

/// Whether a control of the Training window or the confirmation dialog is
/// the one a click names.
fn dialog_control(
    target: ClickTarget,
    (skill_row, train, closes, answer): (
        Option<&super::training::SkillRow>,
        bool,
        Option<&super::skinned::Closes>,
        Option<&super::resurrection::AnswerButton>,
    ),
) -> bool {
    match target {
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

/// Marks the first visible matching control in an open window pressed; the
/// focus system clears it next frame.
fn click(
    target: ClickTarget,
    buttons: &mut Buttons,
    layout: &Layout,
    pointers: &mut Pointers,
) -> bool {
    for (
        entity,
        mut interaction,
        visibility,
        slot,
        scribe,
        store,
        row,
        gem,
        trade,
        tint,
        give,
        (coins, pick),
        (selector, tab, ability, attack, slash, checkbox),
        (slider, drop_down, choice, (skill_row, train, closes, answer, page, combine, map)),
    ) in buttons.iter_mut()
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
            ClickTarget::Pet(command) => slash.is_some_and(|button| button.0 == command),
            ClickTarget::Option(toggle) => checkbox.is_some_and(|checkbox| checkbox.0 == toggle),
            ClickTarget::Slider(level, _) => slider.is_some_and(|slider| slider.level == level),
            ClickTarget::KeyFilter => drop_down.is_some_and(|drop_down| {
                drop_down.choosing() == super::skinned::Choosing::KeyFilter
            }),
            ClickTarget::Choice(index) => choice.is_some_and(|choice| choice.index == index),
            ClickTarget::Answer(_) | ClickTarget::Training(_) => {
                dialog_control(target, (skill_row, train, closes, answer))
            }
            ClickTarget::Page(forward) => page.is_some_and(|page| page.0 == forward),
            ClickTarget::Combine(slot) => combine.is_some_and(|combine| combine.0.0 == slot),
            ClickTarget::Map(action) => map.is_some_and(|button| *button == action),
            ClickTarget::Slot(number) => slot.is_some_and(|slot| slot.0.0 == number),
            ClickTarget::Scribe => scribe,
            ClickTarget::Store => store,
            ClickTarget::BookRow(number) => row.is_some_and(|row| row.0 == number),
            ClickTarget::MemorizeGem(number) => gem.is_some_and(|gem| gem.0 == number),
            ClickTarget::Trade(click) => trade.is_some_and(|action| {
                use super::trade::Action;
                *action
                    == match click {
                        TradeClick::Take(slot) => Action::Take(slot),
                        TradeClick::TakeAll => Action::TakeAll,
                        TradeClick::EndLoot => Action::EndLoot,
                        TradeClick::Buy(slot) => Action::Buy(slot),
                        TradeClick::Sell(slot) => Action::Sell(slot),
                        TradeClick::EndShop => Action::EndShop,
                    }
            }),
            ClickTarget::Tint(number, color) => tint.is_some_and(|action| {
                use super::inventory::colors::Action;
                match (action, color) {
                    (Action::Toggle(slot), None) => slot.0 == number,
                    (Action::Set(slot, index), Some(color)) => slot.0 == number && *index == color,
                    _ => false,
                }
            }),
            ClickTarget::Give => give,
            ClickTarget::Coins(place, coin) => {
                coins.is_some_and(|coins| coins.place == place && coins.coin == coin)
            }
            ClickTarget::Pick(button) => pick.is_some_and(|action| {
                use super::inventory::SplitAction;
                *action
                    == match button {
                        PickButton::Less => SplitAction::Less,
                        PickButton::More => SplitAction::More,
                        PickButton::Min => SplitAction::Minimum,
                        PickButton::Max => SplitAction::Maximum,
                        PickButton::Confirm => SplitAction::Confirm,
                        PickButton::Cancel => SplitAction::Cancel,
                    }
            }),
        };
        if matches && visibility.get() && displayed(entity, layout) {
            // A slider is pressed where the setting is to go.
            if let (ClickTarget::Slider(_, percent), Some(slider), Ok(mut pointer)) =
                (target, slider, pointers.get_mut(entity))
            {
                pointer.normalized =
                    Some(Vec2::new(slider.across(f32::from(percent) / 100.0), 0.0));
            }
            *interaction = Interaction::Pressed;
            return true;
        }
    }
    false
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

/// The player's feet at its accepted position and the target's position, in
/// render coordinates.
fn walk_ends(online: &super::online::OnlineState, body: super::PlayerBody) -> Option<(Vec3, Vec3)> {
    let player = online.world().player()?;
    let spawn = online
        .world()
        .target()
        .selected
        .and_then(|id| online.world().spawn(id).map(|spawn| &spawn.state))?;
    let origin = Vec3::from_array(eq_client_core::render_position(player.position));
    let goal = Vec3::from_array(eq_client_core::render_position(spawn.position));
    Some((origin - Vec3::Y * body.feet_offset, goal))
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
    let transform = players.single().ok()?;
    let to =
        Vec3::from_array(eq_client_core::render_position(spawn.position)) - transform.translation;
    let flat = Vec2::new(to.x, to.z);
    if flat.length() > f32::EPSILON {
        for mut camera in cameras.iter_mut() {
            // W walks along the camera's forward direction, opposite its orbit offset.
            camera.yaw = flat.x.atan2(flat.y) + std::f32::consts::PI;
        }
    }
    Some(flat.length())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut state = SystemState::<(Buttons, Layout, Pointers)>::new(&mut world);
        let (mut buttons, layout, mut pointers) = state.get_mut(&mut world).unwrap();
        assert!(!click(
            ClickTarget::Give,
            &mut buttons,
            &layout,
            &mut pointers
        ));
        let open = button(&mut world, Display::Flex);
        let (mut buttons, layout, mut pointers) = state.get_mut(&mut world).unwrap();
        assert!(click(
            ClickTarget::Give,
            &mut buttons,
            &layout,
            &mut pointers
        ));
        assert_eq!(world.get::<Interaction>(open), Some(&Interaction::Pressed));
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
