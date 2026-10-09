//! Lokaler Chatverlauf als JSON-Datei (nur für den angemeldeten Benutzer
//! lesbar).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::ChatMessage;

/// So viele Nachrichten bleiben je Gespräch erhalten.
const KEEP: usize = 1000;
/// Gleiche Nachricht aus Archiv und lokal: Zeitstempel weichen so weit ab.
/// In Gruppenchats zählt auch der Absender: „ok“ von zwei Leuten sind zwei
/// Nachrichten.
const SAME_WINDOW_MS: i64 = 120_000;

#[derive(Clone)]
pub struct History(Arc<Mutex<Inner>>);

struct Inner {
    path: Option<PathBuf>,
    map: BTreeMap<String, Vec<ChatMessage>>,
}

impl History {
    pub fn open(path: Option<PathBuf>) -> Self {
        let map = path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self(Arc::new(Mutex::new(Inner { path, map })))
    }

    pub fn get(&self, peer: &str) -> Vec<ChatMessage> {
        self.0
            .lock()
            .unwrap()
            .map
            .get(peer)
            .cloned()
            .unwrap_or_default()
    }

    pub fn recent(&self) -> Vec<ChatMessage> {
        let mut last: Vec<_> = self
            .0
            .lock()
            .unwrap()
            .map
            .values()
            .filter_map(|v| v.last().cloned())
            .collect();
        last.sort_by_key(|m| std::cmp::Reverse(m.ts));
        last
    }

    /// Fügt eine Nachricht hinzu. `false`, wenn sie schon bekannt war.
    pub fn add(&self, msg: ChatMessage) -> bool {
        let changed = {
            let mut inner = self.0.lock().unwrap();
            insert(inner.map.entry(msg.peer.clone()).or_default(), msg)
        };
        if changed {
            self.save();
        }
        changed
    }

    /// Übernimmt Nachrichten (z. B. aus dem Archiv). `true` bei Änderung.
    pub fn merge(&self, peer: &str, msgs: Vec<ChatMessage>) -> bool {
        let changed = {
            let mut inner = self.0.lock().unwrap();
            let list = inner.map.entry(peer.to_owned()).or_default();
            msgs.into_iter().fold(false, |acc, m| insert(list, m) | acc)
        };
        if changed {
            self.save();
        }
        changed
    }

    fn save(&self) {
        let inner = self.0.lock().unwrap();
        let Some(path) = &inner.path else {
            return;
        };
        let data = serde_json::to_vec(&inner.map).unwrap_or_default();
        if let Err(e) = write_private(path, &data) {
            tracing::warn!(error = %e, "Chatverlauf nicht gespeichert");
        }
    }
}

fn insert(list: &mut Vec<ChatMessage>, msg: ChatMessage) -> bool {
    let dup = list.iter().any(|m| {
        m.id == msg.id
            || (m.outgoing == msg.outgoing
                && m.sender == msg.sender
                && m.body == msg.body
                && (m.ts - msg.ts).abs() < SAME_WINDOW_MS)
    });
    if dup {
        return false;
    }
    let pos = list.partition_point(|m| m.ts <= msg.ts);
    list.insert(pos, msg);
    if list.len() > KEEP {
        list.drain(..list.len() - KEEP);
    }
    true
}

fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(&tmp)?.write_all(data)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str, out: bool, body: &str, ts: i64) -> ChatMessage {
        ChatMessage {
            id: id.into(),
            peer: "b@pbx".into(),
            outgoing: out,
            body: body.into(),
            ts,
            ..Default::default()
        }
    }

    #[test]
    fn dedupes_and_sorts() {
        let h = History::open(None);
        assert!(h.add(m("1", true, "Hoi", 10_000)));
        assert!(!h.add(m("1", true, "Hoi", 10_000)));
        // Gleiche Nachricht aus dem Archiv mit anderer ID und leicht anderer Zeit
        assert!(!h.merge("b@pbx", vec![m("a1", true, "Hoi", 11_000)]));
        assert!(h.merge("b@pbx", vec![m("a2", false, "Hallo", 5_000)]));
        let all = h.get("b@pbx");
        assert_eq!(
            all.iter().map(|m| m.body.as_str()).collect::<Vec<_>>(),
            ["Hallo", "Hoi"]
        );
        assert_eq!(h.recent()[0].body, "Hoi");
    }

    #[test]
    fn group_messages_from_different_senders_stay() {
        let h = History::open(None);
        let from = |id: &str, sender: &str| ChatMessage {
            sender: sender.into(),
            ..m(id, false, "ok", 20_000)
        };
        assert!(h.add(from("g1", "c@pbx")));
        assert!(h.add(from("g2", "d@pbx")));
        assert!(!h.add(from("g3", "d@pbx")));
    }
}
