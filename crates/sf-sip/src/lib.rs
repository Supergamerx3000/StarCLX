//! Softphone auf Basis von libbaresip.
//!
//! Das Softphone ist bewusst dumm: Es registriert sich, nimmt Anrufe an,
//! legt auf, schaltet stumm und sendet DTMF. Anrufe starten, halten und
//! übergeben läuft über die OneHub-API; die Anlage ruft dafür das Softphone
//! an (siehe `docs/windows-client-analyse.md`).
//!
//! baresip ist ein Prozess-Singleton und läuft auf einem eigenen Thread.
//! Befehle gehen über eine Warteschlange dorthin, Ereignisse kommen über
//! einen tokio-Kanal zurück.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::sync::mpsc;

mod ffi {
    use super::*;

    pub const OP_ADD_UA: c_int = 1;
    pub const OP_ANSWER: c_int = 2;
    pub const OP_HANGUP: c_int = 3;
    pub const OP_MUTE: c_int = 4;
    pub const OP_UNMUTE: c_int = 5;
    pub const OP_DTMF: c_int = 6;
    pub const OP_CONNECT: c_int = 7;
    pub const OP_QUIT: c_int = 8;
    pub const OP_RESET: c_int = 9;
    pub const OP_AUSRC: c_int = 10;

    pub const EV_READY: c_int = 1;
    pub const EV_ERROR: c_int = 2;
    pub const EV_REGISTER_OK: c_int = 3;
    pub const EV_REGISTER_FAIL: c_int = 4;
    pub const EV_CALL_INCOMING: c_int = 5;
    pub const EV_CALL_OUTGOING: c_int = 6;
    pub const EV_CALL_RINGING: c_int = 7;
    pub const EV_CALL_ESTABLISHED: c_int = 8;
    pub const EV_CALL_CLOSED: c_int = 9;
    pub const EV_CALL_MENC: c_int = 10;
    pub const EV_AUDIO_ERROR: c_int = 11;
    pub const EV_CALL_DTMF: c_int = 12;
    pub const EV_END_OF_FILE: c_int = 13;

    #[repr(C)]
    pub struct Event {
        pub ev: c_int,
        pub aor: *const c_char,
        pub call_id: *const c_char,
        pub peer_uri: *const c_char,
        pub peer_name: *const c_char,
        pub text: *const c_char,
        pub answer_delay: c_int,
        pub outgoing: c_int,
    }

    pub type EventCb = unsafe extern "C" fn(ctx: *mut c_void, e: *const Event);

    unsafe extern "C" {
        pub fn sfsip_run(
            config: *const c_char,
            software: *const c_char,
            cb: EventCb,
            ctx: *mut c_void,
        ) -> c_int;
        pub fn sfsip_cmd(op: c_int, a: *const c_char, b: *const c_char) -> c_int;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Softphone läuft bereits in diesem Prozess")]
    AlreadyRunning,
    #[error("Softphone konnte nicht starten")]
    StartFailed,
    #[error("Softphone läuft nicht")]
    NotRunning,
    #[error("ungültiger Wert: {0}")]
    InvalidValue(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallInfo {
    pub call_id: String,
    pub peer_uri: String,
    pub peer_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SipEvent {
    Registered {
        aor: String,
    },
    RegisterFailed {
        aor: String,
        reason: String,
    },
    /// Eingehender Anruf. `auto_answer` ist gesetzt, wenn die Anlage per
    /// Call-Info/Alert-Info um automatische Annahme bittet.
    Incoming {
        call: CallInfo,
        auto_answer: bool,
    },
    Outgoing {
        call: CallInfo,
    },
    Ringing {
        call_id: String,
    },
    Established {
        call_id: String,
    },
    Closed {
        call_id: String,
        reason: String,
    },
    /// Medienverschlüsselung ausgehandelt, z. B. "0,audio,AES_CM_128_HMAC_SHA1_80".
    MediaEncryption {
        call_id: String,
        info: String,
    },
    AudioError {
        info: String,
    },
    /// Taste vom Gegenüber empfangen (RTP-Event oder SIP INFO)
    Dtmf {
        call_id: String,
        key: char,
    },
    /// Die Audioquelle `aufile` ist am Dateiende angekommen.
    EndOfFile {
        call_id: String,
    },
    Error {
        context: String,
    },
}

/// Allgemeine Einstellungen des Softphones.
#[derive(Debug, Clone)]
pub struct Config {
    /// z. B. `pipewire` oder `pulse`, optional mit Gerät: `alsa,default`.
    pub audio_player: String,
    pub audio_source: String,
    /// Feste lokale SIP-Adresse, sonst wählt baresip freie Ports.
    pub sip_listen: Option<String>,
    /// TLS-Zertifikat der Anlage prüfen.
    pub verify_server: bool,
    pub ca_file: Option<String>,
    /// Weitere baresip-Konfigurationszeilen (für Tests und Sonderfälle).
    pub extra: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            audio_player: "pipewire".into(),
            audio_source: "pipewire".into(),
            sip_listen: None,
            verify_server: true,
            ca_file: Some("/etc/ssl/certs/ca-certificates.crt".into()),
            extra: String::new(),
        }
    }
}

impl Config {
    fn render(&self) -> String {
        let mut c = String::new();
        if let Some(l) = &self.sip_listen {
            c += &format!("sip_listen {l}\n");
        }
        c += &format!(
            "sip_verify_server {}\n",
            if self.verify_server { "yes" } else { "no" }
        );
        if let Some(ca) = &self.ca_file {
            c += &format!("sip_cafile {ca}\n");
        }
        c += &format!("audio_player {}\n", self.audio_player);
        c += &format!("audio_source {}\n", self.audio_source);
        c += &format!("audio_alert {}\n", self.audio_player);
        c += "call_max_calls 4\n";
        for m in [
            "g711", "g722", "srtp", "auconv", "auresamp", "stun", "netroam", "pipewire", "pulse",
            "alsa", "ausine", "aubridge", "aufile",
        ] {
            c += &format!("module {m}.so\n");
        }
        c += &self.extra;
        c
    }
}

/// SIP-Zugang, wie ihn `SipDeviceService.RegisterSipDevice` liefert.
#[derive(Clone)]
pub struct Account {
    pub user: String,
    pub password: String,
    pub host: String,
    pub port: u16,
    /// Registrierungsintervall in Sekunden; 0 = nicht registrieren.
    pub register_interval: u32,
    /// Bereits aufgelöste Adresse der Anlage. baresip schickt dann alles
    /// dorthin und fragt kein DNS; dessen eigener Resolver findet unter
    /// Windows manche Namen nicht, die das System auflöst.
    pub outbound: Option<SocketAddr>,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("user", &self.user)
            .field("password", &"***")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("register_interval", &self.register_interval)
            .field("outbound", &self.outbound)
            .finish()
    }
}

impl Account {
    /// baresip-Kontozeile: TLS, SRTP Pflicht (die Anlage verlangt SDES),
    /// manuelle Annahme (die Entscheidung trifft der Aufrufer).
    pub fn to_aor(&self) -> Result<String> {
        for (name, value) in [
            ("user", &self.user),
            ("password", &self.password),
            ("host", &self.host),
        ] {
            if value.is_empty() || value.contains([';', '"', '<', '>', '\n', '\r', ' ']) {
                return Err(Error::InvalidValue(name.into()));
            }
        }
        let outbound = self
            .outbound
            .map(|a| format!(";outbound=\"sip:{a};transport=tls\""))
            .unwrap_or_default();
        Ok(format!(
            "<sip:{user}@{host}:{port};transport=tls>;auth_pass={pw};mediaenc=srtp-mand;\
             regint={regint};answermode=manual;audio_codecs=g722,pcma,pcmu{outbound}",
            user = self.user,
            host = self.host,
            port = self.port,
            pw = self.password,
            regint = self.register_interval,
        ))
    }
}

static RUNNING: AtomicBool = AtomicBool::new(false);

struct Ctx {
    events: mpsc::UnboundedSender<SipEvent>,
    ready: std::sync::mpsc::SyncSender<bool>,
}

fn opt_str(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: baresip übergibt nullterminierte Zeichenketten, die für
        // die Dauer des Callbacks gültig sind.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

unsafe extern "C" fn on_event(ctx: *mut c_void, e: *const ffi::Event) {
    // SAFETY: ctx ist der Box<Ctx>-Zeiger aus `Softphone::start`, der bis
    // nach dem Ende von sfsip_run lebt; e ist für diesen Aufruf gültig.
    let (ctx, e) = unsafe { (&*(ctx as *const Ctx), &*e) };
    let call = || CallInfo {
        call_id: opt_str(e.call_id),
        peer_uri: opt_str(e.peer_uri),
        peer_name: opt_str(e.peer_name),
    };
    let event = match e.ev {
        ffi::EV_READY => {
            let _ = ctx.ready.try_send(true);
            return;
        }
        ffi::EV_ERROR => {
            let context = opt_str(e.text);
            if context == "init" {
                let _ = ctx.ready.try_send(false);
            }
            SipEvent::Error { context }
        }
        ffi::EV_REGISTER_OK => SipEvent::Registered {
            aor: opt_str(e.aor),
        },
        ffi::EV_REGISTER_FAIL => SipEvent::RegisterFailed {
            aor: opt_str(e.aor),
            reason: opt_str(e.text),
        },
        ffi::EV_CALL_INCOMING => SipEvent::Incoming {
            call: call(),
            auto_answer: e.answer_delay >= 0,
        },
        ffi::EV_CALL_OUTGOING => SipEvent::Outgoing { call: call() },
        ffi::EV_CALL_RINGING => SipEvent::Ringing {
            call_id: opt_str(e.call_id),
        },
        ffi::EV_CALL_ESTABLISHED => SipEvent::Established {
            call_id: opt_str(e.call_id),
        },
        ffi::EV_CALL_CLOSED => SipEvent::Closed {
            call_id: opt_str(e.call_id),
            reason: opt_str(e.text),
        },
        ffi::EV_CALL_MENC => SipEvent::MediaEncryption {
            call_id: opt_str(e.call_id),
            info: opt_str(e.text),
        },
        ffi::EV_AUDIO_ERROR => SipEvent::AudioError {
            info: opt_str(e.text),
        },
        ffi::EV_CALL_DTMF => match opt_str(e.text).chars().next() {
            Some(key) => SipEvent::Dtmf {
                call_id: opt_str(e.call_id),
                key,
            },
            None => return,
        },
        ffi::EV_END_OF_FILE => SipEvent::EndOfFile {
            call_id: opt_str(e.call_id),
        },
        _ => return,
    };
    let _ = ctx.events.send(event);
}

/// Laufendes Softphone. Beim Drop wird baresip beendet.
pub struct Softphone {
    thread: Option<JoinHandle<()>>,
}

impl Softphone {
    /// Startet baresip auf einem eigenen Thread und wartet, bis es bereit ist.
    pub fn start(
        config: &Config,
        software: &str,
    ) -> Result<(Self, mpsc::UnboundedReceiver<SipEvent>)> {
        if RUNNING.swap(true, Ordering::SeqCst) {
            return Err(Error::AlreadyRunning);
        }
        let config =
            CString::new(config.render()).map_err(|_| Error::InvalidValue("config".into()))?;
        let software =
            CString::new(software).map_err(|_| Error::InvalidValue("software".into()))?;
        let (tx, rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        let ctx = Box::new(Ctx {
            events: tx,
            ready: ready_tx,
        });

        let thread = std::thread::Builder::new()
            .name("baresip".into())
            .spawn(move || {
                let ctx = Box::into_raw(ctx);
                // SAFETY: Zeiger bleiben gültig, bis sfsip_run zurückkehrt;
                // danach ruft baresip den Callback nicht mehr auf.
                let err = unsafe {
                    ffi::sfsip_run(config.as_ptr(), software.as_ptr(), on_event, ctx.cast())
                };
                if err != 0 {
                    tracing::warn!(err, "baresip beendet mit Fehler");
                }
                // SAFETY: stammt aus Box::into_raw oben.
                drop(unsafe { Box::from_raw(ctx) });
                RUNNING.store(false, Ordering::SeqCst);
            })
            .map_err(|_| Error::StartFailed)?;

        match ready_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(true) => Ok((
                Self {
                    thread: Some(thread),
                },
                rx,
            )),
            _ => {
                let _ = thread.join();
                Err(Error::StartFailed)
            }
        }
    }

    fn cmd(&self, op: c_int, a: Option<&str>, b: Option<&str>) -> Result<()> {
        let a = a
            .map(CString::new)
            .transpose()
            .map_err(|_| Error::InvalidValue("arg".into()))?;
        let b = b
            .map(CString::new)
            .transpose()
            .map_err(|_| Error::InvalidValue("arg".into()))?;
        let ptr = |s: &Option<CString>| s.as_ref().map_or(std::ptr::null(), |s| s.as_ptr());
        // SAFETY: sfsip_cmd kopiert die Zeichenketten.
        match unsafe { ffi::sfsip_cmd(op, ptr(&a), ptr(&b)) } {
            0 => Ok(()),
            _ => Err(Error::NotRunning),
        }
    }

    /// Legt ein Konto an und registriert es (falls `register_interval` > 0).
    pub fn add_account(&self, account: &Account) -> Result<()> {
        self.add_aor(&account.to_aor()?)
    }

    /// Konto aus einer rohen baresip-Kontozeile (für Tests und Sonderfälle).
    pub fn add_aor(&self, aor: &str) -> Result<()> {
        self.cmd(ffi::OP_ADD_UA, Some(aor), None)
    }

    pub fn answer(&self, call_id: &str) -> Result<()> {
        self.cmd(ffi::OP_ANSWER, Some(call_id), None)
    }

    /// Legt den Anruf auf; ohne ID den ersten gefundenen.
    pub fn hangup(&self, call_id: Option<&str>) -> Result<()> {
        self.cmd(ffi::OP_HANGUP, call_id, None)
    }

    pub fn set_mute(&self, call_id: &str, muted: bool) -> Result<()> {
        self.cmd(
            if muted { ffi::OP_MUTE } else { ffi::OP_UNMUTE },
            Some(call_id),
            None,
        )
    }

    pub fn send_dtmf(&self, call_id: &str, digits: &str) -> Result<()> {
        if !digits
            .chars()
            .all(|c| c.is_ascii_digit() || "*#ABCD".contains(c))
        {
            return Err(Error::InvalidValue("dtmf".into()));
        }
        self.cmd(ffi::OP_DTMF, Some(call_id), Some(digits))
    }

    /// Wechselt die Audioquelle eines laufenden Anrufs, z. B. `aufile` mit
    /// einer WAV-Datei als Gerät. Die Datei muss zur Quellrate passen
    /// (siehe `ausrc_srate` in [`Config::extra`]).
    pub fn set_source(&self, call_id: &str, module: &str, device: &str) -> Result<()> {
        if module.contains(',') {
            return Err(Error::InvalidValue("module".into()));
        }
        self.cmd(
            ffi::OP_AUSRC,
            Some(call_id),
            Some(&format!("{module},{device}")),
        )
    }

    /// Baut nach Standby oder Netzwechsel die SIP-Verbindungen neu auf und
    /// registriert alle Konten neu.
    pub fn reset(&self) -> Result<()> {
        self.cmd(ffi::OP_RESET, None, None)
    }

    /// Direkter SIP-Anruf von einem Konto aus. Im Client normalerweise nicht
    /// nötig (Anrufe startet die Anlage), aber für Tests praktisch.
    pub fn connect(&self, from_aor: &str, uri: &str) -> Result<()> {
        self.cmd(ffi::OP_CONNECT, Some(from_aor), Some(uri))
    }
}

impl Drop for Softphone {
    fn drop(&mut self) {
        let _ = self.cmd(ffi::OP_QUIT, None, None);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aor_uses_tls_and_mandatory_srtp() {
        let acc = Account {
            user: "1004.WinClient".into(),
            password: "geheim".into(),
            host: "pbx.example.com".into(),
            port: 5061,
            register_interval: 3600,
            outbound: None,
        };
        let aor = acc.to_aor().unwrap();
        assert!(aor.starts_with("<sip:1004.WinClient@pbx.example.com:5061;transport=tls>"));
        assert!(aor.contains(";mediaenc=srtp-mand"));
        assert!(aor.contains(";answermode=manual"));
    }

    #[test]
    fn aor_rejects_parameter_injection() {
        let acc = Account {
            user: "u".into(),
            password: "x;answermode=auto".into(),
            host: "h".into(),
            port: 5061,
            register_interval: 0,
            outbound: None,
        };
        assert!(acc.to_aor().is_err());
    }

    #[test]
    fn aor_with_resolved_outbound_proxy() {
        let acc = Account {
            user: "1004.WinClient".into(),
            password: "geheim".into(),
            host: "pbx.example.com".into(),
            port: 5061,
            register_interval: 3600,
            outbound: Some("192.0.2.7:5061".parse().unwrap()),
        };
        let aor = acc.to_aor().unwrap();
        assert!(aor.starts_with("<sip:1004.WinClient@pbx.example.com:5061;transport=tls>"));
        assert!(aor.ends_with(";outbound=\"sip:192.0.2.7:5061;transport=tls\""));
        let v6 = Account {
            outbound: Some("[2001:db8::1]:5061".parse().unwrap()),
            ..acc
        };
        assert!(v6.to_aor().unwrap().contains("sip:[2001:db8::1]:5061;"));
    }
}
