//! Tracking's window and following as the Titanium program keeps them (read
//! R14 section 2, R15a): a press opens the window and empties it until the
//! server's list comes; Track follows an entry and closes the window; the
//! session's lines say where the followed spawn lies and when following
//! ends. The zone keeps all of it, so leaving the zone ends it silently, as
//! the program's zoning does.
use super::{Changes, ClientWorld, Notice};
use crate::combat::ConColor;
use crate::tracking::{Tracked, TrackingUpdate};

/// The track window and what is followed, for the zone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackWindow {
    /// Whether the window is open.
    pub open: bool,
    /// The server's last list, in its order; empty from a press until it
    /// comes.
    pub entries: Vec<Tracked>,
    /// The entry chosen, kept across a refresh while it is listed.
    pub selected: Option<u16>,
    /// The spawn followed, from Track until following ends.
    pub following: Option<u16>,
    /// Changes whenever the list or the choice does, so the window draws
    /// its rows again only then.
    pub revision: u64,
}

/// An entry's colour in the track window by the program's own rule (read R14
/// section 2), from the level difference d, theirs less yours: green below
/// the green threshold (three quarters of your level up to 60, your level
/// less 15 above it) when d is under -3, blue for any other d under 0, white
/// at 0, yellow under 3, and red beyond. The program's light blue near the
/// green edge for levels above 7 is not read closely enough to draw, so
/// those entries are blue (recording checklist 6.444). The threshold's
/// rounding (three quarters taken in whole levels, rounded down) and its
/// edge (green only strictly below it) are inferred, under the same label.
/// Consider's colours follow their own rule; the two are not one rule.
#[must_use]
pub fn track_color(yours: u8, theirs: u8) -> ConColor {
    let difference = i16::from(theirs) - i16::from(yours);
    let threshold = if yours <= 60 {
        i16::from(yours) * 3 / 4
    } else {
        i16::from(yours) - 15
    };
    match difference {
        _ if i16::from(theirs) < threshold && difference < -3 => ConColor::Green,
        ..0 => ConColor::Blue,
        0 => ConColor::White,
        1..3 => ConColor::Yellow,
        _ => ConColor::Red,
    }
}

/// A line's arguments with the spawn named first as players see names; the
/// rest, such as a side word's string number, as the session gave them.
fn shown(arguments: &[String]) -> Vec<String> {
    let mut shown = arguments.to_vec();
    match shown.first_mut() {
        Some(name) => *name = crate::entities::display_name(name),
        None => shown.push(String::new()),
    }
    shown
}

impl ClientWorld {
    /// The track window and what is followed.
    #[must_use]
    pub fn track_window(&self) -> &TrackWindow {
        &self.zone.tracking
    }

    /// Chooses an entry of the window, or none.
    pub fn select_tracked(&mut self, spawn_id: Option<u16>) {
        let tracking = &mut self.zone.tracking;
        let selected =
            spawn_id.filter(|id| tracking.entries.iter().any(|entry| entry.spawn_id == *id));
        if tracking.selected != selected {
            tracking.selected = selected;
            tracking.revision += 1;
        }
    }

    /// Closes the window, as its close button or `/stoptracking` does.
    pub fn close_track_window(&mut self) {
        self.zone.tracking.open = false;
    }

    /// The press reached the server: the window opens, emptied until the
    /// list comes.
    pub(super) fn tracking_pressed(&mut self) {
        let tracking = &mut self.zone.tracking;
        tracking.open = true;
        tracking.entries.clear();
        tracking.revision += 1;
    }

    /// What the session said of tracking.
    pub(super) fn tracking_news(&mut self, update: &TrackingUpdate, changes: &mut Changes) {
        let tracking = &mut self.zone.tracking;
        match update {
            TrackingUpdate::Listed(entries) => {
                tracking.entries.clone_from(entries);
                let listed = |id: &u16| entries.iter().any(|entry| entry.spawn_id == *id);
                tracking.selected = tracking.selected.filter(listed);
                tracking.revision += 1;
            }
            TrackingUpdate::Following {
                spawn_id,
                string_id,
                arguments,
            } => {
                tracking.following = Some(*spawn_id);
                tracking.open = false;
                let arguments = shown(arguments);
                let said = format!("Tracking: following {}", arguments[0]);
                changes.notices.push(Notice::ProgramLine {
                    string_id: *string_id,
                    arguments,
                    lines: Vec::new(),
                    said,
                });
            }
            TrackingUpdate::Bearing {
                string_id,
                arguments,
            } => {
                let arguments = shown(arguments);
                let band = crate::tracking::lines::BEARINGS
                    .iter()
                    .position(|line| line == string_id)
                    .unwrap_or_default();
                let lies = [
                    "straight ahead",
                    "ahead to one side",
                    "to one side",
                    "behind to one side",
                    "straight behind",
                ][band];
                let said = format!("Tracking: {} {lies}", arguments[0]);
                // The side word, where the band has one, is a line of its
                // own that the bearing names by its words.
                let words = (arguments.len() > 1).then_some(1).into_iter().collect();
                changes.notices.push(Notice::ProgramLine {
                    string_id: *string_id,
                    arguments,
                    lines: words,
                    said,
                });
            }
            TrackingUpdate::Ended { string_id } => {
                tracking.following = None;
                if let Some(string_id) = string_id {
                    let said = if *string_id == crate::tracking::lines::EXPIRED {
                        "Tracking ended: time limit"
                    } else {
                        "Tracking ended: target gone"
                    };
                    changes.notices.push(Notice::ProgramLine {
                        string_id: *string_id,
                        arguments: Vec::new(),
                        lines: Vec::new(),
                        said: said.to_owned(),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_track_colours_follow_the_programs_level_rule() {
        // Level 20: the green threshold is 15.
        assert_eq!(track_color(20, 14), ConColor::Green);
        assert_eq!(track_color(20, 15), ConColor::Blue);
        assert_eq!(track_color(20, 19), ConColor::Blue);
        assert_eq!(track_color(20, 20), ConColor::White);
        assert_eq!(track_color(20, 22), ConColor::Yellow);
        assert_eq!(track_color(20, 23), ConColor::Red);
        // Low levels: d must also be under -3.
        assert_eq!(track_color(4, 1), ConColor::Blue);
        assert_eq!(track_color(8, 4), ConColor::Green);
        // Above 60 the threshold is the level less 15.
        assert_eq!(track_color(65, 49), ConColor::Green);
        assert_eq!(track_color(65, 50), ConColor::Blue);
    }
}
