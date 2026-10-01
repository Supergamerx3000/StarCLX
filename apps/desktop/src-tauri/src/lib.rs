//! Desktop-App. Anmeldung im Systembrowser mit Rückkehr über
//! `starface-app://login`, stilles Wiederanmelden mit dem Refresh-Token aus
//! dem Schlüsselbund, ein Tray-Symbol und das Softphone.

mod audio;
mod busylight;
mod certs;
mod chat;
mod desktop;
mod fkeys;
mod presence;
mod reach;
mod settings;
mod voicemail;

use serde::Serialize;
use settings::Prefs;
use sf_core::journal::{Journal, JournalEvent};
use sf_core::phone::{CallPhase, Phone, PhoneEvent};
use sf_core::{Session, SessionEvent};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::{Mutex, mpsc};

struct PendingLogin {
    server: String,
    auth: sf_auth::Client,
    pkce: sf_auth::Pkce,
    state: String,
}

pub(crate) struct AppState {
    pending: Mutex<Option<PendingLogin>>,
    session: Mutex<Option<Session>>,
    events: mpsc::UnboundedSender<SessionEvent>,
    pub(crate) phone: Mutex<Option<Phone>>,
    /// Letzter Stand fürs Neuladen der Oberfläche
    phone_status: std::sync::Mutex<PhoneStatus>,
    journal: Mutex<Option<Journal>>,
}

#[derive(Clone, Default, Serialize)]
struct PhoneStatus {
    /// "off", "starting", "ready" oder "error"
    state: String,
    detail: String,
    calls: Vec<sf_core::phone::CallView>,
    muted: bool,
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

pub(crate) fn show_main_window(app: &AppHandle) {
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
    let state = app.state::<AppState>();
    // Erst das alte Softphone beenden, dann ggf. ein neues starten.
    state.phone.lock().await.take();
    audio::update_ringer(app, None);
    busylight::set_mode(app, busylight::Mode::Off);
    let host = session.as_ref().and_then(|s| {
        url::Url::parse(&s.info().server)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
    });
    let hub = session.as_ref().map(|s| s.hub().clone());
    let user_id = session.as_ref().map(|s| s.info().user_id.clone());
    *state.session.lock().await = session;
    *state.journal.lock().await = hub.clone().map(|hub| start_journal(app, hub));
    reach::restart(app, hub.clone()).await;
    voicemail::restart(app, hub.clone()).await;
    fkeys::stop(app).await;
    chat::stop(app).await;
    if let (Some(hub), Some(host), Some(user_id)) = (&hub, &host, user_id) {
        chat::start(app, hub.clone(), host.clone(), user_id);
    }
    if hub.is_none() {
        let _ = app.emit("journal", Vec::<sf_core::journal::Entry>::new());
    }
    match (hub, host) {
        (Some(hub), Some(host)) => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { start_phone(app, hub, host).await });
        }
        _ => update_phone_status(app, |s| *s = PhoneStatus::default()),
    }
}

fn start_journal(app: &AppHandle, hub: sf_onehub::OneHub) -> Journal {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let journal = Journal::start(hub, tx);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            match ev {
                JournalEvent::Entries { entries } => {
                    let _ = app.emit("journal", entries);
                }
                JournalEvent::Missed { entry } => notify_missed(&app, &entry),
                JournalEvent::Error { message } => {
                    let _ = app.emit("journal-error", message);
                }
            }
        }
    });
    journal
}

fn notify_missed(app: &AppHandle, entry: &sf_core::journal::Entry) {
    let prefs = settings::load(app).prefs;
    let group = !entry.group.is_empty();
    if !(if group {
        prefs.notify_missed_group
    } else {
        prefs.notify_missed
    }) {
        return;
    }
    let who = match (entry.name.trim(), entry.number.trim()) {
        ("", "") => "Unbekannt".to_owned(),
        ("", n) | (n, "") => n.to_owned(),
        (name, n) => format!("{name} ({n})"),
    };
    let body = if group {
        format!("{who} über Gruppe {}", entry.group)
    } else {
        who
    };
    if let Err(e) = app
        .notification()
        .builder()
        .title("Verpasster Anruf")
        .body(body)
        .show()
    {
        tracing::warn!(error = %e, "Benachrichtigung nicht angezeigt");
    }
}

fn update_phone_status(app: &AppHandle, f: impl FnOnce(&mut PhoneStatus)) {
    let status = {
        let state = app.state::<AppState>();
        let mut status = state.phone_status.lock().unwrap();
        f(&mut status);
        if status.state.is_empty() {
            status.state = "off".into();
        }
        status.clone()
    };
    let _ = app.emit("phone", status);
}

async fn start_phone(app: AppHandle, hub: sf_onehub::OneHub, host: String) {
    update_phone_status(&app, |s| {
        *s = PhoneStatus {
            state: "starting".into(),
            ..Default::default()
        }
    });
    let prefs = settings::load(&app).prefs;
    if !prefs.softphone {
        update_phone_status(&app, |s| {
            *s = PhoneStatus {
                state: "off".into(),
                detail: "Softphone in den Einstellungen ausgeschaltet".into(),
                ..Default::default()
            }
        });
        return;
    }
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut config = audio::softphone_config(&prefs).await;
    if certs::has_trusted(&app, &host) {
        config.verify_server = false;
    }
    match Phone::start(hub.clone(), &host, &config, env!("CARGO_PKG_VERSION"), tx).await {
        Ok(phone) => {
            let state = app.state::<AppState>();
            if state.session.lock().await.is_none() {
                return; // inzwischen abgemeldet
            }
            if prefs.primary_on_login {
                make_primary(&hub, phone.phone_id()).await;
            }
            *state.phone.lock().await = Some(phone);
            update_phone_status(&app, |s| s.state = "ready".into());
            busylight::set_mode(&app, busylight::Mode::Idle);
        }
        Err(e) => {
            tracing::warn!(error = %e, "Softphone nicht gestartet");
            update_phone_status(&app, |s| {
                s.state = "error".into();
                s.detail = e.to_string();
            });
            return;
        }
    }
    while let Some(ev) = rx.recv().await {
        match ev {
            PhoneEvent::Registered { ok, detail } => update_phone_status(&app, |s| {
                s.state = if ok { "ready" } else { "error" }.into();
                s.detail = detail;
            }),
            PhoneEvent::Calls { calls, muted } => {
                let ringing = calls
                    .iter()
                    .any(|c| c.incoming && c.phase == CallPhase::Ringing);
                let was_ringing = app
                    .state::<AppState>()
                    .phone_status
                    .lock()
                    .unwrap()
                    .calls
                    .iter()
                    .any(|c| c.incoming && c.phase == CallPhase::Ringing);
                let ring_internal = calls
                    .iter()
                    .find(|c| c.incoming && c.phase == CallPhase::Ringing)
                    .map(|c| c.internal);
                audio::update_ringer(&app, ring_internal);
                busylight::set_mode(&app, busylight::Mode::from_calls(&calls));
                update_phone_status(&app, |s| {
                    s.calls = calls;
                    s.muted = muted;
                });
                if ringing && !was_ringing && settings::load(&app).prefs.bring_to_front {
                    show_main_window(&app);
                }
            }
            PhoneEvent::Error { message } => {
                let _ = app.emit("phone-error", message);
            }
        }
    }
}

async fn make_primary(hub: &sf_onehub::OneHub, phone_id: &str) {
    if let Err(e) = sf_core::account::set_primary_phone(hub, phone_id).await {
        tracing::warn!(error = %e, "Softphone nicht als primäres Telefon gesetzt");
    }
}

/// Zuletzt benutzte Anlage, um das Anmeldefeld vorzubelegen.
#[tauri::command]
fn last_server(app: AppHandle) -> Option<String> {
    settings::load(&app).last_server
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
    let Some(server) = settings::load(&app).last_server else {
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
    let server = certs::normalize_server(&server);
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
fn phone_status(state: State<'_, AppState>) -> PhoneStatus {
    let mut status = state.phone_status.lock().unwrap().clone();
    if status.state.is_empty() {
        status.state = "off".into();
    }
    status
}

#[tauri::command]
async fn phone_dial(state: State<'_, AppState>, number: String) -> Result<(), String> {
    let number = clean_number(&number);
    if number.is_empty() {
        return Err("Keine Nummer".into());
    }
    with_phone(&state, async |p| p.dial(&number).await).await
}

#[tauri::command]
async fn phone_answer(
    app: AppHandle,
    state: State<'_, AppState>,
    call_id: String,
) -> Result<(), String> {
    let primary = settings::load(&app).prefs.primary_on_answer;
    with_phone(&state, async |p| {
        p.answer(&call_id)?;
        if primary {
            let hub = state.session.lock().await.as_ref().map(|s| s.hub().clone());
            if let Some(hub) = hub {
                make_primary(&hub, p.phone_id()).await;
            }
        }
        Ok(())
    })
    .await
}

#[tauri::command]
async fn phone_hangup(state: State<'_, AppState>, call_id: String) -> Result<(), String> {
    with_phone(&state, async |p| p.hangup(&call_id).await).await
}

#[tauri::command]
async fn phone_hold(state: State<'_, AppState>, call_id: String, hold: bool) -> Result<(), String> {
    with_phone(&state, async |p| p.hold(&call_id, hold).await).await
}

#[tauri::command]
async fn phone_mute(state: State<'_, AppState>, muted: bool) -> Result<(), String> {
    with_phone(&state, async |p| p.set_mute(muted)).await
}

#[tauri::command]
async fn phone_dtmf(
    state: State<'_, AppState>,
    call_id: String,
    digits: String,
) -> Result<(), String> {
    with_phone(&state, async |p| p.send_dtmf(&call_id, &digits).await).await
}

/// Weitere Funktionen des Call Managers.
#[tauri::command]
async fn phone_action(
    state: State<'_, AppState>,
    action: String,
    call_id: String,
    number: Option<String>,
) -> Result<(), String> {
    // Bei "conference" enthält `number` die weiteren Anruf-IDs, kommagetrennt.
    let raw = number.unwrap_or_default();
    let number = clean_number(&raw);
    with_phone(&state, async |p| match action.as_str() {
        "forward" => p.forward(&call_id, &number).await,
        "voicemail" => p.to_voicemail(&call_id).await,
        "record" => p.record(&call_id).await,
        "switch_phone" => p.switch_phone(&call_id).await,
        "consult" => p.consult(&call_id, &number).await,
        "transfer_consultation" => p.transfer_consultation(&call_id).await,
        "conference" => {
            let mut ids = vec![call_id.clone()];
            ids.extend(
                raw.split(',')
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned),
            );
            p.conference(&ids).await
        }
        _ => Ok(()),
    })
    .await
}

fn clean_number(n: &str) -> String {
    n.chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '-' | '/' | '(' | ')'))
        .collect()
}

async fn with_phone(
    state: &AppState,
    f: impl AsyncFnOnce(&Phone) -> sf_core::phone::PhoneResult<()>,
) -> Result<(), String> {
    let phone = state.phone.lock().await;
    let phone = phone.as_ref().ok_or("Softphone ist nicht bereit")?;
    f(phone).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn get_prefs(app: AppHandle) -> Prefs {
    settings::load(&app).prefs
}

/// Speichert die Einstellungen. Ändert sich etwas am Softphone, startet es neu,
/// sofern gerade kein Gespräch läuft.
#[tauri::command]
async fn save_prefs(
    app: AppHandle,
    state: State<'_, AppState>,
    prefs: Prefs,
) -> Result<(), String> {
    let old = settings::load(&app).prefs;
    settings::update(&app, |s| s.prefs = prefs.clone());
    busylight::refresh(&app);
    desktop::apply_window(&app, &prefs);
    if old.hotkeys != prefs.hotkeys || prefs.hotkeys.enabled {
        desktop::apply_hotkeys(&prefs.hotkeys)
            .map_err(|e| format!("Gespeichert, aber Tastenkürzel nicht eingetragen: {e}"))?;
    }
    if !old.softphone_changed(&prefs) {
        return Ok(());
    }
    if !state.phone_status.lock().unwrap().calls.is_empty() {
        return Err("Gespeichert. Das Softphone übernimmt die Änderung nach dem Gespräch beim nächsten Start.".into());
    }
    restart_phone(&app).await;
    Ok(())
}

async fn restart_phone(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.phone.lock().await.take();
    let session = state.session.lock().await;
    let Some(session) = session.as_ref() else {
        return;
    };
    let hub = session.hub().clone();
    let host = url::Url::parse(&session.info().server)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned));
    if let Some(host) = host {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { start_phone(app, hub, host).await });
    }
}

pub(crate) async fn hub(state: &AppState) -> Result<sf_onehub::OneHub, String> {
    state
        .session
        .lock()
        .await
        .as_ref()
        .map(|s| s.hub().clone())
        .ok_or_else(|| "Nicht angemeldet".to_owned())
}

#[tauri::command]
async fn signaling_numbers(
    state: State<'_, AppState>,
) -> Result<Vec<sf_core::account::SignalingNumber>, String> {
    sf_core::account::signaling_numbers(&hub(&state).await?)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_signaling_number(state: State<'_, AppState>, id: String) -> Result<(), String> {
    sf_core::account::set_signaling_number(&hub(&state).await?, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn contacts_search(
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
async fn contacts_folders(
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
async fn contacts_list(
    state: State<'_, AppState>,
    folder: String,
    term: String,
    offset: i32,
) -> Result<sf_core::directory::Page, String> {
    sf_core::directory::list(&hub(&state).await?, &folder, &term, offset, 50)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn journal_entries(
    state: State<'_, AppState>,
) -> Result<Vec<sf_core::journal::Entry>, String> {
    Ok(state
        .journal
        .lock()
        .await
        .as_ref()
        .map(Journal::entries)
        .unwrap_or_default())
}

/// Rufliste bearbeiten: "delete", "called_back", "not_called_back" oder
/// "comment" (mit `text`).
#[tauri::command]
async fn journal_action(
    state: State<'_, AppState>,
    action: String,
    id: String,
    text: Option<String>,
) -> Result<(), String> {
    let journal = state.journal.lock().await;
    let journal = journal.as_ref().ok_or("Nicht angemeldet")?;
    match action.as_str() {
        "delete" => journal.delete(&id).await,
        "called_back" => journal.set_called_back(&id, true).await,
        "not_called_back" => journal.set_called_back(&id, false).await,
        "comment" => journal.set_comment(&id, &text.unwrap_or_default()).await,
        _ => return Err(format!("Unbekannte Aktion {action}")),
    }
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
    settings::update(app, |s| s.last_server = Some(pending.server));
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
        // Tastenkürzel kommen als `--action …` über denselben Weg.
        .plugin(tauri_plugin_single_instance::init(
            |app, argv, _cwd| match desktop::action_from_args(&argv) {
                Some(action) => desktop::run_action(app, action),
                None => show_main_window(app),
            },
        ))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(chat::ChatState::default())
        .manage(reach::ReachState::default())
        .manage(fkeys::FkeyState::default())
        .manage(voicemail::VoicemailState::default())
        .manage(audio::AudioState::default())
        .manage(busylight::BusylightState::default())
        .manage(AppState {
            pending: Mutex::default(),
            session: Mutex::default(),
            events: events_tx,
            phone: Mutex::default(),
            phone_status: std::sync::Mutex::default(),
            journal: Mutex::default(),
        })
        .setup(move |app| {
            certs::init(app.handle());
            presence::start(app.handle());
            let prefs = settings::load(app.handle()).prefs;
            desktop::apply_window(app.handle(), &prefs);
            desktop::show_on_start(app.handle(), &prefs);
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
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                let _ = window.hide();
                api.prevent_close();
            }
            // Minimieren kommt unter Linux als Grössenänderung an
            WindowEvent::Resized(_)
                if window.is_minimized().unwrap_or(false)
                    && settings::load(window.app_handle()).prefs.minimize_to_tray =>
            {
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            last_server,
            busylight::busylight_info,
            busylight::busylight_test,
            restore_session,
            certs::check_certificate,
            certs::trust_certificate,
            start_login,
            logout,
            phone_status,
            phone_dial,
            phone_answer,
            phone_hangup,
            phone_hold,
            phone_mute,
            phone_dtmf,
            phone_action,
            get_prefs,
            save_prefs,
            signaling_numbers,
            set_signaling_number,
            contacts_search,
            contacts_folders,
            contacts_list,
            journal_entries,
            journal_action,
            chat::chat_status,
            chat::chat_recent,
            chat::chat_conversation,
            chat::chat_send,
            chat::default_download_dir,
            desktop::desktop_info,
            reach::redirects,
            reach::redirect_enable,
            reach::redirect_update,
            reach::fmc_phones,
            reach::fmc_save,
            reach::fmc_enable,
            reach::fmc_delete,
            reach::mailboxes,
            reach::mailbox_record,
            fkeys::fkeys_load,
            fkeys::fkey_presence,
            fkeys::fkey_save,
            fkeys::fkey_delete,
            fkeys::fkeys_reorder,
            fkeys::fkey_dnd,
            fkeys::fkey_park,
            fkeys::fkey_grab,
            voicemail::voicemails,
            voicemail::voicemail_audio,
            voicemail::voicemail_save,
            voicemail::voicemail_move,
            voicemail::voicemail_delete,
            voicemail::voicemail_via_phone,
            chat::pick_download_dir,
            audio::audio_info,
            audio::audio_preview,
            audio::audio_stop,
            audio::mic_test,
            audio::pick_ringtone
        ])
        .run(tauri::generate_context!())
        .expect("Tauri-App konnte nicht starten");
}
