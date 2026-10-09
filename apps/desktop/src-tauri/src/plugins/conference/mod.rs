//! Plugin Konferenzen: geplante Konferenzen der Anlage anlegen, ändern,
//! löschen und starten.

use sf_core::conference::{self, Conference, ConferenceEvent, Draft, Invalid, SaveError, Watcher};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, mpsc};

use crate::i18n::t;
use crate::{AppState, hub};

#[derive(Default)]
pub struct ConferenceState {
    watcher: Mutex<Option<Watcher>>,
}

pub async fn session_ended(app: &AppHandle) {
    restart(app, None).await;
}

pub async fn session_started(app: &AppHandle, hub: sf_onehub::OneHub) {
    restart(app, Some(hub)).await;
}

async fn restart(app: &AppHandle, hub: Option<sf_onehub::OneHub>) {
    let state = app.state::<ConferenceState>();
    *state.watcher.lock().await = hub.map(|hub| {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(ConferenceEvent::Changed) = rx.recv().await {
                let _ = app.emit("conferences-changed", ());
            }
        });
        Watcher::start(hub, tx)
    });
}

fn hub_error(e: &sf_onehub::Error) -> String {
    match e.permission_denied() {
        Some(_) => t("Keine Berechtigung für Konferenzen").to_owned(),
        None => e.to_string(),
    }
}

#[tauri::command]
pub async fn conferences(state: State<'_, AppState>) -> Result<Vec<Conference>, String> {
    conference::list(&hub(&state).await?)
        .await
        .map_err(|e| hub_error(&e))
}

#[tauri::command]
pub async fn conference_save(state: State<'_, AppState>, draft: Draft) -> Result<(), String> {
    conference::save(&hub(&state).await?, draft)
        .await
        .map_err(|e| match e {
            SaveError::Invalid(Invalid::Name) => t("Bitte einen Namen eingeben").to_owned(),
            SaveError::Invalid(Invalid::Recurrence) => t("Unbekannte Wiederholung").to_owned(),
            SaveError::Invalid(Invalid::Participant) => {
                t("Jeder Teilnehmer braucht eine Nummer oder E-Mail-Adresse").to_owned()
            }
            SaveError::Hub(e) => hub_error(&e),
        })
}

#[tauri::command]
pub async fn conference_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    conference::delete(&hub(&state).await?, &id)
        .await
        .map_err(|e| hub_error(&e))
}

/// Startet die Konferenz; die Anlage holt das Softphone hinein.
#[tauri::command]
pub async fn conference_start(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let phone_id = crate::plugins::call::softphone_id(&app)
        .await
        .ok_or(t("Das Softphone ist nicht aktiv."))?;
    conference::start(&hub(&state).await?, &id, &phone_id)
        .await
        .map_err(|e| hub_error(&e))
}
