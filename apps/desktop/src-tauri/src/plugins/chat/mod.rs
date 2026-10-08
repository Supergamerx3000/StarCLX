//! Chat-Anbindung der Oberfläche: startet [`sf_chat::Chat`] zur Sitzung und
//! reicht Ereignisse als Tauri-Events weiter.

use serde::Serialize;
use sf_chat::{Chat, ChatEvent, ChatMessage, Contact, Transfer, TransferState};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::{Mutex, mpsc};

use crate::i18n::{t, tf};

#[derive(Default)]
pub struct ChatState {
    pub chat: Mutex<Option<Chat>>,
    status: std::sync::Mutex<ChatStatus>,
    /// Zuletzt gesendeter eigener Status; was die Anlage anders meldet, kam
    /// von einem anderen Client
    last_sent: std::sync::Mutex<Option<(sf_chat::Availability, String)>>,
    /// Dateiübertragungen dieser Sitzung
    transfers: std::sync::Mutex<std::collections::BTreeMap<String, Transfer>>,
}

#[derive(Clone, Default, Serialize)]
pub struct ChatStatus {
    online: bool,
    detail: String,
    own: String,
    contacts: Vec<Contact>,
}

/// Startet den Chat für die Sitzung (im Hintergrund, Fehler nur als Status).
pub fn session_started(app: &AppHandle, hub: sf_onehub::OneHub, host: String, user_id: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        set_status(&app, |s| {
            *s = ChatStatus {
                detail: t("Verbinde …").into(),
                ..Default::default()
            }
        });
        let jid = match hub.chat_jid(&user_id).await {
            Ok(jid) if !jid.is_empty() => jid,
            Ok(_) => {
                return set_status(&app, |s| {
                    s.detail = t("Kein Chat-Konto auf der Anlage").into()
                });
            }
            Err(e) => {
                let detail = tf("Chat nicht verfügbar: {e}", &[("e", &e.to_string())]);
                return set_status(&app, |s| s.detail = detail);
            }
        };
        let file = app
            .path()
            .app_data_dir()
            .ok()
            .map(|d| d.join(format!("chat-{}.json", jid.replace(['/', '\\'], "_"))));
        let token = hub.token().clone();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let chat = match Chat::start(&jid, &host, move || token.get(), file, tx) {
            Ok(c) => c,
            Err(e) => return set_status(&app, |s| s.detail = e.to_string()),
        };
        if app
            .state::<crate::AppState>()
            .session
            .lock()
            .await
            .is_none()
        {
            return; // inzwischen abgemeldet
        }
        set_status(&app, |s| s.own = jid);
        // Erst den Stand der Anlage übernehmen, damit der Verbindungsaufbau
        // keinen Status überschreibt, der anderswo gesetzt wurde.
        match sf_core::fkeys::current_state(&hub, &user_id).await {
            Ok(Some(s)) => sync_own(&app, &s),
            Ok(None) => {}
            Err(e) => tracing::warn!(error = %e, "eigener Status nicht abgefragt"),
        }
        let (availability, text) = own(&crate::settings::load(&app).prefs, crate::presence::away());
        *app.state::<ChatState>().last_sent.lock().unwrap() = Some((availability, text.clone()));
        chat.set_presence(availability, &text);
        *app.state::<ChatState>().chat.lock().await = Some(chat);
        while let Some(ev) = rx.recv().await {
            match ev {
                ChatEvent::State { online, detail } => set_status(&app, |s| {
                    s.online = online;
                    s.detail = detail;
                }),
                ChatEvent::Roster { contacts } => set_status(&app, |s| s.contacts = contacts),
                ChatEvent::Message { message, notify } => {
                    if notify {
                        notify_message(&app, &message);
                    }
                    let _ = app.emit("chat-message", message);
                }
                ChatEvent::History { peer, messages } => {
                    let _ = app.emit("chat-history", (peer, messages));
                }
                ChatEvent::Transfer { transfer } => {
                    if transfer.state == TransferState::Offered {
                        notify_offer(&app, &transfer);
                    }
                    app.state::<ChatState>()
                        .transfers
                        .lock()
                        .unwrap()
                        .insert(transfer.id.clone(), transfer.clone());
                    let _ = app.emit("chat-transfer", transfer);
                }
            }
        }
    });
}

/// Eigener Status aus der Auswahl und der automatischen Abwesenheit: Die
/// greift nur, solange man „Verfügbar“ gewählt hat, und nimmt dann den Text
/// für Abwesenheit, sonst den eigenen.
pub fn own(prefs: &crate::settings::Prefs, auto_away: bool) -> (sf_chat::Availability, String) {
    use sf_chat::Availability as A;
    let chosen = match prefs.chat_availability.as_str() {
        "away" => A::Away,
        "dnd" => A::DoNotDisturb,
        _ => A::Available,
    };
    if chosen == A::Available && auto_away {
        let text = if prefs.away_text.is_empty() {
            &prefs.chat_text
        } else {
            &prefs.away_text
        };
        return (A::Away, text.clone());
    }
    (chosen, prefs.chat_text.clone())
}

/// Schickt den aktuellen eigenen Status an den Chat.
pub async fn apply_own(app: &AppHandle) {
    let (availability, text) = own(&crate::settings::load(app).prefs, crate::presence::away());
    let state = app.state::<ChatState>();
    if let Some(chat) = state.chat.lock().await.as_ref() {
        *state.last_sent.lock().unwrap() = Some((availability, text.clone()));
        chat.set_presence(availability, &text);
    }
}

fn availability_of(chat: &str) -> Option<sf_chat::Availability> {
    use sf_chat::Availability as A;
    match chat {
        "available" => Some(A::Available),
        "away" => Some(A::Away),
        "dnd" => Some(A::DoNotDisturb),
        _ => None,
    }
}

fn availability_name(a: sf_chat::Availability) -> &'static str {
    use sf_chat::Availability as A;
    match a {
        A::Available => "available",
        A::Away => "away",
        A::DoNotDisturb => "dnd",
    }
}

/// Übernimmt den eigenen Status, wie die Anlage ihn meldet, wenn ihn ein
/// anderer Client (z. B. die STARFACE-App) gesetzt hat. Während der
/// automatischen Abwesenheit meldet die Anlage unseren eigenen Wert.
pub fn sync_own(app: &AppHandle, s: &sf_core::fkeys::UserState) {
    if crate::presence::away() {
        return;
    }
    let Some(availability) = availability_of(s.chat) else {
        return;
    };
    let server = (availability, s.chat_message.clone());
    {
        let state = app.state::<ChatState>();
        let mut last = state.last_sent.lock().unwrap();
        if last.as_ref() == Some(&server) {
            return;
        }
        *last = Some(server.clone());
    }
    crate::settings::update(app, |st| {
        st.prefs.chat_availability = availability_name(server.0).into();
        st.prefs.chat_text = server.1;
    });
}

/// Eigenen Status wählen (Menü am Profilbild). Ein eigener Text landet in
/// der Liste der gespeicherten Status. Liefert die Liste zurück.
#[tauri::command]
pub async fn chat_set_own(
    app: AppHandle,
    availability: String,
    text: String,
) -> Result<Vec<crate::settings::ChatPreset>, String> {
    if availability_of(&availability).is_none() {
        return Err(format!("unbekannter Status: {availability}"));
    }
    let text = text.trim().to_owned();
    crate::settings::update(&app, |s| {
        let preset = crate::settings::ChatPreset {
            availability: availability.clone(),
            text: text.clone(),
        };
        if !text.is_empty() && !s.prefs.chat_presets.contains(&preset) {
            s.prefs.chat_presets.push(preset);
        }
        s.prefs.chat_availability = availability;
        s.prefs.chat_text = text;
    });
    apply_own(&app).await;
    Ok(crate::settings::load(&app).prefs.chat_presets)
}

/// Gespeicherten Status löschen; liefert die übrige Liste.
#[tauri::command]
pub fn chat_delete_preset(
    app: AppHandle,
    availability: String,
    text: String,
) -> Vec<crate::settings::ChatPreset> {
    crate::settings::update(&app, |s| {
        s.prefs
            .chat_presets
            .retain(|p| !(p.availability == availability && p.text == text));
    });
    crate::settings::load(&app).prefs.chat_presets
}

/// Beendet den Chat (Abmelden) und hinterlässt den Statustext.
pub async fn session_ended(app: &AppHandle) {
    let chat = app.state::<ChatState>().chat.lock().await.take();
    if let Some(chat) = chat {
        chat.shutdown(&crate::settings::load(app).prefs.offline_text)
            .await;
    }
    set_status(app, |s| *s = ChatStatus::default());
    app.state::<ChatState>().transfers.lock().unwrap().clear();
}

fn set_status(app: &AppHandle, f: impl FnOnce(&mut ChatStatus)) {
    let status = {
        let state = app.state::<ChatState>();
        let mut s = state.status.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    let _ = app.emit("chat-status", status);
}

fn notify_message(app: &AppHandle, m: &ChatMessage) {
    let body: String = m.body.chars().take(140).collect();
    notify(app, &m.peer, body);
}

fn notify_offer(app: &AppHandle, t: &Transfer) {
    notify(
        app,
        &t.peer,
        tf("Datei angeboten: {name}", &[("name", &t.name)]),
    );
}

/// Ton und Benachrichtigung, wenn das Fenster nicht im Vordergrund ist
fn notify(app: &AppHandle, peer: &str, body: String) {
    let focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    if focused {
        return;
    }
    let prefs = crate::settings::load(app).prefs;
    if prefs.chat_sound {
        crate::audio::play_message_tone(app, &prefs);
    }
    if !prefs.chat_notify {
        return;
    }
    let name = app
        .state::<ChatState>()
        .status
        .lock()
        .unwrap()
        .contacts
        .iter()
        .find(|c| c.jid == peer)
        .map_or_else(|| peer.to_owned(), |c| c.name.clone());
    if let Err(e) = app.notification().builder().title(name).body(body).show() {
        tracing::warn!(error = %e, "Benachrichtigung nicht angezeigt");
    }
}

#[tauri::command]
pub fn chat_status(state: State<'_, ChatState>) -> ChatStatus {
    state.status.lock().unwrap().clone()
}

/// Letzte Nachricht je Gespräch, neueste zuerst
#[tauri::command]
pub async fn chat_recent(state: State<'_, ChatState>) -> Result<Vec<ChatMessage>, String> {
    Ok(state
        .chat
        .lock()
        .await
        .as_ref()
        .map(Chat::recent)
        .unwrap_or_default())
}

#[tauri::command]
pub async fn chat_conversation(
    state: State<'_, ChatState>,
    peer: String,
) -> Result<Vec<ChatMessage>, String> {
    let chat = state.chat.lock().await;
    let chat = chat.as_ref().ok_or(t("Chat ist nicht verbunden"))?;
    Ok(chat.conversation(&peer))
}

#[tauri::command]
pub async fn chat_send(
    state: State<'_, ChatState>,
    peer: String,
    body: String,
) -> Result<(), String> {
    if body.trim().is_empty() {
        return Ok(());
    }
    let chat = state.chat.lock().await;
    let chat = chat.as_ref().ok_or(t("Chat ist nicht verbunden"))?;
    chat.send(&peer, body.trim_end());
    Ok(())
}

/// Standardordner für empfangene Dateien
#[tauri::command]
pub fn default_download_dir(app: AppHandle) -> String {
    app.path()
        .download_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Ordner für empfangene Dateien aus den Einstellungen, sonst Downloads
fn download_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = crate::settings::load(app).prefs.download_dir;
    if !dir.trim().is_empty() {
        return Ok(dir.trim().into());
    }
    app.path()
        .download_dir()
        .map_err(|_| t("Kein Ordner für empfangene Dateien").to_owned())
}

/// Übertragungen dieser Sitzung (für die Oberfläche nach dem Laden)
#[tauri::command]
pub fn chat_transfers(state: State<'_, ChatState>) -> Vec<Transfer> {
    state.transfers.lock().unwrap().values().cloned().collect()
}

/// Dateiauswahl zum Senden
#[tauri::command]
pub async fn chat_pick_files(app: AppHandle) -> Result<Vec<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(t("Dateien senden"))
        .pick_files(move |f| {
            let _ = tx.send(f);
        });
    let files = rx.await.map_err(|e| e.to_string())?.unwrap_or_default();
    Ok(files
        .into_iter()
        .filter_map(|f| f.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect())
}

#[tauri::command]
pub async fn chat_send_files(
    state: State<'_, ChatState>,
    peer: String,
    paths: Vec<String>,
) -> Result<(), String> {
    let chat = state.chat.lock().await;
    let chat = chat.as_ref().ok_or(t("Chat ist nicht verbunden"))?;
    for path in paths {
        chat.send_file(&peer, path.into());
    }
    Ok(())
}

#[tauri::command]
pub async fn chat_accept_file(
    app: AppHandle,
    state: State<'_, ChatState>,
    id: String,
) -> Result<(), String> {
    let dir = download_dir(&app)?;
    let chat = state.chat.lock().await;
    let chat = chat.as_ref().ok_or(t("Chat ist nicht verbunden"))?;
    chat.accept_file(&id, dir);
    Ok(())
}

#[tauri::command]
pub async fn chat_decline_file(state: State<'_, ChatState>, id: String) -> Result<(), String> {
    let chat = state.chat.lock().await;
    let chat = chat.as_ref().ok_or(t("Chat ist nicht verbunden"))?;
    chat.decline_file(&id);
    Ok(())
}

#[tauri::command]
pub async fn chat_cancel_file(state: State<'_, ChatState>, id: String) -> Result<(), String> {
    let chat = state.chat.lock().await;
    let chat = chat.as_ref().ok_or(t("Chat ist nicht verbunden"))?;
    chat.cancel_file(&id);
    Ok(())
}

/// Pfad einer fertigen Übertragung; andere Pfade öffnet die Oberfläche nicht.
fn transfer_path(state: &ChatState, id: &str) -> Result<String, String> {
    state
        .transfers
        .lock()
        .unwrap()
        .get(id)
        .filter(|t| t.state == TransferState::Done && !t.path.is_empty())
        .map(|t| t.path.clone())
        .ok_or_else(|| t("Datei nicht gefunden").to_owned())
}

/// Öffnet eine übertragene Datei mit dem Standardprogramm.
#[tauri::command]
pub fn chat_open_file(
    app: AppHandle,
    state: State<'_, ChatState>,
    id: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(transfer_path(&state, &id)?, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Zeigt eine übertragene Datei im Dateimanager.
#[tauri::command]
pub fn chat_show_file(
    app: AppHandle,
    state: State<'_, ChatState>,
    id: String,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .reveal_item_in_dir(transfer_path(&state, &id)?)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pick_download_dir(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(t("Ordner für empfangene Dateien"))
        .pick_folder(move |f| {
            let _ = tx.send(f);
        });
    let Some(dir) = rx.await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let path = dir.into_path().map_err(|e| e.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_chat::Availability as A;

    fn prefs(availability: &str, text: &str, away_text: &str) -> crate::settings::Prefs {
        crate::settings::Prefs {
            chat_availability: availability.into(),
            chat_text: text.into(),
            away_text: away_text.into(),
            ..Default::default()
        }
    }

    #[test]
    fn chosen_status_and_text() {
        assert_eq!(
            own(&prefs("available", "", ""), false),
            (A::Available, String::new())
        );
        assert_eq!(
            own(&prefs("away", "Mittag", ""), false),
            (A::Away, "Mittag".into())
        );
        assert_eq!(
            own(&prefs("dnd", "", ""), false),
            (A::DoNotDisturb, String::new())
        );
        assert_eq!(own(&prefs("quatsch", "", ""), false).0, A::Available);
    }

    #[test]
    fn auto_away_only_when_available() {
        // Automatisch abwesend: Text für Abwesenheit, sonst der eigene
        assert_eq!(
            own(&prefs("available", "Im Büro", "Gleich zurück"), true),
            (A::Away, "Gleich zurück".into())
        );
        assert_eq!(
            own(&prefs("available", "Im Büro", ""), true),
            (A::Away, "Im Büro".into())
        );
        // Eine eigene Wahl bleibt
        assert_eq!(
            own(&prefs("dnd", "Meeting", "Gleich zurück"), true),
            (A::DoNotDisturb, "Meeting".into())
        );
    }
}
