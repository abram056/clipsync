use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use arboard::Clipboard;
use clipboard_core::channels::AppCommandTx;

pub struct ClipboardMonitor {
    poll_interval: Duration,
}

/// Clipboard output is owned by the frontend so tests do not need a desktop
/// clipboard implementation.
pub trait ClipboardWriter {
    fn write_text(&mut self, text: &str) -> Result<(), arboard::Error>;
}

pub struct SystemClipboardWriter {
    clipboard: Clipboard,
}

impl SystemClipboardWriter {
    pub fn new() -> Result<Self, arboard::Error> {
        Ok(Self {
            clipboard: Clipboard::new()?,
        })
    }
}

impl ClipboardWriter for SystemClipboardWriter {
    fn write_text(&mut self, text: &str) -> Result<(), arboard::Error> {
        self.clipboard.set_text(text)
    }
}

impl ClipboardMonitor {
    pub fn new(poll_interval_ms: u64) -> Self {
        Self {
            poll_interval: Duration::from_millis(poll_interval_ms),
        }
    }

    pub fn spawn(
        self,
        cmd_tx: AppCommandTx,
        shutdown: Arc<AtomicBool>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let mut clipboard = match Clipboard::new() {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("clipboard_monitor: failed to initialize clipboard: {}", e);
                    return;
                }
            };

            let mut last_content = String::new();

            while !shutdown.load(Ordering::Relaxed) {
                match clipboard.get_text() {
                    Ok(text) => {
                        if !text.is_empty() && text != last_content {
                            let _ = cmd_tx.send(
                                clipboard_core::channels::AppCommand::ClipboardChanged {
                                    content: text.clone(),
                                    content_type: "text/plain".to_string(),
                                },
                            );
                            last_content = text;
                        }
                    }
                    Err(arboard::Error::ClipboardNotSupported) => {
                        tracing::debug!(
                            "clipboard_monitor: clipboard not supported on this platform"
                        );
                        std::thread::sleep(self.poll_interval);
                        continue;
                    }
                    Err(e) => {
                        tracing::debug!("clipboard_monitor: error reading clipboard: {}", e);
                    }
                }
                std::thread::sleep(self.poll_interval);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ClipboardWriter;

    struct MockClipboard {
        written: Option<String>,
    }

    impl ClipboardWriter for MockClipboard {
        fn write_text(&mut self, text: &str) -> Result<(), arboard::Error> {
            self.written = Some(text.to_owned());
            Ok(())
        }
    }

    #[test]
    fn writer_adapter_accepts_text_without_system_clipboard() {
        let mut clipboard = MockClipboard { written: None };
        clipboard.write_text("remote update").unwrap();
        assert_eq!(clipboard.written.as_deref(), Some("remote update"));
    }
}
