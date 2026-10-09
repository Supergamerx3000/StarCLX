//! Plugins: abgeschlossene Funktionen. Der Kern ruft sie beim An- und
//! Abmelden der Reihe nach auf; alles andere erfahren sie über den Bus.
//! Sie sind fest eingebaut, nicht nachgeladen.
//!
//! Jedes Plugin hat einen eigenen Ordner, hier für den Rust-Teil und unter
//! `src/lib/plugins/<name>/` für seine Oberfläche.

use tauri::AppHandle;

pub mod busylight;
pub mod call;
pub mod callactions;
pub mod chat;
pub mod contacts;
pub mod doorcam;
pub mod fkeys;
pub mod journal;
pub mod reach;
pub mod voicemail;

/// Angaben zur neuen Sitzung
pub struct Login {
    pub hub: sf_onehub::OneHub,
    /// Rechnername der Anlage
    pub host: String,
    pub user_id: String,
    /// Vor- und Nachname (Spitzname in Gruppenchats)
    pub display_name: String,
}

/// Beim Start einmal: Plugins hängen sich an den Bus.
pub fn start(app: &AppHandle) {
    busylight::start(app);
    callactions::start(app);
}

/// Abgemeldet oder Sitzung gewechselt; wartet, bis alle fertig sind.
pub async fn session_ended(app: &AppHandle) {
    call::session_ended(app).await;
    journal::session_ended(app).await;
    reach::session_ended(app).await;
    voicemail::session_ended(app).await;
    fkeys::session_ended(app).await;
    chat::session_ended(app).await;
}

/// Angemeldet
pub async fn session_started(app: &AppHandle, login: Login) {
    journal::session_started(app, login.hub.clone()).await;
    reach::session_started(app, login.hub.clone()).await;
    voicemail::session_started(app, login.hub.clone()).await;
    chat::session_started(
        app,
        login.hub.clone(),
        login.host.clone(),
        login.user_id,
        login.display_name,
    );
    call::session_started(app, login.hub, login.host);
}
