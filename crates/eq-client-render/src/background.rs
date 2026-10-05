//! One replaceable asset job, scoped to the admission that requested it.
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};

pub(crate) struct Background<T> {
    task: Option<(u64, Task<T>)>,
}

impl<T> Default for Background<T> {
    fn default() -> Self {
        Self { task: None }
    }
}

impl<T: Send + 'static> Background<T> {
    /// Runs archive work on the task pool, never on the frame thread.
    pub(crate) fn start(&mut self, admission: u64, work: impl FnOnce() -> T + Send + 'static) {
        self.task = Some((
            admission,
            AsyncComputeTaskPool::get_or_init(TaskPool::new).spawn(async move { work() }),
        ));
    }

    /// Whether this admission's job has yet to hand over its result.
    pub(crate) fn pending(&self, admission: Option<u64>) -> bool {
        self.task.as_ref().is_some_and(|(id, _)| Some(*id) == admission)
    }

    /// Obsolete results are discarded without waiting for their read to finish.
    pub(crate) fn poll(&mut self, admission: Option<u64>) -> Option<T> {
        let (id, task) = self.task.as_mut()?;
        if Some(*id) != admission {
            self.clear();
            return None;
        }
        let result = block_on(poll_once(task))?;
        self.clear();
        Some(result)
    }

    pub(crate) fn clear(&mut self) {
        self.task = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn a_blocked_loader_does_not_block_event_consumption() {
        let (release, wait) = mpsc::channel();
        let mut job = Background::default();
        job.start(7, move || wait.recv().unwrap());
        let (events, receiver) = mpsc::sync_channel(1024);
        for batch in 0..16 {
            for n in 0..256 {
                events.try_send(batch * 256 + n).unwrap();
            }
            assert!(job.poll(Some(7)).is_none());
            for n in 0..256 {
                assert_eq!(receiver.try_recv().unwrap(), batch * 256 + n);
            }
        }
        release.send(42).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(result) = job.poll(Some(7)) {
                assert_eq!(result, 42);
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "loader never completed"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn an_old_admission_cannot_install_its_result() {
        let mut job = Background::default();
        job.start(1, || 42);
        assert!(job.pending(Some(1)));
        assert!(!job.pending(Some(2)));
        assert_eq!(job.poll(Some(2)), None);
        assert!(!job.pending(Some(1)));
        assert_eq!(job.poll(Some(1)), None);
    }
}
