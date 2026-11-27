//! On-demand movement reports join a captured scene to a worker counter boundary.
//! No movement is sent, polled periodically or inferred from proposal receipts.
use eq_client_core::MovementPacketSnapshot;
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Instant};

const MAX_PENDING: usize = 16;

struct Pending {
    session_id: u64,
    captured: Instant,
    label: String,
    state: Value,
}

/// The scene observation precedes the separately timestamped worker boundary.
pub(super) struct Report {
    pub(super) label: String,
    pub(super) state: Value,
}

impl Report {
    pub(super) fn log(self) {
        bevy::log::info!(label = self.label, movement_state = %self.state, "Script movement report");
    }
}

#[derive(Default)]
pub(super) struct Reports {
    next_id: u64,
    clock_origin: Option<Instant>,
    pending: BTreeMap<u64, Pending>,
}

impl Reports {
    /// Retain at most a small bounded set; a missing answer remains unavailable.
    pub(super) fn request(
        &mut self,
        session_id: u64,
        label: &str,
        state: Value,
        captured: Instant,
    ) -> Result<u64, Report> {
        let Some(request_id) = self.next_id.checked_add(1) else {
            return Err(unavailable(label, state, "request_identity_exhausted"));
        };
        if self.pending.len() >= MAX_PENDING {
            return Err(unavailable(label, state, "snapshot_queue_full"));
        }
        self.next_id = request_id;
        self.clock_origin.get_or_insert(captured);
        self.pending.insert(
            request_id,
            Pending {
                session_id,
                captured,
                label: label.to_owned(),
                state,
            },
        );
        Ok(request_id)
    }

    pub(super) fn answer(
        &mut self,
        admission: Option<u64>,
        snapshot: MovementPacketSnapshot,
    ) -> Option<Report> {
        let pending = self.pending.get(&snapshot.request_id)?;
        if admission != Some(snapshot.session_id)
            || pending.session_id != snapshot.session_id
            || snapshot.observed < pending.captured
        {
            return None;
        }
        let mut pending = self.pending.remove(&snapshot.request_id)?;
        let origin = self.clock_origin?;
        pending.state["movement_packets"] = json!({
            "basis": "successful_player_position_transport_submissions",
            "session_id": snapshot.session_id,
            "request_id": snapshot.request_id,
            "run_id": snapshot.run_id,
            "sent": snapshot.sent,
            "worker_unix_timestamp_ms": snapshot.unix_timestamp_ms,
            "scene_monotonic_ms": pending.captured.saturating_duration_since(origin).as_secs_f64() * 1000.0,
            "worker_monotonic_ms": snapshot.observed.saturating_duration_since(origin).as_secs_f64() * 1000.0,
            "scene_to_worker_ms": snapshot.observed.duration_since(pending.captured).as_secs_f64() * 1000.0,
        });
        Some(Report {
            label: pending.label,
            state: pending.state,
        })
    }

    pub(super) fn failed(&mut self, request_id: u64) -> Option<Report> {
        self.pending
            .remove(&request_id)
            .map(|pending| unavailable(&pending.label, pending.state, "snapshot_not_queued"))
    }

    /// Do not let an old admission's delayed answer label a new observation.
    pub(super) fn reset(&mut self) {
        for (_, pending) in std::mem::take(&mut self.pending) {
            unavailable(&pending.label, pending.state, "admission_ended").log();
        }
    }
}

pub(super) fn unavailable(label: &str, mut state: Value, reason: &str) -> Report {
    state["movement_packets"] = json!({ "sent": null, "unavailable": reason });
    Report {
        label: label.to_owned(),
        state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn requesting_a_movement_report_queues_only_one_read_only_boundary() {
        use crate::online::{OnlineState, testing};
        use eq_client_core::ClientCommand;
        use std::sync::mpsc;

        let mut online = OnlineState::new(true);
        testing::admit(&mut online, 7, testing::player(4));
        let before = online.world().player().unwrap().position;
        let (sender, queue) = mpsc::sync_channel(4);
        let outbox = crate::outbox::Outbox::new(Some(sender));
        online.report_movement("start", json!({ "session_position": before }), &outbox);
        assert!(matches!(
            queue.try_recv(),
            Ok(ClientCommand::MovementPacketSnapshot {
                session_id: 7,
                request_id: 1
            })
        ));
        assert!(queue.try_recv().is_err());
        assert_eq!(online.world().player().unwrap().position, before);
        let answer = snapshot(1, Instant::now());
        let report = online.movement_reports.answer(Some(7), answer).unwrap();
        assert_eq!(report.state["movement_packets"]["sent"], 2);
        assert_eq!(online.world().player().unwrap().position, before);
    }

    fn snapshot(request_id: u64, observed: Instant) -> MovementPacketSnapshot {
        MovementPacketSnapshot {
            session_id: 7,
            request_id,
            run_id: 23,
            sent: 2,
            observed,
            unix_timestamp_ms: 1234,
        }
    }

    #[test]
    fn a_report_counts_worker_sends_separately_from_accepted_proposals() {
        let captured = Instant::now();
        let mut reports = Reports::default();
        let request = reports
            .request(7, "end", json!({ "accepted_proposals": 120 }), captured)
            .ok()
            .unwrap();
        let answer = snapshot(request, captured + Duration::from_millis(25));
        let report = reports.answer(Some(7), answer).unwrap();
        assert_eq!(report.label, "end");
        assert_eq!(report.state["accepted_proposals"], 120);
        assert_eq!(report.state["movement_packets"]["sent"], 2);
        assert_eq!(report.state["movement_packets"]["run_id"], 23);
        assert_eq!(report.state["movement_packets"]["scene_to_worker_ms"], 25.0);
        assert_eq!(report.state["movement_packets"]["scene_monotonic_ms"], 0.0);
        assert_eq!(
            report.state["movement_packets"]["worker_monotonic_ms"],
            25.0
        );
        assert!(reports.answer(Some(7), answer).is_none());
        let second = reports
            .request(7, "next", json!({}), captured + Duration::from_millis(100))
            .ok()
            .unwrap();
        let later = reports
            .answer(
                Some(7),
                snapshot(second, captured + Duration::from_millis(130)),
            )
            .unwrap();
        assert_eq!(
            later.state["movement_packets"]["worker_monotonic_ms"],
            130.0
        );
    }

    #[test]
    fn delayed_or_unrelated_answers_cannot_cross_a_report_boundary() {
        let captured = Instant::now();
        let mut reports = Reports::default();
        let first = reports
            .request(7, "start", json!({}), captured)
            .ok()
            .unwrap();
        let answer = snapshot(first, captured);
        assert!(reports.answer(Some(8), answer).is_none());
        assert!(
            reports
                .answer(Some(7), snapshot(first + 1, captured))
                .is_none()
        );
        assert!(
            reports
                .answer(
                    Some(7),
                    snapshot(
                        first,
                        captured.checked_sub(Duration::from_millis(1)).unwrap()
                    )
                )
                .is_none()
        );
        reports.reset();
        let next = reports.request(7, "new", json!({}), captured).ok().unwrap();
        assert_ne!(first, next);
        assert!(reports.answer(Some(7), answer).is_none());
        assert!(reports.answer(Some(7), snapshot(next, captured)).is_some());
    }

    #[test]
    fn unavailable_or_full_reports_never_invent_zero_sent_packets() {
        let mut reports = Reports::default();
        let captured = Instant::now();
        for _ in 0..MAX_PENDING {
            assert!(reports.request(7, "pending", json!({}), captured).is_ok());
        }
        let report = reports
            .request(7, "full", json!({}), captured)
            .err()
            .unwrap();
        assert!(report.state["movement_packets"]["sent"].is_null());
        let failed = reports.failed(1).unwrap();
        assert!(failed.state["movement_packets"]["sent"].is_null());
    }
}
