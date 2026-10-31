//! What the window knows of the display and the adapter, kept for the
//! machine report a session sends from its own thread. The window side
//! refreshes the snapshot; the session only reads it, so the report never
//! asks the windowing or graphics libraries anything off their thread.
use std::sync::{Arc, Mutex, PoisonError};

/// The display and adapter as the window last saw them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Display {
    /// The desktop's mode on the monitor the window is on: width, height
    /// and refresh rate in whole hertz.
    pub desktop: (u32, u32, u32),
    /// The game screen's size: the window's own mode when it holds the
    /// monitor exclusively, else the desktop's.
    pub screen: (u32, u32),
    /// The monitor's place among the monitors, from 0, left to right.
    pub monitor: usize,
    /// The monitor's name as the windowing system gives it, if any.
    pub monitor_name: Option<String>,
    /// The graphics adapter's name, as the renderer reports it.
    pub adapter: String,
}

/// A [`Display`] shared between the window, which writes it, and the
/// sessions, which read it.
#[derive(Clone, Debug, Default)]
pub struct DisplaySnapshot(Arc<Mutex<Display>>);

impl DisplaySnapshot {
    /// The display as last seen.
    #[must_use]
    pub fn get(&self) -> Display {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Keeps `display` as the latest, where it changed.
    pub fn set(&self, display: Display) {
        let mut kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if *kept != display {
            *kept = display;
        }
    }
}

impl PartialEq for DisplaySnapshot {
    /// The same snapshot, shared, not one that happens to hold the same.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_is_shared_between_its_writer_and_readers() {
        let window = DisplaySnapshot::default();
        let session = window.clone();
        assert_eq!(session.get(), Display::default());
        let display = Display {
            desktop: (1920, 1080, 60),
            screen: (1920, 1080),
            monitor: 0,
            monitor_name: None,
            adapter: "Test Adapter".into(),
        };
        window.set(display.clone());
        assert_eq!(session.get(), display);
        assert_eq!(window, session);
        assert_ne!(window, DisplaySnapshot::default());
    }
}
