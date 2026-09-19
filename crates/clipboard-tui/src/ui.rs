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
                format!(" · {}", device_name(state, entry.origin_device_id)),
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

fn device_name(state: &UiState, device_id: uuid::Uuid) -> &str {
    state
        .devices
        .iter()
        .find(|device| device.device_id == device_id)
        .map(|device| device.name.as_str())
        .unwrap_or("Unknown device")
}

impl UiState {
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

    let lines = vec![
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
        Line::from(""),
        Line::from(Span::styled(
            "Controls:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(" Tab     Switch pane"),
        Line::from(" ↑/↓     Move selection"),
        Line::from(" Enter   Restore entry"),
        Line::from(" P       Pair device"),
        Line::from(" F       Forget device"),
        Line::from(" A/R     Approve/Reject"),
        Line::from(" S       Pause/Resume"),
        Line::from(" Q       Quit"),
    ];

    if let Some(ref msg) = state.status_message {
        let mut lines = lines.clone();
        lines.insert(12, Line::from(""));
        lines.insert(
            13,
            Line::from(Span::styled(
                msg.as_str(),
                Style::default().fg(Color::Yellow),
            )),
        );
    }

    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Status "))
        .wrap(Wrap { trim: true });
    f.render_widget(paragraph, area);
}

fn render_pairing_overlay(f: &mut Frame, state: &UiState) {
    if let Some(prompt) = state.pairing_prompts.first() {
        let area = f.size();
        let popup_width = 40.min(area.width - 4);
        let popup_height = 7;
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
                    30u64.saturating_sub(prompt.received_at.elapsed().as_secs())
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
