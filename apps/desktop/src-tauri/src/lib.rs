//! Desktop-App. Meilenstein 1: Anmeldung im Systembrowser, Rückkehr über
//! `starface-app://login`, danach Verbindung zur OneHub-API.

use serde::Serialize;
use sf_onehub::{OneHub, TokenHandle};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Mutex;

struct PendingLogin {
    server: String,
    auth: sf_auth::Client,
    pkce: sf_auth::Pkce,
    state: String,
}

#[derive(Default)]
struct AppState {
    pending: Mutex<Option<PendingLogin>>,
    hub: Mutex<Option<OneHub>>,
}

#[derive(Clone, Serialize)]
struct SessionInfo {
    server_version: String,
    display_name: String,
}

/// Startet den Login im Systembrowser.
#[tauri::command]
async fn start_login(
    app: AppHandle,
    state: State<'_, AppState>,
    server: String,
) -> Result<(), String> {
    let auth = sf_auth::Client::discover(&server)
        .await
        .map_err(|e| e.to_string())?;
    let pkce = sf_auth::Pkce::generate().map_err(|e| e.to_string())?;
    let login_state = sf_auth::Pkce::generate()
        .map_err(|e| e.to_string())?
        .verifier;
    let url = auth
        .authorize_url(&pkce, &login_state)
        .map_err(|e| e.to_string())?;
    *state.pending.lock().await = Some(PendingLogin {
        server,
        auth,
        pkce,
        state: login_state,
    });
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|e| e.to_string())
}

async fn finish_login(app: &AppHandle, redirect: &str) -> Result<SessionInfo, String> {
    let state = app.state::<AppState>();
    let pending = state
        .pending
        .lock()
        .await
        .take()
        .ok_or("Kein Login ausstehend")?;
    let code = sf_auth::code_from_redirect(redirect, &pending.state)
        .ok_or("Antwort der Anlage enthält keinen gültigen Code")?;
    let tokens = pending
        .auth
        .exchange_code(&code, &pending.pkce)
        .await
        .map_err(|e| e.to_string())?;

    let host = url::Url::parse(&pending.server)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .ok_or("Ungültige Server-Adresse")?;
    let hub = OneHub::connect(
        &host,
        sf_onehub::DEFAULT_PORT,
        TokenHandle::new(tokens.access_token),
    )
    .await
    .map_err(|e| e.to_string())?;
    let server_version = hub.server_version().await.map_err(|e| e.to_string())?;
    let user = hub
        .me()
        .get_user(())
        .await
        .map_err(|e| e.to_string())?
        .into_inner()
        .user
        .unwrap_or_default();
    // TODO(Meilenstein 1): Refresh-Token im Secret Service ablegen und
    // Access-Token vor Ablauf erneuern.
    *state.hub.lock().await = Some(hub);
    Ok(SessionInfo {
        server_version,
        display_name: format!("{} {}", user.first_name, user.last_name),
    })
}

fn handle_urls(app: &AppHandle, urls: Vec<String>) {
    for url in urls
        .into_iter()
        .filter(|u| u.starts_with(sf_auth::REDIRECT_URI))
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let result = finish_login(&app, &url).await;
            let _ = match result {
                Ok(info) => app.emit("session", info),
                Err(e) => app.emit("login-error", e),
            };
        });
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Unter Linux startet der Browser für starface-app:// einen zweiten
        // Prozess; single-instance reicht die URL an die laufende App weiter.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .setup(|app| {
            #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
            app.deep_link().register_all()?;
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                handle_urls(
                    &handle,
                    event.urls().iter().map(|u| u.to_string()).collect(),
                );
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![start_login])
        .run(tauri::generate_context!())
        .expect("Tauri-App konnte nicht starten");
}
