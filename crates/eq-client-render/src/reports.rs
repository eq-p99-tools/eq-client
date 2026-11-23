//! `/petition`, `/bugreport` and `/feedback`: a report to the server's
//! staff ([`eq_client_core::reports`]), recording checklist 6.511.
//!
//! A person reads each report, so only a line the player types sends one.
//! A social, a hotbutton, a button or a script step reaches the ordinary
//! dispatch, which answers as it does today ("This client does not take
//! /petition that way yet."). Where the session does not offer the report,
//! a typed line answers the same.
//!
//! The official client opens windows for some of these, and its words for
//! them are unread (6.511a, b). Here the words follow the command on the
//! line. A bug report's first word may name its category with a colon
//! ("pathing: the guard walks into the wall"). A word that names no
//! category leaves the whole line as the text, under Other. The report
//! carries the player's place and heading, and the target where there is
//! one.
use eq_client_core::{
    Capability, ClientCommand,
    reports::{BugCategory, BugPlace, BugReport, BugTarget},
    world::ClientWorld,
};

/// What a typed line asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// A petition, with its words.
    Petition(String),
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
            "bugreport" => Some(Self::Bug(words)),
            "feedback" => Some(Self::Feedback(words)),
            _ => None,
        }
    }

    const fn name(&self) -> &'static str {
        match self {
            Self::Petition(_) => "petition",
            Self::Bug(_) => "bugreport",
            Self::Feedback(_) => "feedback",
        }
    }

    const fn capability(&self) -> Capability {
        match self {
            Self::Petition(_) => Capability::Petitioning,
            Self::Bug(_) => Capability::ReportingBugs,
            Self::Feedback(_) => Capability::SendingFeedback,
        }
    }

    fn words(&self) -> &str {
        match self {
            Self::Petition(words) | Self::Bug(words) | Self::Feedback(words) => words,
        }
    }
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
pub(crate) fn offered(asked: &Asked, world: &ClientWorld) -> Result<(), String> {
    if world.can(asked.capability()) {
        Ok(())
    } else {
        Err(format!(
            "This client does not take /{} that way yet.",
            asked.name()
        ))
    }
}

/// The command a typed report sends, for this admission, or the line that
/// answers it instead.
pub(crate) fn command(
    asked: &Asked,
    world: &ClientWorld,
    stamp: crate::outbox::Stamp,
) -> Result<ClientCommand, String> {
    let empty = match asked {
        Asked::Bug(words) => category(words).1.is_empty(),
        _ => asked.words().is_empty(),
    };
    if empty {
        // This client's own words; the official usage line is unread.
        return Err(format!("Use /{} with what to say.", asked.name()));
    }
    Ok(match asked {
        Asked::Petition(words) => ClientCommand::SendPetition {
            session_id: stamp.session_id,
            text: words.clone(),
            created: stamp.created,
        },
        Asked::Feedback(words) => ClientCommand::SendFeedback {
            session_id: stamp.session_id,
            text: words.clone(),
            created: stamp.created,
        },
        Asked::Bug(words) => {
            let (category, text) = category(words);
            ClientCommand::ReportBug {
                session_id: stamp.session_id,
                report: BugReport {
                    category,
                    text: text.to_owned(),
                    place: world.player().map(|player| BugPlace {
                        x: player.position.x,
                        y: player.position.y,
                        z: player.position.z,
                        heading: player.position.heading,
                    }),
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
    })
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
}
