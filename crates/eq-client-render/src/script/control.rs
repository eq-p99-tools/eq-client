//! Opt-in local control of the existing executor. No gameplay is implemented here.
mod files;
mod protocol;
mod runbook;

use std::{collections::VecDeque, path::PathBuf, sync::Arc, time::Duration};

use bevy::{input_focus::InputFocus, prelude::*};
use serde_json::{Value, json};

use super::execution::FlowStatus;
use super::{Phase, Program, Request, RunControl, RunStatus, Script};
use crate::background::Background;
use protocol::{Envelope, Operation};

const POLL: Duration = Duration::from_millis(250);
const HISTORY: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Receipt {
    id: u64,
    outcome: &'static str,
    code: &'static str,
    run_id: Option<u64>,
    line: Option<usize>,
    message: Option<String>,
}

impl Receipt {
    fn new(id: u64, outcome: &'static str, code: &'static str, run_id: Option<u64>) -> Self {
        Self {
            id,
            outcome,
            code,
            run_id,
            line: None,
            message: None,
        }
    }

    fn rejected(id: u64, code: &'static str) -> Self {
        Self::new(id, "rejected", code, None)
    }

    fn json(&self) -> Value {
        json!({"id":self.id,"outcome":self.outcome,"code":self.code,
            "run_id":self.run_id,"line":self.line,"message":self.message})
    }
}

#[derive(Clone, Debug)]
struct Snapshot {
    id: u64,
    origin: &'static str,
    phase: Phase,
    queued: bool,
    line: Option<usize>,
    completed: usize,
    authored_actions: usize,
    executed_statements: Option<usize>,
    elapsed_ms: u64,
    message: Option<String>,
}

impl Snapshot {
    fn update(&mut self, status: &RunStatus, queued: bool) {
        self.phase = status.phase();
        self.queued = queued && self.phase.active();
        self.line = status.line();
        (self.completed, self.authored_actions) = status.progress();
        self.executed_statements = status.flow().map(FlowStatus::statements);
        self.elapsed_ms = u64::try_from(status.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.message = status
            .events()
            .back()
            .map(|event| protocol::message(&event.message));
    }

    fn terminal(&self) -> bool {
        !self.phase.active() && self.phase != Phase::Idle
    }

    fn json(&self) -> Value {
        let phase = if self.queued {
            "queued"
        } else {
            phase_name(self.phase)
        };
        let total = self
            .executed_statements
            .is_none()
            .then_some(self.authored_actions);
        let mut result = json!({"id":self.id,"origin":self.origin,"phase":phase,"line":self.line,
            "completed_steps":self.completed,"total_steps":total,
            "elapsed_ms":self.elapsed_ms,"message":self.message});
        if let Some(statements) = self.executed_statements {
            result["authored_actions"] = json!(self.authored_actions);
            result["executed_statements"] = json!(statements);
        }
        result
    }
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Idle => "idle",
        Phase::Running => "running",
        Phase::WaitingForFocus => "waiting_for_focus",
        Phase::WaitingForInput => "waiting_for_input",
        Phase::WaitingForPointer => "waiting_for_pointer",
        Phase::Pausing => "pausing",
        Phase::Paused => "paused",
        Phase::Following => "following",
        Phase::Completed => "completed",
        Phase::Stopped => "stopped",
        Phase::Failed => "failed",
    }
}

struct Tracked {
    identity: Arc<()>,
    snapshot: Snapshot,
    archived: bool,
}

/// The active local control channel. Its presence enables the Runbook menu entry.
#[derive(Resource)]
pub(crate) struct Bridge {
    root: PathBuf,
    instance: String,
    closed: bool,
    writer: files::Writer,
    next_poll: Duration,
    revision: u64,
    next_run: u64,
    processed: Option<(Envelope, Receipt)>,
    receipt: Option<Receipt>,
    current: Option<Tracked>,
    runbook: Option<runbook::Run>,
    history: VecDeque<Snapshot>,
    protocol_error: Option<String>,
    io_error: Option<String>,
    job: Background<Result<Option<Vec<u8>>, String>>,
}

impl Bridge {
    fn new(root: PathBuf, instance: String) -> Self {
        let writer = files::Writer::new(&root);
        Self {
            root,
            instance,
            closed: false,
            writer,
            next_poll: Duration::ZERO,
            revision: 0,
            next_run: 0,
            processed: None,
            receipt: None,
            current: None,
            runbook: None,
            history: VecDeque::new(),
            protocol_error: None,
            io_error: None,
            job: Background::default(),
        }
    }

    fn archive(&mut self) {
        let Some(current) = &mut self.current else {
            return;
        };
        if current.archived || !current.snapshot.terminal() {
            return;
        }
        if self.history.len() == HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(current.snapshot.clone());
        current.archived = true;
    }

    fn bind(&mut self, status: &RunStatus, origin: &'static str) -> Option<u64> {
        let id = self.next_run.checked_add(1)?;
        if let Some(current) = &mut self.current
            && !current.snapshot.terminal()
        {
            current.snapshot.phase = Phase::Failed;
            current.snapshot.queued = false;
            current.snapshot.message =
                Some("Executor replaced before its terminal outcome was observed".into());
        }
        self.archive();
        self.next_run = id;
        let mut snapshot = Snapshot {
            id,
            origin,
            phase: Phase::Idle,
            queued: false,
            line: None,
            completed: 0,
            authored_actions: 0,
            executed_statements: None,
            elapsed_ms: 0,
            message: None,
        };
        snapshot.update(status, true);
        self.current = Some(Tracked {
            identity: status.identity.clone(),
            snapshot,
            archived: false,
        });
        Some(id)
    }

    fn observe(&mut self, status: &RunStatus, queued: bool) {
        if status.phase() == Phase::Idle {
            return;
        }
        if !self
            .current
            .as_ref()
            .is_some_and(|run| Arc::ptr_eq(&run.identity, &status.identity))
            && self.bind(status, "other").is_none()
        {
            self.protocol_error =
                Some("Run identifiers exhausted; control cannot name another run".into());
            return;
        }
        if let Some(current) = &mut self.current {
            current.snapshot.update(status, queued);
        }
        self.archive();
    }

    /// A consumed id can never dispatch again, even after its earlier run ends.
    fn fresh(&mut self, request: &Envelope) -> bool {
        self.protocol_error = None;
        let rejected = if request.instance != self.instance {
            Some(Receipt::rejected(request.id, "wrong_instance"))
        } else if let Some((previous, receipt)) = &self.processed {
            match request.id.cmp(&previous.id) {
                std::cmp::Ordering::Equal => Some(if request == previous {
                    receipt.clone()
                } else {
                    Receipt::rejected(request.id, "id_conflict")
                }),
                std::cmp::Ordering::Less => Some(Receipt::rejected(request.id, "stale_id")),
                std::cmp::Ordering::Greater => None,
            }
        } else {
            None
        };
        if let Some(receipt) = rejected {
            self.receipt = Some(receipt);
            false
        } else {
            true
        }
    }

    fn finish_request(&mut self, request: Envelope, receipt: Receipt) {
        self.receipt = Some(receipt.clone());
        self.processed = Some((request, receipt));
    }

    fn snapshot(&self) -> Vec<u8> {
        serde_json::to_vec(&json!({"version":protocol::VERSION,"instance":self.instance,
            "client_state":if self.closed { "closed" } else { "open" },
            "revision":self.revision,"last_processed_id":self.processed.as_ref().map_or(0, |(request, _)| request.id),
            "last_request":self.receipt.as_ref().map(Receipt::json),
            "run":self.current.as_ref().map(|run| run.snapshot.json()),
            "runbook":self.runbook.as_ref().map(runbook::Run::json),
            "recent_runs":self.history.iter().map(Snapshot::json).collect::<Vec<_>>(),
            "protocol_error":self.protocol_error,"io_error":self.io_error})).expect("bounded control values serialize")
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        if self.revision == 0 {
            return;
        }
        self.closed = true;
        self.revision = self.revision.saturating_add(1);
        if let Some(current) = &mut self.current
            && !current.snapshot.terminal()
        {
            current.snapshot.phase = Phase::Failed;
            current.snapshot.queued = false;
            current.snapshot.message = Some("Client closed before run completed".into());
        }
        self.archive();
        if let Err(error) = self.writer.finish(self.snapshot(), Duration::from_secs(1)) {
            warn!(%error, "Script control shutdown status unavailable");
        }
    }
}

/// Install the receiver and its checklist together after creating the inbox.
pub(crate) fn install(app: &mut App) -> std::io::Result<()> {
    let Some(root) = app
        .world()
        .resource::<crate::ViewerSettings>()
        .0
        .script_control
        .clone()
    else {
        return Ok(());
    };
    files::prepare(&root)?;
    let instance = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    app.insert_resource(Bridge::new(root, instance))
        .add_systems(
            PreUpdate,
            receive
                .after(bevy::input::InputSystems)
                .before(super::drive),
        )
        .add_systems(PreUpdate, observe.after(super::drive))
        .add_systems(Last, publish);
    runbook::view::install(app);
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn observe(script: Option<Res<Script>>, status: Res<RunStatus>, mut bridge: ResMut<Bridge>) {
    observe_run(&mut bridge, script.as_deref(), &status);
}

fn observe_run(bridge: &mut Bridge, script: Option<&Script>, status: &RunStatus) {
    match script {
        Some(script) => bridge.observe(script.status(), script.started.is_none()),
        None => bridge.observe(status, false),
    }
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn receive(
    mut commands: Commands,
    mut bridge: ResMut<Bridge>,
    script: Option<Res<Script>>,
    status: Res<RunStatus>,
    (settings, online): (Res<crate::ViewerSettings>, Res<crate::online::OnlineState>),
    mut control: ResMut<RunControl>,
    mut focus: Option<ResMut<InputFocus>>,
) {
    observe_run(&mut bridge, script.as_deref(), &status);
    let Some(result) = bridge.job.poll(Some(0)) else {
        return;
    };
    let bytes = match result {
        Ok(bytes) => {
            bridge.io_error = None;
            bytes
        }
        Err(error) => {
            if bridge.io_error.as_ref() != Some(&error) {
                warn!(%error, "Script control I/O failed");
            }
            bridge.io_error = Some(protocol::message(&error));
            return;
        }
    };
    let Some(bytes) = bytes else { return };
    let request = match protocol::parse(&bytes) {
        Ok(request) => request,
        Err(error) => {
            bridge.protocol_error = Some(error.into());
            return;
        }
    };
    if !bridge.fresh(&request) {
        return;
    }
    let receipt = match &request.operation {
        Operation::Start(_) if script.is_some() => Receipt::rejected(request.id, "busy"),
        Operation::Start(source) => start_request(
            &mut bridge,
            (request.id, source),
            (&settings.0, online.enabled),
            &mut commands,
            focus.as_deref_mut(),
        ),
        Operation::Stop(id) => {
            stop_request(&bridge, request.id, *id, script.as_deref(), &mut control)
        }
        Operation::Status => Receipt::new(
            request.id,
            "observed",
            "status",
            bridge.current.as_ref().map(|run| run.snapshot.id),
        ),
        Operation::RunbookStart(definition) => {
            bridge.runbook = Some(runbook::Run::new(request.id, definition.clone()));
            Receipt::new(request.id, "accepted", "runbook_started", None)
        }
        Operation::RunbookResult(update) => match bridge
            .runbook
            .as_mut()
            .ok_or("runbook_changed")
            .and_then(|run| run.record(update))
        {
            Ok(()) => Receipt::new(request.id, "accepted", "runbook_result_recorded", None),
            Err(code) => Receipt::rejected(request.id, code),
        },
    };
    bridge.finish_request(request, receipt);
}

fn start_request(
    bridge: &mut Bridge,
    (request_id, source): (u64, &str),
    (settings, online): (&crate::ViewerConfig, bool),
    commands: &mut Commands,
    focus: Option<&mut InputFocus>,
) -> Receipt {
    let program = match prepare(source, &bridge.root, settings.local_session) {
        Ok(program) => program,
        Err(error) => {
            let mut receipt = Receipt::rejected(request_id, "invalid_program");
            receipt.line = error.line;
            receipt.message = Some(protocol::message(&error.message));
            return receipt;
        }
    };
    let script = Script::for_viewer(program, settings, online);
    let Some(id) = bridge.bind(script.status(), "control") else {
        return Receipt::rejected(request_id, "run_ids_exhausted");
    };
    commands.insert_resource(script.status().clone());
    commands.insert_resource(RunControl::default());
    commands.insert_resource(script);
    if let Some(focus) = focus {
        focus.clear();
    }
    Receipt::new(request_id, "accepted", "start_queued", Some(id))
}

fn stop_request(
    bridge: &Bridge,
    request_id: u64,
    run_id: u64,
    script: Option<&Script>,
    control: &mut RunControl,
) -> Receipt {
    if bridge
        .current
        .as_ref()
        .is_none_or(|run| run.snapshot.id != run_id)
    {
        Receipt::rejected(request_id, "run_changed")
    } else if !script.is_some_and(|script| script.status().phase().active()) {
        Receipt::rejected(request_id, "not_running")
    } else {
        control.request(Request::Stop);
        Receipt::new(request_id, "stop_requested", "stop_requested", Some(run_id))
    }
}

fn prepare(
    source: &str,
    base: &std::path::Path,
    local: bool,
) -> Result<Program, super::ParseError> {
    let program = Program::parse(source, base)?;
    if program.statement_count() == 0 {
        return Err(super::ParseError {
            line: None,
            message: "Add a statement before running".into(),
        });
    }
    program.check_local_commands(local)?;
    Ok(program)
}

#[allow(clippy::needless_pass_by_value)]
fn publish(
    time: Res<Time<Real>>,
    script: Option<Res<Script>>,
    status: Res<RunStatus>,
    mut bridge: ResMut<Bridge>,
) {
    observe_run(&mut bridge, script.as_deref(), &status);
    if bridge.job.pending(Some(0)) || time.elapsed() < bridge.next_poll {
        return;
    }
    bridge.next_poll = time.elapsed() + POLL;
    bridge.revision = bridge.revision.saturating_add(1);
    let bytes = bridge.snapshot();
    let writer = bridge.writer.clone();
    bridge.job.start(0, move || writer.exchange(&bytes));
}

#[cfg(test)]
mod tests;
