//! Telefonieren: das Softphone (baresip) als Audio-Endgerät, gesteuert über
//! die Anrufsteuerung der Anlage (gRPC `CallService`).
//!
//! Die Anlage kennt die Anrufe; das Softphone liefert nur den Ton. Beim Wählen
//! ruft die Anlage zuerst das Softphone an, das diesen Rückruf automatisch
//! annimmt. Eingehende Anrufe klingeln am Softphone und werden dort
//! angenommen. Auflegen, Halten und DTMF laufen über gRPC.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use sf_onehub::OneHub;
use sf_onehub::sf_proto::v1;
use sf_sip::{SipEvent, Softphone};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Löst die Anlage über das Betriebssystem auf (wie die HTTPS-Anmeldung),
/// IPv4 bevorzugt. `None`, wenn das scheitert; dann fragt baresip selbst.
async fn resolve(host: &str, port: u16) -> Option<std::net::SocketAddr> {
    match tokio::net::lookup_host((host, port)).await {
        Ok(addrs) => {
            let addrs: Vec<_> = addrs.collect();
            addrs
                .iter()
                .find(|a| a.is_ipv4())
                .or(addrs.first())
                .copied()
        }
        Err(e) => {
            tracing::warn!(host, error = %e, "Anlage nicht auflösbar");
            None
        }
    }
}

/// So lange nach `dial` gilt ein eingehender SIP-Anruf als Rückruf der
/// Anlage und wird automatisch angenommen.
const DIAL_WINDOW: Duration = Duration::from_secs(20);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// So lange nach dem Angebot der Anlage lässt sich „Rückruf bei Besetzt“
/// noch auslösen; sie beendet den besetzten Anruf meist sofort.
const CALLBACK_WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum PhoneError {
    #[error(transparent)]
    OneHub(#[from] sf_onehub::Error),
    #[error(transparent)]
    Sip(#[from] sf_sip::Error),
    #[error("App-Telefon SIP/{0} nicht auf der Anlage gefunden")]
    NoPhone(String),
    /// Dem Benutzer fehlt das Recht `uci_autoprovisioning`; ohne es gibt die
    /// Anlage keine SIP-Zugangsdaten für App-Telefone heraus.
    #[error("Recht „Autoprovisionierung“ (uci_autoprovisioning) fehlt: {0}")]
    NoProvisioningRight(String),
    #[error("SIP-Registrierung fehlgeschlagen: {0}")]
    Register(String),
    #[error("Dieser Anruf klingelt nicht am Softphone")]
    NotRinging,
    #[error("Keine Voicemailbox vorhanden")]
    NoMailbox,
    #[error("Kein besetzter Anruf, für den ein Rückruf möglich ist")]
    NoCallback,
    #[error("Dieser Anruf kommt nicht von einer Türsprechstelle")]
    NoDoor,
    #[error(
        "Call2Go angefordert, aber kein aktives Mobiltelefon (iFMC) hinterlegt. \
         Ohne weiteres Telefon passiert nichts."
    )]
    NoSwitchTarget,
}

pub type PhoneResult<T> = std::result::Result<T, PhoneError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallPhase {
    /// Wird aufgebaut, noch kein Klingeln
    Setup,
    /// Klingelt bei uns
    Ringing,
    /// Klingelt beim Gegenüber
    Ringback,
    Connected,
    Held,
    Other,
}

/// Ein Anruf, wie ihn die Oberfläche zeigt.
#[derive(Debug, Clone, Serialize)]
pub struct CallView {
    pub id: String,
    pub phase: CallPhase,
    pub incoming: bool,
    /// Interner Anruf (für den passenden Klingelton)
    pub internal: bool,
    pub remote_name: String,
    pub remote_number: String,
    /// Eigene Nummer bzw. Gruppe, über die der Anruf kam
    pub local_name: String,
    pub local_number: String,
    /// Bei einer Rückfrage: der gehaltene Anruf, zu dem sie gehört
    pub consultation_of: Option<String>,
    pub recording: bool,
    /// Unix-Zeit in Millisekunden
    pub connected_since: Option<i64>,
    /// Anruf einer Türsprechstelle mit Kamera
    pub door_cam: bool,
    /// Tür lässt sich per DTMF öffnen
    pub door_open: bool,
    #[serde(skip)]
    sip_call_ids: Vec<String>,
    /// Kamera-URL der Türsprechstelle; kann Zugangsdaten enthalten und
    /// bleibt deshalb im Backend.
    #[serde(skip)]
    door_cam_url: String,
    #[serde(skip)]
    door_dtmf: String,
}

impl CallView {
    fn set_door(&mut self, cam_url: String, dtmf: String) {
        self.door_cam = !cam_url.trim().is_empty();
        self.door_open = !dtmf.trim().is_empty();
        self.door_cam_url = cam_url.trim().to_owned();
        self.door_dtmf = dtmf.trim().to_owned();
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PhoneEvent {
    /// Softphone an der Anlage angemeldet (`true`) oder nicht
    Registered {
        ok: bool,
        detail: String,
    },
    Calls {
        calls: Vec<CallView>,
        muted: bool,
        /// Rückruf bei Besetzt: "available" (angeboten), "active" oder ""
        callback: &'static str,
    },
    Error {
        message: String,
    },
}

#[derive(Default)]
struct Inner {
    calls: BTreeMap<String, CallView>,
    /// Eingehende SIP-Anrufe, die noch klingeln
    sip_ringing: Vec<String>,
    /// Angenommene SIP-Anrufe
    sip_active: Vec<String>,
    dial_until: Option<Instant>,
    muted: bool,
    /// Anruf, für den die Anlage „Rückruf bei Besetzt“ anbietet, seit wann
    callback_offer: Option<(String, Instant)>,
    /// Anrufe mit aktivem Rückruf bei Besetzt
    callback_active: Vec<String>,
}

impl Inner {
    fn callback_state(&self) -> &'static str {
        if !self.callback_active.is_empty() {
            "active"
        } else if self
            .callback_offer
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() < CALLBACK_WINDOW)
        {
            "available"
        } else {
            ""
        }
    }
}

pub struct Phone {
    hub: OneHub,
    phone_id: String,
    sip: Arc<Softphone>,
    inner: Arc<Mutex<Inner>>,
    events: mpsc::UnboundedSender<PhoneEvent>,
    tasks: Vec<JoinHandle<()>>,
}

impl Phone {
    /// Holt die SIP-Zugangsdaten, meldet das Softphone an und abonniert die
    /// Anruf-Ereignisse der Anlage.
    pub async fn start(
        hub: OneHub,
        host: &str,
        config: &sf_sip::Config,
        app_version: &str,
        events: mpsc::UnboundedSender<PhoneEvent>,
    ) -> PhoneResult<Self> {
        let creds = match hub
            .register_sip_device(sf_onehub::SIP_DEVICE_ID, app_version)
            .await
        {
            Ok(c) => c,
            // Ohne uci_autoprovisioning legt die Anlage kein Linux-App-Telefon
            // an; das einer Desktop-App bekommt jeder Benutzer.
            Err(e) if e.permission_denied().is_some() => {
                tracing::info!(
                    reason = e.permission_denied(),
                    "Linux-App-Telefon verweigert, nehme das einer Desktop-App"
                );
                hub.register_sip_device(sf_onehub::SIP_DEVICE_ID_FALLBACK, app_version)
                    .await
                    .map_err(|e| match e.permission_denied() {
                        Some(msg) => PhoneError::NoProvisioningRight(msg.to_owned()),
                        None => e.into(),
                    })?
            }
            Err(e) => return Err(e.into()),
        };
        let phone_id = hub
            .phone_id_for_sip_user(&creds.user)
            .await?
            .ok_or_else(|| PhoneError::NoPhone(creds.user.clone()))?;

        // Ohne Zertifikatsprüfung (Cloud, bestätigte Zertifikate) darf
        // baresip die Anlage per IP ansprechen; die löst hier das System auf.
        let outbound = if config.verify_server {
            None
        } else {
            resolve(host, creds.port).await
        };

        let software = format!("starclx/{app_version}");
        let (sip, mut sip_rx) = Softphone::start(config, &software)?;
        let sip = Arc::new(sip);
        sip.add_account(&sf_sip::Account {
            user: creds.user,
            password: creds.password,
            host: host.to_owned(),
            port: creds.port,
            register_interval: 3600,
            outbound,
        })?;

        // Auf die erste Registrierung warten, damit ein Fehler beim Start
        // sichtbar wird.
        let first = tokio::time::timeout(Duration::from_secs(15), async {
            while let Some(ev) = sip_rx.recv().await {
                match ev {
                    SipEvent::Registered { .. } => return Ok(()),
                    SipEvent::RegisterFailed { reason, .. } => return Err(reason),
                    _ => {}
                }
            }
            Err("Softphone beendet".to_owned())
        })
        .await
        .unwrap_or_else(|_| Err("keine Antwort der Anlage".to_owned()));
        first.map_err(PhoneError::Register)?;
        let _ = events.send(PhoneEvent::Registered {
            ok: true,
            detail: String::new(),
        });

        let inner = Arc::new(Mutex::new(Inner::default()));
        let mut phone = Self {
            hub,
            phone_id,
            sip,
            inner,
            events,
            tasks: Vec::new(),
        };
        phone.tasks.push(phone.spawn_sip_driver(sip_rx));
        phone.tasks.push(phone.spawn_pbx_driver());
        Ok(phone)
    }

    /// ID des App-Telefons auf der Anlage
    pub fn phone_id(&self) -> &str {
        &self.phone_id
    }

    /// Wählt über die Anlage. Sie ruft zuerst das Softphone an.
    pub async fn dial(&self, number: &str) -> PhoneResult<()> {
        self.inner.lock().unwrap().dial_until = Some(Instant::now() + DIAL_WINDOW);
        let req = v1::call::PlaceCallRequest {
            number: number.to_owned(),
            requested_call_id: None,
            phone_id: Some(v1::types::PhoneId {
                id: self.phone_id.clone(),
            }),
        };
        if let Err(e) = self.hub.call().place_call(req).await {
            self.inner.lock().unwrap().dial_until = None;
            return Err(sf_onehub::Error::from(e).into());
        }
        Ok(())
    }

    /// Rückruf bei Besetzt: aktiviert ihn für den zuletzt besetzten Anruf
    /// oder bricht aktive Rückrufe ab. Liefert, ob danach einer aktiv ist.
    pub async fn toggle_callback(&self) -> PhoneResult<bool> {
        let (offer, active) = {
            let inner = self.inner.lock().unwrap();
            let offer = inner
                .callback_offer
                .clone()
                .filter(|(_, t)| t.elapsed() < CALLBACK_WINDOW)
                .map(|(id, _)| id);
            (offer, inner.callback_active.clone())
        };
        let mut svc = self.hub.call_back_on_busy();
        if let Some(id) = offer {
            svc.activate_call_back_on_busy(v1::ccbs::ActivateCallBackOnBusyRequest {
                call_id: Some(call_id_of(&id)),
            })
            .await
            .map_err(sf_onehub::Error::from)?;
            return Ok(true);
        }
        if active.is_empty() {
            return Err(PhoneError::NoCallback);
        }
        for id in active {
            svc.deactivate_call_back_on_busy(v1::ccbs::DeactivateCallBackOnBusyRequest {
                call_id: Some(call_id_of(&id)),
            })
            .await
            .map_err(sf_onehub::Error::from)?;
        }
        Ok(false)
    }

    /// Nimmt einen eingehenden Anruf am Softphone an.
    pub fn answer(&self, call_id: &str) -> PhoneResult<()> {
        let sip_id = {
            let inner = self.inner.lock().unwrap();
            let known = inner
                .calls
                .get(call_id)
                .map(|c| c.sip_call_ids.clone())
                .unwrap_or_default();
            inner
                .sip_ringing
                .iter()
                .find(|id| known.contains(id))
                .or_else(|| {
                    // Ohne Zuordnung nur, wenn es eindeutig ist.
                    (inner.sip_ringing.len() == 1).then(|| &inner.sip_ringing[0])
                })
                .cloned()
                .ok_or(PhoneError::NotRinging)?
        };
        self.sip.answer(&sip_id)?;
        Ok(())
    }

    pub async fn hangup(&self, call_id: &str) -> PhoneResult<()> {
        let req = v1::call::HangupCallRequest {
            call_id: Some(call_id_of(call_id)),
        };
        self.hub
            .call()
            .hangup_call(req)
            .await
            .map_err(sf_onehub::Error::from)?;
        Ok(())
    }

    pub async fn hold(&self, call_id: &str, hold: bool) -> PhoneResult<()> {
        let call = self.hub.call();
        let result = if hold {
            call.clone()
                .hold_call(v1::call::HoldCallRequest {
                    call_id: Some(call_id_of(call_id)),
                })
                .await
        } else {
            call.clone()
                .resume_call(v1::call::ResumeCallRequest {
                    call_id: Some(call_id_of(call_id)),
                    phone_id: Some(v1::types::PhoneId {
                        id: self.phone_id.clone(),
                    }),
                })
                .await
        };
        result.map_err(sf_onehub::Error::from)?;
        Ok(())
    }

    pub async fn send_dtmf(&self, call_id: &str, digits: &str) -> PhoneResult<()> {
        let req = v1::call::SendDtmfRequest {
            call_id: Some(call_id_of(call_id)),
            digits: digits.to_owned(),
        };
        self.hub
            .call()
            .send_dtmf(req)
            .await
            .map_err(sf_onehub::Error::from)?;
        Ok(())
    }

    /// Kamera-URL der Türsprechstelle, von der dieser Anruf kommt
    pub fn door_cam_url(&self, call_id: &str) -> Option<String> {
        let inner = self.inner.lock().unwrap();
        let call = inner.calls.get(call_id)?;
        call.door_cam.then(|| call.door_cam_url.clone())
    }

    /// Öffnet die Tür: schickt den DTMF-Code, den die Anlage für die
    /// Türsprechstelle hinterlegt hat (wie „Tür öffnen“ im Windows-Client).
    pub async fn open_door(&self, call_id: &str) -> PhoneResult<()> {
        let code = {
            let inner = self.inner.lock().unwrap();
            inner
                .calls
                .get(call_id)
                .filter(|c| c.door_open)
                .map(|c| c.door_dtmf.clone())
                .ok_or(PhoneError::NoDoor)?
        };
        self.send_dtmf(call_id, &code).await
    }

    /// Leitet einen (klingelnden) Anruf an eine andere Nummer weiter.
    pub async fn forward(&self, call_id: &str, number: &str) -> PhoneResult<()> {
        let req = v1::call::ForwardCallRequest {
            call_id: Some(call_id_of(call_id)),
            number: number.to_owned(),
        };
        self.hub
            .call()
            .forward_call(req)
            .await
            .map_err(sf_onehub::Error::from)?;
        Ok(())
    }

    /// Schickt einen Anruf auf die erste eigene Voicemailbox.
    pub async fn to_voicemail(&self, call_id: &str) -> PhoneResult<()> {
        let mut vm = self.hub.voicemail();
        let mailbox = vm
            .get_mailboxes(())
            .await
            .map_err(sf_onehub::Error::from)?
            .into_inner()
            .mailboxes
            .into_iter()
            .next()
            .and_then(|m| m.mailbox_id)
            .ok_or(PhoneError::NoMailbox)?;
        let req = v1::voicemail::TransferCallToVoicemailBoxRequest {
            call_id: Some(call_id_of(call_id)),
            mailbox_id: Some(mailbox),
        };
        vm.transfer_call_to_voicemail_box(req)
            .await
            .map_err(sf_onehub::Error::from)?;
        Ok(())
    }

    /// Startet oder beendet die Aufnahme auf der Anlage.
    pub async fn record(&self, call_id: &str) -> PhoneResult<()> {
        let req = v1::call::RecordCallRequest {
            call_id: Some(call_id_of(call_id)),
        };
        self.hub
            .call()
            .record_call(req)
            .await
            .map_err(sf_onehub::Error::from)?;
        Ok(())
    }

    /// Rufweitergabe (Call2Go): das Gespräch auf ein anderes eigenes Telefon
    /// holen lassen.
    pub async fn switch_phone(&self, call_id: &str) -> PhoneResult<()> {
        let req = v1::call::SwitchPhoneRequest {
            call_id: Some(call_id_of(call_id)),
        };
        self.hub
            .call()
            .switch_phone(req)
            .await
            .map_err(sf_onehub::Error::from)?;
        // Die Anlage nimmt den Auftrag auch ohne Ziel an und tut dann nichts.
        // Ein Tischtelefon kann ebenfalls Ziel sein, daher nur ein Hinweis
        // nach dem Auftrag, wenn kein aktives iFMC-Telefon existiert.
        let fmc = self
            .hub
            .fmc_phone()
            .get_fmc_phones(())
            .await
            .map_err(sf_onehub::Error::from)?
            .into_inner()
            .fmc_phones;
        if !fmc.iter().any(|p| p.enabled) {
            return Err(PhoneError::NoSwitchTarget);
        }
        Ok(())
    }

    /// Rückfrage: hält `call_id` und ruft `number` an.
    pub async fn consult(&self, call_id: &str, number: &str) -> PhoneResult<()> {
        self.inner.lock().unwrap().dial_until = Some(Instant::now() + DIAL_WINDOW);
        let req = v1::call::PlaceConsultationCallRequest {
            number: number.to_owned(),
            call_id: Some(call_id_of(call_id)),
            phone_id: Some(v1::types::PhoneId {
                id: self.phone_id.clone(),
            }),
        };
        if let Err(e) = self.hub.call().place_consultation_call(req).await {
            self.inner.lock().unwrap().dial_until = None;
            return Err(sf_onehub::Error::from(e).into());
        }
        Ok(())
    }

    /// Verbindet die Rückfrage mit dem gehaltenen Anruf und steigt aus.
    pub async fn transfer_consultation(&self, call_id: &str) -> PhoneResult<()> {
        let req = v1::call::TransferConsultationCallRequest {
            call_id: Some(call_id_of(call_id)),
        };
        self.hub
            .call()
            .transfer_consultation_call(req)
            .await
            .map_err(sf_onehub::Error::from)?;
        Ok(())
    }

    /// Startet eine Konferenz aus den angegebenen Anrufen (z. B. gehaltenes
    /// Gespräch und Rückfrage).
    pub async fn conference(&self, call_ids: &[String]) -> PhoneResult<()> {
        let req = v1::conference::InitiateConferenceRequest {
            call_ids: call_ids.iter().map(|id| call_id_of(id)).collect(),
            phone_id: Some(v1::types::PhoneId {
                id: self.phone_id.clone(),
            }),
        };
        self.hub
            .conference_call()
            .initiate_conference(req)
            .await
            .map_err(sf_onehub::Error::from)?;
        Ok(())
    }

    /// Nach dem Aufwachen aus dem Standby: SIP-Verbindung neu aufbauen und
    /// neu registrieren, sonst laufen eingehende Anrufe bis zur nächsten
    /// regulären Registrierung ins Leere.
    pub fn resume(&self) {
        if let Err(e) = self.sip.reset() {
            tracing::warn!(error = %e, "Softphone nicht neu verbunden");
        }
    }

    /// Schaltet das Mikrofon für alle Gespräche am Softphone stumm.
    pub fn set_mute(&self, muted: bool) -> PhoneResult<()> {
        let active = {
            let mut inner = self.inner.lock().unwrap();
            inner.muted = muted;
            inner.sip_active.clone()
        };
        for id in active {
            self.sip.set_mute(&id, muted)?;
        }
        self.publish();
        Ok(())
    }

    pub fn calls(&self) -> Vec<CallView> {
        self.inner.lock().unwrap().calls.values().cloned().collect()
    }

    fn publish(&self) {
        publish(&self.inner, &self.events);
    }

    fn spawn_sip_driver(&self, mut rx: mpsc::UnboundedReceiver<SipEvent>) -> JoinHandle<()> {
        let inner = self.inner.clone();
        let events = self.events.clone();
        let sip = self.sip.clone();
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                tracing::debug!(?ev, "SIP");
                match ev {
                    SipEvent::Registered { .. } => {
                        let _ = events.send(PhoneEvent::Registered {
                            ok: true,
                            detail: String::new(),
                        });
                    }
                    SipEvent::RegisterFailed { reason, .. } => {
                        let _ = events.send(PhoneEvent::Registered {
                            ok: false,
                            detail: reason,
                        });
                    }
                    SipEvent::Incoming { call, auto_answer } => {
                        let answer = {
                            let mut inner = inner.lock().unwrap();
                            let ours = inner.dial_until.is_some_and(|t| Instant::now() < t);
                            if auto_answer || ours {
                                inner.dial_until = None;
                                true
                            } else {
                                inner.sip_ringing.push(call.call_id.clone());
                                false
                            }
                        };
                        if answer && let Err(e) = sip.answer(&call.call_id) {
                            let _ = events.send(PhoneEvent::Error {
                                message: e.to_string(),
                            });
                        }
                    }
                    SipEvent::Established { call_id } => {
                        let muted = {
                            let mut inner = inner.lock().unwrap();
                            inner.sip_ringing.retain(|id| *id != call_id);
                            inner.sip_active.push(call_id.clone());
                            inner.muted
                        };
                        if muted {
                            let _ = sip.set_mute(&call_id, true);
                        }
                    }
                    SipEvent::Closed { call_id, .. } => {
                        let mut inner = inner.lock().unwrap();
                        inner.sip_ringing.retain(|id| *id != call_id);
                        inner.sip_active.retain(|id| *id != call_id);
                        if inner.sip_active.is_empty() {
                            inner.muted = false;
                        }
                    }
                    SipEvent::AudioError { info } => {
                        let _ = events.send(PhoneEvent::Error {
                            message: format!("Audiofehler: {info}"),
                        });
                    }
                    SipEvent::Error { context } => {
                        tracing::warn!(%context, "Softphone-Fehler");
                    }
                    _ => {}
                }
                publish(&inner, &events);
            }
        })
    }

    fn spawn_pbx_driver(&self) -> JoinHandle<()> {
        let hub = self.hub.clone();
        let inner = self.inner.clone();
        let events = self.events.clone();
        tokio::spawn(crate::reconnect::forever(
            "Anruf-Ereignisse",
            MAX_BACKOFF,
            move || {
                let (hub, inner, events) = (hub.clone(), inner.clone(), events.clone());
                async move { pbx_session(&hub, &inner, &events).await }
            },
        ))
    }
}

impl Drop for Phone {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// Liest den aktuellen Stand und dann den Ereignisstrom, bis er endet.
async fn pbx_session(
    hub: &OneHub,
    inner: &Arc<Mutex<Inner>>,
    events: &mpsc::UnboundedSender<PhoneEvent>,
) -> sf_onehub::Result<()> {
    let calls = hub.call().get_calls(()).await?.into_inner().calls;
    {
        let mut inner = inner.lock().unwrap();
        inner.calls = calls
            .iter()
            .map(|c| (id_of(&c.call_id), view_of(c)))
            .collect();
    }
    publish(inner, events);

    let mut stream = hub.call().subscribe_call_events(()).await?.into_inner();
    while let Some(ev) = stream.message().await? {
        if let Some(ev) = ev.call_event {
            tracing::debug!(?ev, "Anlage");
            let offer = matches!(
                ev,
                v1::call::call_event_response::CallEvent::CallBackOnBusyStateChanged(_)
            );
            apply(&mut inner.lock().unwrap(), ev);
            publish(inner, events);
            // Abgelaufenes Angebot auch ohne weiteres Ereignis zurücknehmen
            if offer {
                let (inner, events) = (inner.clone(), events.clone());
                tokio::spawn(async move {
                    tokio::time::sleep(CALLBACK_WINDOW + Duration::from_secs(1)).await;
                    publish(&inner, &events);
                });
            }
        }
    }
    Ok(())
}

fn apply(inner: &mut Inner, ev: v1::call::call_event_response::CallEvent) {
    use v1::call::call_event_response::CallEvent as E;
    match ev {
        E::CallCreated(e) => {
            for c in &e.calls {
                inner.calls.insert(id_of(&c.call_id), view_of(c));
            }
        }
        E::CallProvisionalChanged(e) => {
            if let Some(c) = inner.calls.get_mut(&id_of(&e.call_id)) {
                if let Some(r) = e.remote_participant {
                    c.remote_name = r.name;
                    c.remote_number = r.number;
                }
                if e.door_line_cam_url.is_some() || e.door_line_dtmf_code.is_some() {
                    c.set_door(
                        e.door_line_cam_url
                            .unwrap_or_else(|| c.door_cam_url.clone()),
                        e.door_line_dtmf_code.unwrap_or_else(|| c.door_dtmf.clone()),
                    );
                }
            }
        }
        E::CallStateChanged(e) => {
            if let Some(c) = inner.calls.get_mut(&id_of(&e.call_id)) {
                c.phase = phase_of(e.call_state());
                if let Some(ts) = e.connected_timestamp {
                    c.connected_since = Some(millis(&ts));
                }
            }
        }
        E::CallRecordingChanged(e) => {
            if let Some(c) = inner.calls.get_mut(&id_of(&e.call_id)) {
                c.recording = e.record_by != 0;
            }
        }
        E::CallMetadataChanged(e) => {
            if let Some(c) = inner.calls.get_mut(&id_of(&e.call_id)) {
                c.sip_call_ids = e.sip_call_ids;
            }
        }
        E::CallDisconnected(e) => {
            inner.calls.remove(&id_of(&e.call_id));
        }
        E::CallBackOnBusyStateChanged(e) => {
            use v1::call::CallBackOnBusyState as S;
            let id = id_of(&e.call_id);
            match e.call_back_on_busy_state() {
                S::Available => inner.callback_offer = Some((id, Instant::now())),
                S::Active => {
                    if !inner.callback_active.contains(&id) {
                        inner.callback_active.push(id.clone());
                    }
                    inner.callback_offer.take_if(|(o, _)| *o == id);
                }
                S::Inactive => {
                    inner.callback_active.retain(|a| *a != id);
                    inner.callback_offer.take_if(|(o, _)| *o == id);
                }
                S::Unspecified => {}
            }
        }
        _ => {}
    }
}

fn publish(inner: &Mutex<Inner>, events: &mpsc::UnboundedSender<PhoneEvent>) {
    let (calls, muted, callback) = {
        let inner = inner.lock().unwrap();
        (
            inner.calls.values().cloned().collect(),
            inner.muted,
            inner.callback_state(),
        )
    };
    let _ = events.send(PhoneEvent::Calls {
        calls,
        muted,
        callback,
    });
}

fn call_id_of(id: &str) -> v1::types::CallId {
    v1::types::CallId { id: id.to_owned() }
}

fn id_of(id: &Option<v1::types::CallId>) -> String {
    id.as_ref().map(|c| c.id.clone()).unwrap_or_default()
}

fn millis(ts: &prost_types::Timestamp) -> i64 {
    ts.seconds * 1000 + i64::from(ts.nanos / 1_000_000)
}

fn phase_of(state: v1::types::CallState) -> CallPhase {
    use v1::types::CallState as S;
    match state {
        S::Provisional => CallPhase::Setup,
        S::Ringing => CallPhase::Ringing,
        S::RingBack => CallPhase::Ringback,
        S::Connected | S::ConferenceActive => CallPhase::Connected,
        S::Parked => CallPhase::Held,
        _ => CallPhase::Other,
    }
}

fn view_of(c: &v1::call::Call) -> CallView {
    let remote = c.remote_participant.clone().unwrap_or_default();
    let local = c.local_participant.clone().unwrap_or_default();
    let mut view = CallView {
        id: id_of(&c.call_id),
        phase: phase_of(c.call_state()),
        incoming: c.call_direction() == v1::types::CallDirection::Inbound,
        internal: c.is_internal_call,
        remote_name: remote.name,
        remote_number: remote.number,
        local_name: local.name,
        local_number: local.number,
        consultation_of: c
            .consultation_call_id
            .as_ref()
            .map(|id| id.id.clone())
            .filter(|id| !id.is_empty()),
        recording: c.record_by != 0,
        connected_since: c.connected_timestamp.as_ref().map(millis),
        door_cam: false,
        door_open: false,
        sip_call_ids: c.sip_call_ids.clone(),
        door_cam_url: String::new(),
        door_dtmf: String::new(),
    };
    view.set_door(c.door_line_cam_url.clone(), c.door_line_dtmf_code.clone());
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    use v1::call::call_event_response::CallEvent as E;

    fn call(id: &str, state: v1::types::CallState) -> v1::call::Call {
        v1::call::Call {
            call_id: Some(call_id_of(id)),
            call_state: state as i32,
            call_direction: v1::types::CallDirection::Inbound as i32,
            remote_participant: Some(v1::types::RemoteParticipant {
                number: "12".into(),
                name: "Kollege".into(),
                user_id: None,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn events_build_call_list() {
        let mut inner = Inner::default();
        apply(
            &mut inner,
            E::CallCreated(v1::call::CallCreatedEvent {
                calls: vec![call("a", v1::types::CallState::Ringing)],
            }),
        );
        let c = &inner.calls["a"];
        assert_eq!(c.phase, CallPhase::Ringing);
        assert!(c.incoming);
        assert_eq!(c.remote_name, "Kollege");

        apply(
            &mut inner,
            E::CallStateChanged(v1::call::CallStateChangedEvent {
                call_id: Some(call_id_of("a")),
                call_state: v1::types::CallState::Connected as i32,
                connected_timestamp: Some(prost_types::Timestamp {
                    seconds: 10,
                    nanos: 500_000_000,
                }),
            }),
        );
        assert_eq!(inner.calls["a"].phase, CallPhase::Connected);
        assert_eq!(inner.calls["a"].connected_since, Some(10_500));

        apply(
            &mut inner,
            E::CallDisconnected(v1::call::CallDisconnectedEvent {
                call_id: Some(call_id_of("a")),
            }),
        );
        assert!(inner.calls.is_empty());
    }

    #[test]
    fn door_line_brings_camera_and_code() {
        let mut inner = Inner::default();
        let mut door = call("t", v1::types::CallState::Ringing);
        door.door_line_cam_url = "http://admin:geheim@tuer/video.mjpg".into();
        apply(
            &mut inner,
            E::CallCreated(v1::call::CallCreatedEvent { calls: vec![door] }),
        );
        let c = &inner.calls["t"];
        assert!(c.door_cam && !c.door_open);
        // Die Zugangsdaten gehen nicht an die Oberfläche.
        assert!(!serde_json::to_string(c).unwrap().contains("geheim"));

        apply(
            &mut inner,
            E::CallProvisionalChanged(v1::call::CallProvisionalChangedEvent {
                call_id: Some(call_id_of("t")),
                door_line_dtmf_code: Some("*9".into()),
                ..Default::default()
            }),
        );
        let c = &inner.calls["t"];
        assert!(c.door_cam && c.door_open);
        assert_eq!(c.door_cam_url, "http://admin:geheim@tuer/video.mjpg");
        assert_eq!(c.door_dtmf, "*9");
    }
}
