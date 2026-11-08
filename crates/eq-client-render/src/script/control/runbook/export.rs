//! A captured checklist export, serialized independently of subsequent row updates.
use serde_json::{Value, json};

use super::{
    super::{files, protocol},
    Run,
};
use crate::background::Background;

#[derive(Default)]
pub(in crate::script::control) struct Export {
    last: Option<Snapshot>,
    job: Background<Result<(), String>>,
}

struct Snapshot {
    request_id: Option<u64>,
    runbook_id: u64,
    revision: u64,
    result: Option<Result<(), String>>,
}

impl Export {
    pub(super) fn busy(&self) -> bool {
        self.job.pending(Some(0))
    }

    pub(in crate::script::control) fn queue(
        &mut self,
        writer: files::Writer,
        run: &Run,
        request_id: Option<u64>,
        instance: &str,
        admission_id: Option<u64>,
    ) -> Result<(), &'static str> {
        self.poll();
        if self.busy() {
            return Err("runbook_export_busy");
        }
        let bytes = serde_json::to_vec(&json!({
            "version":protocol::VERSION,"instance":instance,"request_id":request_id,
            "admission_id":admission_id,"runbook":run.json(),
        }))
        .expect("bounded runbook export serializes");
        self.last = Some(Snapshot {
            request_id,
            runbook_id: run.id,
            revision: run.revision,
            result: None,
        });
        self.job.start(0, move || writer.export_runbook(&bytes));
        Ok(())
    }

    pub(in crate::script::control) fn poll(&mut self) {
        if let Some(result) = self.job.poll(Some(0))
            && let Some(last) = &mut self.last
        {
            last.result = Some(result.map_err(|error| protocol::message(&error)));
        }
    }

    pub(in crate::script::control) fn json(&self, closed: bool) -> Option<Value> {
        let last = self.last.as_ref()?;
        let (state, error) = match &last.result {
            None if closed => ("unconfirmed", None),
            None => ("writing", None),
            Some(Ok(())) => ("written", None),
            Some(Err(error)) => ("failed", Some(error)),
        };
        Some(json!({
            "request_id":last.request_id,"runbook_id":last.runbook_id,"revision":last.revision,
            "state":state,"path":files::RUNBOOK_EXPORT,"error":error,
        }))
    }

    pub(super) fn label(&self, run: &Run) -> &'static str {
        let Some(last) = self.last.as_ref().filter(|last| last.runbook_id == run.id) else {
            return "";
        };
        match last.result {
            None => "Writing export…",
            Some(Ok(())) if last.revision == run.revision => "Saved runbook-export.json",
            Some(Ok(())) => "Export contains an earlier checklist revision",
            Some(Err(_)) => "Export failed",
        }
    }
}
