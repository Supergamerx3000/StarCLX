//! Interner Ereignis-Bus. Der Kern meldet hier, was passiert; Plugins hören
//! zu, statt dass der Kern jedes einzeln aufruft.

use sf_core::phone::CallView;
use tauri::{AppHandle, Manager};
use tokio::sync::broadcast;

#[derive(Clone, Debug)]
pub enum Event {
    /// Softphone beendet (Abmelden, neue Sitzung)
    PhoneStopped,
    /// Softphone gestartet und bereit
    PhoneReady,
    /// Anrufe haben sich geändert
    Calls { calls: Vec<CallView> },
    /// Einstellungen gespeichert
    PrefsSaved,
}

pub struct Bus(broadcast::Sender<Event>);

impl Default for Bus {
    fn default() -> Self {
        Self(broadcast::channel(256).0)
    }
}

pub fn publish(app: &AppHandle, event: Event) {
    // Ohne Zuhörer ist das kein Fehler
    let _ = app.state::<Bus>().0.send(event);
}

/// Ruft `handle` in einer eigenen Aufgabe für jedes Ereignis auf, in der
/// Reihenfolge, in der sie gemeldet wurden.
pub fn listen(
    app: &AppHandle,
    plugin: &'static str,
    mut handle: impl FnMut(&AppHandle, Event) + Send + 'static,
) {
    let mut rx = app.state::<Bus>().0.subscribe();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => handle(&app, event),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(plugin, n, "Ereignisse verpasst");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}
