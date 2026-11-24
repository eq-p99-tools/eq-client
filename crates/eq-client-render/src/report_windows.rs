//! The Titanium program's bug report and feedback windows, which
//! `/bugreport` and `/feedback` open (recording checklist 6.511, C1b; reads
//! R49-23 and the bug report window's fill and send passes, eqgame.exe).
//!
//! **The bug report window** (`BugReportWindow`, class built by 0x4e9980):
//! - Opening it fills the three location labels with the player's place,
//!   each truncated to a whole number (0x4e9825, `%d` of ftol), keeps the
//!   heading the same way, clears its four check boxes and blanks the
//!   target's name (the reset 0x4e9ed8). This client resets it on every
//!   open; that the program's show runs the reset each time is inferred.
//! - **Location** fills the place again.
//! - **Target**, checked with a target, names it (the cleaned name) and
//!   carries the target; checked with none, it stays unchecked; unchecked,
//!   it blanks the name (0x4ea38b).
//! - **Dup**, **Crash** and **Flags** are plain check boxes, sent as the
//!   report's bits 1, 2 and 8 (with the target's 4).
//! - The drop-down picks the category. The program names its categories
//!   from a table in itself (0x791158); this client uses the names the
//!   servers know them by, its own words.
//! - **Send** with an empty box says line 0x22b0 and sends nothing;
//!   otherwise it sends the report, clears the box and says 0x22ae once the
//!   session says it went. The window hides either way. The program first
//!   asks to confirm (line 0x1aa3) unless one of its settings
//!   ([0x9254ee]) is set; that setting is unread, so this client sends
//!   at once (a departure, labelled).
//! - **Cancel** says line 0x22af and closes the window; the box keeps its
//!   words.
//!
//! **The feedback window** (`FeedbackWindow`, class built by 0x5013c9):
//! - **Send** with an empty box only closes the window; otherwise it sends
//!   the feedback with the player's place and heading, clears the box and
//!   closes, with no line.
//! - **Cancel** clears the box and closes the window.
//!
//! A click in either window's text box gives it the keyboard, as the map's
//! search does; a script clicks it (`click("report", "bug_text")`) and then
//! types with `press(...)`.
use crate::{
    chat::ChatState,
    keys::{Typing, search},
    online::OnlineState,
    outbox::Outbox,
    reports::ReportLine,
    windows::{Shown, WindowId},
};
use bevy::{
    clipboard::{Clipboard, ClipboardRead},
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
};
use eq_client_core::{
    ClientCommand,
    reports::{BugCategory, BugPlace, BugReport, BugTarget},
    world::ClientWorld,
};

/// The Titanium program's line when its bug report window is cancelled.
const BUG_CANCELLED: u32 = 0x22af;
/// The longest bug report the program's window sends (read R49-23).
const BUG_LONGEST: usize = 1024;
/// The longest feedback this client sends ([`eq_client_core::reports`]).
const FEEDBACK_LONGEST: usize = eq_client_core::reports::FEEDBACK_LONGEST;

/// Which of the two windows.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Report {
    /// The bug report window.
    Bug,
    /// The feedback window.
    Feedback,
}

impl Report {
    /// The window it is.
    pub(crate) const fn window(self) -> WindowId {
        match self {
            Self::Bug => WindowId::BugReport,
            Self::Feedback => WindowId::Feedback,
        }
    }
}

/// A button of either window, by what it does.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReportButton {
    /// The bug report's Location: fills the place again.
    Location,
    /// The bug report's Dup check box: the bug can be made to happen again.
    Repeatable,
    /// The bug report's Crash check box.
    Crash,
    /// The bug report's Flags check box: the server is to note the
    /// character's flags.
    Flags,
    /// The bug report's Target check box.
    Target,
    /// A window's Cancel.
    Cancel(Report),
    /// A window's Send.
    Send(Report),
}

impl ReportButton {
    /// The button a skin's `ScreenID` names in one of these windows.
    pub(crate) fn of(id: &str, owner: WindowId) -> Option<Self> {
        Some(match (owner, id) {
            (WindowId::BugReport, "LocationButton") => Self::Location,
            (WindowId::BugReport, "DupButton") => Self::Repeatable,
            (WindowId::BugReport, "CrashButton") => Self::Crash,
            (WindowId::BugReport, "FlagsButton") => Self::Flags,
            (WindowId::BugReport, "TargetButton") => Self::Target,
            (WindowId::BugReport, "CancelButton") => Self::Cancel(Report::Bug),
            (WindowId::BugReport, "SendButton") => Self::Send(Report::Bug),
            (WindowId::Feedback, "FBKW_CancelButton") => Self::Cancel(Report::Feedback),
            (WindowId::Feedback, "FBKW_SendButton") => Self::Send(Report::Feedback),
            _ => return None,
        })
    }
}

/// A label either window fills.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReportLabel {
    /// One of the bug report's place labels: x (0), y (1) or z (2).
    Place(usize),
    /// The bug report's target's name.
    TargetName,
    /// The feedback window's name of the player.
    UserName,
}

impl ReportLabel {
    /// The label a skin's piece name names in one of these windows.
    pub(crate) fn of(name: &str, owner: WindowId) -> Option<Self> {
        Some(match (owner, name) {
            (WindowId::BugReport, "BUGW_LocLabelX") => Self::Place(0),
            (WindowId::BugReport, "BUGW_LocLabelY") => Self::Place(1),
            (WindowId::BugReport, "BUGW_LocLabelZ") => Self::Place(2),
            (WindowId::BugReport, "BUGW_TargetName") => Self::TargetName,
            (WindowId::Feedback, "FBKW_UserName") => Self::UserName,
            _ => return None,
        })
    }
}

/// The text box of one of the windows, which a click gives the keyboard.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ReportBox(pub(crate) Report);

impl ReportBox {
    /// The text box a skin's `ScreenID` names in one of these windows.
    pub(crate) fn of(id: Option<&str>, owner: WindowId) -> Option<Self> {
        match (owner, id) {
            (WindowId::BugReport, Some("Edit")) => Some(Self(Report::Bug)),
            (WindowId::Feedback, Some("FBKW_FeedbackInput")) => Some(Self(Report::Feedback)),
            _ => None,
        }
    }
}

/// The words in a window's text box, as it shows them.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ReportWords(pub(crate) Report);

/// What the bug report window holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct BugForm {
    /// The category chosen, by its place in [`BugCategory::ALL`].
    pub(crate) category: usize,
    /// The words in its box.
    pub(crate) text: String,
    /// The place its labels show, in whole numbers.
    pub(crate) place: Option<[i32; 3]>,
    /// The heading kept with the place, a whole number of the game's 512.
    pub(crate) heading: i32,
    /// The Dup check box.
    pub(crate) repeatable: bool,
    /// The Crash check box.
    pub(crate) crash: bool,
    /// The Flags check box.
    pub(crate) character_flags: bool,
    /// The target the Target check box carries, by the name the server sent.
    pub(crate) target: Option<BugTarget>,
}

impl BugForm {
    /// The program's reset, on opening: the place, and no boxes checked.
    fn reset(&mut self, world: &ClientWorld) {
        self.repeatable = false;
        self.crash = false;
        self.character_flags = false;
        self.target = None;
        self.locate(world);
    }

    /// The place and heading, truncated toward zero as the program's ftol
    /// does.
    fn locate(&mut self, world: &ClientWorld) {
        if let Some(player) = world.player() {
            let position = &player.position;
            #[allow(clippy::cast_possible_truncation)] // ftol, as the program.
            {
                self.place = Some([position.x as i32, position.y as i32, position.z as i32]);
                self.heading = position.heading as i32;
            }
        }
    }

    /// The category chosen.
    pub(crate) fn category(&self) -> BugCategory {
        BugCategory::ALL
            .get(self.category)
            .copied()
            .unwrap_or(BugCategory::Other)
    }

    /// The report its Send sends, or None for an empty box.
    fn report(&self) -> Option<BugReport> {
        let text = self.text.trim();
        if text.is_empty() {
            return None;
        }
        #[allow(clippy::cast_precision_loss)] // Whole numbers the labels show.
        let place = self.place.map(|[x, y, z]| BugPlace {
            x: x as f32,
            y: y as f32,
            z: z as f32,
            heading: self.heading as f32,
        });
        Some(BugReport {
            category: self.category(),
            text: text.to_owned(),
            place,
            target: self.target.clone(),
            repeatable: self.repeatable,
            crash: self.crash,
            character_flags: self.character_flags,
        })
    }
}

/// What the two windows hold, and which box has the keyboard.
#[derive(Resource, Debug, Default)]
pub(crate) struct ReportWindows {
    /// The bug report window's fields.
    pub(crate) bug: BugForm,
    /// The words in the feedback window's box.
    pub(crate) feedback: String,
    /// The box that has the keyboard.
    pub(crate) editing: Option<Report>,
}

impl ReportWindows {
    /// Opens a window as its command does.
    pub(crate) fn open(&mut self, report: Report, world: &ClientWorld, shown: &mut Shown) {
        if report == Report::Bug {
            self.bug.reset(world);
        }
        shown.open(report.window());
    }

    /// Whether a check box is checked.
    pub(crate) fn checked(&self, button: ReportButton) -> bool {
        match button {
            ReportButton::Repeatable => self.bug.repeatable,
            ReportButton::Crash => self.bug.crash,
            ReportButton::Flags => self.bug.character_flags,
            ReportButton::Target => self.bug.target.is_some(),
            ReportButton::Location | ReportButton::Cancel(_) | ReportButton::Send(_) => false,
        }
    }

    /// The words a box holds.
    fn words_mut(&mut self, report: Report) -> &mut String {
        match report {
            Report::Bug => &mut self.bug.text,
            Report::Feedback => &mut self.feedback,
        }
    }

    /// The words a box holds.
    pub(crate) fn words(&self, report: Report) -> &str {
        match report {
            Report::Bug => &self.bug.text,
            Report::Feedback => &self.feedback,
        }
    }

    /// What a label shows.
    fn label(&self, label: ReportLabel, world: &ClientWorld) -> String {
        match label {
            ReportLabel::Place(axis) => self
                .bug
                .place
                .map_or_else(String::new, |place| place[axis].to_string()),
            ReportLabel::TargetName => {
                self.bug.target.as_ref().map_or_else(String::new, |target| {
                    eq_client_core::entities::display_name(&target.name)
                })
            }
            ReportLabel::UserName => world
                .player()
                .map_or_else(String::new, |player| player.name.clone()),
        }
    }
}

/// What a button press asks of the chat and the session.
#[derive(Debug, PartialEq)]
enum Pressed {
    /// Nothing more.
    Done,
    /// Say this line.
    Say(ReportLine),
    /// Send this, and say the line once the session says it went.
    Send(Box<ClientCommand>, Option<ReportLine>),
}

impl ReportWindows {
    /// Does what a button does.
    fn press(
        &mut self,
        button: ReportButton,
        world: &ClientWorld,
        stamp: Option<crate::outbox::Stamp>,
        shown: &mut Shown,
    ) -> Pressed {
        match button {
            ReportButton::Location => self.bug.locate(world),
            ReportButton::Repeatable => self.bug.repeatable = !self.bug.repeatable,
            ReportButton::Crash => self.bug.crash = !self.bug.crash,
            ReportButton::Flags => self.bug.character_flags = !self.bug.character_flags,
            // Checked with no target, it stays unchecked (0x4ea38b).
            ReportButton::Target => {
                self.bug.target = if self.bug.target.is_some() {
                    None
                } else {
                    target(world)
                };
            }
            ReportButton::Cancel(report) => {
                shown.close(report.window());
                self.release(report);
                return match report {
                    // The box keeps its words.
                    Report::Bug => Pressed::Say(ReportLine::new(
                        BUG_CANCELLED,
                        crate::reports::PROGRAM_COLOUR,
                        "Your bug report was cancelled.",
                    )),
                    Report::Feedback => {
                        self.feedback.clear();
                        Pressed::Done
                    }
                };
            }
            ReportButton::Send(report) => {
                shown.close(report.window());
                self.release(report);
                return self.send(report, world, stamp);
            }
        }
        Pressed::Done
    }

    /// The Send of a window, which has closed.
    fn send(
        &mut self,
        report: Report,
        world: &ClientWorld,
        stamp: Option<crate::outbox::Stamp>,
    ) -> Pressed {
        match report {
            Report::Bug => {
                let Some(bug) = self.bug.report() else {
                    return Pressed::Say(crate::reports::bug_empty());
                };
                let Some(stamp) = stamp else {
                    return Pressed::Done;
                };
                self.bug.text.clear();
                Pressed::Send(
                    Box::new(ClientCommand::ReportBug {
                        session_id: stamp.session_id,
                        report: bug,
                        created: stamp.created,
                    }),
                    Some(crate::reports::bug_sent()),
                )
            }
            Report::Feedback => {
                let text = self.feedback.trim().to_owned();
                if text.is_empty() {
                    return Pressed::Done;
                }
                let Some(stamp) = stamp else {
                    return Pressed::Done;
                };
                self.feedback.clear();
                Pressed::Send(
                    Box::new(ClientCommand::SendFeedback {
                        session_id: stamp.session_id,
                        text,
                        place: crate::reports::place(world),
                        created: stamp.created,
                    }),
                    None,
                )
            }
        }
    }

    /// Takes the keyboard from a box whose window closed.
    fn release(&mut self, report: Report) {
        if self.editing == Some(report) {
            self.editing = None;
        }
    }
}

/// The player's target, as the Target check box carries it.
fn target(world: &ClientWorld) -> Option<BugTarget> {
    let spawn_id = world.target_intent()?.spawn_id?;
    let spawn = world.spawn(spawn_id)?;
    Some(BugTarget {
        spawn_id,
        name: spawn.state.name.clone(),
    })
}

/// Opens the window a typed command asked for, where the skin has it. The
/// window has no frame until it opens, so the skin's file is asked, not
/// what has been drawn.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn open(
    mut chat: ResMut<ChatState>,
    (online, settings, skin): (
        Res<OnlineState>,
        Res<crate::ViewerSettings>,
        Res<crate::skin::UiSkin>,
    ),
    mut screens: ResMut<crate::skinned::Screens>,
    mut shown: ResMut<Shown>,
    mut windows: ResMut<ReportWindows>,
) {
    let Some(report) = chat.report_window.take() else {
        return;
    };
    let drawn = settings
        .0
        .eq_directory
        .as_deref()
        .is_some_and(|directory| screens.defines(directory, &skin.0, report.window()));
    if drawn {
        windows.open(report, online.world(), &mut shown);
    } else {
        // A departure: the program always has its window.
        let name = match report {
            Report::Bug => "bugreport",
            Report::Feedback => "feedback",
        };
        chat.history.push(crate::chat::system_line(format!(
            "The installed skin has no /{name} window. Turn on Typed Reports to send one from the line."
        )));
    }
}

/// Does what the windows' buttons do.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn buttons(
    clicks: crate::clicks::Clicks<ReportButton>,
    online: Res<OnlineState>,
    outbox: Res<Outbox>,
    mut shown: ResMut<Shown>,
    mut chat: ResMut<ChatState>,
    mut windows: ResMut<ReportWindows>,
) {
    let world = online.world();
    for button in clicks.iter() {
        let stamp = outbox.stamp(world).ok();
        match windows.press(*button, world, stamp, &mut shown) {
            Pressed::Done => {}
            Pressed::Say(line) => chat.report_lines.push(line),
            Pressed::Send(command, echo) => match outbox.send(world, *command) {
                Ok(()) => chat.report_echo = echo,
                Err(refusal) => chat.history.push(crate::chat::system_line(refusal.text())),
            },
        }
    }
}

/// Gives a window's box the keyboard when it is clicked, and takes it back
/// when the window closes or another place is clicked.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn focus(
    mouse: Res<ButtonInput<MouseButton>>,
    shown: Res<Shown>,
    boxes: Query<(&Interaction, &ReportBox)>,
    mut windows: ResMut<ReportWindows>,
    mut typing: ResMut<Typing>,
) {
    if windows
        .editing
        .is_some_and(|report| !shown.displayed(report.window()))
    {
        windows.editing = None;
    }
    if mouse.any_just_pressed([MouseButton::Left, MouseButton::Right]) {
        windows.editing = boxes.iter().find_map(|(interaction, ReportBox(report))| {
            (*interaction == Interaction::Pressed).then_some(*report)
        });
        if windows.editing.is_some() {
            typing.composing = false;
            typing.counting = false;
            typing.map_search = false;
        }
    }
    let reporting = windows.editing.is_some();
    if typing.reporting != reporting {
        typing.reporting = reporting;
    }
}

/// Types into the box that has the keyboard. Enter and Escape give the
/// keyboard back and keep the words.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn input(
    mut keyboard: MessageReader<KeyboardInput>,
    mut typing: ResMut<Typing>,
    mut windows: ResMut<ReportWindows>,
    keys: Res<ButtonInput<Key>>,
    mut clipboard: Option<ResMut<Clipboard>>,
    mut pending: Local<Option<ClipboardRead>>,
) {
    let Some(report) = windows.editing.filter(|_| typing.reporting) else {
        keyboard.clear();
        *pending = None;
        return;
    };
    let events: Vec<_> = keyboard
        .read_with_id()
        .filter(|(_, id)| !typing.spell_search_captured(*id))
        .map(|(event, _)| event.clone())
        .collect();
    let longest = match report {
        Report::Bug => BUG_LONGEST,
        Report::Feedback => FEEDBACK_LONGEST,
    };
    if let Some(finish) = search::edit_up_to(
        windows.words_mut(report),
        longest,
        &events,
        &keys,
        clipboard.as_deref_mut(),
        &mut pending,
    ) {
        windows.editing = None;
        typing.reporting = false;
        typing.escape_consumed |= finish == search::Finish::Escape;
    }
}

/// Writes the windows' labels and boxes, with the caret in the box that has
/// the keyboard.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(crate) fn show(
    online: Res<OnlineState>,
    windows: Res<ReportWindows>,
    mut labels: Query<(&ReportLabel, &mut Text), Without<ReportWords>>,
    mut boxes: Query<(&ReportWords, &mut Text), Without<ReportLabel>>,
) {
    for (label, mut text) in &mut labels {
        let wanted = windows.label(*label, online.world());
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
    for (ReportWords(report), mut text) in &mut boxes {
        let mut wanted = windows.words(*report).to_owned();
        if windows.editing == Some(*report) {
            wanted.push('|');
        }
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}

/// The names the category drop-down lists, in the servers' order.
pub(crate) fn category_names() -> Vec<String> {
    BugCategory::ALL
        .iter()
        .map(|category| category.name().to_owned())
        .collect()
}

#[cfg(test)]
mod tests;
