//! Erkennt das Aufwachen aus dem Standby und bringt die Verbindungen wieder
//! in Gang: Access-Token sofort erneuern, Softphone neu registrieren und die
//! Oberfläche neu laden lassen (Event "resumed").
//!
//! Erkennung ohne Plattform-API: Die monotone Uhr steht während des Standbys
//! (macOS) bzw. läuft anders als die Wanduhr. Springt die Wanduhr zwischen
//! zwei Prüfungen deutlich weiter als die monotone Uhr, hat der Rechner
//! geschlafen.

use std::time::{Duration, Instant, SystemTime};

use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;

const POLL: Duration = Duration::from_secs(5);
/// Ab dieser Lücke zwischen Wanduhr und monotoner Uhr gilt es als Standby
const GAP: Duration = Duration::from_secs(20);

/// Wie lange der Rechner geschlafen hat, falls überhaupt
fn slept(wall: Duration, mono: Duration) -> Option<Duration> {
    wall.checked_sub(mono).filter(|gap| *gap >= GAP)
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut wall = SystemTime::now();
        let mut mono = Instant::now();
        loop {
            tokio::time::sleep(POLL).await;
            let (now_wall, now_mono) = (SystemTime::now(), Instant::now());
            let gap = now_wall
                .duration_since(wall)
                .ok()
                .and_then(|w| slept(w, now_mono - mono));
            (wall, mono) = (now_wall, now_mono);
            if let Some(gap) = gap {
                tracing::info!(?gap, "Aufgewacht aus dem Standby, verbinde neu");
                resume(&app).await;
            }
        }
    });
}

async fn resume(app: &AppHandle) {
    let state = app.state::<AppState>();
    // Erst das Token, sonst schlagen die Neuladungen mit 401 fehl.
    let Some(resumer) = state.session.lock().await.as_ref().map(|s| s.resumer()) else {
        return;
    };
    let fresh = resumer.resume().await;
    if !fresh {
        tracing::warn!("Token nach dem Aufwachen noch nicht erneuert");
    }
    if let Some(phone) = state.phone.lock().await.as_ref() {
        phone.resume();
    }
    let _ = app.emit("resumed", ());
    // Umleitungen und Erreichbarkeit neu laden
    let _ = app.emit("reach-changed", ());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_detection() {
        let s = Duration::from_secs;
        assert_eq!(slept(s(5), s(5)), None);
        assert_eq!(slept(s(20), s(5)), None);
        assert_eq!(slept(s(3605), s(5)), Some(s(3600)));
        assert_eq!(slept(s(5), s(30)), None);
    }
}
