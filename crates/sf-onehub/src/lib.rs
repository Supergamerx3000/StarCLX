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
            .tls_config(ClientTlsConfig::new().with_native_roots().domain_name(host))?
            .user_agent(concat!("starface-linuxclient/", env!("CARGO_PKG_VERSION")))?
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

    pub async fn server_version(&self) -> Result<String> {
        Ok(self
            .system()
            .get_server_version(())
            .await?
            .into_inner()
            .version)
    }
}
