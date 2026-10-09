//! Headset-Tasten über USB-HID-Telefonie (`/dev/hidraw*`): Jabra, Poly und
//! EPOS halten sich an die Telephony Usage Page des HID-Standards.
//!
//! Das Headset meldet nur Tastendrücke (Hörer ab/auf, Stumm); was passiert,
//! entscheidet die App. In die andere Richtung schaltet die App Klingeln,
//! Gesprächs- und Stumm-LED. Wo im Report welches Feld liegt, steht im
//! Report-Descriptor des Geräts, deshalb braucht es keine Herstellerlisten.
//!
//! Zugriff auf hidraw braucht eine udev-Regel (liegt dem .deb bei).

pub mod descriptor;

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

use descriptor::{Telephony, Usage, Values, get_bits, set_bits, usage};

const HOOK_SWITCH: Usage = usage(0x0B, 0x20);
const PHONE_MUTE: Usage = usage(0x0B, 0x2F);
const RINGER: Usage = usage(0x0B, 0x9E);
const LED_MUTE: Usage = usage(0x08, 0x09);
const LED_OFF_HOOK: Usage = usage(0x08, 0x17);
const LED_RING: Usage = usage(0x08, 0x18);
const LED_HOLD: Usage = usage(0x08, 0x20);

const RESCAN_EVERY: Duration = Duration::from_secs(3);

/// Tastendruck am Headset
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// Hörer abgenommen (Gespräch annehmen)
    HookOff,
    /// Hörer aufgelegt (Gespräch beenden)
    HookOn,
    /// Gesprächstaste bei Geräten, die keinen Zustand melden
    HookToggle,
    /// Stummtaste
    Mute,
}

/// Was das Headset anzeigen soll
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct State {
    pub ringing: bool,
    /// Im Gespräch (auch beim Wählen oder Halten)
    pub off_hook: bool,
    pub muted: bool,
    pub held: bool,
}

/// Gefundene Headsets bzw. der Grund, warum keines nutzbar ist
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub devices: Vec<String>,
    pub error: Option<String>,
}

struct Found {
    dev: PathBuf,
    name: String,
    tel: Telephony,
}

/// Sucht Geräte mit Telefonietasten unter `/sys/class/hidraw`.
fn find() -> Vec<Found> {
    find_in(Path::new("/sys/class/hidraw"))
}

fn find_in(sys: &Path) -> Vec<Found> {
    let Ok(dir) = std::fs::read_dir(sys) else {
        return Vec::new();
    };
    let mut out: Vec<Found> = dir
        .flatten()
        .filter_map(|e| {
            let desc = std::fs::read(e.path().join("device/report_descriptor")).ok()?;
            let tel = descriptor::parse(&desc);
            if !tel.has_input(HOOK_SWITCH) && !tel.has_input(PHONE_MUTE) {
                return None;
            }
            let uevent =
                std::fs::read_to_string(e.path().join("device/uevent")).unwrap_or_default();
            let name = uevent
                .lines()
                .find_map(|l| l.strip_prefix("HID_NAME="))
                .unwrap_or("Headset")
                .to_owned();
            Some(Found {
                dev: Path::new("/dev").join(e.file_name()),
                name,
                tel,
            })
        })
        .collect();
    out.sort_by(|a, b| a.dev.cmp(&b.dev));
    out
}

/// Prüft angesteckte Headsets auf Zugriffsrechte, ohne etwas zu senden.
pub fn probe() -> Status {
    let found = find();
    let error = found
        .iter()
        .find_map(|f| OpenOptions::new().read(true).write(true).open(&f.dev).err())
        .map(|e| describe(&e));
    Status {
        devices: found.into_iter().map(|f| f.name).collect(),
        error,
    }
}

fn describe(e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        "Keine Berechtigung für das Gerät (udev-Regel fehlt)".into()
    } else {
        e.to_string()
    }
}

/// Merkt sich je Eingabefeld den letzten Wert und macht aus Änderungen
/// Tastendrücke.
struct Decoder {
    tel: Telephony,
    last: Vec<u32>,
}

impl Decoder {
    fn new(tel: Telephony) -> Self {
        let last = vec![0; tel.inputs.len()];
        Self { tel, last }
    }

    fn feed(&mut self, report: &[u8]) -> Vec<Button> {
        let (id, data) = if self.tel.numbered {
            match report.split_first() {
                Some((id, data)) => (*id, data),
                None => return Vec::new(),
            }
        } else {
            (0, report)
        };
        let mut out = Vec::new();
        for (field, last) in self.tel.inputs.iter().zip(self.last.iter_mut()) {
            if field.report_id != id {
                continue;
            }
            let value = get_bits(data, field.offset, field.size);
            let before = std::mem::replace(last, value);
            match &field.values {
                Values::Variable(u) => {
                    let pressed = value != 0 && before == 0;
                    let released = value == 0 && before != 0;
                    match *u {
                        HOOK_SWITCH if field.relative && pressed => out.push(Button::HookToggle),
                        HOOK_SWITCH if !field.relative && pressed => out.push(Button::HookOff),
                        HOOK_SWITCH if !field.relative && released => out.push(Button::HookOn),
                        PHONE_MUTE if pressed => out.push(Button::Mute),
                        _ => {}
                    }
                }
                Values::Array { usages, min } => {
                    // Arrays melden gedrückte Tasten, keinen Zustand
                    let index = |v: u32| usize::try_from(v as i32 - min).ok();
                    let now = index(value).and_then(|i| usages.get(i));
                    let then = index(before).and_then(|i| usages.get(i));
                    if now != then {
                        match now {
                            Some(&HOOK_SWITCH) => out.push(Button::HookToggle),
                            Some(&PHONE_MUTE) => out.push(Button::Mute),
                            _ => {}
                        }
                    }
                }
            }
        }
        out
    }
}

/// Output-Reports für einen Zustand, je mit Report-Nummer vorne (0 ohne
/// Nummern, wie hidraw es erwartet). Nur Reports mit bekannten Feldern.
fn output_reports(tel: &Telephony, state: State) -> Vec<Vec<u8>> {
    let value = |u: Usage| match u {
        LED_OFF_HOOK => Some(state.off_hook),
        LED_RING | RINGER => Some(state.ringing),
        LED_MUTE => Some(state.muted && state.off_hook),
        LED_HOLD => Some(state.held),
        _ => None,
    };
    let mut out = Vec::new();
    for &(id, bits) in &tel.output_bits {
        let mut buf = vec![0u8; 1 + bits.div_ceil(8)];
        buf[0] = id;
        let mut known = false;
        for f in tel.outputs.iter().filter(|f| f.report_id == id) {
            if let Values::Variable(u) = f.values
                && let Some(on) = value(u)
            {
                known = true;
                set_bits(&mut buf[1..], f.offset, f.size, u32::from(on));
            }
        }
        if known {
            out.push(buf);
        }
    }
    out
}

/// Hält die angesteckten Headsets auf dem Stand der Anrufe und meldet
/// Tastendrücke an `on_button`.
pub struct Headset {
    tx: mpsc::Sender<Msg>,
    thread: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

enum Msg {
    Set(State),
    Status(mpsc::Sender<Status>),
    Gone(PathBuf),
}

impl Headset {
    pub fn start(on_button: impl Fn(Button) + Send + Sync + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker = Worker {
            state: State::default(),
            devices: Vec::new(),
            error: None,
            tx: tx.clone(),
            stop: stop.clone(),
            on_button: Arc::new(on_button),
        };
        let thread = std::thread::Builder::new()
            .name("headset".into())
            .spawn(move || worker.run(&rx))
            .ok();
        Self { tx, thread, stop }
    }

    /// Zustand übernehmen; wird auch bei unverändertem Zustand neu
    /// geschrieben, damit ein Headset nach einem Tastendruck ohne Wirkung
    /// wieder stimmt.
    pub fn set(&self, state: State) {
        let _ = self.tx.send(Msg::Set(state));
    }

    pub fn status(&self) -> Status {
        let (tx, rx) = mpsc::channel();
        if self.tx.send(Msg::Status(tx)).is_err() {
            return Status::default();
        }
        rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default()
    }
}

impl Drop for Headset {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.tx.send(Msg::Set(State::default()));
        let (dead, _) = mpsc::channel();
        drop(std::mem::replace(&mut self.tx, dead));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Open {
    dev: PathBuf,
    name: String,
    tel: Telephony,
    file: File,
}

struct Worker {
    state: State,
    devices: Vec<Open>,
    error: Option<String>,
    /// Für die Lese-Threads, die ein abgezogenes Gerät melden
    tx: mpsc::Sender<Msg>,
    stop: Arc<AtomicBool>,
    on_button: Arc<dyn Fn(Button) + Send + Sync>,
}

impl Worker {
    fn run(mut self, rx: &mpsc::Receiver<Msg>) {
        self.scan();
        loop {
            match rx.recv_timeout(RESCAN_EVERY) {
                Ok(Msg::Set(state)) => {
                    self.state = state;
                    for i in 0..self.devices.len() {
                        self.write(i);
                    }
                }
                Ok(Msg::Status(reply)) => {
                    self.scan();
                    let _ = reply.send(Status {
                        devices: self.devices.iter().map(|d| d.name.clone()).collect(),
                        error: self.error.clone(),
                    });
                }
                Ok(Msg::Gone(dev)) => {
                    if let Some(d) = self.devices.iter().find(|d| d.dev == dev) {
                        tracing::info!(name = %d.name, "Headset getrennt");
                    }
                    self.devices.retain(|d| d.dev != dev);
                }
                Err(RecvTimeoutError::Timeout) => self.scan(),
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if self.stop.load(Ordering::Relaxed) {
                return;
            }
        }
    }

    /// Neu angesteckte Headsets öffnen, Zustand schreiben und lesen.
    fn scan(&mut self) {
        self.error = None;
        for found in find() {
            if self.devices.iter().any(|d| d.dev == found.dev) {
                continue;
            }
            let opened = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&found.dev)
                .and_then(|f| Ok((f.try_clone()?, f)));
            let (reader, file) = match opened {
                Ok(f) => f,
                Err(e) => {
                    tracing::debug!(error = %e, dev = %found.dev.display(), "Headset nicht geöffnet");
                    self.error = Some(describe(&e));
                    continue;
                }
            };
            tracing::info!(name = %found.name, dev = %found.dev.display(), "Headset gefunden");
            self.spawn_reader(&found, reader);
            self.devices.push(Open {
                dev: found.dev,
                name: found.name,
                tel: found.tel,
                file,
            });
            self.write(self.devices.len() - 1);
        }
    }

    fn spawn_reader(&self, found: &Found, mut file: File) {
        let mut decoder = Decoder::new(found.tel.clone());
        let dev = found.dev.clone();
        let tx = self.tx.clone();
        let stop = self.stop.clone();
        let on_button = self.on_button.clone();
        let spawned = std::thread::Builder::new()
            .name("headset-read".into())
            .spawn(move || {
                let mut buf = [0u8; 256];
                loop {
                    let n = match file.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    for button in decoder.feed(&buf[..n]) {
                        tracing::debug!(?button, "Headset-Taste");
                        on_button(button);
                    }
                }
                let _ = tx.send(Msg::Gone(dev));
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "Headset-Lesethread nicht gestartet");
        }
    }

    fn write(&mut self, i: usize) {
        let d = &mut self.devices[i];
        for report in output_reports(&d.tel, self.state) {
            if let Err(e) = d.file.write_all(&report) {
                tracing::debug!(error = %e, name = %d.name, "Headset nicht beschrieben");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wie in descriptor.rs: Report 2 mit Hook (abs), Mute (rel), LEDs
    const DESC: &[u8] = &[
        0x05, 0x0B, 0x09, 0x05, 0xA1, 0x01, 0x85, 0x02, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x09,
        0x20, 0x95, 0x01, 0x81, 0x02, 0x09, 0x2F, 0x81, 0x06, 0x95, 0x06, 0x81, 0x01, 0x05, 0x08,
        0x09, 0x17, 0x09, 0x09, 0x09, 0x18, 0x95, 0x03, 0x91, 0x02, 0x05, 0x0B, 0x09, 0x9E, 0x95,
        0x01, 0x91, 0x02, 0x95, 0x04, 0x91, 0x01, 0xC0,
    ];

    #[test]
    fn hook_and_mute_presses() {
        let mut d = Decoder::new(descriptor::parse(DESC));
        assert_eq!(d.feed(&[2, 0b01]), vec![Button::HookOff]);
        // Gleicher Zustand nochmal: nichts
        assert_eq!(d.feed(&[2, 0b01]), vec![]);
        assert_eq!(d.feed(&[2, 0b11]), vec![Button::Mute]);
        assert_eq!(d.feed(&[2, 0b01]), vec![]);
        assert_eq!(d.feed(&[2, 0b00]), vec![Button::HookOn]);
        // Fremder Report: nichts
        assert_eq!(d.feed(&[1, 0b11]), vec![]);
    }

    #[test]
    fn relative_hook_toggles() {
        let mut desc = DESC.to_vec();
        desc[19] = 0x06; // Hook Switch relativ
        let mut d = Decoder::new(descriptor::parse(&desc));
        assert_eq!(d.feed(&[2, 0b01]), vec![Button::HookToggle]);
        assert_eq!(d.feed(&[2, 0b00]), vec![]);
        assert_eq!(d.feed(&[2, 0b01]), vec![Button::HookToggle]);
    }

    #[test]
    fn leds_and_ringer() {
        let tel = descriptor::parse(DESC);
        let ringing = State {
            ringing: true,
            ..State::default()
        };
        // Off-Hook Bit 0, Mute Bit 1, Ring Bit 2, Ringer Bit 3
        assert_eq!(output_reports(&tel, ringing), vec![vec![2, 0b1100]]);
        let muted = State {
            off_hook: true,
            muted: true,
            ..State::default()
        };
        assert_eq!(output_reports(&tel, muted), vec![vec![2, 0b0011]]);
        // Stumm ohne Gespräch: LED aus
        let idle_muted = State {
            muted: true,
            ..State::default()
        };
        assert_eq!(output_reports(&tel, idle_muted), vec![vec![2, 0]]);
    }

    #[test]
    fn finds_headsets_in_sysfs() {
        let root = std::env::temp_dir().join(format!("sf-headset-{}", std::process::id()));
        let mouse = [0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0xC0];
        for (name, desc, hid) in [
            ("hidraw0", &mouse[..], "Maus"),
            ("hidraw4", DESC, "GN Jabra Evolve2 65"),
        ] {
            let dev = root.join(name).join("device");
            std::fs::create_dir_all(&dev).unwrap();
            std::fs::write(dev.join("report_descriptor"), desc).unwrap();
            std::fs::write(dev.join("uevent"), format!("HID_NAME={hid}\n")).unwrap();
        }
        let found = find_in(&root);
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].dev, PathBuf::from("/dev/hidraw4"));
        assert_eq!(found[0].name, "GN Jabra Evolve2 65");
    }
}
