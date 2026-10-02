//! OAuth2-Anmeldung an der STARFACE.
//!
//! Die Anlage akzeptiert für Desktop-Clients nur die Client-ID `windows-app`
//! mit dem Redirect `starface-app://login` und dem Scope `pbx-login`
//! (Loopback-Redirects und `openid` werden abgelehnt, live geprüft an
//! 10.0.2.6). Unter Linux nimmt ein `x-scheme-handler/starface-app` den Code
//! entgegen.

use std::time::{Duration, SystemTime};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

pub mod secret;

pub const CLIENT_ID: &str = "windows-app";
pub const REDIRECT_URI: &str = "starface-app://login";
pub const SCOPE: &str = "pbx-login";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ungültige Server-Adresse: {0}")]
    Url(#[from] url::ParseError),
    #[error("HTTP-Fehler: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Anlage lehnt ab: {error} {description}")]
    Rejected { error: String, description: String },
    #[error("Schlüsselbund: {0}")]
    Secret(#[from] keyring_core::Error),
    #[error("Zufallsgenerator nicht verfügbar: {0}")]
    Random(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Relevanter Teil von `/.well-known/openid-configuration`.
#[derive(Debug, Clone, Deserialize)]
pub struct Discovery {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    /// Bei Cloud-Anlagen mit Edge-Node gesetzt; dann braucht jede
    /// Token-Anfrage `resource=edgenode://<id>`.
    #[serde(rename = "edgeNodeId", default)]
    pub edge_node_id: Option<String>,
    #[serde(default)]
    pub revocation_endpoint: Option<String>,
}

/// PKCE-Paar (RFC 7636, Methode S256).
#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn generate() -> Result<Self> {
        let mut buf = [0u8; 32];
        getrandom::fill(&mut buf).map_err(|e| Error::Random(e.to_string()))?;
        Ok(Self::from_verifier(URL_SAFE_NO_PAD.encode(buf)))
    }

    pub fn from_verifier(verifier: String) -> Self {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Lebensdauer des Access-Tokens in Sekunden (an der Testanlage 300).
    pub expires_in: u64,
    #[serde(skip, default = "SystemTime::now")]
    pub received_at: SystemTime,
}

impl Tokens {
    /// Läuft das Access-Token innerhalb von `margin` ab?
    pub fn expires_within(&self, margin: Duration) -> bool {
        let expiry = self.received_at + Duration::from_secs(self.expires_in);
        SystemTime::now() + margin >= expiry
    }
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
    #[serde(default)]
    error_description: String,
}

/// OAuth-Client für eine Anlage, z. B. `https://pbx.example.com`.
pub struct Client {
    http: reqwest::Client,
    discovery: Discovery,
}

impl Client {
    pub async fn discover(server: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("starclx/", env!("CARGO_PKG_VERSION")))
            .tls_backend_preconfigured(sf_tls::client_config())
            .build()?;
        let url = Url::parse(server)?.join("/.well-known/openid-configuration")?;
        // Die Anlage leitet auf /auth/realms/pbx/… weiter; reqwest folgt dem.
        let discovery = http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(Self { http, discovery })
    }

    pub fn discovery(&self) -> &Discovery {
        &self.discovery
    }

    /// URL für den Systembrowser.
    pub fn authorize_url(&self, pkce: &Pkce, state: &str) -> Result<Url> {
        let mut url = Url::parse(&self.discovery.authorization_endpoint)?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", CLIENT_ID)
            .append_pair("redirect_uri", REDIRECT_URI)
            .append_pair("scope", SCOPE)
            .append_pair("state", state)
            .append_pair("code_challenge", &pkce.challenge)
            .append_pair("code_challenge_method", "S256");
        if let Some(resource) = self.resource() {
            url.query_pairs_mut().append_pair("resource", &resource);
        }
        Ok(url)
    }

    pub async fn exchange_code(&self, code: &str, pkce: &Pkce) -> Result<Tokens> {
        self.token_request(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", &pkce.verifier),
        ])
        .await
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<Tokens> {
        self.token_request(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .await
    }

    /// Nur für Entwicklung und Tests: braucht das Benutzerrecht
    /// „API access with Password Grant“.
    pub async fn password_grant(&self, username: &str, password: &str) -> Result<Tokens> {
        self.token_request(&[
            ("grant_type", "password"),
            ("username", username),
            ("password", password),
            ("scope", SCOPE),
        ])
        .await
    }

    /// Widerruft ein Refresh-Token beim Abmelden. Fehlt der Endpunkt, ist das
    /// kein Fehler: das Token wird dann nur lokal gelöscht.
    pub async fn revoke(&self, refresh_token: &str) -> Result<()> {
        let Some(endpoint) = &self.discovery.revocation_endpoint else {
            return Ok(());
        };
        let form = [
            ("client_id", CLIENT_ID),
            ("token", refresh_token),
            ("token_type_hint", "refresh_token"),
        ];
        self.http
            .post(endpoint)
            .form(&form)
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    fn resource(&self) -> Option<String> {
        self.discovery
            .edge_node_id
            .as_ref()
            .map(|id| format!("edgenode://{id}"))
    }

    async fn token_request(&self, params: &[(&str, &str)]) -> Result<Tokens> {
        let mut form: Vec<(&str, &str)> = vec![("client_id", CLIENT_ID)];
        form.extend_from_slice(params);
        let resource = self.resource();
        if let Some(resource) = &resource {
            form.push(("resource", resource));
        }
        let resp = self
            .http
            .post(&self.discovery.token_endpoint)
            .form(&form)
            .send()
            .await?;
        if !resp.status().is_success() {
            let body: ErrorBody = resp.json().await?;
            return Err(Error::Rejected {
                error: body.error,
                description: body.error_description,
            });
        }
        Ok(resp.json().await?)
    }
}

/// Liest `code` aus dem Redirect `starface-app://login?code=…&state=…` und
/// prüft `state`.
pub fn code_from_redirect(redirect: &str, expected_state: &str) -> Option<String> {
    let url = Url::parse(redirect).ok()?;
    let mut code = None;
    let mut state_ok = false;
    for (k, v) in url.query_pairs() {
        match &*k {
            "code" => code = Some(v.into_owned()),
            "state" => state_ok = v == expected_state,
            _ => {}
        }
    }
    code.filter(|_| state_ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_example() {
        // RFC 7636, Anhang B
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk".into());
        assert_eq!(
            pkce.challenge,
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn generated_verifier_has_valid_length() {
        let pkce = Pkce::generate().unwrap();
        assert!((43..=128).contains(&pkce.verifier.len()));
    }

    #[test]
    fn redirect_requires_matching_state() {
        let r = "starface-app://login?code=abc&state=xyz";
        assert_eq!(code_from_redirect(r, "xyz").as_deref(), Some("abc"));
        assert_eq!(code_from_redirect(r, "other"), None);
    }

    #[test]
    fn authorize_url_carries_edge_node_resource() {
        let client = Client {
            http: reqwest::Client::new(),
            discovery: Discovery {
                authorization_endpoint: "https://pbx/auth/realms/pbx/oauth2/auth".into(),
                token_endpoint: "https://pbx/auth/realms/pbx/oauth2/token".into(),
                edge_node_id: Some("42".into()),
                revocation_endpoint: None,
            },
        };
        let pkce = Pkce::from_verifier("v".repeat(43));
        let url = client.authorize_url(&pkce, "s").unwrap();
        let q: Vec<_> = url.query_pairs().collect();
        assert!(q.iter().any(|(k, v)| k == "client_id" && v == CLIENT_ID));
        assert!(
            q.iter()
                .any(|(k, v)| k == "redirect_uri" && v == REDIRECT_URI)
        );
        assert!(
            q.iter()
                .any(|(k, v)| k == "resource" && v == "edgenode://42")
        );
    }

    #[test]
    fn discovery_parses_edge_node_id() {
        let d: Discovery = serde_json::from_str(
            r#"{"authorization_endpoint":"a","token_endpoint":"t","edgeNodeId":"7"}"#,
        )
        .unwrap();
        assert_eq!(d.edge_node_id.as_deref(), Some("7"));
    }
}
