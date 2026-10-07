//! Plugins: abgeschlossene Funktionen, die über den Bus erfahren, was im
//! Kern passiert. Sie sind fest eingebaut, nicht nachgeladen.
//!
//! Jedes Plugin hat einen eigenen Ordner, hier für den Rust-Teil und unter
//! `src/lib/plugins/<name>/` für seine Oberfläche.

use tauri::AppHandle;

pub mod busylight;
pub mod call;
pub mod callactions;

/// Alle Plugins einhängen, bevor der Kern etwas meldet
pub fn start(app: &AppHandle) {
    busylight::start(app);
    call::start(app);
    callactions::start(app);
}
