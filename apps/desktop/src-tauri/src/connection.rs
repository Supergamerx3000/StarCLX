//! Erreichbarkeit der Anlage. Bricht die Verbindung ab (Netz weg, VPN
//! getrennt), zeigt die Oberfläche einen einzigen Hinweis statt Fehlern in
//! jedem Modul; die Einzelfehler stehen nur im Protokoll. Ist die Anlage
//! wieder erreichbar, wird alles neu geladen wie nach dem Standby.
//!
//! Geprüft wird mit einer kleinen Anfrage (Version der Anlage). Jede Antwort
//! der Anlage zählt als erreichbar, auch eine Fehlermeldung; nur ausbleibende
//! Antworten nicht.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;

/// Prüfabstand bei bestehender Verbindung
const POLL: Duration = Duration::from_secs(10);
/// Prüfabstand nach einem Fehlschlag und solange die Anlage weg ist
const RETRY: Duration = Duration::from_secs(3);
/// So lange auf die Antwort warten
const TIMEOUT: Duration = Duration::from_secs(5);
/// Erst nach so vielen Fehlschlägen hintereinander gilt die Anlage als weg,
/// damit ein einzelner verlorener Ping keinen Hinweis auslöst
const FAILURES: u32 = 2;

static ONLINE: AtomicBool = AtomicBool::new(true);

/// Anlage erreichbar (oder keine Sitzung)
pub fn online() -> bool {
    ONLINE.load(Ordering::Relaxed)
}

/// Erreichbarkeit setzen; `true`, wenn sich etwas geändert hat
fn set_online(app: &AppHandle, online: bool) -> bool {
    if ONLINE.swap(online, Ordering::Relaxed) == online {
        return false;
    }
    let _ = app.emit("connection", online);
    true
}

/// Neue oder beendete Sitzung: der alte Stand gilt nicht mehr
pub fn reset(app: &AppHandle) {
    set_online(app, true);
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut failures = 0;
        loop {
            let wait = if failures > 0 { RETRY } else { POLL };
            tokio::time::sleep(wait).await;
            let hub = app
                .state::<AppState>()
                .session
                .lock()
                .await
                .as_ref()
                .map(|s| s.hub().clone());
            let Some(hub) = hub else {
                // Ohne Sitzung gibt es nichts zu melden
                failures = 0;
                set_online(&app, true);
                continue;
            };
            match reachable(&hub).await {
                Ok(()) => {
                    failures = 0;
                    if set_online(&app, true) {
                        tracing::info!("Anlage wieder erreichbar, verbinde neu");
                        crate::wake::resume(&app).await;
                    }
                }
                Err(e) => {
                    failures += 1;
                    tracing::debug!(error = %e, failures, "Anlage antwortet nicht");
                    if failures >= FAILURES && set_online(&app, false) {
                        tracing::warn!(error = %e, "Verbindung zur Anlage verloren");
                    }
                }
            }
        }
    });
}

async fn reachable(hub: &sf_onehub::OneHub) -> Result<(), String> {
    match tokio::time::timeout(TIMEOUT, hub.server_version()).await {
        Err(_) => Err("keine Antwort".into()),
        Ok(Err(e)) if e.unreachable() => Err(e.to_string()),
        Ok(_) => Ok(()),
    }
}

/// Stand für die Oberfläche beim Start
#[tauri::command]
pub fn connection_online() -> bool {
    online()
}
