//! Protokoll für Fehlerberichte: wie bisher auf stderr (Filter aus
//! `RUST_LOG`), dazu immer eine Datei im Log-Ordner der App, die man in den
//! Einstellungen speichern und uns schicken kann. Die Datei wechselt bei
//! [`MAX_SIZE`]; die vorige bleibt als `starclx.1.log` erhalten.
//!
//! Bis der Log-Ordner feststeht (Tauri-Setup), sammelt ein Puffer die
//! Meldungen, damit auch Fehler beim Start in der Datei landen.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use tauri::{AppHandle, Manager};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, fmt, reload};

use crate::i18n::t;

const FILE: &str = "starclx.log";
const PREVIOUS: &str = "starclx.1.log";
const MAX_SIZE: u64 = 5 * 1024 * 1024;
/// Obergrenze für den Puffer bis zum Öffnen der Datei
const EARLY_MAX: usize = 256 * 1024;

const NORMAL: &str = "info";
/// Ausführlich: eigene Crates mit Anruf- und Verbindungsdetails
const VERBOSE: &str = "info,starclx_lib=debug,sf_core=debug,sf_onehub=debug,sf_auth=debug,\
                       sf_chat=debug,sf_sip=debug,sf_audio=debug,sf_busylight=debug";

enum Sink {
    Early(Vec<u8>),
    File { dir: PathBuf, file: File, size: u64 },
    Off,
}

static SINK: Mutex<Sink> = Mutex::new(Sink::Early(Vec::new()));
static SET_FILTER: OnceLock<Box<dyn Fn(EnvFilter) + Send + Sync>> = OnceLock::new();

fn sink() -> MutexGuard<'static, Sink> {
    SINK.lock().unwrap_or_else(|e| e.into_inner())
}

struct Writer;

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut sink = sink();
        match &mut *sink {
            Sink::Early(early) => {
                if early.len() + buf.len() <= EARLY_MAX {
                    early.extend_from_slice(buf);
                }
            }
            Sink::File { dir, file, size } => {
                if *size + buf.len() as u64 > MAX_SIZE {
                    match rotate(dir) {
                        Ok(f) => (*file, *size) = (f, 0),
                        Err(_) => {
                            *sink = Sink::Off;
                            return Ok(buf.len());
                        }
                    }
                }
                // Schreibfehler (Platte voll …) dürfen die App nicht stören.
                if file.write_all(buf).is_ok() {
                    *size += buf.len() as u64;
                }
            }
            Sink::Off => {}
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Aktuelle Datei zur vorigen machen und eine neue anlegen
fn rotate(dir: &Path) -> std::io::Result<File> {
    let _ = std::fs::rename(dir.join(FILE), dir.join(PREVIOUS));
    File::create(dir.join(FILE))
}

/// Richtet das Protokoll ein. Vor allem anderen aufrufen.
pub fn init() {
    let (file_filter, handle) = reload::Layer::new(EnvFilter::new(NORMAL));
    tracing_subscriber::registry()
        .with(fmt::layer().with_filter(EnvFilter::from_default_env()))
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_writer(|| Writer)
                .with_filter(file_filter),
        )
        .init();
    let _ = SET_FILTER.set(Box::new(move |f| {
        let _ = handle.reload(f);
    }));

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(%info, "Absturz");
        previous(info);
    }));
}

/// Öffnet die Datei im Log-Ordner der App und schreibt den Puffer hinein.
pub fn open(app: &AppHandle) {
    let result = app
        .path()
        .app_log_dir()
        .map_err(|e| e.to_string())
        .and_then(|dir| open_in(&dir).map_err(|e| format!("{}: {e}", dir.display())));
    if let Err(e) = result {
        *sink() = Sink::Off;
        tracing::warn!(error = %e, "Protokolldatei nicht geöffnet");
        return;
    }
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        system = %system_info().replace('\n', "; "),
        "StarCLX gestartet"
    );
}

fn open_in(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(FILE);
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let mut file = if size > MAX_SIZE {
        rotate(dir)?
    } else {
        OpenOptions::new().create(true).append(true).open(&path)?
    };
    let mut sink = sink();
    let early = match std::mem::replace(&mut *sink, Sink::Off) {
        Sink::Early(early) => early,
        _ => Vec::new(),
    };
    file.write_all(&early)?;
    let size = file.metadata()?.len();
    *sink = Sink::File {
        dir: dir.to_owned(),
        file,
        size,
    };
    Ok(())
}

/// Schaltet zwischen normalem und ausführlichem Protokoll um.
pub fn set_verbose(verbose: bool) {
    if let Some(set) = SET_FILTER.get() {
        set(EnvFilter::new(if verbose { VERBOSE } else { NORMAL }));
    }
}

/// Betriebssystem, Version und unter Linux Distribution und Desktop
fn system_info() -> String {
    let mut lines = vec![format!(
        "{} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )];
    #[cfg(target_os = "linux")]
    {
        let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
        if let Some(name) = os_release
            .lines()
            .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        {
            lines.push(name.trim_matches('"').to_owned());
        }
        if let Ok(kernel) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
            lines.push(format!("Kernel {}", kernel.trim()));
        }
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        lines.push(format!(
            "Desktop {} ({})",
            env("XDG_CURRENT_DESKTOP"),
            env("XDG_SESSION_TYPE")
        ));
        if std::env::var_os("APPIMAGE").is_some() {
            lines.push("AppImage".into());
        } else if std::env::var_os("FLATPAK_ID").is_some() {
            lines.push("Flatpak".into());
        }
    }
    lines.join("\n")
}

/// Inhalt für „Protokoll speichern“: Kopf, vorige und aktuelle Datei
fn report(dir: &Path) -> Vec<u8> {
    let mut out = format!(
        "StarCLX {}\n{}\n\n",
        env!("CARGO_PKG_VERSION"),
        system_info()
    )
    .into_bytes();
    // Sperren, damit keine halbe Zeile mitkommt
    let _sink = sink();
    for name in [PREVIOUS, FILE] {
        if let Ok(mut f) = File::open(dir.join(name)) {
            let _ = f.read_to_end(&mut out);
        }
    }
    out
}

fn log_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_log_dir().map_err(|e| e.to_string())
}

/// Speichert das Protokoll an einem Ort, den der Benutzer wählt. Liefert den
/// Pfad oder `None` bei Abbruch.
#[tauri::command]
pub async fn log_export(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let dir = log_dir(&app)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title(t("Protokoll speichern"))
        .set_file_name("starclx-log.txt")
        .add_filter(t("Textdatei"), &["txt", "log"])
        .save_file(move |f| {
            let _ = tx.send(f);
        });
    let Some(target) = rx.await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let target = target.into_path().map_err(|e| e.to_string())?;
    let data = tauri::async_runtime::spawn_blocking(move || report(&dir))
        .await
        .map_err(|e| e.to_string())?;
    std::fs::write(&target, data).map_err(|e| e.to_string())?;
    Ok(Some(target.to_string_lossy().into_owned()))
}

/// Öffnet den Log-Ordner im Dateimanager.
#[tauri::command]
pub fn log_open_dir(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = log_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_keeps_previous_file() {
        let dir = std::env::temp_dir().join(format!("starclx-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), "alt\n").unwrap();
        let mut f = rotate(&dir).unwrap();
        f.write_all(b"neu\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(PREVIOUS)).unwrap(),
            "alt\n"
        );
        let r = String::from_utf8(report(&dir)).unwrap();
        assert!(r.starts_with("StarCLX "));
        assert!(r.ends_with("alt\nneu\n"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
