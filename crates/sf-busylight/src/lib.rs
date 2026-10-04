//! Kuando Busylight (Alpha, Omega und ältere Lync-Modelle) über `/dev/hidraw*`.
//!
//! Das Licht erwartet 64-Byte-Pakete aus bis zu sieben Schritten zu je acht
//! Bytes (Farbe, Ein-/Auszeit, Ton) und einer Prüfsumme. Ohne Nachricht
//! schaltet es sich nach etwa 30 Sekunden ab; [`Busylight`] schickt deshalb
//! regelmässig ein Keep-Alive und sucht neu angesteckte Geräte.
//!
//! Zugriff auf hidraw braucht eine udev-Regel (liegt dem .deb bei). Unter
//! macOS und Windows läuft der Zugriff über hidapi.

#[cfg(target_os = "linux")]
use std::fs::OpenOptions;
#[cfg(target_os = "linux")]
use std::io::Write;
#[cfg(target_os = "linux")]
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Hersteller-/Produkt-IDs: Plenom (alle Modelle) und das alte Microchip-Gerät
const VENDOR_PLENOM: u32 = 0x27BB;
const LEGACY: (u32, u32) = (0x04D8, 0xF848);

const KEEPALIVE_EVERY: Duration = Duration::from_secs(10);
const RESCAN_EVERY: Duration = Duration::from_secs(5);

/// Eingebaute Töne des Busylight, in Anzeigereihenfolge (Name, Code)
pub const TONES: &[(&str, u8)] = &[
    ("OpenOffice", 1),
    ("Quiet", 2),
    ("Funky", 3),
    ("FairyTale", 4),
    ("KuandoTrain", 5),
    ("TelephoneNordic", 6),
    ("TelephoneOriginal", 7),
    ("TelephonePickMeUp", 8),
    ("Buzz", 11),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const RED: Rgb = Rgb(100, 0, 0);
    pub const GREEN: Rgb = Rgb(0, 100, 0);
    pub const YELLOW: Rgb = Rgb(100, 60, 0);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Light {
    Off,
    Steady(Rgb),
    /// Blinken; optional mit Ton (Name aus [`TONES`], Lautstärke 0..=100)
    Blink {
        color: Rgb,
        tone: Option<(String, u8)>,
    },
}

/// Gefundene Geräte bzw. der Grund, warum keines nutzbar ist
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub devices: Vec<String>,
    pub error: Option<String>,
}

/// Sucht angesteckte Busylights unter `/sys/class/hidraw`.
#[cfg(target_os = "linux")]
pub fn find() -> Vec<PathBuf> {
    find_in(Path::new("/sys/class/hidraw"))
}

#[cfg(target_os = "linux")]
fn find_in(sys: &Path) -> Vec<PathBuf> {
    let Ok(dir) = std::fs::read_dir(sys) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = dir
        .flatten()
        .filter(|e| {
            std::fs::read_to_string(e.path().join("device/uevent")).is_ok_and(|u| is_busylight(&u))
        })
        .map(|e| Path::new("/dev").join(e.file_name()))
        .collect();
    out.sort();
    out
}

/// Prüft `HID_ID=0003:000027BB:00003BCD` aus einer uevent-Datei.
#[cfg(target_os = "linux")]
fn is_busylight(uevent: &str) -> bool {
    uevent
        .lines()
        .find_map(|l| l.strip_prefix("HID_ID="))
        .and_then(|id| {
            let mut parts = id.split(':').skip(1);
            let vendor = u32::from_str_radix(parts.next()?, 16).ok()?;
            let product = u32::from_str_radix(parts.next()?, 16).ok()?;
            Some(vendor == VENDOR_PLENOM || (vendor, product) == LEGACY)
        })
        .unwrap_or(false)
}

/// Prüft angesteckte Geräte auf Schreibrechte, ohne etwas zu senden.
pub fn probe() -> Status {
    let devices = find();
    let error = devices
        .iter()
        .find_map(|d| check(d).err())
        .map(|e| describe(&e));
    Status {
        devices: devices.iter().map(|d| d.display().to_string()).collect(),
        error,
    }
}

fn volume_code(volume: u8) -> u8 {
    ((u16::from(volume.min(100)) * 7 + 50) / 100) as u8
}

fn tone_code(name: &str) -> Option<u8> {
    TONES.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// Baut das 64-Byte-Paket für einen Zustand.
fn packet(light: &Light) -> [u8; 64] {
    // Schritt 0: Sprung auf sich selbst (Endlosschleife), Ton wird gesetzt.
    let (rgb, on, off, audio) = match light {
        Light::Off => (Rgb(0, 0, 0), 0, 0, 0),
        Light::Steady(c) => (*c, 0, 0, 0),
        Light::Blink { color, tone } => {
            let audio = tone
                .as_ref()
                .and_then(|(name, vol)| Some(tone_code(name)? << 3 | volume_code(*vol)))
                .unwrap_or(0);
            (*color, 5, 5, audio)
        }
    };
    let mut p = [0u8; 64];
    p[..8].copy_from_slice(&[0x10, 0, rgb.0, rgb.1, rgb.2, on, off, 0x80 | audio]);
    finish(p)
}

fn keepalive() -> [u8; 64] {
    let mut p = [0u8; 64];
    p[0] = 0x80 | 0x0F; // Keep-Alive, Zeitlimit 15 s
    finish(p)
}

fn finish(mut p: [u8; 64]) -> [u8; 64] {
    p[59..62].fill(0xFF);
    let sum: u16 = p[..62].iter().map(|b| u16::from(*b)).sum();
    p[62..].copy_from_slice(&sum.to_be_bytes());
    p
}

/// Öffnet das Gerät zum Schreiben, ohne etwas zu senden.
#[cfg(target_os = "linux")]
fn check(dev: &Path) -> std::io::Result<()> {
    OpenOptions::new().write(true).open(dev).map(drop)
}

/// hidraw: erstes Byte ist die Report-Nummer (0 = ohne Nummern)
#[cfg(target_os = "linux")]
fn write(dev: &Path, p: &[u8; 64]) -> std::io::Result<()> {
    OpenOptions::new()
        .write(true)
        .open(dev)?
        .write_all(&report(p))
}

/// HID-Report: erstes Byte ist die Report-Nummer (0 = ohne Nummern)
fn report(p: &[u8; 64]) -> [u8; 65] {
    let mut buf = [0u8; 65];
    buf[1..].copy_from_slice(p);
    buf
}

/// macOS und Windows: über hidapi. Die Gerätepfade von hidapi stehen hier
/// als `PathBuf`, damit der Rest gleich bleibt.
#[cfg(not(target_os = "linux"))]
mod hid {
    use std::ffi::CString;
    use std::path::{Path, PathBuf};

    use hidapi::HidApi;

    fn io_err(e: impl std::fmt::Display) -> std::io::Error {
        std::io::Error::other(e.to_string())
    }

    pub fn find() -> Vec<PathBuf> {
        let Ok(api) = HidApi::new() else {
            return Vec::new();
        };
        let mut out: Vec<PathBuf> = api
            .device_list()
            .filter(|d| {
                let (vendor, product) = (u32::from(d.vendor_id()), u32::from(d.product_id()));
                vendor == super::VENDOR_PLENOM || (vendor, product) == super::LEGACY
            })
            .filter_map(|d| d.path().to_str().ok().map(PathBuf::from))
            .collect();
        out.sort();
        out.dedup();
        out
    }

    fn open(dev: &Path) -> std::io::Result<hidapi::HidDevice> {
        let api = HidApi::new().map_err(io_err)?;
        let path = CString::new(dev.to_string_lossy().as_bytes()).map_err(io_err)?;
        api.open_path(&path).map_err(io_err)
    }

    pub fn check(dev: &Path) -> std::io::Result<()> {
        open(dev).map(drop)
    }

    pub fn write(dev: &Path, p: &[u8; 64]) -> std::io::Result<()> {
        open(dev)?
            .write(&super::report(p))
            .map(drop)
            .map_err(io_err)
    }
}

#[cfg(not(target_os = "linux"))]
pub use hid::find;
#[cfg(not(target_os = "linux"))]
use hid::{check, write};

/// Hält den gewünschten Zustand auf allen angesteckten Busylights.
pub struct Busylight {
    tx: mpsc::Sender<Msg>,
    thread: Option<JoinHandle<()>>,
}

enum Msg {
    Set(Light),
    Status(mpsc::Sender<Status>),
}

impl Busylight {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("busylight".into())
            .spawn(move || run(&rx))
            .ok();
        Self { tx, thread }
    }

    pub fn set(&self, light: Light) {
        let _ = self.tx.send(Msg::Set(light));
    }

    pub fn status(&self) -> Status {
        let (tx, rx) = mpsc::channel();
        if self.tx.send(Msg::Status(tx)).is_err() {
            return Status::default();
        }
        rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default()
    }
}

impl Drop for Busylight {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Set(Light::Off));
        // Kanal schliessen, damit der Thread das Aus noch schreibt und endet
        let (dead, _) = mpsc::channel();
        drop(std::mem::replace(&mut self.tx, dead));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn describe(e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        "Keine Berechtigung für das Gerät (udev-Regel fehlt)".into()
    } else {
        e.to_string()
    }
}

struct Worker {
    light: Light,
    status: Status,
    /// Geräte, die den aktuellen Zustand schon bekommen haben
    done: Vec<PathBuf>,
}

impl Worker {
    /// Sucht Geräte und schreibt den Zustand auf alle, die ihn noch nicht haben.
    fn scan(&mut self) {
        let devices = find();
        self.done.retain(|d| devices.contains(d));
        self.status.devices = devices.iter().map(|d| d.display().to_string()).collect();
        self.status.error = None;
        let packet = packet(&self.light);
        for dev in &devices {
            if self.done.contains(dev) {
                continue;
            }
            match write(dev, &packet) {
                Ok(()) => self.done.push(dev.clone()),
                Err(e) => {
                    tracing::debug!(error = %e, dev = %dev.display(), "Busylight nicht beschrieben");
                    self.status.error = Some(describe(&e));
                }
            }
        }
    }

    fn keepalive(&self) {
        for dev in &self.done {
            let _ = write(dev, &keepalive());
        }
    }
}

fn run(rx: &mpsc::Receiver<Msg>) {
    let mut w = Worker {
        light: Light::Off,
        status: Status::default(),
        done: Vec::new(),
    };
    let mut last_scan = Instant::now();
    let mut last_keepalive = Instant::now();
    loop {
        let active = w.light != Light::Off;
        // Ausgeschaltet nur auf Nachrichten warten; sonst neue Geräte suchen
        // und das Licht wachhalten.
        let wait = if active {
            RESCAN_EVERY.saturating_sub(last_scan.elapsed())
        } else {
            Duration::from_secs(3600)
        };
        match rx.recv_timeout(wait) {
            Ok(Msg::Set(light)) => {
                if light != w.light {
                    w.light = light;
                    w.done.clear();
                    w.scan();
                    last_scan = Instant::now();
                    last_keepalive = Instant::now();
                }
            }
            Ok(Msg::Status(reply)) => {
                w.scan();
                last_scan = Instant::now();
                let _ = reply.send(w.status.clone());
            }
            Err(RecvTimeoutError::Timeout) => {
                if active {
                    w.scan();
                    last_scan = Instant::now();
                    if last_keepalive.elapsed() >= KEEPALIVE_EVERY {
                        w.keepalive();
                        last_keepalive = Instant::now();
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn recognises_kuando_ids() {
        assert!(is_busylight(
            "DRIVER=hid-generic\nHID_ID=0003:000027BB:00003BCD\nHID_NAME=x"
        ));
        assert!(is_busylight("HID_ID=0003:000004D8:0000F848"));
        assert!(!is_busylight("HID_ID=0003:0000046D:0000C52B"));
        assert!(!is_busylight("HID_NAME=nothing"));
    }

    #[test]
    fn packet_layout_and_checksum() {
        let p = packet(&Light::Steady(Rgb::RED));
        assert_eq!(&p[..8], &[0x10, 0, 100, 0, 0, 0, 0, 0x80]);
        assert_eq!(&p[59..62], &[0xFF; 3]);
        let sum: u16 = p[..62].iter().map(|b| u16::from(*b)).sum();
        assert_eq!(u16::from_be_bytes([p[62], p[63]]), sum);
    }

    #[test]
    fn blink_with_tone() {
        let p = packet(&Light::Blink {
            color: Rgb::RED,
            tone: Some(("Quiet".into(), 100)),
        });
        assert_eq!(p[5..7], [5, 5]);
        assert_eq!(p[7], 0x80 | 2 << 3 | 7);
        let silent = packet(&Light::Blink {
            color: Rgb::RED,
            tone: Some(("unbekannt".into(), 50)),
        });
        assert_eq!(silent[7], 0x80);
        assert_eq!(volume_code(0), 0);
        assert_eq!(volume_code(50), 4);
    }

    #[test]
    fn keepalive_packet() {
        let p = keepalive();
        assert_eq!(p[0], 0x8F);
        assert_eq!(u16::from_be_bytes([p[62], p[63]]), 0x8F + 3 * 0xFF);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn finds_devices_in_sysfs() {
        let root = std::env::temp_dir().join(format!("sf-busylight-{}", std::process::id()));
        for (name, id) in [
            ("hidraw0", "0003:0000046D:0000C52B"),
            ("hidraw3", "0003:000027BB:00003BCA"),
        ] {
            let dev = root.join(name).join("device");
            std::fs::create_dir_all(&dev).unwrap();
            std::fs::write(dev.join("uevent"), format!("HID_ID={id}\n")).unwrap();
        }
        let found = find_in(&root);
        std::fs::remove_dir_all(&root).ok();
        assert_eq!(found, vec![PathBuf::from("/dev/hidraw3")]);
    }
}
