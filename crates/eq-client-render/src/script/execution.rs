//! Observable run state and requests shared by the script driver and its UI.

use std::{collections::VecDeque, sync::Arc, time::Duration};

use bevy::prelude::Resource;

const HISTORY_LIMIT: usize = 64;

/// What the script executor is doing, independent of its diagnostic log.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// No scenario has run in this client yet.
    #[default]
    Idle,
    /// Executing an action or waiting for its expected result.
    Running,
    /// Waiting for the attended client to regain focus.
    WaitingForFocus,
    /// Waiting for an explicitly background run to have exclusive input.
    WaitingForInput,
    /// Waiting for the physical pointer to leave before injecting a click.
    WaitingForPointer,
    /// Finishing the current action before pausing.
    Pausing,
    /// Paused between actions, with no synthetic input held.
    Paused,
    /// Waiting for complete lines appended to a followed script file.
    Following,
    /// Every queued action finished.
    Completed,
    /// The user stopped execution.
    Stopped,
    /// An action or runtime constraint stopped execution.
    Failed,
}

impl Phase {
    /// Whether a run still owns the executor, including a paused run.
    #[must_use]
    pub const fn active(self) -> bool {
        matches!(
            self,
            Self::Running
                | Self::WaitingForFocus
                | Self::WaitingForInput
                | Self::WaitingForPointer
                | Self::Pausing
                | Self::Paused
                | Self::Following
        )
    }
}

/// An entry in the bounded execution history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunEvent {
    /// One-based source line, absent for callers supplying actions without source.
    pub line: Option<usize>,
    /// The event's description; it never includes environment credentials.
    pub message: String,
}

/// The latest run's source, progress and result, retained after execution ends.
#[derive(Clone, Debug, Default, Resource)]
pub struct RunStatus {
    /// Stable for this execution, even when follow mode extends its source.
    pub(super) identity: Arc<()>,
    pub(super) source: Arc<str>,
    pub(super) phase: Phase,
    pub(super) line: Option<usize>,
    pub(super) started_steps: usize,
    pub(super) completed_steps: usize,
    pub(super) total: usize,
    pub(super) elapsed: Duration,
    pub(super) events: VecDeque<RunEvent>,
    pub(super) revision: u64,
    /// An immediate action also stays active until the next frame settles it.
    pub(super) step_pending: bool,
}

impl RunStatus {
    pub(super) fn queued(total: usize, source: Arc<str>) -> Self {
        Self {
            total,
            source,
            phase: Phase::Running,
            ..Self::default()
        }
    }

    /// The immutable source snapshot used by this run.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The executor's current phase or the last run's final outcome.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// The active or last executed action's one-based source line.
    #[must_use]
    pub const fn line(&self) -> Option<usize> {
        self.line
    }

    /// Actions that finished, and the total accepted so far.
    #[must_use]
    pub const fn progress(&self) -> (usize, usize) {
        (self.completed_steps, self.total)
    }

    /// Time spent in the run, excluding explicit pauses between actions.
    #[must_use]
    pub const fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Recent execution events, oldest first.
    #[must_use]
    pub fn events(&self) -> &VecDeque<RunEvent> {
        &self.events
    }

    pub(super) fn event(&mut self, message: impl Into<String>) {
        if self.events.len() == HISTORY_LIMIT {
            self.events.pop_front();
        }
        self.events.push_back(RunEvent {
            line: self.line,
            message: message.into(),
        });
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn begin(&mut self, line: Option<usize>) {
        self.line = line;
        self.started_steps += 1;
        self.step_pending = true;
        self.phase = Phase::Running;
        self.event(format!("Started step {}", self.started_steps));
    }

    pub(super) fn complete_step(&mut self) {
        if std::mem::take(&mut self.step_pending) {
            self.completed_steps += 1;
            self.event(format!("Finished step {}", self.completed_steps));
        }
    }

    pub(super) fn finish(&mut self, phase: Phase, message: impl Into<String>) {
        self.phase = phase;
        self.event(message);
    }
}

/// A control request consumed by the driver before it injects more input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    Pause,
    Resume,
    Step,
    Stop,
}

/// Pauses occur only at action boundaries, after held input has been released.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum RunMode {
    #[default]
    Continuous,
    PauseRequested,
    Paused(Duration),
    OneStep,
}

/// The UI's pending request. A stop takes precedence over another pending request.
#[derive(Default, Resource)]
pub(crate) struct RunControl(Option<Request>);

impl RunControl {
    pub(crate) fn request(&mut self, request: Request) {
        if self.0 != Some(Request::Stop) {
            self.0 = Some(request);
        }
    }

    pub(super) fn take(&mut self) -> Option<Request> {
        self.0.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopping_wins_over_a_later_resume() {
        let mut control = RunControl::default();
        control.request(Request::Stop);
        control.request(Request::Resume);
        assert_eq!(control.take(), Some(Request::Stop));
        assert_eq!(control.take(), None);
    }

    #[test]
    fn completing_an_action_twice_does_not_advance_progress_twice() {
        let mut status = RunStatus::queued(2, Arc::from("# note\nreport sample\n"));
        status.begin(Some(2));
        status.complete_step();
        status.complete_step();
        assert_eq!(status.progress(), (1, 2));
        assert_eq!(status.line(), Some(2));
        status.finish(Phase::Failed, "Expected result did not arrive");
        assert!(!status.phase().active());
        assert_eq!(status.events().back().unwrap().line, Some(2));
    }

    #[test]
    fn execution_history_is_bounded() {
        let mut status = RunStatus::default();
        for index in 0..200 {
            status.event(format!("step {index}"));
        }
        assert_eq!(status.events().len(), HISTORY_LIMIT);
        assert_eq!(status.events().back().unwrap().message, "step 199");
    }
}
