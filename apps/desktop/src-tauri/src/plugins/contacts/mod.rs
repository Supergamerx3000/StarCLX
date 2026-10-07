//! Plugin Adressbuch: Suche, Ordner, Kontakte anlegen, ändern und löschen.

use tauri::State;

use crate::i18n::t;
use crate::{AppState, hub};

#[tauri::command]
pub async fn contacts_search(
    state: State<'_, AppState>,
    term: String,
) -> Result<Vec<sf_core::directory::ContactView>, String> {
    if term.trim().chars().count() < 2 {
        return Ok(Vec::new());
    }
    sf_core::directory::search(&hub(&state).await?, &term, 8)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn contacts_folders(
    state: State<'_, AppState>,
) -> Result<Vec<sf_core::directory::Folder>, String> {
    let (hub, server) = state
        .session
        .lock()
        .await
        .as_ref()
        .map(|s| (s.hub().clone(), s.info().server.clone()))
        .ok_or("Nicht angemeldet")?;
    sf_core::directory::folders(&hub, &server)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn contacts_list(
    state: State<'_, AppState>,
    folder: String,
    term: String,
    offset: i32,
) -> Result<sf_core::directory::Page, String> {
    sf_core::directory::list(&hub(&state).await?, &folder, &term, offset, 50)
        .await
        .map_err(|e| e.to_string())
}

/// Formular für einen neuen (`id` leer) oder bestehenden Kontakt
#[tauri::command]
pub async fn contact_form(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<sf_core::contact_form::Field>, String> {
    let hub = hub(&state).await?;
    if id.is_empty() {
        sf_core::contact_form::empty(&hub).await
    } else {
        sf_core::contact_form::load(&hub, &id).await
    }
    .map_err(|e| e.to_string())
}

/// Speichert einen Kontakt: neu in `folder`, sonst Änderung an `id`
#[tauri::command]
pub async fn contact_save(
    state: State<'_, AppState>,
    id: String,
    folder: String,
    fields: Vec<sf_core::contact_form::Field>,
) -> Result<(), String> {
    if sf_core::contact_form::missing_name(&fields) {
        return Err(t("Bitte Nachname oder Firma ausfüllen.").into());
    }
    let hub = hub(&state).await?;
    if id.is_empty() {
        sf_core::contact_form::create(&hub, &folder, &fields).await
    } else {
        sf_core::contact_form::update(&hub, &id, &fields).await
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn contact_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    sf_core::contact_form::delete(&hub(&state).await?, &id)
        .await
        .map_err(|e| e.to_string())
}
