//! Chat der Anlage über XMPP.
//!
//! Die Anlage betreibt Openfire auf Port 5222 mit STARTTLS. Benutzername ist
//! der lokale Teil der Jabber-ID aus `ChatService.GetChatId`, Passwort das
//! OAuth-Access-Token. Weil das Token nach wenigen Minuten abläuft, baut
//! [`Chat`] jede Verbindung mit dem aktuellen Token neu auf.
//!
//! Unterstützt: Kontaktliste mit Präsenz, Einzelchats, Nachrichten anderer
//! Geräte (Carbons), verzögerte Zustellung, Verlauf aus dem Serverarchiv
//! (XEP-0136) und ein lokaler Verlauf als JSON-Datei.

mod history;
mod xml;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_xmpp::connect::DnsConfig;
use tokio_xmpp::jid::{BareJid, Jid};
use tokio_xmpp::minidom::Element;
use tokio_xmpp::parsers::message::{Lang, Message, MessageType};
use tokio_xmpp::parsers::presence::{Presence, Show, Type as PresenceType};
use tokio_xmpp::xmlstream::Timeouts;
use tokio_xmpp::{Client, Event, Stanza};

pub use history::History;

const PORT: u16 = 5222;
const MAX_BACKOFF: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ungültige Jabber-ID: {0}")]
    Jid(String),
}

/// Ein Eintrag der Kontaktliste
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Contact {
    pub jid: String,
    pub name: String,
    /// "online", "chat", "away", "xa", "dnd" oder "offline"
    pub show: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub id: String,
    /// Gesprächspartner (bare JID)
    pub peer: String,
    pub outgoing: bool,
    pub body: String,
    /// Unix-Zeit in Millisekunden
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatEvent {
    State {
        online: bool,
        detail: String,
    },
    Roster {
        contacts: Vec<Contact>,
    },
    /// Neue Nachricht. `notify` ist gesetzt, wenn sie gerade eingetroffen ist
    /// und von jemand anderem stammt.
    Message {
        message: ChatMessage,
        notify: bool,
    },
    /// Verlauf eines Gesprächs hat sich geändert (z. B. aus dem Archiv)
    History {
        peer: String,
        messages: Vec<ChatMessage>,
    },
}

enum Command {
    Send {
        to: String,
        body: String,
    },
    LoadArchive {
        peer: String,
    },
    Presence(Own),
    /// Abmelden mit Statustext, danach endet die Verbindung
    Offline {
        status: String,
    },
}

/// Eigener Status, wie ihn die anderen sehen
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Own {
    pub away: bool,
    pub status: String,
}

impl Own {
    fn presence(&self) -> Presence {
        let mut p = Presence::available();
        if self.away {
            p.show = Some(Show::Away);
        }
        if !self.status.is_empty() {
            p.set_status(Lang::default(), self.status.clone());
        }
        p
    }
}

pub struct Chat {
    commands: mpsc::UnboundedSender<Command>,
    history: History,
    task: JoinHandle<()>,
}

impl Chat {
    /// Startet den Chat im Hintergrund. `token` liefert bei jedem
    /// Verbindungsaufbau das aktuelle Access-Token.
    pub fn start(
        jid: &str,
        host: &str,
        token: impl Fn() -> String + Send + 'static,
        history_file: Option<PathBuf>,
        events: mpsc::UnboundedSender<ChatEvent>,
    ) -> Result<Self, Error> {
        let jid: BareJid = jid.parse().map_err(|_| Error::Jid(jid.to_owned()))?;
        // Mehrere rustls-Backends sind im Spiel; eines muss Standard sein.
        let _ = tokio_xmpp::rustls::crypto::aws_lc_rs::default_provider().install_default();
        let history = History::open(history_file);
        let (tx, rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(run(
            jid,
            host.to_owned(),
            Box::new(token),
            history.clone(),
            events,
            rx,
        ));
        Ok(Self {
            commands: tx,
            history,
            task,
        })
    }

    pub fn send(&self, to: &str, body: &str) {
        let _ = self.commands.send(Command::Send {
            to: to.to_owned(),
            body: body.to_owned(),
        });
    }

    /// Lokaler Verlauf eines Gesprächs; lädt zusätzlich das Serverarchiv nach.
    pub fn conversation(&self, peer: &str) -> Vec<ChatMessage> {
        let _ = self.commands.send(Command::LoadArchive {
            peer: peer.to_owned(),
        });
        self.history.get(peer)
    }

    /// Letzte Nachricht je Gespräch, neueste zuerst
    pub fn recent(&self) -> Vec<ChatMessage> {
        self.history.recent()
    }

    /// Setzt den eigenen Status (auch für spätere Neuverbindungen).
    pub fn set_presence(&self, away: bool, status: &str) {
        let _ = self.commands.send(Command::Presence(Own {
            away,
            status: status.to_owned(),
        }));
    }

    /// Meldet sich mit Statustext ab und wartet kurz, bis das gesendet ist.
    pub async fn shutdown(mut self, status: &str) {
        let _ = self.commands.send(Command::Offline {
            status: status.to_owned(),
        });
        let _ = tokio::time::timeout(Duration::from_secs(2), &mut self.task).await;
    }
}

impl Drop for Chat {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct Conn {
    own: BareJid,
    roster: BTreeMap<String, Contact>,
    /// Offene Archiv-Anfragen: IQ-ID → Gesprächspartner
    archive_requests: BTreeMap<String, String>,
    roster_request: String,
    history: History,
    events: mpsc::UnboundedSender<ChatEvent>,
}

async fn run(
    jid: BareJid,
    host: String,
    token: Box<dyn Fn() -> String + Send>,
    history: History,
    events: mpsc::UnboundedSender<ChatEvent>,
    mut commands: mpsc::UnboundedReceiver<Command>,
) {
    let mut backoff = Duration::from_secs(2);
    let mut own = Own::default();
    loop {
        let mut client = Client::new_starttls(
            jid.clone(),
            token(),
            DnsConfig::no_srv(&host, PORT),
            Timeouts::default(),
        );
        let mut conn = Conn {
            own: jid.clone(),
            roster: BTreeMap::new(),
            archive_requests: BTreeMap::new(),
            roster_request: String::new(),
            history: history.clone(),
            events: events.clone(),
        };
        let mut online = false;
        let reason = loop {
            tokio::select! {
                ev = client.next() => match ev {
                    None => break "Verbindung beendet".to_owned(),
                    Some(Event::Online { .. }) => {
                        online = true;
                        backoff = Duration::from_secs(2);
                        let _ = events.send(ChatEvent::State { online: true, detail: String::new() });
                        for stanza in conn.on_online(&own) {
                            if let Err(e) = client.send_stanza(stanza).await {
                                tracing::warn!(error = %e, "Chat: Senden fehlgeschlagen");
                            }
                        }
                    }
                    Some(Event::Disconnected(e)) => break e.to_string(),
                    Some(Event::Stanza(s)) => conn.on_stanza(s),
                },
                cmd = commands.recv() => match cmd {
                    None => return,
                    Some(Command::Presence(p)) => {
                        own = p;
                        if online && let Err(e) = client.send_stanza(own.presence().into()).await {
                            tracing::warn!(error = %e, "Chat: Status nicht gesendet");
                        }
                    }
                    Some(Command::Offline { status }) => {
                        if online {
                            let mut p = Presence::unavailable();
                            if !status.is_empty() {
                                p.set_status(Lang::default(), status);
                            }
                            let _ = client.send_stanza(p.into()).await;
                        }
                        let _ = client.send_end().await;
                        return;
                    }
                    Some(cmd) if online => {
                        if let Some(stanza) = conn.on_command(cmd)
                            && let Err(e) = client.send_stanza(stanza).await
                        {
                            tracing::warn!(error = %e, "Chat: Senden fehlgeschlagen");
                        }
                    }
                    Some(Command::Send { .. }) => {
                        let _ = events.send(ChatEvent::State {
                            online: false,
                            detail: "Nicht verbunden; Nachricht nicht gesendet".into(),
                        });
                    }
                    Some(Command::LoadArchive { .. }) => {}
                },
            }
        };
        tracing::warn!(%reason, "Chat getrennt");
        let _ = events.send(ChatEvent::State {
            online: false,
            detail: reason,
        });
        let _ = client.send_end().await;
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn new_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    format!("sf{}-{}", now_ms(), N.fetch_add(1, Ordering::Relaxed))
}

impl Conn {
    fn on_online(&mut self, own: &Own) -> Vec<Stanza> {
        self.roster_request = new_id();
        vec![
            xml::iq_get(&self.roster_request, xml::roster_query()),
            xml::iq_set(&new_id(), xml::carbons_enable()),
            own.presence().into(),
        ]
    }

    fn on_command(&mut self, cmd: Command) -> Option<Stanza> {
        match cmd {
            Command::Send { to, body } => {
                let to_jid: BareJid = to.parse().ok()?;
                let id = new_id();
                let mut msg = Message::new_with_type(MessageType::Chat, Some(Jid::from(to_jid)))
                    .with_body(Lang::default(), body.clone());
                msg.id = Some(tokio_xmpp::parsers::message::Id(id.clone()));
                let message = ChatMessage {
                    id,
                    peer: to,
                    outgoing: true,
                    body,
                    ts: now_ms(),
                };
                self.store(message, false);
                Some(msg.into())
            }
            Command::LoadArchive { peer } => {
                let id = new_id();
                self.archive_requests.insert(id.clone(), peer.clone());
                Some(xml::iq_get(&id, xml::archive_retrieve(&peer, 50)))
            }
            // werden in `run` behandelt
            Command::Presence(_) | Command::Offline { .. } => None,
        }
    }

    fn store(&mut self, message: ChatMessage, notify: bool) {
        if self.history.add(message.clone()) {
            let _ = self.events.send(ChatEvent::Message { message, notify });
        }
    }

    fn on_stanza(&mut self, stanza: Stanza) {
        match stanza {
            Stanza::Message(m) => self.on_message(m),
            Stanza::Presence(p) => self.on_presence(p),
            Stanza::Iq(iq) => {
                let id = iq.id().to_owned();
                let el: Option<Element> = match iq.into_payload() {
                    tokio_xmpp::parsers::iq::IqPayload::Result(el) => el,
                    _ => None,
                };
                if id == self.roster_request {
                    if let Some(el) = el {
                        self.on_roster(&el);
                    }
                } else if let Some(peer) = self.archive_requests.remove(&id)
                    && let Some(el) = el
                {
                    let messages = xml::parse_archive(&el, &peer);
                    if self.history.merge(&peer, messages) {
                        let _ = self.events.send(ChatEvent::History {
                            messages: self.history.get(&peer),
                            peer,
                        });
                    }
                }
            }
        }
    }

    fn on_roster(&mut self, query: &Element) {
        for item in xml::roster_items(query) {
            self.roster.entry(item.jid.clone()).or_insert(item);
        }
        self.publish_roster();
    }

    fn publish_roster(&self) {
        let _ = self.events.send(ChatEvent::Roster {
            contacts: self.roster.values().cloned().collect(),
        });
    }

    fn on_presence(&mut self, p: Presence) {
        let Some(from) = p.from.as_ref().map(|j| j.to_bare().to_string()) else {
            return;
        };
        if from == self.own.to_string() {
            return;
        }
        let show = match p.type_ {
            PresenceType::None => match p.show {
                Some(Show::Away) => "away",
                Some(Show::Xa) => "xa",
                Some(Show::Dnd) => "dnd",
                Some(Show::Chat) => "chat",
                None => "online",
            },
            PresenceType::Unavailable => "offline",
            _ => return,
        };
        let status = p.statuses.values().next().cloned().unwrap_or_default();
        let entry = self.roster.entry(from.clone()).or_insert_with(|| Contact {
            name: from.split('@').next().unwrap_or_default().to_owned(),
            jid: from,
            show: String::new(),
            status: String::new(),
        });
        entry.show = show.to_owned();
        entry.status = status;
        self.publish_roster();
    }

    fn on_message(&mut self, m: Message) {
        let own = self.own.to_string();
        // Carbons: Nachrichten, die über ein anderes Gerät liefen
        if m.from
            .as_ref()
            .is_some_and(|f| f.to_bare().to_string() == own)
            && let Some((fwd, sent)) = xml::carbon(&m)
        {
            if let Some(msg) = xml::chat_message(&fwd.message, fwd.delay.as_ref(), &own, sent) {
                self.store(msg, !sent);
            }
            return;
        }
        if !matches!(m.type_, MessageType::Chat | MessageType::Normal) {
            return;
        }
        let delay = xml::delay_of(&m);
        if let Some(msg) = xml::chat_message(&m, delay.as_ref(), &own, false) {
            self.store(msg, delay.is_none());
        }
    }
}
