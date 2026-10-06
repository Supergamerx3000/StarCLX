//! Sitzung mit einer STARFACE-Anlage.
//!
//! Eine [`Session`] hält die gRPC-Verbindung, legt das Refresh-Token im
//! Schlüsselbund ab und erneuert das Access-Token (an der Testanlage 300 s
//! gültig) im Hintergrund, bevor es abläuft.

pub mod account;
pub mod contact_form;
pub mod directory;
pub mod fkeys;
pub mod group;
pub mod journal;
pub mod phone;
pub mod redirect;
pub mod voicemail;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sf_auth::Tokens;
use sf_onehub::{OneHub, TokenHandle};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

/// Erneuern, sobald das Access-Token in weniger als dieser Zeit abläuft.
const REFRESH_MARGIN: Duration = Duration::from_secs(60);
/// Prüfintervall. Bewusst kurz; die Ablaufzeit wird an der Wanduhr gemessen.
/// Nach dem Aufwachen aus dem Standby erneuert [`Session::resume`] sofort.
const CHECK_INTERVAL: Duration = Duration::from_secs(15);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Auth(#[from] sf_auth::Error),
    #[error(transparent)]
    OneHub(#[from] sf_onehub::Error),
    #[error("ungültige Server-Adresse: {0}")]
    Server(String),
    #[error("Hintergrundaufgabe abgebrochen")]
    Join(#[from] tokio::task::JoinError),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// Die Anlage hat das Refresh-Token abgelehnt; neu anmelden.
    LoggedOut { reason: String },
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub server: String,
    pub server_version: String,
    pub first_name: String,
    pub last_name: String,
    pub user_id: String,
    /// Anlage an den STARFACE-Cloud-Diensten (Login über den zentralen
    /// STARFACE-Login, gRPC über das Cloud-Gateway)
    pub cloud: bool,
}

/// Erneuert das Access-Token sofort, z. B. nach dem Aufwachen aus dem Standby.
#[derive(Clone)]
pub struct Resumer(mpsc::UnboundedSender<oneshot::Sender<()>>);

impl Resumer {
    /// `true`, sobald ein frisches Token gilt; `false` nach Zeitüberschreitung
    /// (z. B. Netz noch nicht da; die Erneuerung läuft dann weiter).
    pub async fn resume(&self) -> bool {
        let (tx, rx) = oneshot::channel();
        if self.0.send(tx).is_err() {
            return false;
        }
        matches!(
            tokio::time::timeout(Duration::from_secs(30), rx).await,
            Ok(Ok(()))
        )
    }
}

pub struct Session {
    hub: OneHub,
    info: SessionInfo,
    auth: std::sync::Arc<sf_auth::Client>,
    refresher: JoinHandle<()>,
    /// Sofort erneuern; der Sender meldet den Erfolg zurück
    wake: mpsc::UnboundedSender<oneshot::Sender<()>>,
}

impl Session {
    /// Baut nach einem erfolgreichen Login die Sitzung auf und speichert das
    /// Refresh-Token.
    pub async fn start(
        server: &str,
        auth: sf_auth::Client,
        tokens: Tokens,
        events: mpsc::UnboundedSender<SessionEvent>,
    ) -> Result<Self> {
        let host = url::Url::parse(server)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .ok_or_else(|| Error::Server(server.to_owned()))?;
        let token = TokenHandle::new(tokens.access_token.clone());
        // Cloud-Anlagen: gRPC über das Gateway der STARFACE-Cloud
        let (grpc_host, grpc_port) = auth
            .grpc_endpoint()
            .unwrap_or_else(|| (host.clone(), sf_onehub::DEFAULT_PORT));
        let hub = OneHub::connect(&grpc_host, grpc_port, token.clone()).await?;

        let server_version = hub.server_version().await?;
        let user = hub
            .me()
            .get_user(())
            .await
            .map_err(sf_onehub::Error::from)?
            .into_inner()
            .user;
        let user = user.unwrap_or_default();

        // Ohne Schlüsselbund (kein gnome-keyring/KWallet) trotzdem anmelden,
        // nur eben nicht über das Beenden hinaus.
        STORE_WARNED.store(false, Ordering::Relaxed);
        if let Some(rt) = &tokens.refresh_token
            && let Err(e) = store_refresh_token(server, rt).await
        {
            STORE_WARNED.store(true, Ordering::Relaxed);
            tracing::warn!(error = %e, "Refresh-Token nicht gespeichert; Anmeldung gilt bis zum Beenden");
        }

        let auth = std::sync::Arc::new(auth);
        let (wake, wake_rx) = mpsc::unbounded_channel();
        let refresher = tokio::spawn(refresh_loop(
            server.to_owned(),
            auth.clone(),
            tokens,
            token,
            events,
            CHECK_INTERVAL,
            wake_rx,
        ));
        let info = SessionInfo {
            server: server.to_owned(),
            server_version,
            first_name: user.first_name,
            last_name: user.last_name,
            user_id: user.user_id.map(|u| u.id).unwrap_or_default(),
            cloud: auth.discovery().edge_node_id.is_some(),
        };
        Ok(Self {
            hub,
            info,
            auth,
            refresher,
            wake,
        })
    }

    /// Stellt eine Sitzung aus dem gespeicherten Refresh-Token wieder her.
    /// `None`, wenn nichts gespeichert ist oder die Anlage es nicht mehr
    /// annimmt; dann ist ein neuer Browser-Login nötig.
    pub async fn restore(
        server: &str,
        events: mpsc::UnboundedSender<SessionEvent>,
    ) -> Result<Option<Self>> {
        let Some(refresh_token) = load_refresh_token(server).await else {
            return Ok(None);
        };
        let auth = sf_auth::Client::discover(server).await?;
        let tokens = match auth.refresh(&refresh_token).await {
            Ok(tokens) => tokens,
            Err(sf_auth::Error::Rejected { error, .. }) => {
                tracing::info!(%error, "gespeichertes Refresh-Token abgelehnt");
                delete_refresh_token(server).await;
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };
        Self::start(server, auth, tokens, events).await.map(Some)
    }

    pub fn hub(&self) -> &OneHub {
        &self.hub
    }

    pub fn info(&self) -> &SessionInfo {
        &self.info
    }

    /// Griff zum sofortigen Erneuern des Tokens, nutzbar ohne die Sitzung
    /// selbst festzuhalten.
    pub fn resumer(&self) -> Resumer {
        Resumer(self.wake.clone())
    }

    /// Meldet ab: Token bei der Anlage widerrufen (soweit möglich) und aus dem
    /// Schlüsselbund löschen. Gelingt das nicht, ist man trotzdem abgemeldet.
    pub async fn logout(self) -> Result<()> {
        self.refresher.abort();
        let server = self.info.server.clone();
        if let Some(rt) = load_refresh_token(&server).await
            && let Err(e) = self.auth.revoke(&rt).await
        {
            tracing::warn!(error = %e, "Widerruf fehlgeschlagen, lösche Token nur lokal");
        }
        delete_refresh_token(&server).await;
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.refresher.abort();
    }
}

/// Schon gewarnt, dass das Refresh-Token nicht gespeichert werden kann;
/// gilt bis zur nächsten Anmeldung.
static STORE_WARNED: AtomicBool = AtomicBool::new(false);

async fn store_refresh_token(server: &str, token: &str) -> Result<()> {
    let (server, token) = (server.to_owned(), token.to_owned());
    tokio::task::spawn_blocking(move || sf_auth::secret::save_refresh_token(&server, &token))
        .await??;
    Ok(())
}

/// Gespeichertes Refresh-Token; ohne Schlüsselbund wie „nichts gespeichert“.
async fn load_refresh_token(server: &str) -> Option<String> {
    let key = server.to_owned();
    match tokio::task::spawn_blocking(move || sf_auth::secret::load_refresh_token(&key)).await {
        Ok(Ok(token)) => token,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "Schlüsselbund nicht lesbar");
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "Schlüsselbund nicht lesbar");
            None
        }
    }
}

/// Löscht das gespeicherte Token; ein fehlender Schlüsselbund ist kein Fehler.
async fn delete_refresh_token(server: &str) {
    let server = server.to_owned();
    match tokio::task::spawn_blocking(move || sf_auth::secret::delete_refresh_token(&server)).await
    {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!(error = %e, "Refresh-Token nicht gelöscht"),
        Err(e) => tracing::warn!(error = %e, "Refresh-Token nicht gelöscht"),
    }
}

async fn refresh_loop(
    server: String,
    auth: std::sync::Arc<sf_auth::Client>,
    mut tokens: Tokens,
    handle: TokenHandle,
    events: mpsc::UnboundedSender<SessionEvent>,
    check_interval: Duration,
    mut wake: mpsc::UnboundedReceiver<oneshot::Sender<()>>,
) {
    let mut backoff = Duration::from_secs(5);
    // Warten auf eine erzwungene Erneuerung (Session::resume)
    let mut waiting: Vec<oneshot::Sender<()>> = Vec::new();
    loop {
        let forced = tokio::select! {
            () = tokio::time::sleep(check_interval) => !waiting.is_empty(),
            Some(w) = wake.recv() => {
                waiting.push(w);
                true
            }
        };
        if !forced && !tokens.expires_within(REFRESH_MARGIN) {
            continue;
        }
        let Some(refresh_token) = tokens.refresh_token.clone() else {
            let _ = events.send(SessionEvent::LoggedOut {
                reason: "kein Refresh-Token".into(),
            });
            return;
        };
        match auth.refresh(&refresh_token).await {
            Ok(mut fresh) => {
                handle.set(fresh.access_token.clone());
                // Die Anlage kann das Refresh-Token rotieren; sonst das alte behalten.
                match &fresh.refresh_token {
                    Some(rt) if *rt != refresh_token => {
                        if let Err(e) = store_refresh_token(&server, rt).await {
                            // Ohne Schlüsselbund scheitert das bei jeder Erneuerung;
                            // einmal warnen reicht.
                            if STORE_WARNED.swap(true, Ordering::Relaxed) {
                                tracing::debug!(error = %e, "Refresh-Token nicht gespeichert");
                            } else {
                                tracing::warn!(error = %e, "Refresh-Token nicht gespeichert");
                            }
                        }
                    }
                    Some(_) => {}
                    None => fresh.refresh_token = Some(refresh_token),
                }
                tokens = fresh;
                backoff = Duration::from_secs(5);
                tracing::debug!("Access-Token erneuert");
                for w in waiting.drain(..) {
                    let _ = w.send(());
                }
            }
            Err(sf_auth::Error::Rejected { error, description }) => {
                delete_refresh_token(&server).await;
                let _ = events.send(SessionEvent::LoggedOut {
                    reason: format!("{error} {description}"),
                });
                return;
            }
            Err(e) => {
                tracing::warn!(error = %e, ?backoff, "Token-Erneuerung fehlgeschlagen, neuer Versuch");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Minimaler OAuth-Server: Discovery plus Token-Endpunkt mit festgelegten
    /// Antworten. Gibt die Adresse und die empfangenen Token-Anfragen zurück.
    async fn fake_oauth(
        replies: Vec<(u16, &'static str)>,
    ) -> (String, std::sync::Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
        let (b, s) = (base.clone(), seen.clone());
        tokio::spawn(async move {
            let mut replies = replies.into_iter();
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap();
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                let (status, body) = if req.starts_with("GET /rpc/oauth/login-config") {
                    // Anlage ohne Login-Konfiguration (ältere Version)
                    (404, String::new())
                } else if req.starts_with("GET /.well-known") {
                    (
                        200,
                        format!(
                            r#"{{"authorization_endpoint":"{b}/auth","token_endpoint":"{b}/token"}}"#
                        ),
                    )
                } else {
                    s.lock()
                        .unwrap()
                        .push(req.split("\r\n\r\n").nth(1).unwrap_or("").to_owned());
                    let (st, body) = replies.next().unwrap_or((500, "{}"));
                    (st, body.to_owned())
                };
                let resp = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                sock.write_all(resp.as_bytes()).await.unwrap();
            }
        });
        (base, seen)
    }

    fn expiring(refresh: &str) -> Tokens {
        serde_json::from_str(&format!(
            r#"{{"access_token":"at-0","refresh_token":"{refresh}","expires_in":0}}"#
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn resume_forces_refresh_of_valid_token() {
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        let (server, seen) = fake_oauth(vec![(
            200,
            r#"{"access_token":"at-1","refresh_token":"rt-1","expires_in":300}"#,
        )])
        .await;
        let auth = std::sync::Arc::new(sf_auth::Client::discover(&server).await.unwrap());
        let handle = TokenHandle::new("at-0");
        let valid: Tokens = serde_json::from_str(
            r#"{"access_token":"at-0","refresh_token":"rt-1","expires_in":300}"#,
        )
        .unwrap();
        let (wake, wake_rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(refresh_loop(
            server,
            auth,
            valid,
            handle.clone(),
            mpsc::unbounded_channel().0,
            Duration::from_secs(3600),
            wake_rx,
        ));
        let (tx, rx) = oneshot::channel();
        wake.send(tx).unwrap();
        tokio::time::timeout(Duration::from_secs(5), rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(handle.get(), "at-1");
        assert_eq!(seen.lock().unwrap().len(), 1);
        task.abort();
    }

    #[tokio::test]
    async fn refreshes_and_stores_rotated_token_then_logs_out_on_rejection() {
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        let (server, seen) = fake_oauth(vec![
            (
                200,
                r#"{"access_token":"at-1","refresh_token":"rt-2","expires_in":0}"#,
            ),
            (
                400,
                r#"{"error":"invalid_grant","error_description":"Session not active"}"#,
            ),
        ])
        .await;
        let auth = std::sync::Arc::new(sf_auth::Client::discover(&server).await.unwrap());
        let handle = TokenHandle::new("at-0");
        let (tx, mut rx) = mpsc::unbounded_channel();

        let task = tokio::spawn(refresh_loop(
            server.clone(),
            auth,
            expiring("rt-1"),
            handle.clone(),
            tx,
            Duration::from_millis(10),
            mpsc::unbounded_channel().1,
        ));

        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap();
        assert!(
            matches!(event, Some(SessionEvent::LoggedOut { reason }) if reason.contains("invalid_grant"))
        );
        task.await.unwrap();

        let seen = seen.lock().unwrap();
        assert!(seen[0].contains("refresh_token=rt-1"));
        assert!(
            seen[1].contains("refresh_token=rt-2"),
            "rotiertes Token muss weiterverwendet werden"
        );
        assert_eq!(handle.get(), "at-1");
        // Nach der Ablehnung ist das Token aus dem Schlüsselbund entfernt.
        assert_eq!(sf_auth::secret::load_refresh_token(&server).unwrap(), None);
    }
}
