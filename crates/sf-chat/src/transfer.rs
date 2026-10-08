//! Dateiübertragung wie im STARFACE-Windows-Client (Smack): Angebot per
//! Stream Initiation (XEP-0096), die Daten In-Band über den Chat-Server
//! (XEP-0047). Die Anlage hat keinen Proxy für direkte Verbindungen, darum
//! nur dieser Weg; Smack bietet ihn beim Senden mit an und nimmt ihn beim
//! Empfangen an.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;
use tokio_xmpp::minidom::Element;

use crate::xml::n;

pub const NS_SI: &str = "http://jabber.org/protocol/si";
pub const NS_FT: &str = "http://jabber.org/protocol/si/profile/file-transfer";
pub const NS_IBB: &str = "http://jabber.org/protocol/ibb";
const NS_FEATURE_NEG: &str = "http://jabber.org/protocol/feature-neg";
const NS_DATA: &str = "jabber:x:data";

/// Blockgröße beim Senden (vor Base64). Lehnt die Gegenseite ab, gilt
/// [`SMALL_BLOCK`], der Standard von Smack.
pub const BLOCK: usize = 16 * 1024;
pub const SMALL_BLOCK: usize = 4096;
/// Größter Block, den wir beim Empfang annehmen
const MAX_BLOCK: usize = 64 * 1024;
/// So viele Datenblöcke dürfen gleichzeitig unbestätigt unterwegs sein
const WINDOW: usize = 8;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferState {
    /// Eingehend: wartet auf Annehmen oder Ablehnen
    Offered,
    /// Ausgehend: wartet auf die Antwort der Gegenseite
    Waiting,
    Running,
    Done,
    Declined,
    Cancelled,
    Failed,
}

impl TransferState {
    pub fn active(self) -> bool {
        matches!(self, Self::Offered | Self::Waiting | Self::Running)
    }
}

/// Stand einer Übertragung, wie ihn die Oberfläche anzeigt
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Transfer {
    pub id: String,
    /// Gesprächspartner (bare JID)
    pub peer: String,
    pub outgoing: bool,
    pub name: String,
    pub size: u64,
    /// Übertragene Bytes
    pub done: u64,
    pub state: TransferState,
    /// Gespeicherte bzw. gesendete Datei
    pub path: String,
    pub error: String,
    /// Unix-Zeit in Millisekunden
    pub ts: i64,
}

/// Angebot einer Datei an `sid`
pub fn offer(sid: &str, name: &str, size: u64) -> Element {
    Element::builder("si", NS_SI)
        .attr(n("id"), sid)
        .attr(n("mime-type"), "application/octet-stream")
        .attr(n("profile"), NS_FT)
        .append(
            Element::builder("file", NS_FT)
                .attr(n("name"), name)
                .attr(n("size"), size.to_string())
                .build(),
        )
        .append(feature_neg(
            "form",
            Element::builder("field", NS_DATA)
                .attr(n("var"), "stream-method")
                .attr(n("type"), "list-single")
                .append(
                    Element::builder("option", NS_DATA)
                        .append(Element::builder("value", NS_DATA).append(NS_IBB).build())
                        .build(),
                )
                .build(),
        ))
        .build()
}

fn feature_neg(kind: &str, field: Element) -> Element {
    Element::builder("feature", NS_FEATURE_NEG)
        .append(
            Element::builder("x", NS_DATA)
                .attr(n("type"), kind)
                .append(field)
                .build(),
        )
        .build()
}

/// Ein empfangenes Angebot
#[derive(Debug, PartialEq, Eq)]
pub struct Offer {
    pub sid: String,
    pub name: String,
    pub size: u64,
    /// Die Gegenseite bietet In-Band an
    pub ibb: bool,
}

pub fn parse_offer(si: &Element) -> Option<Offer> {
    if !si.is("si", NS_SI) {
        return None;
    }
    let file = si.get_child("file", NS_FT)?;
    let ibb = stream_methods(si).any(|m| m == NS_IBB);
    Some(Offer {
        sid: si.attr("id")?.to_owned(),
        name: file.attr("name").unwrap_or_default().to_owned(),
        size: file.attr("size").and_then(|s| s.parse().ok()).unwrap_or(0),
        ibb,
    })
}

/// Werte des Felds `stream-method` (Optionen im Angebot, Wahl in der Antwort)
fn stream_methods(si: &Element) -> impl Iterator<Item = String> + '_ {
    si.get_child("feature", NS_FEATURE_NEG)
        .and_then(|f| f.get_child("x", NS_DATA))
        .into_iter()
        .flat_map(|x| x.children())
        .filter(|f| f.is("field", NS_DATA) && f.attr("var") == Some("stream-method"))
        .flat_map(|f| {
            f.children()
                .filter(|c| c.is("option", NS_DATA))
                .filter_map(|o| o.get_child("value", NS_DATA))
                .chain(f.children().filter(|c| c.is("value", NS_DATA)))
        })
        .map(|v| v.text())
}

/// Antwort auf ein Angebot: angenommen, Übertragung In-Band
pub fn accept() -> Element {
    Element::builder("si", NS_SI)
        .append(feature_neg(
            "submit",
            Element::builder("field", NS_DATA)
                .attr(n("var"), "stream-method")
                .append(Element::builder("value", NS_DATA).append(NS_IBB).build())
                .build(),
        ))
        .build()
}

/// Die Gegenseite hat In-Band gewählt
pub fn chose_ibb(si: &Element) -> bool {
    si.is("si", NS_SI) && stream_methods(si).any(|m| m == NS_IBB)
}

pub fn ibb_open(sid: &str, block: usize) -> Element {
    Element::builder("open", NS_IBB)
        .attr(n("sid"), sid)
        .attr(n("block-size"), block.to_string())
        .attr(n("stanza"), "iq")
        .build()
}

pub fn ibb_data(sid: &str, seq: u16, bytes: &[u8]) -> Element {
    Element::builder("data", NS_IBB)
        .attr(n("sid"), sid)
        .attr(n("seq"), seq.to_string())
        .append(BASE64.encode(bytes))
        .build()
}

pub fn ibb_close(sid: &str) -> Element {
    Element::builder("close", NS_IBB)
        .attr(n("sid"), sid)
        .build()
}

#[derive(Debug, PartialEq, Eq)]
pub enum Ibb {
    Open {
        sid: String,
        block: usize,
    },
    Data {
        sid: String,
        seq: u16,
        bytes: Vec<u8>,
    },
    Close {
        sid: String,
    },
}

/// IBB-Element; `None`, wenn es keins ist oder kaputt ist
pub fn parse_ibb(el: &Element) -> Option<Ibb> {
    if el.ns() != NS_IBB {
        return None;
    }
    let sid = el.attr("sid")?.to_owned();
    match el.name() {
        "open" => Some(Ibb::Open {
            sid,
            block: el.attr("block-size")?.parse().ok()?,
        }),
        "data" => {
            let text: String = el.text().split_whitespace().collect();
            Some(Ibb::Data {
                sid,
                seq: el.attr("seq")?.parse().ok()?,
                bytes: BASE64.decode(text).ok()?,
            })
        }
        "close" => Some(Ibb::Close { sid }),
        _ => None,
    }
}

/// Dateiname ohne Pfadanteile und Steuerzeichen
pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = base
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .trim_start_matches('.')
        .to_owned();
    if clean.is_empty() {
        "Datei".into()
    } else {
        clean
    }
}

/// Freier Pfad in `dir`: „name.pdf“, sonst „name (1).pdf“ usw.
pub fn free_path(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    if !path.exists() {
        return path;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("unendlich viele Namen")
}

/// Senden: liest die Datei blockweise und hält höchstens [`WINDOW`] Blöcke
/// unbestätigt.
pub struct Outgoing {
    pub t: Transfer,
    /// Volle JID der Gegenseite
    pub to: String,
    pub block: usize,
    file: File,
    seq: u16,
    in_flight: usize,
    eof: bool,
    reported: u64,
}

impl Outgoing {
    pub fn new(t: Transfer, to: String, file: File) -> Self {
        Self {
            t,
            to,
            block: BLOCK,
            file,
            seq: 0,
            in_flight: 0,
            eof: false,
            reported: 0,
        }
    }

    /// Nächste Datenblöcke als (IBB-Element, Länge)
    pub fn fill(&mut self) -> std::io::Result<Vec<(Element, usize)>> {
        let mut out = Vec::new();
        while !self.eof && self.in_flight < WINDOW {
            let mut buf = vec![0; self.block];
            let n = read_full(&mut self.file, &mut buf)?;
            if n == 0 {
                self.eof = true;
                break;
            }
            out.push((ibb_data(&self.t.id, self.seq, &buf[..n]), n));
            self.seq = self.seq.wrapping_add(1);
            self.in_flight += 1;
            if n < self.block {
                self.eof = true;
            }
        }
        Ok(out)
    }

    /// Ein Block wurde bestätigt; `true`, wenn die Oberfläche den Fortschritt
    /// erfahren soll.
    pub fn acked(&mut self, len: usize) -> bool {
        self.in_flight = self.in_flight.saturating_sub(1);
        self.t.done += len as u64;
        progress_due(&mut self.reported, self.t.done, self.t.size)
    }

    /// Alles gesendet und bestätigt
    pub fn finished(&self) -> bool {
        self.eof && self.in_flight == 0
    }
}

fn read_full(file: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match file.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// Fortschritt höchstens je Prozent melden
fn progress_due(reported: &mut u64, done: u64, size: u64) -> bool {
    let step = (size / 100).max(1);
    if done >= size || done >= *reported + step {
        *reported = done;
        true
    } else {
        false
    }
}

/// Empfangen
pub struct Incoming {
    pub t: Transfer,
    /// Volle JID der Gegenseite
    pub from: String,
    /// ID des Angebots, auf das noch geantwortet werden muss
    pub offer_iq: String,
    file: Option<File>,
    seq: u16,
    reported: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DataError {
    /// Block außer der Reihe oder ohne geöffnete Übertragung
    Order,
    Io(String),
}

impl Incoming {
    pub fn new(t: Transfer, from: String, offer_iq: String) -> Self {
        Self {
            t,
            from,
            offer_iq,
            file: None,
            seq: 0,
            reported: 0,
        }
    }

    /// Legt die Zieldatei an (beim Annehmen).
    pub fn create(&mut self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let path = free_path(dir, &self.t.name);
        self.file = Some(File::create(&path)?);
        self.t.path = path.to_string_lossy().into_owned();
        self.t.state = TransferState::Running;
        Ok(())
    }

    pub fn accepts_block(&self, block: usize) -> bool {
        self.file.is_some() && block <= MAX_BLOCK
    }

    /// Schreibt einen Block; `Ok(true)`, wenn die Oberfläche den Fortschritt
    /// erfahren soll.
    pub fn data(&mut self, seq: u16, bytes: &[u8]) -> Result<bool, DataError> {
        let file = self.file.as_mut().ok_or(DataError::Order)?;
        if seq != self.seq {
            return Err(DataError::Order);
        }
        file.write_all(bytes)
            .map_err(|e| DataError::Io(e.to_string()))?;
        self.seq = self.seq.wrapping_add(1);
        self.t.done += bytes.len() as u64;
        Ok(progress_due(&mut self.reported, self.t.done, self.t.size))
    }

    /// Gegenseite hat geschlossen: fertig, wenn alles da ist.
    pub fn close(&mut self) {
        if let Some(mut f) = self.file.take() {
            let _ = f.flush();
        }
        if self.t.state != TransferState::Running {
            return;
        }
        if self.t.size == 0 || self.t.done >= self.t.size {
            self.t.state = TransferState::Done;
        } else {
            self.fail("Übertragung unvollständig".into());
        }
    }

    /// Abbruch: angefangene Datei löschen
    pub fn fail(&mut self, error: String) {
        self.discard(TransferState::Failed);
        self.t.error = error;
    }

    pub fn discard(&mut self, state: TransferState) {
        let had_file = self.file.take().is_some() || self.t.state == TransferState::Running;
        if had_file && !self.t.path.is_empty() {
            let _ = std::fs::remove_file(&self.t.path);
            self.t.path.clear();
        }
        self.t.state = state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transfer(outgoing: bool) -> Transfer {
        Transfer {
            id: "s1".into(),
            peer: "bob@pbx.test".into(),
            outgoing,
            name: "a.txt".into(),
            size: 0,
            done: 0,
            state: TransferState::Waiting,
            path: String::new(),
            error: String::new(),
            ts: 0,
        }
    }

    #[test]
    fn smack_offer_is_parsed() {
        // So bietet Smack eine Datei an (beide Verfahren)
        let si: Element = r#"<si xmlns="http://jabber.org/protocol/si" id="jsi_123" mime-type="text/plain" profile="http://jabber.org/protocol/si/profile/file-transfer">
            <file xmlns="http://jabber.org/protocol/si/profile/file-transfer" name="Bericht.pdf" size="1022"><desc>x</desc></file>
            <feature xmlns="http://jabber.org/protocol/feature-neg">
              <x xmlns="jabber:x:data" type="form">
                <field var="stream-method" type="list-single">
                  <option><value>http://jabber.org/protocol/bytestreams</value></option>
                  <option><value>http://jabber.org/protocol/ibb</value></option>
                </field>
              </x>
            </feature>
        </si>"#
            .parse()
            .unwrap();
        assert_eq!(
            parse_offer(&si),
            Some(Offer {
                sid: "jsi_123".into(),
                name: "Bericht.pdf".into(),
                size: 1022,
                ibb: true,
            })
        );
    }

    #[test]
    fn own_offer_and_answer_round_trip() {
        let o = parse_offer(&offer("s1", "a b.txt", 5)).unwrap();
        assert_eq!((o.name.as_str(), o.size, o.ibb), ("a b.txt", 5, true));
        assert!(chose_ibb(&accept()));
        let bytestreams: Element = r#"<si xmlns="http://jabber.org/protocol/si"><feature xmlns="http://jabber.org/protocol/feature-neg"><x xmlns="jabber:x:data" type="submit"><field var="stream-method"><value>http://jabber.org/protocol/bytestreams</value></field></x></feature></si>"#
            .parse()
            .unwrap();
        assert!(!chose_ibb(&bytestreams));
    }

    #[test]
    fn ibb_round_trip() {
        assert_eq!(
            parse_ibb(&ibb_open("s1", 4096)),
            Some(Ibb::Open {
                sid: "s1".into(),
                block: 4096
            })
        );
        assert_eq!(
            parse_ibb(&ibb_data("s1", 7, b"hallo")),
            Some(Ibb::Data {
                sid: "s1".into(),
                seq: 7,
                bytes: b"hallo".to_vec()
            })
        );
        assert_eq!(
            parse_ibb(&ibb_close("s1")),
            Some(Ibb::Close { sid: "s1".into() })
        );
        // Zeilenumbrüche im Base64 sind erlaubt
        let el: Element =
            "<data xmlns='http://jabber.org/protocol/ibb' sid='s1' seq='0'>aGFs\nbG8=</data>"
                .parse()
                .unwrap();
        assert!(matches!(parse_ibb(&el), Some(Ibb::Data { bytes, .. }) if bytes == b"hallo"));
    }

    #[test]
    fn names_are_made_safe() {
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("C:\\Users\\x\\Bild.png"), "Bild.png");
        assert_eq!(safe_name(".bashrc"), "bashrc");
        assert_eq!(safe_name(""), "Datei");
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sf-chat-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn free_path_counts_up() {
        let dir = temp_dir("free");
        assert_eq!(free_path(&dir, "a.txt"), dir.join("a.txt"));
        std::fs::write(dir.join("a.txt"), "").unwrap();
        std::fs::write(dir.join("a (1).txt"), "").unwrap();
        assert_eq!(free_path(&dir, "a.txt"), dir.join("a (2).txt"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn file_goes_through_in_blocks() {
        let dir = temp_dir("blocks");
        let src = dir.join("src.bin");
        let data: Vec<u8> = (0..10_000u32).map(|i| i as u8).collect();
        std::fs::write(&src, &data).unwrap();

        let mut out = Outgoing::new(
            Transfer {
                size: data.len() as u64,
                ..transfer(true)
            },
            "bob@pbx.test/x".into(),
            File::open(&src).unwrap(),
        );
        out.block = 4096;
        let mut inc = Incoming::new(
            Transfer {
                size: data.len() as u64,
                state: TransferState::Offered,
                ..transfer(false)
            },
            "alice@pbx.test/y".into(),
            "iq1".into(),
        );
        inc.create(&dir.join("in")).unwrap();

        let blocks = out.fill().unwrap();
        assert_eq!(blocks.len(), 3);
        for (el, len) in blocks {
            let Some(Ibb::Data { seq, bytes, .. }) = parse_ibb(&el) else {
                panic!("kein Datenblock")
            };
            inc.data(seq, &bytes).unwrap();
            out.acked(len);
        }
        assert!(out.fill().unwrap().is_empty());
        assert!(out.finished());
        assert_eq!(out.t.done, 10_000);
        inc.close();
        assert_eq!(inc.t.state, TransferState::Done);
        assert_eq!(std::fs::read(&inc.t.path).unwrap(), data);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn out_of_order_block_is_rejected_and_partial_file_removed() {
        let dir = temp_dir("order");
        let mut inc = Incoming::new(
            Transfer {
                size: 10,
                state: TransferState::Offered,
                ..transfer(false)
            },
            "alice@pbx.test/y".into(),
            "iq1".into(),
        );
        inc.create(&dir).unwrap();
        assert_eq!(inc.data(1, b"x"), Err(DataError::Order));
        inc.data(0, b"abc").unwrap();
        let path = inc.t.path.clone();
        inc.close();
        assert_eq!(inc.t.state, TransferState::Failed);
        assert!(!Path::new(&path).exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
