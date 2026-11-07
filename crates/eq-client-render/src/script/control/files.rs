//! Fixed-file exchanges, performed off the frame thread after initial setup.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use bevy::tasks::{AsyncComputeTaskPool, TaskPool};

use super::protocol::MAX_REQUEST_BYTES;

// Keep the original executor budget and allow a full checklist alongside it.
pub(super) const MAX_STATUS_BYTES: usize = (128 + 48) * 1024;

/// Serializes the last background exchange with final shutdown publication.
#[derive(Clone)]
pub(super) struct Writer {
    root: PathBuf,
    closed: Arc<Mutex<bool>>,
}

impl Writer {
    pub(super) fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            closed: Arc::new(Mutex::new(false)),
        }
    }

    pub(super) fn exchange(&self, status: &[u8]) -> Result<Option<Vec<u8>>, String> {
        let closed = self.closed.lock().map_err(|_| "Control writer failed")?;
        if *closed {
            return Err("Control writer is closed".into());
        }
        exchange(&self.root, status)
    }

    /// Called as the app is destroyed, after frame execution has stopped.
    pub(super) fn close(&self, status: &[u8]) -> Result<(), String> {
        let mut closed = self.closed.lock().map_err(|_| "Control writer failed")?;
        *closed = true;
        publish(&self.root, status)
            .map_err(|error| format!("Cannot publish final control status: {error}"))
    }

    /// One off-frame attempt; app shutdown never joins a stuck file worker.
    pub(super) fn finish(&self, status: Vec<u8>, limit: Duration) -> Result<(), String> {
        let writer = self.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        AsyncComputeTaskPool::get_or_init(TaskPool::new)
            .spawn(async move {
                let _ = sender.send(writer.close(&status));
            })
            .detach();
        receiver
            .recv_timeout(limit)
            .map_err(|_| "Final control status unavailable within shutdown deadline".to_owned())?
    }
}

/// Exclusive directory creation also prevents replay from a previous process.
pub(super) fn prepare(root: &Path) -> io::Result<()> {
    fs::create_dir(root)
}

fn ordinary(metadata: &fs::Metadata) -> bool {
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    true
}

fn read(root: &Path) -> io::Result<Option<Vec<u8>>> {
    let path = root.join("request.json");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
        Ok(metadata) if ordinary(&metadata) => (),
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Request must be an ordinary file",
            ));
        }
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_REQUEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Request exceeds the 256 KiB limit",
        ));
    }
    Ok(Some(bytes))
}

fn publish(root: &Path, status: &[u8]) -> io::Result<()> {
    if status.len() > MAX_STATUS_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Status exceeds its size limit",
        ));
    }
    let temporary = root.join("status.json.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(status)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, root.join("status.json"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

/// Publishing failure prevents accepting another request through this exchange.
pub(super) fn exchange(root: &Path, status: &[u8]) -> Result<Option<Vec<u8>>, String> {
    publish(root, status).map_err(|error| format!("Cannot publish control status: {error}"))?;
    read(root).map_err(|error| format!("Cannot read control request: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "eq-control-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn ownership_is_exclusive_and_each_exchange_publishes_a_complete_snapshot() {
        let root = directory();
        prepare(&root).unwrap();
        assert_eq!(
            prepare(&root).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(exchange(&root, b"{\"revision\":1}").unwrap(), None);
        fs::write(root.join("request.pending"), b"request").unwrap();
        fs::rename(root.join("request.pending"), root.join("request.json")).unwrap();
        assert_eq!(
            exchange(&root, b"{\"revision\":2}").unwrap(),
            Some(b"request".to_vec())
        );
        assert_eq!(
            fs::read(root.join("status.json")).unwrap(),
            b"{\"revision\":2}"
        );
        assert!(!root.join("status.json.tmp").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn oversized_or_non_file_inboxes_never_dispatch_and_preserve_status() {
        let root = directory();
        prepare(&root).unwrap();
        fs::write(root.join("request.json"), vec![b' '; MAX_REQUEST_BYTES + 1]).unwrap();
        assert!(exchange(&root, b"{}").unwrap_err().contains("256 KiB"));
        assert_eq!(fs::read(root.join("status.json")).unwrap(), b"{}");
        fs::remove_file(root.join("request.json")).unwrap();
        fs::create_dir(root.join("request.json")).unwrap();
        assert!(
            exchange(&root, b"{}")
                .unwrap_err()
                .contains("ordinary file")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_late_background_job_cannot_overwrite_the_closed_snapshot() {
        let root = directory();
        prepare(&root).unwrap();
        let writer = Writer::new(&root);
        let delayed = writer.clone();
        writer.close(b"{\"client_state\":\"closed\"}").unwrap();
        assert!(delayed.exchange(b"{\"client_state\":\"open\"}").is_err());
        assert_eq!(
            fs::read(root.join("status.json")).unwrap(),
            b"{\"client_state\":\"closed\"}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shutdown_has_a_deadline_instead_of_joining_a_blocked_exchange() {
        let root = directory();
        prepare(&root).unwrap();
        let writer = Writer::new(&root);
        let held = writer.closed.lock().unwrap();
        let began = std::time::Instant::now();
        assert!(
            writer
                .finish(
                    b"{\"client_state\":\"closed\"}".to_vec(),
                    Duration::from_millis(20)
                )
                .is_err()
        );
        assert!(began.elapsed() < Duration::from_secs(2));
        drop(held);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !root.join("status.json").exists() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(writer.exchange(b"old").is_err());
        assert_eq!(
            fs::read(root.join("status.json")).unwrap(),
            b"{\"client_state\":\"closed\"}"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
