//! Plugin Call: das Softphone mit Anrufen, Halten, Rückfrage, Konferenz usw.
//! Startet bei der Anmeldung, meldet Anrufe auf dem Bus und steuert den
//! Klingelton.

use serde::Serialize;
use sf_core::phone::{CallPhase, Phone, PhoneEvent};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, mpsc};

use crate::bus::{self, Event};
use crate::i18n::t;
use crate::settings::{self, Prefs};
use crate::{AppState, audio, certs, show_main_window};

#[derive(Default)]
pub struct CallState {
    phone: Mutex<Option<Phone>>,
    /// Letzter Stand fürs Neuladen der Oberfläche
    status: std::sync::Mutex<PhoneStatus>,
}

pub async fn session_ended(app: &AppHandle) {
    app.state::<CallState>().phone.lock().await.take();
    audio::update_ringer(app, None);
    bus::publish(app, Event::PhoneStopped);
    update_phone_status(app, |s| *s = PhoneStatus::default());
}

pub fn session_started(app: &AppHandle, hub: sf_onehub::OneHub, host: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { start_phone(app, hub, host).await });
}

/// ID des Softphones an der Anlage, sofern es läuft
pub async fn softphone_id(app: &AppHandle) -> Option<String> {
    app.state::<CallState>()
        .phone
        .lock()
        .await
        .as_ref()
        .map(|p| p.phone_id().to_owned())
}

/// Nach dem Aufwachen neu registrieren
pub async fn resume(app: &AppHandle) {
    if let Some(phone) = app.state::<CallState>().phone.lock().await.as_ref() {
        phone.resume();
    }
}

/// Geänderte Softphone-Einstellungen übernehmen; während eines Gesprächs
/// erst beim nächsten Start.
pub async fn apply_prefs(app: &AppHandle, old: &Prefs, prefs: &Prefs) -> Result<(), String> {
    if !old.softphone_changed(prefs) {
        return Ok(());
    }
    if !app
        .state::<CallState>()
        .status
        .lock()
        .unwrap()
        .calls
        .is_empty()
    {
        return Err(t("Gespeichert. Das Softphone übernimmt die Änderung nach dem Gespräch beim nächsten Start.").into());
    }
    restart_phone(app).await;
    Ok(())
}

#[derive(Clone, Default, Serialize)]
pub struct PhoneStatus {
    /// "off", "starting", "ready" oder "error"
    state: String,
    detail: String,
    calls: Vec<sf_core::phone::CallView>,
    muted: bool,
}

fn update_phone_status(app: &AppHandle, f: impl FnOnce(&mut PhoneStatus)) {
    let status = {
        let state = app.state::<CallState>();
        let mut status = state.status.lock().unwrap();
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
                detail: t("Softphone in den Einstellungen ausgeschaltet").into(),
                ..Default::default()
            }
        });
        return;
    }
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut config = audio::softphone_config(&prefs).await;
    // Cloud-Anlagen nutzen für SIP ein Zertifikat der privaten „STARFACE CA“
    // (auf die IP ausgestellt), das kein System kennt; baresip kann es nicht
    // einzeln bestätigen. Wie bei bestätigten Zertifikaten nicht prüfen.
    let cloud = app
        .state::<AppState>()
        .session
        .lock()
        .await
        .as_ref()
        .is_some_and(|s| s.info().cloud);
    if cloud || certs::is_confirmed(&host).await {
        config.verify_server = false;
    }
    match Phone::start(hub.clone(), &host, &config, env!("CARGO_PKG_VERSION"), tx).await {
        Ok(phone) => {
            if app.state::<AppState>().session.lock().await.is_none() {
                return; // inzwischen abgemeldet
            }
            if prefs.primary_on_login {
                make_primary(&hub, phone.phone_id()).await;
            }
            *app.state::<CallState>().phone.lock().await = Some(phone);
            update_phone_status(&app, |s| s.state = "ready".into());
            bus::publish(&app, Event::PhoneReady);
        }
        Err(e) => {
            tracing::warn!(error = %e, "Softphone nicht gestartet");
            let detail = match &e {
                sf_core::phone::PhoneError::NoProvisioningRight(_) => t(
                    "Dem Benutzer fehlt in der Anlage das Recht für App-Telefone (uci_autoprovisioning). Der Administrator kann es unter Benutzer → Rechte freischalten.",
                )
                .to_owned(),
                _ => e.to_string(),
            };
            update_phone_status(&app, |s| {
                s.state = "error".into();
                s.detail = detail;
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
                    .state::<CallState>()
                    .status
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
                bus::publish(
                    &app,
                    Event::Calls {
                        calls: calls.clone(),
                    },
                );
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

#[tauri::command]
pub fn phone_status(state: State<'_, CallState>) -> PhoneStatus {
    let mut status = state.status.lock().unwrap().clone();
    if status.state.is_empty() {
        status.state = "off".into();
    }
    status
}

#[tauri::command]
pub async fn phone_dial(state: State<'_, CallState>, number: String) -> Result<(), String> {
    let number = clean_number(&number);
    if number.is_empty() {
        return Err(t("Keine Nummer").into());
    }
    with_phone(&state, async |p| p.dial(&number).await).await
}

#[tauri::command]
pub async fn phone_answer(
    app: AppHandle,
    state: State<'_, CallState>,
    call_id: String,
) -> Result<(), String> {
    let primary = settings::load(&app).prefs.primary_on_answer;
    with_phone(&state, async |p| {
        p.answer(&call_id)?;
        if primary && let Ok(hub) = crate::hub(&app.state::<AppState>()).await {
            make_primary(&hub, p.phone_id()).await;
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn phone_hangup(state: State<'_, CallState>, call_id: String) -> Result<(), String> {
    with_phone(&state, async |p| p.hangup(&call_id).await).await
}

#[tauri::command]
pub async fn phone_hold(
    state: State<'_, CallState>,
    call_id: String,
    hold: bool,
) -> Result<(), String> {
    with_phone(&state, async |p| p.hold(&call_id, hold).await).await
}

#[tauri::command]
pub async fn phone_mute(state: State<'_, CallState>, muted: bool) -> Result<(), String> {
    with_phone(&state, async |p| p.set_mute(muted)).await
}

#[tauri::command]
pub async fn phone_dtmf(
    state: State<'_, CallState>,
    call_id: String,
    digits: String,
) -> Result<(), String> {
    with_phone(&state, async |p| p.send_dtmf(&call_id, &digits).await).await
}

/// Weitere Funktionen des Call Managers.
#[tauri::command]
pub async fn phone_action(
    state: State<'_, CallState>,
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
    state: &CallState,
    f: impl AsyncFnOnce(&Phone) -> sf_core::phone::PhoneResult<()>,
) -> Result<(), String> {
    let phone = state.phone.lock().await;
    let phone = phone.as_ref().ok_or(t("Softphone ist nicht bereit"))?;
    f(phone).await.map_err(|e| e.to_string())
}

async fn restart_phone(app: &AppHandle) {
    app.state::<CallState>().phone.lock().await.take();
    let state = app.state::<AppState>();
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
