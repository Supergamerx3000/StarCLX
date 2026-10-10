//! Chat-Status automatisch auf „Abwesend“: bei Inaktivität, aktivem
//! Bildschirmschoner oder gesperrtem Bildschirm. Abgefragt wird über D-Bus
//! (GNOME/Mutter, freedesktop-ScreenSaver als Rückfall, logind).

use std::time::Duration;

use tauri::AppHandle;
use zbus::blocking::Connection;

use crate::settings::{self, Prefs};

/// Aktueller automatischer Zustand, damit ein neu verbundener Chat ihn erbt
static AWAY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Ist man gerade automatisch abwesend? (für den Start des Chats)
pub fn away() -> bool {
    AWAY.load(std::sync::atomic::Ordering::Relaxed)
}

/// Ab dieser Zeit ohne Eingabe gilt man als abwesend
const IDLE_AWAY: Duration = Duration::from_secs(10 * 60);
const POLL: Duration = Duration::from_secs(5);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Desktop {
    idle: Option<Duration>,
    screensaver: bool,
    locked: bool,
}

fn is_away(prefs: &Prefs, d: Desktop) -> bool {
    (prefs.away_on_idle && d.idle.is_some_and(|i| i >= IDLE_AWAY))
        || (prefs.away_on_screensaver && d.screensaver)
        || (prefs.away_on_lock && d.locked)
}

struct Probe {
    session: Option<Connection>,
    system: Option<Connection>,
    login_session: String,
}

impl Probe {
    fn new() -> Self {
        let login_session = std::env::var("XDG_SESSION_ID").map_or_else(
            |_| "/org/freedesktop/login1/session/auto".to_owned(),
            |id| format!("/org/freedesktop/login1/session/{}", escape(&id)),
        );
        Self {
            session: Connection::session().ok(),
            system: Connection::system().ok(),
            login_session,
        }
    }

    /// Fragt nur ab, was die Einstellungen auch auswerten
    fn read(&self, prefs: &Prefs) -> Desktop {
        Desktop {
            idle: prefs.away_on_idle.then(|| self.idle()).flatten(),
            screensaver: prefs.away_on_screensaver && self.screensaver(),
            locked: prefs.away_on_lock && self.locked(),
        }
    }

    fn idle(&self) -> Option<Duration> {
        let c = self.session.as_ref()?;
        let gnome = c
            .call_method(
                Some("org.gnome.Mutter.IdleMonitor"),
                "/org/gnome/Mutter/IdleMonitor/Core",
                Some("org.gnome.Mutter.IdleMonitor"),
                "GetIdletime",
                &(),
            )
            .and_then(|m| m.body().deserialize::<u64>());
        if let Ok(ms) = gnome {
            return Some(Duration::from_millis(ms));
        }
        // KDE und andere: Sekunden
        c.call_method(
            Some("org.freedesktop.ScreenSaver"),
            "/org/freedesktop/ScreenSaver",
            Some("org.freedesktop.ScreenSaver"),
            "GetSessionIdleTime",
            &(),
        )
        .and_then(|m| m.body().deserialize::<u32>())
        .ok()
        .map(|s| Duration::from_secs(s.into()))
    }

    fn screensaver(&self) -> bool {
        let Some(c) = self.session.as_ref() else {
            return false;
        };
        [
            ("org.gnome.ScreenSaver", "/org/gnome/ScreenSaver"),
            (
                "org.freedesktop.ScreenSaver",
                "/org/freedesktop/ScreenSaver",
            ),
        ]
        .iter()
        .find_map(|(name, path)| {
            c.call_method(Some(*name), *path, Some(*name), "GetActive", &())
                .and_then(|m| m.body().deserialize::<bool>())
                .ok()
        })
        .unwrap_or(false)
    }

    fn locked(&self) -> bool {
        let Some(c) = self.system.as_ref() else {
            return false;
        };
        c.call_method(
            Some("org.freedesktop.login1"),
            self.login_session.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.login1.Session", "LockedHint"),
        )
        .and_then(|m| m.body().deserialize::<zbus::zvariant::OwnedValue>())
        .ok()
        .and_then(|v| bool::try_from(v).ok())
        .unwrap_or(false)
    }
}

/// logind kodiert Sitzungs-IDs im Objektpfad (Ziffern/Buchstaben bleiben)
fn escape(id: &str) -> String {
    id.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                (b as char).to_string()
            } else {
                format!("_{b:02x}")
            }
        })
        .collect()
}

/// Startet die Überwachung im Hintergrund; sie läuft bis zum Programmende.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("presence".into())
        .spawn(move || {
            let probe = Probe::new();
            let mut away = false;
            loop {
                std::thread::sleep(POLL);
                let prefs = settings::load(&app).prefs;
                let now = is_away(&prefs, probe.read(&prefs));
                if now != away {
                    away = now;
                    AWAY.store(away, std::sync::atomic::Ordering::Relaxed);
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        crate::plugins::chat::apply_own(&app).await
                    });
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn away_rules() {
        let prefs = Prefs::default();
        let idle = |m: u64| Desktop {
            idle: Some(Duration::from_secs(m * 60)),
            ..Default::default()
        };
        assert!(!is_away(&prefs, idle(2)));
        assert!(is_away(&prefs, idle(10)));
        assert!(is_away(
            &prefs,
            Desktop {
                locked: true,
                ..Default::default()
            }
        ));
        let off = Prefs {
            away_on_idle: false,
            away_on_lock: false,
            ..Prefs::default()
        };
        assert!(!is_away(&off, idle(30)));
        assert!(!is_away(
            &off,
            Desktop {
                locked: true,
                ..Default::default()
            }
        ));
        assert!(is_away(
            &off,
            Desktop {
                screensaver: true,
                ..Default::default()
            }
        ));
    }

    #[test]
    fn session_path_escaping() {
        assert_eq!(escape("2"), "2");
        assert_eq!(escape("c1-x"), "c1_2dx");
    }
}
