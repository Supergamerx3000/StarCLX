//! Chat-Anbindung der Oberfläche: startet [`sf_chat::Chat`] zur Sitzung und
//! reicht Ereignisse als Tauri-Events weiter.

use serde::Serialize;
use sf_chat::{Chat, ChatEvent, ChatMessage, Contact};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::{Mutex, mpsc};

#[derive(Default)]
pub struct ChatState {
    pub chat: Mutex<Option<Chat>>,
    status: std::sync::Mutex<ChatStatus>,
}

#[derive(Clone, Default, Serialize)]
pub struct ChatStatus {
    online: bool,
    detail: String,
    own: String,
    contacts: Vec<Contact>,
}

/// Startet den Chat für die Sitzung (im Hintergrund, Fehler nur als Status).
pub fn start(app: &AppHandle, hub: sf_onehub::OneHub, host: String, user_id: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        set_status(&app, |s| {
            *s = ChatStatus {
                detail: "Verbinde …".into(),
                ..Default::default()
            }
        });
        let jid = match hub.chat_jid(&user_id).await {
            Ok(jid) if !jid.is_empty() => jid,
            Ok(_) => {
                return set_status(&app, |s| s.detail = "Kein Chat-Konto auf der Anlage".into());
            }
            Err(e) => return set_status(&app, |s| s.detail = format!("Chat nicht verfügbar: {e}")),
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
        if crate::presence::away() {
            chat.set_presence(true, &crate::settings::load(&app).prefs.away_text);
        }
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
            }
        }
    });
}

/// Beendet den Chat (Abmelden) und hinterlässt den Statustext.
pub async fn stop(app: &AppHandle) {
    let chat = app.state::<ChatState>().chat.lock().await.take();
    if let Some(chat) = chat {
        chat.shutdown(&crate::settings::load(app).prefs.offline_text)
            .await;
    }
    set_status(app, |s| *s = ChatStatus::default());
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
        .find(|c| c.jid == m.peer)
        .map_or_else(|| m.peer.clone(), |c| c.name.clone());
    let body: String = m.body.chars().take(140).collect();
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
    let chat = chat.as_ref().ok_or("Chat ist nicht verbunden")?;
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
    let chat = chat.as_ref().ok_or("Chat ist nicht verbunden")?;
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

#[tauri::command]
pub async fn pick_download_dir(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Ordner für empfangene Dateien")
        .pick_folder(move |f| {
            let _ = tx.send(f);
        });
    let Some(dir) = rx.await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let path = dir.into_path().map_err(|e| e.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}
