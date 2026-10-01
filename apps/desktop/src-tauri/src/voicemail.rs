//! Voicemail-Ansicht: Nachrichten der Anlage abhören, verschieben, löschen.

use sf_core::voicemail::{self, Voicemail, VoicemailEvent, Watcher};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::{Mutex, mpsc};

use crate::{AppState, hub};

#[derive(Default)]
pub struct VoicemailState {
    watcher: Mutex<Option<Watcher>>,
}

/// Beim An- und Abmelden aufrufen.
pub async fn restart(app: &AppHandle, hub: Option<sf_onehub::OneHub>) {
    let state = app.state::<VoicemailState>();
    *state.watcher.lock().await = hub.map(|hub| {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(ev) = rx.recv().await {
                match ev {
                    VoicemailEvent::Changed => {
                        let _ = app.emit("voicemail-changed", ());
                    }
                    VoicemailEvent::Created { voicemail } => notify(&app, &voicemail),
                }
            }
        });
        Watcher::start(hub, tx)
    });
}

fn notify(app: &AppHandle, v: &Voicemail) {
    let who = match (v.name.trim(), v.number.trim()) {
        ("", "") => "Unbekannt".to_owned(),
        ("", n) | (n, "") => n.to_owned(),
        (name, n) => format!("{name} ({n})"),
    };
    if let Err(e) = app
        .notification()
        .builder()
        .title("Neue Voicemail")
        .body(format!("von {who}"))
        .show()
    {
        tracing::warn!(error = %e, "Benachrichtigung nicht angezeigt");
    }
}

#[tauri::command]
pub async fn voicemails(state: State<'_, AppState>) -> Result<Vec<Voicemail>, String> {
    voicemail::list(&hub(&state).await?)
        .await
        .map_err(|e| e.to_string())
}

/// Die Aufnahme als WAV-Bytes (kommt im Frontend als ArrayBuffer an).
#[tauri::command]
pub async fn voicemail_audio(
    state: State<'_, AppState>,
    id: String,
) -> Result<tauri::ipc::Response, String> {
    let data = voicemail::download(&hub(&state).await?, &id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(tauri::ipc::Response::new(data))
}

#[tauri::command]
pub async fn voicemail_save(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Voicemail speichern")
        .set_file_name(format!("{name}.wav"))
        .add_filter("WAV-Audio", &["wav"])
        .save_file(move |f| {
            let _ = tx.send(f);
        });
    let Some(path) = rx.await.map_err(|e| e.to_string())? else {
        return Ok(false);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    let data = voicemail::download(&hub(&state).await?, &id)
        .await
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| format!("Nicht gespeichert: {e}"))?;
    Ok(true)
}

#[tauri::command]
pub async fn voicemail_move(
    state: State<'_, AppState>,
    id: String,
    folder: String,
) -> Result<(), String> {
    let folder = voicemail::folder_of(&folder).ok_or("Unbekannter Ordner")?;
    voicemail::move_to(&hub(&state).await?, &id, folder)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn voicemail_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    voicemail::delete(&hub(&state).await?, &id)
        .await
        .map_err(|e| e.to_string())
}

/// Die Anlage ruft das Softphone an und spielt die Nachricht vor.
#[tauri::command]
pub async fn voicemail_via_phone(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let phone_id = state
        .phone
        .lock()
        .await
        .as_ref()
        .map(|p| p.phone_id().to_owned())
        .ok_or("Das Softphone ist nicht aktiv.")?;
    voicemail::play_via_phone(&hub(&state).await?, &id, &phone_id)
        .await
        .map_err(|e| e.to_string())
}
