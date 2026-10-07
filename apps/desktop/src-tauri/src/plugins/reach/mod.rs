//! Erreichbarkeit: Umleitungen, Parallelruf (iFMC) und Voicemail-Ansage.
//! Änderungen gehen direkt an die Anlage; die Oberfläche lädt nach jedem
//! Ereignis ("reach-changed") neu.

use sf_core::redirect::{self, FmcPhone, Mailbox, Redirect, RedirectTarget, Watcher};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, mpsc};

use crate::i18n::t;
use crate::{AppState, hub};

#[derive(Default)]
pub struct ReachState {
    watcher: Mutex<Option<Watcher>>,
}

pub async fn session_ended(app: &AppHandle) {
    restart(app, None).await;
}

pub async fn session_started(app: &AppHandle, hub: sf_onehub::OneHub) {
    restart(app, Some(hub)).await;
}

async fn restart(app: &AppHandle, hub: Option<sf_onehub::OneHub>) {
    let state = app.state::<ReachState>();
    let mut watcher = state.watcher.lock().await;
    *watcher = hub.map(|hub| {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while rx.recv().await.is_some() {
                let _ = app.emit("reach-changed", ());
            }
        });
        Watcher::start(hub, tx)
    });
}

#[tauri::command]
pub async fn redirects(state: State<'_, AppState>) -> Result<Vec<Redirect>, String> {
    redirect::redirects(&hub(&state).await?)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn redirect_enable(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    redirect::set_redirect_enabled(&hub(&state).await?, &id, enabled)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn redirect_update(
    state: State<'_, AppState>,
    id: String,
    target: RedirectTarget,
    timeout_secs: Option<i64>,
) -> Result<(), String> {
    redirect::update_redirect(&hub(&state).await?, &id, &target, timeout_secs)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fmc_phones(state: State<'_, AppState>) -> Result<Vec<FmcPhone>, String> {
    redirect::fmc_phones(&hub(&state).await?)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fmc_save(state: State<'_, AppState>, phone: FmcPhone) -> Result<(), String> {
    if phone.number.trim().is_empty() {
        return Err(t("Bitte eine Rufnummer eingeben.").into());
    }
    redirect::save_fmc_phone(&hub(&state).await?, &phone)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fmc_enable(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    redirect::set_fmc_enabled(&hub(&state).await?, &id, enabled)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fmc_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    redirect::delete_fmc_phone(&hub(&state).await?, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mailboxes(state: State<'_, AppState>) -> Result<Vec<Mailbox>, String> {
    redirect::mailboxes(&hub(&state).await?)
        .await
        .map_err(|e| e.to_string())
}

/// Die Anlage ruft das Softphone an und verbindet mit dem Menü der Box.
#[tauri::command]
pub async fn mailbox_record(
    app: AppHandle,
    state: State<'_, AppState>,
    mailbox: String,
) -> Result<(), String> {
    let phone_id = crate::plugins::call::softphone_id(&app)
        .await
        .ok_or(t("Das Softphone ist nicht aktiv."))?;
    redirect::call_mailbox(&hub(&state).await?, &mailbox, &phone_id)
        .await
        .map_err(|e| e.to_string())
}
