//! What the client writes about itself, so that a client that ends leaves
//! its reason behind even when nothing kept its standard error: a log file
//! in the settings folder, with the last run's kept beside it, and a line
//! for each way the client ends that would otherwise say nothing, such as
//! the window being asked to close.
use bevy::{
    log::{BoxedLayer, tracing_subscriber::Layer},
    prelude::*,
    window::WindowCloseRequested,
};
use std::{fs::File, io, path::Path};

/// The log file, in the settings folder.
pub(crate) const FILE: &str = "eq-client.log";
/// The previous run's log, which the next run keeps.
pub(crate) const PREVIOUS: &str = "eq-client.previous.log";

/// A layer that copies what the client logs into [`FILE`] in the settings
/// folder, once the folder is known; none without one, or where the file
/// cannot be made.
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
            eprintln!("Cannot write {FILE} in {}: {error}", directory.display());
            return None;
        }
    };
    Some(
        bevy::log::tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .boxed(),
    )
}

/// Starts a new log file, keeping the last one as [`PREVIOUS`].
fn open(directory: &Path) -> io::Result<File> {
    std::fs::create_dir_all(directory)?;
    let current = directory.join(FILE);
    if current.exists() {
        std::fs::rename(&current, directory.join(PREVIOUS))?;
    }
    File::create(current)
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
            use std::io::Write;
            let mut first = open(&directory).unwrap();
            first.write_all(b"first run").unwrap();
        }
        let second = open(&directory).unwrap();
        drop(second);
        assert_eq!(
            std::fs::read_to_string(directory.join(PREVIOUS)).unwrap(),
            "first run"
        );
        assert_eq!(std::fs::read_to_string(directory.join(FILE)).unwrap(), "");
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
