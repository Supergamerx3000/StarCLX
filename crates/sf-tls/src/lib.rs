//! TLS-Prüfung für Anlagen mit eigenem Zertifikat.
//!
//! Lokale Anlagen haben oft ein selbstsigniertes Zertifikat oder eines, das
//! nicht zum Hostnamen bzw. zur IP passt. Wie im Windows-Client kann der
//! Benutzer ein solches Zertifikat einmal bestätigen; danach gilt genau
//! dieses Zertifikat (SHA-256 des Serverzertifikats) als vertrauenswürdig.
//! Alles andere wird weiterhin gegen die Systemzertifikate geprüft.
//!
//! Die bestätigten Fingerabdrücke liegen prozessweit, damit HTTPS, gRPC und
//! XMPP dieselbe Prüfung verwenden.

use std::collections::BTreeSet;
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::Duration;

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use sha2::{Digest, Sha256};

pub use rustls;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

static TRUSTED: RwLock<BTreeSet<String>> = RwLock::new(BTreeSet::new());

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ungültiger Servername: {0}")]
    ServerName(String),
    #[error("Verbindung fehlgeschlagen: {0}")]
    Io(#[from] std::io::Error),
    #[error("TLS: {0}")]
    Tls(#[from] rustls::Error),
    #[error("Zeitüberschreitung beim Verbindungsaufbau")]
    Timeout,
    #[error("Anlage hat kein Zertifikat gesendet")]
    NoCertificate,
}

/// Fingerabdruck im üblichen Format `AB:CD:…` (SHA-256).
pub fn fingerprint(cert: &[u8]) -> String {
    Sha256::digest(cert)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Ersetzt die Liste der bestätigten Zertifikate.
pub fn set_trusted(fingerprints: impl IntoIterator<Item = String>) {
    *TRUSTED.write().unwrap() = fingerprints.into_iter().map(normalize).collect();
}

/// Bestätigt ein weiteres Zertifikat.
pub fn trust(fingerprint: &str) {
    TRUSTED
        .write()
        .unwrap()
        .insert(normalize(fingerprint.to_owned()));
}

fn normalize(fingerprint: String) -> String {
    fingerprint.trim().to_ascii_uppercase()
}

fn is_trusted(cert: &[u8]) -> bool {
    TRUSTED.read().unwrap().contains(&fingerprint(cert))
}

fn provider() -> Arc<CryptoProvider> {
    static PROVIDER: LazyLock<Arc<CryptoProvider>> =
        LazyLock::new(|| Arc::new(rustls::crypto::aws_lc_rs::default_provider()));
    PROVIDER.clone()
}

fn webpki() -> Arc<WebPkiServerVerifier> {
    static WEBPKI: LazyLock<Arc<WebPkiServerVerifier>> = LazyLock::new(|| {
        let mut roots = RootCertStore::empty();
        let native = rustls_native_certs::load_native_certs();
        for e in &native.errors {
            tracing::warn!(error = %e, "Systemzertifikate teilweise nicht gelesen");
        }
        roots.add_parsable_certificates(native.certs);
        WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider())
            .build()
            .expect("Prüfung ohne Wurzelzertifikate")
    });
    WEBPKI.clone()
}

/// Prüft gegen die Systemzertifikate und lässt zusätzlich bestätigte
/// Zertifikate zu (ohne Namensprüfung: der Benutzer hat genau dieses
/// Zertifikat für die Anlage bestätigt).
#[derive(Debug)]
pub struct Verifier {
    inner: Arc<WebPkiServerVerifier>,
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            Err(rustls::Error::InvalidCertificate(_)) if is_trusted(end_entity) => {
                Ok(ServerCertVerified::assertion())
            }
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

pub fn verifier() -> Arc<Verifier> {
    Arc::new(Verifier { inner: webpki() })
}

/// rustls-Konfiguration mit [`Verifier`], ohne ALPN.
pub fn client_config() -> ClientConfig {
    ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("Standard-TLS-Versionen")
        .dangerous()
        .with_custom_certificate_verifier(verifier())
        .with_no_client_auth()
}

/// Zertifikat einer Anlage, das der Benutzer bestätigen muss.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Untrusted {
    pub fingerprint: String,
    /// Grund, z. B. „UnknownIssuer“ oder „NotValidForName“.
    pub reason: String,
}

/// Baut eine TLS-Verbindung zu `host:port` auf und prüft das Zertifikat.
/// `None`: vertrauenswürdig (System oder bereits bestätigt).
pub async fn probe(host: &str, port: u16) -> Result<Option<Untrusted>, Error> {
    let name = ServerName::try_from(host.to_owned()).map_err(|_| Error::ServerName(host.into()))?;
    let seen = Arc::new(Probe {
        inner: verifier(),
        seen: Mutex::default(),
    });
    let config = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(seen.clone())
        .with_no_client_auth();
    let connect = async {
        let tcp = tokio::net::TcpStream::connect((host, port)).await?;
        tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(name, tcp)
            .await
    };
    let result = tokio::time::timeout(PROBE_TIMEOUT, connect)
        .await
        .map_err(|_| Error::Timeout)?;
    match seen.seen.lock().unwrap().take() {
        Some((_, None)) => Ok(None),
        Some((fingerprint, Some(reason))) => Ok(Some(Untrusted {
            fingerprint,
            reason,
        })),
        // Kein Zertifikat gesehen: der Fehler liegt vor der Prüfung.
        None => match result {
            Ok(_) => Err(Error::NoCertificate),
            Err(e) => Err(e.into()),
        },
    }
}

/// Merkt sich das Ergebnis der Prüfung und bricht den Handshake bei einem
/// nicht vertrauenswürdigen Zertifikat ab.
#[derive(Debug)]
struct Probe {
    inner: Arc<Verifier>,
    seen: Mutex<Option<(String, Option<String>)>>,
}

impl ServerCertVerifier for Probe {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let result = self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        );
        let reason = match &result {
            Ok(_) => None,
            Err(rustls::Error::InvalidCertificate(e)) => Some(format!("{e:?}")),
            Err(_) => return result,
        };
        *self.seen.lock().unwrap() = Some((fingerprint(end_entity), reason));
        result
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    /// TLS-Server mit selbstsigniertem Zertifikat für `localhost`.
    async fn self_signed_server() -> (u16, String) {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let der = cert.cert.der().clone();
        let fp = fingerprint(&der);
        let key = rustls::pki_types::PrivateKeyDer::Pkcs8(cert.signing_key.serialize_der().into());
        let config = rustls::ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![der], key)
            .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                if let Ok(mut tls) = acceptor.accept(tcp).await {
                    let _ = tls.write_all(b"ok").await;
                    let _ = tls.shutdown().await;
                }
            }
        });
        (port, fp)
    }

    #[test]
    fn fingerprint_format() {
        let fp = fingerprint(b"");
        assert!(fp.starts_with("E3:B0:C4:42"));
        assert_eq!(fp.len(), 32 * 3 - 1);
    }

    // Ein Test, weil die bestätigten Zertifikate prozessweit gelten.
    #[tokio::test]
    async fn self_signed_needs_confirmation() {
        let (port, fp) = self_signed_server().await;
        set_trusted([]);
        let untrusted = probe("localhost", port).await.unwrap().unwrap();
        assert_eq!(untrusted.fingerprint, fp);
        assert!(untrusted.reason.contains("UnknownIssuer"), "{untrusted:?}");

        trust(&fp.to_lowercase());
        assert_eq!(probe("localhost", port).await.unwrap(), None);

        // Anderes Zertifikat unter derselben Adresse: wieder nachfragen
        let (other, _) = self_signed_server().await;
        assert!(probe("localhost", other).await.unwrap().is_some());
        set_trusted([]);
    }
}
