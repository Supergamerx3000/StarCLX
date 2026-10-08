//! Funktionstasten: Bearbeiten über die REST-API der Anlage, Zustände
//! (Besetztlampenfeld, Ruhe) über die OneHub-Präsenz, Gruppen-Anmeldung über
//! den GroupService.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use sf_core::fkeys::{FunctionKey, Keys, Presence, Rest, UserState};
use sf_core::group::{Groups, Membership};
use sf_core::module::{Module, Modules};
use sf_core::redirect::RedirectTarget;
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
    modules: Mutex<Option<Modules>>,
    me_events: Mutex<Option<sf_core::account::MeEvents>>,
    module_list: std::sync::Mutex<Vec<Module>>,
    /// Benutzerbilder als data:-URL je User-ID; `None` = keins hinterlegt
    avatars: std::sync::Mutex<HashMap<String, Option<String>>>,
}

/// Beim Abmelden die Präsenz beenden.
pub async fn session_ended(app: &AppHandle) {
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
                    crate::plugins::chat::sync_own(&app, own);
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

/// Einstellungen einer Umleitung, bevor eine Taste sie programmiert hat
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedRedirect {
    pub target: RedirectTarget,
    pub timeout_secs: i64,
    pub enabled: bool,
}

/// Umleitung (Art) mit Zielabfrage. Die Anlage kennt das nicht; der Client
/// merkt sich, welche Tasten fragen und was vorher eingestellt war.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FkeyRedirects {
    /// Tasten-IDs, die beim Einschalten nach dem Ziel fragen
    pub ask: BTreeSet<String>,
    /// Umleitungs-ID → Einstellungen vor dem Programmieren
    pub saved: BTreeMap<String, SavedRedirect>,
}

#[derive(Serialize)]
pub struct RedirectOptions {
    ask: Vec<String>,
    /// Umleitungen, die gerade über eine Taste programmiert sind
    programmed: Vec<String>,
}

#[tauri::command]
pub fn fkey_redirect_options(app: AppHandle) -> RedirectOptions {
    let r = crate::settings::load(&app).fkey_redirects;
    RedirectOptions {
        ask: r.ask.into_iter().collect(),
        programmed: r.saved.into_keys().collect(),
    }
}

#[tauri::command]
pub fn fkey_set_ask(app: AppHandle, key_id: String, ask: bool) {
    crate::settings::update(&app, |s| {
        if ask {
            s.fkey_redirects.ask.insert(key_id);
        } else {
            s.fkey_redirects.ask.remove(&key_id);
        }
    });
}

/// Leitet die Umleitungen `ids` auf `number` um und schaltet sie ein. Was
/// vorher eingestellt war, bleibt für [`fkey_redirect_restore`] gespeichert;
/// beim erneuten Programmieren gilt weiter der erste Stand.
#[tauri::command]
pub async fn fkey_redirect_program(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
    number: String,
    timeout_secs: Option<i64>,
) -> Result<(), String> {
    let number = number.trim().to_owned();
    if number.is_empty() {
        return Err(crate::i18n::t("Bitte eine Zielrufnummer eingeben.").into());
    }
    let hub = hub(&state).await?;
    let list = sf_core::redirect::redirects(&hub)
        .await
        .map_err(|e| e.to_string())?;
    let targets: Vec<_> = list.iter().filter(|r| ids.contains(&r.id)).collect();
    // Erst merken, dann ändern: bricht etwas ab, lässt sich trotzdem
    // zurückstellen.
    crate::settings::update(&app, |s| {
        for r in &targets {
            s.fkey_redirects
                .saved
                .entry(r.id.clone())
                .or_insert_with(|| SavedRedirect {
                    target: r.target.clone(),
                    timeout_secs: r.timeout_secs,
                    enabled: r.enabled,
                });
        }
    });
    let target = RedirectTarget {
        number: Some(number),
        mailbox: None,
    };
    for r in targets {
        let timeout = timeout_secs.filter(|_| r.kind == "timeout");
        sf_core::redirect::update_redirect(&hub, &r.id, &target, timeout)
            .await
            .map_err(|e| e.to_string())?;
        if !r.enabled {
            sf_core::redirect::set_redirect_enabled(&hub, &r.id, true)
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Stellt die Umleitungen `ids` wieder so ein wie vor dem Programmieren;
/// ohne gespeicherten Stand werden sie nur ausgeschaltet.
#[tauri::command]
pub async fn fkey_redirect_restore(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> Result<(), String> {
    let hub = hub(&state).await?;
    let list = sf_core::redirect::redirects(&hub)
        .await
        .map_err(|e| e.to_string())?;
    for r in list.iter().filter(|r| ids.contains(&r.id)) {
        let saved = crate::settings::load(&app)
            .fkey_redirects
            .saved
            .get(&r.id)
            .cloned();
        let enabled = match &saved {
            Some(s) => {
                if s.target != RedirectTarget::default() {
                    let timeout =
                        (r.kind == "timeout" && s.timeout_secs > 0).then_some(s.timeout_secs);
                    sf_core::redirect::update_redirect(&hub, &r.id, &s.target, timeout)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                s.enabled
            }
            None => false,
        };
        if r.enabled != enabled {
            sf_core::redirect::set_redirect_enabled(&hub, &r.id, enabled)
                .await
                .map_err(|e| e.to_string())?;
        }
        crate::settings::update(&app, |s| {
            s.fkey_redirects.saved.remove(&r.id);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirect_options_survive_old_settings() {
        let s: crate::settings::Settings = serde_json::from_str(r#"{"last_server":"x"}"#).unwrap();
        assert_eq!(s.fkey_redirects, FkeyRedirects::default());
        let r: FkeyRedirects = serde_json::from_str(
            r#"{"ask":["1001"],"saved":{"r1":{"target":{"number":"0791234567","mailbox":null},"timeout_secs":20,"enabled":false}}}"#,
        )
        .unwrap();
        assert!(r.ask.contains("1001"));
        assert_eq!(r.saved["r1"].target.number.as_deref(), Some("0791234567"));
    }

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
