//! Verbindung zur OneHub-gRPC-API der STARFACE (`https://<anlage>:9092`).
//!
//! Jeder Aufruf trägt `Authorization: Bearer <access-token>`. Das Token wird
//! über [`TokenHandle`] gesetzt und kann nach einem Refresh ausgetauscht
//! werden, ohne die Verbindung neu aufzubauen.

use std::sync::{Arc, RwLock};

use sf_proto::v1;
use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tonic::{Request, Status};

pub use sf_proto;

pub const DEFAULT_PORT: u16 = 9092;

/// Softphone-ID für RegisterSipDevice. Die Anlage leitet daraus den
/// Telefontyp ab (Enum `StarfaceType` im Anlagen-Code) und legt ein
/// passendes App-Telefon an. Bekannte IDs: Windows
/// 163C00A2-C2F1-4FFE-9474-49283C379852, Mac
/// E22FF5B3-38ED-4A5E-966A-80B4785A2FF7, iPhone
/// FFFDF5C8-E74F-4B18-B258-4B66C3FFFFAB, Android
/// 244C5E4C-4011-4F76-A7E1-52D40F399C6B. Hier: "UCC Client for Linux".
pub const SIP_DEVICE_ID: &str = "D4CC1516-EC90-42C7-8F70-B0853B173232";

fn phone_matches_sip_user(phone_name: &str, sip_user: &str) -> bool {
    phone_name.strip_prefix("SIP/").unwrap_or(phone_name) == sip_user
}

/// Antwort von `SipDeviceService.RegisterSipDevice`.
#[derive(Clone)]
pub struct SipCredentials {
    pub user: String,
    pub password: String,
    pub realm: String,
    pub port: u16,
}

impl std::fmt::Debug for SipCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SipCredentials")
            .field("user", &self.user)
            .field("password", &"***")
            .field("realm", &self.realm)
            .field("port", &self.port)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Verbindung fehlgeschlagen: {0}")]
    Transport(#[from] tonic::transport::Error),
    #[error("gRPC-Fehler: {0}")]
    Status(#[from] Status),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Gemeinsam genutztes Access-Token.
#[derive(Clone, Default)]
pub struct TokenHandle(Arc<RwLock<String>>);

impl TokenHandle {
    pub fn new(token: impl Into<String>) -> Self {
        Self(Arc::new(RwLock::new(token.into())))
    }

    pub fn set(&self, token: impl Into<String>) {
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = token.into();
    }

    pub fn get(&self) -> String {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[derive(Clone)]
pub struct BearerInterceptor(TokenHandle);

impl Interceptor for BearerInterceptor {
    fn call(&mut self, mut req: Request<()>) -> std::result::Result<Request<()>, Status> {
        let value = MetadataValue::try_from(format!("Bearer {}", self.0.get()))
            .map_err(|_| Status::unauthenticated("Token enthält ungültige Zeichen"))?;
        req.metadata_mut().insert("authorization", value);
        Ok(req)
    }
}

type Svc = tonic::service::interceptor::InterceptedService<Channel, BearerInterceptor>;

/// Eine Verbindung zur Anlage. Günstig zu klonen; alle Clients teilen sich
/// denselben HTTP/2-Kanal.
#[derive(Clone)]
pub struct OneHub {
    channel: Channel,
    token: TokenHandle,
}

macro_rules! service {
    ($name:ident, $($client:ident)::+) => {
        pub fn $name(&self) -> $($client)::+<Svc> {
            $($client)::+::with_interceptor(self.channel.clone(), self.interceptor())
        }
    };
}

impl OneHub {
    /// `host` ohne Schema, z. B. `pbx.example.com`.
    pub async fn connect(host: &str, port: u16, token: TokenHandle) -> Result<Self> {
        let channel = Endpoint::from_shared(format!("https://{host}:{port}"))?
            .tls_config_with_verifier(ClientTlsConfig::new().domain_name(host), sf_tls::verifier())?
            .user_agent(concat!("starclx/", env!("CARGO_PKG_VERSION")))?
            .connect()
            .await?;
        Ok(Self { channel, token })
    }

    pub fn token(&self) -> &TokenHandle {
        &self.token
    }

    fn interceptor(&self) -> BearerInterceptor {
        BearerInterceptor(self.token.clone())
    }

    service!(
        health,
        v1::health::health_service_client::HealthServiceClient
    );
    service!(
        system,
        v1::system::system_service_client::SystemServiceClient
    );
    service!(me, v1::me::me_service_client::MeServiceClient);
    service!(call, v1::call::call_service_client::CallServiceClient);
    service!(
        sip_device,
        v1::sipdevice::sip_device_service_client::SipDeviceServiceClient
    );
    service!(chat, v1::chat::chat_service_client::ChatServiceClient);
    service!(
        conference_call,
        v1::conference::conference_call_service_client::ConferenceCallServiceClient
    );
    service!(
        voicemail,
        v1::voicemail::voice_mail_service_client::VoiceMailServiceClient
    );
    service!(
        contact,
        v1::contact::contact_service_client::ContactServiceClient
    );
    service!(
        simple_contact,
        v1::contact::phonenumber::simple_contact_service_client::SimpleContactServiceClient
    );
    service!(
        journal,
        v1::journal::journal_service_client::JournalServiceClient
    );
    service!(
        user_id_lookup,
        v1::useridlookup::user_id_lookup_service_client::UserIdLookupServiceClient
    );
    service!(
        presence,
        v1::presence::presence_service_client::PresenceServiceClient
    );
    service!(
        redirect,
        v1::redirect::redirect_service_client::RedirectServiceClient
    );
    service!(
        fmc_phone,
        v1::fmcphone::fmc_phone_service_client::FmcPhoneServiceClient
    );

    /// Holt die SIP-Zugangsdaten für das App-Telefon zu `device_id` (siehe
    /// [`SIP_DEVICE_ID`]). Legt das Telefon bei Bedarf auf der Anlage an.
    pub async fn register_sip_device(
        &self,
        device_id: &str,
        app_version: &str,
    ) -> Result<SipCredentials> {
        let req = v1::sipdevice::RegisterSipDeviceRequest {
            sip_device_id: Some(v1::types::SipDeviceId {
                id: device_id.into(),
            }),
            app_version: app_version.into(),
        };
        let cfg = self
            .sip_device()
            .register_sip_device(req)
            .await?
            .into_inner()
            .sip_device_config
            .ok_or_else(|| Status::internal("RegisterSipDevice ohne Konfiguration"))?;
        Ok(SipCredentials {
            user: cfg.sip_user_id.map(|u| u.id).unwrap_or_default(),
            password: cfg.password,
            realm: cfg.realm,
            port: u16::try_from(cfg.port)
                .map_err(|_| Status::internal(format!("ungültiger SIP-Port {}", cfg.port)))?,
        })
    }

    /// Telefon-ID des App-Telefons, das zu einem SIP-Benutzer gehört. Die
    /// Anlage nennt es `SIP/<sip_user>`.
    pub async fn phone_id_for_sip_user(&self, sip_user: &str) -> Result<Option<String>> {
        let phones = self.me().get_phones(()).await?.into_inner().phones;
        Ok(phones
            .into_iter()
            .find(|p| phone_matches_sip_user(&p.name, sip_user))
            .and_then(|p| p.phone_id)
            .map(|id| id.id))
    }

    /// Jabber-ID eines Benutzers für den Chat (XMPP)
    pub async fn chat_jid(&self, user_id: &str) -> Result<String> {
        let res = self
            .chat()
            .get_chat_id(v1::chat::GetChatIdRequest {
                user_id: Some(v1::types::UserId {
                    id: user_id.to_owned(),
                }),
            })
            .await?
            .into_inner();
        Ok(res.chat_id.map(|c| c.id).unwrap_or_default())
    }

    pub async fn server_version(&self) -> Result<String> {
        Ok(self
            .system()
            .get_server_version(())
            .await?
            .into_inner()
            .version)
    }
}

#[cfg(test)]
mod tests {
    use super::phone_matches_sip_user;

    #[test]
    fn app_phone_name_carries_sip_prefix() {
        assert!(phone_matches_sip_user(
            "SIP/1006.WinClient",
            "1006.WinClient"
        ));
        assert!(phone_matches_sip_user("1006.WinClient", "1006.WinClient"));
        assert!(!phone_matches_sip_user(
            "SIP/1004.WinClient",
            "1006.WinClient"
        ));
    }
}
