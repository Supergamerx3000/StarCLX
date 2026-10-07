//! `sfctl`: Kommandozeile für die STARFACE, auf demselben Kern wie die
//! Desktop-App. Dient vorerst als Entwicklungs- und Testwerkzeug.

mod wav;

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use sf_onehub::sf_proto::v1;
use sf_onehub::{OneHub, TokenHandle};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Adresse der Anlage, z. B. https://pbx.example.com
    #[arg(long, env = "SF_SERVER")]
    server: String,
    /// Port der OneHub-gRPC-API
    #[arg(long, env = "SF_GRPC_PORT", default_value_t = sf_onehub::DEFAULT_PORT)]
    grpc_port: u16,
    /// Access-Token; ohne Token wird per Password-Grant angemeldet
    #[arg(long, env = "SF_TOKEN", hide_env_values = true)]
    token: Option<String>,
    #[arg(long, env = "SF_USER")]
    user: Option<String>,
    #[arg(long, env = "SF_PASSWORD", hide_env_values = true)]
    password: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Version der Anlage
    Version,
    /// Eigenes Profil
    Me,
    /// Eigene Telefone
    Phones,
    /// Anruf starten; die Anlage ruft zuerst das gewählte Telefon an
    Call {
        number: String,
        /// Telefon-ID (siehe `sfctl phones`), sonst das Primärtelefon
        #[arg(long)]
        phone_id: Option<String>,
    },
    /// Softphone anmelden und Anrufe über das App-Telefon führen
    Softphone {
        /// Diese Nummer anrufen; die Anlage klingelt zuerst am Softphone,
        /// das automatisch annimmt
        #[arg(long)]
        call: Option<String>,
        /// Auch fremde eingehende Anrufe sofort annehmen
        #[arg(long)]
        answer_all: bool,
        /// Nach so vielen Sekunden beenden (sonst mit Strg+C)
        #[arg(long)]
        seconds: Option<u64>,
        /// TLS-Zertifikat der Anlage für SIP nicht prüfen
        #[arg(long)]
        insecure_sip: bool,
        /// Geräte-ID für RegisterSipDevice; bestimmt, welches App-Telefon
        /// die Anlage verwendet
        #[arg(long, env = "SF_SIP_DEVICE_ID", default_value = sf_onehub::SIP_DEVICE_ID)]
        sip_device_id: String,
    },
    /// Anrufen und eine Ansage abspielen (Text oder WAV-Datei), optional mit
    /// Bestätigung per Taste.
    ///
    /// Rückgabewert: 0 angenommen (und bestätigt, falls verlangt),
    /// 2 nicht angenommen, 3 nicht bestätigt, 1 Fehler.
    Say {
        /// Rufnummer, die angerufen wird
        number: String,
        /// Ansagetext; `-` liest ihn von der Standardeingabe. Mit `--wav`
        /// nicht nötig.
        #[arg(required_unless_present = "wav")]
        text: Option<String>,
        /// Statt Sprachausgabe diese WAV-Datei (16-bit-PCM) abspielen
        #[arg(long, conflicts_with = "text")]
        wav: Option<PathBuf>,
        /// Bestätigung mit dieser Taste verlangen (0–9, * oder #)
        #[arg(long, value_parser = parse_key, value_name = "TASTE")]
        confirm: Option<char>,
        /// So oft abspielen; mit `--confirm` endet es früher, sobald die
        /// Taste gedrückt wird
        #[arg(long, default_value_t = 3)]
        repeat: u32,
        /// Pause zwischen den Wiederholungen in Sekunden
        #[arg(long, default_value_t = 1.0)]
        pause: f64,
        /// Nach der letzten Ansage so lange auf die Taste warten (Sekunden)
        #[arg(long, default_value_t = 10)]
        confirm_wait: u64,
        /// So lange klingeln lassen (Sekunden)
        #[arg(long, default_value_t = 45)]
        ring_timeout: u64,
        /// Befehl für die Sprachausgabe, läuft über `sh -c`. Er bekommt den
        /// Text in `$TEXT` und schreibt eine WAV-Datei nach `$WAV`.
        #[arg(long, env = "SF_TTS", default_value = DEFAULT_TTS)]
        tts: String,
        /// Bei `--confirm` keinen Hinweis auf die Taste an den Text anhängen
        #[arg(long)]
        no_hint: bool,
        /// TLS-Zertifikat der Anlage für SIP nicht prüfen
        #[arg(long)]
        insecure_sip: bool,
        #[arg(long, env = "SF_SIP_DEVICE_ID", default_value = sf_onehub::SIP_DEVICE_ID)]
        sip_device_id: String,
    },
    /// Chat-Nachricht senden oder Kontakte auflisten
    #[command(subcommand)]
    Chat(ChatCommand),
    /// Eigenen Status anzeigen; mit Optionen setzen
    Status {
        /// Nicht stören ein- oder ausschalten
        #[arg(long, value_parser = parse_on_off, value_name = "on|off")]
        dnd: Option<bool>,
        /// Statustext setzen; leerer Text löscht ihn
        #[arg(long)]
        text: Option<String>,
    },
}

#[derive(Subcommand)]
enum ChatCommand {
    /// Nachricht senden
    Send {
        /// Empfänger: Jabber-ID, Benutzername (Teil vor dem @) oder Name
        /// aus `sfctl chat contacts`
        to: String,
        /// Nachricht; `-` liest sie von der Standardeingabe
        text: String,
    },
    /// Chat-Kontakte mit Status auflisten
    Contacts,
}

const DEFAULT_TTS: &str = r#"espeak-ng -v de -s 150 -w "$WAV" "$TEXT""#;

fn parse_key(s: &str) -> Result<char, String> {
    match s.chars().collect::<Vec<_>>().as_slice() {
        [c] if c.is_ascii_digit() || *c == '*' || *c == '#' => Ok(*c),
        _ => Err("eine Taste 0–9, * oder # erwartet".into()),
    }
}

fn parse_on_off(s: &str) -> Result<bool, String> {
    match s {
        "on" | "an" | "ein" | "1" | "true" => Ok(true),
        "off" | "aus" | "0" | "false" => Ok(false),
        _ => Err("on oder off erwartet".into()),
    }
}

#[tokio::main]
async fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let host = url::Url::parse(&cli.server)
        .context("SF_SERVER muss eine URL sein, z. B. https://pbx.example.com")?
        .host_str()
        .context("SF_SERVER enthält keinen Hostnamen")?
        .to_owned();

    let token = match (&cli.token, &cli.user, &cli.password) {
        (Some(t), _, _) => t.clone(),
        (None, Some(u), Some(p)) => {
            let auth = sf_auth::Client::discover(&cli.server).await?;
            auth.password_grant(u, p).await?.access_token
        }
        _ => bail!("SF_TOKEN oder SF_USER und SF_PASSWORD setzen"),
    };

    let hub = OneHub::connect(&host, cli.grpc_port, TokenHandle::new(token)).await?;

    match cli.command {
        Command::Version => println!("{}", hub.server_version().await?),
        Command::Me => {
            let user = hub
                .me()
                .get_user(())
                .await?
                .into_inner()
                .user
                .unwrap_or_default();
            println!("{} {} <{}>", user.first_name, user.last_name, user.email);
            for n in user.phone_numbers {
                println!("  {n:?}");
            }
        }
        Command::Phones => {
            let primary = hub.me().get_primary_phone(()).await?.into_inner().phone;
            let primary_id = primary.and_then(|p| p.phone_id).map(|id| id.id);
            for phone in hub.me().get_phones(()).await?.into_inner().phones {
                let id = phone.phone_id.map(|id| id.id).unwrap_or_default();
                let mark = if Some(&id) == primary_id.as_ref() {
                    "*"
                } else {
                    " "
                };
                println!("{mark} {id:>6}  {}", phone.name);
            }
        }
        Command::Call { number, phone_id } => {
            let req = v1::call::PlaceCallRequest {
                number,
                requested_call_id: None,
                phone_id: phone_id.map(|id| v1::types::PhoneId { id }),
            };
            let call = hub.call().place_call(req).await?.into_inner();
            println!("{}", call.call_id.map(|c| c.id).unwrap_or_default());
        }
        Command::Softphone {
            call,
            answer_all,
            seconds,
            insecure_sip,
            sip_device_id,
        } => {
            let opts = SoftphoneOpts {
                number: call,
                answer_all,
                seconds,
                insecure_sip,
                sip_device_id,
            };
            softphone(&hub, &host, opts).await?
        }
        Command::Chat(cmd) => chat(&hub, &host, cmd).await?,
        Command::Status { dnd, text } => status(&hub, dnd, text).await?,
        Command::Say {
            number,
            text,
            wav,
            confirm,
            repeat,
            pause,
            confirm_wait,
            ring_timeout,
            tts,
            no_hint,
            insecure_sip,
            sip_device_id,
        } => {
            let opts = SayOpts {
                number,
                confirm,
                repeat: repeat.max(1),
                pause: Duration::from_secs_f64(pause.max(0.0)),
                confirm_wait: Duration::from_secs(confirm_wait),
                ring_timeout: Duration::from_secs(ring_timeout),
                insecure_sip,
                sip_device_id,
            };
            let audio = announcement(text, wav, &tts, confirm.filter(|_| !no_hint))?;
            let outcome = say(&hub, &host, &audio.path, opts).await?;
            eprintln!("{}", outcome.describe());
            return Ok(ExitCode::from(outcome.code()));
        }
    }
    Ok(ExitCode::SUCCESS)
}

async fn own_user_id(hub: &OneHub) -> Result<String> {
    let user = hub.me().get_user(()).await?.into_inner().user;
    user.and_then(|u| u.user_id)
        .map(|id| id.id)
        .context("Anlage liefert keine eigene Benutzer-ID")
}

/// So lange wird nach der ersten Kontaktliste noch auf Nachzügler gewartet;
/// Präsenzen anderer können die Liste vor der vollständigen Antwort melden.
const ROSTER_SETTLE: Duration = Duration::from_millis(800);

async fn chat(hub: &OneHub, host: &str, cmd: ChatCommand) -> Result<()> {
    let jid = hub.chat_jid(&own_user_id(hub).await?).await?;
    if jid.is_empty() {
        bail!("kein Chat-Konto auf der Anlage");
    }
    let token = hub.token().clone();
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let chat = sf_chat::Chat::start(&jid, host, move || token.get(), None, tx)?;

    // Warten, bis die Verbindung steht und die Kontaktliste da ist.
    let mut contacts: Option<Vec<sf_chat::Contact>> = None;
    let mut last_error = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let wait = match contacts {
            Some(_) => ROSTER_SETTLE,
            None => deadline.saturating_duration_since(tokio::time::Instant::now()),
        };
        match tokio::time::timeout(wait, events.recv()).await {
            Ok(Some(sf_chat::ChatEvent::Roster { contacts: c })) => contacts = Some(c),
            Ok(Some(sf_chat::ChatEvent::State {
                online: false,
                detail,
            })) => last_error = detail,
            Ok(Some(_)) => {}
            Ok(None) => bail!("Chat beendet"),
            Err(_) if contacts.is_some() => break,
            Err(_) if last_error.is_empty() => bail!("keine Chat-Verbindung innerhalb von 20 s"),
            Err(_) => bail!("keine Chat-Verbindung: {last_error}"),
        }
    }
    let contacts = contacts.unwrap_or_default();

    match cmd {
        ChatCommand::Contacts => {
            for c in &contacts {
                let status = if c.status.is_empty() {
                    String::new()
                } else {
                    format!("  „{}“", c.status)
                };
                println!("{:<8} {:<30} {}{status}", c.show, c.name, c.jid);
            }
        }
        ChatCommand::Send { to, text } => {
            let body = if text == "-" {
                let mut s = String::new();
                std::io::stdin()
                    .read_to_string(&mut s)
                    .context("Standardeingabe lesen")?;
                s.trim_end().to_owned()
            } else {
                text
            };
            if body.trim().is_empty() {
                bail!("leere Nachricht");
            }
            let peer = find_contact(&contacts, &to)?;
            chat.send(&peer, &body);
            println!("gesendet an {peer}");
        }
    }
    chat.shutdown("").await;
    Ok(())
}

/// Sucht den Empfänger in der Kontaktliste: zuerst die genaue Jabber-ID,
/// dann den Benutzernamen vor dem @, dann den Namen (genau, sonst als
/// eindeutiger Teil). Eine nicht gelistete Jabber-ID wird so übernommen.
fn find_contact(contacts: &[sf_chat::Contact], to: &str) -> Result<String> {
    let want = to.trim().to_lowercase();
    let node = |c: &sf_chat::Contact| c.jid.split('@').next().unwrap_or_default().to_lowercase();
    let tiers: [&dyn Fn(&sf_chat::Contact) -> bool; 4] = [
        &|c| c.jid.to_lowercase() == want,
        &|c| node(c) == want,
        &|c| c.name.to_lowercase() == want,
        &|c| c.name.to_lowercase().contains(&want),
    ];
    for matches in tiers {
        let found: Vec<_> = contacts.iter().filter(|c| matches(c)).collect();
        match found.as_slice() {
            [] => continue,
            [c] => return Ok(c.jid.clone()),
            many => {
                let names: Vec<_> = many
                    .iter()
                    .map(|c| format!("{} <{}>", c.name, c.jid))
                    .collect();
                bail!("„{to}“ ist nicht eindeutig: {}", names.join(", "));
            }
        }
    }
    if want.contains('@') {
        return Ok(to.trim().to_owned());
    }
    bail!("Kontakt „{to}“ nicht gefunden (siehe `sfctl chat contacts`)")
}

async fn status(hub: &OneHub, dnd: Option<bool>, text: Option<String>) -> Result<()> {
    if let Some(enabled) = dnd {
        hub.me()
            .set_do_not_disturb(v1::me::SetDoNotDisturbRequest { enabled })
            .await
            .context("Nicht stören setzen")?;
    }
    if let Some(message) = text {
        hub.me()
            .set_chat_presence_message(v1::me::SetChatPresenceMessageRequest { message })
            .await
            .context("Statustext setzen")?;
    }
    print_status(hub).await
}

async fn print_status(hub: &OneHub) -> Result<()> {
    use v1::presence::presence_state::PresenceState as S;
    let user_id = own_user_id(hub).await?;
    let ids = || v1::presence::UserIdList {
        user_ids: vec![v1::types::UserId {
            id: user_id.clone(),
        }],
    };
    let states = hub
        .presence()
        .subscribe_presence_states(v1::presence::SubscribePresenceStatesRequest {
            presence_ids: Some(
                v1::presence::subscribe_presence_states_request::PresenceIds::UserIdList(ids()),
            ),
            return_presence_states: true,
        })
        .await?
        .into_inner()
        .presence_states;
    let _ = hub
        .presence()
        .unsubscribe_presence_states(v1::presence::UnsubscribePresenceStatesRequest {
            presence_ids: Some(
                v1::presence::unsubscribe_presence_states_request::PresenceIds::UserIdList(ids()),
            ),
        })
        .await;
    let Some(u) = states.into_iter().find_map(|s| match s.presence_state {
        Some(S::UserPresenceState(u)) => Some(u),
        _ => None,
    }) else {
        bail!("Anlage liefert keinen Status");
    };
    let chat = v1::presence::ChatState::try_from(u.chat_state)
        .map(|s| {
            s.as_str_name()
                .trim_start_matches("CHAT_STATE_")
                .to_lowercase()
        })
        .unwrap_or_default();
    let phone = v1::presence::TelephonyState::try_from(u.telephony_state)
        .map(|s| {
            s.as_str_name()
                .trim_start_matches("TELEPHONY_STATE_")
                .to_lowercase()
        })
        .unwrap_or_default();
    let on_off = |b: bool| if b { "an" } else { "aus" };
    println!("Chat:        {chat}");
    println!("Statustext:  {}", u.chat_state_message);
    println!("Nicht stören: {}", on_off(u.dnd_enabled));
    println!("Umleitung:   {}", on_off(u.redirect_enabled));
    println!("Telefon:     {phone}");
    Ok(())
}

/// Temporäre Datei, die beim Drop gelöscht wird
struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(name: &str) -> Self {
        Self {
            path: std::env::temp_dir().join(format!("sfctl-{}-{name}", std::process::id())),
        }
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn key_name(key: char) -> String {
    match key {
        '*' => "Stern".into(),
        '#' => "Raute".into(),
        c => c.to_string(),
    }
}

/// Bereitet die Ansage als WAV mit 16 kHz mono vor: aus einer Datei oder
/// per Sprachausgabe aus dem Text.
fn announcement(
    text: Option<String>,
    wav: Option<PathBuf>,
    tts: &str,
    hint: Option<char>,
) -> Result<TempFile> {
    let source = match (wav, text) {
        (Some(path), _) => {
            std::fs::read(&path).with_context(|| format!("{} lesen", path.display()))?
        }
        (None, Some(text)) => {
            let mut text = if text == "-" {
                let mut s = String::new();
                std::io::stdin()
                    .read_to_string(&mut s)
                    .context("Standardeingabe lesen")?;
                s
            } else {
                text
            };
            text = text.trim().to_owned();
            if text.is_empty() {
                bail!("leerer Ansagetext");
            }
            if let Some(key) = hint {
                text = format!(
                    "{text}. Bitte bestätigen Sie mit der Taste {}.",
                    key_name(key)
                );
            }
            let raw = TempFile::new("tts.wav");
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg(tts)
                .env("TEXT", &text)
                .env("WAV", &raw.path)
                .stdin(std::process::Stdio::null())
                .status()
                .with_context(|| format!("Sprachausgabe starten: {tts}"))?;
            if !status.success() {
                bail!("Sprachausgabe fehlgeschlagen ({status}): {tts}");
            }
            std::fs::read(&raw.path).context("Sprachausgabe hat keine WAV-Datei geschrieben")?
        }
        (None, None) => bail!("Text oder --wav angeben"),
    };
    let out = TempFile::new("ansage.wav");
    std::fs::write(&out.path, wav::to_16k_mono(&source)?).context("Ansage schreiben")?;
    Ok(out)
}

struct SayOpts {
    number: String,
    confirm: Option<char>,
    repeat: u32,
    pause: Duration,
    confirm_wait: Duration,
    ring_timeout: Duration,
    insecure_sip: bool,
    sip_device_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SayOutcome {
    /// Angenommen, Ansage gespielt (ohne Bestätigung verlangt)
    Played,
    Confirmed(char),
    NotAnswered,
    NotConfirmed,
}

impl SayOutcome {
    fn code(self) -> u8 {
        match self {
            Self::Played | Self::Confirmed(_) => 0,
            Self::NotAnswered => 2,
            Self::NotConfirmed => 3,
        }
    }

    fn describe(self) -> String {
        match self {
            Self::Played => "angenommen, Ansage abgespielt".into(),
            Self::Confirmed(k) => format!("bestätigt mit Taste {k}"),
            Self::NotAnswered => "nicht angenommen".into(),
            Self::NotConfirmed => "angenommen, aber nicht bestätigt".into(),
        }
    }
}

/// Ruhige Quelle, solange keine Ansage läuft; stumm geschaltet sendet sie
/// Stille, hält aber den Medienstrom am Laufen.
const IDLE_SOURCE: (&str, &str) = ("ausine", "440");

/// Ruft über die Anlage an: Die Anlage klingelt zuerst am Softphone, das
/// stumm annimmt; sobald das Gegenüber abnimmt, läuft die Ansage.
async fn say(hub: &OneHub, host: &str, wav: &std::path::Path, opts: SayOpts) -> Result<SayOutcome> {
    use v1::call::call_event_response::CallEvent as E;
    use v1::types::CallState;

    let wav = wav.to_str().context("Pfad der Ansage ist kein UTF-8")?;
    let config = sf_sip::Config {
        verify_server: !opts.insecure_sip,
        audio_source: format!("{},{}", IDLE_SOURCE.0, IDLE_SOURCE.1),
        // Nichts lokal abspielen; auch ohne Audiogerät (Server, Cron) lauffähig.
        audio_player: "aufile,/dev/null".into(),
        // Feste Quellrate, damit die Ansage-WAV ohne Umrechnung passt.
        extra: format!("ausrc_srate {}\nausrc_channels 1\n", wav::RATE),
        ..Default::default()
    };
    let sp = start_softphone(hub, host, &opts.sip_device_id, &config, false).await?;
    let (phone, mut sip) = (sp.phone, sp.events);
    let Some(phone_id) = sp.phone_id else {
        bail!(
            "App-Telefon SIP/{} nicht in `sfctl phones` gefunden; kein Anruf gestartet",
            sp.sip_user
        );
    };
    let mut events = call_events(hub);

    let req = v1::call::PlaceCallRequest {
        number: opts.number.clone(),
        requested_call_id: None,
        phone_id: Some(v1::types::PhoneId { id: phone_id }),
    };
    let placed = hub
        .call()
        .place_call(req)
        .await?
        .into_inner()
        .call_id
        .map(|c| c.id)
        .context("PlaceCall ohne Anruf-ID")?;
    let placed_at = Instant::now();
    eprintln!("wähle {} …", opts.number);

    let ring_deadline = Instant::now() + opts.ring_timeout;
    let mut sip_call: Option<String> = None;
    let mut connected = false;
    let mut plays = 0u32;
    let mut playing = false;
    // Nächster Zeitpunkt, zu dem etwas zu tun ist: Ansage starten oder
    // (nach der letzten) das Warten auf die Taste beenden.
    let mut next_step: Option<Instant> = None;

    let outcome = loop {
        let waiting_for_answer = (!connected).then_some(ring_deadline);
        tokio::select! {
            _ = tokio::signal::ctrl_c() => bail!("abgebrochen"),
            _ = sleep_until(waiting_for_answer) => break SayOutcome::NotAnswered,
            _ = sleep_until(next_step) => {
                next_step = None;
                let Some(call) = &sip_call else { continue };
                if plays >= opts.repeat {
                    // Wartezeit für die Taste ist um.
                    break SayOutcome::NotConfirmed;
                }
                phone.set_source(call, "aufile", wav)?;
                phone.set_mute(call, false)?;
                playing = true;
                plays += 1;
                eprintln!("Ansage {plays}/{}", opts.repeat);
            }
            Some(ev) = events.recv() => {
                let answered = match ev {
                    Some(E::CallStateChanged(e)) => {
                        e.call_id.is_some_and(|c| c.id == placed)
                            && e.call_state == CallState::Connected as i32
                    }
                    Some(E::CallCreated(e)) => e.calls.iter().any(|c| {
                        c.call_id.as_ref().is_some_and(|c| c.id == placed)
                            && c.call_state == CallState::Connected as i32
                    }),
                    Some(E::CallDisconnected(e)) if e.call_id.as_ref().is_some_and(|c| c.id == placed) => {
                        break if connected { finished(opts.confirm) } else { SayOutcome::NotAnswered };
                    }
                    _ => false,
                };
                if answered && !connected {
                    connected = true;
                    eprintln!("angenommen");
                    // Kurz warten, damit der Anfang nicht verschluckt wird.
                    next_step = Some(Instant::now() + Duration::from_millis(700));
                }
            }
            ev = sip.recv() => {
                let Some(ev) = ev else { bail!("Softphone beendet") };
                match ev {
                    sf_sip::SipEvent::Incoming { call, auto_answer } => {
                        let ours = placed_at.elapsed() < PLACE_CALL_WINDOW;
                        if sip_call.is_none() && (auto_answer || ours) {
                            phone.answer(&call.call_id)?;
                            phone.set_mute(&call.call_id, true)?;
                            sip_call = Some(call.call_id);
                        }
                    }
                    sf_sip::SipEvent::Closed { call_id, .. } if sip_call.as_ref() == Some(&call_id) => {
                        break if connected { finished(opts.confirm) } else { SayOutcome::NotAnswered };
                    }
                    sf_sip::SipEvent::EndOfFile { call_id } if playing && sip_call.as_ref() == Some(&call_id) => {
                        playing = false;
                        phone.set_mute(&call_id, true)?;
                        phone.set_source(&call_id, IDLE_SOURCE.0, IDLE_SOURCE.1)?;
                        if plays < opts.repeat {
                            next_step = Some(Instant::now() + opts.pause);
                        } else if opts.confirm.is_none() {
                            break SayOutcome::Played;
                        } else {
                            next_step = Some(Instant::now() + opts.confirm_wait);
                        }
                    }
                    sf_sip::SipEvent::Dtmf { call_id, key }
                        if connected && sip_call.as_ref() == Some(&call_id) && opts.confirm == Some(key) =>
                    {
                        break SayOutcome::Confirmed(key);
                    }
                    sf_sip::SipEvent::Dtmf { key, .. } => eprintln!("Taste {key}"),
                    _ => {}
                }
            }
        }
    };

    if let Some(call) = &sip_call {
        let _ = phone.hangup(Some(call));
        // Dem Auflegen etwas Zeit geben, bevor baresip beendet wird.
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            while let Some(ev) = sip.recv().await {
                if matches!(ev, sf_sip::SipEvent::Closed { .. }) {
                    break;
                }
            }
        })
        .await;
    } else {
        // Klingelt noch am Softphone oder beim Gegenüber: Anruf zurückziehen.
        let _ = hub
            .call()
            .hangup_call(v1::call::HangupCallRequest {
                call_id: Some(v1::types::CallId { id: placed }),
            })
            .await;
    }
    drop(phone);
    Ok(outcome)
}

/// Ergebnis, wenn das Gegenüber auflegt, bevor alles gespielt ist
fn finished(confirm: Option<char>) -> SayOutcome {
    match confirm {
        Some(_) => SayOutcome::NotConfirmed,
        None => SayOutcome::Played,
    }
}

/// Zeitfenster nach `PlaceCall`, in dem ein eingehender Anruf als Rückruf
/// der Anlage gilt und automatisch angenommen wird.
const PLACE_CALL_WINDOW: Duration = Duration::from_secs(20);

struct SoftphoneOpts {
    number: Option<String>,
    answer_all: bool,
    seconds: Option<u64>,
    insecure_sip: bool,
    sip_device_id: String,
}

struct StartedSoftphone {
    phone: sf_sip::Softphone,
    events: tokio::sync::mpsc::UnboundedReceiver<sf_sip::SipEvent>,
    /// Telefon-ID des App-Telefons für `PlaceCall`
    phone_id: Option<String>,
    sip_user: String,
}

/// Meldet das App-Telefon an der Anlage an und wartet auf die Registrierung.
async fn start_softphone(
    hub: &OneHub,
    host: &str,
    sip_device_id: &str,
    config: &sf_sip::Config,
    verbose: bool,
) -> Result<StartedSoftphone> {
    let creds = hub
        .register_sip_device(sip_device_id, env!("CARGO_PKG_VERSION"))
        .await
        .context("RegisterSipDevice")?;
    let phone_id = hub.phone_id_for_sip_user(&creds.user).await?;
    if verbose {
        println!(
            "SIP-Benutzer {} (Realm {}, Port {}), Telefon-ID {}",
            creds.user,
            creds.realm,
            creds.port,
            phone_id.as_deref().unwrap_or("unbekannt")
        );
    }

    let software = concat!("starclx/", env!("CARGO_PKG_VERSION"));
    let (phone, mut sip) = sf_sip::Softphone::start(config, software)?;
    phone.add_account(&sf_sip::Account {
        user: creds.user.clone(),
        password: creds.password.clone(),
        host: host.to_owned(),
        port: creds.port,
        register_interval: 3600,
        outbound: None,
    })?;

    let registered = tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(ev) = sip.recv().await {
            if verbose {
                println!("SIP: {ev:?}");
            }
            match ev {
                sf_sip::SipEvent::Registered { .. } => return Ok(()),
                sf_sip::SipEvent::RegisterFailed { reason, .. } => bail!("Registrierung: {reason}"),
                _ => {}
            }
        }
        bail!("Softphone beendet")
    })
    .await;
    registered.context("keine SIP-Registrierung innerhalb von 15 s")??;
    Ok(StartedSoftphone {
        phone,
        events: sip,
        phone_id,
        sip_user: creds.user,
    })
}

type CallEvent = Option<v1::call::call_event_response::CallEvent>;

/// Anruf-Ereignisse der Anlage. Der Ereignisstrom liefert seine
/// Antwort-Header erst mit dem ersten Ereignis; deshalb nebenher lesen und
/// nicht darauf warten.
fn call_events(hub: &OneHub) -> tokio::sync::mpsc::UnboundedReceiver<CallEvent> {
    let (events_tx, events) = tokio::sync::mpsc::unbounded_channel();
    let events_hub = hub.clone();
    tokio::spawn(async move {
        let result = async {
            let mut stream = events_hub
                .call()
                .subscribe_call_events(())
                .await?
                .into_inner();
            while let Some(ev) = stream.message().await? {
                if events_tx.send(ev.call_event).is_err() {
                    break;
                }
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(e) = result {
            eprintln!("Anruf-Ereignisse der Anlage: {e}");
        }
    });
    events
}

async fn softphone(hub: &OneHub, host: &str, opts: SoftphoneOpts) -> Result<()> {
    let SoftphoneOpts {
        number,
        answer_all,
        seconds,
        insecure_sip,
        sip_device_id,
    } = opts;
    let deadline = seconds.map(|s| Instant::now() + Duration::from_secs(s));

    let config = sf_sip::Config {
        verify_server: !insecure_sip,
        ..Default::default()
    };
    let sp = start_softphone(hub, host, &sip_device_id, &config, true).await?;
    let (phone, mut sip, phone_id) = (sp.phone, sp.events, sp.phone_id);
    // Ohne Telefon-ID würde PlaceCall das Primärtelefon nehmen, also nicht
    // dieses Softphone.
    if number.is_some() && phone_id.is_none() {
        bail!(
            "App-Telefon SIP/{} nicht in `sfctl phones` gefunden; kein Anruf gestartet",
            sp.sip_user
        );
    }
    let mut events = call_events(hub);

    let mut placed_at = None;
    if let Some(number) = number {
        let req = v1::call::PlaceCallRequest {
            number,
            requested_call_id: None,
            phone_id: phone_id.map(|id| v1::types::PhoneId { id }),
        };
        let id = hub.call().place_call(req).await?.into_inner().call_id;
        println!("PlaceCall: {:?}", id.map(|c| c.id));
        placed_at = Some(Instant::now());
    }

    let mut active: Option<String> = None;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = sleep_until(deadline) => break,
            Some(ev) = events.recv() => println!("Anlage: {ev:?}"),
            ev = sip.recv() => {
                let Some(ev) = ev else { bail!("Softphone beendet") };
                println!("SIP: {ev:?}");
                match ev {
                    sf_sip::SipEvent::Incoming { call, auto_answer } => {
                        let ours = placed_at.is_some_and(|t| t.elapsed() < PLACE_CALL_WINDOW);
                        if auto_answer || ours || answer_all {
                            println!("nehme an: {}", call.peer_uri);
                            phone.answer(&call.call_id)?;
                            active = Some(call.call_id);
                            placed_at = None;
                        } else {
                            println!("klingelt: {} (mit --answer-all annehmen)", call.peer_uri);
                        }
                    }
                    sf_sip::SipEvent::Closed { call_id, .. }
                        if active.as_ref() == Some(&call_id) && !answer_all =>
                    {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    if let Some(id) = active {
        let _ = phone.hangup(Some(&id));
    }
    drop(phone);
    Ok(())
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d.into()).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(jid: &str, name: &str) -> sf_chat::Contact {
        sf_chat::Contact {
            jid: jid.into(),
            name: name.into(),
            show: "online".into(),
            status: String::new(),
        }
    }

    #[test]
    fn finds_contact() {
        let list = [
            contact("anna@pbx", "Anna Muster"),
            contact("annabelle@pbx", "Annabelle Beispiel"),
            contact("bob@pbx", "Bob Test"),
        ];
        assert_eq!(find_contact(&list, "bob@pbx").unwrap(), "bob@pbx");
        assert_eq!(find_contact(&list, "Anna").unwrap(), "anna@pbx");
        assert_eq!(find_contact(&list, "anna muster").unwrap(), "anna@pbx");
        assert_eq!(find_contact(&list, "test").unwrap(), "bob@pbx");
        assert!(find_contact(&list, "beis").is_ok());
        assert!(find_contact(&list, "e").is_err());
        assert_eq!(find_contact(&list, "neu@pbx").unwrap(), "neu@pbx");
        assert!(find_contact(&list, "niemand").is_err());
    }
}
