//! How long the player lets a run go (checklist 6.594). Only the workbench's
//! selector and `--script-run-length` choose it; no statement or file header
//! can set or raise it. Every length keeps the same paces, per-step waits,
//! input rules and Stop: it lifts only the run's overall counts.

use std::time::Duration;

/// The default run's time cap, as before the player could choose.
pub(super) const DEFAULT_RUNTIME: Duration = Duration::from_mins(15);
/// The longest timed run the player can ask for: one day.
pub const MAX_MINUTES: u16 = 1440;
/// The timed lengths the workbench's selector steps through, in minutes.
const OFFERED_MINUTES: [u16; 6] = [30, 60, 120, 240, 480, MAX_MINUTES];

/// A run's length, the player's choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bevy::prelude::Resource)]
pub enum RunLength {
    /// The bounds every run had before: 15 minutes, 5,000 statements, 500
    /// actions and loops of 100.
    #[default]
    Default,
    /// The given minutes, from 1 to [`MAX_MINUTES`], with no count of
    /// statements, actions or loop iterations.
    Minutes(u16),
    /// No limit of time or count: the run goes until it ends or Stop.
    Unlimited,
}

impl RunLength {
    /// Reads `default`, `none` or a number of minutes, as
    /// `--script-run-length` takes them.
    ///
    /// # Errors
    ///
    /// Anything else, or minutes outside 1 to [`MAX_MINUTES`].
    pub fn parse(text: &str) -> Result<Self, String> {
        match text.trim() {
            "default" => Ok(Self::Default),
            "none" => Ok(Self::Unlimited),
            minutes => minutes
                .parse::<u16>()
                .ok()
                .filter(|minutes| (1..=MAX_MINUTES).contains(minutes))
                .map(Self::Minutes)
                .ok_or_else(|| {
                    format!("a run length is default, none or minutes from 1 to {MAX_MINUTES}")
                }),
        }
    }

    /// The run's time cap, if it has one. Explicit pauses don't count.
    #[must_use]
    pub const fn runtime(self) -> Option<Duration> {
        match self {
            Self::Default => Some(DEFAULT_RUNTIME),
            Self::Minutes(minutes) => Some(Duration::from_mins(minutes as u64)),
            Self::Unlimited => None,
        }
    }

    /// Whether the run counts its statements, actions and loop iterations
    /// against the default bounds.
    #[must_use]
    pub const fn counted(self) -> bool {
        matches!(self, Self::Default)
    }

    /// The next choice on the workbench's selector: the default, then each
    /// offered time, then no limit, then the default again.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Default => Self::Minutes(OFFERED_MINUTES[0]),
            Self::Minutes(minutes) => OFFERED_MINUTES
                .iter()
                .find(|offered| **offered > minutes)
                .map_or(Self::Unlimited, |offered| Self::Minutes(*offered)),
            Self::Unlimited => Self::Default,
        }
    }

    /// How long the run may go, for the Execution tab's status line.
    #[must_use]
    pub fn span(self) -> String {
        match self {
            Self::Default => "15 min".into(),
            Self::Minutes(minutes) => format!("{minutes} min"),
            Self::Unlimited => "no limit".into(),
        }
    }

    /// The selector's label.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Default => "Length: default (15 min)".into(),
            Self::Minutes(minutes) if minutes % 60 == 0 => {
                format!("Length: {} h", minutes / 60)
            }
            Self::Minutes(minutes) => format!("Length: {minutes} min"),
            Self::Unlimited => "Length: no limit".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_MINUTES, RunLength};
    use std::time::Duration;

    #[test]
    fn the_flag_reads_default_none_and_minutes() {
        assert_eq!(RunLength::parse("default"), Ok(RunLength::Default));
        assert_eq!(RunLength::parse("none"), Ok(RunLength::Unlimited));
        assert_eq!(RunLength::parse("30"), Ok(RunLength::Minutes(30)));
        assert_eq!(
            RunLength::parse("1440"),
            Ok(RunLength::Minutes(MAX_MINUTES))
        );
        for refused in ["0", "1441", "-5", "", "forever", "1.5"] {
            assert!(RunLength::parse(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn only_the_default_counts_and_each_time_caps() {
        assert_eq!(RunLength::Default.runtime(), Some(Duration::from_mins(15)));
        assert_eq!(
            RunLength::Minutes(30).runtime(),
            Some(Duration::from_mins(30))
        );
        assert_eq!(RunLength::Unlimited.runtime(), None);
        assert!(RunLength::Default.counted());
        assert!(!RunLength::Minutes(30).counted());
        assert!(!RunLength::Unlimited.counted());
    }

    #[test]
    fn the_selector_steps_round_to_the_default() {
        let mut length = RunLength::Default;
        let mut seen = Vec::new();
        for _ in 0..8 {
            length = length.next();
            seen.push(length);
        }
        assert_eq!(
            seen,
            [
                RunLength::Minutes(30),
                RunLength::Minutes(60),
                RunLength::Minutes(120),
                RunLength::Minutes(240),
                RunLength::Minutes(480),
                RunLength::Minutes(1440),
                RunLength::Unlimited,
                RunLength::Default,
            ]
        );
        // A length from the flag steps to the next offered one.
        assert_eq!(RunLength::Minutes(45).next(), RunLength::Minutes(60));
        assert_eq!(RunLength::Minutes(90).label(), "Length: 90 min");
        assert_eq!(RunLength::Minutes(120).label(), "Length: 2 h");
    }
}
