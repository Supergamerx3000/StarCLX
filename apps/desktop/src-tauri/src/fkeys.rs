//! Funktionstasten: Bearbeiten über die REST-API der Anlage, Zustände
//! (Besetztlampenfeld, Ruhe) über die OneHub-Präsenz.

use std::collections::HashMap;

use sf_core::fkeys::{FunctionKey, Keys, Presence, Rest, UserState};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, mpsc};

use crate::{AppState, hub};

#[derive(Default)]
pub struct FkeyState {
    presence: Mutex<Option<Presence>>,
    states: std::sync::Mutex<HashMap<String, UserState>>,
}

/// Beim Abmelden die Präsenz beenden.
pub async fn stop(app: &AppHandle) {
    let state = app.state::<FkeyState>();
    state.presence.lock().await.take();
    state.states.lock().unwrap().clear();
}

async fn rest(state: &AppState) -> Result<(Rest, sf_onehub::OneHub, String), String> {
    let (hub, server, user) = state
        .session
        .lock()
        .await
        .as_ref()
        .map(|s| {
            (
                s.hub().clone(),
                s.info().server.clone(),
                s.info().user_id.clone(),
            )
        })
        .ok_or(crate::i18n::t("Nicht angemeldet"))?;
    let rest = Rest::new(&server, &hub).map_err(|e| e.to_string())?;
    Ok((rest, hub, user))
}

/// Lädt die Tasten und verfolgt die Zustände der darin vorkommenden User.
#[tauri::command]
pub async fn fkeys_load(
    app: AppHandle,
    state: State<'_, AppState>,
    fk: State<'_, FkeyState>,
) -> Result<Keys, String> {
    let (rest, hub, me) = rest(&state).await?;
    let mut keys = rest.load().await.map_err(|e| e.to_string())?;
    keys.me.clone_from(&me);
    let ids: Vec<i32> = keys.accounts.iter().map(|a| a.account_id).collect();
    match sf_core::fkeys::user_ids(&hub, &ids).await {
        Ok(map) => {
            for a in &mut keys.accounts {
                if let Some(u) = map.get(&a.account_id) {
                    a.user_ids.clone_from(u);
                }
            }
        }
        Err(e) => tracing::warn!(error = %e, "User-IDs für Funktionstasten nicht ermittelt"),
    }
    let mut users: Vec<String> = keys
        .keys
        .iter()
        .filter_map(|k| k.blf_account_id)
        .filter_map(|id| keys.accounts.iter().find(|a| a.account_id == id))
        .flat_map(|a| a.user_ids.iter().cloned())
        .collect();
    users.push(me);
    users.sort();
    users.dedup();
    let (tx, mut rx) = mpsc::unbounded_channel();
    *fk.presence.lock().await = Some(Presence::start(hub, users, tx));
    tauri::async_runtime::spawn(async move {
        while let Some(states) = rx.recv().await {
            *app.state::<FkeyState>().states.lock().unwrap() = states.clone();
            let _ = app.emit("fkey-presence", states);
        }
    });
    Ok(keys)
}

/// Letzter bekannter Zustand (User-ID → Telefonie/Ruhe)
#[tauri::command]
pub fn fkey_presence(fk: State<'_, FkeyState>) -> HashMap<String, UserState> {
    fk.states.lock().unwrap().clone()
}

#[tauri::command]
pub async fn fkey_save(
    state: State<'_, AppState>,
    set: String,
    key: FunctionKey,
) -> Result<(), String> {
    let (rest, ..) = rest(&state).await?;
    rest.save(&set, &key).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fkey_delete(
    state: State<'_, AppState>,
    set: String,
    id: String,
) -> Result<(), String> {
    let (rest, ..) = rest(&state).await?;
    rest.delete(&set, &id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fkeys_reorder(
    state: State<'_, AppState>,
    set: String,
    name: String,
    order: Vec<String>,
    keys: Vec<FunctionKey>,
) -> Result<(), String> {
    let (rest, ..) = rest(&state).await?;
    rest.reorder(&set, &name, &order, &keys)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fkey_dnd(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    sf_core::fkeys::set_dnd(&hub(&state).await?, enabled)
        .await
        .map_err(|e| e.to_string())
}

async fn softphone(state: &AppState) -> Option<String> {
    state
        .phone
        .lock()
        .await
        .as_ref()
        .map(|p| p.phone_id().to_owned())
}

/// Ohne `call_id` wird das auf `number` geparkte Gespräch zurückgeholt.
#[tauri::command]
pub async fn fkey_park(
    state: State<'_, AppState>,
    call_id: Option<String>,
    number: String,
) -> Result<(), String> {
    let phone = softphone(&state).await;
    sf_core::fkeys::park(
        &hub(&state).await?,
        call_id.as_deref(),
        &number,
        phone.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

/// Holt den Anruf heran, der beim überwachten User klingelt.
#[tauri::command]
pub async fn fkey_grab(state: State<'_, AppState>, user_id: String) -> Result<(), String> {
    let phone = softphone(&state).await;
    sf_core::fkeys::grab(&hub(&state).await?, &user_id, phone.as_deref())
        .await
        .map_err(|e| e.to_string())
}
