//! Funktionstasten: Bearbeiten über die REST-API der Anlage, Zustände
//! (Besetztlampenfeld, Ruhe) über die OneHub-Präsenz, Gruppen-Anmeldung über
//! den GroupService.

use std::collections::HashMap;

use sf_core::fkeys::{FunctionKey, Keys, Presence, Rest, UserState};
use sf_core::group::{Groups, Membership};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, mpsc};

use crate::plugins::call::softphone_id;
use crate::{AppState, hub};

#[derive(Default)]
pub struct FkeyState {
    presence: Mutex<Option<Presence>>,
    states: std::sync::Mutex<HashMap<String, UserState>>,
    groups: Mutex<Option<Groups>>,
    memberships: std::sync::Mutex<Vec<Membership>>,
}

/// Beim Abmelden die Präsenz beenden.
pub async fn stop(app: &AppHandle) {
    let state = app.state::<FkeyState>();
    state.presence.lock().await.take();
    state.states.lock().unwrap().clear();
    state.groups.lock().await.take();
    state.memberships.lock().unwrap().clear();
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
    *fk.presence.lock().await = Some(Presence::start(hub.clone(), users, tx));
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(states) = rx.recv().await {
                *app.state::<FkeyState>().states.lock().unwrap() = states.clone();
                let _ = app.emit("fkey-presence", states);
            }
        });
    }
    // Gruppen nur verfolgen, wenn es eine Taste dafür gibt
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

/// Letzter bekannter Stand der Gruppen-Mitgliedschaften
#[tauri::command]
pub fn fkey_groups(fk: State<'_, FkeyState>) -> Vec<Membership> {
    fk.memberships.lock().unwrap().clone()
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

/// Ohne `call_id` wird das auf `number` geparkte Gespräch zurückgeholt.
#[tauri::command]
pub async fn fkey_park(
    app: AppHandle,
    state: State<'_, AppState>,
    call_id: Option<String>,
    number: String,
) -> Result<(), String> {
    let phone = softphone_id(&app).await;
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
pub async fn fkey_grab(
    app: AppHandle,
    state: State<'_, AppState>,
    user_id: String,
) -> Result<(), String> {
    let phone = softphone_id(&app).await;
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
