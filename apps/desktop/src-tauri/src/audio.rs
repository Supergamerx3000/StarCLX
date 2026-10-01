//! Audio der Oberfläche: Klingeln auf dem gewählten Gerät, Testton,
//! Mikrofontest und die Geräteauswahl fürs Softphone.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use sf_audio::{Devices, MicMeter, Playback};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::settings::{self, Prefs};

#[derive(Default)]
pub struct AudioState {
    ringer: Mutex<Option<Playback>>,
    /// Vorschau oder Testton aus den Einstellungen
    preview: Mutex<Option<Playback>>,
    meter: Mutex<Option<MicMeter>>,
    /// Hinweiston für Chat-Nachrichten
    chime: Mutex<Option<Playback>>,
}

fn devices() -> Devices {
    sf_audio::list().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Audiogeräte nicht gelesen");
        Devices::default()
    })
}

/// baresip-Einstellungen für Lautsprecher und Mikrofon aus der Reihenfolge
/// in den Einstellungen.
pub async fn softphone_config(prefs: &Prefs) -> sf_sip::Config {
    let mut config = sf_sip::Config::default();
    if prefs.speakers.is_empty() && prefs.microphones.is_empty() {
        return config;
    }
    let present = tauri::async_runtime::spawn_blocking(devices)
        .await
        .unwrap_or_default();
    if let Some(dev) = sf_audio::pick(&prefs.speakers, &present.speakers) {
        config.audio_player = format!("pipewire,{dev}");
    }
    if let Some(dev) = sf_audio::pick(&prefs.microphones, &present.microphones) {
        config.audio_source = format!("pipewire,{dev}");
    }
    config
}

/// PCM eines Klingeltons: eingebauter Name oder Pfad einer WAV-Datei.
fn ringtone(name: &str) -> Arc<Vec<i16>> {
    if name.ends_with(".wav") || name.contains('/') {
        match sf_audio::load_wav(std::path::Path::new(name)) {
            Ok(pcm) => return Arc::new(pcm),
            Err(e) => tracing::warn!(error = %e, name, "Klingelton nicht geladen"),
        }
    }
    Arc::new(sf_audio::tones::render(name))
}

/// Spielt den Hinweiston für eine neue Chat-Nachricht auf dem Klingelgerät.
pub fn play_message_tone(app: &AppHandle, prefs: &Prefs) {
    let device = sf_audio::pick(&prefs.ring_devices, &devices().speakers);
    let tone = Arc::new(sf_audio::tones::message_tone());
    *app.state::<AudioState>().chime.lock().unwrap() = Some(Playback::start(device, tone, false));
}

/// Startet oder beendet das Klingeln je nach Anrufstand.
pub fn update_ringer(app: &AppHandle, ringing: Option<bool>) {
    let state = app.state::<AudioState>();
    let mut ringer = state.ringer.lock().unwrap();
    match ringing {
        None => {
            ringer.take();
        }
        Some(_) if ringer.is_some() => {}
        Some(internal) => {
            let prefs = settings::load(app).prefs;
            if !prefs.ringtone {
                return;
            }
            let tone = if internal {
                &prefs.ringtone_internal
            } else {
                &prefs.ringtone_external
            };
            let samples = ringtone(tone);
            let device = sf_audio::pick(&prefs.ring_devices, &devices().speakers);
            *ringer = Some(Playback::start(device, samples, true));
        }
    }
}

#[derive(Serialize)]
pub struct AudioInfo {
    devices: Devices,
    ringtones: Vec<String>,
}

#[tauri::command]
pub async fn audio_info() -> AudioInfo {
    AudioInfo {
        devices: tauri::async_runtime::spawn_blocking(devices)
            .await
            .unwrap_or_default(),
        ringtones: sf_audio::tones::NAMES
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
    }
}

fn device_arg(device: Option<String>) -> Option<String> {
    device.filter(|d| !d.is_empty() && d != sf_audio::DEFAULT_DEVICE)
}

/// Spielt einen Klingelton (oder mit `name = None` den Testton) einmal ab.
#[tauri::command]
pub fn audio_preview(state: State<'_, AudioState>, name: Option<String>, device: Option<String>) {
    let samples = name.map_or_else(|| Arc::new(sf_audio::tones::test_tone()), |n| ringtone(&n));
    *state.preview.lock().unwrap() = Some(Playback::start(device_arg(device), samples, false));
}

#[tauri::command]
pub fn audio_stop(state: State<'_, AudioState>) {
    state.preview.lock().unwrap().take();
    state.meter.lock().unwrap().take();
}

/// Mikrofontest: meldet den Pegel als Event "mic-level" (0..1).
#[tauri::command]
pub fn mic_test(app: AppHandle, state: State<'_, AudioState>, device: Option<String>) {
    let emitter = app.clone();
    *state.meter.lock().unwrap() = Some(MicMeter::start(device_arg(device), move |level| {
        let _ = emitter.emit("mic-level", level);
    }));
}

/// Eigene WAV-Datei als Klingelton auswählen. Gibt den Pfad zurück.
#[tauri::command]
pub async fn pick_ringtone(app: AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Klingelton auswählen")
        .add_filter("WAV-Datei", &["wav"])
        .pick_file(move |f| {
            let _ = tx.send(f);
        });
    let Some(file) = rx.await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let path = file.into_path().map_err(|e| e.to_string())?;
    sf_audio::load_wav(&path).map_err(|e| e.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}
