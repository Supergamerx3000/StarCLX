//! Personalisierung und Desktop-Integration: Fensterverhalten,
//! Erscheinungsbild, Tastenkürzel, Rufnummern-Links und Autostart.
//!
//! Unter Wayland darf eine App keine globalen Tastenkürzel abfangen. Unter
//! GNOME werden die Kürzel deshalb als eigene Tastenkombinationen
//! eingetragen; diese starten `starclx --action …`, und single-instance
//! reicht die Aktion an die laufende App weiter. Andere Desktops (KDE …)
//! und das Flathub-Flatpak gehen über das GlobalShortcuts-Portal
//! ([`crate::shortcuts`]).

use std::path::PathBuf;

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
    /// Beim Desktop eintragen (sonst nur anzeigen)
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
    pub(crate) fn binding(&self, action: &str) -> &str {
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
    /// Kürzel über das GlobalShortcuts-Portal (KDE und andere)
    portal: bool,
    /// Befehl, den man in anderen Desktops selbst auf eine Taste legen kann
    command: String,
}

fn is_gnome() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|d| d.split(':').any(|p| p.eq_ignore_ascii_case("gnome")))
}

/// Wie die Kürzel beim Desktop landen
#[derive(Clone, Copy, PartialEq, Eq)]
enum Backend {
    /// Eigene Tastenkombinationen in den GNOME-Einstellungen
    Gnome,
    /// GlobalShortcuts-Portal
    Portal,
    None,
}

/// GNOME-Einstellungen, solange sie erreichbar sind (auch bei GNOME vor 48,
/// das noch kein Portal kennt); sonst das Portal, wo es eines gibt.
fn backend() -> Backend {
    if is_gnome() && !crate::flatpak::sandboxed() {
        Backend::Gnome
    } else if crate::shortcuts::available() {
        Backend::Portal
    } else {
        Backend::None
    }
}

fn is_wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Pfad zum Programm; beim AppImage das AppImage selbst, nicht der Mount
fn program() -> String {
    if let Some(id) = crate::flatpak::app_id() {
        return format!("flatpak run {id}");
    }
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
    if crate::flatpak::app_id().is_some() {
        return format!("{} --action {action}", program());
    }
    format!("\"{}\" --action {action}", program())
}

#[tauri::command]
pub fn desktop_info() -> DesktopInfo {
    let backend = backend();
    DesktopInfo {
        wayland: is_wayland(),
        gnome: backend == Backend::Gnome,
        portal: backend == Backend::Portal,
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

/// Rufnummern-Links, die StarCLX auf Wunsch öffnet
pub const TEL_SCHEMES: [&str; 3] = ["tel", "callto", "sip"];

/// Registriert starface-app:// immer, tel:/callto:/sip: nur mit `tel`. Läuft
/// im Hintergrund (xdg-mime ist langsam); Fehler landen nur im Log.
pub fn register_schemes(app: &AppHandle, tel: bool) {
    // Wichtig für AppImage und `tauri dev`; das .deb bringt eine eigene
    // .desktop-Datei mit. Fehlt z. B. xdg-mime, startet die App trotzdem.
    // Flatpak: die exportierte .desktop-Datei nennt die Schemata schon,
    // hier wird StarCLX nur als Standard eingetragen. Abmelden geht dort
    // nicht, ein anderes Programm wählt man in den Systemeinstellungen.
    if let Some(id) = crate::flatpak::app_id() {
        if crate::flatpak::sandboxed() {
            return;
        }
        std::thread::spawn(move || {
            let tel_schemes = if tel { &TEL_SCHEMES[..] } else { &[] };
            for scheme in ["starface-app"].iter().chain(tel_schemes) {
                let result = crate::flatpak::host_command("xdg-mime")
                    .args(["default", &format!("{id}.desktop")])
                    .arg(format!("x-scheme-handler/{scheme}"))
                    .status();
                if !result.is_ok_and(|s| s.success()) {
                    tracing::warn!(scheme, "Link-Schema nicht als Standard eingetragen");
                }
            }
        });
        return;
    }
    #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
    {
        let app = app.clone();
        std::thread::spawn(move || {
            use tauri_plugin_deep_link::DeepLinkExt;
            let dl = app.deep_link();
            if let Err(e) = dl.register("starface-app") {
                tracing::warn!(error = %e, "starface-app:// nicht registriert");
            }
            for scheme in TEL_SCHEMES {
                let result = if tel {
                    dl.register(scheme)
                } else if dl.is_registered(scheme).unwrap_or(false) {
                    dl.unregister(scheme)
                } else {
                    Ok(())
                };
                if let Err(e) = result {
                    tracing::warn!(error = %e, scheme, "Rufnummern-Link nicht (ab)gemeldet");
                }
            }
        });
    }
    #[cfg(not(any(target_os = "linux", all(debug_assertions, windows))))]
    let _ = (app, tel);
}

/// Ist das ein tel:-, callto:- oder sip:-Link?
pub fn is_tel_url(url: &str) -> bool {
    url.split_once(':')
        .is_some_and(|(s, _)| TEL_SCHEMES.iter().any(|t| t.eq_ignore_ascii_case(s)))
}

/// Rufnummer aus einem tel:/callto:/sip:-Link: ohne Schema und `//`, bis
/// `;`, `?` oder `@`, dekodiert, nur Ziffern, `+`, `*` und `#`.
pub fn number_from_url(url: &str) -> Option<String> {
    if !is_tel_url(url) {
        return None;
    }
    let rest = url.split_once(':')?.1.trim_start_matches('/');
    let end = rest.find([';', '?', '@']).unwrap_or(rest.len());
    let number: String = percent_decode(&rest[..end])
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '+' | '*' | '#'))
        .collect();
    number.chars().any(|c| c.is_ascii_digit()).then_some(number)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = (b[i] == b'%')
            .then(|| s.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(v) => {
                out.push(v);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `$XDG_CONFIG_HOME/autostart/starclx.desktop` (sonst ~/.config/autostart)
fn autostart_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("autostart").join("starclx.desktop"))
}

/// Pfad als Argument der Exec-Zeile: in Anführungszeichen, Sonderzeichen
/// maskiert, danach `\` für den Zeichenketten-Wert verdoppelt, `%` als `%%`.
fn exec_quote(path: &str) -> String {
    let mut q = String::from("\"");
    for c in path.chars().filter(|c| !c.is_control()) {
        match c {
            '"' | '`' | '$' => {
                q.push_str("\\\\");
                q.push(c);
            }
            '\\' => q.push_str("\\\\\\\\"),
            '%' => q.push_str("%%"),
            _ => q.push(c),
        }
    }
    q.push('"');
    q
}

fn autostart_entry(program: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=StarCLX\nExec={}\nIcon=starclx\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        exec_quote(program)
    )
}

/// Legt den Autostart-Eintrag an bzw. entfernt ihn. Beim Start erneut
/// aufgerufen, damit der Pfad nach einem Update stimmt. Als Flatpak macht
/// das das Background-Portal; ausgeschaltet wird es dort nur bei einer
/// Änderung gefragt, sonst käme die Rückfrage bei jedem Start.
pub fn apply_autostart(on: bool, changed: bool) -> Result<(), String> {
    if crate::flatpak::app_id().is_some() {
        return if on || changed {
            crate::flatpak::request_autostart(on)
        } else {
            Ok(())
        };
    }
    let Some(path) = autostart_file() else {
        return Ok(());
    };
    if on {
        let entry = autostart_entry(&program());
        if std::fs::read_to_string(&path).is_ok_and(|old| old == entry) {
            return Ok(());
        }
        path.parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, entry))
            .map_err(|e| e.to_string())
    } else {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

const MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
const KEYBINDING_DIR: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings";

fn gsettings(args: &[&str]) -> Result<String, String> {
    let out = crate::flatpak::host_command("gsettings")
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

/// Trägt die Tastenkürzel beim Desktop ein bzw. entfernt sie wieder.
pub fn apply_hotkeys(app: &AppHandle, hotkeys: &Hotkeys) -> Result<(), String> {
    match backend() {
        Backend::Gnome => apply_gnome(hotkeys),
        Backend::Portal => crate::shortcuts::apply(app, hotkeys),
        Backend::None if !hotkeys.enabled => Ok(()),
        Backend::None if crate::flatpak::sandboxed() => Err(t("Im Flatpak von Flathub lassen sich Tastenkürzel nicht automatisch eintragen. Lege den angezeigten Befehl in den Systemeinstellungen selbst auf eine Taste.").into()),
        Backend::None => Err(t("Dieser Desktop bietet keine globalen Tastenkürzel an. Lege den angezeigten Befehl in den Systemeinstellungen selbst auf eine Taste.").into()),
    }
}

/// Trägt die Tastenkürzel in GNOME ein bzw. entfernt sie wieder. Andere
/// Tastenkombinationen des Benutzers bleiben unangetastet.
fn apply_gnome(hotkeys: &Hotkeys) -> Result<(), String> {
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

    #[test]
    fn tel_numbers() {
        assert_eq!(
            number_from_url("tel:+41%2044%20123").as_deref(),
            Some("+4144123")
        );
        assert_eq!(
            number_from_url("callto://0041441234567").as_deref(),
            Some("0041441234567")
        );
        assert_eq!(
            number_from_url("sip:12@pbx.local;transport=tls").as_deref(),
            Some("12")
        );
        assert_eq!(number_from_url("tel:044-123?x").as_deref(), Some("044123"));
        assert_eq!(number_from_url("SIP:*21#").as_deref(), Some("*21#"));
        assert_eq!(number_from_url("tel:abc"), None);
        assert_eq!(number_from_url("starface-app://login?code=1"), None);
        assert_eq!(number_from_url("garbage"), None);
        assert!(is_tel_url("callto:x") && !is_tel_url("https://x"));
    }

    #[test]
    fn autostart_exec() {
        let e = autostart_entry("/opt/Star CLX/starclx");
        assert!(e.contains("Exec=\"/opt/Star CLX/starclx\"\n"));
        assert!(e.contains("X-GNOME-Autostart-enabled=true"));
        assert_eq!(exec_quote("/a$b%c"), r#""/a\\$b%%c""#);
        assert_eq!(exec_quote("/a\\b"), r#""/a\\\\b""#);
    }
}
