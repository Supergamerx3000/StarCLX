//! Systemweite Vorgaben für den Rollout: `starclxrc` in den Ordnern von
//! `$XDG_CONFIG_DIRS` (ohne Angabe `/etc/xdg`), im Format der KDE-Konfigdateien.
//!
//! - `schluessel=wert` ist ein Startwert: gilt, bis der Benutzer die
//!   Einstellung selbst ändert.
//! - `schluessel[$i]=wert` sperrt die Einstellung (KDE-Kiosk); sie gilt immer
//!   und ist in der App ausgegraut. `[Gruppe][$i]` sperrt eine ganze Gruppe,
//!   `[$i]` vor der ersten Gruppe die ganze Datei.
//!
//! Die Schlüssel heissen wie die Felder von [`Prefs`], dazu `server` für die
//! Anlage. Gruppen sind frei wählbar, die Vorlage benutzt `[General]`. Der
//! erste Ordner in `$XDG_CONFIG_DIRS` hat Vorrang; gesperrte Werte kann ein
//! späterer Ordner nicht mehr ändern. Als Flatpak liest die App zusätzlich
//! `/run/host/etc/xdg` (sichtbar mit `--filesystem=host-etc:ro`).

use std::path::PathBuf;
use std::sync::OnceLock;

use serde_json::{Map, Value};

use crate::settings::Prefs;

pub const FILE_NAME: &str = "starclxrc";
/// Schlüssel für die Anlage (sonst Felder von `Prefs`)
const SERVER: &str = "server";

#[derive(Debug, Default, PartialEq)]
pub struct Policy {
    /// Alle Vorgaben, gesperrt oder nicht
    pub values: Map<String, Value>,
    /// Gesperrte Schlüssel
    pub locked: Vec<String>,
}

/// Vorgaben des Systems, einmal beim ersten Zugriff gelesen
pub fn get() -> &'static Policy {
    static POLICY: OnceLock<Policy> = OnceLock::new();
    POLICY.get_or_init(|| {
        let files: Vec<String> = config_dirs()
            .iter()
            .filter_map(|d| std::fs::read_to_string(d.join(FILE_NAME)).ok())
            .collect();
        let policy = Policy::from_files(files.iter().map(String::as_str));
        if !policy.values.is_empty() {
            tracing::info!(
                keys = policy.values.len(),
                locked = ?policy.locked,
                "Systemvorgaben gelesen"
            );
        }
        policy
    })
}

/// Ordner mit Vorgaben, wichtigster zuerst
fn config_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var("XDG_CONFIG_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/etc/xdg".into())
        .split(':')
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .collect();
    if crate::flatpak::app_id().is_some() {
        dirs.insert(0, PathBuf::from("/run/host/etc/xdg"));
    }
    dirs
}

impl Policy {
    /// Liest Dateiinhalte, wichtigste zuerst.
    pub fn from_files<'a>(files: impl DoubleEndedIterator<Item = &'a str>) -> Self {
        let defaults = serde_json::to_value(Prefs::default()).unwrap_or_default();
        let mut policy = Policy::default();
        // Von hinten nach vorn: wichtigere Dateien überschreiben, ausser die
        // Einstellung ist schon gesperrt.
        for content in files.rev() {
            for entry in parse(content) {
                if policy.locked.contains(&entry.key) {
                    continue;
                }
                let Some(value) = coerce(&defaults, &entry.key, &entry.value) else {
                    tracing::warn!(key = entry.key, "Unbekannte oder ungültige Vorgabe");
                    continue;
                };
                policy.values.insert(entry.key.clone(), value);
                if entry.locked {
                    policy.locked.push(entry.key);
                }
            }
        }
        policy
    }

    pub fn is_locked(&self, key: &str) -> bool {
        self.locked.iter().any(|k| k == key)
    }

    /// Vorgegebene oder gesperrte Anlage
    pub fn server(&self) -> Option<&str> {
        self.values.get(SERVER).and_then(Value::as_str)
    }

    /// Einstellungen aus Vorgaben, gespeicherten Werten des Benutzers
    /// (`stored`, auch unvollständig) und gesperrten Werten, in dieser
    /// Reihenfolge.
    pub fn prefs(&self, stored: Option<&Value>) -> Prefs {
        let mut value = serde_json::to_value(Prefs::default()).unwrap_or_default();
        for (key, v) in self.values.iter().filter(|(k, _)| *k != SERVER) {
            merge_key(&mut value, key, v);
        }
        if let Some(stored) = stored {
            merge(&mut value, stored);
        }
        match serde_json::from_value(value) {
            Ok(mut prefs) => {
                self.enforce(&mut prefs);
                prefs
            }
            // Ohne die gespeicherten Werte passen die Vorgaben (geprüft in coerce)
            Err(e) if stored.is_some() => {
                tracing::warn!(error = %e, "Gespeicherte Einstellungen ungültig");
                self.prefs(None)
            }
            Err(_) => Prefs::default(),
        }
    }

    /// Setzt die gesperrten Werte ein.
    pub fn enforce(&self, prefs: &mut Prefs) {
        if self.locked.iter().all(|k| k == SERVER) {
            return;
        }
        let Ok(mut value) = serde_json::to_value(&*prefs) else {
            return;
        };
        for key in self.locked.iter().filter(|k| *k != SERVER) {
            if let Some(v) = self.values.get(key) {
                merge_key(&mut value, key, v);
            }
        }
        if let Ok(p) = serde_json::from_value(value) {
            *prefs = p;
        }
    }
}

/// Ein Eintrag aus einer Konfigdatei
#[derive(Debug, PartialEq)]
struct Entry {
    key: String,
    value: String,
    locked: bool,
}

/// Liest eine Datei im KConfig-Format (Gruppen, Kommentare, `[$i]`).
/// Einträge mit Sprachangabe (`key[de]=`) gelten nicht.
fn parse(content: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut file_locked = false;
    let mut group_locked = false;
    let mut in_group = false;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && !line.contains('=') {
            if line == "[$i]" && !in_group {
                file_locked = true;
            } else {
                in_group = true;
                group_locked = line.ends_with("[$i]");
            }
            continue;
        }
        let Some((raw_key, value)) = line.split_once('=') else {
            continue;
        };
        let raw_key = raw_key.trim();
        let (key, options) = match raw_key.find('[') {
            Some(i) => (&raw_key[..i], &raw_key[i..]),
            None => (raw_key, ""),
        };
        let mut locked = file_locked || group_locked;
        let mut localized = false;
        for option in options
            .split(['[', ']'])
            .map(str::trim)
            .filter(|o| !o.is_empty())
        {
            match option.strip_prefix('$') {
                Some(flags) => locked |= flags.contains('i'),
                None => localized = true,
            }
        }
        if key.is_empty() || localized {
            continue;
        }
        entries.push(Entry {
            key: key.trim().to_string(),
            value: unescape(value.trim()),
            locked,
        });
    }
    entries
}

/// Ersatzzeichen von KConfig: `\n`, `\t`, `\s` (Leerzeichen), `\\`
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('s') => out.push(' '),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Wandelt den Text in den Typ der Einstellung. `None` bei unbekanntem
/// Schlüssel oder unpassendem Wert.
fn coerce(defaults: &Value, key: &str, text: &str) -> Option<Value> {
    if key == SERVER {
        return (!text.is_empty()).then(|| Value::String(text.to_string()));
    }
    let value = match defaults.get(key)? {
        Value::Bool(_) => match text.to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Value::Bool(true),
            "false" | "0" | "no" | "off" => Value::Bool(false),
            _ => return None,
        },
        Value::Number(_) => serde_json::from_str::<serde_json::Number>(text)
            .ok()
            .map(Value::Number)?,
        Value::String(_) => Value::String(text.to_string()),
        // Listen und Objekte als JSON
        _ => serde_json::from_str(text).ok()?,
    };
    // Passt der Wert in die Einstellungen?
    let mut test = defaults.clone();
    merge_key(&mut test, key, &value);
    serde_json::from_value::<Prefs>(test).ok().map(|_| value)
}

fn merge_key(target: &mut Value, key: &str, value: &Value) {
    if let Value::Object(map) = target {
        match map.get_mut(key) {
            Some(existing) => merge(existing, value),
            None => {
                map.insert(key.to_string(), value.clone());
            }
        }
    }
}

/// Objekte werden zusammengeführt, alles andere ersetzt.
fn merge(target: &mut Value, value: &Value) {
    match (target, value) {
        (Value::Object(t), Value::Object(v)) => {
            for (key, v) in v {
                match t.get_mut(key) {
                    Some(existing) => merge(existing, v),
                    None => {
                        t.insert(key.clone(), v.clone());
                    }
                }
            }
        }
        (t, v) => *t = v.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(files: &[&str]) -> Policy {
        Policy::from_files(files.iter().copied())
    }

    #[test]
    fn parses_kconfig() {
        let entries = parse(
            "# Kommentar\n[$i]\n[General]\nserver=https://pbx\nName[de]=x\naway_text=\\sBin weg\n",
        );
        assert_eq!(
            entries,
            vec![
                Entry {
                    key: "server".into(),
                    value: "https://pbx".into(),
                    locked: true
                },
                Entry {
                    key: "away_text".into(),
                    value: " Bin weg".into(),
                    locked: true
                },
            ]
        );
    }

    #[test]
    fn key_and_group_locks() {
        let p = policy(&[
            "[General]\nautostart[$i]=true\nringtone=false\n[Gesperrt][$i]\ntheme=dark\n",
        ]);
        assert_eq!(p.locked, vec!["autostart".to_string(), "theme".to_string()]);
        assert_eq!(p.values["ringtone"], Value::Bool(false));
    }

    #[test]
    fn types_follow_prefs() {
        let p = policy(&[
            "[General]\ndefault_country_code=49\nbusylight_volume=80\nsoftphone=no\nunbekannt=1\nfkey_columns=viele\n\
             door_cams=[{\"name\":\"Tor\",\"url\":\"rtsp://tor\"}]\nhotkeys={\"enabled\":true}\n",
        ]);
        let prefs = p.prefs(None);
        assert_eq!(prefs.default_country_code, "49");
        assert_eq!(prefs.busylight_volume, 80);
        assert!(!prefs.softphone);
        assert_eq!(prefs.fkey_columns, Prefs::default().fkey_columns);
        assert_eq!(prefs.door_cams[0].name, "Tor");
        assert!(prefs.hotkeys.enabled);
        assert_eq!(prefs.hotkeys.answer, Prefs::default().hotkeys.answer);
        assert!(!p.values.contains_key("unbekannt"));
    }

    #[test]
    fn user_beats_default_lock_beats_user() {
        let p = policy(&["[General]\nringtone=false\ntheme[$i]=dark\n"]);
        let stored = serde_json::json!({ "ringtone": true, "theme": "light" });
        let prefs = p.prefs(Some(&stored));
        assert!(prefs.ringtone);
        assert_eq!(prefs.theme, "dark");
        assert!(!p.prefs(None).ringtone);
    }

    #[test]
    fn first_dir_wins_unless_locked_later() {
        let p = policy(&[
            "[General]\ntheme=dark\nlanguage=en\n",
            "[General]\ntheme=light\nlanguage[$i]=de\n",
        ]);
        assert_eq!(p.values["theme"], "dark");
        assert_eq!(p.values["language"], "de");
        assert!(p.is_locked("language"));
    }

    #[test]
    fn server_lock() {
        let p = policy(&["[General]\nserver[$i]=https://pbx.firma.ch\n"]);
        assert_eq!(p.server(), Some("https://pbx.firma.ch"));
        assert!(p.is_locked("server"));
        assert_eq!(p.prefs(None), Prefs::default());
    }

    #[test]
    fn template_is_valid() {
        // Die Beispiele sind auskommentiert (`#schluessel=wert`).
        let template = include_str!("../../../../packaging/vorlagen/etc/xdg/starclxrc");
        let active: String = template
            .lines()
            .map(|l| match l.strip_prefix('#') {
                Some(rest)
                    if rest.split('=').next().is_some_and(|k| {
                        !k.is_empty()
                            && k.chars()
                                .all(|c| c.is_ascii_lowercase() || "_[$]".contains(c))
                    }) =>
                {
                    rest
                }
                _ => l,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let entries = parse(&active);
        assert!(entries.len() > 20);
        let p = policy(&[&active]);
        for e in &entries {
            assert!(p.values.contains_key(&e.key), "Vorlage: {} ungültig", e.key);
        }
    }
}
