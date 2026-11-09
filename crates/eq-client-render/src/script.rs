//! Bounded scenarios shared by the in-game workbench and command-line runner.
//!
//! Public-server runs require focus unless background input is explicitly enabled.
//! Offline and network-constrained local test runs can proceed without focus;
//! all runs have bounded time and use the ordinary input and action paths.
//! Attended clicks wait for the real pointer to leave the window. Explicit
//! background ownership isolates native UI focus without moving that pointer;
//! each click
//! and hover first glides the script's own pointer to its control (the
//! `pointer` module), scrolling a list the control has scrolled out of view as
//! the wheel does, and lands only on the control under it. A followed
//! script keeps reading complete lines appended to its file, under the same limits.
//! `gm`, `chat` and `social` steps require a network-constrained local EQEmu/TAKP session.
pub(crate) mod control;
mod execution;
mod input;
pub(crate) use input::AuthoredInput;
pub use input::InputMode;
pub(super) use input::install as install_input;
pub(crate) mod language;
mod parse;
pub(crate) mod pointer;
mod program;
mod report;

use execution::RunMode;
pub use execution::{Phase, RunEvent, RunStatus};
pub(crate) use execution::{Request, RunControl};
use program::AppendError;
pub use program::{Instruction, MAX_SOURCE_BYTES, ParseError, Program};
mod source;
pub use source::read_source;

pub use parse::{
    AbilityPage, CharacterClick, ClickTarget, PickButton, RaidClick, SliderClick, Step, TradeClick,
    TrainingClick, parse,
};
pub(crate) use pointer::ScriptPointer;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::PrimaryWindow;

use crate::keys::Act;
use parse::{MAX_STEPS, MAX_WAIT, MoveCommand};

/// The command the steps that move toward something hold: forward the way
/// the camera looks, which they turn toward their aim.
const FORWARD: Act = Act::CameraForward;
/// Longest a `face` step waits for the server to have its heading: a half
/// turn at the turn keys' rate takes about a second.
const FACE_LIMIT: Duration = Duration::from_secs(5);
/// How near the heading the server has must come to a `face` step's, in
/// heading units.
const FACED: f32 = 0.5;
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
#[allow(
    clippy::struct_excessive_bools,
    reason = "pause, local/offline policies and current input ownership are independent facts"
)]
pub struct Script {
    steps: VecDeque<QueuedStep>,
    language: Option<language::Run>,
    current: Option<(Step, Duration)>,
    progress: RunStatus,
    mode: RunMode,
    /// This frame successfully queued a social using the single-step control.
    stepped_social: bool,
    held: Vec<KeyCode>,
    /// The movement commands a moving step holds itself, as the key map
    /// resolves a held key to them, so the step moves under any key table.
    acts: Vec<Act>,
    /// The heading a `face` step turns the player to, until it is reached.
    facing: Option<f32>,
    /// The mouse button a scripted click holds down until the next frame.
    clicked: Option<MouseButton>,
    /// The control a scripted click pressed, which the pointer is over as
    /// the button is let go.
    pressed: Option<Entity>,
    started: Option<Duration>,
    /// Whether this run has entered a live session that must remain connected.
    session: RunSession,
    paused: bool,
    /// Newest chat line already included in a report.
    chat_seen: u64,
    /// Most recent same-zone movement receipt before the run or already consumed.
    relocation_seen: Option<(u64, u64)>,
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
    input_mode: InputMode,
    /// True only after this frame's background ownership check succeeds.
    owns_input: bool,
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
    /// The script pointer's glide to the control a click or hover names.
    glide: Option<pointer::Glide>,
    /// The wheel turning over a list until the control a click or hover
    /// names shows whole in it, before the glide to the control.
    scrolling: Option<pointer::Scrolling>,
    /// The notch the wheel turns this frame over a list, in pixels with
    /// upward positive, which [`pointer::turn`] gives the list.
    notch: Option<(Entity, f32)>,
    /// The control the script pointer last hovered on its way, whose hover
    /// it clears as it leaves.
    pointed: Option<Entity>,
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
    /// Full intent captured by the ordinary character-deletion owner.
    deleting: Option<eq_client_core::deletion::Request>,
    /// The queued menu entry's opener, first waiting for its guarded click
    /// to finish, then for the menu's state after the local release. A
    /// release does not establish that the menu's handler opened it.
    menu_opening: Option<MenuOpening>,
}

/// Login and offline scripts need no admission; admitted runs end with the session.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RunSession {
    BeforeAdmission,
    Admitted,
}

/// A source location travels with its action rather than a parallel queue.
#[derive(Clone, Debug, PartialEq)]
struct QueuedStep {
    step: Step,
    line: Option<usize>,
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
            progress: RunStatus::queued(steps.len(), std::sync::Arc::from("")),
            mode: RunMode::Continuous,
            stepped_social: false,
            steps: steps
                .into_iter()
                .map(|step| QueuedStep { step, line: None })
                .collect(),
            language: None,
            current: None,
            held: Vec::new(),
            acts: Vec::new(),
            facing: None,
            clicked: None,
            pressed: None,
            started: None,
            session: RunSession::BeforeAdmission,
            paused: false,
            chat_seen: 0,
            relocation_seen: None,
            follow: None,
            local: false,
            offline: false,
            input_mode: InputMode::Attended,
            owns_input: false,
            route: None,
            door_wait: None,
            following: None,
            quiet: None,
            search_frames: None,
            last_frame: None,
            hovering: None,
            glide: None,
            scrolling: None,
            notch: None,
            pointed: None,
            awaited: None,
            approached: None,
            chase_seen: None,
            created_before: None,
            deleting: None,
            menu_opening: None,
        }
    }

    /// Queues a source snapshot, retaining line numbers for progress and errors.
    #[must_use]
    pub fn from_program(program: Program) -> Self {
        let accepted = program.statement_count();
        let (source, instructions, code) = program.into_parts();
        let mut script = Self {
            accepted,
            progress: RunStatus::queued(instructions.len(), source),
            ..Self::new(Vec::new())
        };
        if let Some(code) = code {
            script.language = Some(language::Run::new(code, instructions));
            script.progress.flow = Some(execution::FlowStatus::default());
        } else {
            script.steps = instructions
                .into_iter()
                .map(|instruction| QueuedStep {
                    step: instruction.step,
                    line: Some(instruction.line),
                })
                .collect();
        }
        script
    }

    /// Builds a viewer run with the process's explicit input policy and the
    /// existing local/offline permissions. Replacement runs use this same path.
    #[must_use]
    pub fn for_viewer(program: Program, config: &crate::ViewerConfig, online: bool) -> Self {
        Self::from_program(program)
            .local_session(config.local_session)
            .offline_preview(!online)
            .input_mode(config.script_input_mode)
    }

    /// The current run's typed progress, including its immutable source snapshot.
    #[must_use]
    pub const fn status(&self) -> &RunStatus {
        &self.progress
    }

    /// Queues validated steps, then keeps reading complete lines appended to `path`
    /// after the first `offset` bytes.
    #[must_use]
    pub fn following(steps: Vec<Step>, path: PathBuf, offset: usize) -> Self {
        Self::new(steps).with_follow(path, offset)
    }

    /// Continues this snapshot with complete lines appended after `offset` bytes.
    #[must_use]
    pub fn with_follow(mut self, path: PathBuf, offset: usize) -> Self {
        self.follow = Some(Follow {
            path,
            offset,
            next_poll: Duration::ZERO,
            idle: false,
        });
        self
    }

    /// Marks the session local-only (the `Script::local` field); set only when the
    /// network session refuses servers outside this machine's network.
    #[must_use]
    pub fn local_session(mut self, local: bool) -> Self {
        self.local = local;
        self
    }

    /// Selects input ownership without changing session or command permissions.
    #[must_use]
    pub fn input_mode(mut self, mode: InputMode) -> Self {
        self.input_mode = mode;
        self
    }

    /// Whether this frame accepts scripted input without native window focus.
    #[must_use]
    pub(crate) const fn unattended(&self) -> bool {
        match self.input_mode {
            InputMode::Attended => self.local || self.offline,
            InputMode::Background => self.owns_input,
        }
    }

    /// Background runs yield their drawn pointer and carried overlay together
    /// with their input; existing attended/local pointer behavior is retained.
    pub(crate) const fn pointer_owned(&self) -> bool {
        match self.input_mode {
            InputMode::Attended => true,
            InputMode::Background => self.owns_input,
        }
    }

    /// Check the exact local executor and its authority for this frame.
    pub(crate) fn social_dispatch(
        &self,
        owner: &std::sync::Arc<()>,
    ) -> Option<crate::socials::ScriptDispatch> {
        use crate::socials::ScriptDispatch;

        if !self.local
            || !std::sync::Arc::ptr_eq(&self.progress.identity, owner)
            || !matches!(self.progress.phase, Phase::Running | Phase::Following)
        {
            return None;
        }
        match self.mode {
            RunMode::Continuous => Some(ScriptDispatch::Continue),
            RunMode::PauseRequested if self.stepped_social => Some(ScriptDispatch::OneLine),
            _ => None,
        }
    }

    /// Whether native pointer hit testing yields to the explicit script owner.
    pub(crate) const fn background_owns_input(&self) -> bool {
        matches!(self.input_mode, InputMode::Background) && self.owns_input
    }

    /// The movement commands the script holds itself.
    pub(crate) fn acts(&self) -> &[Act] {
        &self.acts
    }

    /// The heading a `face` step turns the player to.
    pub(crate) const fn facing(&self) -> Option<f32> {
        self.facing
    }

    /// Whether cancellation must release input before yielding the window.
    fn holding(&self) -> bool {
        !self.held.is_empty()
            || !self.acts.is_empty()
            || self.facing.is_some()
            || self.clicked.is_some()
    }

    /// Marks the run offline (the `Script::offline` field).
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
        let bytes = match source::read_bytes(&follow.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                self.progress.finish(Phase::Failed, error.to_string());
                self.follow = None;
                return;
            }
            Err(_) => return,
        };
        #[allow(clippy::naive_bytecount)]
        // Bounded 128 KiB source; no SIMD dependency for line labels.
        let first_line = bytes[..follow.offset.min(bytes.len())]
            .iter()
            .filter(|byte| **byte == b'\n')
            .count();
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
        let text = String::from_utf8_lossy(&fresh[..=end]);
        let base = follow.path.parent().unwrap_or_else(|| Path::new("."));
        let parsed = if self.language.is_some() {
            // Appended actions would jump ahead of the running program's next
            // statement, so a control-flow run takes no appended lines.
            Err(AppendError::Structured(Some(1)))
        } else {
            Program::parse_append(&text, base)
        };
        match parsed {
            Ok(instructions) if self.accepted + instructions.len() <= MAX_STEPS => {
                self.accepted += instructions.len();
                self.progress.total += instructions.len();
                self.steps
                    .extend(instructions.into_iter().map(|instruction| QueuedStep {
                        step: instruction.step,
                        line: Some(first_line + instruction.line),
                    }));
                let source = String::from_utf8_lossy(&bytes[..follow.offset]);
                self.progress.source = std::sync::Arc::from(source::without_bom(&source));
            }
            Ok(_) => warn!("Ignored appended steps beyond the {MAX_STEPS}-step limit"),
            Err(AppendError::Invalid(mut error)) => {
                error.line = error.line.map(|line| first_line + line);
                warn!("Ignored appended script lines: {error}");
            }
            Err(AppendError::Structured(line)) => {
                self.progress.line = line.map(|line| first_line + line);
                let snapshot = String::from_utf8_lossy(&bytes[..follow.offset]);
                self.progress.source = std::sync::Arc::from(source::without_bom(&snapshot));
                let reason = "Follow appends accept ordinary action lines; run a complete control-flow file as a new snapshot";
                self.progress.finish(Phase::Failed, reason);
                warn!("Ignored appended script lines: {reason}");
                self.follow = None;
            }
        }
    }

    fn exhausted(&self) -> bool {
        self.steps.is_empty() && self.language.as_ref().is_none_or(language::Run::done)
    }

    /// Keep the ordinary action queue authoritative, including menu-entry retries.
    fn queue_statement(
        &mut self,
        world: &eq_client_core::world::ClientWorld,
        now: Duration,
    ) -> Result<(), String> {
        if self.steps.is_empty()
            && let Some(run) = &mut self.language
            && let language::Tick::Action(step) = run.tick(world, &mut self.progress, now)?
        {
            self.steps.push_back(step);
        }
        Ok(())
    }

    fn release(&mut self, keys: &mut Keyboard, mouse: &mut ButtonInput<MouseButton>) {
        for key in self.held.drain(..) {
            keys.release(key);
        }
        self.acts.clear();
        self.facing = None;
        if let Some(button) = self.clicked.take() {
            // A genuine press of the same button belongs to the user now.
            if !keys.native_button(button) {
                keys.mark_pointer(button);
                mouse.release(button);
            }
        }
        self.pressed = None;
    }

    /// Cancels a synthetic press before releasing it, so cancellation cannot click.
    fn cancel_press(&self, buttons: &mut Buttons, commands: &mut Commands) {
        if let Some(entity) = self.pressed
            && let Ok((_, mut interaction, ..)) = buttons.get_mut(entity)
        {
            *interaction = Interaction::None;
        }
        let cancel_right = self.clicked == Some(MouseButton::Right);
        commands.queue(move |world: &mut World| {
            if let Some(mut button) = world.get_resource_mut::<crate::clicks::LeftButton>() {
                button.cancel();
            }
            if let Some(mut presses) =
                world.get_resource_mut::<crate::hud::hotbar::carry::Presses>()
            {
                *presses = crate::hud::hotbar::carry::Presses::default();
            }
            if cancel_right {
                if let Some(mut press) = world.get_resource_mut::<crate::hud::GemPress>() {
                    press.cancel();
                }
                if let Some(mut press) = world.get_resource_mut::<crate::buffs::BuffPress>() {
                    press.cancel();
                }
            }
        });
    }

    fn stop(&mut self, keys: &mut Keyboard, mouse: &mut ButtonInput<MouseButton>, reason: &str) {
        self.release(keys, mouse);
        self.owns_input = false;
        self.steps.clear();
        self.language = None;
        self.current = None;
        self.follow = None;
        self.route = None;
        self.glide = None;
        self.scrolling = None;
        self.notch = None;
        self.awaited = None;
        self.approached = None;
        self.progress.finish(Phase::Failed, reason);
        info!("Script stopped: {reason}");
    }

    /// Retains the outcome after releasing the active executor resource.
    fn retire(&self, commands: &mut Commands) {
        commands.insert_resource(self.progress.clone());
        commands.remove_resource::<Self>();
    }

    /// Resumes without charging an explicit boundary pause against the run limit.
    fn resume(&mut self, now: Duration, mode: RunMode) {
        if let RunMode::Paused(since) = self.mode {
            if let Some(started) = &mut self.started {
                *started += now.saturating_sub(since);
            }
            self.progress.event("Resumed");
        }
        self.mode = mode;
        self.progress.phase = Phase::Running;
    }

    /// Returns whether another action may start; every caller has released input.
    fn advance(&mut self, now: Duration) -> bool {
        match self.mode {
            RunMode::Continuous => true,
            RunMode::OneStep => {
                self.mode = RunMode::PauseRequested;
                true
            }
            RunMode::PauseRequested => {
                self.mode = RunMode::Paused(now);
                self.progress.phase = Phase::Paused;
                self.progress.event("Paused between actions");
                false
            }
            RunMode::Paused(_) => false,
        }
    }
}

/// Copies observable progress without requiring the editor to inspect executor internals.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn observe(script: Option<Res<Script>>, mut status: ResMut<RunStatus>) {
    if let Some(script) = script {
        // Follow mode can extend the source without replacing this execution.
        if !std::sync::Arc::ptr_eq(&status.identity, &script.progress.identity)
            || !std::sync::Arc::ptr_eq(&status.source, &script.progress.source)
            || status.revision != script.progress.revision
        {
            *status = script.progress.clone();
        } else {
            status.phase = script.progress.phase;
            status.elapsed = script.progress.elapsed;
            status.total = script.progress.total;
        }
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
        (
            Has<super::inventory::StoreCursor>,
            Option<&'static super::inventory::destruction::Action>,
            Has<super::inventory::ground_drop::DropButton>,
        ),
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
            Has<super::group::JoinButton>,
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
type Layout<'w, 's> = Query<
    'w,
    's,
    (
        &'static Node,
        Option<&'static ChildOf>,
        Option<&'static bevy::ui::ComputedStackIndex>,
    ),
>;

/// What a step drives besides the world: chat, window moves, the front end,
/// sounds and the EQ menu's state.
type Services<'w> = (
    ResMut<'w, super::chat::ChatState>,
    ResMut<'w, super::windows::Moves>,
    ResMut<'w, super::login::FrontEnd>,
    ResMut<'w, super::sounds::Sounds>,
    Res<'w, super::windows::menu::Menu>,
    ResMut<'w, super::windows::Shown>,
    ResMut<'w, super::items::ItemState>,
);

type Observed<'w> = (
    Res<'w, super::hud::HudState>,
    Res<'w, crate::outbox::Outbox>,
    ResMut<'w, super::trade::TradeState>,
    Res<'w, super::combat::CombatState>,
    Res<'w, super::motion::Controls>,
    Option<Res<'w, crate::loading::Loading>>,
    Option<Res<'w, super::doors::Hinges>>,
    Option<Res<'w, super::zone_navmesh::ZoneNavMesh>>,
    Option<Res<'w, crate::spellbook::SpellNames>>,
    Option<Res<'w, crate::options::OptionsState>>,
    Option<Res<'w, crate::confirm::Asked>>,
    Option<Res<'w, crate::breath::State>>,
    Option<Res<'w, crate::projectiles::Flights>>,
    Option<Res<'w, crate::socials::State>>,
    Option<Res<'w, crate::options::MovementSync>>,
);

type Input<'w, 's> = (Keyboard<'w, 's>, ResMut<'w, ButtonInput<MouseButton>>);

type DriverContext<'w, 's> = (
    Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    Option<Res<'w, super::ViewerSettings>>,
    Option<ResMut<'w, RunControl>>,
    Query<'w, 's, Has<super::camera::FreeCamera>, With<super::OrbitCamera>>,
);

/// The keyboard as a script presses it: each press and release sets the
/// key's state and sends the typed key the window would, so what reads
/// typed keys (the chat line, the login boxes) takes a scripted key as it
/// takes a real one.
#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct Keyboard<'w, 's> {
    state: ResMut<'w, ButtonInput<KeyCode>>,
    typed: MessageWriter<'w, bevy::input::keyboard::KeyboardInput>,
    window: Query<'w, 's, Entity, With<PrimaryWindow>>,
    background: Option<ResMut<'w, input::BackgroundInput>>,
    authored: Option<ResMut<'w, AuthoredInput>>,
}

impl Keyboard<'_, '_> {
    fn press(&mut self, key: KeyCode) {
        if !self.state.pressed(key) {
            self.send(key, bevy::input::ButtonState::Pressed);
        }
        self.state.press(key);
    }

    fn release(&mut self, key: KeyCode) {
        if self
            .background
            .as_ref()
            .is_some_and(|state| state.keys.contains(&key))
        {
            return;
        }
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
        let id = self.typed.write(bevy::input::keyboard::KeyboardInput {
            key_code,
            logical_key,
            state,
            // A release types nothing.
            text: text.filter(|_| state.is_pressed()).map(Into::into),
            repeat: false,
            window: self.window.single().unwrap_or(Entity::PLACEHOLDER),
        });
        if let Some(background) = &mut self.background {
            background.authored(id);
        }
        if let Some(authored) = &mut self.authored {
            authored.typed.insert(id);
            authored.sent.insert(key_code);
            if state.is_pressed() {
                authored.keys.insert(key_code);
            }
        }
    }

    fn mark_pointer(&mut self, button: MouseButton) {
        if button == MouseButton::Left
            && let Some(authored) = &mut self.authored
        {
            authored.left_button = true;
        }
    }

    fn native_button(&self, button: MouseButton) -> bool {
        self.background
            .as_ref()
            .is_some_and(|state| state.buttons.contains(&button))
    }

    /// A backend focus-loss reset must not retrigger held actions or type text.
    fn restore(&mut self, held: &[KeyCode]) {
        for &key in held {
            if !self.state.pressed(key) {
                self.state.press(key);
                self.state.clear_just_pressed(key);
                self.state.clear_just_released(key);
            }
        }
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
    (mut chat, mut moves, mut front, mut sounds, menu, mut shown, mut item): Services,
    mut observed: Observed,
    players: Query<&Transform, With<super::Player>>,
    bodies: Query<&super::PlayerBody, With<super::Player>>,
    collision: Option<Res<super::Collision>>,
    mut cameras: Query<&mut super::OrbitCamera>,
    (mut buttons, layout, mut pointers, needs, mut aim): (
        Buttons,
        Layout,
        Pointers,
        Query<&crate::outbox::Needs>,
        pointer::Aim,
    ),
    (windows, settings, mut control, detached_camera): DriverContext,
    mut focus: MessageReader<bevy::window::WindowFocused>,
    mut reported_focus: Local<Option<(Entity, bool)>>,
    mut exit: MessageWriter<AppExit>,
) {
    // Observe focus even between runs. A script started from the workbench
    // must not wait for another focus event from an already focused window.
    for event in focus.read().filter(|event| windows.contains(event.window)) {
        *reported_focus = Some((event.window, event.focused));
    }
    let (mut keys, mut mouse) = input;
    if let Some(authored) = &mut keys.authored {
        **authored = AuthoredInput::default();
    }
    let Some(mut script) = script else {
        return;
    };
    let client = settings.map_or_else(Default::default, |settings| settings.0.installed_client);
    // Plain borrows of the fields, so a walk can hold its route and note
    // where it heads at once.
    let script = &mut *script;
    script.stepped_social = false;
    let now = time.elapsed();
    if !script.progress.phase.active() {
        script.retire(&mut commands);
        return;
    }
    if online.enabled && online.world().connected() {
        script.session = RunSession::Admitted;
    }
    if script.session == RunSession::Admitted && online.world().ended() {
        script.cancel_press(&mut buttons, &mut commands);
        script.stop(&mut keys, &mut mouse, "Session ended");
        script.retire(&mut commands);
        return;
    }
    if let Some(request) = control.as_mut().and_then(|control| control.take()) {
        match request {
            Request::Stop => {
                script.cancel_press(&mut buttons, &mut commands);
                script.stop(&mut keys, &mut mouse, "Stopped by user");
                script.progress.phase = Phase::Stopped;
                script.retire(&mut commands);
                return;
            }
            Request::Pause => {
                if !matches!(script.mode, RunMode::Paused(_)) {
                    script.mode = RunMode::PauseRequested;
                    script.progress.phase = Phase::Pausing;
                }
            }
            Request::Resume => script.resume(now, RunMode::Continuous),
            Request::Step if matches!(script.mode, RunMode::Paused(_)) => {
                script.resume(now, RunMode::OneStep);
            }
            Request::Step => (),
        }
    }
    if matches!(script.mode, RunMode::Paused(_)) {
        script.owns_input = false;
        script.last_frame = Some(now);
        return;
    }
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
    if script.started.is_none() {
        script.relocation_seen = online
            .world()
            .last_same_zone_relocation()
            .map(|(revision, relocation)| (relocation.session_id, revision));
    }
    let started = *script.started.get_or_insert(now);
    script.progress.elapsed = now.saturating_sub(started);
    if now.saturating_sub(started) > MAX_RUNTIME {
        script.cancel_press(&mut buttons, &mut commands);
        script.stop(&mut keys, &mut mouse, "maximum runtime reached");
        script.retire(&mut commands);
        return;
    }
    let window = windows.single().ok();
    if script.input_mode == InputMode::Background {
        // Watching the window does not take input ownership. Only genuine
        // keyboard, button or wheel activity yields the background runner.
        let native = keys.background.as_ref().is_none_or(|state| state.busy());
        script.owns_input = !native;
        if !script.owns_input {
            if script.holding() {
                script.cancel_press(&mut buttons, &mut commands);
                script.stop(
                    &mut keys,
                    &mut mouse,
                    "native input reclaimed the background window",
                );
                script.retire(&mut commands);
            } else {
                script.paused = true;
                script.progress.phase = Phase::WaitingForInput;
            }
            return;
        }
        keys.restore(&script.held);
    }
    if script.current.is_none() {
        script.progress.complete_step();
        if script.follow.is_some() && script.mode == RunMode::PauseRequested {
            script.advance(now);
            return;
        }
    }
    script.progress.phase = if script.mode == RunMode::PauseRequested {
        Phase::Pausing
    } else {
        Phase::Running
    };
    if script.current.is_none() && script.exhausted() {
        if script.follow.is_none() {
            script.progress.finish(Phase::Completed, "Script finished");
            script.retire(&mut commands);
            info!("Script finished");
            return;
        }
        script.poll(now);
        if !script.progress.phase.active() {
            script.retire(&mut commands);
            return;
        }
        if script.steps.is_empty() {
            script.progress.phase = Phase::Following;
            return;
        }
    }
    if !may_run(
        script.unattended(),
        reported_focus
            .filter(|(window, _)| windows.contains(*window))
            .map(|(_, focused)| focused),
        window.is_some_and(|window| window.focused),
    ) {
        if script.holding() {
            script.cancel_press(&mut buttons, &mut commands);
            script.stop(
                &mut keys,
                &mut mouse,
                "window lost focus while holding input",
            );
        } else {
            if !script.paused {
                info!("Script paused until the client window is focused");
            }
            script.paused = true;
            script.progress.phase = Phase::WaitingForFocus;
        }
        return;
    }
    if std::mem::take(&mut script.paused) {
        script.progress.phase = Phase::Running;
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
        if matches!(
            script.current,
            Some((Step::Click(ClickTarget::Button("selector", "EQButton")), _))
        ) && let Some(MenuOpening::Clicking(target)) = script.menu_opening
        {
            // The click's pointer wait and glide have their own bounds.
            // The response budget starts only now, before this frame's
            // ordinary click/menu handlers observe the release.
            script.menu_opening = Some(MenuOpening::Released { target, since: now });
        }
        let menu_entry = script
            .current
            .as_ref()
            .is_some_and(|(step, _)| entry_target(step).is_some());
        script.release(&mut keys, &mut mouse);
        script.current = None;
        if menu_entry {
            // The entry's release reaches its handler later this frame.
            // Dispatching another entry now would see the old open menu.
            return;
        }
    }
    if let Some((step, since)) = script.current.clone() {
        let elapsed = now.saturating_sub(since);
        // Scribing and combining start server operations that may consume items.
        // The worker's preceding local moves must be submitted first;
        // successive slot/store/split clicks use the world's local projection.
        if matches!(
            step,
            Step::Click(ClickTarget::Scribe | ClickTarget::Combine(_))
        ) && online.world().inventory_moves_pending()
            && script.clicked.is_none()
        {
            if elapsed > MAX_WAIT {
                script.stop(&mut keys, &mut mouse, "queued item move did not finish");
            }
            return;
        }
        let done = match &step {
            Step::Wait(duration) => elapsed >= *duration,
            Step::DeleteCharacter { .. } => {
                let result = script
                    .deleting
                    .as_ref()
                    .ok_or_else(|| "Character deletion lost its request identity".to_owned())
                    .and_then(|request| online.deletion.observed(request));
                let result = result.and_then(|done| {
                    if !done && elapsed > MAX_ONLINE_WAIT { Err("Character deletion observation timed out; outcome is unconfirmed. Reconnect to inspect the list.".into()) }
                    else { Ok(done) }
                });
                match result {
                    Ok(false) => false,
                    Ok(true) => {
                        info!(character_deletion = %online.deletion.report(), "Character deletion observation");
                        true
                    }
                    Err(reason) => {
                        info!(character_deletion = %online.deletion.report(), "Character deletion observation");
                        script.stop(&mut keys, &mut mouse, &reason);
                        return;
                    }
                }
            }
            Step::Create(character) | Step::CreateRefused(character) => {
                let refusal_expected = matches!(step, Step::CreateRefused(_));
                match answered(
                    online.world(),
                    script.created_before,
                    &character.name,
                    refusal_expected,
                ) {
                    Ok(done) => done,
                    Err(reason) => {
                        script.stop(&mut keys, &mut mouse, &reason);
                        return;
                    }
                }
            }
            Step::WaitRelocation(limit) => {
                let world = online.world();
                let fresh = world
                    .last_same_zone_relocation()
                    .filter(|(revision, relocation)| {
                        world.in_world()
                            && Some((relocation.session_id, *revision)) != script.relocation_seen
                    });
                if let Some((revision, relocation)) = fresh {
                    script.relocation_seen = Some((relocation.session_id, revision));
                    info!(revision, position = ?relocation.position, "Script received same-zone relocation");
                    true
                } else if elapsed >= *limit {
                    script.stop(&mut keys, &mut mouse, "No same-zone relocation received");
                    return;
                } else {
                    false
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
                    if running && script.acts.is_empty() {
                        script.acts = vec![FORWARD];
                    } else if !running && script.holding() {
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
                let world = collision
                    .as_ref()
                    .and_then(|collision| collision.0.as_ref());
                let ends = body
                    .and_then(|body| walk_ends(&online, client, body, script.approached, world));
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
                    let planned = Planned::of(route.planned());
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
                        planner = planned.planner,
                        legs = planned.legs,
                        clear = planned.clear,
                        drops = planned.drops,
                        repaired = planned.repaired,
                        fallback = planned.fallback,
                        "Script route ready"
                    );
                    if let Some(eq_client_core::movement::Planned::Mesh {
                        fallback: Some(why),
                        ..
                    }) = route.planned()
                    {
                        warn!(?why, ?feet, ?goal, "Script route fell back to the grid");
                        script
                            .progress
                            .event(format!("Walk fell back to grid search: {why:?}"));
                    }
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
                        if script.acts.is_empty() {
                            script.acts = vec![FORWARD];
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
            Step::FaceHeading(heading) => {
                let at = online
                    .world()
                    .player()
                    .map(|player| player.position.heading);
                // The shortest turn between the two, in heading units.
                if at.is_some_and(|at| {
                    ((at - heading + 256.0).rem_euclid(512.0) - 256.0).abs() <= FACED
                }) {
                    true
                } else if elapsed >= FACE_LIMIT {
                    script.stop(&mut keys, &mut mouse, "face did not reach its heading");
                    return;
                } else {
                    false
                }
            }
            // Held movement keys are traced too, to measure motion cadence.
            Step::Hold(_, duration) | Step::Move(_, duration) | Step::Trace(duration) => {
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
                if !script.background_owns_input()
                    && window.is_some_and(|window| window.cursor_position().is_some())
                {
                    script.progress.phase = Phase::WaitingForPointer;
                    return;
                }
                let entity = match aim_click(
                    script,
                    (*target, now),
                    (&mut aim, &mut commands),
                    (&mut buttons, &layout, &needs),
                    online.world(),
                ) {
                    Ok(Some(entity)) => entity,
                    Ok(None) => return,
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
                keys.mark_pointer(button);
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
                    if !script.background_owns_input()
                        && window.is_some_and(|window| window.cursor_position().is_some())
                    {
                        script.progress.phase = Phase::WaitingForPointer;
                        return;
                    }
                    let entity = match aim_click(
                        script,
                        (*target, now),
                        (&mut aim, &mut commands),
                        (&mut buttons, &layout, &needs),
                        online.world(),
                    ) {
                        Ok(Some(entity)) => entity,
                        Ok(None) => return,
                        Err(reason) => {
                            script.stop(&mut keys, &mut mouse, &reason);
                            return;
                        }
                    };
                    press(&mut commands, entity, *target, &mut buttons, &mut pointers);
                    keys.mark_pointer(MouseButton::Left);
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
            Step::Hover(target) => {
                // The real pointer coming back over the window ends the glide
                // where it is, and the hover rests on the control as it would
                // without one; a glide that ends over another control stops
                // the script, as a click's does.
                let target_entity = script
                    .glide
                    .filter(|_| script.scrolling.is_none())
                    .map(|glide| glide.target)
                    .or_else(|| find(*target, &buttons, &layout));
                let inside = !script.background_owns_input()
                    && window.is_some_and(|window| window.cursor_position().is_some());
                if let Some(entity) = target_entity.filter(|_| !inside) {
                    match scroll_to(
                        script,
                        (entity, now),
                        (&mut aim, &mut commands),
                        &mut buttons,
                    ) {
                        Ok(true) => (),
                        Ok(false) => return,
                        Err(reason) => {
                            script.stop(&mut keys, &mut mouse, &reason);
                            return;
                        }
                    }
                }
                let ended_over = match target_entity {
                    Some(entity) if !inside => match glide(
                        script,
                        (entity, now),
                        (&mut aim, &mut commands),
                        &mut buttons,
                    ) {
                        Glided::Moving => return,
                        Glided::Over(over) => Some(over),
                    },
                    _ => None,
                };
                script.glide = None;
                script.scrolling = None;
                let Some(entity) = find(*target, &buttons, &layout) else {
                    script.stop(&mut keys, &mut mouse, "hover target is not visible");
                    return;
                };
                if ended_over.is_some_and(|over| over != Some(entity)) {
                    script.stop(&mut keys, &mut mouse, "the hover target is covered");
                    return;
                }
                point(script, Some(entity), &mut commands, &mut buttons);
                script.hovering = Some(entity);
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
        script.glide = None;
        script.scrolling = None;
        script.following = None;
        script.quiet = None;
        script.awaited = None;
        script.approached = None;
        script.deleting = None;
        if entry_target(&step).is_some() {
            // A held menu entry also closes the menu after this release.
            return;
        }
    }
    script.progress.complete_step();
    let single_step = matches!(script.mode, RunMode::OneStep);
    if script.mode != RunMode::PauseRequested
        && script.steps.is_empty()
        && script.language.as_ref().is_some_and(|run| run.paced(now))
    {
        return;
    }
    if !script.advance(now) {
        return;
    }
    if let Err(reason) = script.queue_statement(online.world(), now) {
        script.stop(&mut keys, &mut mouse, &reason);
        script.retire(&mut commands);
        return;
    }
    let Some(QueuedStep { step, line }) = script.steps.pop_front() else {
        return;
    };
    // An entry of the closed EQ menu: the script opens the menu with its own
    // button first, as a player would, then comes back to the entry.
    let (step, authored) = match opens_menu_first(&step, menu.open, script.menu_opening, now) {
        Ok(MenuWait::OpenFirst) => {
            script.menu_opening = entry_target(&step).map(MenuOpening::Clicking);
            script.steps.push_front(QueuedStep { step, line });
            (
                Step::Click(ClickTarget::Button("selector", "EQButton")),
                false,
            )
        }
        Ok(MenuWait::Opening) => {
            // The menu opens a frame or more after the button's release.
            script.steps.push_front(QueuedStep { step, line });
            return;
        }
        Ok(MenuWait::Go) => {
            script.menu_opening = None;
            (step, true)
        }
        Err(reason) => {
            script.stop(&mut keys, &mut mouse, &reason);
            return;
        }
    };
    if authored || script.progress.flow.is_none() {
        script.progress.begin(line);
    } else {
        script.progress.line = line;
    }
    info!(?step, "Script step");
    match &step {
        Step::DeleteCharacter { name, confirmation } => {
            match online.script_character_deletion(name, confirmation, &observed.1) {
                Ok(request) => {
                    script.deleting = Some(request);
                    info!(character_deletion = %online.deletion.report(), "Character deletion observation");
                }
                Err(reason) => {
                    script.stop(&mut keys, &mut mouse, &reason);
                    return;
                }
            }
        }
        Step::Create(character) | Step::CreateRefused(character) => {
            if online.deletion.blocked() {
                script.stop(
                    &mut keys,
                    &mut mouse,
                    "Character deletion is awaiting attention; Back / Disconnect is available",
                );
                return;
            }
            let sent = online.selection.as_ref().is_some_and(|selection| {
                let mut character = character.clone();
                if let Some(rules) = selection.can_create() {
                    character.start_zone =
                        start_zone_for(rules, &character, online.world().pvp_rule());
                }
                observed
                    .1
                    .send(
                        online.world(),
                        eq_client_core::ClientCommand::CreateCharacter {
                            selection_id: selection.id(),
                            character,
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
            if !script.local && super::chat::broadcasts_to_others(command) {
                script.stop(
                    &mut keys,
                    &mut mouse,
                    "emote and random steps require a network-enforced local test session.",
                );
                return;
            }
            if super::socials::Request::parse(command).is_some() {
                script.stop(
                    &mut keys,
                    &mut mouse,
                    "Use a named social step in a local script.",
                );
                return;
            }
            let qol = observed
                .9
                .as_ref()
                .map_or_else(Default::default, |options| options.options.qol);
            let queued = super::chat::client_request(command, &mut chat).unwrap_or_else(|| {
                super::chat::submit_game_command(command, &mut online, &observed.1, &qol)
            });
            if let Err(error) = queued {
                script.stop(&mut keys, &mut mouse, &error);
            } else if command.split_ascii_whitespace().next() == Some("/camp") {
                chat.socials.stop = true;
            }
            return;
        }
        Step::Social(command) => {
            if !script.local || !online.in_world() {
                script.stop(
                    &mut keys,
                    &mut mouse,
                    "social script steps need a local character in the world",
                );
                return;
            }
            let request = super::socials::Request::parse(command)
                .unwrap_or_else(|| Err("expected a social control request".into()))
                .and_then(|request| {
                    chat.queue_social(request, Some(script.progress.identity.clone()))
                });
            match request {
                Ok(()) => script.stepped_social = single_step,
                Err(error) => script.stop(&mut keys, &mut mouse, &error),
            }
            return;
        }
        Step::Give => {
            let world = online.world();
            let Some(target) = world.target().selected else {
                script.stop(&mut keys, &mut mouse, "give needs a target");
                return;
            };
            if world.target_stale() {
                script.stop(&mut keys, &mut mouse, "give target is no longer available");
            } else if let Err(failure) = super::give::offer(target, &mut online, &observed.1) {
                script.stop(&mut keys, &mut mouse, failure.text());
            }
            return;
        }
        Step::Interact {
            admission,
            spawn,
            revision,
        } => {
            if let Err(reason) = observed.2.script_interact(
                &mut online,
                &observed.1,
                &mut shown,
                client,
                (*admission, *spawn, *revision),
            ) {
                script.stop(&mut keys, &mut mouse, reason);
            }
            return;
        }
        Step::RespondToPopup {
            admission,
            serial,
            action,
        } => {
            if let Err(reason) =
                super::dialogs::respond(online.world(), &observed.1, *admission, *serial, *action)
            {
                script.stop(&mut keys, &mut mouse, reason);
            }
            return;
        }
        Step::InspectPlayer {
            admission,
            spawn,
            revision,
        } => {
            if let Err(reason) =
                super::dialogs::inspect(online.world(), &observed.1, *admission, *spawn, *revision)
            {
                script.stop(&mut keys, &mut mouse, reason);
            }
            return;
        }
        Step::InspectLink {
            admission,
            message,
            ordinal,
        } => {
            let result = (|| {
                if !online.in_world() || online.world().session_id() != Some(*admission) {
                    return Err("Item inspection needs the current admission");
                }
                let (_, _, link) = chat
                    .script_links(*admission)
                    .find(|(id, index, _)| id == message && index == ordinal)
                    .ok_or("Item link is unknown or no longer available")?;
                item.inspect_link(link, &online, &observed.1)
            })();
            if let Err(reason) = result {
                script.stop(&mut keys, &mut mouse, reason);
            }
            return;
        }
        Step::ComposeLink {
            admission,
            message,
            ordinal,
        } => {
            if let Err(reason) = chat.compose_link((*admission, *message, *ordinal), &online) {
                script.stop(&mut keys, &mut mouse, reason);
            }
            return;
        }
        Step::ComposeOwnedLink { admission, slot } => {
            let world = online.world();
            if !online.in_world() || world.session_id() != Some(*admission) {
                script.stop(
                    &mut keys,
                    &mut mouse,
                    "Item link composition needs the current admission",
                );
            } else if let Err(reason) = chat.compose_owned_link(*slot, &online) {
                script.stop(&mut keys, &mut mouse, &reason);
            }
            return;
        }
        Step::Gm(_) | Step::Chat(_) => {
            let command = match &step {
                Step::Gm(command) => gm_request(command, script.local, online.world()),
                Step::Chat(chat) => {
                    local_chat(chat, script.local).map(eq_client_core::ClientCommand::SendChat)
                }
                _ => return,
            };
            let sent = command.and_then(|command| {
                observed
                    .1
                    .send(online.world(), command)
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
            if online.deletion.blocked() {
                script.stop(
                    &mut keys,
                    &mut mouse,
                    "Character deletion is awaiting attention; Back / Disconnect is available",
                );
                return;
            }
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
            radius,
        } => {
            let base = if *relative {
                players
                    .single()
                    .map_or(0.0, |transform| report::placement(transform).3)
            } else {
                0.0
            };
            for mut camera in &mut cameras {
                // The view holds the pitch and distance within its own
                // ranges from the next frame, as it holds the mouse's.
                camera.reset_view();
                camera.yaw = super::behind(eq_client_core::render_heading(base + *heading));
                camera.pitch = pitch.to_radians();
                if let Some(radius) = radius {
                    camera.radius = *radius;
                }
            }
            return;
        }
        Step::FaceHeading(heading) => script.facing = Some(*heading),
        Step::Move(command, _) => {
            script.acts = vec![match command {
                MoveCommand::Forward => Act::Forward,
                MoveCommand::Back => Act::Back,
                MoveCommand::CameraForward => Act::CameraForward,
                MoveCommand::CameraBack => Act::CameraBack,
                MoveCommand::StrafeLeft => Act::StrafeLeft,
                MoveCommand::StrafeRight => Act::StrafeRight,
                MoveCommand::TurnLeft => Act::TurnLeft,
                MoveCommand::TurnRight => Act::TurnRight,
            }];
        }
        Step::Click(_) | Step::RightClick(_) | Step::HoldClick(..) => {
            // The script pointer leaves the control a hover rested on.
            script.pointed = script.hovering.take().or(script.pointed);
            if !script.background_owns_input()
                && window.is_some_and(|window| window.cursor_position().is_some())
            {
                info!("Scripted click waits until the pointer leaves the client window");
            }
        }
        Step::Hover(target) => {
            let Some(entity) = find(*target, &buttons, &layout) else {
                script.stop(&mut keys, &mut mouse, "hover target is not visible");
                return;
            };
            if !script.background_owns_input()
                && window.is_some_and(|window| window.cursor_position().is_some())
            {
                // With the real pointer over the window there is no script
                // pointer to glide: the hover rests on the control at once,
                // even one just clicked.
                commands
                    .entity(entity)
                    .remove::<super::tooltip::ScriptPressed>();
                script.hovering = Some(entity);
                return;
            }
            // The script pointer glides there first, leaving the control
            // the last hover rested on.
            script.pointed = script.hovering.take().or(script.pointed);
        }
        Step::Report(label) => {
            report::state(
                label,
                &online,
                &observed,
                players.single().ok(),
                cameras.single().ok(),
            );
            report::movement_state(
                label,
                &online,
                players.single().ok(),
                bodies.single().ok(),
                collision.as_deref().and_then(|world| world.0.as_ref()),
                &observed,
                detached_camera.single().ok(),
            );
            report::surroundings(&online, &observed);
            report::game_messages(&mut script.chat_seen, &chat);
            report::item_links(label, &online, &chat, &item);
            report::dialogs(label, online.world());
            if let Some(flights) = observed.12.as_ref() {
                flights.report(label, online.world().session_id());
            }
            if let Some(socials) = observed.13.as_ref() {
                socials.report(label);
            }
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
            script.progress.phase = Phase::Stopped;
            script.retire(&mut commands);
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
            script.acts = vec![FORWARD];
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
            script.acts = vec![FORWARD];
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
                let floors = collision
                    .as_ref()
                    .and_then(|collision| collision.0.as_ref());
                let (feet, goal) = walk_ends(&online, client, *body, script.approached, floors)?;
                // Ledges are routes only where the session simulates falls.
                let route = eq_client_core::movement::Route::new(feet, goal, *range, body.height)
                    .with_drops(observed.4.airborne.is_some())
                    .with_doors(openable_doors(&online))
                    .with_background(std::sync::Arc::new(super::background::ComputePool));
                // With the zone's mesh ready for this body's size, the mesh
                // plans; else the grid, and the walk says why.
                let mesh = observed
                    .7
                    .as_ref()
                    .map_or(Err(super::zone_navmesh::GridReason::Missing), |meshes| {
                        meshes.for_walker(body.mesh_size())
                    });
                Some(match mesh {
                    Ok(mesh) => (
                        route.with_navigator(Box::new(
                            eq_client_core::movement::navmesh::MeshNavigator::new(mesh),
                        )),
                        None,
                    ),
                    Err(reason) => (route, Some(reason)),
                })
            });
            let creating = creating.elapsed();
            if creating >= SLOW_ROUTE_CALL {
                info!(
                    ms = creating.as_secs_f32() * 1000.0,
                    "Script walk route made slowly"
                );
            }
            let Some((route, grid)) = route else {
                script.stop(&mut keys, &mut mouse, "walk needs a visible target");
                return;
            };
            if let Some(reason) = grid {
                let reason = reason.describe();
                warn!(
                    reason,
                    "Script walk planning with grid search, not the navigation mesh"
                );
                script
                    .progress
                    .event(format!("Walk uses grid search: {reason}"));
            }
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
        | Step::WaitRelocation(_)
        | Step::Wait(_)
        | Step::Trace(_) => (),
    }
    script.current = Some((step, now));
}

/// A semantic menu entry's local opener lifecycle. The release is a local
/// input fact; only a later observation of `Menu::open` establishes opening.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuOpening {
    /// The guarded opener click is waiting, gliding or pressed.
    Clicking(ClickTarget),
    /// The opener's local button was released; its handler still runs later.
    Released {
        target: ClickTarget,
        since: Duration,
    },
}

/// What a step does about the EQ menu before it runs.
#[derive(Debug, PartialEq, Eq)]
enum MenuWait {
    /// The step runs as it is.
    Go,
    /// The step names an entry of the closed menu: the menu's button is
    /// clicked first.
    OpenFirst,
    /// The opener is in progress, or released but the menu has not opened yet.
    Opening,
}

/// How long an entry waits for the EQ menu to open after its button was
/// released before the script ends.
const MENU_OPEN_WAIT: Duration = Duration::from_secs(1);

/// Whether a click step's target is an entry of the EQ menu that a pointer
/// cannot reach while the menu is closed (`click window <key>`, `click
/// actions`), so the menu's own button is clicked first. The opener's own
/// pointer/focus/coverage rules still apply. Its queued time is not response
/// time: the menu opens a frame or more after the button's release, so the
/// matching entry waits from that local release up to
/// [`MENU_OPEN_WAIT`] and the script ends only when the menu stays closed.
fn opens_menu_first(
    step: &Step,
    menu_open: bool,
    opening: Option<MenuOpening>,
    now: Duration,
) -> Result<MenuWait, String> {
    let Some(target) = entry_target(step) else {
        return Ok(MenuWait::Go);
    };
    if menu_open {
        return Ok(MenuWait::Go);
    }
    match opening {
        Some(MenuOpening::Clicking(for_entry)) if for_entry == target => Ok(MenuWait::Opening),
        Some(MenuOpening::Released {
            target: for_entry,
            since,
        }) if for_entry == target => {
            if now.saturating_sub(since) < MENU_OPEN_WAIT {
                Ok(MenuWait::Opening)
            } else {
                Err("the EQ menu did not open for the click".into())
            }
        }
        _ => Ok(MenuWait::OpenFirst),
    }
}

/// The EQ menu entry a click step names, if any.
fn entry_target(step: &Step) -> Option<ClickTarget> {
    match step {
        Step::Click(target) | Step::RightClick(target) | Step::HoldClick(target, _)
            if matches!(target, ClickTarget::Toggle(_) | ClickTarget::ActionsWindow) =>
        {
            Some(*target)
        }
        _ => None,
    }
}

/// Whether the world answered a `create` step's request since it was sent:
/// done once it created the character, and the step's failure when it
/// refused it.
/// The value this selection's generation sends for the script's starting
/// city. Scripts name the city by its Titanium choice number; a generation
/// that sends its own value for the same city (the Mac-era start-zone IDs)
/// gets that value, found through the city's name in the generation's own
/// list for this race, class and deity. A number naming no city, or a city
/// the generation does not offer this combination, goes out as written, so
/// the server's refusal is what the script sees.
fn start_zone_for(
    rules: eq_client_core::creation::Rules,
    character: &eq_client_core::creation::NewCharacter,
    pvp: u32,
) -> u32 {
    let (race, class, deity) = (character.race, character.class, character.deity);
    let named = eq_client_core::creation::Rules::Titanium
        .cities(race, class, deity, pvp)
        .into_iter()
        .find(|city| city.start_zone == character.start_zone);
    named
        .and_then(|wanted| {
            rules
                .cities(race, class, deity, pvp)
                .into_iter()
                .find(|city| city.name == wanted.name)
        })
        .map_or(character.start_zone, |city| city.start_zone)
}

fn answered(
    world: &eq_client_core::world::ClientWorld,
    before: Option<u64>,
    name: &str,
    refusal_expected: bool,
) -> Result<bool, String> {
    use eq_client_core::creation::CreationOutcome;
    let Some(answer) = world
        .creation()
        .filter(|answer| Some(answer.count) != before && answer.name == name)
    else {
        return Ok(false);
    };
    match (answer.outcome, refusal_expected) {
        (CreationOutcome::Created, false) => Ok(true),
        (CreationOutcome::Created, true) => Err(format!(
            "character {name} was created where a refusal was expected"
        )),
        (refused, false) => Err(format!("character creation refused: {refused:?}")),
        (refused, true) => {
            info!(?refused, name, "Script saw the expected creation refusal");
            Ok(true)
        }
    }
}

/// Steps run only while someone watches the window, unless the session is
/// local-only or there is none. A window counts as focused before winit
/// reports anything (and a hidden one never gets a report), so only a
/// reported focus counts.
fn may_run(unattended: bool, reported_focus: Option<bool>, window_focused: bool) -> bool {
    unattended || (reported_focus == Some(true) && window_focused)
}

/// The request for a validated local GM command. Fixture cleanup carries
/// the admission and reconciled NPC selection to the zone's send owner.
fn gm_request(
    command: &str,
    allowed: bool,
    world: &eq_client_core::world::ClientWorld,
) -> Result<eq_client_core::ClientCommand, String> {
    use eq_client_core::{ClientCommand, OutboundChat, SpawnKind};

    if !allowed {
        return Err(
            "gm steps only run on a local EQEmu or TAKP server (EQ_PROTOCOL=eqemu or takp)".into(),
        );
    }
    let words: Vec<_> = command.split_ascii_whitespace().collect();
    let command = parse::gm_command(&words)?;
    let depop = command == "depop";
    let full_spawn = words.first() == Some(&"spawn") && words.len() > 5;
    let titanium = world.generation() == eq_client_core::GameDialect::Titanium;
    if (depop || full_spawn) && !titanium {
        return Err("Full fixture spawn and depop require local EQEmu".into());
    }
    if words.first() == Some(&"traindisc") && !titanium {
        return Err("gm traindisc requires local EQEmu".into());
    }
    if depop {
        let Some((session_id, target)) =
            world
                .session_id()
                .zip(world.target_intent())
                .filter(|(_, target)| {
                    target
                        .spawn_id
                        .and_then(|id| world.spawn(id))
                        .is_some_and(|spawn| spawn.state.kind == SpawnKind::Npc)
                })
        else {
            return Err("gm depop needs a current NPC target".into());
        };
        return Ok(ClientCommand::DepopLocalNpc {
            session_id,
            target,
            created: std::time::Instant::now(),
        });
    }
    Ok(ClientCommand::SendChat(OutboundChat::Say(format!(
        "#{command}"
    ))))
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
/// visible, or a named skin button or slider is greyed out, with the reason
/// its hover gives.
fn reach(
    target: ClickTarget,
    buttons: &Buttons,
    layout: &Layout,
    needs: &Query<&crate::outbox::Needs>,
    world: &eq_client_core::world::ClientWorld,
) -> Result<Entity, String> {
    let entity = find(target, buttons, layout).ok_or("click target is not visible")?;
    match (target, needs.get(entity)) {
        (ClickTarget::Button(..), Ok(needs)) if !needs.offered(world) => {
            Err(format!("that button is greyed out: {}", needs.reason()))
        }
        (ClickTarget::Slider(..), Ok(needs)) if !needs.offered(world) => {
            Err(format!("that slider is greyed out: {}", needs.reason()))
        }
        _ => Ok(entity),
    }
}

/// The frontmost visible matching control in an open window. A semantic
/// action can have more than one button, such as the spellbook entry in
/// the EQ menu and the spellbook button beside the gems. Query iteration
/// order must not send the pointer to a copy under the menu's dismiss shield.
fn find(target: ClickTarget, buttons: &Buttons, layout: &Layout) -> Option<Entity> {
    let mut found = None;
    for (
        entity,
        _,
        visibility,
        slot,
        scribe,
        (store, destroy, drop),
        (row, place, turns, spell_gem, hot_button),
        gem,
        trade,
        tint,
        (give, character, chat_tab),
        (coins, pick, arrow, amount),
        (selector, tab, ability, attack, slash, checkbox, join),
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
            ClickTarget::GroupJoin => join,
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
            ClickTarget::Destroy => destroy == Some(&super::inventory::destruction::Action::Select),
            ClickTarget::Drop => drop,
            ClickTarget::BookRow(number) => row.is_some_and(|row| row.0 == number),
            ClickTarget::BookPlace(number) => place.is_some_and(|place| place.0 == number),
            ClickTarget::BookPage(forward) => turns.is_some_and(|turns| turns.0 == forward),
            ClickTarget::SpellGem(number) => spell_gem.is_some_and(|gem| gem.0 == number),
            ClickTarget::HotButton(number) => hot_button.is_some_and(|slot| slot.0 == number),
            ClickTarget::MemorizeGem(number) => gem.is_some_and(|gem| gem.0 == number),
            ClickTarget::Trade(click) => trade == Some(&trading(click)),
            ClickTarget::Tint(number, color) => tint_control(tint, number, color),
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
            let stack = layout.get(entity).map_or(0, |(.., z)| z.map_or(0, |z| z.0));
            if found.is_none_or(|(front, _)| stack > front) {
                found = Some((stack, entity));
            }
        }
    }
    found.map(|(_, entity)| entity)
}

fn tint_control(
    action: Option<&super::inventory::colors::Action>,
    number: i32,
    color: Option<usize>,
) -> bool {
    use super::inventory::colors::Action;
    action.is_some_and(|action| match (action, color) {
        (Action::Toggle(slot), None) => slot.0 == number,
        (Action::Set(slot, index), Some(color)) => slot.0 == number && *index == color,
        _ => false,
    })
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

/// Where a glide of the script pointer stands after a frame.
enum Glided {
    /// It is still on its way.
    Moving,
    /// It is over, with the control the pointer ended over, if any.
    Over(Option<Entity>),
}

/// One frame of the script pointer's glide to the middle of `target`, begun
/// on the first from where the pointer rests: it moves the pointer and
/// gives the control under it the hover a `hover` step gives.
fn glide(
    script: &mut Script,
    (target, now): (Entity, Duration),
    (aim, commands): (&mut pointer::Aim, &mut Commands),
    buttons: &mut Buttons,
) -> Glided {
    let glide = *script.glide.get_or_insert_with(|| pointer::Glide {
        from: aim.at(),
        began: now,
        target,
        at: None,
    });
    let elapsed = now.saturating_sub(glide.began);
    // A control that moves as the pointer heads for it is followed.
    let to = glide
        .at
        .or_else(|| aim.middle(glide.target))
        .unwrap_or(glide.from);
    let at = pointer::glide_at(glide.from, to, elapsed);
    aim.place(commands, at);
    let over = aim.under(at);
    point(script, over, commands, buttons);
    if elapsed < pointer::GLIDE {
        return Glided::Moving;
    }
    script.glide = None;
    Glided::Over(over)
}

/// One frame of scrolling `target` into view in the nearest list above it
/// that the wheel scrolls, as a player scrolls a list before clicking a row
/// it hides: true once the control shows whole in the list, at once when it
/// already does or lies in no such list. The script pointer first glides
/// just inside the list's edge nearest the control, then the wheel turns a
/// notch a frame toward it; a notch that leaves the list where it was, with
/// the control still not whole in view, is why the script stops.
fn scroll_to(
    script: &mut Script,
    (target, now): (Entity, Duration),
    (aim, commands): (&mut pointer::Aim, &mut Commands),
    buttons: &mut Buttons,
) -> Result<bool, String> {
    let Some(list) = script
        .scrolling
        .map(|scrolling| scrolling.list)
        .or_else(|| aim.wheel_list(target))
    else {
        return Ok(true);
    };
    let (Some(row), Some(view)) = (aim.rect(target), aim.rect(list)) else {
        return Ok(true);
    };
    // The wheel scrolls up and down, so only how high the control lies counts.
    let shows = row.min.y >= view.min.y - 0.5 && row.max.y <= view.max.y + 0.5;
    let up = row.min.y < view.min.y;
    if script.scrolling.is_none() {
        if shows {
            return Ok(true);
        }
        let x = row
            .center()
            .x
            .min(view.max.x - pointer::EDGE)
            .max(view.min.x + pointer::EDGE);
        let y = if up {
            view.min.y + pointer::EDGE
        } else {
            view.max.y - pointer::EDGE
        };
        script.scrolling = Some(pointer::Scrolling { list, before: None });
        script.glide = Some(pointer::Glide {
            from: aim.at(),
            began: now,
            target: list,
            at: Some(Vec2::new(x, y)),
        });
    }
    if script.glide.is_some()
        && matches!(
            glide(script, (list, now), (aim, commands), buttons),
            Glided::Moving
        )
    {
        return Ok(false);
    }
    if shows {
        script.scrolling = None;
        return Ok(true);
    }
    let scrolled = aim.scrolled(list);
    let Some(scrolling) = script.scrolling.as_mut() else {
        return Ok(true);
    };
    if scrolling
        .before
        .is_some_and(|before| before.to_bits() == scrolled.to_bits())
    {
        return Err("the target can't be scrolled into view".to_owned());
    }
    scrolling.before = Some(scrolled);
    let pixels = crate::windows::pointer::LINE_PIXELS;
    script.notch = Some((list, if up { pixels } else { -pixels }));
    Ok(false)
}

/// Rests the script pointer's hover on `over`, as a `hover` step rests it,
/// and clears the hover of the control it leaves.
fn point(
    script: &mut Script,
    over: Option<Entity>,
    commands: &mut Commands,
    buttons: &mut Buttons,
) {
    if script.pointed != over {
        if let Some(left) = script.pointed
            && let Ok((_, mut interaction, ..)) = buttons.get_mut(left)
            && *interaction == Interaction::Hovered
        {
            *interaction = Interaction::None;
        }
        if let Some(entered) = over {
            commands
                .entity(entered)
                .remove::<super::tooltip::ScriptPressed>();
        }
        script.pointed = over;
    }
    if let Some(over) = over
        && let Ok((_, mut interaction, ..)) = buttons.get_mut(over)
    {
        interaction.set_if_neq(Interaction::Hovered);
    }
}

/// A click's aim this frame: none while the control the click names is
/// scrolled into view (see [`scroll_to`]) and the script pointer glides to
/// it, then that control, once the pointer has ended over it; or why the
/// script stops: the control is out of reach as [`reach`] says, cannot be
/// scrolled into view, or another control covers it where the pointer ends.
fn aim_click(
    script: &mut Script,
    (target, now): (ClickTarget, Duration),
    (aim, commands): (&mut pointer::Aim, &mut Commands),
    (buttons, layout, needs): (&mut Buttons, &Layout, &Query<&crate::outbox::Needs>),
    world: &eq_client_core::world::ClientWorld,
) -> Result<Option<Entity>, String> {
    let heading = if let Some(glide) = script.glide.filter(|_| script.scrolling.is_none()) {
        glide.target
    } else {
        let entity = reach(target, buttons, layout, needs, world)?;
        if !scroll_to(script, (entity, now), (&mut *aim, &mut *commands), buttons)? {
            return Ok(None);
        }
        entity
    };
    let Glided::Over(over) = glide(script, (heading, now), (&mut *aim, &mut *commands), buttons)
    else {
        return Ok(None);
    };
    // The click lands as it did without a pointer to glide, on the control
    // found as it lands, only if the pointer is over it.
    let entity = reach(target, buttons, layout, needs, world)?;
    if over == Some(entity) {
        Ok(Some(entity))
    } else {
        aim.report_cover(entity);
        Err("the click target is covered".to_owned())
    }
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
    while let Some((node, parent, _)) = next.and_then(|entity| layout.get(entity).ok()) {
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

/// What planned a route, as the route trace line names it.
struct Planned {
    planner: &'static str,
    legs: usize,
    clear: usize,
    drops: usize,
    repaired: usize,
    fallback: &'static str,
}

impl Planned {
    fn of(planned: Option<&eq_client_core::movement::Planned>) -> Self {
        use eq_client_core::movement::{Fallback, Planned as By};
        match planned {
            Some(By::Mesh {
                legs,
                clear,
                drops,
                repaired,
                fallback,
                ..
            }) => Self {
                planner: "mesh",
                legs: *legs,
                clear: *clear,
                drops: *drops,
                repaired: repaired.len(),
                fallback: match fallback {
                    None => "none",
                    Some(Fallback::OffMesh) => "off_mesh",
                    Some(Fallback::OtherIsland) => "other_island",
                    Some(Fallback::NoCorridor) => "no_corridor",
                    Some(Fallback::Leg { .. }) => "leg",
                },
            },
            Some(By::Grid) | None => Self {
                planner: "grid",
                legs: 0,
                clear: 0,
                drops: 0,
                repaired: 0,
                fallback: "none",
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
        .filter(|door| online.world().doors().supports_use(door.id) && super::doors::swings(door))
        .map(|door| door.id)
        .collect()
}

/// The player's feet at its accepted position and the goal's position, in
/// render coordinates: the world object `object` names, or else the
/// target's soles, as [`spawn_goal`] finds them on `floors`.
fn walk_ends(
    online: &super::online::OnlineState,
    client: eq_client_assets::ui::InstalledClient,
    body: super::PlayerBody,
    object: Option<u32>,
    floors: Option<&eq_client_core::movement::CollisionWorld>,
) -> Option<(Vec3, Vec3)> {
    let world = online.world();
    let player = world.player()?;
    let goal = if let Some(drop_id) = object {
        Vec3::from_array(eq_client_core::render_position(
            world.objects().entries().get(&drop_id)?.position,
        ))
    } else {
        let target = &world
            .target()
            .selected
            .and_then(|id| world.spawn(id))?
            .state;
        spawn_goal(
            floors,
            Vec3::from_array(eq_client_core::render_position(target.position)),
            super::entities::Drawn::of(client, world, (target.race, target.gender, target.size)),
        )
    };
    let origin = Vec3::from_array(eq_client_core::render_position(player.position));
    Some((origin - Vec3::Y * body.feet_offset, goal))
}

/// Where a walk to a spawn reported at `reported` (its body's middle, in
/// render coordinates) aims: its soles, where the client draws a standing
/// one, by [`eq_client_core::drawn_feet_offset`] and then onto the floor of
/// `floors` by [`eq_client_core::grounded_soles`]. Inferred, as those are:
/// item 6.231.
fn spawn_goal(
    floors: Option<&eq_client_core::movement::CollisionWorld>,
    reported: Vec3,
    drawn: super::entities::Drawn,
) -> Vec3 {
    let soles = reported - Vec3::Y * drawn.feet();
    floors.map_or(soles, |floors| {
        Vec3::new(
            soles.x,
            eq_client_core::grounded_soles(floors, soles, drawn.size),
            soles.z,
        )
    })
}

/// The zone's nearest world container of a tradeskill kind to the player.
fn nearest_station(online: &super::online::OnlineState, kind: u8) -> Option<u32> {
    let world = online.world();
    if !crate::outbox::offered(world, eq_client_core::Capability::WorldContainers) {
        return None;
    }
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
        // The character list once the player is out of the world: a camp
        // waits for its logout, not the list left from before.
        Step::WaitSelect => online.selection.is_some() && world.player().is_none(),
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
/// target is drawn: its last report carried on as the generation's program
/// carries it, from when that report was first seen.
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
    Ok(Chase::Aim(eq_client_core::entities::carried(
        world.generation(),
        spawn,
        elapsed.saturating_sub(since),
    )))
}

/// Whether a chase moves forward: only while its target is beyond the distance.
/// Within it the player stands, auto-attack doing the rest.
fn runs(distance: Option<f32>, range: f32) -> bool {
    distance.is_some_and(|distance| distance > range)
}

#[cfg(test)]
mod tests {
    mod background_pointer;
    mod semantic;
    use super::*;
    use eq_client_assets::ui::InstalledClient;

    #[test]
    fn station_steps_require_the_station_offer_even_when_carried_combines_are_offered() {
        use eq_client_core::{
            Capability, WorldEvent,
            ground::{GroundObject, ObjectUpdate},
        };
        let mut online = crate::online::OnlineState::new(true);
        let player = crate::online::testing::player(7);
        crate::online::testing::admit(&mut online, 1, player.clone());
        crate::online::testing::news(
            &mut online,
            [WorldEvent::Entered {
                capabilities: vec![Capability::Tradeskills],
                choices: Vec::new(),
                session_id: 2,
                zone: "qeytoqrg".into(),
                player: Box::new(player),
                far_clip: None,
                spell_landing_flag: None,
            }],
        );
        crate::online::testing::objects(
            &mut online,
            &ObjectUpdate::Spawn(GroundObject {
                drop_id: 72,
                model: "FORGE".into(),
                object_type: 17,
                position: default(),
            }),
        );
        assert!(online.world().can(Capability::Tradeskills));
        assert_eq!(nearest_station(&online, 17), None);
    }

    /// Scripts name the starting city by its Titanium number: Titanium sends
    /// it as written, the Mac-era generation sends its own value for the same
    /// city, and a number naming no city goes out unchanged.
    #[test]
    fn a_scripts_starting_city_is_sent_as_the_generation_sends_it() {
        use eq_client_core::creation::{NewCharacter, Rules};
        // A human paladin of Rodcet Nife, from Qeynos (Titanium city 1).
        let qeynos = |start_zone| {
            NewCharacter::with_points_in("Scripted", (1, 3, 0), (212, start_zone), 0).unwrap()
        };
        assert_eq!(start_zone_for(Rules::Titanium, &qeynos(1), 0), 1);
        let mac = Rules::EqMac
            .cities(1, 3, 212, 0)
            .into_iter()
            .find(|city| city.name == "Qeynos")
            .expect("the Mac-era rules offer Qeynos to this paladin")
            .start_zone;
        assert_ne!(mac, 1);
        assert_eq!(start_zone_for(Rules::EqMac, &qeynos(1), 0), mac);
        assert_eq!(start_zone_for(Rules::EqMac, &qeynos(77), 0), 77);
    }

    /// A flat floor at height 0, 40 across, as a collision world and its
    /// triangles.
    fn flat_floor() -> (eq_client_core::movement::CollisionWorld, Vec<[[f32; 3]; 3]>) {
        let (a, b, c, d) = (
            [-20.0, 0.0, -20.0],
            [-20.0, 0.0, 20.0],
            [20.0, 0.0, 20.0],
            [20.0, 0.0, -20.0],
        );
        let triangles = vec![[a, b, c], [a, c, d]];
        let world =
            eq_client_core::movement::CollisionWorld::new(triangles.iter().copied()).unwrap();
        (world, triangles)
    }

    /// A human of size 6, as both clients draw one.
    const HUMAN: crate::entities::Drawn = crate::entities::Drawn {
        size: 6.0,
        scale: 1.2,
    };

    #[test]
    fn a_walk_to_a_target_aims_at_its_soles_on_the_floor_not_its_middle() {
        use eq_client_core::movement::navmesh::{NavMesh, Settings};
        use eq_client_core::movement::{Route, RouteStep};
        let (world, triangles) = flat_floor();
        // A human of size 6, reported at its middle: 0.625 x 6 over its
        // soles; and one the server reports a little high.
        let offset = HUMAN.feet();
        for over in [0.0, 0.3] {
            let reported = Vec3::new(5.0, offset + over, 2.0);
            let goal = spawn_goal(Some(&world), reported, HUMAN);
            assert!(goal.y.abs() < 1e-3, "{over}: {goal}");
            assert_eq!((goal.x, goal.z), (5.0, 2.0));
        }
        // The mesh finds floor under the soles and none within its reach of
        // the middle.
        let reported = Vec3::new(5.0, offset, 2.0);
        let goal = spawn_goal(Some(&world), reported, HUMAN);
        let (mesh, _) = NavMesh::build(
            &triangles,
            &Settings::walker(6.0),
            2,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
        assert!(mesh.locate(goal).is_some());
        assert!(mesh.locate(reported).is_none());
        // Feet on that floor within reach across have arrived at it.
        let feet = Vec3::new(5.5, 0.0, 2.0);
        let mut route = Route::new(feet, goal, 1.0, 6.0);
        assert_eq!(
            route.next(&world, feet, goal, 100, Duration::ZERO),
            RouteStep::Arrived
        );
    }

    #[test]
    fn with_no_floors_a_target_s_goal_is_its_drawn_soles() {
        let reported = Vec3::new(1.0, 10.0, -3.0);
        let goal = spawn_goal(None, reported, HUMAN);
        let lowered = reported.y - HUMAN.feet();
        assert!((goal.y - lowered).abs() < 1e-6);
        assert_eq!((goal.x, goal.z), (1.0, -3.0));
    }

    #[test]
    fn a_walk_aims_at_a_target_s_soles_and_at_an_object_s_base() {
        use eq_client_core::{WorldEvent, world_position};
        let (world, _) = flat_floor();
        let mut online = super::super::online::OnlineState::new(true);
        super::super::online::testing::admit(
            &mut online,
            1,
            super::super::online::testing::player(1),
        );
        let offset = HUMAN.feet();
        let mut npc = super::super::online::testing::pet(9, 0);
        npc.pet_owner = None;
        npc.race = 1;
        npc.size = 6.0;
        npc.position = world_position([4.0, offset, 1.0], 0.0);
        super::super::online::testing::news(&mut online, [WorldEvent::Spawns(vec![npc])]);
        online.select_target(Some(9));
        let body = super::super::PlayerBody {
            feet_offset: 3.0,
            height: 6.0,
            size: 6.0,
        };
        let (_, goal) =
            walk_ends(&online, InstalledClient::Titanium, body, None, Some(&world)).unwrap();
        assert!(goal.y.abs() < 1e-3, "{goal}");
        assert!(
            (goal.x - 4.0).abs() < 1e-3 && (goal.z - 1.0).abs() < 1e-3,
            "{goal}"
        );
        // An object's position is its base: the walk aims there unchanged.
        let base = world_position([-6.0, 0.4, 3.0], 0.0);
        super::super::online::testing::news(
            &mut online,
            [WorldEvent::Objects(
                eq_client_core::ground::ObjectUpdate::Spawn(eq_client_core::ground::GroundObject {
                    drop_id: 40,
                    model: "IT63_ACTORDEF".into(),
                    position: base,
                    object_type: 15,
                }),
            )],
        );
        let (_, goal) = walk_ends(
            &online,
            InstalledClient::Titanium,
            body,
            Some(40),
            Some(&world),
        )
        .unwrap();
        assert_eq!(
            goal,
            Vec3::from_array(eq_client_core::render_position(base))
        );
    }

    #[test]
    fn a_menu_entry_click_opens_the_closed_menu_first_and_waits_for_it_once() {
        let entry = Step::Click(ClickTarget::Toggle("inventory"));
        let actions = Step::HoldClick(ClickTarget::ActionsWindow, Duration::from_millis(300));
        let button = Step::Click(ClickTarget::Button("selector", "EQButton"));
        let released_at = Duration::from_secs(40);
        for step in [&entry, &actions] {
            let target = entry_target(step).unwrap();
            let clicking = Some(MenuOpening::Clicking(target));
            let released = Some(MenuOpening::Released {
                target,
                since: released_at,
            });
            assert_eq!(
                opens_menu_first(step, false, None, Duration::ZERO),
                Ok(MenuWait::OpenFirst)
            );
            // Queuing or waiting for the pointer never spends response time.
            assert_eq!(
                opens_menu_first(step, false, clicking, released_at),
                Ok(MenuWait::Opening)
            );
            assert_eq!(
                opens_menu_first(step, false, released, released_at),
                Ok(MenuWait::Opening)
            );
            assert_eq!(
                opens_menu_first(
                    step,
                    false,
                    released,
                    released_at + Duration::from_millis(999)
                ),
                Ok(MenuWait::Opening)
            );
            assert_eq!(
                opens_menu_first(step, false, released, released_at + MENU_OPEN_WAIT),
                Err("the EQ menu did not open for the click".into())
            );
            assert_eq!(
                opens_menu_first(step, true, released, released_at + MENU_OPEN_WAIT),
                Ok(MenuWait::Go)
            );
        }
        // A different entry does not inherit another entry's old deadline.
        let another = Some(MenuOpening::Released {
            target: ClickTarget::ActionsWindow,
            since: Duration::ZERO,
        });
        assert_eq!(
            opens_menu_first(&entry, false, another, released_at),
            Ok(MenuWait::OpenFirst)
        );
        assert_eq!(
            opens_menu_first(&button, false, None, released_at),
            Ok(MenuWait::Go)
        );
        assert_eq!(
            opens_menu_first(&Step::Wait(Duration::ZERO), false, another, released_at),
            Ok(MenuWait::Go)
        );
    }

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
        assert_eq!(answered(&world, None, "Testcleric", false), Ok(false));
        answer(&mut world, "Testcleric", CreationOutcome::CreationRefused);
        assert_eq!(
            answered(&world, None, "Testcleric", false),
            Err("character creation refused: CreationRefused".into())
        );
        // A `create_refused` step passes on that same refusal.
        assert_eq!(answered(&world, None, "Testcleric", true), Ok(true));
        // A later step waits past the answer it saw sent, and past another
        // name's, then finishes on its own character's creation.
        let before = world.creation().map(|last| last.count);
        assert_eq!(answered(&world, before, "Testcleric", false), Ok(false));
        answer(&mut world, "Othername", CreationOutcome::Created);
        assert_eq!(answered(&world, before, "Testcleric", false), Ok(false));
        answer(&mut world, "Testcleric", CreationOutcome::Created);
        assert_eq!(answered(&world, before, "Testcleric", false), Ok(true));
        // A `create_refused` step fails when the character is created.
        assert_eq!(
            answered(&world, before, "Testcleric", true),
            Err("character Testcleric was created where a refusal was expected".into())
        );
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
            heading: 128.0,
        };
        gnoll.velocity = [0.0; 3];
        gnoll.forward = 0.5;
        super::super::online::testing::news(&mut online, [WorldEvent::Spawns(vec![gnoll.clone()])]);
        let limit = at(90_000);
        let mut seen = None;
        let aim = |chase: Result<Chase, &'static str>| match chase {
            Ok(Chase::Aim(aim)) => aim.x,
            other => panic!("expected an aim, got {other:?}"),
        };
        // The aim carries the last report on as the generation's program
        // does, from when it was first seen: an NPC with an input of 0.5,
        // facing +X, at 11.64 a second once it settles.
        let world = online.world();
        let expected = eq_client_core::entities::carried(world.generation(), &gnoll, at(500)).x;
        assert!((aim(chase(world, Some(9), &mut seen, at(1_000), limit)) - 10.0).abs() < 1e-3);
        let half = aim(chase(world, Some(9), &mut seen, at(1_500), limit));
        assert!((half - expected).abs() < 1e-3 && half > 14.5, "{half}");
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
        use eq_client_core::{ClientCommand, OutboundChat};
        let world = eq_client_core::world::ClientWorld::default();
        assert!(gm_request("summon", false, &world).is_err());
        assert_eq!(
            gm_request("givemoney 0 0 5 0", true, &world).unwrap(),
            ClientCommand::SendChat(OutboundChat::Say("#givemoney 0 0 5 0".into()))
        );
        assert!(!Script::new(vec![Step::Gm("summon".into())]).local);
        assert!(Script::new(Vec::new()).local_session(true).local);
        // Offline, steps run unattended but a gm step is still refused.
        let offline = Script::new(Vec::new()).offline_preview(true);
        assert!(offline.offline && !offline.local);
    }

    #[test]
    fn a_stun_fixture_keeps_both_local_session_gates() {
        use eq_client_core::{ClientCommand, OutboundChat};
        let world = eq_client_core::world::ClientWorld::default();
        let program = Program::parse("gm stun 8000", std::path::Path::new(".")).unwrap();
        assert_eq!(
            program.instructions()[0].step(),
            &Step::Gm("stun 8000".into())
        );
        assert!(program.check_local_commands(false).is_err());
        assert!(program.check_local_commands(true).is_ok());
        assert!(gm_request("stun 8000", false, &world).is_err());
        assert_eq!(
            gm_request("stun 8000", true, &world).unwrap(),
            ClientCommand::SendChat(OutboundChat::Say("#stun 8000".into()))
        );
    }

    #[test]
    fn discipline_fixture_keeps_the_local_eqemu_policy() {
        use crate::online::{OnlineState, testing as session};
        use eq_client_core::{ClientCommand, GameDialect, OutboundChat};

        let command = "traindisc 55 52";
        let program = Program::parse(&format!("gm {command}"), std::path::Path::new(".")).unwrap();
        assert!(program.check_local_commands(false).is_err());
        assert!(program.check_local_commands(true).is_ok());

        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(7));
        assert!(gm_request(command, false, online.world()).is_err());
        assert!(gm_request("traindisc 52 55", true, online.world()).is_err());
        assert_eq!(
            gm_request(command, true, online.world()),
            Ok(ClientCommand::SendChat(OutboundChat::Say(format!(
                "#{command}"
            ))))
        );
        session::generation(&mut online, GameDialect::EqMac);
        assert!(gm_request(command, true, online.world()).is_err());
    }

    #[test]
    fn full_fixture_setup_requires_local_titanium_at_runtime() {
        use crate::online::{OnlineState, testing as session};
        use eq_client_core::{ClientCommand, GameDialect, OutboundChat};

        let command = "spawn ScriptFixture 60 1 0 200 2 1 0 0 0 3";
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(7));
        assert!(gm_request(command, false, online.world()).is_err());
        assert_eq!(
            gm_request(command, true, online.world()),
            Ok(ClientCommand::SendChat(OutboundChat::Say(format!(
                "#{command}"
            ))))
        );
        assert!(gm_request("spawn ScriptFixture 60 1 0 200", true, online.world()).is_err());
        session::generation(&mut online, GameDialect::EqMac);
        assert!(gm_request(command, true, online.world()).is_err());
        assert!(gm_request("spawn ScriptFixture 1 1", true, online.world()).is_ok());
    }

    #[test]
    fn depop_only_queues_for_the_reconciled_current_npc() {
        use crate::online::{OnlineState, testing as session};
        use eq_client_core::{ClientCommand, SpawnKind, WorldEvent, targets::TargetState};

        let mut online = OnlineState::new(true);
        session::admit(&mut online, 1, session::player(7));
        let mut npc = session::pet(8, 0);
        npc.kind = SpawnKind::Npc;
        npc.pet_owner = None;
        let mut corpse = npc.clone();
        corpse.kind = SpawnKind::NpcCorpse;
        corpse.spawn_id = 9;
        session::spawns(&mut online, vec![npc.clone(), corpse]);
        let target = |spawn_id, revision| WorldEvent::TargetChanged {
            session_id: 1,
            request_id: 0,
            state: TargetState {
                revision,
                spawn_id: Some(spawn_id),
                available: true,
            },
        };
        for (spawn_id, revision) in [(7, 1), (9, 2)] {
            session::news(&mut online, [target(spawn_id, revision)]);
            assert!(gm_request("depop", true, online.world()).is_err());
        }
        session::news(&mut online, [target(8, 3)]);
        assert!(matches!(
            gm_request("depop", true, online.world()),
            Ok(ClientCommand::DepopLocalNpc {
                session_id: 1,
                target: TargetState {
                    revision: 3,
                    spawn_id: Some(8),
                    available: true
                },
                ..
            })
        ));
        assert!(gm_request("depop", false, online.world()).is_err());
        assert!(gm_request("depop 1", true, online.world()).is_err());
        assert!(gm_request("depopzone", true, online.world()).is_err());

        // A reused spawn ID cannot inherit the original selection.
        session::news(
            &mut online,
            [WorldEvent::Despawn(8), WorldEvent::Spawns(vec![npc])],
        );
        assert!(gm_request("depop", true, online.world()).is_err());
        online.select_target(Some(8));
        assert!(gm_request("depop", true, online.world()).is_err());
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
    fn scripted_drop_click_uses_the_inventory_button_and_ordinary_confirmation() {
        use crate::{
            confirm::{AnswerButton, Asked, Question},
            inventory::ground_drop::{DropButton, input},
            online::{OnlineState, testing},
            windows::{Shown, WindowId},
        };
        use eq_client_core::inventory::{InventorySlot, InventoryUpdate};

        let (mut app, _) = background(vec![
            Step::Click(ClickTarget::Drop),
            Step::Wait(Duration::from_millis(100)),
            Step::Click(ClickTarget::Answer(true)),
        ]);
        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 7, testing::player(9));
        testing::empty_carried_coins(&mut online);
        let mut item = crate::preview::items().remove(0);
        item.slot = InventorySlot::CURSOR;
        item.bag_slots = 0;
        item.stack_count = None;
        item.rules.ground_drop_serial = std::num::NonZeroU32::new(17);
        testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![item.clone()]));
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        app.insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .init_resource::<Asked>()
            .add_systems(
                Update,
                (crate::confirm::buttons, input, crate::confirm::window).chain(),
            );
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::Inventory);
        app.world_mut().spawn((
            Button,
            DropButton,
            laid_out(Vec2::new(240.0, 360.0), 40.0, 1),
        ));
        let dialog = app.world_mut().spawn(Node::default()).id();
        app.world_mut().spawn((
            Button,
            AnswerButton(true),
            ChildOf(dialog),
            laid_out(Vec2::new(600.0, 360.0), 40.0, 2),
        ));
        let mut asked = false;
        for ms in (0..=1_500).step_by(10) {
            let visible = app
                .world()
                .resource::<Shown>()
                .is_open(WindowId::Confirmation);
            app.world_mut().get_mut::<Node>(dialog).unwrap().display = if visible {
                Display::Flex
            } else {
                Display::None
            };
            frame_at(&mut app, ms);
            if matches!(
                app.world().resource::<Asked>().question(),
                Some(Question::DropItem { .. })
            ) {
                asked = true;
                assert!(receiver.try_recv().is_err());
            }
        }
        assert!(asked);
        let eq_client_core::ClientCommand::DropItem(request) = receiver.try_recv().unwrap() else {
            panic!("expected one confirmed drop");
        };
        assert_eq!(
            (request.session_id, request.own_id, request.item),
            (7, 9, item)
        );
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn scripted_drop_click_cannot_open_confirmation_when_capability_is_withheld() {
        use crate::{
            confirm::Asked,
            inventory::ground_drop::{DropButton, input},
            online::{OnlineState, testing},
            windows::{Shown, WindowId},
        };
        use eq_client_core::{
            Capability, WorldEvent,
            inventory::{InventorySlot, InventoryUpdate},
        };

        let (mut app, _) = background(vec![Step::Click(ClickTarget::Drop)]);
        let mut online = OnlineState::new(true);
        testing::news(
            &mut online,
            [WorldEvent::Entered {
                capabilities: Capability::ALL
                    .into_iter()
                    .filter(|capability| *capability != Capability::GroundDropping)
                    .collect(),
                choices: vec![],
                session_id: 7,
                zone: "fixture".into(),
                player: Box::new(testing::player(9)),
                far_clip: None,
                spell_landing_flag: None,
            }],
        );
        testing::connect(&mut online, true);
        testing::empty_carried_coins(&mut online);
        let mut item = crate::preview::items().remove(0);
        item.slot = InventorySlot::CURSOR;
        item.bag_slots = 0;
        item.stack_count = None;
        item.rules.ground_drop_serial = std::num::NonZeroU32::new(17);
        testing::inventory(&mut online, InventoryUpdate::Snapshot(vec![item]));
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        app.insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(Some(sender)))
            .init_resource::<Asked>()
            .add_systems(Update, input);
        app.world_mut()
            .resource_mut::<Shown>()
            .open(WindowId::Inventory);
        let button = app
            .world_mut()
            .spawn((
                Button,
                DropButton,
                laid_out(Vec2::new(240.0, 360.0), 40.0, 1),
            ))
            .id();

        for ms in (0..=1_500).step_by(10) {
            frame_at(&mut app, ms);
            assert!(app.world().resource::<Asked>().question().is_none());
        }
        assert_eq!(
            app.world().get::<Node>(button).unwrap().display,
            Display::None
        );
        assert!(receiver.try_recv().is_err());
    }

    /// An offline app driving `steps`, with the real pointer outside its
    /// window and the script pointer's arrow, on a clock the test sets.
    fn pointing(steps: Vec<Step>) -> App {
        let mut app = crate::testing::app();
        app.insert_resource(Time::<Real>::new(std::time::Instant::now()))
            .init_resource::<crate::windows::Moves>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .add_message::<bevy::window::WindowFocused>()
            .add_message::<AppExit>()
            .insert_resource(Script::new(steps).offline_preview(true))
            .add_systems(Startup, pointer::spawn)
            .add_systems(Update, (drive, pointer::draw).chain());
        app
    }

    /// Production `PreUpdate` input/drive/click order, without any OS window or
    /// network connection. The script has public-session input permissions.
    fn background(steps: Vec<Step>) -> (App, Entity) {
        let mut app = crate::testing::app();
        app.insert_resource(Time::<Real>::new(std::time::Instant::now()))
            .init_resource::<crate::windows::Moves>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .add_message::<bevy::input::keyboard::KeyboardFocusLost>()
            .add_message::<bevy::input::mouse::MouseButtonInput>()
            .add_message::<bevy::input::mouse::MouseWheel>()
            .add_message::<bevy::window::WindowFocused>()
            .add_message::<AppExit>()
            .init_resource::<SeenKeys>()
            .add_systems(
                PreUpdate,
                (
                    bevy::input::keyboard::keyboard_input_system,
                    bevy::input::mouse::mouse_button_input_system,
                )
                    .in_set(bevy::input::InputSystems),
            )
            .add_systems(Update, collect_keys);
        crate::install_script(&mut app, (None, None), true, false, InputMode::Background);
        app.insert_resource(Script::new(steps).input_mode(InputMode::Background));
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        (app, window)
    }

    #[derive(Default, Resource)]
    struct SeenKeys(Vec<bevy::input::keyboard::KeyboardInput>);

    #[allow(clippy::needless_pass_by_value)] // Bevy system parameters are wrappers.
    fn collect_keys(
        mut events: MessageReader<bevy::input::keyboard::KeyboardInput>,
        mut seen: ResMut<SeenKeys>,
    ) {
        seen.0.extend(events.read().cloned());
    }

    fn native_key(app: &mut App, window: Entity, key_code: KeyCode, pressed: bool) {
        use bevy::input::{ButtonState, keyboard::KeyboardInput};
        let state = if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        };
        let (logical_key, text) = parse::typed(key_code, false, false);
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key,
            state,
            text: text.filter(|_| pressed).map(Into::into),
            repeat: false,
            window,
        });
    }

    fn set_focus(app: &mut App, window: Entity, focused: bool) {
        app.world_mut().get_mut::<Window>(window).unwrap().focused = focused;
        app.world_mut()
            .write_message(bevy::window::WindowFocused { window, focused });
    }

    fn pressed_give() -> (App, Entity, Entity) {
        let (mut app, window) = background(vec![Step::HoldClick(
            ClickTarget::Give,
            Duration::from_secs(2),
        )]);
        let (give, _) = give_and_attack(&mut app);
        for ms in [0, 10, 260] {
            frame_at(&mut app, ms);
        }
        assert_eq!(interaction(&app, give), Interaction::Pressed);
        assert!(
            app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
        (app, window, give)
    }

    #[test]
    fn background_runs_use_public_permissions_and_do_not_repeat_held_input() {
        use bevy::ecs::system::RunSystemOnce;
        let (mut app, window) = background(vec![Step::Hold(
            vec![KeyCode::KeyW],
            Duration::from_millis(100),
        )]);
        frame_at(&mut app, 0);
        assert!(app.world().resource::<Script>().unattended());
        assert!(!app.world().resource::<Script>().local);
        assert!(!app.world().resource::<Script>().offline);
        assert!(app.world().resource::<Script>().pointer_owned());
        assert!(
            app.world_mut()
                .run_system_once(|keys: crate::keys::Keys| keys.focused())
                .unwrap()
        );
        set_focus(&mut app, window, false);
        app.world_mut()
            .write_message(bevy::input::keyboard::KeyboardFocusLost);
        frame_at(&mut app, 20);
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(keys.pressed(KeyCode::KeyW));
        assert!(!keys.just_pressed(KeyCode::KeyW));
        set_focus(&mut app, window, true);
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(Some(Vec2::new(10.0, 10.0)));
        for ms in [40, 100, 110] {
            frame_at(&mut app, ms);
        }
        assert!(!app.world().contains_resource::<Script>());
        assert_eq!(
            app.world().resource::<RunStatus>().phase(),
            Phase::Completed
        );
        let events = &app.world().resource::<SeenKeys>().0;
        assert_eq!(
            events
                .iter()
                .filter(|event| event.state.is_pressed())
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| !event.state.is_pressed())
                .count(),
            1
        );
        assert_eq!(*app.world().resource::<InputMode>(), InputMode::Background);
    }

    #[test]
    fn default_public_runs_still_wait_for_focus_and_cancel_on_focus_loss() {
        let (mut app, window) = background(Vec::new());
        app.insert_resource(Script::new(vec![Step::Hold(
            vec![KeyCode::KeyW],
            Duration::from_secs(1),
        )]));
        frame_at(&mut app, 0);
        assert_eq!(
            app.world().resource::<RunStatus>().phase(),
            Phase::WaitingForFocus
        );
        set_focus(&mut app, window, true);
        frame_at(&mut app, 10);
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW)
        );
        set_focus(&mut app, window, false);
        frame_at(&mut app, 20);
        assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Failed);
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW)
        );
    }

    #[test]
    fn background_waits_for_native_ownership_and_rechecks_before_the_first_action() {
        let (mut app, window) = background(vec![Step::Press(vec![KeyCode::KeyA])]);
        set_focus(&mut app, window, true);
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .set_cursor_position(Some(Vec2::new(10.0, 10.0)));
        native_key(&mut app, window, KeyCode::KeyW, true);
        frame_at(&mut app, 0);
        assert_eq!(
            app.world().resource::<RunStatus>().phase(),
            Phase::WaitingForInput
        );
        assert!(!app.world().resource::<Script>().pointer_owned());
        frame_at(&mut app, 10);
        assert_eq!(
            app.world().resource::<RunStatus>().phase(),
            Phase::WaitingForInput
        );
        native_key(&mut app, window, KeyCode::KeyW, false);
        frame_at(&mut app, 20);
        assert_eq!(
            app.world().resource::<RunStatus>().phase(),
            Phase::WaitingForInput
        );
        frame_at(&mut app, 30);
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyA)
        );
        frame_at(&mut app, 40);
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyA)
        );
        assert_eq!(
            app.world()
                .resource::<SeenKeys>()
                .0
                .iter()
                .filter(|e| e.key_code == KeyCode::KeyA && e.state.is_pressed())
                .count(),
            1
        );
    }

    #[test]
    fn passive_background_observation_keeps_wait_select_and_reports_running() {
        for (focused, pointer) in [(true, false), (false, true), (true, true)] {
            let (mut app, window) = background(vec![
                Step::WaitSelect,
                Step::Report("observed selection".into()),
                Step::Wait(Duration::from_millis(10)),
            ]);
            set_focus(&mut app, window, focused);
            if pointer {
                app.world_mut()
                    .get_mut::<Window>(window)
                    .unwrap()
                    .set_cursor_position(Some(Vec2::new(10.0, 10.0)));
            }
            for ms in [0, 10] {
                frame_at(&mut app, ms);
                let script = app.world().resource::<Script>();
                assert!(script.pointer_owned());
                assert!(matches!(script.current, Some((Step::WaitSelect, _))));
                assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Running);
            }
            // Only actual selection state satisfies the wait, even while watched.
            app.world_mut()
                .resource_mut::<crate::online::OnlineState>()
                .selection = Some(crate::character_select::Selection::new(1, Vec::new()));
            for ms in [20, 30, 40, 50, 60] {
                frame_at(&mut app, ms);
            }
            assert!(!app.world().contains_resource::<Script>());
            assert_eq!(
                app.world().resource::<RunStatus>().phase(),
                Phase::Completed
            );
            assert_eq!(
                app.world()
                    .resource::<SeenKeys>()
                    .0
                    .iter()
                    .filter(|event| event.state.is_pressed())
                    .count(),
                0
            );
        }
    }

    #[test]
    fn a_paused_background_run_yields_its_pointer_until_explicitly_resumed() {
        let (mut app, window) = background(vec![
            Step::Wait(Duration::from_millis(10)),
            Step::Press(vec![KeyCode::KeyA]),
        ]);
        frame_at(&mut app, 0);
        app.world_mut()
            .resource_mut::<RunControl>()
            .request(Request::Pause);
        frame_at(&mut app, 10);
        frame_at(&mut app, 20);
        assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Paused);
        assert!(!app.world().resource::<Script>().pointer_owned());
        set_focus(&mut app, window, true);
        frame_at(&mut app, 30);
        assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Paused);
        app.world_mut()
            .resource_mut::<RunControl>()
            .request(Request::Resume);
        frame_at(&mut app, 40);
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyA)
        );
    }

    #[test]
    fn native_same_key_survives_background_cancellation_without_a_synthetic_release() {
        let (mut app, window) = background(vec![Step::Hold(
            vec![KeyCode::KeyW],
            Duration::from_secs(1),
        )]);
        frame_at(&mut app, 0);
        set_focus(&mut app, window, true);
        native_key(&mut app, window, KeyCode::KeyW, true);
        frame_at(&mut app, 10);
        assert!(!app.world().contains_resource::<Script>());
        assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Failed);
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW)
        );
        assert!(
            app.world()
                .resource::<SeenKeys>()
                .0
                .iter()
                .all(|event| event.state.is_pressed())
        );
        native_key(&mut app, window, KeyCode::KeyW, false);
        frame_at(&mut app, 20);
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW)
        );
    }

    #[test]
    fn background_yields_to_wheel_input_and_other_app_windows() {
        use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
        let (mut app, window) = background(vec![Step::Hold(
            vec![KeyCode::KeyW],
            Duration::from_secs(1),
        )]);
        frame_at(&mut app, 0);
        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: 1.0,
            window,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
        frame_at(&mut app, 10);
        assert!(!app.world().contains_resource::<Script>());
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW)
        );

        let (mut app, _) = background(vec![Step::Press(vec![KeyCode::KeyA])]);
        let other = app.world_mut().spawn(Window::default()).id();
        native_key(&mut app, other, KeyCode::KeyW, true);
        frame_at(&mut app, 0);
        assert_eq!(
            app.world().resource::<RunStatus>().phase(),
            Phase::WaitingForInput
        );
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyA)
        );
    }

    #[test]
    fn passive_focus_and_pointer_observation_preserve_a_background_mouse_hold() {
        for pointer_return in [false, true] {
            let (mut app, window, give) = pressed_give();
            if pointer_return {
                app.world_mut()
                    .get_mut::<Window>(window)
                    .unwrap()
                    .set_cursor_position(Some(Vec2::new(10.0, 10.0)));
            } else {
                set_focus(&mut app, window, true);
            }
            frame_at(&mut app, 270);
            assert!(app.world().contains_resource::<Script>());
            assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Running);
            assert!(
                app.world()
                    .resource::<ButtonInput<MouseButton>>()
                    .pressed(MouseButton::Left)
            );
            assert_eq!(interaction(&app, give), Interaction::Pressed);
            assert!(
                !app.world()
                    .resource::<crate::clicks::LeftButton>()
                    .clicked(give)
            );
        }
    }

    #[test]
    fn a_native_same_button_press_is_preserved_when_the_script_yields() {
        let (mut app, window, give) = pressed_give();
        app.world_mut()
            .write_message(bevy::input::mouse::MouseButtonInput {
                button: MouseButton::Left,
                state: bevy::input::ButtonState::Pressed,
                window,
            });
        frame_at(&mut app, 270);
        assert!(!app.world().contains_resource::<Script>());
        assert!(
            app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
        assert_eq!(interaction(&app, give), Interaction::None);
        assert!(
            !app.world()
                .resource::<crate::clicks::LeftButton>()
                .clicked(give)
        );
    }

    #[test]
    fn stop_runtime_limit_and_session_loss_cancel_background_mouse_presses() {
        use crate::online::testing as session;
        use eq_client_core::world::Link;
        for cause in ["stop", "runtime", "session"] {
            let (mut app, _, give) = pressed_give();
            let ms = match cause {
                "stop" => {
                    app.world_mut()
                        .resource_mut::<RunControl>()
                        .request(Request::Stop);
                    270
                }
                "runtime" => u64::try_from(MAX_RUNTIME.as_millis()).unwrap() + 1,
                _ => {
                    let mut online = app.world_mut().resource_mut::<crate::online::OnlineState>();
                    online.enabled = true;
                    session::admit(&mut online, 1, session::player(7));
                    app.world_mut().resource_mut::<Script>().session = RunSession::Admitted;
                    session::link(
                        &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
                        Link::Ended,
                    );
                    270
                }
            };
            frame_at(&mut app, ms);
            assert!(!app.world().contains_resource::<Script>());
            assert!(
                !app.world()
                    .resource::<ButtonInput<MouseButton>>()
                    .pressed(MouseButton::Left),
                "{cause}"
            );
            assert!(
                !app.world()
                    .resource::<crate::clicks::LeftButton>()
                    .clicked(give),
                "{cause}"
            );
            assert_eq!(interaction(&app, give), Interaction::None, "{cause}");
        }
    }

    #[test]
    fn background_clicks_keep_real_geometry_and_coverage_checks() {
        let (mut app, _) = background(vec![Step::Click(ClickTarget::Give)]);
        let (give, _) = give_and_attack(&mut app);
        app.world_mut()
            .spawn((Button, laid_out(Vec2::new(240.0, 360.0), 100.0, 10)));
        for ms in [0, 10, 260, 270] {
            frame_at(&mut app, ms);
        }
        assert_ne!(interaction(&app, give), Interaction::Pressed);
        assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Failed);
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
    }

    #[test]
    fn viewer_factory_preserves_mode_and_separates_local_offline_permissions() {
        for mode in [InputMode::Attended, InputMode::Background] {
            for online in [false, true] {
                let config = crate::ViewerConfig {
                    script_input_mode: mode,
                    ..default()
                };
                let run = Script::for_viewer(
                    Program::parse("wait 10", Path::new(".")).unwrap(),
                    &config,
                    online,
                );
                assert_eq!(run.input_mode, mode);
                assert!(!run.local);
                assert_eq!(run.offline, !online);
                assert!(!run.owns_input);
            }
        }
    }

    /// Runs a frame `ms` into the run.
    fn frame_at(app: &mut App, ms: u64) {
        let mut time = app.world_mut().resource_mut::<Time<Real>>();
        let startup = time.startup();
        time.update_with_instant(startup + Duration::from_millis(ms));
        app.update();
    }

    #[test]
    fn pausing_and_single_stepping_release_input_between_actions() {
        let mut app = pointing(vec![
            Step::Hold(vec![KeyCode::KeyW], Duration::from_millis(100)),
            Step::Press(vec![KeyCode::KeyA]),
            Step::Wait(Duration::from_millis(10)),
        ]);
        app.init_resource::<RunControl>()
            .init_resource::<RunStatus>()
            .add_systems(Update, observe.after(drive));
        frame_at(&mut app, 0);
        app.world_mut()
            .resource_mut::<RunControl>()
            .request(Request::Pause);
        frame_at(&mut app, 50);
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW)
        );
        assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Pausing);
        frame_at(&mut app, 100);
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW)
        );
        assert_eq!(app.world().resource::<RunStatus>().phase(), Phase::Paused);
        assert_eq!(app.world().resource::<RunStatus>().progress(), (1, 3));

        app.world_mut()
            .resource_mut::<RunControl>()
            .request(Request::Step);
        frame_at(&mut app, 5_000);
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyA)
        );
        frame_at(&mut app, 5_010);
        let status = app.world().resource::<RunStatus>();
        assert_eq!(status.phase(), Phase::Paused);
        assert_eq!(status.progress(), (2, 3));
        assert_eq!(status.elapsed(), Duration::from_millis(110));
        assert!(
            !app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyA)
        );

        app.world_mut()
            .resource_mut::<RunControl>()
            .request(Request::Stop);
        frame_at(&mut app, 5_020);
        assert!(!app.world().contains_resource::<Script>());
        let status = app.world().resource::<RunStatus>();
        assert_eq!(status.phase(), Phase::Stopped);
        assert_eq!(status.progress(), (2, 3));
    }

    #[test]
    fn terminal_disconnect_releases_a_hold_but_zoning_does_not_end_the_run() {
        use crate::online::testing as session;
        use eq_client_core::world::Link;

        let mut app = pointing(vec![
            Step::Hold(vec![KeyCode::KeyW], Duration::from_secs(10)),
            Step::Press(vec![KeyCode::KeyA]),
        ]);
        {
            let mut online = app.world_mut().resource_mut::<crate::online::OnlineState>();
            online.enabled = true;
            session::admit(&mut online, 1, session::player(7));
        }
        frame_at(&mut app, 0);
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyW)
        );
        session::link(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            Link::Zoning,
        );
        frame_at(&mut app, 10);
        assert!(app.world().contains_resource::<Script>());
        session::link(
            &mut app.world_mut().resource_mut::<crate::online::OnlineState>(),
            Link::Ended,
        );
        frame_at(&mut app, 20);
        assert!(!app.world().contains_resource::<Script>());
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(!keys.pressed(KeyCode::KeyW));
        assert!(!keys.pressed(KeyCode::KeyA));
        let status = app.world().resource::<RunStatus>();
        assert_eq!(status.phase(), Phase::Failed);
        assert_eq!(status.events().back().unwrap().message, "Session ended");
    }

    #[test]
    fn a_source_error_retains_its_line_after_the_executor_is_removed() {
        let mut app = pointing(Vec::new());
        let program = Program::parse("# needs a target\n\nface\n", Path::new(".")).unwrap();
        app.insert_resource(Script::from_program(program).offline_preview(true));
        frame_at(&mut app, 0);
        assert_eq!(
            app.world().resource::<Script>().status().phase(),
            Phase::Failed
        );
        frame_at(&mut app, 10);
        assert!(!app.world().contains_resource::<Script>());
        let status = app.world().resource::<RunStatus>();
        assert_eq!(status.line(), Some(3));
        assert_eq!(status.phase(), Phase::Failed);
        assert!(status.events().back().unwrap().message.contains("target"));
    }

    const GIVE_TARGET: u16 = 42;

    fn give_app(
        capacity: usize,
    ) -> (
        App,
        std::sync::mpsc::Receiver<eq_client_core::ClientCommand>,
    ) {
        use crate::online::{OnlineState, testing as session};
        use eq_client_core::inventory::{InventorySlot, InventoryUpdate};

        let mut app = pointing(Vec::new());
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        set_focus(&mut app, window, true);
        let program = Program::parse("give\nreport after give\n", Path::new(".")).unwrap();
        let mut online = OnlineState::new(true);
        session::admit(&mut online, 9, session::player(7));
        session::spawns(&mut online, vec![session::pet(GIVE_TARGET, 99)]);
        online.select_target(Some(GIVE_TARGET));
        let mut item = crate::preview::items().into_iter().next().unwrap();
        item.slot = InventorySlot::CURSOR;
        session::inventory(&mut online, InventoryUpdate::Snapshot(vec![item]));
        let (tx, rx) = std::sync::mpsc::sync_channel(capacity);
        app.insert_resource(Script::from_program(program))
            .insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(Some(tx)));
        (app, rx)
    }

    fn assert_give_stops(app: &mut App, reason: &str) {
        frame_at(app, 0);
        frame_at(app, 10);
        assert!(!app.world().contains_resource::<Script>());
        let status = app.world().resource::<RunStatus>();
        assert_eq!(status.phase(), Phase::Failed);
        assert_eq!(status.line(), Some(1));
        assert_eq!(status.progress(), (0, 2));
        assert_eq!(status.events().back().unwrap().message, reason);
    }

    #[test]
    fn scripted_give_stops_before_the_next_line_when_the_offer_is_inapplicable() {
        use crate::{
            online::{OnlineState, testing as session},
            outbox::{Outbox, Refusal},
        };
        use eq_client_core::{WorldEvent, exchange::ExchangeUpdate, inventory::InventoryUpdate};

        type Setup = fn(&mut OnlineState);
        let cases: [(Setup, &str); 5] = [
            (|online| online.select_target(None), "give needs a target"),
            (
                |online| session::inventory(online, InventoryUpdate::Snapshot(Vec::new())),
                "give needs an item or coins on the cursor",
            ),
            (
                |online| online.select_target(Some(7)),
                "give needs an NPC or another player",
            ),
            (
                |online| session::spawns(online, vec![session::pet(GIVE_TARGET, 100)]),
                "give target is no longer available",
            ),
            (
                |online| {
                    online.offer_trade(GIVE_TARGET);
                    session::news(
                        online,
                        [WorldEvent::Exchange(ExchangeUpdate::Opened {
                            with: u32::from(GIVE_TARGET),
                        })],
                    );
                    assert!(online.world().exchange().unwrap().open);
                },
                "give cannot start while an exchange is pending or open",
            ),
        ];
        for (setup, reason) in cases {
            let (mut app, rx) = give_app(1);
            setup(&mut app.world_mut().resource_mut::<OnlineState>());
            assert_give_stops(&mut app, reason);
            assert!(rx.try_recv().is_err());
            assert_eq!(
                app.world().resource::<Outbox>().take_refused(),
                Vec::<Refusal>::new()
            );
        }
    }

    #[test]
    fn scripted_give_reports_a_full_or_closed_queue_only_in_the_script_result() {
        use crate::{
            chat::ChatState,
            online::OnlineState,
            outbox::{self, Refusal},
        };
        use bevy::ecs::system::RunSystemOnce;
        use eq_client_core::chat::ChatTab;

        for refusal in [Refusal::Busy, Refusal::Ended] {
            // A rendezvous channel is full until a receiver waits for a send.
            let (mut app, rx) = give_app(0);
            let rx = (refusal == Refusal::Busy).then_some(rx);
            assert_give_stops(&mut app, refusal.text());
            assert!(rx.is_none_or(|rx| rx.try_recv().is_err()));
            app.world_mut().run_system_once(outbox::show).unwrap();
            assert!(
                !app.world()
                    .resource::<ChatState>()
                    .history
                    .lines(ChatTab::All)
                    .iter()
                    .any(|(_, line)| line.message.text == refusal.text())
            );
            assert!(
                app.world()
                    .resource::<OnlineState>()
                    .world()
                    .exchange()
                    .is_none()
            );
        }
    }

    #[test]
    fn scripted_give_queues_one_offer_and_advances_before_the_server_answers() {
        use eq_client_core::{ClientCommand, inventory::InventorySlot};

        let (mut app, rx) = give_app(2);
        frame_at(&mut app, 0);
        assert!(matches!(
            rx.try_recv().unwrap(),
            ClientCommand::OfferTrade {
                session_id: 9,
                with_id: GIVE_TARGET,
                ..
            }
        ));
        frame_at(&mut app, 10);
        frame_at(&mut app, 20);
        let status = app.world().resource::<RunStatus>();
        assert_eq!(status.phase(), Phase::Completed);
        assert_eq!(status.progress(), (2, 2));
        let world = app.world().resource::<crate::online::OnlineState>().world();
        let exchange = world.exchange().unwrap();
        assert_eq!(exchange.with, GIVE_TARGET);
        assert!(!exchange.open);
        assert!(
            world
                .inventory()
                .items()
                .contains_key(&InventorySlot::CURSOR)
        );
        assert!(rx.try_recv().is_err());
    }

    /// A node laid out square around `middle`, `side` across and `stack`
    /// high in the stack.
    fn laid_out(middle: Vec2, side: f32, stack: u32) -> impl Bundle {
        (
            InheritedVisibility::VISIBLE,
            ComputedNode {
                size: Vec2::splat(side),
                ..default()
            },
            UiGlobalTransform::from_xy(middle.x, middle.y),
            bevy::ui::ComputedStackIndex(stack),
        )
    }

    /// The give button, at the window's left, and the attack button, between
    /// it and the window's middle, where the script pointer starts.
    fn give_and_attack(app: &mut App) -> (Entity, Entity) {
        let give = app
            .world_mut()
            .spawn((
                Button,
                super::super::give::GiveButton,
                laid_out(Vec2::new(240.0, 360.0), 40.0, 1),
            ))
            .id();
        let attack = app
            .world_mut()
            .spawn((
                Button,
                super::super::skinned::AttackButton,
                laid_out(Vec2::new(440.0, 360.0), 60.0, 2),
            ))
            .id();
        (give, attack)
    }

    fn interaction(app: &App, entity: Entity) -> Interaction {
        *app.world().get::<Interaction>(entity).unwrap()
    }

    #[test]
    fn a_script_stops_before_pressing_an_unavailable_run_speed_slider() {
        use crate::{
            online::{OnlineState, testing},
            options::OptionsState,
            outbox::{Needs, UNAVAILABLE},
            skinned::{SkinSlider, slide},
        };
        use bevy::ui::RelativeCursorPosition;
        use eq_client_core::{WorldEvent, options::Level};

        let mut online = OnlineState::new(true);
        testing::news(
            &mut online,
            [WorldEvent::Entered {
                capabilities: Vec::new(),
                choices: Vec::new(),
                session_id: 9,
                zone: "qeytoqrg".into(),
                player: Box::new(testing::player(7)),
                far_clip: None,
                spell_landing_flag: None,
            }],
        );
        testing::connect(&mut online, true);
        let level = Level::RunSpeed;
        let capability = level.needs().unwrap();
        assert_eq!(online.world().session_id(), Some(9));
        assert!(!online.world().can(capability));
        let program = Program::parse(
            "click slider run_boost 100\nreport after click\n",
            Path::new("."),
        )
        .unwrap();
        let mut app = pointing(Vec::new());
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        set_focus(&mut app, window, true);
        app.insert_resource(online)
            .insert_resource(Script::from_program(program))
            .add_systems(Update, slide.after(drive));
        app.world_mut()
            .resource_mut::<OptionsState>()
            .options
            .set_level(level, 120);
        let slider = app
            .world_mut()
            .spawn((
                Button,
                SkinSlider::test_level(level),
                Needs::Capability(capability),
                RelativeCursorPosition::default(),
                laid_out(Vec2::new(240.0, 360.0), 80.0, 1),
            ))
            .id();
        for ms in [0, 10, 260, 270, 280, 290] {
            frame_at(&mut app, ms);
            assert_ne!(interaction(&app, slider), Interaction::Pressed);
        }
        assert!(!app.world().contains_resource::<Script>());
        let status = app.world().resource::<RunStatus>();
        assert_eq!(status.phase(), Phase::Failed);
        assert_eq!(status.line(), Some(1));
        assert_eq!(status.progress(), (0, 2));
        assert_eq!(
            status.events().back().unwrap().message,
            format!("that slider is greyed out: {UNAVAILABLE}")
        );
        assert_eq!(
            app.world().resource::<OptionsState>().options.level(level),
            120
        );
    }

    fn script_pointer(app: &App) -> Option<Vec2> {
        app.world().get_resource::<ScriptPointer>().map(|at| at.0)
    }

    #[test]
    fn inventory_script_click_queues_from_projection_before_worker_feedback() {
        use crate::online::testing;
        use bevy::ecs::system::RunSystemOnce;
        use eq_client_core::inventory::{InventorySlot, InventoryUpdate};
        let mut online = crate::online::OnlineState::new(true);
        testing::admit(&mut online, 7, testing::player(7));
        testing::inventory(
            &mut online,
            InventoryUpdate::Snapshot(
                crate::preview::items()
                    .into_iter()
                    .filter(|item| item.slot != InventorySlot::CURSOR)
                    .collect(),
            ),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        let mut app = pointing(vec![Step::Click(ClickTarget::Slot(24))]);
        app.insert_resource(online)
            .insert_resource(crate::outbox::Outbox::new(Some(tx)));
        let first = app
            .world_mut()
            .spawn((
                Button,
                Interaction::Pressed,
                crate::inventory::SlotButton(InventorySlot(251)),
                crate::clicks::OnPress,
            ))
            .id();
        // This setup deliberately picks the whole stack; plain clicks now
        // open the quantity picker before any move can be posted.
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ShiftLeft);
        app.world_mut()
            .run_system_once(crate::inventory::input)
            .unwrap();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::ShiftLeft);
        app.world_mut().despawn(first);
        let eq_client_core::ClientCommand::MoveInventory(first) = rx.try_recv().unwrap() else {
            panic!("expected first move");
        };
        assert_eq!(
            first.quantity,
            eq_client_core::inventory::MoveQuantity::Whole
        );
        assert!(
            app.world()
                .resource::<crate::online::OnlineState>()
                .world()
                .inventory_moves_pending()
        );
        let target = app
            .world_mut()
            .spawn((
                Button,
                crate::inventory::SlotButton(InventorySlot(24)),
                laid_out(Vec2::new(640.0, 360.0), 40.0, 1),
            ))
            .id();
        let mut pressed = false;
        for ms in (0..=1000).step_by(10) {
            frame_at(&mut app, ms);
            if interaction(&app, target) == Interaction::Pressed {
                pressed = true;
                break;
            }
        }
        assert!(pressed, "the script must not wait for worker feedback");
        app.world_mut()
            .run_system_once(crate::inventory::input)
            .unwrap();
        let eq_client_core::ClientCommand::MoveInventory(second) = rx.try_recv().unwrap() else {
            panic!("expected second move before any worker reply");
        };
        assert_eq!(
            (second.from, second.to),
            (InventorySlot::CURSOR, InventorySlot(24))
        );
        assert_eq!(second.revision, first.revision + 1);
        assert_eq!(
            second.sequence.unwrap().after,
            Some(first.sequence.unwrap().id)
        );
        let online = app.world().resource::<crate::online::OnlineState>();
        assert!(
            online
                .world()
                .inventory()
                .items()
                .contains_key(&InventorySlot(24))
        );
        assert!(
            !online
                .world()
                .inventory()
                .items()
                .contains_key(&InventorySlot::CURSOR)
        );
        assert!(online.world().inventory().predicted());
        assert!(online.world().inventory_moves_pending());
    }

    #[test]
    fn a_click_glides_the_pointer_hovering_what_it_crosses_then_presses_its_control() {
        let mut app = pointing(vec![Step::Click(ClickTarget::Give)]);
        let (give, attack) = give_and_attack(&mut app);
        let arrow = |app: &mut App| {
            let world = app.world_mut();
            let mut arrows = world.query_filtered::<&Node, With<pointer::PointerArt>>();
            let node = arrows.single(world).unwrap();
            (node.display, node.left, node.top)
        };
        // The step starts; the glide begins on the next frame, from the
        // window's middle.
        frame_at(&mut app, 0);
        frame_at(&mut app, 0);
        assert_eq!(script_pointer(&app), Some(Vec2::new(640.0, 360.0)));
        // Halfway through its time the pointer is halfway there, over the
        // attack button, which it hovers; nothing is pressed yet.
        frame_at(&mut app, 125);
        let halfway = script_pointer(&app).unwrap();
        assert!(
            halfway.distance(Vec2::new(440.0, 360.0)) < 0.01,
            "{halfway}"
        );
        assert_eq!(interaction(&app, attack), Interaction::Hovered);
        assert_eq!(interaction(&app, give), Interaction::None);
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
        assert_eq!(arrow(&mut app).0, Display::Flex);
        // Once it has left the attack button, that button's hover clears.
        frame_at(&mut app, 200);
        assert!(script_pointer(&app).unwrap().x < 410.0);
        assert_eq!(interaction(&app, attack), Interaction::None);
        // At the end of the glide the pointer rests on the give button's
        // middle and the click presses it.
        frame_at(&mut app, 250);
        assert_eq!(script_pointer(&app), Some(Vec2::new(240.0, 360.0)));
        assert_eq!(interaction(&app, give), Interaction::Pressed);
        assert_eq!(interaction(&app, attack), Interaction::None);
        assert!(
            app.world()
                .get::<super::super::tooltip::ScriptPressed>(give)
                .is_some()
        );
        assert!(
            app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
        assert_eq!(arrow(&mut app), (Display::Flex, px(240), px(360)));
        // The click is let go, the script ends, and the arrow is hidden and
        // the script pointer forgotten.
        frame_at(&mut app, 266);
        frame_at(&mut app, 282);
        frame_at(&mut app, 298);
        assert!(app.world().get_resource::<Script>().is_none());
        assert_eq!(script_pointer(&app), None);
        assert_eq!(arrow(&mut app).0, Display::None);
    }

    #[test]
    fn a_hover_glides_there_hovering_what_it_crosses_and_rests_on_its_control() {
        let mut app = pointing(vec![
            Step::Hover(ClickTarget::Give),
            Step::Wait(Duration::from_secs(1)),
        ]);
        let (give, attack) = give_and_attack(&mut app);
        frame_at(&mut app, 0);
        frame_at(&mut app, 0);
        frame_at(&mut app, 125);
        assert_eq!(interaction(&app, attack), Interaction::Hovered);
        assert_eq!(interaction(&app, give), Interaction::None);
        frame_at(&mut app, 250);
        assert_eq!(script_pointer(&app), Some(Vec2::new(240.0, 360.0)));
        assert_eq!(interaction(&app, attack), Interaction::None);
        assert_eq!(interaction(&app, give), Interaction::Hovered);
        // The hover stays on it as a hover step's does.
        frame_at(&mut app, 400);
        assert_eq!(app.world().resource::<Script>().hovering, Some(give));
        assert_eq!(interaction(&app, give), Interaction::Hovered);
    }

    #[test]
    fn a_window_click_prefers_the_menu_entry_over_the_same_action_in_the_hud() {
        for covered in [false, true] {
            let mut app = pointing(vec![Step::Click(ClickTarget::Toggle("spellbook"))]);
            app.world_mut()
                .resource_mut::<crate::windows::menu::Menu>()
                .open = true;
            let spawn = |app: &mut App, at, stack| {
                app.world_mut()
                    .spawn((
                        Button,
                        crate::windows::SelectorButton(crate::windows::WindowId::Spellbook),
                        laid_out(at, 40.0, stack),
                    ))
                    .id()
            };
            let hud = spawn(&mut app, Vec2::new(80.0, 80.0), 1);
            let menu = spawn(&mut app, Vec2::new(400.0, 300.0), 10);
            // The menu's outside-click shield legitimately covers the HUD copy.
            app.world_mut()
                .spawn((Button, laid_out(Vec2::new(250.0, 250.0), 600.0, 5)));
            if covered {
                app.world_mut().spawn((
                    Node::default(),
                    bevy::ui::FocusPolicy::Block,
                    laid_out(Vec2::new(400.0, 300.0), 40.0, 11),
                ));
            }
            for ms in [0, 0, 125, 250] {
                frame_at(&mut app, ms);
            }
            assert_eq!(interaction(&app, hud), Interaction::None);
            assert_eq!(
                interaction(&app, menu),
                if covered {
                    Interaction::None
                } else {
                    Interaction::Pressed
                }
            );
            if !covered {
                assert_eq!(script_pointer(&app), Some(Vec2::new(400.0, 300.0)));
            }
        }
    }

    #[test]
    fn a_click_stops_on_a_covered_target_but_not_under_its_own_tooltip() {
        // The tooltip its glide raises lies over the control and is ignored,
        // even were it to block the pointer.
        let mut app = pointing(vec![Step::Click(ClickTarget::Give)]);
        let (give, _) = give_and_attack(&mut app);
        app.world_mut().spawn((
            Node::default(),
            super::super::tooltip::TooltipBox,
            bevy::ui::FocusPolicy::Block,
            laid_out(Vec2::new(240.0, 360.0), 200.0, 9),
        ));
        for ms in [0, 0, 125, 250] {
            frame_at(&mut app, ms);
        }
        assert_eq!(interaction(&app, give), Interaction::Pressed);
        // A window over the control takes the pointer: the script stops
        // rather than press what it cannot reach.
        let mut app = pointing(vec![
            Step::Click(ClickTarget::Give),
            Step::Wait(Duration::from_secs(1)),
        ]);
        let (give, _) = give_and_attack(&mut app);
        app.world_mut().spawn((
            Node::default(),
            bevy::ui::FocusPolicy::Block,
            laid_out(Vec2::new(250.0, 360.0), 40.0, 5),
        ));
        for ms in [0, 0, 125, 250] {
            frame_at(&mut app, ms);
        }
        assert_eq!(interaction(&app, give), Interaction::None);
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
        let script = app.world().resource::<Script>();
        assert!(script.current.is_none() && script.steps.is_empty());
    }

    #[test]
    fn a_hover_stops_on_a_covered_target_rather_than_rest_where_it_does_not_point() {
        let mut app = pointing(vec![
            Step::Hover(ClickTarget::Give),
            Step::Wait(Duration::from_secs(1)),
        ]);
        let (give, _) = give_and_attack(&mut app);
        // A window over the control takes the pointer as the glide ends.
        app.world_mut().spawn((
            Node::default(),
            bevy::ui::FocusPolicy::Block,
            laid_out(Vec2::new(250.0, 360.0), 40.0, 5),
        ));
        for ms in [0, 0, 125, 250] {
            frame_at(&mut app, ms);
        }
        assert_eq!(interaction(&app, give), Interaction::None);
        let script = app.world().resource::<Script>();
        assert_eq!(script.hovering, None);
        assert!(script.current.is_none() && script.steps.is_empty());
    }

    /// Where a row of [`merchant_list`] lies with its list unscrolled.
    #[derive(Component)]
    struct Unscrolled(Vec2);

    /// A merchant's list of `rows` wares, 18 rows of 20 pixels showing, and
    /// as much content to scroll as `content` rows fill, in an app driving
    /// `steps` that scrolls it as the wheel does and lays out its rows
    /// where its scroll puts them.
    fn merchant_list(steps: Vec<Step>, rows: u32, content: u32) -> (App, Entity, Vec<Entity>) {
        /// Lays the rows out where the list's scroll puts them, as the UI's
        /// layout does after the lists have taken the wheel.
        #[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
        fn lay_out(
            lists: Query<&ScrollPosition, With<crate::trade::MerchantRows>>,
            mut rows: Query<(&Unscrolled, &mut UiGlobalTransform)>,
        ) {
            let scrolled = lists.single().map_or(0.0, |position| position.y);
            for (unscrolled, mut transform) in &mut rows {
                *transform = UiGlobalTransform::from_xy(unscrolled.0.x, unscrolled.0.y - scrolled);
            }
        }
        let mut app = pointing(steps);
        app.add_message::<bevy::input::mouse::MouseWheel>()
            .add_systems(
                Update,
                (
                    crate::windows::pointer::wheel,
                    pointer::turn,
                    super::super::skinned::scroll_lists,
                    lay_out,
                )
                    .chain()
                    .after(drive),
            );
        // The list spans 540 to 740 across and 120 to 480 down.
        #[allow(clippy::cast_precision_loss, reason = "a handful of rows")]
        let list = app
            .world_mut()
            .spawn((
                crate::trade::MerchantRows::new(Vec::new()),
                crate::windows::pointer::TakesWheel,
                ScrollPosition::default(),
                Node {
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                InheritedVisibility::VISIBLE,
                ComputedNode {
                    size: Vec2::new(200.0, 360.0),
                    content_size: Vec2::new(200.0, 20.0 * content as f32),
                    inverse_scale_factor: 1.0,
                    ..default()
                },
                UiGlobalTransform::from_xy(640.0, 300.0),
                bevy::ui::ComputedStackIndex(1),
            ))
            .id();
        #[allow(clippy::cast_precision_loss, reason = "a handful of rows")]
        let rows = (0..rows)
            .map(|slot| {
                let middle = Vec2::new(640.0, 130.0 + 20.0 * slot as f32);
                app.world_mut()
                    .spawn((
                        Button,
                        crate::trade::Action::Choose(slot),
                        Node::default(),
                        Unscrolled(middle),
                        InheritedVisibility::VISIBLE,
                        ComputedNode {
                            size: Vec2::new(200.0, 20.0),
                            ..default()
                        },
                        UiGlobalTransform::from_xy(middle.x, middle.y),
                        bevy::ui::ComputedStackIndex(2),
                        ChildOf(list),
                    ))
                    .id()
            })
            .collect();
        (app, list, rows)
    }

    fn scrolled(app: &App, list: Entity) -> f32 {
        app.world().get::<ScrollPosition>(list).unwrap().y
    }

    fn stopped(app: &App) -> bool {
        let script = app.world().resource::<Script>();
        script.current.is_none() && script.steps.is_empty()
    }

    #[test]
    fn a_move_holds_its_command_and_a_face_turns_until_the_server_has_the_heading() {
        let admitted = |app: &mut App, heading: f32| {
            let mut player = crate::online::testing::player(1);
            player.position.heading = heading;
            let mut online = app.world_mut().resource_mut::<crate::online::OnlineState>();
            crate::online::testing::admit(&mut online, 1, player);
        };
        let mut app = pointing(vec![
            Step::Move(MoveCommand::Forward, Duration::from_millis(500)),
            Step::FaceHeading(128.0),
            Step::Wait(Duration::from_secs(1)),
        ]);
        admitted(&mut app, 120.0);
        frame_at(&mut app, 0);
        // The step holds the command itself and no key, so it moves the same
        // under every key table, the Titanium program's included.
        assert_eq!(app.world().resource::<Script>().acts(), [Act::Forward]);
        assert_eq!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .get_pressed()
                .count(),
            0
        );
        for ms in [600, 610, 620] {
            frame_at(&mut app, ms);
        }
        let script = app.world().resource::<Script>();
        assert_eq!(script.acts(), &[] as &[Act]);
        assert_eq!(script.facing(), Some(128.0));
        assert!(matches!(script.current, Some((Step::FaceHeading(_), _))));
        // Once the server has the heading, the next step runs and the
        // facing is the player's again.
        admitted(&mut app, 127.8);
        frame_at(&mut app, 700);
        frame_at(&mut app, 710);
        let script = app.world().resource::<Script>();
        assert_eq!(script.facing(), None);
        assert!(matches!(script.current, Some((Step::Wait(_), _))));
        // A heading the server never takes stops the script in time.
        let mut app = pointing(vec![Step::FaceHeading(128.0)]);
        admitted(&mut app, 0.0);
        frame_at(&mut app, 0);
        frame_at(&mut app, 4_000);
        assert!(!stopped(&app));
        frame_at(&mut app, 5_100);
        assert!(stopped(&app));
    }

    #[test]
    fn a_click_on_a_row_scrolled_out_of_view_turns_the_wheel_until_it_shows() {
        let (mut app, list, rows) = merchant_list(
            vec![
                Step::Click(ClickTarget::Trade(TradeClick::Choose(28))),
                Step::Wait(Duration::from_secs(1)),
            ],
            30,
            30,
        );
        // The pointer glides first to the list's bottom edge, under the row.
        for ms in [0, 0, 125] {
            frame_at(&mut app, ms);
        }
        assert_eq!(scrolled(&app, list), 0.0);
        frame_at(&mut app, 250);
        assert_eq!(script_pointer(&app), Some(Vec2::new(640.0, 476.0)));
        // Then the wheel turns down a notch a frame until the row shows
        // whole, the list scrolled as far as it goes.
        let mut seen = vec![scrolled(&app, list)];
        let mut ms = 250;
        while interaction(&app, rows[28]) != Interaction::Pressed && ms < 2000 {
            ms += 16;
            frame_at(&mut app, ms);
            seen.push(scrolled(&app, list));
        }
        assert_eq!(
            interaction(&app, rows[28]),
            Interaction::Pressed,
            "{seen:?}"
        );
        let notches: Vec<_> = seen.iter().copied().filter(|y| *y > 0.0).collect();
        assert_eq!(notches.first(), Some(&24.0));
        assert!(notches.windows(2).all(|pair| pair[1] - pair[0] <= 24.0));
        assert_eq!(scrolled(&app, list), 240.0);
        // The row is clicked where it now shows, wholly inside the list.
        assert_eq!(script_pointer(&app), Some(Vec2::new(640.0, 450.0)));
        assert!(
            app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
        frame_at(&mut app, ms + 16);
        let script = app.world().resource::<Script>();
        assert!(matches!(script.current, Some((Step::Wait(_), _))));
        assert!(script.scrolling.is_none());
    }

    #[test]
    fn a_click_on_a_row_in_view_scrolls_nothing() {
        let (mut app, list, rows) = merchant_list(
            vec![Step::Click(ClickTarget::Trade(TradeClick::Choose(3)))],
            30,
            30,
        );
        for ms in [0, 0, 125, 250] {
            frame_at(&mut app, ms);
        }
        assert_eq!(interaction(&app, rows[3]), Interaction::Pressed);
        assert_eq!(script_pointer(&app), Some(Vec2::new(640.0, 190.0)));
        assert_eq!(scrolled(&app, list), 0.0);
    }

    #[test]
    fn a_row_the_list_cannot_scroll_to_stops_the_script() {
        // The list's content ends with what shows: the wheel moves nothing.
        let (mut app, list, rows) = merchant_list(
            vec![
                Step::Click(ClickTarget::Trade(TradeClick::Choose(28))),
                Step::Wait(Duration::from_secs(1)),
            ],
            30,
            18,
        );
        for ms in [0, 0, 125, 250, 266] {
            frame_at(&mut app, ms);
        }
        assert!(stopped(&app));
        assert_eq!(scrolled(&app, list), 0.0);
        assert_eq!(interaction(&app, rows[28]), Interaction::None);
        // It stopped at the list's edge, never gliding to the row.
        assert_eq!(script_pointer(&app), Some(Vec2::new(640.0, 476.0)));
        assert!(app.world().resource::<Script>().scrolling.is_none());
    }

    #[test]
    fn attended_scripts_wait_for_a_reported_focus() {
        assert!(!may_run(false, None, true));
        assert!(!may_run(false, Some(false), false));
        assert!(may_run(false, Some(true), true));
        assert!(may_run(true, None, true) && may_run(true, Some(false), false));
    }

    #[test]
    fn workbench_runs_keep_focus_observed_before_the_script_existed() {
        use bevy::window::WindowFocused;

        let mut app = pointing(Vec::new());
        app.world_mut().remove_resource::<Script>();
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .unwrap();
        app.world_mut().write_message(WindowFocused {
            window,
            focused: true,
        });
        // Let the startup event expire before Run creates its first script.
        for ms in [0, 10, 20] {
            frame_at(&mut app, ms);
        }
        for ms in [30, 40] {
            app.insert_resource(Script::new(vec![Step::Wait(Duration::from_secs(1))]));
            frame_at(&mut app, ms);
            assert_eq!(
                app.world().resource::<Script>().status().phase,
                Phase::Running
            );
        }

        app.world_mut().remove_resource::<Script>();
        app.world_mut().write_message(WindowFocused {
            window,
            focused: false,
        });
        frame_at(&mut app, 50);
        app.insert_resource(Script::new(vec![Step::Wait(Duration::from_secs(1))]));
        frame_at(&mut app, 60);
        assert_eq!(
            app.world().resource::<Script>().status().phase,
            Phase::WaitingForFocus
        );

        // A different window cannot satisfy the attended-run requirement.
        let other = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(WindowFocused {
            window: other,
            focused: true,
        });
        frame_at(&mut app, 70);
        assert_eq!(
            app.world().resource::<Script>().status().phase,
            Phase::WaitingForFocus
        );
        app.world_mut().write_message(WindowFocused {
            window,
            focused: true,
        });
        frame_at(&mut app, 80);
        assert_eq!(
            app.world().resource::<Script>().status().phase,
            Phase::Running
        );
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
        assert_eq!(
            script.steps.front().map(|queued| &queued.step),
            Some(&Step::Report("a".into()))
        );
        assert_eq!(script.steps.front().unwrap().line, Some(2));
        // An invalid batch is skipped whole; later batches still run.
        std::fs::write(&path, "wait 10\nreport a\nwait 20\nbogus\n").unwrap();
        script.poll(Duration::from_secs(2));
        assert_eq!(script.steps.len(), 1);
        std::fs::write(&path, "wait 10\nreport a\nwait 20\nbogus\nquit\n").unwrap();
        script.poll(Duration::from_secs(3));
        assert_eq!(
            script.steps.back().map(|queued| &queued.step),
            Some(&Step::Quit)
        );
        assert_eq!(script.steps.back().unwrap().line, Some(5));
        let _ = std::fs::remove_file(&path);
    }

    fn structured_preview(source: &str) -> App {
        let program = Program::parse(source, std::path::Path::new(".")).unwrap();
        let mut app = pointing(Vec::new());
        app.insert_resource(Script::from_program(program).offline_preview(true))
            .init_resource::<RunControl>()
            .init_resource::<RunStatus>()
            .add_systems(Update, observe.after(drive));
        app
    }

    #[test]
    fn structured_runs_dispatch_only_the_taken_branch_and_pace_repeated_actions() {
        let mut app = structured_preview(
            "let n: int = 0\nrepeat 2\n  set n = n + 1\n  if n == 1\n    press A\n  else\n    press B\n  end\nend\n",
        );
        for ms in [0, 10, 20, 30, 40] {
            frame_at(&mut app, ms);
        }
        let status = app.world().resource::<RunStatus>();
        let flow = status.flow().unwrap();
        assert_eq!(flow.loops(), [(2, 1, 2)]);
        assert_eq!(flow.branch(), Some((4, true)));
        assert_eq!(flow.variables(), ["n: int = 1"]);
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyA)
        );

        for ms in [50, 60, 70, 80, 139] {
            frame_at(&mut app, ms);
        }
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(!keys.pressed(KeyCode::KeyA));
        assert!(!keys.pressed(KeyCode::KeyB));
        frame_at(&mut app, 140);
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::KeyB)
        );
        frame_at(&mut app, 150);
        frame_at(&mut app, 160);
        let status = app.world().resource::<RunStatus>();
        assert_eq!(status.phase(), Phase::Completed);
        assert_eq!(status.progress(), (2, 2));
        let flow = status.flow().unwrap();
        assert_eq!(flow.branch(), Some((4, false)));
        assert_eq!(flow.variables(), ["n: int = 2"]);
        assert_eq!(flow.loops(), []);
    }

    #[test]
    fn a_loop_without_actions_can_pause_step_and_stop() {
        let mut app = structured_preview("while true limit 100\n  repeat 100\n  end\nend\n");
        frame_at(&mut app, 0);
        app.world_mut()
            .resource_mut::<RunControl>()
            .request(Request::Pause);
        frame_at(&mut app, 1);
        let status = app.world().resource::<RunStatus>();
        assert_eq!(status.phase(), Phase::Paused);
        assert_eq!(status.flow().unwrap().statements(), 1);

        app.world_mut()
            .resource_mut::<RunControl>()
            .request(Request::Step);
        frame_at(&mut app, 2);
        frame_at(&mut app, 3);
        let status = app.world().resource::<RunStatus>();
        assert_eq!(status.phase(), Phase::Paused);
        assert_eq!(status.flow().unwrap().loops(), [(1, 1, 100), (2, 1, 100)]);
        assert_eq!(status.flow().unwrap().statements(), 2);

        app.world_mut()
            .resource_mut::<RunControl>()
            .request(Request::Stop);
        frame_at(&mut app, 4);
        let status = app.world().resource::<RunStatus>();
        assert_eq!(status.phase(), Phase::Stopped);
        assert_eq!(status.flow().unwrap().statements(), 2);
        assert!(!app.world().contains_resource::<Script>());
    }

    #[test]
    fn a_condition_with_unknown_state_stops_on_its_line() {
        let mut app = structured_preview(
            "# offline\nif player.mana == 0\n  report empty\nelse\n  report present\nend",
        );
        frame_at(&mut app, 0);
        let status = app.world().resource::<RunStatus>();
        assert_eq!(status.phase(), Phase::Failed);
        assert_eq!(status.line(), Some(2));
        assert_eq!(status.progress().0, 0);
        assert!(
            status
                .events()
                .back()
                .unwrap()
                .message
                .contains("condition is unknown")
        );
    }

    #[test]
    fn follow_rejects_a_structured_append_before_any_branch_action_can_escape() {
        let path =
            std::env::temp_dir().join(format!("eq-client-flow-follow-{}.txt", std::process::id()));
        std::fs::write(&path, "wait 10\nif false\n").unwrap();
        let mut script = Script::following(Vec::new(), path.clone(), 8);
        script.poll(Duration::ZERO);
        assert_eq!(script.status().phase(), Phase::Failed);
        assert_eq!(script.status().line(), Some(2));
        assert!(script.steps.is_empty());
        std::fs::write(&path, "wait 10\nif false\nquit\nend\n").unwrap();
        script.poll(Duration::from_secs(1));
        assert!(script.steps.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn follow_stops_on_a_bom_prefixed_header_before_the_next_batch() {
        let path = std::env::temp_dir().join(format!(
            "eq-client-bom-flow-follow-{}.txt",
            std::process::id()
        ));
        let prefix = "wait 10\n";
        let header = "\u{feff}\u{feff}if false";
        std::fs::write(&path, format!("{prefix}{header}")).unwrap();
        let mut script = Script::following(Vec::new(), path.clone(), prefix.len());
        script.poll(Duration::ZERO);
        assert!(script.steps.is_empty());
        assert!(script.follow.is_some());

        std::fs::write(&path, format!("{prefix}{header}\n")).unwrap();
        script.poll(Duration::from_secs(1));
        assert_eq!(script.status().phase(), Phase::Failed);
        assert_eq!(script.status().line(), Some(2));
        assert!(script.follow.is_none());

        std::fs::write(&path, format!("{prefix}{header}\nquit\n")).unwrap();
        script.poll(Duration::from_secs(2));
        assert!(script.steps.is_empty());
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
