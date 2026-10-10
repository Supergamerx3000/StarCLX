//! Desktop-App. Anmeldung im Systembrowser mit Rückkehr über
//! `starface-app://login`, stilles Wiederanmelden mit dem Refresh-Token aus
//! dem Schlüsselbund, ein Tray-Symbol und das Softphone.

mod audio;
mod bus;
mod certs;
mod desktop;
mod flatpak;
mod i18n;
mod log;
mod login;
mod plugins;
mod presence;
mod settings;
mod wake;
mod zoom;

use bus::Event;
use i18n::{t, tf};
use plugins::{busylight, chat, fkeys, reach, voicemail};
use serde::Serialize;
use settings::Prefs;
use sf_core::{Session, SessionEvent};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::{Mutex, mpsc};

/// Beenden läuft: Aufräumen vor dem Beenden ist erledigt bzw. unterwegs
static EXITING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
    /// Rufnummer aus einem tel:-Link, bis die Oberfläche sie abholt; leer
    /// heisst: Link ohne Nummer
    dial_request: std::sync::Mutex<Option<String>>,
}

#[derive(Clone, Serialize)]
struct SessionInfo {
    server: String,
    server_version: String,
    display_name: String,
    /// OneHub-ID des angemeldeten Users (Profilbild, eigene Präsenz)
    user_id: String,
}

impl From<&sf_core::SessionInfo> for SessionInfo {
    fn from(i: &sf_core::SessionInfo) -> Self {
        Self {
            server: i.server.clone(),
            server_version: i.server_version.clone(),
            display_name: format!("{} {}", i.first_name, i.last_name)
                .trim()
                .to_owned(),
            user_id: i.user_id.clone(),
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

/// Kleines Fenster mit Rufnummernfeld und Besetztlampenfeldern
pub(crate) fn show_quick_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("quick") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    let built =
        tauri::WebviewWindowBuilder::new(app, "quick", tauri::WebviewUrl::App("quick".into()))
            .title(t("StarCLX Schnellwahl"))
            .inner_size(340.0, 480.0)
            .min_inner_size(260.0, 240.0)
            .build();
    if let Err(e) = built {
        tracing::warn!(error = %e, "Schnellwahl-Fenster nicht geöffnet");
    }
}

/// Schnellwahl schliessen (Esc); das Fenster bleibt für das nächste Mal bestehen.
#[tauri::command]
fn quick_hide(app: AppHandle) {
    if let Some(w) = app.get_webview_window("quick") {
        let _ = w.hide();
    }
}

/// Tooltip des Tray-Symbols: angemeldeter Benutzer oder „abgemeldet“
fn set_tray_tooltip(app: &AppHandle, session: Option<&Session>) {
    let tooltip = session.map_or(t("StarCLX: abgemeldet").to_owned(), |s| {
        format!("StarCLX: {}", SessionInfo::from(s.info()).display_name)
    });
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tooltip));
    }
}

async fn set_session(app: &AppHandle, session: Option<Session>) {
    set_tray_tooltip(app, session.as_ref());
    // Erst alles von der alten Sitzung beenden, dann ggf. neu starten.
    plugins::session_ended(app).await;
    let login = session.as_ref().and_then(|s| {
        let host = url::Url::parse(&s.info().server)
            .ok()?
            .host_str()?
            .to_owned();
        Some(plugins::Login {
            hub: s.hub().clone(),
            host,
            user_id: s.info().user_id.clone(),
            display_name: SessionInfo::from(s.info()).display_name,
        })
    });
    *app.state::<AppState>().session.lock().await = session;
    if let Some(login) = login {
        plugins::session_started(app, login).await;
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

/// Startet den Login im eigenen Anmeldefenster oder, mit `browser`, im
/// Systembrowser.
#[tauri::command]
async fn start_login(
    app: AppHandle,
    state: State<'_, AppState>,
    server: String,
    browser: Option<bool>,
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
    if browser.unwrap_or(false) {
        app.opener()
            .open_url(url.as_str(), None::<&str>)
            .map_err(|e| e.to_string())
    } else {
        login::open(&app, &url).map_err(|e| e.to_string())
    }
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
    mut prefs: Prefs,
) -> Result<(), String> {
    let old = settings::load(&app).prefs;
    // Der eigene Status kommt aus dem Menü bzw. von der Anlage; ein länger
    // offener Einstellungsdialog darf ihn nicht mit altem Stand überschreiben.
    prefs.chat_availability.clone_from(&old.chat_availability);
    prefs.chat_text.clone_from(&old.chat_text);
    prefs.chat_presets.clone_from(&old.chat_presets);
    settings::update(&app, |s| s.prefs = prefs.clone());
    if old.language != prefs.language {
        i18n::set_language(&prefs.language);
        relabel(&app, state.session.lock().await.as_ref());
    }
    log::set_verbose(prefs.verbose_log);
    bus::publish(&app, Event::PrefsSaved);
    desktop::apply_window(&app, &prefs);
    if old.handle_tel_links != prefs.handle_tel_links {
        desktop::register_schemes(&app, prefs.handle_tel_links);
    }
    desktop::apply_autostart(prefs.autostart, old.autostart != prefs.autostart).map_err(|e| {
        tf(
            "Gespeichert, aber Autostart nicht eingerichtet: {e}",
            &[("e", &e)],
        )
    })?;
    if old.hotkeys != prefs.hotkeys || prefs.hotkeys.enabled {
        desktop::apply_hotkeys(&prefs.hotkeys).map_err(|e| {
            tf(
                "Gespeichert, aber Tastenkürzel nicht eingetragen: {e}",
                &[("e", &e)],
            )
        })?;
    }
    plugins::call::apply_prefs(&app, &old, &prefs).await
}

pub(crate) async fn hub(state: &AppState) -> Result<sf_onehub::OneHub, String> {
    state
        .session
        .lock()
        .await
        .as_ref()
        .map(|s| s.hub().clone())
        .ok_or_else(|| t("Nicht angemeldet").to_owned())
}

#[tauri::command]
async fn signaling_numbers(
    state: State<'_, AppState>,
) -> Result<Vec<sf_core::account::SignalingNumber>, String> {
    sf_core::account::signaling_numbers(&hub(&state).await?)
        .await
        .map_err(|e| e.to_string())
}

/// Eigene Telefone mit dem primären
#[tauri::command]
async fn phones(state: State<'_, AppState>) -> Result<Vec<sf_core::account::PhoneView>, String> {
    sf_core::account::phones(&hub(&state).await?)
        .await
        .map_err(|e| e.to_string())
}

/// Primäres Telefon wählen (klingelt bei Anrufen, wählt bei Click-to-Dial)
#[tauri::command]
async fn set_primary_phone(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let hub = hub(&state).await?;
    // Wird das Softphone primär, bekommt das bisherige die Rolle beim
    // Beenden zurück.
    if let Some(soft) = plugins::call::softphone_id(&app).await
        && soft == id
    {
        plugins::call::remember_primary(&app, &hub, &soft).await;
    }
    sf_core::account::set_primary_phone(&hub, &id)
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
        .ok_or(t("Kein Login ausstehend"))?;
    tracing::info!(redirect = %sf_auth::redacted_redirect(redirect), "Rücksprung vom Login");
    let code = sf_auth::code_from_redirect(redirect, &pending.state).map_err(|e| match e {
        sf_auth::RedirectError::Denied { error, description } => tf(
            "Die Anlage hat die Anmeldung abgelehnt: {e}",
            &[("e", format!("{error} {description}").trim())],
        ),
        sf_auth::RedirectError::StateMismatch => {
            t("Die Antwort gehört zu einem älteren Anmeldeversuch. Bitte erneut anmelden.").into()
        }
        sf_auth::RedirectError::NoCode => {
            t("Antwort der Anlage enthält keinen gültigen Code").into()
        }
    })?;
    let tokens = pending
        .auth
        .exchange_code(&code, &pending.pkce)
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!(token = %tokens.summary(), "Token erhalten");
    let session = Session::start(&pending.server, pending.auth, tokens, state.events.clone())
        .await
        .map_err(|e| e.to_string())?;
    settings::update(app, |s| s.last_server = Some(pending.server));
    let info = SessionInfo::from(session.info());
    set_session(app, Some(session)).await;
    Ok(info)
}

/// Rufnummer aus einem tel:-Link abholen (beim Start und nach "dial-request")
#[tauri::command]
fn take_dial_request(state: State<'_, AppState>) -> Option<String> {
    state.dial_request.lock().unwrap().take()
}

pub(crate) fn handle_urls(app: &AppHandle, urls: Vec<String>) {
    for url in urls {
        if desktop::is_tel_url(&url) {
            // Ausgeschaltet: Die .desktop-Datei des Pakets meldet tel: trotzdem an
            if !settings::load(app).prefs.handle_tel_links {
                continue;
            }
            let number = desktop::number_from_url(&url).unwrap_or_default();
            *app.state::<AppState>().dial_request.lock().unwrap() = Some(number);
            show_main_window(app);
            let _ = app.emit("dial-request", ());
            continue;
        }
        if !url.starts_with(sf_auth::REDIRECT_URI) {
            continue;
        }
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

/// Menü des Tray-Symbols in der eingestellten Sprache
fn tray_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let open = MenuItem::with_id(app, "open", t("Öffnen"), true, None::<&str>)?;
    let quick = MenuItem::with_id(app, "quick", t("Schnellwahl"), true, None::<&str>)?;
    let logout_item = MenuItem::with_id(app, "logout", t("Abmelden"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", t("Beenden"), true, None::<&str>)?;
    Menu::with_items(app, &[&open, &quick, &logout_item, &quit])
}

/// Nach einem Sprachwechsel: Tray und offene Fenster neu beschriften
fn relabel(app: &AppHandle, session: Option<&Session>) {
    if let Some(tray) = app.tray_by_id("main") {
        match tray_menu(app) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(e) => tracing::warn!(error = %e, "Tray-Menü nicht neu aufgebaut"),
        }
    }
    set_tray_tooltip(app, session);
    if let Some(w) = app.get_webview_window("quick") {
        let _ = w.set_title(t("StarCLX Schnellwahl"));
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = tray_menu(app)?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip(t("StarCLX: abgemeldet"))
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "quick" => show_quick_window(app),
            "logout" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    if let Err(e) = logout(app.clone(), state).await {
                        tracing::warn!(error = %e, "Abmelden fehlgeschlagen");
                    }
                    let _ = app.emit("logged-out", t("Abgemeldet"));
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
    log::init();
    if let Err(e) = sf_auth::secret::init_system_store() {
        tracing::error!(error = %e, "Schlüsselbund nicht verfügbar; Anmeldung wird nicht gespeichert");
    }

    let (events_tx, mut events_rx) = mpsc::unbounded_channel();

    tauri::Builder::default()
        // Unter Linux startet der Browser für starface-app:// einen zweiten
        // Prozess; single-instance reicht die URL an die laufende App weiter.
        // Tastenkürzel kommen als `--action …` über denselben Weg.
        // Als Flatpak darf die App nur D-Bus-Namen unter ihrer eigenen ID
        // belegen.
        .plugin({
            let builder =
                tauri_plugin_single_instance::Builder::new().callback(|app, argv, _cwd| {
                    match desktop::action_from_args(&argv) {
                        Some(action) => desktop::run_action(app, action),
                        None => show_main_window(app),
                    }
                });
            match flatpak::app_id() {
                Some(id) => builder.dbus_id(id),
                None => builder,
            }
            .build()
        })
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(zoom::init())
        .manage(chat::ChatState::default())
        .manage(reach::ReachState::default())
        .manage(fkeys::FkeyState::default())
        .manage(voicemail::VoicemailState::default())
        .manage(audio::AudioState::default())
        .manage(bus::Bus::default())
        .manage(plugins::call::CallState::default())
        .manage(plugins::conference::ConferenceState::default())
        .manage(plugins::doorcam::DoorCamState::default())
        .manage(plugins::journal::JournalState::default())
        .manage(busylight::BusylightState::default())
        .manage(plugins::headset::HeadsetState::default())
        .manage(AppState {
            pending: Mutex::default(),
            session: Mutex::default(),
            events: events_tx,
            dial_request: std::sync::Mutex::default(),
        })
        .setup(move |app| {
            log::open(app.handle());
            plugins::start(app.handle());
            certs::init(app.handle());
            presence::start(app.handle());
            wake::start(app.handle());
            let prefs = settings::load(app.handle()).prefs;
            log::set_verbose(prefs.verbose_log);
            i18n::set_language(&prefs.language);
            desktop::apply_window(app.handle(), &prefs);
            desktop::show_on_start(app.handle(), &prefs);
            // Tastenkürzel neu eintragen, damit sie nach einem Update oder
            // der Umbenennung auf das aktuelle Programm zeigen.
            if prefs.hotkeys.enabled
                && let Err(e) = desktop::apply_hotkeys(&prefs.hotkeys)
            {
                tracing::warn!(error = %e, "Tastenkürzel nicht eingetragen");
            }
            if let Err(e) = desktop::apply_autostart(prefs.autostart, false) {
                tracing::warn!(error = %e, "Autostart nicht eingerichtet");
            }
            // starface-app:// und ggf. tel:/callto:/sip: für das laufende Binary
            desktop::register_schemes(app.handle(), prefs.handle_tel_links);
            // Kaltstart über einen tel:-Link; ein Login-Rücksprung kann jetzt
            // keinen ausstehenden Login mehr haben.
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                handle_urls(
                    app.handle(),
                    urls.iter()
                        .map(|u| u.to_string())
                        .filter(|u| !u.starts_with(sf_auth::REDIRECT_URI))
                        .collect(),
                );
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
                            let _ = handle.emit(
                                "logged-out",
                                tf("Sitzung beendet: {reason}", &[("reason", &reason)]),
                            );
                            show_main_window(&handle);
                        }
                    }
                }
            });
            Ok(())
        })
        // Schliessen versteckt Haupt- und Schnellwahlfenster nur; die App
        // bleibt im Tray erreichbar. Andere Fenster (Anmeldung) schliessen
        // wirklich, sonst scheitert die nächste Anmeldung am vorhandenen Label.
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. }
                if matches!(window.label(), "main" | "quick") =>
            {
                let _ = window.hide();
                api.prevent_close();
            }
            // Minimieren kommt unter Linux als Grössenänderung an
            WindowEvent::Resized(_)
                if window.label() == "main"
                    && window.is_minimized().unwrap_or(false)
                    && settings::load(window.app_handle()).prefs.minimize_to_tray =>
            {
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            last_server,
            quick_hide,
            busylight::busylight_info,
            busylight::busylight_test,
            plugins::headset::headset_info,
            plugins::headset::headset_test,
            restore_session,
            certs::check_certificate,
            certs::trust_certificate,
            start_login,
            logout,
            plugins::call::phone_status,
            plugins::call::phone_dial,
            plugins::call::phone_answer,
            plugins::call::phone_hangup,
            plugins::call::phone_hold,
            plugins::call::phone_mute,
            plugins::call::phone_callback,
            plugins::call::phone_dtmf,
            plugins::call::phone_action,
            plugins::call::phone_trust_sip_certificate,
            plugins::doorcam::doorcam_watch,
            plugins::doorcam::doorcam_stop,
            get_prefs,
            save_prefs,
            signaling_numbers,
            set_signaling_number,
            phones,
            set_primary_phone,
            plugins::contacts::contacts_search,
            plugins::contacts::contacts_folders,
            plugins::contacts::contacts_list,
            plugins::contacts::contact_form,
            plugins::contacts::contact_save,
            plugins::contacts::contact_delete,
            plugins::journal::journal_entries,
            plugins::journal::journal_action,
            chat::chat_status,
            chat::chat_set_own,
            chat::chat_delete_preset,
            chat::chat_recent,
            chat::chat_conversation,
            chat::chat_send,
            chat::chat_create_room,
            chat::chat_leave_room,
            chat::chat_transfers,
            chat::chat_pick_files,
            chat::chat_send_files,
            chat::chat_accept_file,
            chat::chat_decline_file,
            chat::chat_cancel_file,
            chat::chat_open_file,
            chat::chat_show_file,
            chat::default_download_dir,
            desktop::desktop_info,
            take_dial_request,
            plugins::callactions::call_action_run,
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
            fkeys::fkey_avatar,
            fkeys::account_pick_avatar,
            fkeys::account_set_avatar,
            fkeys::account_delete_avatar,
            fkeys::account_change_password,
            fkeys::account_license,
            fkeys::fkey_save,
            fkeys::fkey_delete,
            fkeys::fkeys_reorder,
            fkeys::fkey_dnd,
            fkeys::fkey_groups,
            fkeys::fkey_modules,
            fkeys::fkey_module_toggle,
            fkeys::fkey_group_toggle,
            fkeys::fkey_park,
            fkeys::fkey_grab,
            fkeys::fkey_redirect_options,
            fkeys::fkey_set_ask,
            fkeys::fkey_redirect_program,
            fkeys::fkey_redirect_restore,
            voicemail::voicemails,
            voicemail::voicemail_audio,
            voicemail::voicemail_save,
            voicemail::voicemail_move,
            voicemail::voicemail_delete,
            voicemail::voicemail_via_phone,
            plugins::conference::conferences,
            plugins::conference::conference_save,
            plugins::conference::conference_delete,
            plugins::conference::conference_start,
            chat::pick_download_dir,
            log::log_export,
            log::log_open_dir,
            audio::audio_info,
            audio::audio_preview,
            audio::audio_stop,
            audio::mic_test,
            audio::pick_ringtone
        ])
        .build(tauri::generate_context!())
        .expect("Tauri-App konnte nicht starten")
        .run(|app, event| {
            // Vor dem Beenden das Softphone abgeben, damit es nicht primäres
            // Telefon bleibt; danach wirklich beenden.
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event
                && !EXITING.swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                api.prevent_exit();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    plugins::call::session_ended(&app).await;
                    app.exit(code.unwrap_or(0));
                });
            }
        });
}
