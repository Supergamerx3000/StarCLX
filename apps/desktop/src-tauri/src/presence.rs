//! Chat-Status automatisch auf „Abwesend“: bei Inaktivität, aktivem
//! Bildschirmschoner oder gesperrtem Bildschirm. Abgefragt wird unter Linux
//! über D-Bus (GNOME/Mutter, freedesktop-ScreenSaver als Rückfall, logind),
//! unter macOS über CoreGraphics (Leerlaufzeit, Bildschirmsperre), unter
//! Windows über user32 (letzte Eingabe, Eingabe-Desktop gesperrt).

use std::time::Duration;

use tauri::{AppHandle, Manager};
#[cfg(target_os = "linux")]
use zbus::blocking::Connection;

use crate::chat::ChatState;
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

#[cfg(target_os = "linux")]
struct Probe {
    session: Option<Connection>,
    system: Option<Connection>,
    login_session: String,
}

#[cfg(target_os = "linux")]
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

    fn read(&self) -> Desktop {
        Desktop {
            idle: self.idle(),
            screensaver: self.screensaver(),
            locked: self.locked(),
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
#[cfg(target_os = "linux")]
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

#[cfg(target_os = "macos")]
struct Probe;

#[cfg(target_os = "macos")]
impl Probe {
    fn new() -> Self {
        Self
    }

    fn read(&self) -> Desktop {
        Desktop {
            idle: Some(mac::idle()),
            // Der Bildschirmschoner sperrt unter macOS in der Regel; die
            // Sperre deckt ihn mit ab.
            screensaver: false,
            locked: mac::locked(),
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;
    use std::time::Duration;

    type CFTypeRef = *const c_void;

    const HID_SYSTEM_STATE: i32 = 1;
    const ANY_INPUT_EVENT: u32 = u32::MAX;
    const UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event: u32) -> f64;
        fn CGSessionCopyCurrentDictionary() -> CFTypeRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFBooleanTrue: CFTypeRef;
        fn CFStringCreateWithCString(
            alloc: CFTypeRef,
            s: *const std::ffi::c_char,
            encoding: u32,
        ) -> CFTypeRef;
        fn CFDictionaryGetValue(dict: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
        fn CFRelease(obj: CFTypeRef);
    }

    /// Zeit seit der letzten Eingabe (Tastatur, Maus, Trackpad)
    pub fn idle() -> Duration {
        // SAFETY: reine Abfrage ohne Zeiger
        let secs =
            unsafe { CGEventSourceSecondsSinceLastEventType(HID_SYSTEM_STATE, ANY_INPUT_EVENT) };
        Duration::from_secs_f64(secs.max(0.0))
    }

    /// Ist der Bildschirm gesperrt? (`CGSSessionScreenIsLocked` der Sitzung)
    pub fn locked() -> bool {
        // SAFETY: Copy/Create-Ergebnisse werden freigegeben, der Wert aus
        // dem Dictionary nur verglichen (Get-Regel, nicht freigeben).
        unsafe {
            let dict = CGSessionCopyCurrentDictionary();
            if dict.is_null() {
                return false;
            }
            let key = CFStringCreateWithCString(
                std::ptr::null(),
                c"CGSSessionScreenIsLocked".as_ptr(),
                UTF8,
            );
            let locked = !key.is_null() && CFDictionaryGetValue(dict, key) == kCFBooleanTrue;
            if !key.is_null() {
                CFRelease(key);
            }
            CFRelease(dict);
            locked
        }
    }
}

#[cfg(windows)]
struct Probe;

#[cfg(windows)]
impl Probe {
    fn new() -> Self {
        Self
    }

    fn read(&self) -> Desktop {
        Desktop {
            idle: win::idle(),
            screensaver: win::screensaver(),
            locked: win::locked(),
        }
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::time::Duration;

    #[repr(C)]
    struct LastInputInfo {
        size: u32,
        time: u32,
    }

    const DESKTOP_SWITCHDESKTOP: u32 = 0x0100;
    const SPI_GETSCREENSAVERRUNNING: u32 = 0x0072;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetLastInputInfo(info: *mut LastInputInfo) -> i32;
        fn OpenInputDesktop(flags: u32, inherit: i32, access: u32) -> *mut c_void;
        fn SwitchDesktop(desktop: *mut c_void) -> i32;
        fn CloseDesktop(desktop: *mut c_void) -> i32;
        fn SystemParametersInfoW(action: u32, param: u32, value: *mut c_void, ini: u32) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetTickCount() -> u32;
    }

    /// Zeit seit der letzten Eingabe in dieser Sitzung
    pub fn idle() -> Option<Duration> {
        let mut info = LastInputInfo {
            size: size_of::<LastInputInfo>() as u32,
            time: 0,
        };
        // SAFETY: info ist gültig und size gesetzt
        if unsafe { GetLastInputInfo(&mut info) } == 0 {
            return None;
        }
        // SAFETY: reine Abfrage
        let now = unsafe { GetTickCount() };
        Some(Duration::from_millis(u64::from(
            now.wrapping_sub(info.time),
        )))
    }

    pub fn screensaver() -> bool {
        let mut running: i32 = 0;
        // SAFETY: running ist ein gültiger BOOL-Ausgabewert
        let ok = unsafe {
            SystemParametersInfoW(SPI_GETSCREENSAVERRUNNING, 0, (&raw mut running).cast(), 0)
        };
        ok != 0 && running != 0
    }

    /// Gesperrt, wenn sich der Eingabe-Desktop nicht übernehmen lässt
    /// (Sperrbildschirm bzw. Winlogon-Desktop).
    pub fn locked() -> bool {
        // SAFETY: Handle wird geprüft und wieder geschlossen
        unsafe {
            let desk = OpenInputDesktop(0, 0, DESKTOP_SWITCHDESKTOP);
            if desk.is_null() {
                return true;
            }
            let ok = SwitchDesktop(desk) != 0;
            CloseDesktop(desk);
            !ok
        }
    }
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
                let now = is_away(&prefs, probe.read());
                if now != away {
                    away = now;
                    AWAY.store(away, std::sync::atomic::Ordering::Relaxed);
                    let app = app.clone();
                    let text = if away { prefs.away_text } else { String::new() };
                    tauri::async_runtime::spawn(async move {
                        if let Some(chat) = app.state::<ChatState>().chat.lock().await.as_ref() {
                            chat.set_presence(away, &text);
                        }
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
    #[cfg(target_os = "linux")]
    fn session_path_escaping() {
        assert_eq!(escape("2"), "2");
        assert_eq!(escape("c1-x"), "c1_2dx");
    }
}
