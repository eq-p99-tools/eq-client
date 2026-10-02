//! What the client writes about itself, so that a client that ends leaves
//! its reason behind even when nothing kept its standard error: a log file
//! in the settings folder, with the last run's kept beside it, and a line
//! for each way the client ends that would otherwise say nothing, such as
//! the window being asked to close. Clients running side by side on one PC
//! each write a file of their own.
use bevy::{
    log::{BoxedLayer, tracing_subscriber::Layer},
    prelude::*,
    window::WindowCloseRequested,
};
use std::{
    fs::File,
    io::{self, Write},
    path::Path,
};

/// How many clients running at once on one PC each get a log of their own.
const SLOTS: u32 = 8;

/// A log slot's file names: its log, its previous run's log, and the lock
/// a running client holds on the slot. The first slot's log is
/// `eq-client.log`, a second client's `eq-client-2.log`, and so on.
fn slot(number: u32) -> [String; 3] {
    let stem = if number == 1 {
        "eq-client".to_owned()
    } else {
        format!("eq-client-{number}")
    };
    [
        format!("{stem}.log"),
        format!("{stem}.previous.log"),
        format!("{stem}.lock"),
    ]
}

/// A client's log file, with the lock that keeps its slot this client's
/// for as long as the file stays open; the lock goes with the process,
/// however it ends.
struct LogFile {
    file: File,
    /// The slot's lock, held while the client runs.
    _lock: File,
    /// The log's file name.
    name: String,
}

impl Write for LogFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.file.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// A layer that copies what the client logs into its log file in the
/// settings folder, once the folder is known; none without one, or where
/// no file can be made.
pub(crate) fn file_layer(app: &mut App) -> Option<BoxedLayer> {
    let directory = app
        .world()
        .get_resource::<super::ViewerSettings>()?
        .0
        .settings_directory
        .clone()?;
    let file = match open(&directory) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("Cannot write a log in {}: {error}", directory.display());
            return None;
        }
    };
    let [first, ..] = slot(1);
    if file.name != first {
        eprintln!(
            "Another client writes {first}, so this one writes {}",
            file.name
        );
    }
    Some(
        bevy::log::tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .boxed(),
    )
}

/// Starts a new log in the first slot no running client holds, keeping
/// that slot's last log as its previous one, so that a second client on the
/// same PC neither writes into the first one's log nor moves it aside.
fn open(directory: &Path) -> io::Result<LogFile> {
    std::fs::create_dir_all(directory)?;
    for number in 1..=SLOTS {
        let [log, previous, lock] = slot(number);
        let held = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(directory.join(lock))?;
        match held.try_lock() {
            Ok(()) => (),
            Err(std::fs::TryLockError::WouldBlock) => continue,
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
        let current = directory.join(&log);
        if current.exists() {
            std::fs::rename(&current, directory.join(previous))?;
        }
        return Ok(LogFile {
            file: File::create(current)?,
            _lock: held,
            name: log,
        });
    }
    Err(io::Error::other(format!(
        "{SLOTS} clients already write logs here"
    )))
}

/// Notes each request to close the window, the one way the client ends
/// that it does not decide itself.
pub(crate) fn close_requests(mut requests: MessageReader<WindowCloseRequested>) {
    for request in requests.read() {
        info!(
            window = ?request.window,
            "The window was asked to close, so the client is ending"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_run_keeps_the_last_runs_log() {
        let directory = std::env::temp_dir().join(format!("eq-client-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        {
            let mut first = open(&directory).unwrap();
            first.write_all(b"first run").unwrap();
        }
        let second = open(&directory).unwrap();
        assert_eq!(second.name, "eq-client.log");
        drop(second);
        assert_eq!(
            std::fs::read_to_string(directory.join("eq-client.previous.log")).unwrap(),
            "first run"
        );
        assert_eq!(
            std::fs::read_to_string(directory.join("eq-client.log")).unwrap(),
            ""
        );
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn clients_running_side_by_side_write_their_own_logs() {
        let directory =
            std::env::temp_dir().join(format!("eq-client-logs-side-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let mut first = open(&directory).unwrap();
        first.write_all(b"first client").unwrap();
        // A second client while the first runs takes the next slot, and
        // leaves the first one's log where it is.
        let mut second = open(&directory).unwrap();
        second.write_all(b"second client").unwrap();
        assert_eq!(
            (first.name.as_str(), second.name.as_str()),
            ("eq-client.log", "eq-client-2.log")
        );
        assert_eq!(
            std::fs::read_to_string(directory.join("eq-client.log")).unwrap(),
            "first client"
        );
        // Once the first has ended, its slot is free for the next client.
        drop(first);
        let third = open(&directory).unwrap();
        assert_eq!(third.name, "eq-client.log");
        assert_eq!(
            std::fs::read_to_string(directory.join("eq-client.previous.log")).unwrap(),
            "first client"
        );
        drop((second, third));
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
