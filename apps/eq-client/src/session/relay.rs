//! How the session's news reaches the window without the session ever
//! waiting on it.
//!
//! The network thread hands every update to a relay and goes straight back
//! to reading the server, so its acknowledgements never stop. The relay
//! feeds the window's queue, which holds [`WINDOW_QUEUE`] updates. When the
//! window falls that far behind, as it can during a slow zone-in, or while
//! it is minimized and not drawing, the relay holds the rest, in order, and
//! hands them over as the window catches up. Nothing is dropped, and being
//! busy never ends the session: the original client, which stops reading
//! the network entirely while it loads a zone, never ends one for that
//! either. Only a window that has stopped taking news altogether, with
//! [`CEILING`] updates waiting, ends the session, so memory cannot run away.
use anyhow::Result;
use eq_client_core::WorldUpdate;
use eq_network::client::CancellationToken;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Instant,
};

/// How many updates the window's own queue holds.
pub(super) const WINDOW_QUEUE: usize = 65_536;

/// How many updates may wait beyond the window's queue before the session
/// gives up on a window that has stopped taking them: about an hour and a
/// half of a busy zone's news at 200 updates a second.
pub(super) const CEILING: usize = 1_048_576;

/// The session's side of the relay.
pub(super) struct Relay {
    sender: mpsc::Sender<WorldUpdate>,
    /// Updates handed to the relay and not yet in the window's queue.
    held: Arc<AtomicUsize>,
    ceiling: usize,
}

/// Starts a relay and returns its sending side and the window's queue.
pub(super) fn start() -> (Relay, mpsc::Receiver<WorldUpdate>) {
    start_with(WINDOW_QUEUE, CEILING)
}

pub(super) fn start_with(
    window_queue: usize,
    ceiling: usize,
) -> (Relay, mpsc::Receiver<WorldUpdate>) {
    let (sender, incoming) = mpsc::channel();
    let (window, updates) = mpsc::sync_channel(window_queue);
    let held = Arc::new(AtomicUsize::new(0));
    let counted = held.clone();
    let spawned = thread::Builder::new()
        .name("session news".to_owned())
        .spawn(move || forward(&incoming, &window, &counted, window_queue));
    if let Err(error) = spawned {
        // Without the relay thread the window gets nothing; the first send
        // then finds the relay gone and stops the session.
        tracing::error!("Could not start the session's news relay: {error}");
    }
    (
        Relay {
            sender,
            held,
            ceiling,
        },
        updates,
    )
}

/// Moves updates into the window's queue in order, waiting for room when the
/// window is behind. Ends when the session closes its side, once everything
/// it sent is in the window's queue, or when the window lets go of its queue.
fn forward(
    incoming: &mpsc::Receiver<WorldUpdate>,
    window: &mpsc::SyncSender<WorldUpdate>,
    held: &AtomicUsize,
    window_queue: usize,
) {
    let mut behind: Option<Instant> = None;
    for update in incoming {
        let delivered = match window.try_send(update) {
            Ok(()) => true,
            Err(mpsc::TrySendError::Full(update)) => {
                if behind.is_none() {
                    behind = Some(Instant::now());
                    tracing::warn!(
                        "The window is {window_queue} updates behind the session; \
                         holding the rest until it catches up"
                    );
                }
                window.send(update).is_ok()
            }
            Err(mpsc::TrySendError::Disconnected(_)) => false,
        };
        if !delivered {
            return;
        }
        let waiting = held.fetch_sub(1, Ordering::AcqRel) - 1;
        if waiting == 0
            && let Some(since) = behind.take()
        {
            tracing::info!(
                seconds = since.elapsed().as_secs_f32(),
                "The window caught up with the session"
            );
        }
    }
}

impl Relay {
    /// Hands one update on without waiting.
    ///
    /// # Errors
    /// Fails once [`CEILING`] updates are waiting beyond the window's queue:
    /// the window has stopped taking the session's news. A window that let
    /// go of its queue instead cancels the session, as a closed window does.
    pub(super) fn send(&self, cancel: &CancellationToken, update: WorldUpdate) -> Result<()> {
        let waiting = self.held.load(Ordering::Acquire);
        if waiting >= self.ceiling {
            anyhow::bail!(
                "the window stopped taking the session's news ({waiting} updates waiting)"
            );
        }
        self.held.fetch_add(1, Ordering::AcqRel);
        if self.sender.send(update).is_err() {
            self.held.fetch_sub(1, Ordering::AcqRel);
            cancel.cancel();
        }
        Ok(())
    }

    /// Hands on the session's last update, whatever is waiting, and closes
    /// the relay. Never waits on the window.
    pub(super) fn close(self, last: WorldUpdate) {
        self.held.fetch_add(1, Ordering::AcqRel);
        if self.sender.send(last).is_err() {
            self.held.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eq_client_core::world::Link;
    use std::time::Duration;

    fn update(n: usize) -> WorldUpdate {
        WorldUpdate::Connection(if n % 2 == 0 {
            Link::Entering
        } else {
            Link::Connected
        })
    }

    fn is(update: &WorldUpdate, n: usize) -> bool {
        matches!(
            (update, n % 2),
            (WorldUpdate::Connection(Link::Entering), 0)
                | (WorldUpdate::Connection(Link::Connected), 1)
        )
    }

    #[test]
    fn a_window_far_behind_gets_everything_in_order_and_the_session_keeps_going() {
        let (relay, updates) = start_with(4, 1_000);
        let cancel = CancellationToken::default();
        // The window takes nothing while 100 updates arrive, 25 times its queue.
        for n in 0..100 {
            relay.send(&cancel, update(n)).unwrap();
        }
        assert!(!cancel.is_cancelled());
        for n in 0..100 {
            let got = updates.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(is(&got, n), "update {n} out of order: {got:?}");
        }
        relay.close(WorldUpdate::Connection(Link::Ended));
        assert!(matches!(
            updates.recv_timeout(Duration::from_secs(5)),
            Ok(WorldUpdate::Connection(Link::Ended))
        ));
        assert!(matches!(
            updates.recv_timeout(Duration::from_secs(5)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn sending_never_waits_on_a_window_that_takes_nothing() {
        let (relay, updates) = start_with(1, 10_000);
        let cancel = CancellationToken::default();
        let (finished, completion) = mpsc::channel();
        let session = thread::spawn(move || {
            for n in 0..5_000 {
                relay.send(&cancel, update(n)).unwrap();
            }
            relay.close(WorldUpdate::Connection(Link::Ended));
            finished.send(()).unwrap();
        });
        let ended = completion.recv_timeout(Duration::from_secs(5));
        drop(updates);
        session.join().unwrap();
        assert!(ended.is_ok(), "the session waited on the window");
    }

    #[test]
    fn only_a_window_that_stopped_taking_news_ends_the_session() {
        let (relay, updates) = start_with(1, 8);
        let cancel = CancellationToken::default();
        // One in the window's queue, one in the relay's hand, and eight
        // waiting: the ceiling.
        let mut failed = None;
        for n in 0..64 {
            if relay.send(&cancel, update(n)).is_err() {
                failed = Some(n);
                break;
            }
        }
        let failed = failed.expect("the ceiling never stopped the session");
        assert!((8..=10).contains(&failed), "stopped after {failed} updates");
        assert!(!cancel.is_cancelled());
        // Everything sent before the ceiling still reaches the window, in order.
        for n in 0..failed {
            let got = updates.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(is(&got, n), "update {n} out of order: {got:?}");
        }
    }

    #[test]
    fn a_window_that_let_go_of_its_queue_cancels_the_session() {
        let (relay, updates) = start_with(4, 1_000);
        drop(updates);
        let cancel = CancellationToken::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut n = 0;
        while !cancel.is_cancelled() {
            relay.send(&cancel, update(n)).unwrap();
            n += 1;
            assert!(Instant::now() < deadline, "the session was never cancelled");
            thread::yield_now();
        }
    }

    #[test]
    fn closing_does_not_need_a_window() {
        let (relay, updates) = start_with(1, 1_000);
        drop(updates);
        relay.close(WorldUpdate::Connection(Link::Ended));
    }
}
