//! XML-Bausteine, die xmpp-parsers nicht fertig mitbringt.

use tokio_xmpp::Stanza;
use tokio_xmpp::minidom::Element;
use tokio_xmpp::parsers::carbons::{Received, Sent};
use tokio_xmpp::parsers::delay::Delay;
use tokio_xmpp::parsers::forwarding::Forwarded;
use tokio_xmpp::parsers::iq::{IqHeader, IqPayload};
use tokio_xmpp::parsers::message::Message;
use tokio_xmpp::parsers::presence::Presence;

use crate::{ChatMessage, Contact};

const NS_ROSTER: &str = "jabber:iq:roster";
const NS_CARBONS: &str = "urn:xmpp:carbons:2";
const NS_DELAY: &str = "urn:xmpp:delay";
const NS_ARCHIVE: &str = "urn:xmpp:archive";
const NS_RSM: &str = "http://jabber.org/protocol/rsm";
pub const NS_DISCO_INFO: &str = "http://jabber.org/protocol/disco#info";
const NS_DISCO_ITEMS: &str = "http://jabber.org/protocol/disco#items";
const NS_CAPS: &str = "http://jabber.org/protocol/caps";

/// Attributname für den Element-Builder
pub fn n(name: &'static str) -> tokio_xmpp::minidom::rxml::NcName {
    name.try_into().expect("gültiger Attributname")
}

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

/// IQ-Abfrage an eine bestimmte Adresse, z. B. einen Dienst der Anlage
pub fn iq_get_to(id: &str, to: &str, payload: Element) -> Option<Stanza> {
    let mut h = header(id);
    h.to = Some(to.parse().ok()?);
    Some(h.assemble(IqPayload::Get(payload)).into())
}

/// Service Discovery (XEP-0030): Was kann diese Adresse?
pub fn disco_info() -> Element {
    Element::builder("query", NS_DISCO_INFO).build()
}

/// Service Discovery (XEP-0030): Welche Dienste gibt es unter dieser Adresse?
pub fn disco_items() -> Element {
    Element::builder("query", NS_DISCO_ITEMS).build()
}

/// Antwort auf `disco#info` an uns: StarCLX mit diesen Features
pub fn own_disco_info(features: &[&str]) -> Element {
    let mut b = Element::builder("query", NS_DISCO_INFO).append(
        Element::builder("identity", NS_DISCO_INFO)
            .attr(n("category"), "client")
            .attr(n("type"), "pc")
            .attr(n("name"), "StarCLX")
            .build(),
    );
    for f in features {
        b = b.append(
            Element::builder("feature", NS_DISCO_INFO)
                .attr(n("var"), *f)
                .build(),
        );
    }
    b.build()
}

const NS_VERSION: &str = "jabber:iq:version";

/// Name und Version eines Clients abfragen (XEP-0092)
pub fn version_query() -> Element {
    Element::builder("query", NS_VERSION).build()
}

/// Antwort auf [`version_query`] an uns
pub fn own_version(version: &str) -> Element {
    Element::builder("query", NS_VERSION)
        .append(
            Element::builder("name", NS_VERSION)
                .append("StarCLX")
                .build(),
        )
        .append(
            Element::builder("version", NS_VERSION)
                .append(version)
                .build(),
        )
        .build()
}

/// „Name Version“ aus der Antwort auf [`version_query`]
pub fn version_of(query: &Element) -> Option<String> {
    if !query.is("query", NS_VERSION) {
        return None;
    }
    let field = |n: &str| {
        query
            .get_child(n, NS_VERSION)
            .map(|e| e.text().trim().to_owned())
            .filter(|t| !t.is_empty())
    };
    let name = field("name")?;
    Some(match field("version") {
        Some(v) => format!("{name} {v}"),
        None => name,
    })
}

/// Name eines STARFACE-Clients aus seiner Ressource, z. B.
/// „StarfaceWindows-v9.0.2.7-…“ → „STARFACE Windows 9.0.2.7“. Die Apps melden
/// sich per Version und Identität alle nur als „Smack“, die Ressource verrät
/// dagegen Plattform und Version.
pub fn starface_client(resource: &str) -> Option<String> {
    let rest = resource
        .get(..8)
        .filter(|p| p.eq_ignore_ascii_case("starface"))
        .map(|_| &resource[8..])?;
    let mut parts = rest.split('-');
    let word = parts.next()?;
    let word = word.strip_suffix("Client").unwrap_or(word);
    let platform = match word.to_ascii_lowercase().as_str() {
        "" => return None,
        "android" => "Android",
        "windows" | "win" => "Windows",
        "ios" | "iphone" | "ipad" => "iPhone",
        "mac" | "macos" | "osx" => "macOS",
        _ => word,
    };
    let version = parts.find_map(|p| {
        p.strip_prefix(['v', 'V'])
            .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()))
    });
    Some(match version {
        Some(v) => format!("STARFACE {platform} {v}"),
        None => format!("STARFACE {platform}"),
    })
}

/// Name der ersten Identität aus einer `disco#info`-Antwort
pub fn identity_name(query: &Element) -> Option<String> {
    query
        .children()
        .filter(|c| c.is("identity", NS_DISCO_INFO))
        .find_map(|c| c.attr("name").filter(|n| !n.is_empty()).map(str::to_owned))
}

/// Adressen der Dienste aus einer `disco#items`-Antwort
pub fn disco_item_jids(query: &Element) -> Vec<String> {
    query
        .children()
        .filter(|c| c.is("item", NS_DISCO_ITEMS))
        .filter_map(|c| c.attr("jid").map(str::to_owned))
        .collect()
}

/// Client-Kennung aus der Präsenz (XEP-0115) als „Node#Ver“
pub fn caps_of(p: &Presence) -> Option<String> {
    let c = p.payloads.iter().find(|e| e.is("c", NS_CAPS))?;
    Some(format!(
        "{}#{}",
        c.attr("node").unwrap_or_default(),
        c.attr("ver").unwrap_or_default()
    ))
}

/// Identitäten und Features aus einer `disco#info`-Antwort, als
/// („Kategorie/Typ Name“, Feature-Namensräume)
pub fn disco_info_summary(query: &Element) -> (Vec<String>, Vec<String>) {
    let identities = query
        .children()
        .filter(|c| c.is("identity", NS_DISCO_INFO))
        .map(|c| {
            let kind = format!(
                "{}/{}",
                c.attr("category").unwrap_or_default(),
                c.attr("type").unwrap_or_default()
            );
            match c.attr("name").filter(|n| !n.is_empty()) {
                Some(name) => format!("{kind} \"{name}\""),
                None => kind,
            }
        })
        .collect();
    let features = query
        .children()
        .filter(|c| c.is("feature", NS_DISCO_INFO))
        .filter_map(|c| c.attr("var").map(str::to_owned))
        .collect();
    (identities, features)
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
                clients: Vec::new(),
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
    #[test]
    fn starface_client_from_resource() {
        assert_eq!(
            starface_client("StarfaceWindows-v9.0.2.7-uspitzer-PA-PF423RH0").as_deref(),
            Some("STARFACE Windows 9.0.2.7")
        );
        assert_eq!(
            starface_client("StarfaceAndroidClient-0c1afba2088b6660").as_deref(),
            Some("STARFACE Android")
        );
        assert_eq!(starface_client("f3891580"), None);
        assert_eq!(starface_client("Starface"), None);
    }

    use super::*;

    #[test]
    fn disco_items_and_info_are_parsed() {
        let items: Element = r#"<query xmlns="http://jabber.org/protocol/disco#items">
            <item jid="proxy.pbx.test" name="Socks 5 Bytestreams Proxy"/>
            <item jid="httpfileupload.pbx.test"/>
        </query>"#
            .parse()
            .unwrap();
        assert_eq!(
            disco_item_jids(&items),
            ["proxy.pbx.test", "httpfileupload.pbx.test"]
        );
        let info: Element = r#"<query xmlns="http://jabber.org/protocol/disco#info">
            <identity category="store" type="file" name="HTTP File Upload"/>
            <identity category="proxy" type="bytestreams"/>
            <feature var="urn:xmpp:http:upload:0"/>
            <feature var="http://jabber.org/protocol/bytestreams"/>
        </query>"#
            .parse()
            .unwrap();
        let (identities, features) = disco_info_summary(&info);
        assert_eq!(
            identities,
            ["store/file \"HTTP File Upload\"", "proxy/bytestreams"]
        );
        assert_eq!(
            features,
            [
                "urn:xmpp:http:upload:0",
                "http://jabber.org/protocol/bytestreams"
            ]
        );
    }

    #[test]
    fn caps_are_read_from_presence() {
        let p: Element = r#"<presence xmlns="jabber:client" from="bob@pbx.test/win">
            <c xmlns="http://jabber.org/protocol/caps" hash="sha-1" node="http://www.igniterealtime.org/projects/smack" ver="abc="/>
        </presence>"#
            .parse()
            .unwrap();
        let p = Presence::try_from(p).unwrap();
        assert_eq!(
            caps_of(&p).as_deref(),
            Some("http://www.igniterealtime.org/projects/smack#abc=")
        );
    }

    #[test]
    fn disco_request_goes_to_service() {
        let Stanza::Iq(iq) = iq_get_to("d1", "proxy.pbx.test", disco_info()).unwrap() else {
            panic!("kein IQ");
        };
        assert_eq!(iq.to().unwrap().to_string(), "proxy.pbx.test");
    }

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
