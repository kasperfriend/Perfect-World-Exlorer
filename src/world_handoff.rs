//! Parent/child handoff for loading a selected map without dropping the current window.
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command},
    sync::mpsc::Sender,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const READY_ENV: &str = "PWE_WORLD_HANDOFF_READY_FILE";
const STATUS_ENV: &str = "PWE_WORLD_HANDOFF_STATUS_FILE";

#[derive(Debug)]
pub enum Message {
    Progress(String),
    Ready,
    Failed(String),
}

pub struct Files {
    ready: PathBuf,
    status: PathBuf,
}
impl Files {
    pub fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let stem = format!("pwe-world-handoff-{}-{nonce}", std::process::id());
        let root = std::env::temp_dir();
        Self {
            ready: root.join(format!("{stem}.ready")),
            status: root.join(format!("{stem}.status")),
        }
    }
    pub fn configure(&self, command: &mut Command) {
        command
            .env(READY_ENV, &self.ready)
            .env(STATUS_ENV, &self.status);
    }
    pub fn watch(self, mut child: Child, tx: Sender<Message>) {
        std::thread::Builder::new()
            .name("world-handoff".into())
            .spawn(move || {
                let mut previous = String::new();
                let started = Instant::now();
                loop {
                    if let Ok(status) = fs::read_to_string(&self.status) {
                        if status != previous {
                            previous.clone_from(&status);
                            let _ = tx.send(Message::Progress(status));
                        }
                    }
                    if self.ready.is_file() {
                        let _ = fs::remove_file(&self.ready);
                        let _ = fs::remove_file(&self.status);
                        let _ = tx.send(Message::Ready);
                        return;
                    }
                    // Never leave the original window on an unbounded loading
                    // overlay if a graphics driver or child event loop stalls.
                    if started.elapsed() >= Duration::from_secs(20 * 60) {
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = fs::remove_file(&self.status);
                        let _ = tx.send(Message::Failed(
                            "The new world did not become ready within 20 minutes.".into(),
                        ));
                        return;
                    }
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            let _ = fs::remove_file(&self.status);
                            let _ = tx.send(Message::Failed(format!(
                                "The new world process exited before it was ready ({status})."
                            )));
                            return;
                        }
                        Ok(None) => {}
                        Err(error) => {
                            let _ = tx.send(Message::Failed(format!(
                                "Could not monitor the new world process: {error}"
                            )));
                            return;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(80));
                }
            })
            .ok();
    }
}

pub fn is_handoff_child() -> bool {
    std::env::var_os(READY_ENV).is_some()
}

/// Publish a status from the loading process for the old window's loading overlay.
pub fn publish_progress(message: &str) {
    let Some(path) = std::env::var_os(STATUS_ENV).map(PathBuf::from) else {
        return;
    };
    // Status text is short; a direct write is enough and lets the parent poll
    // without introducing a second IPC service or named-pipe lifetime.
    let _ = fs::write(path, message.as_bytes());
}

/// Called only after the replacement renderer and its hidden window are ready.
pub fn signal_ready() {
    let Some(path) = std::env::var_os(READY_ENV).map(PathBuf::from) else {
        return;
    };
    let _ = fs::write(path, b"ready");
}
