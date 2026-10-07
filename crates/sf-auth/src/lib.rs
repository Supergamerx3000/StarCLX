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
/// Scope für den Login der Anlage selbst (Realm `pbx`).
pub const SCOPE: &str = "pbx-login";
/// Scopes beim zentralen STARFACE-Login (Anlagen an den Cloud-Diensten).
/// `pbx-login` gibt es dort für `windows-app` nicht; ohne `one-hub-ucapi`
/// stellt er nur ein Profil-Token aus, das die Anlage ablehnt.
pub const CLOUD_SCOPE: &str = "openid one-hub-ucapi one-hub-user-profile";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ungültige Server-Adresse: {0}")]
    Url(#[from] url::ParseError),
    #[error("HTTP-Fehler: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Anlage lehnt ab: {error} {description}")]
    Rejected { error: String, description: String },
    /// Rate-Limit der Anlage (HTTP 429); kein Grund, sich abzumelden.
    #[error("Anlage bittet zu warten (Rate-Limit), {}s", retry_after.as_secs())]
    Limited { retry_after: std::time::Duration },
    /// Störung der Anlage (HTTP 5xx); kein Grund, sich abzumelden.
    #[error("Anlage gestört: HTTP {0}")]
    Server(u16),
    #[error("Schlüsselbund: {0}")]
    Secret(#[from] keyring_core::Error),
    #[error("Zufallsgenerator nicht verfügbar: {0}")]
    Random(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Relevanter Teil von `/.well-known/openid-configuration`.
#[derive(Debug, Clone, Deserialize)]
pub struct Discovery {
    #[serde(default)]
    pub issuer: String,
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
    /// Unkritische Angaben aus dem Access-Token (JWT) fürs Protokoll:
    /// Aussteller, Zielgruppe, Client, Scopes und die Namen aller Felder.
    /// Signatur und persönliche Werte bleiben draußen.
    pub fn summary(&self) -> String {
        let Some(payload) = self.access_token.split('.').nth(1) else {
            return "kein JWT".into();
        };
        let Ok(json) = URL_SAFE_NO_PAD
            .decode(payload.trim_end_matches('='))
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .ok_or(())
        else {
            return "JWT nicht lesbar".into();
        };
        let pick = |k: &str| json.get(k).map(ToString::to_string).unwrap_or_default();
        let keys: Vec<&str> = json
            .as_object()
            .map(|o| o.keys().map(String::as_str).collect())
            .unwrap_or_default();
        format!(
            "iss={} aud={} azp={} scope={} felder={keys:?}",
            pick("iss"),
            pick("aud"),
            pick("azp"),
            pick("scope")
        )
    }

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

/// `/rpc/oauth/login-config` der Anlage (so fragt auch die STARFACE-App).
/// Bei Anlagen an den STARFACE-Cloud-Diensten stehen hier die Edge-Node-ID
/// für die Token-Anfragen und der zentrale gRPC-Zugang.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginConfig {
    #[serde(default)]
    pub oauth_config_url: Option<String>,
    #[serde(default)]
    pub edge_node_id: Option<String>,
    /// z. B. `grpcs://ucapi.unified-cloud.eu`
    #[serde(default, rename = "gRpcUrl")]
    pub grpc_url: Option<String>,
}

/// OAuth-Client für eine Anlage, z. B. `https://pbx.example.com`.
pub struct Client {
    http: reqwest::Client,
    discovery: Discovery,
    login_config: LoginConfig,
}

impl Client {
    pub async fn discover(server: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("starclx/", env!("CARGO_PKG_VERSION")))
            .tls_backend_preconfigured(sf_tls::client_config())
            .build()?;
        let base = Url::parse(server)?;
        // Ältere Anlagen kennen den Endpunkt nicht; dann gelten die Vorgaben.
        let login_config: LoginConfig = match http
            .get(base.join("/rpc/oauth/login-config")?)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
        {
            Ok(r) => r.json().await.unwrap_or_default(),
            Err(_) => LoginConfig::default(),
        };
        let url = match &login_config.oauth_config_url {
            Some(u) => Url::parse(u)?,
            None => base.join("/.well-known/openid-configuration")?,
        };
        // Die Anlage leitet auf /auth/realms/pbx/… bzw. den zentralen
        // STARFACE-Login weiter; reqwest folgt dem.
        let mut discovery: Discovery = http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if discovery.edge_node_id.is_none() {
            discovery
                .edge_node_id
                .clone_from(&login_config.edge_node_id);
        }
        Ok(Self {
            http,
            discovery,
            login_config,
        })
    }

    pub fn discovery(&self) -> &Discovery {
        &self.discovery
    }

    /// Eigener gRPC-Zugang (Host, Port), falls die Anlage einen vorgibt,
    /// z. B. das Gateway der STARFACE-Cloud. Sonst gilt die Anlage selbst.
    pub fn grpc_endpoint(&self) -> Option<(String, u16)> {
        let url = Url::parse(self.login_config.grpc_url.as_deref()?).ok()?;
        let host = url.host_str()?.to_owned();
        Some((host, url.port().unwrap_or(443)))
    }

    /// URL für den Systembrowser.
    pub fn authorize_url(&self, pkce: &Pkce, state: &str) -> Result<Url> {
        let mut url = Url::parse(&self.discovery.authorization_endpoint)?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", CLIENT_ID)
            .append_pair("redirect_uri", REDIRECT_URI)
            .append_pair("state", state)
            .append_pair("code_challenge", &pkce.challenge)
            .append_pair("code_challenge_method", "S256");
        if let Some(scope) = self.scope() {
            url.query_pairs_mut().append_pair("scope", scope);
        }
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
        let mut params = vec![
            ("grant_type", "password"),
            ("username", username),
            ("password", password),
        ];
        if let Some(scope) = self.scope() {
            params.push(("scope", scope));
        }
        self.token_request(&params).await
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

    /// `pbx-login` beim Login der Anlage (Realm `pbx`), sonst die Scopes des
    /// zentralen STARFACE-Logins
    fn scope(&self) -> Option<&'static str> {
        let local = self
            .discovery
            .issuer
            .trim_end_matches('/')
            .ends_with("/realms/pbx")
            || self.discovery.issuer.is_empty();
        Some(if local { SCOPE } else { CLOUD_SCOPE })
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
        let status = resp.status();
        // Nur eine echte Absage (4xx mit OAuth-Fehler) beendet die Sitzung;
        // Rate-Limit und Störungen der Anlage sind vorübergehend.
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok());
            return Err(Error::Limited {
                retry_after: sf_backoff::parse_retry_after(retry_after),
            });
        }
        if status.is_server_error() {
            return Err(Error::Server(status.as_u16()));
        }
        if !status.is_success() {
            let body: ErrorBody = resp.json().await?;
            return Err(Error::Rejected {
                error: body.error,
                description: body.error_description,
            });
        }
        Ok(resp.json().await?)
    }
}

/// Warum ein Rücksprung keinen Code liefert
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedirectError {
    /// Die Anlage meldet einen Fehler (`error`, `error_description`)
    Denied { error: String, description: String },
    /// `state` fehlt oder gehört zu einem anderen Anmeldeversuch
    StateMismatch,
    /// Weder Code noch Fehler in der Antwort
    NoCode,
}

/// Liest `code` aus dem Redirect `starface-app://login?code=…&state=…` und
/// prüft `state`. Parameter im Fragment (`#code=…`) werden ebenfalls
/// gelesen, falls die Anlage `response_mode=fragment` verwendet.
pub fn code_from_redirect(
    redirect: &str,
    expected_state: &str,
) -> std::result::Result<String, RedirectError> {
    let url = Url::parse(redirect).map_err(|_| RedirectError::NoCode)?;
    let fragment = url::form_urlencoded::parse(url.fragment().unwrap_or_default().as_bytes());
    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut description = String::new();
    for (k, v) in url.query_pairs().chain(fragment) {
        match &*k {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            "error" => error = Some(v.into_owned()),
            "error_description" => description = v.into_owned(),
            _ => {}
        }
    }
    if let Some(error) = error {
        return Err(RedirectError::Denied { error, description });
    }
    let code = code.ok_or(RedirectError::NoCode)?;
    if state.as_deref() != Some(expected_state) {
        return Err(RedirectError::StateMismatch);
    }
    Ok(code)
}

/// Rücksprung fürs Protokoll: Code und State geschwärzt
pub fn redacted_redirect(redirect: &str) -> String {
    let Ok(mut url) = Url::parse(redirect) else {
        return "<ungültige URL>".into();
    };
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| {
            let v = if matches!(&*k, "code" | "state" | "session_state") {
                "…".into()
            } else {
                v.into_owned()
            };
            (k.into_owned(), v)
        })
        .collect();
    url.query_pairs_mut().clear().extend_pairs(pairs);
    url.set_fragment(url.fragment().map(|_| "…"));
    url.to_string()
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
        assert_eq!(code_from_redirect(r, "xyz").as_deref(), Ok("abc"));
        assert_eq!(
            code_from_redirect(r, "other"),
            Err(RedirectError::StateMismatch)
        );
        assert_eq!(
            code_from_redirect("starface-app://login?code=abc", "xyz"),
            Err(RedirectError::StateMismatch)
        );
    }

    #[test]
    fn redirect_errors_and_fragment() {
        assert_eq!(
            code_from_redirect(
                "starface-app://login?error=access_denied&error_description=Kein+Recht&state=xyz",
                "xyz"
            ),
            Err(RedirectError::Denied {
                error: "access_denied".into(),
                description: "Kein Recht".into()
            })
        );
        assert_eq!(
            code_from_redirect("starface-app://login?state=xyz", "xyz"),
            Err(RedirectError::NoCode)
        );
        assert_eq!(
            code_from_redirect("starface-app://login#state=xyz&code=abc", "xyz").as_deref(),
            Ok("abc")
        );
        assert_eq!(
            redacted_redirect("starface-app://login?state=s&code=c&iss=x"),
            "starface-app://login?state=%E2%80%A6&code=%E2%80%A6&iss=x"
        );
    }

    #[test]
    fn authorize_url_carries_edge_node_resource() {
        let client = Client {
            http: reqwest::Client::new(),
            discovery: Discovery {
                issuer: "http://pbx/auth/realms/pbx".into(),
                authorization_endpoint: "https://pbx/auth/realms/pbx/oauth2/auth".into(),
                token_endpoint: "https://pbx/auth/realms/pbx/oauth2/token".into(),
                edge_node_id: Some("42".into()),
                revocation_endpoint: None,
            },
            login_config: LoginConfig::default(),
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

    /// Anlagen-Login: Scope pbx-login; zentraler STARFACE-Login: Cloud-Scopes
    #[test]
    fn scope_depends_on_identity_provider() {
        let client = |issuer: &str| Client {
            http: reqwest::Client::new(),
            discovery: Discovery {
                issuer: issuer.into(),
                authorization_endpoint: "https://idp/auth".into(),
                token_endpoint: "https://idp/token".into(),
                edge_node_id: None,
                revocation_endpoint: None,
            },
            login_config: LoginConfig::default(),
        };
        let scope = |issuer: &str| {
            client(issuer)
                .authorize_url(&Pkce::from_verifier("v".repeat(43)), "s")
                .unwrap()
                .query_pairs()
                .find(|(k, _)| k == "scope")
                .map(|(_, v)| v.into_owned())
        };
        assert_eq!(
            scope("http://pbx.example.com/auth/realms/pbx").as_deref(),
            Some(SCOPE)
        );
        assert_eq!(
            scope("https://login.unified-cloud.eu/realms/one-hub").as_deref(),
            Some(CLOUD_SCOPE)
        );
    }

    #[test]
    fn login_config_of_cloud_pbx() {
        let c: LoginConfig = serde_json::from_str(
            r#"{"oAuthFlowRequired":true,"issuer":"https://pbx/auth/realms/pbx",
                "oauthConfigUrl":"https://pbx/.well-known/openid-configuration",
                "gRpcUrl":"grpcs://ucapi.example.eu","edgeNodeId":"aecd"}"#,
        )
        .unwrap();
        assert_eq!(c.edge_node_id.as_deref(), Some("aecd"));
        let client = Client {
            http: reqwest::Client::new(),
            discovery: serde_json::from_str(
                r#"{"authorization_endpoint":"a","token_endpoint":"t"}"#,
            )
            .unwrap(),
            login_config: c,
        };
        assert_eq!(
            client.grpc_endpoint(),
            Some(("ucapi.example.eu".to_owned(), 443))
        );
        let local: LoginConfig =
            serde_json::from_str(r#"{"gRpcUrl":null,"edgeNodeId":null}"#).unwrap();
        assert!(local.grpc_url.is_none() && local.edge_node_id.is_none());
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
