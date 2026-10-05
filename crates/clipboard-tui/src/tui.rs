use std::io;
use std::sync::mpsc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use clipboard_core::AppHandle;
use clipboard_proto::event::{Event, EventType, PairingRejectReason};
use crossterm::event::{self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::clipboard_monitor::{ClipboardMonitor, ClipboardWriter, SystemClipboardWriter};
use crate::ui::{self, DeviceRow, HistoryRow, PairingPrompt, UiState};

/// Owns terminal mode: enters raw mode plus the alternate screen, and
/// restores both on drop so an early return from [`run`] (a failed clipboard
/// init, for instance) cannot leave the shell without echo or stuck in the
/// alternate screen.
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        // Constructed before entering the alternate screen so that any later
        // failure still leaves raw mode restored.
        let guard = Self;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

pub fn run(
    handle: AppHandle,
    poll_interval_ms: u64,
    pairing_timeout_secs: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let terminal_guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let mut state = UiState {
        paused: handle.is_paused()?,
        pairing_timeout_secs,
        ..UiState::default()
    };

    let trusted = handle.trusted_peers()?;
    for peer in &trusted {
        state.devices.push(DeviceRow {
            device_id: peer.device_id,
            name: peer.device_name.clone(),
            platform: peer
                .platform
                .unwrap_or_else(clipboard_proto::message::current_platform),
            discovered: false,
            last_seen: None,
            connected: false,
            trusted: true,
        });
    }

    let initial_history = handle.history(50)?;
    state.history_size_bytes = handle.history_size_bytes()?;
    state.history = initial_history
        .into_iter()
        .map(|e| HistoryRow {
            clipboard_id: e.clipboard_id,
            content_preview: e.content.clone(),
            created_at: e.created_at,
            origin_device_id: e.origin_device_id,
        })
        .collect();

    let mut event_rx = handle.event_tx().subscribe();
    let (tx, rx) = mpsc::channel();

    let shutdown = Arc::new(AtomicBool::new(false));
    let input_tx = tx.clone();
    let input_shutdown = shutdown.clone();
    let input_thread = std::thread::spawn(move || {
        while !input_shutdown.load(Ordering::Relaxed) {
            if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                match event::read() {
                    Ok(CrosstermEvent::Key(key)) => {
                        if key.kind == KeyEventKind::Press {
                            let _ = input_tx.send(InputEvent::Key(key));
                        }
                    }
                    Ok(CrosstermEvent::Resize(..)) => {
                        let _ = input_tx.send(InputEvent::Resize);
                    }
                    _ => {}
                }
            }
        }
    });

    let core_tx = tx.clone();
    let core_shutdown = shutdown.clone();
    let core_thread = std::thread::spawn(move || {
        while !core_shutdown.load(Ordering::Relaxed) {
            match event_rx.try_recv() {
                Ok(event) => {
                    let _ = core_tx.send(InputEvent::Core(event));
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                    tracing::warn!("tui: skipped {} core events", skipped);
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
            }
        }
    });

    let cmd_tx = handle.app_cmd_tx();
    let monitor_thread = ClipboardMonitor::new(poll_interval_ms).spawn(cmd_tx, shutdown.clone());
    let mut clipboard =
        SystemClipboardWriter::new().map_err(|e| format!("failed to initialize clipboard: {e}"))?;

    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(200);

    loop {
        terminal.draw(|f| ui::render(f, &state))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::from_millis(0));

        match rx.recv_timeout(timeout) {
            Ok(InputEvent::Key(key)) => {
                if handle_key(&mut state, &handle, &mut clipboard, key.code) {
                    break;
                }
            }
            Ok(InputEvent::Core(event)) => {
                handle_core_event(&mut state, &handle, &mut clipboard, event);
            }
            // Nothing in state to update: reaching the top of the loop
            // calls terminal.draw(), which re-reads the terminal size, so
            // this arm exists purely to repaint now instead of waiting for
            // the periodic tick.
            Ok(InputEvent::Resize) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }

        state.expire_status();
    }

    shutdown.store(true, Ordering::Relaxed);
    let _ = input_thread.join();
    let _ = core_thread.join();
    let _ = monitor_thread.join();
    drop(terminal_guard);
    terminal.show_cursor()?;
    handle.stop();
    Ok(())
}

enum InputEvent {
    Key(KeyEvent),
    Core(Event),
    /// A terminal resize. The reader used to discard these, which left the
    /// redraw to the periodic tick rather than making it immediate.
    Resize,
}

fn handle_key(
    state: &mut UiState,
    handle: &AppHandle,
    clipboard: &mut dyn ClipboardWriter,
    code: KeyCode,
) -> bool {
    match code {
        KeyCode::Char('q') | KeyCode::Char('Q') => {
            return true;
        }
        KeyCode::Tab => {
            state.active_pane = (state.active_pane + 1) % 3;
        }
        KeyCode::Up => match state.active_pane {
            0 => {
                if state.selected_device > 0 {
                    state.selected_device -= 1;
                }
            }
            1 if state.selected_history > 0 => state.selected_history -= 1,
            _ => {}
        },
        KeyCode::Down => match state.active_pane {
            0 => {
                if state.selected_device + 1 < state.devices.len() {
                    state.selected_device += 1;
                }
            }
            1 if state.selected_history + 1 < state.history.len() => state.selected_history += 1,
            _ => {}
        },
        KeyCode::Enter => {
            if state.active_pane == 1 && state.selected_history < state.history.len() {
                let entry = &state.history[state.selected_history];
                handle.restore_history_entry(entry.clipboard_id);
                match clipboard.write_text(&entry.content_preview) {
                    Ok(()) => state.set_status("Restored to clipboard"),
                    Err(error) => state.set_status(format!("Clipboard write failed: {error}")),
                }
            }
        }
        KeyCode::Char('p') | KeyCode::Char('P') => {
            if state.active_pane == 0 && state.selected_device < state.devices.len() {
                let device = &state.devices[state.selected_device];
                if device.discovered && !device.trusted {
                    handle.request_pairing(device.device_id);
                    state.set_status(format!("Pairing request sent to {}", device.name));
                }
            }
        }
        KeyCode::Char('f') | KeyCode::Char('F') => {
            if state.active_pane == 0 && state.selected_device < state.devices.len() {
                let device = state.devices[state.selected_device].clone();
                if device.trusted {
                    let name = device.name.clone();
                    if handle.forget_device(device.device_id).is_ok() {
                        state.set_status(format!("Forgot {}", name));
                        state.devices.retain(|d| d.device_id != device.device_id);
                        if state.selected_device >= state.devices.len() && state.selected_device > 0
                        {
                            state.selected_device -= 1;
                        }
                    } else {
                        state.set_status(format!("Could not forget {}", name));
                    }
                }
            }
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            if let Some(prompt) = state.pairing_prompts.first() {
                // Removal and the status line belong to the
                // PairingAccepted/PairingFailed event, which is where the
                // device name is still available.
                handle.approve_pairing(prompt.device_id);
            }
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            if let Some(prompt) = state.pairing_prompts.first() {
                // Same as approve: PairingRejected removes the prompt and
                // reports the outcome.
                handle.reject_pairing(prompt.device_id, "rejected by user");
            }
        }
        KeyCode::Char('s') | KeyCode::Char('S') => {
            state.paused = !state.paused;
            handle.set_paused(state.paused);
            state.set_status(if state.paused {
                "Sync paused".to_string()
            } else {
                "Sync resumed".to_string()
            });
        }
        _ => {}
    }
    false
}

fn handle_core_event(
    state: &mut UiState,
    handle: &AppHandle,
    clipboard: &mut dyn ClipboardWriter,
    event: Event,
) {
    match event.event_type {
        EventType::DeviceDiscovered(p) => {
            let existing = state
                .devices
                .iter_mut()
                .find(|d| d.device_id == p.device_id);
            if let Some(device) = existing {
                device.discovered = true;
                device.last_seen = Some(Instant::now());
                // Discovery proves reachability, not a live connection:
                // DeviceConnected owns that flag (doc 09:128).
            } else {
                state.devices.push(DeviceRow {
                    device_id: p.device_id,
                    name: p.device_name.clone(),
                    platform: p.platform,
                    discovered: true,
                    last_seen: Some(Instant::now()),
                    connected: false,
                    trusted: false,
                });
            }
            if state.selected_device >= state.devices.len() {
                state.selected_device = state.devices.len() - 1;
            }
        }
        EventType::DeviceConnected(p) => {
            if let Some(device) = state
                .devices
                .iter_mut()
                .find(|d| d.device_id == p.device_id)
            {
                device.connected = true;
            }
        }
        EventType::DeviceDisconnected(p) => {
            if let Some(device) = state
                .devices
                .iter_mut()
                .find(|d| d.device_id == p.device_id)
            {
                device.connected = false;
            }
        }
        EventType::PairingRequested(p) => {
            // Later pairing events carry ids only (no name), so make sure the
            // requester is listed by name while we still have it.
            if !state.devices.iter().any(|d| d.device_id == p.device_id) {
                state.devices.push(DeviceRow {
                    device_id: p.device_id,
                    name: p.device_name.clone(),
                    platform: p.platform,
                    discovered: false,
                    last_seen: None,
                    connected: false,
                    trusted: false,
                });
            }
            state.pairing_prompts.push(PairingPrompt {
                request_id: p.request_id,
                device_id: p.device_id,
                device_name: p.device_name.clone(),
                received_at: Instant::now(),
            });
        }
        EventType::PairingAccepted(p) => {
            let label = state
                .pairing_prompts
                .iter()
                .find(|pr| pr.request_id == p.request_id)
                .map(|pr| pr.device_name.clone())
                .unwrap_or_else(|| state.device_label(p.device_id).to_string());
            state
                .pairing_prompts
                .retain(|pr| pr.request_id != p.request_id);
            state.set_status(format!("Paired with {label}"));
            let trusted = match handle.trusted_peers() {
                Ok(trusted) => trusted,
                Err(error) => {
                    state.set_status(format!("Could not refresh trusted devices: {error}"));
                    return;
                }
            };
            for peer in &trusted {
                if !state.devices.iter().any(|d| d.device_id == peer.device_id) {
                    state.devices.push(DeviceRow {
                        device_id: peer.device_id,
                        name: peer.device_name.clone(),
                        platform: peer
                            .platform
                            .unwrap_or_else(clipboard_proto::message::current_platform),
                        discovered: false,
                        last_seen: None,
                        connected: false,
                        trusted: true,
                    });
                }
            }
            for device in &mut state.devices {
                device.trusted = trusted.iter().any(|t| t.device_id == device.device_id);
            }
        }
        EventType::PairingRejected(p) => {
            let label = state
                .pairing_prompts
                .iter()
                .find(|pr| pr.request_id == p.request_id)
                .map(|pr| pr.device_name.clone())
                .unwrap_or_else(|| state.device_label(p.device_id).to_string());
            state
                .pairing_prompts
                .retain(|pr| pr.request_id != p.request_id);
            // One event covers three outcomes: our own reject, the peer's
            // deny, and the configured timeout (the core emits it for all
            // three), so the pairing outcome is always reported (doc 09:157).
            let message = match p.reason {
                PairingRejectReason::Timeout => format!("Pairing with {label} timed out"),
                PairingRejectReason::UserDenied => format!("Pairing with {label} rejected"),
                PairingRejectReason::InternalError => format!("Pairing with {label} failed"),
            };
            state.set_status(message);
        }
        EventType::PairingFailed(p) => {
            // The payload carries no device id, so the name can only come
            // from the matching prompt.
            let label = state
                .pairing_prompts
                .iter()
                .find(|pr| pr.request_id == p.request_id)
                .map(|pr| pr.device_name.clone())
                .unwrap_or_else(|| "the peer".to_string());
            state
                .pairing_prompts
                .retain(|pr| pr.request_id != p.request_id);
            let message = format!("Pairing with {label} failed: {}", p.error);
            state.set_status(message.clone());
            state.last_error = Some(message);
        }
        EventType::DeviceConnectionFailed(p) => {
            // Retries follow the backoff schedule, so only the persistent
            // line is updated: a transient flash would fire on every attempt.
            let message = format!(
                "Connection to {} failed: {}",
                state.device_label(p.device_id),
                p.error
            );
            state.last_error = Some(message);
        }
        EventType::ClipboardUpdatedFromRemote(p) => match clipboard.write_text(&p.content) {
            Ok(()) => {
                let origin = state.device_label(p.origin_device_id).to_string();
                state.set_status(format!("Received from {origin}"));
                refresh_history(state, handle);
            }
            Err(error) => state.set_status(format!("Clipboard write failed: {error}")),
        },
        EventType::ClipboardHistoryUpdated(_) => {
            refresh_history(state, handle);
        }
        EventType::ClipboardRestored(_) => {
            state.set_status("Entry restored");
        }
        EventType::NetworkError(p) => {
            let message = format!("Network error: {}", p.error);
            state.set_status(message.clone());
            state.last_error = Some(message);
        }
        EventType::ProtocolError(p) => {
            let message = format!("Protocol error: {}", p.description);
            state.set_status(message.clone());
            state.last_error = Some(message);
        }
        _ => {}
    }
}

fn refresh_history(state: &mut UiState, handle: &AppHandle) {
    let history = match handle.history(50) {
        Ok(history) => history,
        Err(error) => {
            state.set_status(format!("Could not refresh history: {error}"));
            return;
        }
    };
    let history_size = match handle.history_size_bytes() {
        Ok(size) => size,
        Err(error) => {
            state.set_status(format!("Could not refresh history size: {error}"));
            return;
        }
    };
    state.history_size_bytes = history_size;
    state.history = history
        .into_iter()
        .map(|entry| HistoryRow {
            clipboard_id: entry.clipboard_id,
            content_preview: entry.content,
            created_at: entry.created_at,
            origin_device_id: entry.origin_device_id,
        })
        .collect();
}
