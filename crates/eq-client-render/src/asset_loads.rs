//! Bounded, deduplicated archive work. Each cache owns a queue and cancels its
//! generation on admission reset; only its caller installs completed GPU data.
use crate::background::Background;
use std::{
    collections::{HashSet, VecDeque},
    hash::Hash,
};

/// A lookup distinguishes a pending read from a known missing installed asset.
pub(crate) enum Asset<T> {
    Pending,
    Missing,
    Ready(T),
}

const CAPACITY: usize = 64;

struct Request<K, V> {
    key: K,
    load: Box<dyn FnOnce() -> Option<V> + Send + Sync>,
}

/// One running decode and a bounded waiting queue, scoped to its cache owner.
pub(crate) struct Loads<K, V> {
    generation: u64,
    active: Background<(K, Option<V>)>,
    waiting: VecDeque<Request<K, V>>,
    pending: HashSet<K>,
}

impl<K, V> Default for Loads<K, V> {
    fn default() -> Self {
        Self {
            generation: 0,
            active: Background::default(),
            waiting: VecDeque::new(),
            pending: HashSet::new(),
        }
    }
}

impl<K: Clone + Eq + Hash + Send + 'static, V: Send + 'static> Loads<K, V> {
    /// Identifies this cache lifetime, including failed archive-open attempts.
    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    /// Enqueues once per key. False means capacity was reached; callers may
    /// retry next frame and must not remember that as a missing asset.
    pub(crate) fn request(
        &mut self,
        key: K,
        load: impl FnOnce() -> Option<V> + Send + Sync + 'static,
    ) -> bool {
        if self.pending.contains(&key) {
            return true;
        }
        if self.pending.len() >= CAPACITY {
            return false;
        }
        self.pending.insert(key.clone());
        self.waiting.push_back(Request {
            key,
            load: Box::new(load),
        });
        self.start();
        true
    }

    /// Takes at most one result per frame, then starts the next read.
    pub(crate) fn poll(&mut self) -> Option<(K, Option<V>)> {
        let result = self.active.poll(Some(self.generation));
        if let Some((key, _)) = &result {
            self.pending.remove(key);
        }
        self.start();
        result
    }

    fn start(&mut self) {
        if !self.active.pending(Some(self.generation))
            && let Some(request) = self.waiting.pop_front()
        {
            self.active
                .start(self.generation, move || (request.key, (request.load)()));
        }
    }

    /// Abandons old results without waiting for a filesystem read to finish.
    pub(crate) fn clear(&mut self) {
        self.active.clear();
        self.waiting.clear();
        self.pending.clear();
        self.generation = self.generation.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        time::{Duration, Instant},
    };

    fn next<K: Clone + Eq + Hash + Send + 'static, V: Send + 'static>(
        loads: &mut Loads<K, V>,
    ) -> (K, Option<V>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(value) = loads.poll() {
                return value;
            }
            assert!(Instant::now() < deadline, "asset job did not finish");
            std::thread::yield_now();
        }
    }

    #[test]
    fn requests_are_deduplicated_and_capacity_is_bounded() {
        let (release, wait) = mpsc::channel();
        let wait = Mutex::new(wait);
        let count = Arc::new(AtomicUsize::new(0));
        let mut loads = Loads::default();
        let runs = count.clone();
        assert!(loads.request(0, move || {
            runs.fetch_add(1, Ordering::SeqCst);
            wait.lock().unwrap().recv().ok()
        }));
        assert!(loads.request(0, || panic!("duplicate read")));
        for key in 1..CAPACITY {
            assert!(loads.request(key, move || Some(key)));
        }
        assert!(!loads.request(CAPACITY, || Some(CAPACITY)));
        assert!(loads.poll().is_none());
        release.send(0).unwrap();
        assert_eq!(next(&mut loads), (0, Some(0)));
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(loads.request(CAPACITY, || Some(CAPACITY)));
        for key in 1..=CAPACITY {
            assert_eq!(next(&mut loads), (key, Some(key)));
        }
    }

    #[test]
    fn reset_discards_an_in_flight_result_and_its_waiting_work() {
        let (release, wait) = mpsc::channel();
        let (started, start) = mpsc::channel();
        let wait = Mutex::new(wait);
        let mut loads = Loads::default();
        loads.request("old", move || {
            started.send(()).unwrap();
            wait.lock().unwrap().recv().ok()
        });
        start.recv_timeout(Duration::from_secs(5)).unwrap();
        loads.request("waiting", || panic!("obsolete queued work"));
        loads.clear();
        release.send(1).unwrap();
        loads.request("new", || Some(2));
        assert_eq!(next(&mut loads), ("new", Some(2)));
        assert!(loads.poll().is_none());
    }
}
