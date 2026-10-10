//! Globale Tastenkürzel über das XDG-Portal „GlobalShortcuts“ (KDE Plasma,
//! Hyprland, GNOME ab 48 auch im Flatpak). StarCLX meldet seine Kürzel beim
//! Portal an, der Desktop fragt beim ersten Mal nach und meldet jeden
//! Tastendruck als Signal, solange StarCLX läuft. Welche Taste am Ende gilt,
//! entscheidet der Desktop: Unter KDE ändert man sie danach in den
//! Systemeinstellungen.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once, OnceLock};

use tauri::AppHandle;
use zbus::blocking::Connection;
use zbus::blocking::proxy::{Builder, Proxy};
use zbus::proxy::CacheProperties;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use crate::desktop::{ACTIONS, Hotkeys};
use crate::i18n::t;

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const IFACE: &str = "org.freedesktop.portal.GlobalShortcuts";

/// Eigene Verbindung, damit Sitzung und Signale zusammenbleiben
fn conn() -> Option<&'static Connection> {
    static CONN: OnceLock<Option<Connection>> = OnceLock::new();
    CONN.get_or_init(|| Connection::session().ok()).as_ref()
}

/// Gerade gültige Sitzung; Signale anderer Sitzungen werden ignoriert
static SESSION: Mutex<Option<OwnedObjectPath>> = Mutex::new(None);
/// Zählt Anmeldungen, damit eine überholte Anmeldung ihre Sitzung schliesst
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Zuletzt angemeldete Kürzel; unverändert wird nicht neu angemeldet
static APPLIED: Mutex<Option<Hotkeys>> = Mutex::new(None);
static TOKEN: AtomicU64 = AtomicU64::new(0);
static LISTENER: Once = Once::new();

fn proxy<'a>(conn: &Connection, path: String, iface: &'a str) -> zbus::Result<Proxy<'a>> {
    Builder::new(conn)
        .destination(DEST)?
        .path(path)?
        .interface(iface)?
        .cache_properties(CacheProperties::No)
        .build()
}

/// Bietet der Desktop das Portal an?
pub fn available() -> bool {
    conn().is_some_and(|c| {
        c.call_method(
            Some(DEST),
            PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(IFACE, "version"),
        )
        .is_ok()
    })
}

/// Meldet die Kürzel neu an bzw. ab. Die Anmeldung läuft im Hintergrund,
/// weil der Desktop dabei nachfragen kann; Fehler landen im Log.
pub fn apply(app: &AppHandle, hotkeys: &Hotkeys) -> Result<(), String> {
    {
        let mut applied = APPLIED.lock().unwrap_or_else(|e| e.into_inner());
        if applied.as_ref() == Some(hotkeys) {
            return Ok(());
        }
        *applied = Some(hotkeys.clone());
    }
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    close_session();
    let shortcuts: Vec<(String, String, String)> = ACTIONS
        .iter()
        .filter(|(a, _)| hotkeys.enabled && !hotkeys.binding(a).is_empty())
        .map(|(a, label)| {
            (
                (*a).to_owned(),
                t(label).to_owned(),
                xdg_trigger(hotkeys.binding(a)),
            )
        })
        .collect();
    if shortcuts.is_empty() {
        return Ok(());
    }
    let conn = conn().ok_or_else(|| t("Keine Verbindung zum Desktop (D-Bus)").to_owned())?;
    let handle = app.clone();
    LISTENER.call_once(move || {
        std::thread::spawn(move || {
            if let Err(e) = listen(conn, |id| crate::desktop::run_action(&handle, id)) {
                tracing::warn!(error = %e, "Tastenkürzel-Signale nicht empfangen");
            }
        });
    });
    std::thread::spawn(move || match bind(conn, &shortcuts) {
        Ok(session) => {
            let mut current = SESSION.lock().unwrap_or_else(|e| e.into_inner());
            if GENERATION.load(Ordering::SeqCst) == generation {
                *current = Some(session);
            } else {
                close(conn, &session);
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "Tastenkürzel nicht beim Portal angemeldet");
            // Beim nächsten Speichern erneut versuchen
            *APPLIED.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    });
    Ok(())
}

/// Öffnet die Einstellungen des Desktops für die Kürzel von StarCLX (KDE:
/// Systemeinstellungen › Kurzbefehle).
#[tauri::command]
pub fn configure_hotkeys() -> Result<(), String> {
    let session = SESSION.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let (Some(conn), Some(session)) = (conn(), session) else {
        return Err(t("Die Tastenkürzel sind noch nicht angemeldet.").into());
    };
    let options: HashMap<&str, Value> = HashMap::new();
    conn.call_method(
        Some(DEST),
        PATH,
        Some(IFACE),
        "ConfigureShortcuts",
        &(session.as_ref(), "", options),
    )
    .map(|_| ())
    .map_err(|_| t("Der Desktop kann die Tastenkürzel nicht selbst anzeigen. Du findest sie in den Systemeinstellungen.").into())
}

fn close_session() {
    let old = SESSION.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let (Some(conn), Some(old)) = (conn(), old) {
        close(conn, &old);
    }
}

fn close(conn: &Connection, session: &OwnedObjectPath) {
    let _ = conn.call_method(
        Some(DEST),
        session.as_str(),
        Some("org.freedesktop.portal.Session"),
        "Close",
        &(),
    );
}

/// Absender in Objektpfaden des Portals: `:1.42` → `1_42`
fn sender(conn: &Connection) -> String {
    conn.unique_name()
        .map(|n| n.trim_start_matches(':').replace('.', "_"))
        .unwrap_or_default()
}

fn token() -> String {
    format!("starclx{}", TOKEN.fetch_add(1, Ordering::Relaxed))
}

/// Ruft eine Portal-Methode auf und wartet auf deren Antwort-Signal.
fn request<B>(conn: &Connection, method: &str, token: &str, body: &B) -> Result<(), String>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    let path = format!("{PATH}/request/{}/{token}", sender(conn));
    let request = proxy(conn, path, "org.freedesktop.portal.Request").map_err(|e| e.to_string())?;
    // Vor dem Aufruf abonnieren, sonst kann die Antwort verloren gehen
    let mut responses = request
        .receive_signal("Response")
        .map_err(|e| e.to_string())?;
    conn.call_method(Some(DEST), PATH, Some(IFACE), method, body)
        .map_err(|e| e.to_string())?;
    let msg = responses
        .next()
        .ok_or_else(|| format!("{method}: keine Antwort"))?;
    let (code, _results) = msg
        .body()
        .deserialize::<(u32, HashMap<String, OwnedValue>)>()
        .map_err(|e| e.to_string())?;
    match code {
        0 => Ok(()),
        1 => Err(format!("{method}: abgelehnt")),
        _ => Err(format!("{method}: Fehler {code}")),
    }
}

/// Neue Sitzung anlegen und die Kürzel daran binden
fn bind(
    conn: &Connection,
    shortcuts: &[(String, String, String)],
) -> Result<OwnedObjectPath, String> {
    let session_token = token();
    let session =
        OwnedObjectPath::try_from(format!("{PATH}/session/{}/{session_token}", sender(conn)))
            .map_err(|e| e.to_string())?;
    let create = token();
    let options: HashMap<&str, Value> = HashMap::from([
        ("handle_token", Value::from(create.as_str())),
        ("session_handle_token", Value::from(session_token.as_str())),
    ]);
    request(conn, "CreateSession", &create, &(options,))?;

    let list: Vec<(&str, HashMap<&str, Value>)> = shortcuts
        .iter()
        .map(|(id, description, trigger)| {
            (
                id.as_str(),
                HashMap::from([
                    ("description", Value::from(description.as_str())),
                    ("preferred_trigger", Value::from(trigger.as_str())),
                ]),
            )
        })
        .collect();
    let bind = token();
    let options: HashMap<&str, Value> =
        HashMap::from([("handle_token", Value::from(bind.as_str()))]);
    let path: ObjectPath = session.as_ref();
    if let Err(e) = request(conn, "BindShortcuts", &bind, &(path, list, "", options)) {
        close(conn, &session);
        return Err(e);
    }
    Ok(session)
}

/// Tastendrücke der aktuellen Sitzung als Aktion ausführen
fn listen(conn: &Connection, run: impl Fn(&str)) -> zbus::Result<()> {
    let portal = proxy(conn, PATH.to_owned(), IFACE)?;
    for msg in portal.receive_signal("Activated")? {
        let Ok((session, id, _, _)) =
            msg.body()
                .deserialize::<(OwnedObjectPath, String, u64, HashMap<String, OwnedValue>)>()
        else {
            continue;
        };
        let current = SESSION.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if current.as_ref() == Some(&session) && ACTIONS.iter().any(|(a, _)| *a == id) {
            run(&id);
        }
    }
    Ok(())
}

/// GTK-Kürzel (`<Control><Shift>w`) im Format des Portals (`CTRL+SHIFT+w`)
fn xdg_trigger(gtk: &str) -> String {
    let mut parts = Vec::new();
    let mut rest = gtk;
    while let Some((m, r)) = rest.strip_prefix('<').and_then(|r| r.split_once('>')) {
        if let Some(m) = match m.to_ascii_lowercase().as_str() {
            "control" | "ctrl" | "primary" => Some("CTRL"),
            "shift" => Some("SHIFT"),
            "alt" | "mod1" => Some("ALT"),
            "super" | "meta" | "mod4" => Some("LOGO"),
            _ => None,
        } {
            parts.push(m);
        }
        rest = r;
    }
    // Namen aus dem Browser in X-Keysym-Namen
    parts.push(match rest {
        "Enter" => "Return",
        " " => "space",
        "ArrowUp" => "Up",
        "ArrowDown" => "Down",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        "PageUp" => "Prior",
        "PageDown" => "Next",
        k => k,
    });
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_format() {
        assert_eq!(xdg_trigger("<Control><Shift>w"), "CTRL+SHIFT+w");
        assert_eq!(xdg_trigger("<Alt><Super>F5"), "ALT+LOGO+F5");
        assert_eq!(xdg_trigger("<Primary>Enter"), "CTRL+Return");
        assert_eq!(xdg_trigger("F9"), "F9");
    }

    /// Nachgebautes Portal, nur die Teile, die StarCLX braucht
    struct MockPortal;

    #[zbus::interface(name = "org.freedesktop.portal.GlobalShortcuts")]
    impl MockPortal {
        #[zbus(property, name = "version")]
        fn version(&self) -> u32 {
            2
        }

        async fn create_session(
            &self,
            options: HashMap<String, OwnedValue>,
            #[zbus(header)] hdr: zbus::message::Header<'_>,
            #[zbus(connection)] conn: &zbus::Connection,
        ) -> OwnedObjectPath {
            respond(conn, &hdr, &options).await
        }

        async fn bind_shortcuts(
            &self,
            session: OwnedObjectPath,
            shortcuts: Vec<(String, HashMap<String, OwnedValue>)>,
            _parent: String,
            options: HashMap<String, OwnedValue>,
            #[zbus(header)] hdr: zbus::message::Header<'_>,
            #[zbus(connection)] conn: &zbus::Connection,
        ) -> OwnedObjectPath {
            assert!(session.as_str().starts_with(PATH));
            let (_, props) = &shortcuts[0];
            assert_eq!(
                String::try_from(props["preferred_trigger"].try_clone().unwrap()).unwrap(),
                "CTRL+SHIFT+w"
            );
            respond(conn, &hdr, &options).await
        }
    }

    async fn respond(
        conn: &zbus::Connection,
        hdr: &zbus::message::Header<'_>,
        options: &HashMap<String, OwnedValue>,
    ) -> OwnedObjectPath {
        let token = String::try_from(options["handle_token"].try_clone().unwrap()).unwrap();
        let sender = hdr
            .sender()
            .unwrap()
            .trim_start_matches(':')
            .replace('.', "_");
        let path = OwnedObjectPath::try_from(format!("{PATH}/request/{sender}/{token}")).unwrap();
        conn.emit_signal(
            hdr.sender().map(|s| s.as_str()),
            &path,
            "org.freedesktop.portal.Request",
            "Response",
            &(0u32, HashMap::<&str, Value>::new()),
        )
        .await
        .unwrap();
        path
    }

    /// Braucht einen eigenen Session-Bus: `dbus-run-session -- cargo test -p
    /// starclx portal_roundtrip -- --ignored`
    #[test]
    #[ignore]
    fn portal_roundtrip() {
        let server = zbus::blocking::connection::Builder::session()
            .unwrap()
            .name(DEST)
            .unwrap()
            .serve_at(PATH, MockPortal)
            .unwrap()
            .build()
            .unwrap();
        assert!(available());
        let conn = conn().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            listen(conn, |id| tx.send(id.to_owned()).unwrap()).unwrap();
        });
        // Abo des Signals abwarten
        std::thread::sleep(std::time::Duration::from_millis(300));
        let shortcuts = [(
            "dial-selection".to_owned(),
            "Markierte Rufnummer wählen".to_owned(),
            xdg_trigger("<Control><Shift>w"),
        )];
        let session = bind(conn, &shortcuts).unwrap();
        assert!(session.as_str().contains("/session/"));
        *SESSION.lock().unwrap() = Some(session.clone());
        // Tastendruck einer fremden und der eigenen Sitzung
        for (session, id) in [
            (format!("{PATH}/session/x/y"), "answer"),
            (session.to_string(), "dial-selection"),
        ] {
            server
                .emit_signal(
                    None::<&str>,
                    PATH,
                    IFACE,
                    "Activated",
                    &(
                        ObjectPath::try_from(session).unwrap(),
                        id,
                        0u64,
                        HashMap::<&str, Value>::new(),
                    ),
                )
                .unwrap();
        }
        let got = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(got, "dial-selection");
    }
}
