//! Dateiübertragung in der Verbindung: Angebote, IBB-Blöcke und die
//! Antworten auf IQ-Anfragen anderer Clients. Die Protokollbausteine stehen
//! in [`crate::transfer`].

use std::path::Path;
use std::time::Instant;

use tokio_xmpp::Stanza;
use tokio_xmpp::jid::Jid;
use tokio_xmpp::minidom::Element;
use tokio_xmpp::parsers::iq::{IqHeader, IqPayload};
use tokio_xmpp::parsers::presence::{Presence, Type as PresenceType};
use tokio_xmpp::parsers::stanza_error::{DefinedCondition, ErrorType, StanzaError};

use crate::transfer::{
    self, DataError, Ibb, Incoming, NS_FT, NS_IBB, NS_SI, Outgoing, SMALL_BLOCK, Transfer,
    TransferState,
};
use crate::{ChatEvent, Conn, Pending, new_id, now_ms, xml};

const NS_PING: &str = "urn:xmpp:ping";
const NS_ROSTER: &str = "jabber:iq:roster";
const NS_VERSION: &str = "jabber:iq:version";

fn iq(to: Option<Jid>, id: String, payload: IqPayload) -> Stanza {
    IqHeader { from: None, to, id }.assemble(payload).into()
}

fn error(type_: ErrorType, condition: DefinedCondition, text: &str) -> IqPayload {
    IqPayload::Error(StanzaError::new(type_, condition, "de", text))
}

fn jid(s: &str) -> Option<Jid> {
    s.parse().ok()
}

/// Lesbarer Grund aus einer Fehlerantwort
fn reason(e: &StanzaError) -> String {
    e.texts
        .values()
        .next()
        .cloned()
        .unwrap_or_else(|| format!("{:?}", e.defined_condition))
}

impl Conn {
    pub(crate) fn emit(&self, t: &Transfer) {
        let _ = self.events.send(ChatEvent::Transfer {
            transfer: t.clone(),
        });
    }

    /// Merkt sich die Clients der Kollegen, damit ein Angebot an eine volle
    /// JID gehen kann.
    pub(crate) fn track_resource(&mut self, p: &Presence) {
        let Some(from) = p.from.as_ref().filter(|j| j.resource().is_some()) else {
            return;
        };
        let bare = from.to_bare().to_string();
        if bare == self.own.to_string() {
            return;
        }
        let full = from.to_string();
        match p.type_ {
            PresenceType::None => {
                let key = xml::caps_of(p).unwrap_or_else(|| full.clone());
                self.resources
                    .entry(bare)
                    .or_default()
                    .insert(full, (key, Instant::now()));
            }
            PresenceType::Unavailable => {
                if let Some(r) = self.resources.get_mut(&bare) {
                    r.remove(&full);
                }
            }
            _ => {}
        }
    }

    /// Client des Kontakts, der per Discovery meldet, dass er Dateien annimmt;
    /// bei mehreren der mit der jüngsten Präsenz (ältere können verwaiste
    /// Sitzungen sein). Clients ohne Antwort (z. B. die Android-App) zählen
    /// nicht: Sie lassen ein Angebot unbeantwortet.
    fn file_target(&self, peer: &str) -> Result<String, &'static str> {
        let clients = self
            .resources
            .get(peer)
            .filter(|r| !r.is_empty())
            .ok_or("Kontakt ist nicht online")?;
        clients
            .iter()
            .filter(|(_, (key, _))| self.client_info.get(key).and_then(|i| i.files) == Some(true))
            .max_by_key(|(_, (_, seen))| *seen)
            .map(|(full, _)| full.clone())
            .ok_or("Der Client des Kontakts kann keine Dateien empfangen")
    }

    fn new_transfer(&self, peer: &str, outgoing: bool, name: String, size: u64) -> Transfer {
        Transfer {
            id: new_id(),
            peer: peer.to_owned(),
            outgoing,
            name,
            size,
            done: 0,
            state: TransferState::Waiting,
            path: String::new(),
            error: String::new(),
            ts: now_ms(),
        }
    }

    /// Ohne Verbindung: Übertragung sofort als gescheitert melden
    pub(crate) fn offline_file(&self, peer: &str, path: &Path) {
        let name = transfer::safe_name(&path.to_string_lossy());
        let mut t = self.new_transfer(peer, true, name, 0);
        t.state = TransferState::Failed;
        t.error = "Chat nicht verbunden".into();
        self.emit(&t);
    }

    pub(crate) fn send_file(&mut self, peer: &str, path: &Path) -> Option<Stanza> {
        let name = transfer::safe_name(&path.to_string_lossy());
        let opened = std::fs::File::open(path).and_then(|f| {
            let meta = f.metadata()?;
            if meta.is_file() {
                Ok((f, meta.len()))
            } else {
                Err(std::io::Error::other("keine Datei"))
            }
        });
        let (file, size) = match opened {
            Ok(x) => x,
            Err(e) => {
                let mut t = self.new_transfer(peer, true, name, 0);
                t.state = TransferState::Failed;
                t.error = e.to_string();
                self.emit(&t);
                return None;
            }
        };
        let mut t = self.new_transfer(peer, true, name, size);
        t.path = path.to_string_lossy().into_owned();
        let to = match self.file_target(peer) {
            Ok(to) => to,
            Err(e) => {
                t.state = TransferState::Failed;
                t.error = e.into();
                self.emit(&t);
                return None;
            }
        };
        let id = new_id();
        let stanza = iq(
            jid(&to),
            id.clone(),
            IqPayload::Set(transfer::offer(&t.id, &t.name, size)),
        );
        self.pending.insert(id, Pending::Offer(t.id.clone()));
        tracing::info!(to = %to, name = %t.name, size, "Datei angeboten");
        self.emit(&t);
        self.outgoing
            .insert(t.id.clone(), Outgoing::new(t, to, file));
        Some(stanza)
    }

    pub(crate) fn accept_file(&mut self, id: &str, dir: &Path) -> Vec<Stanza> {
        let Some(inc) = self.incoming.get_mut(id) else {
            return Vec::new();
        };
        if inc.t.state != TransferState::Offered {
            return Vec::new();
        }
        let to = jid(&inc.from);
        let offer_iq = inc.offer_iq.clone();
        match inc.create(dir) {
            Ok(()) => {
                let t = inc.t.clone();
                self.emit(&t);
                vec![iq(
                    to,
                    offer_iq,
                    IqPayload::Result(Some(transfer::accept())),
                )]
            }
            Err(e) => {
                inc.fail(format!("Datei nicht angelegt: {e}"));
                let t = inc.t.clone();
                self.incoming.remove(id);
                self.emit(&t);
                vec![iq(
                    to,
                    offer_iq,
                    error(
                        ErrorType::Cancel,
                        DefinedCondition::Forbidden,
                        "Offer Declined",
                    ),
                )]
            }
        }
    }

    pub(crate) fn decline_file(&mut self, id: &str) -> Option<Stanza> {
        let mut inc = self.incoming.remove(id)?;
        if inc.t.state != TransferState::Offered {
            self.incoming.insert(id.to_owned(), inc);
            return None;
        }
        inc.t.state = TransferState::Declined;
        self.emit(&inc.t);
        Some(iq(
            jid(&inc.from),
            inc.offer_iq,
            error(
                ErrorType::Cancel,
                DefinedCondition::Forbidden,
                "Offer Declined",
            ),
        ))
    }

    pub(crate) fn cancel_file(&mut self, id: &str) -> Option<Stanza> {
        if let Some(mut out) = self.outgoing.remove(id) {
            let running = out.t.state == TransferState::Running;
            out.t.state = TransferState::Cancelled;
            self.emit(&out.t);
            return running.then(|| self.close_stanza(&out.to, id));
        }
        let mut inc = self.incoming.remove(id)?;
        match inc.t.state {
            TransferState::Offered => {
                self.incoming.insert(id.to_owned(), inc);
                self.decline_file(id)
            }
            TransferState::Running => {
                inc.discard(TransferState::Cancelled);
                self.emit(&inc.t);
                Some(self.close_stanza(&inc.from, id))
            }
            _ => None,
        }
    }

    fn close_stanza(&mut self, to: &str, sid: &str) -> Stanza {
        let id = new_id();
        self.pending
            .insert(id.clone(), Pending::Close(sid.to_owned()));
        iq(jid(to), id, IqPayload::Set(transfer::ibb_close(sid)))
    }

    /// Bei neuer Verbindung: alles Laufende ist verloren.
    pub(crate) fn drop_transfers(&mut self) {
        for (_, mut out) in std::mem::take(&mut self.outgoing) {
            out.t.state = TransferState::Failed;
            out.t.error = "Verbindung unterbrochen".into();
            self.emit(&out.t);
        }
        for (_, mut inc) in std::mem::take(&mut self.incoming) {
            inc.fail("Verbindung unterbrochen".into());
            self.emit(&inc.t);
        }
        self.pending.retain(|_, p| matches!(p, Pending::Disco(_)));
    }

    fn fail_outgoing(&mut self, sid: &str, error: String, close: bool) -> Vec<Stanza> {
        let Some(mut out) = self.outgoing.remove(sid) else {
            return Vec::new();
        };
        tracing::warn!(name = %out.t.name, %error, "Datei nicht gesendet");
        out.t.state = TransferState::Failed;
        out.t.error = error;
        self.emit(&out.t);
        if close {
            vec![self.close_stanza(&out.to, sid)]
        } else {
            Vec::new()
        }
    }

    /// Nächste Datenblöcke senden; am Ende schließen.
    fn pump(&mut self, sid: &str) -> Vec<Stanza> {
        let Some(out) = self.outgoing.get_mut(sid) else {
            return Vec::new();
        };
        let blocks = match out.fill() {
            Ok(b) => b,
            Err(e) => return self.fail_outgoing(sid, e.to_string(), true),
        };
        let to = out.to.clone();
        let finished = out.finished();
        let mut stanzas = Vec::new();
        for (el, len) in blocks {
            let id = new_id();
            self.pending
                .insert(id.clone(), Pending::Data(sid.to_owned(), len));
            stanzas.push(iq(jid(&to), id, IqPayload::Set(el)));
        }
        if finished {
            stanzas.push(self.close_stanza(&to, sid));
        }
        stanzas
    }

    /// Antwort auf eine eigene Anfrage zu einer Übertragung
    pub(crate) fn on_transfer_result(
        &mut self,
        request: Pending,
        result: Result<Option<Element>, StanzaError>,
    ) -> Vec<Stanza> {
        match request {
            Pending::Offer(sid) => {
                let Some(out) = self.outgoing.get_mut(&sid) else {
                    return Vec::new();
                };
                match result {
                    Ok(Some(si)) if transfer::chose_ibb(&si) => {
                        out.t.state = TransferState::Running;
                        let (to, block) = (out.to.clone(), out.block);
                        let t = out.t.clone();
                        self.emit(&t);
                        let id = new_id();
                        self.pending.insert(id.clone(), Pending::Open(sid.clone()));
                        vec![iq(
                            jid(&to),
                            id,
                            IqPayload::Set(transfer::ibb_open(&sid, block)),
                        )]
                    }
                    Ok(_) => self.fail_outgoing(
                        &sid,
                        "Gegenseite unterstützt die Übertragung nicht".into(),
                        false,
                    ),
                    Err(e)
                        if matches!(
                            e.defined_condition,
                            DefinedCondition::Forbidden | DefinedCondition::NotAcceptable
                        ) =>
                    {
                        if let Some(mut out) = self.outgoing.remove(&sid) {
                            out.t.state = TransferState::Declined;
                            self.emit(&out.t);
                        }
                        Vec::new()
                    }
                    Err(e) => self.fail_outgoing(&sid, reason(&e), false),
                }
            }
            Pending::Open(sid) => match result {
                Ok(_) => self.pump(&sid),
                Err(e) => {
                    // Zu große Blöcke: mit dem Standard von Smack noch einmal
                    if let Some(out) = self.outgoing.get_mut(&sid)
                        && e.defined_condition == DefinedCondition::ResourceConstraint
                        && out.block > SMALL_BLOCK
                    {
                        out.block = SMALL_BLOCK;
                        let to = out.to.clone();
                        let id = new_id();
                        self.pending.insert(id.clone(), Pending::Open(sid.clone()));
                        return vec![iq(
                            jid(&to),
                            id,
                            IqPayload::Set(transfer::ibb_open(&sid, SMALL_BLOCK)),
                        )];
                    }
                    tracing::warn!(error = ?e.defined_condition, "IBB nicht geöffnet");
                    self.fail_outgoing(&sid, reason(&e), false)
                }
            },
            Pending::Data(sid, len) => match result {
                Ok(_) => {
                    let Some(out) = self.outgoing.get_mut(&sid) else {
                        return Vec::new();
                    };
                    if out.acked(len) {
                        let t = out.t.clone();
                        self.emit(&t);
                    }
                    self.pump(&sid)
                }
                Err(e) => {
                    tracing::warn!(error = ?e.defined_condition, "Datenblock abgelehnt");
                    self.fail_outgoing(&sid, reason(&e), true)
                }
            },
            Pending::Close(sid) => {
                if let Some(mut out) = self.outgoing.remove(&sid) {
                    if out.finished() && out.t.state == TransferState::Running {
                        out.t.state = TransferState::Done;
                        tracing::info!(name = %out.t.name, "Datei gesendet");
                    }
                    self.emit(&out.t);
                }
                Vec::new()
            }
            Pending::Disco(_) | Pending::Version(_) | Pending::RoomConfig(_) => Vec::new(),
        }
    }

    /// Anfragen anderer an uns (`type="get"`)
    pub(crate) fn on_iq_get(&mut self, from: Option<Jid>, id: String, el: Element) -> Vec<Stanza> {
        let payload = if el.is("query", xml::NS_DISCO_INFO) {
            IqPayload::Result(Some(xml::own_disco_info(&[
                xml::NS_DISCO_INFO,
                crate::muc::NS_MUC,
                NS_PING,
                NS_VERSION,
                NS_SI,
                NS_FT,
                NS_IBB,
            ])))
        } else if el.is("query", NS_VERSION) {
            IqPayload::Result(Some(xml::own_version(env!("CARGO_PKG_VERSION"))))
        } else if el.is("ping", NS_PING) {
            IqPayload::Result(None)
        } else {
            error(ErrorType::Cancel, DefinedCondition::ServiceUnavailable, "")
        };
        vec![iq(from, id, payload)]
    }

    /// Anfragen anderer an uns (`type="set"`)
    pub(crate) fn on_iq_set(&mut self, from: Option<Jid>, id: String, el: Element) -> Vec<Stanza> {
        if el.is("query", NS_ROSTER) {
            // Änderung der Kontaktliste: bestätigen, die Liste kommt beim
            // nächsten Verbindungsaufbau neu
            return vec![iq(from, id, IqPayload::Result(None))];
        }
        let Some(sender) = from.clone() else {
            return vec![iq(
                from,
                id,
                error(ErrorType::Cancel, DefinedCondition::ServiceUnavailable, ""),
            )];
        };
        if el.is("si", NS_SI) {
            return self.on_offer(sender, id, &el);
        }
        if let Some(ibb) = transfer::parse_ibb(&el) {
            return self.on_ibb(sender, id, ibb);
        }
        vec![iq(
            from,
            id,
            error(ErrorType::Cancel, DefinedCondition::ServiceUnavailable, ""),
        )]
    }

    fn on_offer(&mut self, from: Jid, id: String, el: &Element) -> Vec<Stanza> {
        let Some(offer) = transfer::parse_offer(el) else {
            return vec![iq(
                Some(from),
                id,
                error(ErrorType::Modify, DefinedCondition::BadRequest, ""),
            )];
        };
        if !offer.ibb {
            return vec![iq(
                Some(from),
                id,
                error(
                    ErrorType::Cancel,
                    DefinedCondition::BadRequest,
                    "No Valid Streams",
                ),
            )];
        }
        if self.incoming.contains_key(&offer.sid) || self.outgoing.contains_key(&offer.sid) {
            return vec![iq(
                Some(from),
                id,
                error(ErrorType::Cancel, DefinedCondition::Conflict, ""),
            )];
        }
        let peer = from.to_bare().to_string();
        let t = Transfer {
            id: offer.sid.clone(),
            state: TransferState::Offered,
            ..self.new_transfer(&peer, false, transfer::safe_name(&offer.name), offer.size)
        };
        tracing::info!(from = %from, name = %t.name, size = t.size, "Datei angeboten bekommen");
        self.emit(&t);
        self.incoming
            .insert(offer.sid, Incoming::new(t, from.to_string(), id));
        Vec::new()
    }

    fn on_ibb(&mut self, from: Jid, id: String, ibb: Ibb) -> Vec<Stanza> {
        let sender = from.to_string();
        let to = Some(from);
        let not_found = || error(ErrorType::Cancel, DefinedCondition::ItemNotFound, "");
        match ibb {
            Ibb::Open { sid, block } => {
                let payload = match self.incoming.get(&sid).filter(|i| i.from == sender) {
                    Some(inc) if inc.accepts_block(block) => IqPayload::Result(None),
                    Some(_) => error(ErrorType::Modify, DefinedCondition::ResourceConstraint, ""),
                    None => not_found(),
                };
                vec![iq(to, id, payload)]
            }
            Ibb::Data { sid, seq, bytes } => {
                let Some(inc) = self.incoming.get_mut(&sid).filter(|i| i.from == sender) else {
                    return vec![iq(to, id, not_found())];
                };
                match inc.data(seq, &bytes) {
                    Ok(progress) => {
                        if progress {
                            let t = inc.t.clone();
                            self.emit(&t);
                        }
                        vec![iq(to, id, IqPayload::Result(None))]
                    }
                    Err(e) => {
                        let text = match e {
                            DataError::Order => "Block außer der Reihe".to_owned(),
                            DataError::Io(e) => e,
                        };
                        inc.fail(text);
                        let t = inc.t.clone();
                        self.incoming.remove(&sid);
                        self.emit(&t);
                        vec![iq(
                            to,
                            id,
                            error(ErrorType::Cancel, DefinedCondition::UnexpectedRequest, ""),
                        )]
                    }
                }
            }
            Ibb::Close { sid } => {
                if let Some(mut inc) = self.incoming.remove(&sid) {
                    if inc.from != sender {
                        self.incoming.insert(sid, inc);
                        return vec![iq(to, id, not_found())];
                    }
                    inc.close();
                    if inc.t.state == TransferState::Done {
                        tracing::info!(name = %inc.t.name, path = %inc.t.path, "Datei empfangen");
                    }
                    self.emit(&inc.t);
                } else if let Some(out) = self.outgoing.get(&sid).filter(|o| o.to == sender) {
                    // Gegenseite bricht ab
                    let mut t = out.t.clone();
                    self.outgoing.remove(&sid);
                    t.state = TransferState::Cancelled;
                    t.error = "Von der Gegenseite abgebrochen".into();
                    self.emit(&t);
                } else {
                    return vec![iq(to, id, not_found())];
                }
                vec![iq(to, id, IqPayload::Result(None))]
            }
        }
    }
}
