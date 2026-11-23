//! `/petition`, `/guidehelp`, `/bugreport` and `/feedback`: a report to the
//! server's staff ([`eq_client_core::reports`]), recording checklist 6.511.
//!
//! A line typed by anyone reaches here: the keyboard, a social or a script's
//! `slash` step, as a script may do whatever the player can. Where the
//! session does not offer the report, the line answers "This client does
//! not take /petition that way yet." The session paces each kind.
//!
//! On Titanium each command follows the program (read R49-23):
//! - `/petition` (0x4683af) takes the form the world sets
//!   ([`eq_client_core::world::ClientWorld::petition_form`]). In its text
//!   form it sends the words and echoes them in the installed line 0x3025;
//!   with no words it prints 0x3332 and sends nothing. `/guidehelp`
//!   (0x46844e) is the same text form, with lines 0xde8 and 0xde3.
//! - `/bugreport` and `/feedback` open the program's windows and ignore the
//!   words. This client does not draw those windows yet, so the **Typed
//!   Reports** setting (on until they are built) sends the words instead:
//!   a bug report then prints 0x22ae, or 0x22b0 with no words, and feedback
//!   prints nothing, as the program's windows do.
//!
//! A bug report's first word may name its category with a colon
//! ("pathing: the guard walks into the wall"); a word that names no
//! category leaves the whole line as the text, under Other. Bug reports and
//! feedback carry the player's place and heading, and a bug report the
//! target where there is one. The `EQMac` and `RoF2` programs are unread, so
//! they keep this client's own words.
use super::chat::Said;
use super::hud::messages::Messages;
use eq_client_core::{
    Capability, ClientCommand, GameDialect,
    reports::{BugCategory, BugPlace, BugReport, BugTarget, PetitionForm},
    world::ClientWorld,
};

// The lines are the installation's, by their ids; the fallbacks are this
// client's own words where an installation lacks them.

/// The Titanium program's petition echo, with the text (read R49-23).
const PETITION_SENT: u32 = 0x3025;
/// The Titanium program's line for `/petition` with no words.
const PETITION_EMPTY: u32 = 0x3332;
/// The Titanium program's `/guidehelp` echo, with the text.
const GUIDE_SENT: u32 = 0xde8;
/// The Titanium program's line for `/guidehelp` with no words.
const GUIDE_EMPTY: u32 = 0xde3;
/// The Titanium program's line after its bug report window sends.
const BUG_SENT: u32 = 0x22ae;
/// The Titanium program's line for its bug report window's empty box.
const BUG_EMPTY: u32 = 0x22b0;
/// The chat colour of the program's lines here; the petition echoes use 0.
const PROGRAM_COLOUR: u32 = 0xf;

/// What a typed line asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// A petition, with its words.
    Petition(String),
    /// A petition by `/guidehelp`, with its words.
    GuideHelp(String),
    /// A bug report, with its words.
    Bug(String),
    /// Feedback, with its words.
    Feedback(String),
}

impl Asked {
    /// What a typed, spelled-out line asks, if it is one of these.
    pub(crate) fn typed(input: &str) -> Option<Self> {
        let command = input.strip_prefix('/')?;
        let (name, words) = command
            .split_once(char::is_whitespace)
            .unwrap_or((command, ""));
        let words = words.trim().to_owned();
        match name.to_ascii_lowercase().as_str() {
            "petition" => Some(Self::Petition(words)),
            "guidehelp" => Some(Self::GuideHelp(words)),
            "bugreport" => Some(Self::Bug(words)),
            "feedback" => Some(Self::Feedback(words)),
            _ => None,
        }
    }

    const fn name(&self) -> &'static str {
        match self {
            Self::Petition(_) => "petition",
            Self::GuideHelp(_) => "guidehelp",
            Self::Bug(_) => "bugreport",
            Self::Feedback(_) => "feedback",
        }
    }

    const fn capability(&self) -> Capability {
        match self {
            Self::Petition(_) | Self::GuideHelp(_) => Capability::Petitioning,
            Self::Bug(_) => Capability::ReportingBugs,
            Self::Feedback(_) => Capability::SendingFeedback,
        }
    }

    fn words(&self) -> &str {
        match self {
            Self::Petition(words)
            | Self::GuideHelp(words)
            | Self::Bug(words)
            | Self::Feedback(words) => words,
        }
    }
}

/// Whether the generation's program has been read for these commands
/// (read R49-23): only Titanium's.
const fn reads_r49_23(dialect: GameDialect) -> bool {
    match dialect {
        GameDialect::Titanium => true,
        GameDialect::EqMac | GameDialect::Rof2 => false,
    }
}

/// A line a report command prints: an installed line by its id, with this
/// client's words where the installation lacks it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReportLine {
    id: u32,
    arguments: Vec<String>,
    colour: u32,
    fallback: &'static str,
}

impl ReportLine {
    const fn new(id: u32, colour: u32, fallback: &'static str) -> Self {
        Self {
            id,
            arguments: Vec::new(),
            colour,
            fallback,
        }
    }

    /// The line as the chat says it.
    pub(crate) fn said(&self, messages: Option<&Messages>) -> Said {
        messages
            .map_or_else(
                || Said::own(self.fallback),
                |messages| messages.said_or(self.id, &self.arguments, self.fallback),
            )
            .typed(self.colour)
    }
}

/// Prints the lines report commands asked for.
#[allow(clippy::needless_pass_by_value)] // Bevy system parameters are value wrappers.
pub(super) fn say(
    mut chat: bevy::prelude::ResMut<super::chat::ChatState>,
    messages: Option<bevy::prelude::Res<Messages>>,
) {
    if chat.report_lines.is_empty() {
        return;
    }
    let lines = std::mem::take(&mut chat.report_lines);
    for line in lines {
        chat.history
            .push(super::chat::system_line(line.said(messages.as_deref())));
    }
}

/// What a typed report does.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Answer {
    /// Sends the command, then, once the session says it went, prints the
    /// echo where the program has one.
    Send(Box<ClientCommand>, Option<ReportLine>),
    /// Prints a line and sends nothing.
    Say(ReportLine),
    /// Does nothing, as the program's empty feedback window does.
    Nothing,
}

/// A bug report's category and text from its words: a leading word and a
/// colon naming a category, else the whole line under Other.
fn category(words: &str) -> (BugCategory, &str) {
    words
        .split_once(':')
        .and_then(|(word, rest)| {
            BugCategory::from_word(word).map(|category| (category, rest.trim()))
        })
        .unwrap_or((BugCategory::Other, words))
}

/// Whether the session offers the report; where it does not, the line
/// that answers today.
fn offered(asked: &Asked, world: &ClientWorld) -> Result<(), String> {
    if world.can(asked.capability()) {
        Ok(())
    } else {
        Err(format!(
            "This client does not take /{} that way yet.",
            asked.name()
        ))
    }
}

/// What a typed report does in this world, or the line that answers it
/// instead. `typed_reports` is the Typed Reports setting.
pub(crate) fn answer(
    asked: &Asked,
    world: &ClientWorld,
    stamp: crate::outbox::Stamp,
    typed_reports: bool,
) -> Result<Answer, String> {
    let program = reads_r49_23(world.generation());
    if matches!(asked, Asked::GuideHelp(_)) && !program {
        return Err("This client does not take /guidehelp that way yet.".to_owned());
    }
    if matches!(asked, Asked::Bug(_) | Asked::Feedback(_)) && !typed_reports {
        // The program opens its window and ignores the words; this client
        // has no window yet (6.511, C1b).
        return Err(format!(
            "This client has no /{} window yet. Turn on Typed Reports to send one from the line.",
            asked.name()
        ));
    }
    if let Asked::Petition(_) = asked {
        match world.petition_form() {
            PetitionForm::Text => {}
            // TODO(6.511, C1b): the program shows or hides its petition
            // window here ([0x9928a8]) and ignores the words. Which skin
            // screen that is is unread, so this client takes the text form:
            // a departure, labelled.
            PetitionForm::Window => bevy::log::debug!(
                "The world turns the petition window on; this client has none yet, so the petition takes the text form (6.511 departure)"
            ),
        }
    }
    let empty = match asked {
        Asked::Bug(words) => category(words).1.is_empty(),
        _ => asked.words().is_empty(),
    };
    if empty {
        if !program {
            // This client's own words; that program's lines are unread.
            return Err(format!("Use /{} with what to say.", asked.name()));
        }
        return Ok(match asked {
            Asked::Petition(_) => Answer::Say(ReportLine::new(
                PETITION_EMPTY,
                PROGRAM_COLOUR,
                "Use /petition with what to say.",
            )),
            Asked::GuideHelp(_) => Answer::Say(ReportLine::new(
                GUIDE_EMPTY,
                PROGRAM_COLOUR,
                "Use /guidehelp with what to say.",
            )),
            Asked::Bug(_) => Answer::Say(ReportLine::new(
                BUG_EMPTY,
                PROGRAM_COLOUR,
                "Say what the bug report is about.",
            )),
            Asked::Feedback(_) => Answer::Nothing,
        });
    }
    offered(asked, world)?;
    let echo = program.then(|| echo(asked)).flatten();
    Ok(Answer::Send(Box::new(command(asked, world, stamp)), echo))
}

/// The line the program prints once a report goes, where it prints one.
fn echo(asked: &Asked) -> Option<ReportLine> {
    let (id, colour, fallback) = match asked {
        Asked::Petition(_) => (PETITION_SENT, 0, "Your petition was sent."),
        // The echo's colour is inferred to match the petition's.
        Asked::GuideHelp(_) => (GUIDE_SENT, 0, "Your request for a guide was sent."),
        Asked::Bug(_) => (BUG_SENT, PROGRAM_COLOUR, "Your bug report was sent."),
        Asked::Feedback(_) => return None,
    };
    let mut line = ReportLine::new(id, colour, fallback);
    if matches!(asked, Asked::Petition(_) | Asked::GuideHelp(_)) {
        line.arguments.push(asked.words().to_owned());
    }
    Some(line)
}

/// Where the player stands, when the client knows.
fn place(world: &ClientWorld) -> Option<BugPlace> {
    world.player().map(|player| BugPlace {
        x: player.position.x,
        y: player.position.y,
        z: player.position.z,
        heading: player.position.heading,
    })
}

/// The command a typed report sends, for this admission.
fn command(asked: &Asked, world: &ClientWorld, stamp: crate::outbox::Stamp) -> ClientCommand {
    match asked {
        Asked::Petition(words) | Asked::GuideHelp(words) => ClientCommand::SendPetition {
            session_id: stamp.session_id,
            text: words.clone(),
            created: stamp.created,
        },
        Asked::Feedback(words) => ClientCommand::SendFeedback {
            session_id: stamp.session_id,
            text: words.clone(),
            place: place(world),
            created: stamp.created,
        },
        Asked::Bug(words) => {
            let (category, text) = category(words);
            ClientCommand::ReportBug {
                session_id: stamp.session_id,
                report: BugReport {
                    category,
                    text: text.to_owned(),
                    place: place(world),
                    target: world.target_intent().and_then(|target| {
                        let spawn_id = target.spawn_id?;
                        let spawn = world.spawn(spawn_id)?;
                        Some(BugTarget {
                            spawn_id,
                            name: spawn.state.name.clone(),
                        })
                    }),
                },
                created: stamp.created,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_lines_name_their_report_and_keep_their_words() {
        assert_eq!(
            Asked::typed("/petition  I am stuck. "),
            Some(Asked::Petition("I am stuck.".into()))
        );
        assert_eq!(
            Asked::typed("/BugReport pathing: the guard"),
            Some(Asked::Bug("pathing: the guard".into()))
        );
        assert_eq!(
            Asked::typed("/feedback"),
            Some(Asked::Feedback(String::new()))
        );
        assert_eq!(
            Asked::typed("/GuideHelp  where is the bank"),
            Some(Asked::GuideHelp("where is the bank".into()))
        );
        assert_eq!(Asked::typed("/pet attack"), None);
        assert_eq!(Asked::typed("petition"), None);
    }

    #[test]
    fn a_leading_category_word_needs_its_colon_and_a_known_name() {
        assert_eq!(
            category("pathing: the guard walks into the wall"),
            (BugCategory::Pathing, "the guard walks into the wall")
        );
        assert_eq!(
            category("Doors: it will not open"),
            (BugCategory::DoorsAndPortals, "it will not open")
        );
        // Not a category: the whole line is the text, under Other.
        assert_eq!(
            category("Note: the guard walks into the wall"),
            (BugCategory::Other, "Note: the guard walks into the wall")
        );
        assert_eq!(
            category("pathing the guard"),
            (BugCategory::Other, "pathing the guard")
        );
    }

    /// A Titanium world with the player admitted, and a stamp for it.
    fn admitted() -> (crate::online::OnlineState, crate::outbox::Stamp) {
        let (sender, _receiver) = std::sync::mpsc::sync_channel(4);
        let mut online = crate::online::OnlineState::new(true);
        crate::online::testing::admit(&mut online, 1, crate::online::testing::player(1));
        let outbox = crate::outbox::Outbox::new(Some(sender));
        let stamp = outbox.stamp(online.world()).unwrap();
        (online, stamp)
    }

    #[test]
    fn with_typed_reports_off_a_bug_report_and_feedback_send_nothing() {
        let (online, stamp) = admitted();
        let world = online.world();
        assert_eq!(world.generation(), GameDialect::Titanium);
        for asked in [
            Asked::Bug("the guard walks into the wall".into()),
            Asked::Feedback("a note".into()),
        ] {
            let refused = answer(&asked, world, stamp, false).unwrap_err();
            assert!(refused.contains("Typed Reports"), "{refused}");
            assert!(matches!(
                answer(&asked, world, stamp, true),
                Ok(Answer::Send(..))
            ));
        }
        // A petition is the program's own text form either way.
        assert!(matches!(
            answer(&Asked::Petition("I am stuck".into()), world, stamp, false),
            Ok(Answer::Send(..))
        ));
    }

    #[test]
    fn a_sent_report_carries_the_programs_echo_by_its_id() {
        let (online, stamp) = admitted();
        let world = online.world();
        let echo = |asked: Asked| match answer(&asked, world, stamp, true) {
            Ok(Answer::Send(_, echo)) => echo,
            other => panic!("{other:?}"),
        };
        let petition = echo(Asked::Petition("I am stuck".into())).unwrap();
        assert_eq!(
            (petition.id, petition.colour, petition.arguments.as_slice()),
            (PETITION_SENT, 0, ["I am stuck".to_owned()].as_slice())
        );
        let guide = echo(Asked::GuideHelp("where is the bank".into())).unwrap();
        assert_eq!((guide.id, guide.colour), (GUIDE_SENT, 0));
        let bug = echo(Asked::Bug("pathing: the guard".into())).unwrap();
        assert_eq!((bug.id, bug.colour), (BUG_SENT, PROGRAM_COLOUR));
        assert_eq!(bug.arguments, Vec::<String>::new());
        // The program's feedback window prints nothing once it sends.
        assert_eq!(echo(Asked::Feedback("a note".into())), None);
        // Without an installation's lines, this client's words stand in.
        assert_eq!(bug.said(None).words.text, "Your bug report was sent.");
    }
}
