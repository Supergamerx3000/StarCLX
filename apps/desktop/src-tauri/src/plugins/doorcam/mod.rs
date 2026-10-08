//! Plugin Türkamera: zeigt das Bild einer Türsprechstelle mit Kamera.
//!
//! Zwei Quellen wie im Windows-Client: Ruft eine Türsprechstelle an, nennt
//! die Anlage ihre Kamera-URL beim Anruf (der Call Manager zeigt dann das
//! Bild); ausserdem lassen sich Kameras in den Einstellungen mit Name und
//! URL anlegen und als Kachel anzeigen.
//!
//! Die Bilder holt `sf-doorcam` hier im Backend und schickt sie als JPEG
//! (Base64) über einen Kanal an die Oberfläche. So gelten dieselben
//! TLS-Regeln wie für die Anlage, und Zugangsdaten aus Kamera-URLs der
//! Anlage erreichen die Oberfläche nie.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use crate::flatpak;
use crate::i18n::{t, tf};
use crate::plugins::call;

/// Höchstens so viele Bilder pro Sekunde an die Oberfläche
const MIN_GAP: Duration = Duration::from_millis(100);
/// Wartezeit bis zum nächsten Versuch nach einem Fehler
const RETRY: Duration = Duration::from_secs(3);

/// In den Einstellungen angelegte Kamera
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DoorCam {
    pub name: String,
    /// `rtsp://…`, MJPEG- oder Einzelbild-URL, Zugangsdaten in der URL
    pub url: String,
}

#[derive(Default)]
pub struct DoorCamState {
    next: AtomicU64,
    watches: Mutex<HashMap<u64, JoinHandle<()>>>,
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DoorCamEvent {
    /// JPEG in Base64
    Frame { data: String },
    /// Kamera gerade nicht erreichbar; es folgt ein neuer Versuch.
    Error { message: String },
}

/// Bilder einer Kamera holen, bis `doorcam_stop` kommt. Entweder die Kamera
/// der Türsprechstelle, die gerade anruft (`call_id`), oder eine
/// angelegte Kamera (`url`).
#[tauri::command]
pub async fn doorcam_watch(
    app: AppHandle,
    state: State<'_, DoorCamState>,
    call_id: Option<String>,
    url: Option<String>,
    channel: Channel<DoorCamEvent>,
) -> Result<u64, String> {
    let url = match (&call_id, url) {
        (Some(id), _) => call::door_cam_url(&app, id)
            .await
            .ok_or(t("Keine Türkamera zu diesem Anruf"))?,
        (None, Some(url)) if !url.trim().is_empty() => url,
        _ => return Err(t("Keine Kamera-URL").into()),
    };
    let id = state.next.fetch_add(1, Ordering::Relaxed);
    let options = options();
    let task = tauri::async_runtime::spawn(async move {
        loop {
            let mut last: Option<Instant> = None;
            let mut stop = false;
            let result = sf_doorcam::watch(&url, &options, |frame| {
                // Kamera eines beendeten Anrufs nicht weiter abfragen, auch
                // wenn die Oberfläche das Abmelden verpasst (Neuladen)
                if call_id.as_ref().is_some_and(|id| !call::has_call(&app, id)) {
                    stop = true;
                    return false;
                }
                if last.is_some_and(|l| l.elapsed() < MIN_GAP) {
                    return true;
                }
                last = Some(Instant::now());
                let data = base64::engine::general_purpose::STANDARD.encode(&frame);
                stop = channel.send(DoorCamEvent::Frame { data }).is_err();
                !stop
            })
            .await;
            let Err(e) = result else { return };
            if stop {
                return;
            }
            // Ohne URL protokollieren: sie kann Zugangsdaten enthalten.
            tracing::info!(error = %e, "Türkamera");
            let message = message(&e);
            if channel.send(DoorCamEvent::Error { message }).is_err() {
                return;
            }
            tokio::time::sleep(RETRY).await;
            if call_id.as_ref().is_some_and(|id| !call::has_call(&app, id)) {
                return;
            }
        }
    });
    state.watches.lock().unwrap().insert(id, task);
    Ok(id)
}

#[tauri::command]
pub fn doorcam_stop(state: State<'_, DoorCamState>, id: u64) {
    if let Some(task) = state.watches.lock().unwrap().remove(&id) {
        task.abort();
    }
}

/// RTSP entpackt ffmpeg; im eigenen Flatpak das des Systems.
fn options() -> sf_doorcam::Options {
    let ffmpeg = if flatpak::app_id().is_some() && !flatpak::sandboxed() {
        vec!["flatpak-spawn".into(), "--host".into(), "ffmpeg".into()]
    } else {
        vec!["ffmpeg".into()]
    };
    sf_doorcam::Options { ffmpeg }
}

fn message(e: &sf_doorcam::Error) -> String {
    use sf_doorcam::Error as E;
    let detail = match e {
        E::Url(d) | E::Ffmpeg(d) => d.clone(),
        E::Http(e) => e.to_string(),
        E::Status(s) => s.to_string(),
        _ => String::new(),
    };
    let text = match e {
        E::Url(_) => "Ungültige Kamera-URL: {detail}",
        E::Http(_) => "Kamera nicht erreichbar: {detail}",
        E::Status(_) => "Kamera antwortet mit {detail}",
        E::NoImage => "Kamera liefert kein Bild",
        E::Stalled => "Kamera sendet keine Bilder mehr",
        E::NoFfmpeg => "Für RTSP-Kameras wird ffmpeg benötigt",
        E::Ffmpeg(_) => "ffmpeg: {detail}",
    };
    tf(text, &[("detail", &detail)])
}
