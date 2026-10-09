//! Chat der Anlage über XMPP.
//!
//! Die Anlage betreibt Openfire auf Port 5222 mit STARTTLS. Benutzername ist
//! der lokale Teil der Jabber-ID aus `ChatService.GetChatId`, Passwort das
//! OAuth-Access-Token. Weil das Token nach wenigen Minuten abläuft, holt
//! [`Chat`] bei jedem Verbindungsaufbau, auch bei automatischen
//! Neuverbindungen, das aktuelle Token. (Der fertige `tokio_xmpp::Client`
//! verbindet sich mit dem Passwort vom Start neu und scheitert dann ewig.)
//!
//! Unterstützt: Kontaktliste mit Präsenz, Einzelchats, Gruppenchats (siehe
//! [`muc`]), Nachrichten anderer Geräte (Carbons), verzögerte Zustellung,
//! Verlauf aus dem Serverarchiv (XEP-0136) und ein lokaler Verlauf als
//! JSON-Datei.
//!
//! Nach der ersten Anmeldung fragt der Client per Service Discovery
//! (XEP-0030) ab, welche Dienste und Features die Anlage und die Clients der
//! Kollegen (auch eigene andere Geräte) anbieten, und schreibt das ins
//! Protokoll.
//!
//! Dateien gehen wie beim Windows-Client per Stream Initiation und In-Band
//! über den Chat-Server (siehe [`transfer`]).

mod files;
mod history;
pub mod muc;
mod tls;
pub mod transfer;
mod xml;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use transfer::{Incoming, Outgoing};

use futures::StreamExt;
use serde::Serialize;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_xmpp::connect::ServerConnector;
use tokio_xmpp::jid::{BareJid, Jid};
use tokio_xmpp::minidom::Element;
#[cfg(test)]
use tokio_xmpp::parsers::iq::IqHeader;
use tokio_xmpp::parsers::iq::{Iq, IqPayload};
use tokio_xmpp::parsers::message::{Lang, Message, MessageType};
use tokio_xmpp::parsers::ns;
use tokio_xmpp::parsers::presence::{Presence, Show, Type as PresenceType};
use tokio_xmpp::parsers::stanza_error::StanzaError;
use tokio_xmpp::stanzastream::{Connection, Event, StanzaStream, StreamEvent};
use tokio_xmpp::xmlstream::{StreamHeader, Timeouts};
use tokio_xmpp::{Stanza, client_login};

use sf_backoff::Backoff;

pub use history::History;
pub use muc::{Member, Room};
pub use transfer::{Transfer, TransferState};

const PORT: u16 = 5222;
/// Höchstens so viele Clients je Programmstart per Discovery abfragen
const MAX_CLIENT_DISCO: usize = 200;
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// In diesem Takt werden abgewiesene Gruppenräume erneut betreten
const ROOM_RETRY: Duration = Duration::from_secs(20);
/// Nach so langer Stille fragt der Client per Ping nach, ob die Verbindung
/// noch steht; ohne Antwort in derselben Zeit gilt sie als tot. Der Standard
/// von tokio-xmpp (300 s) ist zu lang: Bei Cloud-Anlagen kappte ein Proxy
/// die stille Verbindung vorher, der Chat trennte sich alle fünf Minuten.
const TIMEOUTS: Timeouts = Timeouts {
    read_timeout: Duration::from_secs(60),
    response_timeout: Duration::from_secs(60),
};

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
    /// Angemeldete Clients (Geräte) des Kontakts
    pub clients: Vec<Client>,
}

/// Ein angemeldeter Client eines Kontakts
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Client {
    pub resource: String,
    /// Name und Version, wie der Client sie meldet; leer, wenn er nicht antwortet
    pub name: String,
    /// Kann Dateien empfangen; `None`, solange (oder weil) er nicht antwortet
    pub files: Option<bool>,
}

/// Was ein Client über sich meldet (je Client-Art)
#[derive(Debug, Default)]
struct ClientInfo {
    name: Option<String>,
    files: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub id: String,
    /// Gesprächspartner (bare JID), bei Gruppenchats der Raum
    pub peer: String,
    pub outgoing: bool,
    pub body: String,
    /// Unix-Zeit in Millisekunden
    pub ts: i64,
    /// Gruppenchat: Absender (bare JID, leer, wenn unbekannt)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sender: String,
    /// Gruppenchat: Name des Absenders, wie er ihn im Raum angibt
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sender_name: String,
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
    /// Neue oder geänderte Dateiübertragung
    Transfer {
        transfer: Transfer,
    },
    /// Gruppenchats und wer darin anwesend ist
    Rooms {
        rooms: Vec<Room>,
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
    SendFile {
        to: String,
        path: PathBuf,
    },
    AcceptFile {
        id: String,
        dir: PathBuf,
    },
    DeclineFile {
        id: String,
    },
    CancelFile {
        id: String,
    },
    /// Eigener Name und Gruppen der Anlage (für die Gruppenchats)
    Groups {
        display_name: String,
        groups: Vec<String>,
    },
    CreateRoom {
        room: String,
        subject: String,
        members: Vec<String>,
    },
    LeaveRoom {
        room: String,
    },
    /// Abmelden mit Statustext, danach endet die Verbindung
    Offline {
        status: String,
    },
}

/// Verfügbarkeit im Chat, wie in der STARFACE-App
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Availability {
    #[default]
    Available,
    Away,
    DoNotDisturb,
}

/// Eigener Status, wie ihn die anderen sehen
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Own {
    pub availability: Availability,
    pub status: String,
}

impl Own {
    fn presence(&self) -> Presence {
        let mut p = Presence::available();
        p.show = match self.availability {
            Availability::Available => None,
            Availability::Away => Some(Show::Away),
            Availability::DoNotDisturb => Some(Show::Dnd),
        };
        if !self.status.is_empty() {
            p.set_status(Lang::default(), self.status.clone());
        }
        p
    }
}

pub struct Chat {
    commands: mpsc::UnboundedSender<Command>,
    own: BareJid,
    history: History,
    task: JoinHandle<()>,
}

impl Chat {
    /// Startet den Chat im Hintergrund. `token` liefert bei jedem
    /// Verbindungsaufbau das aktuelle Access-Token.
    pub fn start(
        jid: &str,
        host: &str,
        token: impl Fn() -> String + Send + Sync + 'static,
        history_file: Option<PathBuf>,
        events: mpsc::UnboundedSender<ChatEvent>,
    ) -> Result<Self, Error> {
        let jid: BareJid = jid.parse().map_err(|_| Error::Jid(jid.to_owned()))?;
        // Mehrere rustls-Backends sind im Spiel; eines muss Standard sein.
        let _ = tokio_xmpp::rustls::crypto::aws_lc_rs::default_provider().install_default();
        let rooms_file = history_file
            .as_ref()
            .map(|p| p.with_extension("rooms.json"));
        let history = History::open(history_file);
        let (tx, rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(run(
            jid.clone(),
            host.to_owned(),
            Arc::new(token),
            history.clone(),
            rooms_file,
            events,
            rx,
        ));
        Ok(Self {
            commands: tx,
            own: jid,
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

    /// Bietet `to` (bare JID) eine Datei an.
    pub fn send_file(&self, to: &str, path: PathBuf) {
        let _ = self.commands.send(Command::SendFile {
            to: to.to_owned(),
            path,
        });
    }

    /// Nimmt eine angebotene Datei an und speichert sie in `dir`.
    pub fn accept_file(&self, id: &str, dir: PathBuf) {
        let _ = self.commands.send(Command::AcceptFile {
            id: id.to_owned(),
            dir,
        });
    }

    pub fn decline_file(&self, id: &str) {
        let _ = self
            .commands
            .send(Command::DeclineFile { id: id.to_owned() });
    }

    /// Bricht eine Übertragung ab (in beide Richtungen).
    pub fn cancel_file(&self, id: &str) {
        let _ = self
            .commands
            .send(Command::CancelFile { id: id.to_owned() });
    }

    /// Eigener Name und die Gruppen der Anlage, in denen man Mitglied ist.
    /// Danach betritt der Chat die Gruppenräume (siehe [`muc`]).
    pub fn set_groups(&self, display_name: &str, groups: Vec<String>) {
        let _ = self.commands.send(Command::Groups {
            display_name: display_name.to_owned(),
            groups,
        });
    }

    /// Startet einen spontanen Gruppenchat und lädt `members` (bare JIDs)
    /// ein. Ohne Thema heisst er nach den Eingeladenen. Liefert den Raum.
    pub fn create_room(&self, subject: &str, members: Vec<String>) -> String {
        let room = muc::new_room(&self.own);
        let _ = self.commands.send(Command::CreateRoom {
            room: room.clone(),
            subject: subject.to_owned(),
            members,
        });
        room
    }

    /// Verlässt einen spontanen Gruppenchat.
    pub fn leave_room(&self, room: &str) {
        let _ = self.commands.send(Command::LeaveRoom {
            room: room.to_owned(),
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
    pub fn set_presence(&self, availability: Availability, status: &str) {
        let _ = self.commands.send(Command::Presence(Own {
            availability,
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
    /// Offene eigene IQ-Anfragen: IQ-ID → wozu
    pending: BTreeMap<String, Pending>,
    /// Discovery lief schon (einmal je Programmstart reicht)
    discovered: bool,
    /// Schon abgefragte Client-Arten (Caps-Kennung oder volle JID)
    clients_seen: BTreeSet<String>,
    /// Clients der Kollegen, die online sind: bare JID → volle JID →
    /// (Kennung, letzte Präsenz)
    resources: BTreeMap<String, BTreeMap<String, (String, Instant)>>,
    /// Client-Art (Kennung wie in `clients_seen`) → was sie über sich meldet
    client_info: BTreeMap<String, ClientInfo>,
    outgoing: BTreeMap<String, Outgoing>,
    incoming: BTreeMap<String, Incoming>,
    /// Gruppenchats: Raum (bare JID) → Stand
    rooms: BTreeMap<String, muc::RoomState>,
    /// Gruppen der Anlage, in denen man Mitglied ist
    groups: Vec<String>,
    /// Gruppen, die in der Kontaktliste vorkommen
    roster_groups: BTreeSet<String>,
    /// Eigener Spitzname in Räumen; leer, bis die Gruppen bekannt sind
    nick: String,
    /// Spontane Gruppenchats, damit sie einen Neustart überdauern
    rooms_file: Option<PathBuf>,
    history: History,
    events: mpsc::UnboundedSender<ChatEvent>,
}

/// Token, das bei jedem Verbindungsaufbau neu abgefragt wird
type TokenFn = Arc<dyn Fn() -> String + Send + Sync>;

/// Meldet sich an der Anlage an. Liefert einen authentifizierten, noch nicht
/// gebundenen Stream; die Bindung übernimmt der `StanzaStream`.
async fn login(
    jid: &BareJid,
    host: &str,
    password: String,
) -> Result<Connection, tokio_xmpp::Error> {
    let jid = Jid::from(jid.clone());
    let server = tls::StartTls {
        host: host.to_owned(),
        port: PORT,
    };
    let (stream, binding) = server.connect(&jid, ns::JABBER_CLIENT, TIMEOUTS).await?;
    let (features, stream) = stream.recv_features().await?;
    let creds = sasl::common::Credentials::default()
        .with_username(jid.node().map(|n| n.as_str()).unwrap_or_default())
        .with_password(password)
        .with_channel_binding(binding);
    let stream = client_login(stream, features.sasl_mechanisms, creds).await?;
    let stream = stream
        .send_header(StreamHeader {
            to: Some(jid.domain().as_str().to_owned().into()),
            from: None,
            id: None,
        })
        .await?;
    let (features, stream) = stream.recv_features().await?;
    Ok(Connection {
        stream: stream.box_stream(),
        features,
        identity: jid,
    })
}

/// Baut für den `StanzaStream` Verbindungen auf, jedes Mal mit dem aktuellen
/// Token. Fehlschläge werden gemeldet und mit wachsendem Abstand wiederholt.
/// Der Abstand gilt über Neuverbindungen hinweg: Bricht eine eben aufgebaute
/// Verbindung gleich wieder ab, wartet der nächste Versuch länger.
fn connector(
    jid: BareJid,
    host: String,
    token: TokenFn,
    events: mpsc::UnboundedSender<ChatEvent>,
) -> Box<dyn FnMut(Option<String>, oneshot::Sender<Connection>) + Send + 'static> {
    let pacing = Arc::new(std::sync::Mutex::new(Pacing {
        backoff: Backoff::new(Duration::from_secs(2), MAX_BACKOFF),
        connected_at: None,
    }));
    Box::new(move |_, slot| {
        let (jid, host, token, events) = (jid.clone(), host.clone(), token.clone(), events.clone());
        let pacing = pacing.clone();
        tokio::spawn(async move {
            let ran = pacing
                .lock()
                .unwrap()
                .connected_at
                .take()
                .map(|t| t.elapsed());
            if let Some(ran) = ran {
                let wait = pacing.lock().unwrap().backoff.next(ran, None);
                tokio::time::sleep(wait).await;
            }
            loop {
                match login(&jid, &host, token()).await {
                    Ok(conn) => {
                        pacing.lock().unwrap().connected_at = Some(Instant::now());
                        let _ = slot.send(conn);
                        return;
                    }
                    Err(e) => {
                        let wait = pacing.lock().unwrap().backoff.next(Duration::ZERO, None);
                        tracing::warn!(error = %e, ?wait, "Chat-Anmeldung fehlgeschlagen");
                        let _ = events.send(ChatEvent::State {
                            online: false,
                            detail: format!("Anmeldung fehlgeschlagen: {e}"),
                        });
                        if slot.is_closed() {
                            return;
                        }
                        tokio::time::sleep(wait).await;
                    }
                }
            }
        });
    })
}

enum Disco {
    /// `disco#items` an die Domain der Anlage
    Items,
    /// `disco#info` an diese Adresse
    Info(String),
    /// `disco#info` an einen Client; Ergebnis gilt für die Client-Art `key`
    Client { jid: String, key: String },
}

/// Wozu eine eigene IQ-Anfrage gehört
enum Pending {
    Disco(Disco),
    /// Dateiangebot (Übertragungs-ID)
    Offer(String),
    /// IBB öffnen
    Open(String),
    /// Datenblock mit Länge
    Data(String, usize),
    Close(String),
    /// Name und Version eines Clients (XEP-0092) für die Client-Art
    Version(String),
    /// Einstellungen eines neu angelegten Raums
    RoomConfig(String),
}

struct Pacing {
    backoff: Backoff,
    /// Zeitpunkt der letzten gelungenen Anmeldung
    connected_at: Option<Instant>,
}

async fn run(
    jid: BareJid,
    host: String,
    token: TokenFn,
    history: History,
    rooms_file: Option<PathBuf>,
    events: mpsc::UnboundedSender<ChatEvent>,
    mut commands: mpsc::UnboundedReceiver<Command>,
) {
    let mut own = Own::default();
    let mut conn = Conn {
        own: jid.clone(),
        roster: BTreeMap::new(),
        archive_requests: BTreeMap::new(),
        roster_request: String::new(),
        pending: BTreeMap::new(),
        discovered: false,
        clients_seen: BTreeSet::new(),
        resources: BTreeMap::new(),
        client_info: BTreeMap::new(),
        outgoing: BTreeMap::new(),
        incoming: BTreeMap::new(),
        rooms: muc::load_rooms(rooms_file.as_deref()),
        groups: Vec::new(),
        roster_groups: BTreeSet::new(),
        nick: String::new(),
        rooms_file,
        history,
        events: events.clone(),
    };
    loop {
        let mut stream = StanzaStream::new(
            connector(jid.clone(), host.clone(), token.clone(), events.clone()),
            16,
        );
        let mut online = false;
        let mut retry = tokio::time::interval(ROOM_RETRY);
        loop {
            tokio::select! {
                _ = retry.tick(), if online => {
                    for stanza in conn.join_rooms() {
                        stream.send(Box::new(stanza)).await;
                    }
                }
                ev = stream.next() => match ev {
                    None => {
                        // Der Stream gibt nur bei schweren Fehlern auf; neu beginnen.
                        tracing::warn!("Chat-Stream beendet, neuer Versuch");
                        let _ = events.send(ChatEvent::State { online: false, detail: "Verbindung beendet".into() });
                        break;
                    }
                    Some(Event::Stream(StreamEvent::Reset { .. })) => {
                        online = true;
                        let _ = events.send(ChatEvent::State { online: true, detail: String::new() });
                        for stanza in conn.on_online(&own) {
                            stream.send(Box::new(stanza)).await;
                        }
                    }
                    Some(Event::Stream(StreamEvent::Resumed)) => {
                        online = true;
                        let _ = events.send(ChatEvent::State { online: true, detail: String::new() });
                    }
                    Some(Event::Stream(StreamEvent::Suspended)) => {
                        online = false;
                        tracing::warn!("Chat getrennt, verbinde neu");
                        let _ = events.send(ChatEvent::State { online: false, detail: "Verbindung unterbrochen, verbinde neu …".into() });
                    }
                    Some(Event::Stanza(s)) => {
                        for stanza in conn.on_stanza(s) {
                            stream.send(Box::new(stanza)).await;
                        }
                    }
                },
                cmd = commands.recv() => match cmd {
                    None => return stream.close().await,
                    Some(Command::Presence(p)) => {
                        own = p;
                        if online {
                            stream.send(Box::new(own.presence().into())).await;
                        }
                    }
                    Some(Command::Offline { status }) => {
                        if online {
                            let mut p = Presence::unavailable();
                            if !status.is_empty() {
                                p.set_status(Lang::default(), status);
                            }
                            let mut sent = stream.send(Box::new(p.into())).await;
                            let _ = tokio::time::timeout(Duration::from_secs(1), sent.wait_for(tokio_xmpp::stanzastream::StanzaStage::Sent)).await;
                        }
                        return stream.close().await;
                    }
                    Some(Command::Groups { display_name, groups }) => {
                        let out = conn.set_groups(&display_name, groups);
                        if online {
                            for stanza in out {
                                stream.send(Box::new(stanza)).await;
                            }
                        }
                    }
                    Some(cmd) if online => {
                        for stanza in conn.on_command(cmd) {
                            stream.send(Box::new(stanza)).await;
                        }
                    }
                    Some(Command::SendFile { to, path }) => conn.offline_file(&to, &path),
                    Some(Command::Send { .. }) => {
                        let _ = events.send(ChatEvent::State {
                            online: false,
                            detail: "Nicht verbunden; Nachricht nicht gesendet".into(),
                        });
                    }
                    Some(Command::LeaveRoom { room }) => {
                        conn.leave_room(&room);
                    }
                    Some(Command::CreateRoom { .. }) => {
                        let _ = events.send(ChatEvent::State {
                            online: false,
                            detail: "Nicht verbunden; Gruppenchat nicht gestartet".into(),
                        });
                    }
                    Some(Command::LoadArchive { .. } | Command::AcceptFile { .. } | Command::DeclineFile { .. } | Command::CancelFile { .. }) => {}
                },
            }
        }
        // Zufallsanteil, damit nicht alle Clients gleichzeitig neu beginnen
        let wait = Backoff::new(MAX_BACKOFF, MAX_BACKOFF).next(Duration::ZERO, None);
        tokio::time::sleep(wait).await;
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
        // Laufende Übertragungen überleben keine neue Verbindung
        self.drop_transfers();
        self.resources.clear();
        self.rooms_offline();
        let mut out = vec![
            xml::iq_get(&self.roster_request, xml::roster_query()),
            xml::iq_set(&new_id(), xml::carbons_enable()),
            own.presence().into(),
        ];
        // Gruppenräume kommen nach der Kontaktliste dazu
        out.extend(self.join_rooms());
        if !self.discovered {
            self.discovered = true;
            let domain = self.own.domain().to_string();
            out.extend(self.disco(Disco::Items, &domain));
            out.extend(self.disco(Disco::Info(domain.clone()), &domain));
        }
        out
    }

    fn disco(&mut self, request: Disco, to: &str) -> Option<Stanza> {
        let id = new_id();
        let payload = match request {
            Disco::Items => xml::disco_items(),
            Disco::Info(_) | Disco::Client { .. } => xml::disco_info(),
        };
        let stanza = xml::iq_get_to(&id, to, payload)?;
        self.pending.insert(id, Pending::Disco(request));
        Some(stanza)
    }

    /// Antwort auf eine Discovery-Anfrage; liefert Folgeanfragen an die
    /// gefundenen Dienste.
    fn on_disco(
        &mut self,
        request: Disco,
        result: Result<Option<Element>, StanzaError>,
    ) -> Vec<Stanza> {
        let el = match result {
            Ok(Some(el)) => el,
            Ok(None) => return Vec::new(),
            Err(error) => {
                let what = match &request {
                    Disco::Items => self.own.domain().to_string(),
                    Disco::Info(jid) | Disco::Client { jid, .. } => jid.clone(),
                };
                tracing::info!(%what, error = ?error.defined_condition, "Chat-Discovery abgelehnt");
                if let Disco::Client { key, .. } = request {
                    self.client_info.entry(key).or_default().files = Some(false);
                    self.publish_roster();
                }
                return Vec::new();
            }
        };
        match request {
            Disco::Items => {
                let jids = xml::disco_item_jids(&el);
                tracing::info!(services = ?jids, "Chat-Discovery: Dienste der Anlage");
                jids.iter()
                    .filter_map(|jid| self.disco(Disco::Info(jid.clone()), jid))
                    .collect()
            }
            Disco::Info(jid) => {
                let (identities, features) = xml::disco_info_summary(&el);
                tracing::info!(%jid, ?identities, ?features, "Chat-Discovery: Features");
                Vec::new()
            }
            Disco::Client { jid, key } => {
                let (identities, features) = xml::disco_info_summary(&el);
                tracing::info!(%jid, ?identities, ?features, "Chat-Discovery: Features");
                let info = self.client_info.entry(key).or_default();
                info.files = Some(features.iter().any(|f| f == transfer::NS_FT));
                if info.name.is_none() {
                    info.name = xml::identity_name(&el);
                }
                self.publish_roster();
                Vec::new()
            }
        }
    }

    fn on_command(&mut self, cmd: Command) -> Vec<Stanza> {
        match cmd {
            Command::Send { to, body } if self.is_room(&to) => self.send_to_room(&to, body),
            Command::Send { to, body } => {
                let Ok(to_jid) = to.parse::<BareJid>() else {
                    return Vec::new();
                };
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
                    ..Default::default()
                };
                self.store(message, false);
                vec![msg.into()]
            }
            // Gruppenchats führt das Archiv nicht; ihren Verlauf liefert der Raum
            Command::LoadArchive { peer } if self.is_room(&peer) => Vec::new(),
            Command::LoadArchive { peer } => {
                let id = new_id();
                self.archive_requests.insert(id.clone(), peer.clone());
                vec![xml::iq_get(&id, xml::archive_retrieve(&peer, 50))]
            }
            Command::SendFile { to, path } => self.send_file(&to, &path).into_iter().collect(),
            Command::AcceptFile { id, dir } => self.accept_file(&id, &dir),
            Command::DeclineFile { id } => self.decline_file(&id).into_iter().collect(),
            Command::CancelFile { id } => self.cancel_file(&id).into_iter().collect(),
            Command::CreateRoom {
                room,
                subject,
                members,
            } => self.create_room(room, &subject, members),
            Command::LeaveRoom { room } => self.leave_room(&room),
            Command::Groups {
                display_name,
                groups,
            } => self.set_groups(&display_name, groups),
            // werden in `run` behandelt
            Command::Presence(_) | Command::Offline { .. } => Vec::new(),
        }
    }

    fn store(&mut self, message: ChatMessage, notify: bool) {
        if self.history.add(message.clone()) {
            let _ = self.events.send(ChatEvent::Message { message, notify });
        }
    }

    fn on_stanza(&mut self, stanza: Stanza) -> Vec<Stanza> {
        match stanza {
            Stanza::Message(m) => self.on_message(m),
            Stanza::Presence(p) => self.on_presence(p),
            Stanza::Iq(iq) => self.on_iq(iq),
        }
    }

    fn on_iq(&mut self, iq: Iq) -> Vec<Stanza> {
        let (header, payload) = iq.split();
        let id = header.id;
        let payload = match payload {
            IqPayload::Get(el) => return self.on_iq_get(header.from, id, el),
            IqPayload::Set(el) => return self.on_iq_set(header.from, id, el),
            other => other,
        };
        if let Some(request) = self.pending.remove(&id) {
            let result = match payload {
                IqPayload::Result(el) => Ok(el),
                IqPayload::Error(e) => Err(e),
                _ => return Vec::new(),
            };
            return match request {
                Pending::Disco(request) => self.on_disco(request, result),
                Pending::Version(key) => {
                    if let Ok(Some(el)) = result
                        && let Some(version) = xml::version_of(&el)
                    {
                        tracing::info!(client = %key, %version, "Chat-Discovery: Version");
                        self.client_info.entry(key).or_default().name = Some(version);
                        self.publish_roster();
                    }
                    Vec::new()
                }
                Pending::RoomConfig(room) => {
                    if let Err(e) = result {
                        tracing::warn!(%room, error = ?e.defined_condition, "Gruppenchat: Einstellungen abgelehnt");
                    }
                    self.room_configured(&room)
                }
                other => self.on_transfer_result(other, result),
            };
        }
        let el: Option<Element> = match payload {
            IqPayload::Result(el) => el,
            _ => None,
        };
        if id == self.roster_request {
            if let Some(el) = el {
                return self.on_roster(&el);
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
        Vec::new()
    }

    /// Kontaktliste; liefert die Anfragen zum Betreten der Gruppenräume.
    fn on_roster(&mut self, query: &Element) -> Vec<Stanza> {
        for item in xml::roster_items(query) {
            self.roster.entry(item.jid.clone()).or_insert(item);
        }
        self.publish_roster();
        self.roster_groups = xml::roster_groups(query);
        self.want_group_rooms();
        self.join_rooms()
    }

    fn publish_roster(&self) {
        let contacts = self
            .roster
            .values()
            .map(|c| Contact {
                clients: self.clients_of(&c.jid),
                ..c.clone()
            })
            .collect();
        let _ = self.events.send(ChatEvent::Roster { contacts });
    }

    fn clients_of(&self, bare: &str) -> Vec<Client> {
        let Some(resources) = self.resources.get(bare) else {
            return Vec::new();
        };
        resources
            .iter()
            .map(|(full, (key, _))| {
                let info = self.client_info.get(key);
                let resource = full.split_once('/').map_or("", |(_, r)| r);
                Client {
                    resource: resource.to_owned(),
                    name: xml::starface_client(resource)
                        .or_else(|| info.and_then(|i| i.name.clone()))
                        .unwrap_or_default(),
                    files: info.and_then(|i| i.files),
                }
            })
            .collect()
    }

    /// Liefert ggf. eine Discovery-Anfrage an den Client des Absenders.
    fn on_presence(&mut self, p: Presence) -> Vec<Stanza> {
        if let Some(room) = p.from.as_ref().map(Jid::to_bare)
            && self.is_room(room.as_str())
        {
            return self.on_room_presence(room, &p);
        }
        self.track_resource(&p);
        let disco = self.disco_client(&p);
        let Some(from) = p.from.as_ref().map(|j| j.to_bare().to_string()) else {
            return disco;
        };
        if from == self.own.to_string() {
            return disco;
        }
        // Raum, den man nicht (mehr) kennt: kein Kontakt
        if from.ends_with(&format!("@{}", self.muc_service())) {
            return Vec::new();
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
            _ => return disco,
        };
        // Meldet sich nur eines von mehreren Geräten ab, bleibt der Kontakt online.
        if show == "offline" && self.resources.get(&from).is_some_and(|r| !r.is_empty()) {
            self.publish_roster();
            return disco;
        }
        let status = p.statuses.values().next().cloned().unwrap_or_default();
        let entry = self.roster.entry(from.clone()).or_insert_with(|| Contact {
            name: from.split('@').next().unwrap_or_default().to_owned(),
            jid: from,
            show: String::new(),
            status: String::new(),
            clients: Vec::new(),
        });
        entry.show = show.to_owned();
        entry.status = status;
        self.publish_roster();
        disco
    }

    /// Fragt einen Client einmal je Client-Art (XEP-0115-Kennung) ab, was er
    /// kann, z. B. welche Verfahren für Dateiübertragung.
    fn disco_client(&mut self, p: &Presence) -> Vec<Stanza> {
        if p.type_ != PresenceType::None || self.clients_seen.len() >= MAX_CLIENT_DISCO {
            return Vec::new();
        }
        let Some(from) = p.from.as_ref().filter(|j| j.resource().is_some()) else {
            return Vec::new();
        };
        let from = from.to_string();
        let caps = xml::caps_of(p);
        // Kennung, unter der das Ergebnis gemerkt wird
        let key = caps.clone().unwrap_or_else(|| from.clone());
        if !self.clients_seen.insert(key.clone()) {
            return Vec::new();
        }
        tracing::info!(jid = %from, caps = caps.as_deref().unwrap_or("-"), "Chat-Discovery: frage Client");
        let version = new_id();
        let mut out: Vec<Stanza> = xml::iq_get_to(&version, &from, xml::version_query())
            .into_iter()
            .collect();
        self.pending.insert(version, Pending::Version(key.clone()));
        out.extend(self.disco(
            Disco::Client {
                jid: from.clone(),
                key,
            },
            &from,
        ));
        out
    }

    fn on_message(&mut self, m: Message) -> Vec<Stanza> {
        if let Some(out) = self.on_room_message(&m) {
            return out;
        }
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
            return Vec::new();
        }
        if !matches!(m.type_, MessageType::Chat | MessageType::Normal) {
            return Vec::new();
        }
        let delay = xml::delay_of(&m);
        if let Some(msg) = xml::chat_message(&m, delay.as_ref(), &own, false) {
            self.store(msg, delay.is_none());
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: &str = "alice@pbx.test/linux";
    const BOB: &str = "bob@pbx.test/StarfaceWindows";

    fn conn(jid: &str) -> (Conn, mpsc::UnboundedReceiver<ChatEvent>) {
        let (events, rx) = mpsc::unbounded_channel();
        let own: Jid = jid.parse().unwrap();
        let conn = Conn {
            own: own.to_bare(),
            roster: BTreeMap::new(),
            archive_requests: BTreeMap::new(),
            roster_request: String::new(),
            pending: BTreeMap::new(),
            discovered: true,
            clients_seen: BTreeSet::new(),
            resources: BTreeMap::new(),
            client_info: BTreeMap::new(),
            outgoing: BTreeMap::new(),
            incoming: BTreeMap::new(),
            rooms: BTreeMap::new(),
            groups: Vec::new(),
            roster_groups: BTreeSet::new(),
            nick: String::new(),
            rooms_file: None,
            history: History::open(None),
            events,
        };
        (conn, rx)
    }

    fn online(from: &str) -> Stanza {
        let mut p = Presence::available();
        p.from = Some(from.parse().unwrap());
        p.into()
    }

    /// Stellt Stanzas zu wie der Server (mit Absender), bis nichts mehr
    /// unterwegs ist. Liefert die Zahl der zugestellten Stanzas.
    fn deliver(a: &mut Conn, b: &mut Conn, mut queue: Vec<(bool, Stanza)>) -> usize {
        let mut n = 0;
        while !queue.is_empty() {
            let mut next = Vec::new();
            for (from_a, stanza) in queue {
                n += 1;
                let (from, to) = if from_a {
                    (ALICE, &mut *b)
                } else {
                    (BOB, &mut *a)
                };
                let Stanza::Iq(mut iq) = stanza else {
                    panic!("nur IQs erwartet")
                };
                *iq.from_mut() = Some(from.parse().unwrap());
                next.extend(to.on_stanza(iq.into()).into_iter().map(|s| (!from_a, s)));
            }
            queue = next;
        }
        n
    }

    fn transfers(rx: &mut mpsc::UnboundedReceiver<ChatEvent>) -> Vec<Transfer> {
        std::iter::from_fn(|| rx.try_recv().ok())
            .filter_map(|e| match e {
                ChatEvent::Transfer { transfer } => Some(transfer),
                _ => None,
            })
            .collect()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sf-chat-conn-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn file_is_offered_accepted_and_transferred() {
        let (mut alice, mut arx) = conn(ALICE);
        let (mut bob, mut brx) = conn(BOB);
        let hello = bob.on_stanza(online(ALICE));
        deliver(
            &mut alice,
            &mut bob,
            hello.into_iter().map(|s| (false, s)).collect(),
        );
        let alice_client = &bob.clients_of("alice@pbx.test")[0];
        assert_eq!(alice_client.files, Some(true));
        assert!(
            alice_client.name.starts_with("StarCLX "),
            "{alice_client:?}"
        );
        let dir = temp_dir("ok");
        let src = dir.join("Bericht.pdf");
        let data: Vec<u8> = (0..200_000u32).map(|i| (i * 7) as u8).collect();
        std::fs::write(&src, &data).unwrap();

        // Bob bietet an, Alice sieht das Angebot
        let offer = bob.on_command(Command::SendFile {
            to: "alice@pbx.test".into(),
            path: src,
        });
        deliver(
            &mut alice,
            &mut bob,
            offer.into_iter().map(|s| (false, s)).collect(),
        );
        let offered = transfers(&mut arx).pop().unwrap();
        assert_eq!(offered.state, TransferState::Offered);
        assert_eq!(
            (offered.name.as_str(), offered.size, offered.peer.as_str()),
            ("Bericht.pdf", 200_000, "bob@pbx.test")
        );
        assert_eq!(
            transfers(&mut brx).pop().unwrap().state,
            TransferState::Waiting
        );

        // Alice nimmt an, der Rest läuft von selbst
        let inbox = dir.join("in");
        let accept = alice.on_command(Command::AcceptFile {
            id: offered.id.clone(),
            dir: inbox.clone(),
        });
        let n = deliver(
            &mut alice,
            &mut bob,
            accept.into_iter().map(|s| (true, s)).collect(),
        );
        // Annahme, dann Öffnen, 7 Blöcke à 32 KiB und Schließen mit je einer Antwort
        assert_eq!(n, 1 + 2 * (1 + 7 + 1));

        let got = transfers(&mut arx).pop().unwrap();
        assert_eq!(got.state, TransferState::Done);
        assert_eq!(std::fs::read(&got.path).unwrap(), data);
        assert_eq!(got.path, inbox.join("Bericht.pdf").to_string_lossy());
        let sent = transfers(&mut brx).pop().unwrap();
        assert_eq!((sent.state, sent.done), (TransferState::Done, 200_000));
        assert!(alice.incoming.is_empty(), "alice");
        assert!(bob.outgoing.is_empty(), "bob out");
        assert!(bob.pending.values().all(|p| matches!(p, Pending::Disco(_))));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn declined_offer_reaches_sender() {
        let (mut alice, mut arx) = conn(ALICE);
        let (mut bob, mut brx) = conn(BOB);
        let hello = bob.on_stanza(online(ALICE));
        deliver(
            &mut alice,
            &mut bob,
            hello.into_iter().map(|s| (false, s)).collect(),
        );
        let dir = temp_dir("decline");
        let src = dir.join("a.txt");
        std::fs::write(&src, "hallo").unwrap();
        let offer = bob.on_command(Command::SendFile {
            to: "alice@pbx.test".into(),
            path: src,
        });
        deliver(
            &mut alice,
            &mut bob,
            offer.into_iter().map(|s| (false, s)).collect(),
        );
        let id = transfers(&mut arx).pop().unwrap().id;
        let decline = alice.on_command(Command::DeclineFile { id });
        deliver(
            &mut alice,
            &mut bob,
            decline.into_iter().map(|s| (true, s)).collect(),
        );
        assert_eq!(
            transfers(&mut arx).pop().unwrap().state,
            TransferState::Declined
        );
        assert_eq!(
            transfers(&mut brx).pop().unwrap().state,
            TransferState::Declined
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn client_must_announce_file_transfer() {
        let (mut bob, mut brx) = conn(BOB);
        // Präsenz ohne Antwort auf Discovery, wie die Android-App
        bob.on_stanza(online(ALICE));
        let clients = bob.clients_of("alice@pbx.test");
        assert_eq!(clients.len(), 1);
        assert_eq!(
            (clients[0].resource.as_str(), clients[0].files),
            ("linux", None)
        );
        let dir = temp_dir("silent");
        let src = dir.join("a.txt");
        std::fs::write(&src, "hallo").unwrap();
        let out = bob.on_command(Command::SendFile {
            to: "alice@pbx.test".into(),
            path: src,
        });
        assert!(out.is_empty());
        let t = transfers(&mut brx).pop().unwrap();
        assert_eq!(t.state, TransferState::Failed);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn contact_stays_online_while_another_device_is() {
        let (mut bob, _brx) = conn(BOB);
        bob.on_stanza(online("alice@pbx.test/StarfaceWindows-v9.0.2.7-alice-PC"));
        bob.on_stanza(online("alice@pbx.test/StarfaceAndroidClient-0c1a"));
        let mut gone = Presence::unavailable();
        gone.from = Some(
            "alice@pbx.test/StarfaceWindows-v9.0.2.7-alice-PC"
                .parse()
                .unwrap(),
        );
        bob.on_stanza(gone.into());
        assert_eq!(bob.roster["alice@pbx.test"].show, "online");
        let names: Vec<_> = bob
            .clients_of("alice@pbx.test")
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, ["STARFACE Android"]);
    }

    #[test]
    fn offline_contact_cannot_get_files() {
        let (mut bob, mut brx) = conn(BOB);
        let dir = temp_dir("offline");
        let src = dir.join("a.txt");
        std::fs::write(&src, "hallo").unwrap();
        let out = bob.on_command(Command::SendFile {
            to: "alice@pbx.test".into(),
            path: src,
        });
        assert!(out.is_empty());
        let t = transfers(&mut brx).pop().unwrap();
        assert_eq!(
            (t.state, t.error.as_str()),
            (TransferState::Failed, "Kontakt ist nicht online")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn stanza(xml: &str) -> Stanza {
        let el: Element = xml.parse().unwrap();
        match el.name() {
            "presence" => Presence::try_from(el).unwrap().into(),
            "message" => Message::try_from(el).unwrap().into(),
            _ => Iq::try_from(el).unwrap().into(),
        }
    }

    fn xml_of(s: &Stanza) -> String {
        String::from(&Element::from(s))
    }

    fn events(rx: &mut mpsc::UnboundedReceiver<ChatEvent>) -> Vec<ChatEvent> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    const ROOM: &str = "vertrieb@chatrooms.pbx.test";

    /// Der Raum bestätigt die Einstellungen aus `config` (erste Stanza)
    fn configured(conn: &mut Conn, room: &str, config: &Stanza) -> Vec<String> {
        let Stanza::Iq(iq) = config else {
            panic!("keine Einstellungen")
        };
        let id = iq.id().to_owned();
        conn.on_stanza(stanza(&format!(
            r#"<iq xmlns="jabber:client" type="result" from="{room}" id="{id}"/>"#
        )))
        .iter()
        .map(xml_of)
        .collect()
    }
    const NICK: &str = "Alice Muster <alice@pbx.test>";

    /// Alice ist in der Gruppe „Vertrieb“, die auch in der Kontaktliste steht
    fn in_group() -> (Conn, mpsc::UnboundedReceiver<ChatEvent>) {
        let (mut alice, rx) = conn(ALICE);
        assert!(
            alice
                .set_groups("Alice Muster", vec!["Vertrieb".into(), "Lager".into()])
                .is_empty()
        );
        alice.roster_request = "r1".into();
        let out = alice.on_stanza(stanza(
            r#"<iq xmlns="jabber:client" type="result" id="r1"><query xmlns="jabber:iq:roster">
                <item jid="bob@pbx.test" name="Bob"><group>Vertrieb</group></item>
            </query></iq>"#,
        ));
        assert_eq!(out.len(), 1, "nur Vertrieb hat einen Chat");
        let join = xml_of(&out[0]);
        assert!(
            join.contains(&format!("to='{ROOM}/Alice Muster &lt;alice@pbx.test&gt;'")),
            "{join}"
        );
        assert!(join.contains("maxstanzas='100'"), "{join}");
        (alice, rx)
    }

    #[test]
    fn group_room_is_created_when_missing() {
        let (mut alice, _rx) = in_group();
        let out = alice.on_stanza(stanza(&format!(
            r#"<presence xmlns="jabber:client" from="{ROOM}/{}"><x xmlns="http://jabber.org/protocol/muc#user">
                <item affiliation="owner" role="moderator" jid="{ALICE}"/><status code="110"/><status code="201"/>
            </x></presence>"#,
            NICK.replace('<', "&lt;").replace('>', "&gt;")
        )));
        assert!(alice.rooms[ROOM].joined);
        let xml: Vec<_> = out.iter().map(xml_of).collect();
        assert_eq!(xml.len(), 1, "erst einrichten: {xml:?}");
        assert!(
            xml[0].contains("muc#roomconfig_persistentroom") && xml[0].contains("<value>1</value>")
        );
        let xml = configured(&mut alice, ROOM, &out[0]);
        assert_eq!(xml.len(), 1, "{xml:?}");
        assert!(xml[0].contains("<subject>Vertrieb</subject>"), "{}", xml[0]);
    }

    #[test]
    fn group_messages_carry_sender_and_replay_once() {
        let (mut alice, mut rx) = in_group();
        alice.on_stanza(stanza(&format!(
            r#"<presence xmlns="jabber:client" from="{ROOM}/{}"><x xmlns="http://jabber.org/protocol/muc#user">
                <item affiliation="member" role="participant"/><status code="110"/></x></presence>"#,
            NICK.replace('<', "&lt;").replace('>', "&gt;")
        )));
        // Bob ist da
        alice.on_stanza(stanza(&format!(
            r#"<presence xmlns="jabber:client" from="{ROOM}/Bob &lt;bob@pbx.test&gt;"/>"#
        )));
        let ChatEvent::Rooms { rooms } = events(&mut rx).pop().unwrap() else {
            panic!("keine Räume")
        };
        assert_eq!(
            rooms[0].members,
            [Member {
                jid: "bob@pbx.test".into(),
                name: "Bob".into()
            }]
        );
        // Verlauf beim Betreten, dann Thema
        alice.on_stanza(stanza(&format!(
            r#"<message xmlns="jabber:client" from="{ROOM}/Bob &lt;bob@pbx.test&gt;" type="groupchat" id="h1">
                <body>Von gestern</body><delay xmlns="urn:xmpp:delay" stamp="2026-10-08T10:00:00Z"/></message>"#
        )));
        assert!(
            events(&mut rx).is_empty(),
            "Verlauf erst mit dem Thema melden"
        );
        alice.on_stanza(stanza(&format!(
            r#"<message xmlns="jabber:client" from="{ROOM}" type="groupchat"><subject>Vertrieb</subject></message>"#
        )));
        let ev = events(&mut rx);
        assert!(
            matches!(&ev[0], ChatEvent::History { peer, messages } if peer == ROOM && messages.len() == 1)
        );
        // Neue Nachricht von Bob
        alice.on_stanza(stanza(&format!(
            r#"<message xmlns="jabber:client" from="{ROOM}/Bob &lt;bob@pbx.test&gt;" type="groupchat" id="m2"><body>Hoi zäme</body></message>"#
        )));
        let ev = events(&mut rx);
        let ChatEvent::Message { message, notify } = &ev[0] else {
            panic!("keine Nachricht")
        };
        assert!(notify);
        assert_eq!(
            (
                message.peer.as_str(),
                message.sender.as_str(),
                message.sender_name.as_str(),
                message.outgoing
            ),
            (ROOM, "bob@pbx.test", "Bob", false)
        );
        // Eigene Nachricht: gesendet und vom Raum zurück nur einmal im Verlauf
        let out = alice.on_command(Command::Send {
            to: ROOM.into(),
            body: "Hallo".into(),
        });
        let sent = xml_of(&out[0]);
        assert!(
            sent.contains("type='groupchat'") && sent.contains(&format!("to='{ROOM}'")),
            "{sent}"
        );
        let id = alice.history.get(ROOM).last().unwrap().id.clone();
        alice.on_stanza(stanza(&format!(
            r#"<message xmlns="jabber:client" from="{ROOM}/{}" type="groupchat" id="{id}"><body>Hallo</body></message>"#,
            NICK.replace('<', "&lt;").replace('>', "&gt;")
        )));
        let all = alice.history.get(ROOM);
        assert_eq!(all.len(), 3);
        assert!(all[2].outgoing);
        // Das Archiv wird für Räume nicht gefragt
        assert!(
            alice
                .on_command(Command::LoadArchive { peer: ROOM.into() })
                .is_empty()
        );
    }

    #[test]
    fn invitation_joins_and_own_room_invites() {
        let (mut alice, _rx) = in_group();
        let out = alice.on_stanza(stanza(
            r#"<message xmlns="jabber:client" from="abc@chatrooms.pbx.test" to="alice@pbx.test">
                <x xmlns="http://jabber.org/protocol/muc#user"><invite from="bob@pbx.test/win"><reason>Komm</reason></invite></x>
            </message>"#,
        ));
        assert!(xml_of(&out[0]).contains("to='abc@chatrooms.pbx.test/Alice Muster"));
        assert!(!alice.rooms["abc@chatrooms.pbx.test"].group);

        // Eigener spontaner Gruppenchat: Einladungen nach dem Anlegen
        alice.roster.insert(
            "bob@pbx.test".into(),
            Contact {
                jid: "bob@pbx.test".into(),
                name: "Bob".into(),
                show: "online".into(),
                status: String::new(),
                clients: Vec::new(),
            },
        );
        let out = alice.on_command(Command::CreateRoom {
            room: muc::new_room(&alice.own),
            subject: String::new(),
            members: vec!["bob@pbx.test".into()],
        });
        assert_eq!(out.len(), 1);
        let (jid, room) = alice.rooms.iter().find(|(j, _)| j.len() > 40).unwrap();
        assert_eq!(room.name, "Bob");
        let jid = jid.clone();
        let out = alice.on_stanza(stanza(&format!(
            r#"<presence xmlns="jabber:client" from="{jid}/{}"><x xmlns="http://jabber.org/protocol/muc#user">
                <item affiliation="owner" role="moderator"/><status code="110"/><status code="201"/></x></presence>"#,
            NICK.replace('<', "&lt;").replace('>', "&gt;")
        )));
        let xml: Vec<_> = out.iter().map(xml_of).collect();
        assert_eq!(xml.len(), 1, "erst einrichten: {xml:?}");
        assert!(
            xml[0].contains("muc#roomconfig_persistentroom") && xml[0].contains("<value>0</value>")
        );
        let xml = configured(&mut alice, &jid, &out[0]);
        assert_eq!(xml.len(), 2, "{xml:?}");
        assert!(xml[0].contains("<subject>Bob</subject>"), "{}", xml[0]);
        assert!(xml[1].contains("<invite to='bob@pbx.test'"), "{}", xml[1]);
        // Verlassen geht nur bei spontanen Gruppenchats
        assert_eq!(
            alice
                .on_command(Command::LeaveRoom { room: jid.clone() })
                .len(),
            1
        );
        assert!(!alice.rooms.contains_key(&jid));
        assert!(
            alice
                .on_command(Command::LeaveRoom { room: ROOM.into() })
                .is_empty()
        );
    }

    #[test]
    fn spontaneous_rooms_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("sf-chat-rooms-{}", new_id()));
        let file = dir.join("chat.rooms.json");
        let (mut alice, _rx) = in_group();
        alice.rooms_file = Some(file.clone());
        let jid = muc::new_room(&alice.own);
        alice.on_command(Command::CreateRoom {
            room: jid.clone(),
            subject: "test".into(),
            members: vec!["bob@pbx.test".into()],
        });

        // Neustart: Raum mit Namen wieder da und wird betreten
        let (mut alice, _rx) = in_group();
        alice.rooms = muc::load_rooms(Some(&file));
        alice.rooms_file = Some(file.clone());
        assert_eq!(alice.rooms[&jid].name, "test");
        let out = alice.join_rooms();
        assert!(out.iter().any(|s| xml_of(s).contains(&jid)));

        // Raum entsteht beim Betreten neu: Gruppenchat war zu Ende
        let out = alice.on_stanza(stanza(&format!(
            r#"<presence xmlns="jabber:client" from="{jid}/{}"><x xmlns="http://jabber.org/protocol/muc#user">
                <item affiliation="owner" role="moderator"/><status code="110"/><status code="201"/></x></presence>"#,
            NICK.replace('<', "&lt;").replace('>', "&gt;")
        )));
        let xml: Vec<_> = out.iter().map(xml_of).collect();
        assert!(xml.len() == 1 && xml[0].contains("unavailable"), "{xml:?}");
        assert!(!alice.rooms.contains_key(&jid));
        assert!(muc::load_rooms(Some(&file)).is_empty());

        // Späte Präsenz aus dem Raum macht keinen Kontakt daraus
        alice.on_stanza(stanza(&format!(
            r#"<presence xmlns="jabber:client" type="unavailable" from="{jid}/Bob"/>"#
        )));
        assert!(!alice.roster.contains_key(&jid));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn disco_and_ping_are_answered() {
        let (mut alice, _rx) = conn(ALICE);
        let get = |payload: Element| {
            let mut iq = IqHeader {
                from: Some(BOB.parse().unwrap()),
                to: None,
                id: "q1".into(),
            }
            .assemble(IqPayload::Get(payload));
            *iq.from_mut() = Some(BOB.parse().unwrap());
            Stanza::Iq(iq)
        };
        let answer = alice.on_stanza(get(xml::disco_info()));
        let Some(Stanza::Iq(iq)) = answer.into_iter().next() else {
            panic!("keine Antwort")
        };
        let IqPayload::Result(Some(info)) = iq.into_payload() else {
            panic!("kein Ergebnis")
        };
        let (_, features) = xml::disco_info_summary(&info);
        assert!(features.iter().any(|f| f == transfer::NS_FT));
        let ping: Element = "<ping xmlns='urn:xmpp:ping'/>".parse().unwrap();
        let Some(Stanza::Iq(iq)) = alice.on_stanza(get(ping)).into_iter().next() else {
            panic!("keine Antwort")
        };
        assert!(matches!(iq.into_payload(), IqPayload::Result(None)));
    }
}
