//! Funktionstasten: Bearbeiten über die REST-API der Anlage, Zustände
//! (Besetztlampenfeld, Ruhe) über die OneHub-Präsenz, Gruppen-Anmeldung über
//! den GroupService.

use std::collections::HashMap;

use sf_core::fkeys::{FunctionKey, Keys, Presence, Rest, UserState};
use sf_core::group::{Groups, Membership};
use sf_core::module::{Module, Modules};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, mpsc};

use crate::{AppState, hub};

#[derive(Default)]
pub struct FkeyState {
    presence: Mutex<Option<Presence>>,
    states: std::sync::Mutex<HashMap<String, UserState>>,
    groups: Mutex<Option<Groups>>,
    memberships: std::sync::Mutex<Vec<Membership>>,
    modules: Mutex<Option<Modules>>,
    me_events: Mutex<Option<sf_core::account::MeEvents>>,
    module_list: std::sync::Mutex<Vec<Module>>,
    /// Benutzerbilder als data:-URL je User-ID; `None` = keins hinterlegt
    avatars: std::sync::Mutex<HashMap<String, Option<String>>>,
}

/// Beim Abmelden die Präsenz beenden.
pub async fn stop(app: &AppHandle) {
    let state = app.state::<FkeyState>();
    state.presence.lock().await.take();
    state.states.lock().unwrap().clear();
    state.groups.lock().await.take();
    state.memberships.lock().unwrap().clear();
    state.modules.lock().await.take();
    state.me_events.lock().await.take();
    state.module_list.lock().unwrap().clear();
    state.avatars.lock().unwrap().clear();
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
    for k in keys
        .keys
        .iter()
        .filter(|k| k.function_key_type == "SIGNALNUMBER")
    {
        tracing::debug!(name = %k.name, display_number_id = ?k.display_number_id, "Rufnummer-Taste");
    }
    // Geänderte Benutzerbilder beim nächsten Laden neu holen
    fk.avatars.lock().unwrap().clear();
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
    // Besetztlampenfelder ohne User sind Gruppen
    let blf: Vec<i32> = keys.keys.iter().filter_map(|k| k.blf_account_id).collect();
    let no_user: Vec<sf_core::fkeys::Account> = keys
        .accounts
        .iter()
        .filter(|a| a.user_ids.is_empty() && blf.contains(&a.account_id))
        .cloned()
        .collect();
    let mut groups = Vec::new();
    if !no_user.is_empty() {
        match sf_core::fkeys::group_ids(&hub, &no_user).await {
            Ok(map) => {
                for a in &mut keys.accounts {
                    if let Some(g) = map.get(&a.account_id) {
                        a.user_ids = vec![g.clone()];
                        a.group = true;
                        groups.push(g.clone());
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "Gruppen für Funktionstasten nicht ermittelt"),
        }
    }
    let mut users: Vec<String> = keys
        .keys
        .iter()
        .filter_map(|k| k.blf_account_id)
        .filter_map(|id| keys.accounts.iter().find(|a| a.account_id == id))
        .filter(|a| !a.group)
        .flat_map(|a| a.user_ids.iter().cloned())
        .collect();
    users.push(me.clone());
    users.sort();
    users.dedup();
    let (tx, mut rx) = mpsc::unbounded_channel();
    *fk.presence.lock().await = Some(Presence::start(hub.clone(), users, groups, tx));
    {
        let app = app.clone();
        let me = me.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(states) = rx.recv().await {
                if let Some(own) = states.get(&me) {
                    crate::chat::sync_own(&app, own);
                }
                *app.state::<FkeyState>().states.lock().unwrap() = states.clone();
                let _ = app.emit("fkey-presence", states);
            }
        });
    }
    // Gruppen nur verfolgen, wenn es eine Taste dafür gibt
    // Eigene Einstellungen (Rufnummer, Telefon, Bild, Rechte) live nachziehen
    {
        use sf_core::account::MeChange;
        let (tx, mut rx) = mpsc::unbounded_channel::<MeChange>();
        let app = app.clone();
        let me = me.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(change) = rx.recv().await {
                let name = match change {
                    MeChange::Signaling => "me-signaling",
                    MeChange::Phones => "me-phones",
                    MeChange::Avatar => {
                        app.state::<FkeyState>().avatars.lock().unwrap().remove(&me);
                        "me-avatar"
                    }
                    MeChange::Permission => "me-permission",
                };
                let _ = app.emit(name, ());
            }
        });
        *fk.me_events.lock().await = Some(sf_core::account::MeEvents::start(hub.clone(), tx));
    }
    // Module nur verfolgen, wenn es eine Taste dafür gibt
    let has_module_key = keys
        .keys
        .iter()
        .any(|k| k.function_key_type == "MODULEACTIVATION");
    *fk.modules.lock().await = has_module_key.then(|| {
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<Module>>();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(list) = rx.recv().await {
                *app.state::<FkeyState>().module_list.lock().unwrap() = list.clone();
                let _ = app.emit("fkey-modules", list);
            }
        });
        Modules::start(hub.clone(), tx)
    });
    let has_group_key = keys
        .keys
        .iter()
        .any(|k| k.function_key_type == "GROUPLOGIN");
    *fk.groups.lock().await = has_group_key.then(|| {
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<Membership>>();
        tauri::async_runtime::spawn(async move {
            while let Some(list) = rx.recv().await {
                *app.state::<FkeyState>().memberships.lock().unwrap() = list.clone();
                let _ = app.emit("fkey-groups", list);
            }
        });
        Groups::start(hub, tx)
    });
    Ok(keys)
}

/// Benutzerbild als data:-URL, zwischengespeichert bis zum nächsten Laden
/// der Tasten. `None`, wenn keins hinterlegt ist.
#[tauri::command]
pub async fn fkey_avatar(
    state: State<'_, AppState>,
    fk: State<'_, FkeyState>,
    user_id: String,
) -> Result<Option<String>, String> {
    if let Some(url) = fk.avatars.lock().unwrap().get(&user_id) {
        return Ok(url.clone());
    }
    let hub = hub(&state).await?;
    let url = hub
        .avatar(&user_id)
        .await
        .map_err(|e| e.to_string())?
        .map(|data| data_url(&data));
    fk.avatars.lock().unwrap().insert(user_id, url.clone());
    Ok(url)
}

/// Bilddatei als data:-URL; das Format steht in den ersten Bytes.
fn data_url(data: &[u8]) -> String {
    use base64::Engine;
    let mime = if data.starts_with(b"\x89PNG") {
        "image/png"
    } else if data.starts_with(b"GIF8") {
        "image/gif"
    } else if data.len() > 12 && &data[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    format!("data:{mime};base64,{b64}")
}

/// Letzter bekannter Zustand (User-ID → Telefon, Ruhe, Chat, Umleitung)
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

/// Letzter bekannter Stand der Gruppen-Mitgliedschaften
#[tauri::command]
pub fn fkey_groups(fk: State<'_, FkeyState>) -> Vec<Membership> {
    fk.memberships.lock().unwrap().clone()
}

/// Letzter bekannter Stand der Module
#[tauri::command]
pub fn fkey_modules(fk: State<'_, FkeyState>) -> Vec<Module> {
    fk.module_list.lock().unwrap().clone()
}

/// Schaltet die Module einer Taste: sind alle aktiv, werden sie
/// abgeschaltet, sonst eingeschaltet. Liefert den neuen Zustand.
#[tauri::command]
pub async fn fkey_module_toggle(
    state: State<'_, AppState>,
    module_ids: Vec<String>,
) -> Result<bool, String> {
    let hub = hub(&state).await?;
    let list = sf_core::module::modules(&hub).await.map_err(module_error)?;
    let targets: Vec<&Module> = list.iter().filter(|m| module_ids.contains(&m.id)).collect();
    if targets.is_empty() {
        return Err(crate::i18n::t("Modul nicht gefunden oder nicht freigegeben").into());
    }
    if targets.iter().any(|m| m.read_only) {
        return Err(crate::i18n::t("Dieses Modul darf nicht geschaltet werden").into());
    }
    let on = !targets.iter().all(|m| m.active);
    for m in targets {
        sf_core::module::set_active(&hub, &m.id, on)
            .await
            .map_err(module_error)?;
    }
    Ok(on)
}

/// Fehlendes Recht als verständliche Meldung
fn module_error(e: sf_onehub::Error) -> String {
    match e.permission_denied() {
        Some(_) => crate::i18n::t("Keine Berechtigung, Module zu schalten").into(),
        None => e.to_string(),
    }
}

/// Gruppen einer Taste: über die IDs der Taste, sonst über den Namen in
/// `Gruppe[Name]`, wie ihn die Anlage für die Taste vergibt.
fn key_groups<'a>(list: &'a [Membership], ids: &[i32], key_name: &str) -> Vec<&'a Membership> {
    let by_id: Vec<_> = list
        .iter()
        .filter(|m| ids.iter().any(|id| sf_core::group::matches(m, *id)))
        .collect();
    if !by_id.is_empty() {
        return by_id;
    }
    let name = key_name
        .split_once('[')
        .and_then(|(_, rest)| rest.strip_suffix(']'))
        .unwrap_or(key_name);
    list.iter().filter(|m| m.name == name).collect()
}

/// Gruppen-Taste: ist man in einer der Gruppen angemeldet, von allen
/// abmelden, sonst bei allen anmelden. Gibt den neuen Zustand zurück.
#[tauri::command]
pub async fn fkey_group_toggle(
    state: State<'_, AppState>,
    group_ids: Vec<i32>,
    key_name: String,
) -> Result<bool, String> {
    let hub = hub(&state).await?;
    let list = sf_core::group::memberships(&hub)
        .await
        .map_err(|e| e.to_string())?;
    let targets = key_groups(&list, &group_ids, &key_name);
    if targets.is_empty() {
        return Err(crate::i18n::t("Gruppe nicht gefunden oder kein Mitglied").into());
    }
    if let Some(m) = targets.iter().find(|m| m.read_only) {
        return Err(crate::i18n::tf(
            "Die Anmeldung in „{name}“ lässt sich nicht ändern",
            &[("name", &m.name)],
        ));
    }
    let on = !targets.iter().any(|m| m.logged_on);
    // Nur Gruppen umschalten, die noch nicht im Zielzustand sind
    for m in targets.into_iter().filter(|m| m.logged_on != on) {
        sf_core::group::set_logged_on(&hub, &m.id, on)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(on)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str, logon: &str, name: &str) -> Membership {
        Membership {
            id: id.into(),
            logon_id: logon.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn group_key_resolution() {
        let list = [m("a", "4711", "DSS Zentrale"), m("b", "4712", "Support")];
        let ids = |v: Vec<&Membership>| v.iter().map(|m| m.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(key_groups(&list, &[4712], "egal")), ["b"]);
        assert_eq!(ids(key_groups(&list, &[1], "Gruppe[DSS Zentrale]")), ["a"]);
        assert!(key_groups(&list, &[1], "Gruppe[Fremd]").is_empty());
    }
}
