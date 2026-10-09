//! Gruppenchats (Multi-User Chat, XEP-0045) wie beim Windows-Client.
//!
//! Für jede Gruppe der Anlage, in der man Mitglied ist und die auch als
//! Gruppe in der Kontaktliste steht (Gruppen mit Chat-Option), gibt es einen
//! dauerhaften Raum `<name>@chatrooms.<anlage>`. Der Name ist der
//! Gruppenname klein geschrieben, ohne Zeichen ausser a-z, 0-9, `_`, `-` und
//! `.`. Wer zuerst kommt, legt den Raum an. Dazu kommen spontane Gruppenchats
//! in Räumen mit zufälligem Namen, die man selbst startet oder zu denen man
//! eingeladen wird; sie verschwinden, wenn alle gegangen sind.
//!
//! Der Spitzname im Raum ist „Name <Jabber-ID>“, daraus lesen die Clients
//! Absender und Namen. Den Verlauf liefert der Raum beim Betreten (die
//! letzten 100 Nachrichten aus 27 Tagen); das Serverarchiv (XEP-0136) führt
//! keine Gruppenchats.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use tokio_xmpp::Stanza;
use tokio_xmpp::jid::{BareJid, Jid};
use tokio_xmpp::minidom::Element;
use tokio_xmpp::parsers::iq::{IqHeader, IqPayload};
use tokio_xmpp::parsers::message::{Lang, Message, MessageType};
use tokio_xmpp::parsers::presence::{Presence, Type as PresenceType};

use crate::xml::n;
use crate::{ChatEvent, ChatMessage, Conn, Pending, new_id, now_ms};

pub const NS_MUC: &str = "http://jabber.org/protocol/muc";
const NS_MUC_USER: &str = "http://jabber.org/protocol/muc#user";
const NS_MUC_OWNER: &str = "http://jabber.org/protocol/muc#owner";
const NS_DATA: &str = "jabber:x:data";
/// Direkte Einladung (XEP-0249)
const NS_CONFERENCE: &str = "jabber:x:conference";
/// Alte Form der verzögerten Zustellung (XEP-0091), die Smack noch liest
const NS_LEGACY_DELAY: &str = "jabber:x:delay";
/// Dienst der Anlage für Gruppenchats (vor der Domain)
const SERVICE: &str = "chatrooms";
/// Verlauf beim Betreten, wie beim Windows-Client
const HISTORY_STANZAS: u32 = 100;
const HISTORY_DAYS: i64 = 27;
/// So oft wird ein abgewiesenes Betreten je Verbindung wiederholt (etwa
/// wenn ein anderer den Raum gerade anlegt und er noch gesperrt ist)
const MAX_FAILURES: u8 = 5;
/// Status 110: Präsenz betrifft einen selbst; 201: Raum wurde neu angelegt
const STATUS_SELF: u16 = 110;
const STATUS_CREATED: u16 = 201;

/// Ein Gruppenchat für die Oberfläche
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Room {
    pub jid: String,
    pub name: String,
    /// Raum einer Gruppe der Anlage; sonst ein spontaner Gruppenchat
    pub group: bool,
    /// Man ist im Raum angemeldet
    pub joined: bool,
    /// Anwesende ausser einem selbst
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Member {
    /// Bare JID, leer, wenn der Raum sie nicht verrät
    pub jid: String,
    pub name: String,
}

#[derive(Debug, Default)]
pub(crate) struct RoomState {
    pub name: String,
    pub group: bool,
    pub joined: bool,
    /// Betreten ist angefragt
    pub requested: bool,
    /// Fehlgeschlagene Versuche in dieser Verbindung
    pub failures: u8,
    pub password: Option<String>,
    /// Spitzname → Teilnehmer
    pub occupants: BTreeMap<String, Member>,
    /// Nach dem Anlegen einzuladen
    pub invite: Vec<String>,
    /// Verlauf beim Betreten ist eingegangen, aber noch nicht gemeldet
    pub replayed: bool,
}

/// Lokaler Teil des Raums einer Gruppe, wie beim Windows-Client
pub fn room_local(group: &str) -> String {
    group
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        .collect()
}

/// Eigener Spitzname in Räumen: „Name <Jabber-ID>“
pub fn nick(display_name: &str, own: &BareJid) -> String {
    let name = display_name.trim();
    let name = if name.is_empty() {
        own.node().map_or("", |n| n.as_str())
    } else {
        name
    };
    format!("{name} <{own}>")
}

/// Name und Jabber-ID aus einem Spitznamen „Name <Jabber-ID>“
pub fn parse_nick(nick: &str) -> (String, Option<String>) {
    if let Some(inner) = nick.strip_suffix('>')
        && let Some((name, jid)) = inner.split_once('<')
    {
        let jid = jid.trim();
        if !jid.is_empty() {
            return (name.trim().to_owned(), Some(jid.to_owned()));
        }
    }
    (nick.to_owned(), None)
}

/// Zufälliger Raumname für spontane Gruppenchats (im Format einer GUID wie
/// beim Windows-Client)
fn random_local() -> String {
    use std::hash::BuildHasher;
    let a = std::collections::hash_map::RandomState::new().hash_one(now_ms());
    let b = std::collections::hash_map::RandomState::new().hash_one(a);
    let h = format!("{a:016x}{b:016x}");
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

/// Neuer Raum für einen spontanen Gruppenchat
pub fn new_room(own: &BareJid) -> String {
    format!("{}@{SERVICE}.{}", random_local(), own.domain())
}

fn occupant_jid(room: &str, nick: &str) -> Option<Jid> {
    format!("{room}/{nick}").parse().ok()
}

/// Raum betreten (oder anlegen), mit dem Verlauf der letzten Tage
fn join(room: &str, nick: &str, password: Option<&str>) -> Option<Stanza> {
    let since = chrono::DateTime::from_timestamp_millis(now_ms() - HISTORY_DAYS * 86_400_000)?
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let mut x = Element::builder("x", NS_MUC).append(
        Element::builder("history", NS_MUC)
            .attr(n("maxstanzas"), HISTORY_STANZAS.to_string())
            .attr(n("since"), since)
            .build(),
    );
    if let Some(pw) = password {
        x = x.append(Element::builder("password", NS_MUC).append(pw).build());
    }
    let mut p = Presence::available();
    p.to = Some(occupant_jid(room, nick)?);
    p.payloads.push(x.build());
    Some(p.into())
}

fn leave(room: &str, nick: &str) -> Option<Stanza> {
    let mut p = Presence::unavailable();
    p.to = Some(occupant_jid(room, nick)?);
    Some(p.into())
}

fn field(var: &str, value: &str) -> Element {
    Element::builder("field", NS_DATA)
        .attr(n("var"), var)
        .append(Element::builder("value", NS_DATA).append(value).build())
        .build()
}

/// Einstellungen für einen neu angelegten Raum, wie beim Windows-Client:
/// Gruppenräume dauerhaft und öffentlich, spontane nur solange jemand drin ist
fn config(group: bool, description: &str) -> Element {
    let flag = if group { "1" } else { "0" };
    let mut x = Element::builder("x", NS_DATA)
        .attr(n("type"), "submit")
        .append(field(
            "FORM_TYPE",
            "http://jabber.org/protocol/muc#roomconfig",
        ));
    if !description.is_empty() {
        x = x
            .append(field("muc#roomconfig_roomdesc", description))
            .append(field("muc#roomconfig_changesubject", "1"));
    }
    x = x
        .append(field("muc#roomconfig_persistentroom", flag))
        .append(field("muc#roomconfig_allowinvites", "1"))
        .append(field(
            "muc#roomconfig_maxusers",
            if group { "3000" } else { "50" },
        ))
        .append(field("muc#roomconfig_publicroom", flag));
    Element::builder("query", NS_MUC_OWNER)
        .append(x.build())
        .build()
}

fn subject(room: &BareJid, text: &str) -> Stanza {
    let mut m = Message::new_with_type(MessageType::Groupchat, Some(Jid::from(room.clone())));
    m.subjects.insert(Lang::default(), text.to_owned());
    m.into()
}

/// Einladung über den Raum (XEP-0045, „mediated invitation“), wie Smack sie
/// schickt
fn invite(room: &BareJid, to: &str, reason: &str) -> Stanza {
    let mut m = Message::new_with_type(MessageType::Normal, Some(Jid::from(room.clone())));
    m.payloads.push(
        Element::builder("x", NS_MUC_USER)
            .append(
                Element::builder("invite", NS_MUC_USER)
                    .attr(n("to"), to)
                    .append(
                        Element::builder("reason", NS_MUC_USER)
                            .append(reason)
                            .build(),
                    )
                    .build(),
            )
            .build(),
    );
    m.into()
}

/// `muc#user`-Angaben einer Präsenz: echte JID und Statuscodes
fn muc_user(payloads: &[Element]) -> (Option<String>, BTreeSet<u16>) {
    let Some(x) = payloads.iter().find(|e| e.is("x", NS_MUC_USER)) else {
        return (None, BTreeSet::new());
    };
    let jid = x
        .get_child("item", NS_MUC_USER)
        .and_then(|i| i.attr("jid"))
        .and_then(|j| j.parse::<Jid>().ok())
        .map(|j| j.to_bare().to_string());
    let codes = x
        .children()
        .filter(|c| c.is("status", NS_MUC_USER))
        .filter_map(|c| c.attr("code")?.parse().ok())
        .collect();
    (jid, codes)
}

/// Raum und ggf. Passwort aus einer Einladung (über den Raum oder direkt)
fn invitation(m: &Message) -> Option<(String, Option<String>)> {
    for p in &m.payloads {
        if p.is("x", NS_MUC_USER) && p.get_child("invite", NS_MUC_USER).is_some() {
            let room = m.from.as_ref()?.to_bare().to_string();
            let password = p.get_child("password", NS_MUC_USER).map(Element::text);
            return Some((room, password));
        }
        if p.is("x", NS_CONFERENCE) {
            let room = p.attr("jid")?.parse::<BareJid>().ok()?.to_string();
            return Some((room, p.attr("password").map(str::to_owned)));
        }
    }
    None
}

/// Zeitstempel einer verzögerten Nachricht in ms, auch in der alten Form
/// `jabber:x:delay` (`20261009T12:00:00`)
fn delayed(m: &Message) -> Option<i64> {
    if let Some(d) = crate::xml::delay_of(m) {
        return Some(d.stamp.0.timestamp_millis());
    }
    let stamp = m
        .payloads
        .iter()
        .find(|p| p.is("x", NS_LEGACY_DELAY))?
        .attr("stamp")?;
    chrono::NaiveDateTime::parse_from_str(stamp, "%Y%m%dT%H:%M:%S")
        .ok()
        .map(|t| t.and_utc().timestamp_millis())
}

impl Conn {
    fn service(&self) -> String {
        format!("{SERVICE}.{}", self.own.domain())
    }

    pub(crate) fn is_room(&self, jid: &str) -> bool {
        self.rooms.contains_key(jid)
    }

    /// Eigener Name und Gruppen der Anlage; liefert die Anfragen zum Betreten
    /// neuer Gruppenräume.
    pub(crate) fn set_groups(&mut self, display_name: &str, groups: Vec<String>) -> Vec<Stanza> {
        self.nick = nick(display_name, &self.own);
        self.groups = groups;
        self.want_group_rooms();
        self.join_rooms()
    }

    /// Legt Räume für Gruppen an, die es auch in der Kontaktliste gibt (so
    /// entscheidet der Windows-Client, welche Gruppen einen Chat haben).
    pub(crate) fn want_group_rooms(&mut self) {
        let service = self.service();
        for group in &self.groups {
            let local = room_local(group);
            if local.is_empty() || !self.roster_groups.contains(group) {
                continue;
            }
            self.rooms
                .entry(format!("{local}@{service}"))
                .or_insert_with(|| RoomState {
                    name: group.clone(),
                    group: true,
                    ..Default::default()
                });
        }
        self.publish_rooms();
    }

    /// Betritt alle Räume, für die das in dieser Verbindung noch aussteht.
    pub(crate) fn join_rooms(&mut self) -> Vec<Stanza> {
        if self.nick.is_empty() {
            return Vec::new();
        }
        let nick = self.nick.clone();
        self.rooms
            .iter_mut()
            .filter(|(_, r)| !r.requested)
            .filter_map(|(jid, r)| {
                r.requested = true;
                tracing::info!(room = %jid, "Gruppenchat: betrete Raum");
                join(jid, &nick, r.password.as_deref())
            })
            .collect()
    }

    /// Neue Verbindung: alle Räume neu betreten
    pub(crate) fn rooms_offline(&mut self) {
        for r in self.rooms.values_mut() {
            r.joined = false;
            r.requested = false;
            r.failures = 0;
            r.occupants.clear();
        }
        self.publish_rooms();
    }

    /// Spontanen Gruppenchat starten und `members` einladen
    pub(crate) fn create_room(
        &mut self,
        jid: String,
        subject: &str,
        members: Vec<String>,
    ) -> Vec<Stanza> {
        let name = if subject.trim().is_empty() {
            members
                .iter()
                .map(|m| self.contact_name(m))
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            subject.trim().to_owned()
        };
        tracing::info!(room = %jid, "Gruppenchat: lege Raum an");
        self.rooms.insert(
            jid,
            RoomState {
                name,
                invite: members,
                ..Default::default()
            },
        );
        self.publish_rooms();
        self.join_rooms()
    }

    /// Spontanen Gruppenchat verlassen; Gruppenräume bleiben.
    pub(crate) fn leave_room(&mut self, jid: &str) -> Vec<Stanza> {
        if self.rooms.get(jid).is_none_or(|r| r.group) {
            return Vec::new();
        }
        self.rooms.remove(jid);
        self.publish_rooms();
        leave(jid, &self.nick).into_iter().collect()
    }

    fn contact_name(&self, jid: &str) -> String {
        self.roster.get(jid).map_or_else(
            || jid.split('@').next().unwrap_or_default().to_owned(),
            |c| c.name.clone(),
        )
    }

    pub(crate) fn publish_rooms(&self) {
        let rooms = self
            .rooms
            .iter()
            .map(|(jid, r)| Room {
                jid: jid.clone(),
                name: r.name.clone(),
                group: r.group,
                joined: r.joined,
                members: r.occupants.values().cloned().collect(),
            })
            .collect();
        let _ = self.events.send(ChatEvent::Rooms { rooms });
    }

    /// Präsenz aus einem Raum: wer da ist, und ob man selbst drin ist
    pub(crate) fn on_room_presence(&mut self, room: BareJid, p: &Presence) -> Vec<Stanza> {
        let key = room.to_string();
        let own = self.own.to_string();
        let nick = p
            .from
            .as_ref()
            .and_then(|f| f.resource())
            .map(|r| r.as_str().to_owned())
            .unwrap_or_default();
        let (real, codes) = muc_user(&p.payloads);
        let Some(r) = self.rooms.get_mut(&key) else {
            return Vec::new();
        };
        let me = codes.contains(&STATUS_SELF) || nick == self.nick;
        let mut out = Vec::new();
        match p.type_ {
            PresenceType::Error => {
                r.joined = false;
                r.failures += 1;
                // Später noch einmal (siehe `join_rooms` im Takt von `run`)
                r.requested = r.failures >= MAX_FAILURES;
                tracing::warn!(room = %key, failures = r.failures, "Gruppenchat: Raum nicht betreten");
            }
            PresenceType::Unavailable if me => r.joined = false,
            PresenceType::Unavailable => {
                r.occupants.remove(&nick);
            }
            PresenceType::None if me => {
                r.joined = true;
                if codes.contains(&STATUS_CREATED) {
                    tracing::info!(room = %key, "Gruppenchat: Raum neu angelegt");
                    let id = new_id();
                    let iq = IqHeader {
                        from: None,
                        to: Some(Jid::from(room.clone())),
                        id: id.clone(),
                    }
                    .assemble(IqPayload::Set(config(r.group, &r.name)));
                    out.push(iq.into());
                    out.push(subject(&room, &r.name));
                    self.pending.insert(id, Pending::RoomConfig(key.clone()));
                }
                let reason = format!(
                    "{} lädt dich in den Gruppenchat ein",
                    parse_nick(&self.nick).0
                );
                out.extend(r.invite.drain(..).map(|to| invite(&room, &to, &reason)));
            }
            PresenceType::None => {
                let (name, from_nick) = parse_nick(&nick);
                let jid = real.or(from_nick).unwrap_or_default();
                if jid != own {
                    r.occupants.insert(nick, Member { jid, name });
                }
            }
            _ => return Vec::new(),
        }
        self.publish_rooms();
        out
    }

    /// Nachricht an einen Raum senden
    pub(crate) fn send_to_room(&mut self, room: &str, body: String) -> Vec<Stanza> {
        let Ok(to) = room.parse::<BareJid>() else {
            return Vec::new();
        };
        let id = new_id();
        let mut msg = Message::new_with_type(MessageType::Groupchat, Some(Jid::from(to)))
            .with_body(Lang::default(), body.clone());
        msg.id = Some(tokio_xmpp::parsers::message::Id(id.clone()));
        self.store(
            ChatMessage {
                id,
                peer: room.to_owned(),
                outgoing: true,
                body,
                ts: now_ms(),
                sender: self.own.to_string(),
                sender_name: String::new(),
            },
            false,
        );
        vec![msg.into()]
    }

    /// Nachricht aus einem Raum; `None`, wenn sie keinen Raum betrifft.
    pub(crate) fn on_room_message(&mut self, m: &Message) -> Option<Vec<Stanza>> {
        let from = m.from.as_ref()?;
        let room = from.to_bare().to_string();
        if !self.rooms.contains_key(&room) {
            return self.on_invitation(m);
        }
        if m.type_ == MessageType::Error {
            tracing::warn!(%room, "Gruppenchat: Fehler vom Raum");
            return Some(Vec::new());
        }
        if m.type_ != MessageType::Groupchat {
            return Some(Vec::new());
        }
        // Thema: kommt nach dem Verlauf beim Betreten
        if let Some((_, s)) = m.get_best_subject(vec!["de", "en"]) {
            let r = self.rooms.get_mut(&room)?;
            if !r.group && !s.trim().is_empty() {
                r.name = s.trim().to_owned();
            }
            self.flush_replay(&room);
            self.publish_rooms();
            return Some(Vec::new());
        }
        let nick = from.resource()?.as_str().to_owned();
        let (_, body) = m.get_best_body_cloned(vec!["de", "en"])?;
        if body.trim().is_empty() {
            return Some(Vec::new());
        }
        let r = self.rooms.get(&room)?;
        let (mut name, mut jid) = parse_nick(&nick);
        if jid.is_none()
            && let Some(o) = r.occupants.get(&nick)
        {
            (name, jid) = (o.name.clone(), Some(o.jid.clone()));
        }
        let own = self.own.to_string();
        let outgoing = nick == self.nick || jid.as_deref() == Some(own.as_str());
        let delay = delayed(m);
        let ts = delay.unwrap_or_else(now_ms);
        let msg = ChatMessage {
            id: m
                .id
                .as_ref()
                .map_or_else(|| format!("m{ts}{}", u8::from(outgoing)), |i| i.0.clone()),
            peer: room.clone(),
            outgoing,
            body,
            ts,
            sender: jid.unwrap_or_default(),
            sender_name: if outgoing { String::new() } else { name },
        };
        if delay.is_some() {
            // Verlauf beim Betreten: still übernehmen, gemeldet wird gesammelt
            if self.history.add(msg) {
                self.rooms.get_mut(&room)?.replayed = true;
            }
        } else {
            self.flush_replay(&room);
            self.store(msg, !outgoing);
        }
        Some(Vec::new())
    }

    fn flush_replay(&mut self, room: &str) {
        if let Some(r) = self.rooms.get_mut(room)
            && std::mem::take(&mut r.replayed)
        {
            let _ = self.events.send(ChatEvent::History {
                peer: room.to_owned(),
                messages: self.history.get(room),
            });
        }
    }

    /// Einladung in einen spontanen Gruppenchat: gleich betreten, wie der
    /// Windows-Client
    fn on_invitation(&mut self, m: &Message) -> Option<Vec<Stanza>> {
        let (room, password) = invitation(m)?;
        if room.parse::<BareJid>().is_err() || self.rooms.contains_key(&room) {
            return Some(Vec::new());
        }
        tracing::info!(%room, "Gruppenchat: Einladung");
        self.rooms.insert(
            room,
            RoomState {
                password,
                ..Default::default()
            },
        );
        self.publish_rooms();
        Some(self.join_rooms())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_names_follow_the_windows_client() {
        assert_eq!(room_local("Vertrieb Team"), "vertriebteam");
        assert_eq!(room_local("Büro_Zürich-2.OG"), "bro_zrich-2.og");
        assert_eq!(room_local("ÄÖÜ"), "");
    }

    #[test]
    fn nick_carries_name_and_jid() {
        let own: BareJid = "1001@pbx.test".parse().unwrap();
        assert_eq!(nick("Claude Star", &own), "Claude Star <1001@pbx.test>");
        assert_eq!(nick(" ", &own), "1001 <1001@pbx.test>");
        assert_eq!(
            parse_nick("Claude Star <1001@pbx.test>"),
            ("Claude Star".into(), Some("1001@pbx.test".into()))
        );
        assert_eq!(parse_nick("Gast"), ("Gast".into(), None));
        // Der Spitzname taugt als Ressource einer JID
        assert!(occupant_jid("a@chatrooms.pbx.test", &nick("Claude Star", &own)).is_some());
    }

    #[test]
    fn legacy_delay_is_read() {
        let el: Element =
            r#"<message xmlns="jabber:client" from="r@chatrooms.pbx/x" type="groupchat">
            <body>Hoi</body><x xmlns="jabber:x:delay" stamp="20261009T10:00:00"/>
        </message>"#
                .parse()
                .unwrap();
        let m = Message::try_from(el).unwrap();
        assert_eq!(
            delayed(&m),
            chrono::DateTime::parse_from_rfc3339("2026-10-09T10:00:00Z")
                .ok()
                .map(|t| t.timestamp_millis())
        );
    }

    #[test]
    fn random_rooms_differ() {
        let (a, b) = (random_local(), random_local());
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
    }
}
