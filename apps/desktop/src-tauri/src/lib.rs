//! Desktop-App. Anmeldung im Systembrowser mit Rückkehr über
//! `starface-app://login`, stilles Wiederanmelden mit dem Refresh-Token aus
//! dem Schlüsselbund und ein Tray-Symbol.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sf_core::{Session, SessionEvent};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::{Mutex, mpsc};

struct PendingLogin {
    server: String,
    auth: sf_auth::Client,
    pkce: sf_auth::Pkce,
    state: String,
}

struct AppState {
    pending: Mutex<Option<PendingLogin>>,
    session: Mutex<Option<Session>>,
    events: mpsc::UnboundedSender<SessionEvent>,
}

#[derive(Clone, Serialize)]
struct SessionInfo {
    server: String,
    server_version: String,
    display_name: String,
}

impl From<&sf_core::SessionInfo> for SessionInfo {
    fn from(i: &sf_core::SessionInfo) -> Self {
        Self {
            server: i.server.clone(),
            server_version: i.server_version.clone(),
            display_name: format!("{} {}", i.first_name, i.last_name)
                .trim()
                .to_owned(),
        }
    }
}

/// Nicht geheime Einstellungen, als JSON im Konfigurationsordner.
#[derive(Default, Serialize, Deserialize)]
struct Settings {
    last_server: Option<String>,
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("settings.json"))
}

fn load_settings(app: &AppHandle) -> Settings {
    settings_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_settings(app: &AppHandle, settings: &Settings) {
    let Some(path) = settings_path(app) else {
        return;
    };
    let result = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| {
            std::fs::write(
                &path,
                serde_json::to_vec_pretty(settings).unwrap_or_default(),
            )
        });
    if let Err(e) = result {
        tracing::warn!(error = %e, "Einstellungen nicht gespeichert");
    }
}

fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

async fn set_session(app: &AppHandle, session: Option<Session>) {
    let tooltip = session
        .as_ref()
        .map_or("STARFACE: abgemeldet".to_owned(), |s| {
            format!("STARFACE: {}", SessionInfo::from(s.info()).display_name)
        });
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tooltip));
    }
    *app.state::<AppState>().session.lock().await = session;
}

/// Zuletzt benutzte Anlage, um das Anmeldefeld vorzubelegen.
#[tauri::command]
fn last_server(app: AppHandle) -> Option<String> {
    load_settings(&app).last_server
}

/// Stilles Wiederanmelden beim Start. `None` heisst: Browser-Login nötig.
#[tauri::command]
async fn restore_session(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<SessionInfo>, String> {
    if let Some(session) = state.session.lock().await.as_ref() {
        return Ok(Some(session.info().into()));
    }
    let Some(server) = load_settings(&app).last_server else {
        return Ok(None);
    };
    match Session::restore(&server, state.events.clone())
        .await
        .map_err(|e| e.to_string())?
    {
        Some(session) => {
            let info = SessionInfo::from(session.info());
            set_session(&app, Some(session)).await;
            Ok(Some(info))
        }
        None => Ok(None),
    }
}

/// Startet den Login im Systembrowser.
#[tauri::command]
async fn start_login(
    app: AppHandle,
    state: State<'_, AppState>,
    server: String,
) -> Result<(), String> {
    let server = server.trim().trim_end_matches('/').to_owned();
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

#[tauri::command]
async fn logout(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let session = state.session.lock().await.take();
    set_session(&app, None).await;
    if let Some(session) = session {
        session.logout().await.map_err(|e| e.to_string())?;
    }
    Ok(())
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
    let session = Session::start(&pending.server, pending.auth, tokens, state.events.clone())
        .await
        .map_err(|e| e.to_string())?;
    save_settings(
        app,
        &Settings {
            last_server: Some(pending.server),
        },
    );
    let info = SessionInfo::from(session.info());
    set_session(app, Some(session)).await;
    Ok(info)
}

fn handle_urls(app: &AppHandle, urls: Vec<String>) {
    for url in urls
        .into_iter()
        .filter(|u| u.starts_with(sf_auth::REDIRECT_URI))
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            show_main_window(&app);
            let _ = match finish_login(&app, &url).await {
                Ok(info) => app.emit("session", info),
                Err(e) => app.emit("login-error", e),
            };
        });
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Öffnen", true, None::<&str>)?;
    let logout_item = MenuItem::with_id(app, "logout", "Abmelden", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Beenden", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &logout_item, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("STARFACE: abgemeldet")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "logout" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    if let Err(e) = logout(app.clone(), state).await {
                        tracing::warn!(error = %e, "Abmelden fehlgeschlagen");
                    }
                    let _ = app.emit("logged-out", "Abgemeldet");
                    show_main_window(&app);
                });
            }
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    if let Err(e) = sf_auth::secret::init_system_store() {
        tracing::error!(error = %e, "Schlüsselbund nicht verfügbar; Anmeldung wird nicht gespeichert");
    }

    let (events_tx, mut events_rx) = mpsc::unbounded_channel();

    tauri::Builder::default()
        // Unter Linux startet der Browser für starface-app:// einen zweiten
        // Prozess; single-instance reicht die URL an die laufende App weiter.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app)
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            pending: Mutex::default(),
            session: Mutex::default(),
            events: events_tx,
        })
        .setup(move |app| {
            // Registriert starface-app:// für das laufende Binary (wichtig für
            // AppImage und `tauri dev`; das .deb bringt eine eigene .desktop-Datei
            // mit). Fehlt z. B. xdg-mime, soll die App trotzdem starten.
            #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
            if let Err(e) = app.deep_link().register_all() {
                tracing::warn!(error = %e, "starface-app:// nicht registriert");
            }
            let handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                handle_urls(
                    &handle,
                    event.urls().iter().map(|u| u.to_string()).collect(),
                );
            });
            build_tray(app.handle())?;

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Some(event) = events_rx.recv().await {
                    match event {
                        SessionEvent::LoggedOut { reason } => {
                            set_session(&handle, None).await;
                            let _ = handle.emit("logged-out", format!("Sitzung beendet: {reason}"));
                            show_main_window(&handle);
                        }
                    }
                }
            });
            Ok(())
        })
        // Schliessen versteckt das Fenster nur; die App bleibt im Tray erreichbar.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            last_server,
            restore_session,
            start_login,
            logout
        ])
        .run(tauri::generate_context!())
        .expect("Tauri-App konnte nicht starten");
}
