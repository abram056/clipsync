use std::time::{Duration, Instant};

use clipboard_proto::types::Platform;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

#[derive(Debug, Clone)]
pub struct DeviceRow {
    pub device_id: uuid::Uuid,
    pub name: String,
    pub platform: Platform,
    pub discovered: bool,
    pub last_seen: Option<Instant>,
    pub connected: bool,
    pub trusted: bool,
}

#[derive(Debug, Clone)]
pub struct HistoryRow {
    pub clipboard_id: uuid::Uuid,
    pub content_preview: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub origin_device_id: uuid::Uuid,
}

#[derive(Debug, Clone)]
pub struct PairingPrompt {
    pub request_id: uuid::Uuid,
    pub device_id: uuid::Uuid,
    pub device_name: String,
    pub received_at: Instant,
}

#[derive(Default)]
pub struct UiState {
    pub devices: Vec<DeviceRow>,
    pub history: Vec<HistoryRow>,
    pub history_size_bytes: u64,
    pub paused: bool,
    pub pairing_prompts: Vec<PairingPrompt>,
    pub selected_device: usize,
    pub selected_history: usize,
    pub active_pane: usize,
    pub status_message: Option<String>,
    pub status_expires_at: Option<Instant>,
    /// Last error, shown until a newer one replaces it (doc 09:130).
    pub last_error: Option<String>,
    /// The configured pairing timeout, so the overlay countdown tracks the
    /// value the core actually enforces rather than a hardcoded guess.
    pub pairing_timeout_secs: u64,
}

pub fn render(f: &mut Frame, state: &UiState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(45),
            Constraint::Percentage(25),
        ])
        .split(f.size());

    render_devices(f, state, chunks[0]);
    render_history(f, state, chunks[1]);
    render_status(f, state, chunks[2]);

    if !state.pairing_prompts.is_empty() {
        render_pairing_overlay(f, state);
    }
}

fn render_devices(f: &mut Frame, state: &UiState, area: Rect) {
    let title = if state.active_pane == 0 {
        " Devices [Tab: switch] "
    } else {
        " Devices "
    };

    let mut items: Vec<ListItem> = Vec::new();
    for (i, device) in state.devices.iter().enumerate() {
        let indicator = if device.connected {
            Span::styled("● ", Style::default().fg(Color::Green))
        } else if device.discovered {
            Span::styled("○ ", Style::default().fg(Color::DarkGray))
        } else {
            Span::styled("· ", Style::default().fg(Color::DarkGray))
        };

        let name = if device.trusted {
            Span::styled(device.name.clone(), Style::default().fg(Color::Cyan))
        } else {
            Span::raw(device.name.clone())
        };

        let suffix = if device.trusted {
            Span::styled(" [trusted]", Style::default().fg(Color::DarkGray))
        } else {
            Span::raw("")
        };

        let style = if state.active_pane == 0 && i == state.selected_device {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };

        let platform = Span::styled(
            format!(" ({})", device.platform),
            Style::default().fg(Color::DarkGray),
        );
        let line = Line::from(vec![indicator, name, platform, suffix]);
        items.push(ListItem::new(line).style(style));
    }

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(list, area);
}

fn render_history(f: &mut Frame, state: &UiState, area: Rect) {
    let title = if state.active_pane == 1 {
        " History [Tab: switch] "
    } else {
        " History "
    };

    let mut items: Vec<ListItem> = Vec::new();
    for (i, entry) in state.history.iter().enumerate() {
        let time = entry.created_at.with_timezone(&chrono::Local);
        let time_str = time.format("%H:%M").to_string();

        let preview: String = entry.content_preview.chars().take(40).collect();
        let line = Line::from(vec![
            Span::styled(
                format!("{} ", time_str),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(preview),
            Span::styled(
                format!(" · {}", state.device_label(entry.origin_device_id)),
                Style::default().fg(Color::DarkGray),
            ),
        ]);

        let style = if state.active_pane == 1 && i == state.selected_history {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };

        items.push(ListItem::new(line).style(style));
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "  No clipboard history",
            Style::default().fg(Color::DarkGray),
        ))));
    }

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(list, area);
}

impl UiState {
    /// Name for a device id, falling back to the wording doc 09 requires
    /// when the id is not in the device list.
    pub fn device_label(&self, device_id: uuid::Uuid) -> &str {
        self.devices
            .iter()
            .find(|device| device.device_id == device_id)
            .map(|device| device.name.as_str())
            .unwrap_or("Unknown device")
    }

    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status_message = Some(message.into());
        self.status_expires_at = Some(Instant::now() + Duration::from_secs(4));
    }

    pub fn expire_status(&mut self) {
        if self
            .status_expires_at
            .is_some_and(|expiry| Instant::now() >= expiry)
        {
            self.status_message = None;
            self.status_expires_at = None;
        }
    }
}

fn render_status(f: &mut Frame, state: &UiState, area: Rect) {
    let sync_status = if state.paused { "OFF" } else { "ON" };
    let sync_color = if state.paused {
        Color::Red
    } else {
        Color::Green
    };

    let peer_count = state.devices.iter().filter(|d| d.connected).count();

    let size_bytes = state.history_size_bytes;
    let size_str = if size_bytes >= 1_048_576 {
        format!("{:.1} MB", size_bytes as f64 / 1_048_576.0)
    } else if size_bytes >= 1024 {
        format!("{:.1} KB", size_bytes as f64 / 1024.0)
    } else {
        format!("{} B", size_bytes)
    };

    // Layout order matters: on a standard 24-line terminal the status pane
    // has only ~4 usable rows, so the transient message and the persistent
    // error sit directly after the stats — the old code spliced them into
    // the controls block (index 12), where they were never visible.
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Sync: ", Style::default()),
            Span::styled(sync_status, Style::default().fg(sync_color)),
        ]),
        Line::from(vec![
            Span::styled("Peers: ", Style::default()),
            Span::raw(format!("{}", peer_count)),
        ]),
        Line::from(vec![
            Span::styled("History: ", Style::default()),
            Span::raw(format!("{}/2 MB", size_str)),
        ]),
    ];

    if let Some(msg) = &state.status_message {
        lines.push(Line::from(Span::styled(
            msg.as_str(),
            Style::default().fg(Color::Yellow),
        )));
    }

    if let Some(error) = &state.last_error {
        lines.push(Line::from(vec![
            Span::styled("Last error: ", Style::default().fg(Color::Red)),
            Span::styled(error.as_str(), Style::default().fg(Color::Red)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Controls:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(" Tab     Switch pane"));
    lines.push(Line::from(" ↑/↓     Move selection"));
    lines.push(Line::from(" Enter   Restore entry"));
    lines.push(Line::from(" P       Pair device"));
    lines.push(Line::from(" F       Forget device"));
    lines.push(Line::from(" A/R     Approve/Reject"));
    lines.push(Line::from(" S       Pause/Resume"));
    lines.push(Line::from(" Q       Quit"));

    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Status "))
        .wrap(Wrap { trim: true });
    f.render_widget(paragraph, area);
}

fn render_pairing_overlay(f: &mut Frame, state: &UiState) {
    if let Some(prompt) = state.pairing_prompts.first() {
        let area = f.size();
        // Saturating maths: a tiny terminal (e.g. while resizing) must not
        // underflow and abort the app.
        let popup_width = 40.min(area.width.saturating_sub(4));
        let popup_height = 7.min(area.height);
        let x = (area.width - popup_width) / 2;
        let y = (area.height - popup_height) / 2;
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        let lines = vec![
            Line::from(Span::styled(
                "Pairing Request",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::raw("From: "),
                Span::styled(prompt.device_name.clone(), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(Span::styled(
                format!(
                    "Expires in {}s",
                    state
                        .pairing_timeout_secs
                        .saturating_sub(prompt.received_at.elapsed().as_secs())
                ),
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "[A] Approve   [R] Reject",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Pairing ")
                    .style(Style::default().fg(Color::Yellow)),
            )
            .wrap(Wrap { trim: true });
        f.render_widget(paragraph, popup_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render `state` into a TestBackend buffer and return it as text; each
    /// row comes back as a quoted line via TestBackend's Display impl.
    fn render_screen(state: &UiState, width: u16, height: u16) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))
                .expect("TestBackend");
        terminal.draw(|f| super::render(f, state)).expect("render");
        format!("{}", terminal.backend())
    }

    fn device_row(
        id: uuid::Uuid,
        name: &str,
        discovered: bool,
        connected: bool,
        trusted: bool,
    ) -> DeviceRow {
        DeviceRow {
            device_id: id,
            name: name.to_string(),
            platform: Platform::Linux,
            discovered,
            last_seen: None,
            connected,
            trusted,
        }
    }

    /// doc 09:128 distinguishes discovered (○) from connected (●). Discovery
    /// used to set `connected`, so every unpaired device rendered as live.
    #[test]
    fn discovered_peer_is_not_rendered_as_connected() {
        let mut state = UiState::default();
        state.devices.push(device_row(
            uuid::Uuid::new_v4(),
            "Phone",
            true,
            false,
            false,
        ));

        let screen = render_screen(&state, 80, 24);
        assert!(screen.contains('○'), "discovered device: {screen}");
        assert!(!screen.contains('●'), "must not look connected: {screen}");
    }

    #[test]
    fn peer_count_counts_only_connected_devices() {
        let mut state = UiState::default();
        state
            .devices
            .push(device_row(uuid::Uuid::new_v4(), "Laptop", true, true, true));
        state.devices.push(device_row(
            uuid::Uuid::new_v4(),
            "Phone",
            true,
            false,
            false,
        ));
        state.devices.push(device_row(
            uuid::Uuid::new_v4(),
            "Tablet",
            false,
            false,
            true,
        ));

        let screen = render_screen(&state, 80, 24);
        assert!(screen.contains("Peers: 1"), "{screen}");
        assert!(screen.contains('●'), "the connected one: {screen}");
        assert!(screen.contains('○'), "the discovered one: {screen}");
    }

    #[test]
    fn device_label_resolves_names_and_falls_back_to_unknown() {
        let mut state = UiState::default();
        let laptop = uuid::Uuid::new_v4();
        state
            .devices
            .push(device_row(laptop, "Laptop", false, false, true));

        assert_eq!(state.device_label(laptop), "Laptop");
        // doc 09:58 wording for an origin that is not in the list.
        assert_eq!(state.device_label(uuid::Uuid::new_v4()), "Unknown device");
    }

    /// The old code spliced the message into the controls block at index 12,
    /// which a 24-line terminal never reaches.
    #[test]
    fn status_message_is_visible_on_a_standard_terminal() {
        let mut state = UiState::default();
        state.set_status("Sync paused");

        let screen = render_screen(&state, 80, 24);
        assert!(screen.contains("Sync paused"), "{screen}");
    }

    #[test]
    fn last_error_is_visible_on_a_standard_terminal() {
        let state = UiState {
            last_error: Some("boom".to_string()),
            ..UiState::default()
        };

        let screen = render_screen(&state, 80, 24);
        assert!(screen.contains("Last error: boom"), "{screen}");
    }

    #[test]
    fn status_precedes_last_error_precedes_controls() {
        let mut state = UiState {
            last_error: Some("boom".to_string()),
            ..UiState::default()
        };
        state.set_status("Sync paused");

        let screen = render_screen(&state, 80, 50);
        let status_at = screen.find("Sync paused").expect("status rendered");
        let error_at = screen.find("Last error: boom").expect("error rendered");
        let controls_at = screen.find("Controls:").expect("controls rendered");
        assert!(status_at < error_at, "{screen}");
        assert!(error_at < controls_at, "{screen}");
    }

    fn prompt_with_age(age: Duration) -> PairingPrompt {
        PairingPrompt {
            request_id: uuid::Uuid::new_v4(),
            device_id: uuid::Uuid::new_v4(),
            device_name: "Phone".to_string(),
            received_at: Instant::now() - age,
        }
    }

    /// The overlay used to count down from a hardcoded 30 s while the core
    /// times out at sync.pairing_timeout_secs (default 60). The expected
    /// value is derived from the same prompt after rendering, so the test
    /// cannot flake on timing.
    #[test]
    fn pairing_countdown_tracks_the_configured_timeout() {
        let state = UiState {
            pairing_timeout_secs: 45,
            pairing_prompts: vec![prompt_with_age(Duration::from_secs(10))],
            ..UiState::default()
        };

        let screen = render_screen(&state, 80, 24);
        let expected =
            45u64.saturating_sub(state.pairing_prompts[0].received_at.elapsed().as_secs());
        assert_eq!(expected, 35, "prompt age must stay inside one second");
        assert!(
            screen.contains(&format!("Expires in {expected}s")),
            "{screen}"
        );
        assert!(
            !screen.contains("Expires in 20s"),
            "hardcoded 30 s countdown is back: {screen}"
        );
    }

    /// `area.width - 4` and `area.height - 7` used to underflow on a
    /// terminal smaller than the popup, aborting the app mid-resize.
    #[test]
    fn pairing_overlay_survives_a_tiny_terminal() {
        let state = UiState {
            pairing_timeout_secs: 60,
            pairing_prompts: vec![prompt_with_age(Duration::ZERO)],
            ..UiState::default()
        };

        let screen = render_screen(&state, 3, 4);
        assert!(!screen.is_empty());
    }
}
