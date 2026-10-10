//! Lokale Einstellungen als JSON im Konfigurationsordner. Geheimnisse liegen
//! nie hier, sondern im Schlüsselbund.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    /// Anlage des zuletzt benutzten Kontos (füllt auch das Anmeldefeld vor)
    pub last_server: Option<String>,
    /// Benutzer-ID des zuletzt benutzten Kontos auf `last_server`; fehlt bei
    /// Einstellungen aus Versionen ohne Kontenliste
    #[serde(default)]
    pub last_user: Option<String>,
    /// Gespeicherte Konten in der Reihenfolge, in der sie dazukamen
    #[serde(default)]
    pub accounts: Vec<Account>,
    #[serde(default)]
    pub prefs: Prefs,
    /// Vom Benutzer bestätigte Zertifikate (SHA-256) je Anlage
    #[serde(default)]
    pub trusted_certs: BTreeMap<String, BTreeSet<String>>,
    /// Bestätigte SIP-Zertifikate (SHA-256) je Hostname der Anlage, für
    /// Zertifikate, die nicht nach den Systemzertifikaten gelten
    #[serde(default)]
    pub sip_certs: BTreeMap<String, String>,
    /// Aus Versionen ohne Kontenliste; zieht beim ersten Konto dorthin um
    #[serde(default, skip_serializing_if = "Option::is_none")]
    primary_before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fkey_redirects: Option<crate::plugins::fkeys::FkeyRedirects>,
}

/// Ein gespeichertes Konto: Anlage und Benutzer. Das Refresh-Token liegt im
/// Schlüsselbund (`sf_core::account_key`), nie hier.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub server: String,
    pub user_id: String,
    #[serde(default)]
    pub display_name: String,
    /// Primäres Telefon, bevor das Softphone es wurde; bekommt die Rolle
    /// beim Beenden zurück
    #[serde(default)]
    pub primary_before: Option<String>,
    /// Umleitungstasten mit Zielabfrage und die Einstellungen, die beim
    /// Ausschalten wiederkommen
    #[serde(default)]
    pub fkey_redirects: crate::plugins::fkeys::FkeyRedirects,
}

impl Settings {
    /// Das zuletzt benutzte Konto; während einer Sitzung das angemeldete
    pub fn active(&self) -> Option<&Account> {
        let (server, user) = (self.last_server.as_deref()?, self.last_user.as_deref()?);
        self.accounts
            .iter()
            .find(|a| a.server == server && a.user_id == user)
    }

    pub fn active_mut(&mut self) -> Option<&mut Account> {
        let server = self.last_server.clone()?;
        let user = self.last_user.clone()?;
        self.accounts
            .iter_mut()
            .find(|a| a.server == server && a.user_id == user)
    }

    /// Nach dem Anmelden: Konto aufnehmen bzw. auffrischen und als zuletzt
    /// benutzt merken. Das erste Konto übernimmt die Werte aus Versionen
    /// ohne Kontenliste.
    pub fn remember(&mut self, server: &str, user_id: &str, display_name: &str) {
        let first = self.accounts.is_empty();
        let pos = match self
            .accounts
            .iter()
            .position(|a| a.server == server && a.user_id == user_id)
        {
            Some(pos) => pos,
            None => {
                self.accounts.push(Account {
                    server: server.to_owned(),
                    user_id: user_id.to_owned(),
                    ..Default::default()
                });
                self.accounts.len() - 1
            }
        };
        let legacy = (self.primary_before.take(), self.fkey_redirects.take());
        let account = &mut self.accounts[pos];
        account.display_name = display_name.to_owned();
        if first {
            account.primary_before = account.primary_before.take().or(legacy.0);
            if let Some(r) = legacy.1 {
                account.fkey_redirects = r;
            }
        }
        self.last_server = Some(server.to_owned());
        self.last_user = Some(user_id.to_owned());
    }

    /// Konto aus der Liste nehmen
    pub fn forget(&mut self, server: &str, user_id: &str) {
        self.accounts
            .retain(|a| !(a.server == server && a.user_id == user_id));
        if self.last_server.as_deref() == Some(server) && self.last_user.as_deref() == Some(user_id)
        {
            self.last_user = None;
        }
    }
}

/// Ein gespeicherter eigener Status
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatPreset {
    /// "available", "away" oder "dnd"
    pub availability: String,
    pub text: String,
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
    /// Tasten, Klingeln und LEDs von USB-Headsets (Jabra, Poly, EPOS)
    pub headset: bool,
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
    /// Selbst gewählter Chat-Status: "available", "away" oder "dnd"
    pub chat_availability: String,
    /// Selbst gesetzter Statustext
    pub chat_text: String,
    /// Gespeicherte eigene Status (Symbol und Text), wie in der STARFACE-App
    pub chat_presets: Vec<ChatPreset>,
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
    /// Angelegte Türkameras (Name und URL), als Kachel anzeigbar
    pub door_cams: Vec<crate::plugins::doorcam::DoorCam>,
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
            headset: true,
            chat_notify: true,
            chat_sound: true,
            download_dir: String::new(),
            away_on_idle: true,
            away_on_screensaver: true,
            away_on_lock: true,
            away_text: String::new(),
            offline_text: String::new(),
            chat_availability: "available".into(),
            chat_text: String::new(),
            chat_presets: Vec::new(),
            theme: "system".into(),
            language: "de".into(),
            start_minimized: false,
            autostart: false,
            handle_tel_links: true,
            call_actions: Vec::new(),
            door_cams: Vec::new(),
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

/// Gespeicherte Einstellungen mit den Vorgaben des Systems (siehe
/// policy.rs): Startwerte für Fehlendes, gesperrte Werte immer.
pub fn load(app: &AppHandle) -> Settings {
    let raw: Option<serde_json::Value> = settings_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok());
    with_policy(raw, crate::policy::get())
}

fn with_policy(raw: Option<serde_json::Value>, policy: &crate::policy::Policy) -> Settings {
    let mut settings: Settings = raw
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    settings.prefs = policy.prefs(raw.as_ref().and_then(|v| v.get("prefs")));
    if let Some(server) = policy.server()
        && (settings.last_server.is_none() || policy.is_locked("server"))
    {
        settings.last_server = Some(server.to_string());
    }
    settings
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
    fn first_account_takes_over_old_values() {
        let mut s: Settings = serde_json::from_str(
            r#"{"last_server":"https://a","primary_before":"p1","fkey_redirects":{"ask":["7"]}}"#,
        )
        .unwrap();
        assert!(s.active().is_none());
        s.remember("https://a", "u1", "Anna");
        let a = s.active().unwrap();
        assert_eq!(a.primary_before.as_deref(), Some("p1"));
        assert!(a.fkey_redirects.ask.contains("7"));
        // Zweites Konto beginnt leer, das erste bleibt, wie es war
        s.remember("https://b", "u2", "Ben");
        assert_eq!(s.active().unwrap().primary_before, None);
        assert_eq!(s.accounts.len(), 2);
        s.remember("https://a", "u1", "Anna A.");
        assert_eq!(s.accounts.len(), 2);
        assert_eq!(s.active().unwrap().display_name, "Anna A.");
        assert_eq!(s.active().unwrap().primary_before.as_deref(), Some("p1"));
        // Alte Felder werden nicht mehr geschrieben
        let json = serde_json::to_value(&s).unwrap();
        assert!(json.get("primary_before").is_none());
        assert!(json.get("fkey_redirects").is_none());
        s.forget("https://a", "u1");
        assert!(s.active().is_none());
        assert_eq!(s.accounts.len(), 1);
    }

    #[test]
    fn partial_prefs_keep_other_defaults() {
        let s: Settings = serde_json::from_str(r#"{"prefs":{"ringtone":false}}"#).unwrap();
        assert!(!s.prefs.ringtone);
        assert!(s.prefs.softphone);
    }

    #[test]
    fn policy_applies() {
        let policy = crate::policy::Policy::from_files(
            ["[General]\nserver=https://pbx\nringtone[$i]=false\nautostart=true\n"].into_iter(),
        );
        let fresh = with_policy(None, &policy);
        assert_eq!(fresh.last_server.as_deref(), Some("https://pbx"));
        assert!(fresh.prefs.autostart);
        let raw = serde_json::json!({
            "last_server": "https://andere",
            "prefs": { "ringtone": true, "autostart": false }
        });
        let user = with_policy(Some(raw), &policy);
        assert_eq!(user.last_server.as_deref(), Some("https://andere"));
        assert!(!user.prefs.ringtone);
        assert!(!user.prefs.autostart);
    }
}
