//! Personalisierung: Fensterverhalten, Erscheinungsbild und Tastenkürzel.
//!
//! Unter Wayland darf eine App keine globalen Tastenkürzel abfangen. Die
//! Kürzel werden deshalb als eigene Tastenkombinationen in GNOME eingetragen;
//! diese starten `starclx --action …`, und single-instance reicht
//! die Aktion an die laufende App weiter.

use std::process::Command;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Theme};

use crate::i18n::{t, tf};
use crate::settings::Prefs;

/// Aktionen, die per Tastenkürzel oder Kommandozeile ausgelöst werden
pub const ACTIONS: [(&str, &str); 5] = [
    ("dial-selection", "Markierte Rufnummer wählen"),
    ("dial-clipboard", "Rufnummer aus Zwischenablage wählen"),
    ("answer", "Softphone-Anruf annehmen"),
    ("hangup", "Aktuellen Anruf beenden"),
    ("toggle-view", "Ansicht umschalten"),
];

/// Tastenkürzel im GTK-Format, z. B. `<Control><Shift>w`; leer heisst keins.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    /// In GNOME eintragen (sonst nur anzeigen)
    pub enabled: bool,
    pub dial_selection: String,
    pub dial_clipboard: String,
    pub answer: String,
    pub hangup: String,
    pub toggle_view: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            enabled: false,
            dial_selection: "<Control><Shift>w".into(),
            dial_clipboard: String::new(),
            answer: "<Control><Shift>m".into(),
            hangup: "<Control><Shift>a".into(),
            toggle_view: String::new(),
        }
    }
}

impl Hotkeys {
    fn binding(&self, action: &str) -> &str {
        match action {
            "dial-selection" => &self.dial_selection,
            "dial-clipboard" => &self.dial_clipboard,
            "answer" => &self.answer,
            "hangup" => &self.hangup,
            "toggle-view" => &self.toggle_view,
            _ => "",
        }
    }
}

#[derive(Serialize)]
pub struct DesktopInfo {
    wayland: bool,
    gnome: bool,
    /// Befehl, den man in anderen Desktops selbst auf eine Taste legen kann
    command: String,
}

fn is_gnome() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|d| d.split(':').any(|p| p.eq_ignore_ascii_case("gnome")))
}

fn is_wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Pfad zum Programm; beim AppImage das AppImage selbst, nicht der Mount
fn program() -> String {
    std::env::var("APPIMAGE")
        .ok()
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "starclx".into())
}

fn command_for(action: &str) -> String {
    format!("\"{}\" --action {action}", program())
}

#[tauri::command]
pub fn desktop_info() -> DesktopInfo {
    DesktopInfo {
        wayland: is_wayland(),
        gnome: is_gnome(),
        command: command_for("<aktion>"),
    }
}

/// Wendet Fenster-Einstellungen an (beim Start und nach dem Speichern).
pub fn apply_window(app: &AppHandle, prefs: &Prefs) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    let _ = w.set_always_on_top(prefs.always_on_top);
    let _ = w.set_theme(match prefs.theme.as_str() {
        "dark" => Some(Theme::Dark),
        "light" => Some(Theme::Light),
        _ => None,
    });
}

/// Fenster beim Programmstart: sichtbar, minimiert oder nur im Tray.
pub fn show_on_start(app: &AppHandle, prefs: &Prefs) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    if !prefs.start_minimized {
        let _ = w.show();
    } else if !prefs.minimize_to_tray {
        let _ = w.show();
        let _ = w.minimize();
    }
}

/// Liest `--action <name>` aus der Kommandozeile.
pub fn action_from_args(args: &[String]) -> Option<&str> {
    let i = args.iter().position(|a| a == "--action")?;
    let action = args.get(i + 1)?.as_str();
    ACTIONS.iter().any(|(a, _)| *a == action).then_some(action)
}

#[derive(Clone, Serialize)]
struct Hotkey {
    action: String,
    /// Text aus Markierung bzw. Zwischenablage
    text: Option<String>,
}

/// Führt eine Aktion aus der Kommandozeile aus; die Oberfläche erledigt den Rest.
pub fn run_action(app: &AppHandle, action: &str) {
    let text = match action {
        "dial-selection" => read_text(true),
        "dial-clipboard" => read_text(false),
        _ => None,
    };
    if action == "toggle-view" {
        crate::show_main_window(app);
    }
    let _ = app.emit(
        "hotkey",
        Hotkey {
            action: action.into(),
            text,
        },
    );
}

fn read_text(primary: bool) -> Option<String> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let text = if primary {
        use arboard::{GetExtLinux, LinuxClipboardKind};
        cb.get().clipboard(LinuxClipboardKind::Primary).text()
    } else {
        cb.get_text()
    };
    text.ok().filter(|t| !t.trim().is_empty())
}

const MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
const KEYBINDING_DIR: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings";

fn gsettings(args: &[&str]) -> Result<String, String> {
    let out = Command::new("gsettings")
        .args(args)
        .output()
        .map_err(|e| tf("gsettings nicht ausführbar: {e}", &[("e", &e.to_string())]))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// GVariant-Zeichenkette
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Pfade aus einer GVariant-Liste wie `['/a/', '/b/']` oder `@as []`
fn parse_list(s: &str) -> Vec<String> {
    s.split('\'')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

fn own_path(action: &str) -> String {
    format!("{KEYBINDING_DIR}/starface-{action}/")
}

/// Neue Liste: fremde Einträge bleiben, eigene werden ersetzt.
fn merged_list(current: &[String], hotkeys: &Hotkeys) -> Vec<String> {
    let mut list: Vec<String> = current
        .iter()
        .filter(|p| !p.contains("/starface-"))
        .cloned()
        .collect();
    if hotkeys.enabled {
        list.extend(
            ACTIONS
                .iter()
                .filter(|(a, _)| !hotkeys.binding(a).is_empty())
                .map(|(a, _)| own_path(a)),
        );
    }
    list
}

/// Trägt die Tastenkürzel in GNOME ein bzw. entfernt sie wieder. Andere
/// Tastenkombinationen des Benutzers bleiben unangetastet.
pub fn apply_hotkeys(hotkeys: &Hotkeys) -> Result<(), String> {
    if !is_gnome() {
        return if hotkeys.enabled {
            Err(t("Tastenkürzel lassen sich nur unter GNOME automatisch eintragen.").into())
        } else {
            Ok(())
        };
    }
    let current = parse_list(&gsettings(&["get", MEDIA_KEYS, "custom-keybindings"])?);
    for (action, label) in ACTIONS {
        let schema = format!("{MEDIA_KEYS}.custom-keybinding:{}", own_path(action));
        let binding = hotkeys.binding(action);
        if hotkeys.enabled && !binding.is_empty() {
            gsettings(&[
                "set",
                &schema,
                "name",
                &quote(&format!("StarCLX: {}", t(label))),
            ])?;
            gsettings(&["set", &schema, "command", &quote(&command_for(action))])?;
            gsettings(&["set", &schema, "binding", &quote(binding)])?;
        } else if current.contains(&own_path(action)) {
            gsettings(&["reset", &schema, "binding"])?;
        }
    }
    let list = merged_list(&current, hotkeys);
    if list != current {
        let value = format!(
            "[{}]",
            list.iter().map(|p| quote(p)).collect::<Vec<_>>().join(", ")
        );
        gsettings(&["set", MEDIA_KEYS, "custom-keybindings", &value])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_args() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            action_from_args(&args(&["sf", "--action", "answer"])),
            Some("answer")
        );
        assert_eq!(action_from_args(&args(&["sf", "--action", "rm"])), None);
        assert_eq!(action_from_args(&args(&["sf"])), None);
    }

    #[test]
    fn keybinding_list() {
        assert!(parse_list("@as []").is_empty());
        let current = parse_list(&format!("['/x/custom0/', '{}']", own_path("toggle-view")));
        assert_eq!(current.len(), 2);
        let on = Hotkeys {
            enabled: true,
            ..Hotkeys::default()
        };
        assert_eq!(
            merged_list(&current, &on),
            vec![
                "/x/custom0/".to_owned(),
                own_path("dial-selection"),
                own_path("answer"),
                own_path("hangup"),
            ]
        );
        assert_eq!(
            merged_list(&current, &Hotkeys::default()),
            vec!["/x/custom0/".to_owned()]
        );
    }

    #[test]
    fn gvariant_quoting() {
        assert_eq!(quote("a'b"), r"'a\'b'");
        assert_eq!(
            quote(r#""/opt/x" --action answer"#),
            r#"'"/opt/x" --action answer'"#
        );
    }
}
