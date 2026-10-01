//! XML-Bausteine, die xmpp-parsers nicht fertig mitbringt.

use tokio_xmpp::Stanza;
use tokio_xmpp::minidom::Element;
use tokio_xmpp::parsers::carbons::{Received, Sent};
use tokio_xmpp::parsers::delay::Delay;
use tokio_xmpp::parsers::forwarding::Forwarded;
use tokio_xmpp::parsers::iq::{IqHeader, IqPayload};
use tokio_xmpp::parsers::message::Message;

use crate::{ChatMessage, Contact};

const NS_ROSTER: &str = "jabber:iq:roster";
const NS_CARBONS: &str = "urn:xmpp:carbons:2";
const NS_DELAY: &str = "urn:xmpp:delay";
const NS_ARCHIVE: &str = "urn:xmpp:archive";
const NS_RSM: &str = "http://jabber.org/protocol/rsm";

fn header(id: &str) -> IqHeader {
    IqHeader {
        from: None,
        to: None,
        id: id.to_owned(),
    }
}

pub fn iq_get(id: &str, payload: Element) -> Stanza {
    header(id).assemble(IqPayload::Get(payload)).into()
}

pub fn iq_set(id: &str, payload: Element) -> Stanza {
    header(id).assemble(IqPayload::Set(payload)).into()
}

pub fn roster_query() -> Element {
    Element::builder("query", NS_ROSTER).build()
}

pub fn carbons_enable() -> Element {
    Element::builder("enable", NS_CARBONS).build()
}

/// Letzte `max` Nachrichten mit `peer` aus dem Serverarchiv (XEP-0136).
pub fn archive_retrieve(peer: &str, max: u32) -> Element {
    let peer = peer
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;");
    format!(
        r#"<retrieve xmlns="{NS_ARCHIVE}" with="{peer}"><set xmlns="{NS_RSM}"><max>{max}</max><before/></set></retrieve>"#
    )
    .parse()
    .expect("gültiges XML")
}

pub fn roster_items(query: &Element) -> Vec<Contact> {
    query
        .children()
        .filter(|c| c.is("item", NS_ROSTER))
        .filter_map(|c| {
            let jid = c.attr("jid")?.to_owned();
            let name = c.attr("name").filter(|n| !n.trim().is_empty()).map_or_else(
                || jid.split('@').next().unwrap_or_default().to_owned(),
                str::to_owned,
            );
            Some(Contact {
                jid,
                name,
                show: "offline".into(),
                status: String::new(),
            })
        })
        .collect()
}

fn parse_time(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.timestamp_millis())
}

/// Antwort auf [`archive_retrieve`]: `<chat with start><from secs><body/></from>…`
pub fn parse_archive(chat: &Element, peer: &str) -> Vec<ChatMessage> {
    if !chat.is("chat", NS_ARCHIVE) {
        return Vec::new();
    }
    let start = chat.attr("start").and_then(parse_time).unwrap_or(0);
    chat.children()
        .filter_map(|c| {
            let outgoing = match c.name() {
                "to" => true,
                "from" => false,
                _ => return None,
            };
            let body = c.get_child("body", NS_ARCHIVE)?.text();
            let ts = c
                .attr("utc")
                .and_then(parse_time)
                .or_else(|| {
                    c.attr("secs")
                        .and_then(|s| s.parse::<i64>().ok())
                        .map(|s| start + s * 1000)
                })
                .unwrap_or(start);
            Some(ChatMessage {
                id: format!("a{ts}{}", u8::from(outgoing)),
                peer: peer.to_owned(),
                outgoing,
                body,
                ts,
            })
        })
        .collect()
}

/// Weitergeleitete Nachricht aus einem Carbon; `true` heisst gesendet.
pub fn carbon(m: &Message) -> Option<(Forwarded, bool)> {
    for p in &m.payloads {
        if p.is("sent", NS_CARBONS) {
            return Sent::try_from(p.clone()).ok().map(|s| (s.forwarded, true));
        }
        if p.is("received", NS_CARBONS) {
            return Received::try_from(p.clone())
                .ok()
                .map(|r| (r.forwarded, false));
        }
    }
    None
}

pub fn delay_of(m: &Message) -> Option<Delay> {
    m.payloads
        .iter()
        .find(|p| p.is("delay", NS_DELAY))
        .and_then(|p| Delay::try_from(p.clone()).ok())
}

pub fn chat_message(
    m: &Message,
    delay: Option<&Delay>,
    own: &str,
    sent: bool,
) -> Option<ChatMessage> {
    let (_, body) = m.get_best_body_cloned(vec!["de", "en"])?;
    if body.trim().is_empty() {
        return None;
    }
    let from = m.from.as_ref().map(|j| j.to_bare().to_string());
    let outgoing = sent || from.as_deref() == Some(own);
    let peer = if outgoing {
        m.to.as_ref().map(|j| j.to_bare().to_string())?
    } else {
        from?
    };
    let ts = delay.map_or_else(crate::now_ms, |d| d.stamp.0.timestamp_millis());
    Some(ChatMessage {
        id: m
            .id
            .as_ref()
            .map_or_else(|| format!("m{ts}{}", u8::from(outgoing)), |i| i.0.clone()),
        peer,
        outgoing,
        body,
        ts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_request_escapes_peer() {
        let el = archive_retrieve("a\"b@pbx", 50);
        assert_eq!(el.attr("with"), Some("a\"b@pbx"));
        assert!(el.get_child("set", NS_RSM).is_some());
    }

    #[test]
    fn archive_reply_is_parsed() {
        let xml = r#"<chat xmlns="urn:xmpp:archive" with="b@pbx" start="2026-09-30T10:00:00.000Z">
            <from secs="0"><body>Hallo</body></from>
            <to secs="5"><body>Hoi</body></to>
            <note>x</note>
        </chat>"#;
        let el: Element = xml.parse().unwrap();
        let msgs = parse_archive(&el, "b@pbx");
        assert_eq!(msgs.len(), 2);
        assert!(!msgs[0].outgoing);
        assert_eq!(msgs[0].body, "Hallo");
        assert!(msgs[1].outgoing);
        assert_eq!(msgs[1].ts - msgs[0].ts, 5000);
    }

    #[test]
    fn roster_items_get_names() {
        let xml = r#"<query xmlns="jabber:iq:roster">
            <item jid="1001@pbx" name="Claude Star"/>
            <item jid="1002@pbx"/>
        </query>"#;
        let el: Element = xml.parse().unwrap();
        let items = roster_items(&el);
        assert_eq!(items[0].name, "Claude Star");
        assert_eq!(items[1].name, "1002");
    }

    #[test]
    fn carbon_sent_message_is_outgoing() {
        let xml = r#"<message xmlns="jabber:client" from="a@pbx" to="a@pbx/linux">
            <sent xmlns="urn:xmpp:carbons:2">
              <forwarded xmlns="urn:xmpp:forward:0">
                <message xmlns="jabber:client" from="a@pbx/win" to="b@pbx" type="chat" id="x1"><body>Von Windows</body></message>
              </forwarded>
            </sent>
        </message>"#;
        let el: Element = xml.parse().unwrap();
        let m = Message::try_from(el).unwrap();
        let (fwd, sent) = carbon(&m).unwrap();
        assert!(sent);
        let msg = chat_message(&fwd.message, fwd.delay.as_ref(), "a@pbx", sent).unwrap();
        assert!(msg.outgoing);
        assert_eq!(msg.peer, "b@pbx");
        assert_eq!(msg.id, "x1");
    }
}
