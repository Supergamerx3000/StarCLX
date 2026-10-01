//! Zertifikate lokaler Anlagen bestätigen, wie im Windows-Client.
//!
//! Vor dem Login prüft die Oberfläche mit [`check_certificate`] den
//! Web-Port und den gRPC-Port der Anlage. Ist ein Zertifikat nicht
//! vertrauenswürdig (meist selbstsigniert), zeigt sie den Fingerabdruck an;
//! bestätigt der Benutzer, merkt sich [`trust_certificate`] ihn in den
//! Einstellungen.

use serde::Serialize;
use tauri::AppHandle;

use crate::settings;

#[derive(Serialize)]
pub struct UntrustedCert {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
    pub reason: String,
}

/// Lädt die bestätigten Zertifikate beim Start.
pub fn init(app: &AppHandle) {
    sf_tls::set_trusted(settings::load(app).trusted_certs.into_values().flatten());
}

/// Ergänzt `https://`, wenn nur ein Hostname eingegeben wurde.
pub fn normalize_server(server: &str) -> String {
    let server = server.trim().trim_end_matches('/');
    if server.contains("://") {
        server.to_owned()
    } else {
        format!("https://{server}")
    }
}

/// Hat der Benutzer für diese Anlage ein Zertifikat bestätigt? Dann prüft
/// auch das Softphone das Zertifikat nicht gegen die Systemzertifikate:
/// die Anlage nutzt für SIP ein Zertifikat ihrer eigenen CA, und baresip
/// kann keine einzelnen Zertifikate bestätigen.
pub fn has_trusted(app: &AppHandle, host: &str) -> bool {
    settings::load(app)
        .trusted_certs
        .keys()
        .any(|server| host_of(server).as_deref() == Some(host))
}

fn host_of(server: &str) -> Option<String> {
    url::Url::parse(server)
        .ok()?
        .host_str()
        .map(|h| h.trim_matches(['[', ']']).to_owned())
}

/// Erstes nicht vertrauenswürdiges Zertifikat der Anlage, sonst `None`.
#[tauri::command]
pub async fn check_certificate(server: String) -> Result<Option<UntrustedCert>, String> {
    let url = url::Url::parse(&normalize_server(&server)).map_err(|e| e.to_string())?;
    let host = host_of(url.as_str()).ok_or("Server-Adresse ohne Hostname")?;
    let mut ports = vec![sf_onehub::DEFAULT_PORT];
    if url.scheme() == "https" {
        ports.insert(0, url.port_or_known_default().unwrap_or(443));
    }
    for port in ports {
        match sf_tls::probe(&host, port).await {
            Ok(None) => {}
            Ok(Some(cert)) => {
                return Ok(Some(UntrustedCert {
                    host,
                    port,
                    fingerprint: cert.fingerprint,
                    reason: cert.reason,
                }));
            }
            // Nicht erreichbar o. ä.: das meldet der Login selbst genauer.
            Err(e) => tracing::info!(error = %e, %host, port, "Zertifikat nicht geprüft"),
        }
    }
    Ok(None)
}

#[tauri::command]
pub fn trust_certificate(app: AppHandle, server: String, fingerprint: String) {
    sf_tls::trust(&fingerprint);
    settings::update(&app, |s| {
        s.trusted_certs
            .entry(normalize_server(&server))
            .or_default()
            .insert(fingerprint.to_ascii_uppercase());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_hostname_gets_https() {
        assert_eq!(normalize_server(" pbx.local/ "), "https://pbx.local");
        assert_eq!(normalize_server("http://10.0.0.1"), "http://10.0.0.1");
    }

    #[test]
    fn host_without_brackets() {
        assert_eq!(host_of("https://[fd00::1]:443").as_deref(), Some("fd00::1"));
        assert_eq!(host_of("https://pbx.local").as_deref(), Some("pbx.local"));
    }
}
