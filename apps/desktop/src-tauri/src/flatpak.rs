//! Als Flatpak läuft die App in einer Sandbox: Programme des Systems
//! (gsettings, xdg-mime, Programme bei Anruf) sind nur über
//! `flatpak-spawn --host` erreichbar, der Autostart geht über das
//! Background-Portal. Ausserhalb von Flatpak ändert sich nichts.
//!
//! Das eigene Flatpak (packaging/flatpak) darf Programme des Systems
//! starten, das von Flathub (packaging/flathub) nicht.

use std::collections::HashMap;
use std::process::Command;

use zbus::zvariant::Value;

use crate::i18n::t;

/// Flatpak-ID, wenn die App als Flatpak läuft
pub fn app_id() -> Option<String> {
    std::env::var("FLATPAK_ID").ok().filter(|id| !id.is_empty())
}

/// Flatpak ohne Zugriff auf Programme des Systems (Flathub)
pub fn sandboxed() -> bool {
    app_id().is_some()
        && !std::fs::read_to_string("/.flatpak-info").is_ok_and(|info| host_access(&info))
}

/// Darf die App laut `/.flatpak-info` `flatpak-spawn --host` benutzen?
fn host_access(info: &str) -> bool {
    info.lines()
        .map(|l| l.replace(' ', ""))
        .any(|l| l == "org.freedesktop.Flatpak=talk" || l == "org.freedesktop.Flatpak=own")
}

/// Befehl für ein Programm des Systems, nicht der Sandbox
pub fn host_command(program: &str) -> Command {
    if app_id().is_some() {
        let mut c = Command::new("flatpak-spawn");
        c.args(["--host", program]);
        c
    } else {
        Command::new(program)
    }
}

/// Autostart über das Background-Portal ein- oder ausschalten. Das Portal
/// legt den Eintrag in ~/.config/autostart selbst an (mit `flatpak run`).
pub fn request_autostart(on: bool) -> Result<(), String> {
    let conn = zbus::blocking::Connection::session().map_err(|e| e.to_string())?;
    let options: HashMap<&str, Value> = HashMap::from([
        ("reason", Value::from(t("StarCLX beim Anmelden starten"))),
        ("autostart", Value::from(on)),
        ("commandline", Value::from(vec!["starclx"])),
        ("dbus-activatable", Value::from(false)),
    ]);
    // Die Antwort kommt später als Signal; ob der Benutzer zustimmt,
    // entscheidet die Desktop-Umgebung.
    conn.call_method(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        Some("org.freedesktop.portal.Background"),
        "RequestBackground",
        &("", options),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_access_from_info() {
        let own = "[Application]\nname=x\n\n[Session Bus Policy]\norg.freedesktop.Flatpak=talk\n";
        assert!(host_access(own));
        let flathub = "[Session Bus Policy]\norg.freedesktop.secrets=talk\n";
        assert!(!host_access(flathub));
    }
}
