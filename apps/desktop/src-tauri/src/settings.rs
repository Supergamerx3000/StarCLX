//! Lokale Einstellungen als JSON im Konfigurationsordner. Geheimnisse liegen
//! nie hier, sondern im Schlüsselbund.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    pub last_server: Option<String>,
    #[serde(default)]
    pub prefs: Prefs,
    /// Vom Benutzer bestätigte Zertifikate (SHA-256) je Anlage
    #[serde(default)]
    pub trusted_certs: BTreeMap<String, BTreeSet<String>>,
}

/// Benutzereinstellungen der Oberfläche, aufgebaut wie im Windows-Client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Softphone verwenden
    pub softphone: bool,
    /// Softphone bei der Anmeldung als primäres Telefon auswählen
    pub primary_on_login: bool,
    /// Bei Rufannahme das Softphone als primäres Telefon auswählen
    pub primary_on_answer: bool,
    /// Benachrichtigung über verpasste Anrufe (ohne Gruppenanrufe)
    pub notify_missed: bool,
    /// Benachrichtigung bei verpassten Gruppenanrufen
    pub notify_missed_group: bool,
    /// Bevorzugte Geräte in Reihenfolge (PipeWire-/Pulse-Namen). Leer heisst
    /// Systemstandard.
    pub speakers: Vec<String>,
    pub microphones: Vec<String>,
    pub ring_devices: Vec<String>,
    pub ringtone: bool,
    pub ringtone_internal: String,
    pub ringtone_external: String,
    /// Eigene Klingeltöne (WAV-Dateien)
    pub custom_ringtones: Vec<String>,
    /// Beim Empfang eines Anrufs die App in den Vordergrund bringen
    pub bring_to_front: bool,
    pub busylight: bool,
    pub busylight_sound: String,
    /// 0 bis 100
    pub busylight_volume: u8,
    /// Chat: Desktop-Benachrichtigung und Ton bei neuer Nachricht
    pub chat_notify: bool,
    pub chat_sound: bool,
    /// Ordner für empfangene Dateien; leer heisst Downloads
    pub download_dir: String,
    /// Chat-Status automatisch auf Abwesend
    pub away_on_idle: bool,
    pub away_on_screensaver: bool,
    pub away_on_lock: bool,
    /// Statustext bei Abwesenheit bzw. beim Abmelden
    pub away_text: String,
    pub offline_text: String,
    /// Erscheinungsbild: "system", "dark" oder "light"
    pub theme: String,
    /// Sprache der Oberfläche (bisher nur "de")
    pub language: String,
    pub start_minimized: bool,
    /// Beim Anmelden am Rechner starten (XDG-Autostart)
    pub autostart: bool,
    /// tel:-, callto:- und sip:-Links mit StarCLX öffnen
    pub handle_tel_links: bool,
    /// URL oder Programm bei Anruf
    pub call_actions: Vec<crate::plugins::callactions::CallAction>,
    /// Landesvorwahl ohne "+" für die Umrechnung nationaler Nummern
    pub default_country_code: String,
    /// Beim Minimieren nur noch im Tray anzeigen
    pub minimize_to_tray: bool,
    pub always_on_top: bool,
    pub hotkeys: crate::desktop::Hotkeys,
    /// Spalten im Funktionstasten-Raster (lokal, wie in Windows)
    pub fkey_columns: u8,
    /// Arbeitsbereich: "tabs" (Reiter) oder "free" (frei angeordnete Kacheln)
    pub workspace: String,
    /// Lage der Kacheln im freien Arbeitsbereich; gehört der Oberfläche
    pub workspace_tiles: serde_json::Value,
    /// Ausführliches Protokoll (Anruf- und Verbindungsdetails)
    pub verbose_log: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            softphone: true,
            primary_on_login: false,
            primary_on_answer: false,
            notify_missed: true,
            notify_missed_group: true,
            speakers: Vec::new(),
            microphones: Vec::new(),
            ring_devices: Vec::new(),
            ringtone: true,
            ringtone_internal: "Klassisch".into(),
            ringtone_external: "Klassisch".into(),
            custom_ringtones: Vec::new(),
            bring_to_front: true,
            busylight: false,
            busylight_sound: String::new(),
            busylight_volume: 50,
            chat_notify: true,
            chat_sound: true,
            download_dir: String::new(),
            away_on_idle: true,
            away_on_screensaver: true,
            away_on_lock: true,
            away_text: String::new(),
            offline_text: String::new(),
            theme: "system".into(),
            language: "de".into(),
            start_minimized: false,
            autostart: false,
            handle_tel_links: true,
            call_actions: Vec::new(),
            default_country_code: "41".into(),
            minimize_to_tray: false,
            always_on_top: false,
            hotkeys: crate::desktop::Hotkeys::default(),
            fkey_columns: 3,
            workspace: "tabs".into(),
            workspace_tiles: serde_json::Value::Null,
            verbose_log: false,
        }
    }
}

impl Prefs {
    /// Änderungen, die einen Neustart des Softphones brauchen
    pub fn softphone_changed(&self, other: &Prefs) -> bool {
        self.softphone != other.softphone
            || self.speakers != other.speakers
            || self.microphones != other.microphones
    }
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("settings.json"))
}

pub fn load(app: &AppHandle) -> Settings {
    settings_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &Settings) {
    let Some(path) = settings_path(app) else {
        return;
    };
    let result = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| {
            std::fs::write(
                &path,
                serde_json::to_vec_pretty(settings).unwrap_or_default(),
            )
        });
    if let Err(e) = result {
        tracing::warn!(error = %e, "Einstellungen nicht gespeichert");
    }
}

/// Liest, ändert und speichert die Einstellungen in einem Schritt.
pub fn update(app: &AppHandle, f: impl FnOnce(&mut Settings)) {
    let mut s = load(app);
    f(&mut s);
    save(app, &s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_file_gets_default_prefs() {
        let s: Settings = serde_json::from_str(r#"{"last_server":"https://pbx"}"#).unwrap();
        assert_eq!(s.last_server.as_deref(), Some("https://pbx"));
        assert_eq!(s.prefs, Prefs::default());
    }

    #[test]
    fn partial_prefs_keep_other_defaults() {
        let s: Settings = serde_json::from_str(r#"{"prefs":{"ringtone":false}}"#).unwrap();
        assert!(!s.prefs.ringtone);
        assert!(s.prefs.softphone);
    }
}
