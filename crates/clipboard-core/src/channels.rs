use clipboard_proto::event::Event;
use clipboard_proto::message::Envelope;
use clipboard_proto::types::PeerInfo;
use tokio::sync::{broadcast, mpsc, oneshot};

// ---------------------------------------------------------------------------
// App commands — AppHandle → coordinator
// ---------------------------------------------------------------------------

pub type AppCommandTx = mpsc::UnboundedSender<AppCommand>;
pub type AppCommandRx = mpsc::UnboundedReceiver<AppCommand>;

pub enum AppCommand {
    RequestPairing {
        device_id: uuid::Uuid,
    },
    ApprovePairing {
        device_id: uuid::Uuid,
    },
    RejectPairing {
        device_id: uuid::Uuid,
        reason: String,
    },
    ClipboardChanged {
        content: String,
        content_type: String,
    },
    RestoreHistoryEntry {
        clipboard_id: uuid::Uuid,
    },
    ListHistory {
        limit: usize,
        response: oneshot::Sender<Vec<clipboard_proto::types::HistoryEntry>>,
    },
    ClearHistory {
        response: oneshot::Sender<Result<(), String>>,
    },
    IsPaused {
        response: oneshot::Sender<bool>,
    },
    SetPaused {
        paused: bool,
    },
    Stop {
        response: oneshot::Sender<()>,
    },
}

// ---------------------------------------------------------------------------
// Messaging commands — coordinator → messaging
// ---------------------------------------------------------------------------

pub type MessagingCommandTx = mpsc::UnboundedSender<MessagingCommand>;
pub type MessagingCommandRx = mpsc::UnboundedReceiver<MessagingCommand>;

pub enum MessagingCommand {
    ConnectTo {
        peer: PeerInfo,
    },
    Send {
        device_id: uuid::Uuid,
        envelope: Envelope,
    },
    ReloadTrusted,
    Stop,
}

// ---------------------------------------------------------------------------
// Inbound protocol mailbox — messaging → coordinator
// ---------------------------------------------------------------------------

pub type InboundEnvelopeTx = mpsc::UnboundedSender<InboundEnvelope>;
pub type InboundEnvelopeRx = mpsc::UnboundedReceiver<InboundEnvelope>;

#[derive(Debug, Clone)]
pub struct InboundEnvelope {
    pub device_id: uuid::Uuid,
    pub envelope: Envelope,
}

// ---------------------------------------------------------------------------
// Event bus — any module → UI/coordinator
// ---------------------------------------------------------------------------

pub type EventBusTx = broadcast::Sender<Event>;
pub type EventBusRx = broadcast::Receiver<Event>;

pub fn event_bus(capacity: usize) -> (EventBusTx, EventBusRx) {
    broadcast::channel(capacity)
}

// ---------------------------------------------------------------------------
// Channel creation helpers
// ---------------------------------------------------------------------------

pub fn app_command_channel() -> (AppCommandTx, AppCommandRx) {
    mpsc::unbounded_channel()
}

pub fn messaging_command_channel() -> (MessagingCommandTx, MessagingCommandRx) {
    mpsc::unbounded_channel()
}

pub fn inbound_envelope_channel() -> (InboundEnvelopeTx, InboundEnvelopeRx) {
    mpsc::unbounded_channel()
}

pub struct Channels {
    pub app_cmd_tx: AppCommandTx,
    pub app_cmd_rx: AppCommandRx,
    pub msg_cmd_tx: MessagingCommandTx,
    pub msg_cmd_rx: MessagingCommandRx,
    pub inbound_tx: InboundEnvelopeTx,
    pub inbound_rx: InboundEnvelopeRx,
    pub event_tx: EventBusTx,
    pub event_rx: EventBusRx,
}

impl Default for Channels {
    fn default() -> Self {
        Self::new()
    }
}

impl Channels {
    pub fn new() -> Self {
        let (app_cmd_tx, app_cmd_rx) = app_command_channel();
        let (msg_cmd_tx, msg_cmd_rx) = messaging_command_channel();
        let (inbound_tx, inbound_rx) = inbound_envelope_channel();
        let (event_tx, event_rx) = event_bus(512);
        Self {
            app_cmd_tx,
            app_cmd_rx,
            msg_cmd_tx,
            msg_cmd_rx,
            inbound_tx,
            inbound_rx,
            event_tx,
            event_rx,
        }
    }
}
