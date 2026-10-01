//! STARTTLS wie in `tokio_xmpp::connect::StartTlsServerConnector`, aber mit
//! der Zertifikatsprüfung aus `sf_tls`, damit vom Benutzer bestätigte
//! Zertifikate lokaler Anlagen auch für den Chat gelten.

use std::borrow::Cow;
use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use sasl::common::ChannelBinding;
use tokio::io::BufStream;
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_xmpp::connect::{DnsConfig, ServerConnector};
use tokio_xmpp::error::{Error, ProtocolError};
use tokio_xmpp::jid::Jid;
use tokio_xmpp::parsers::starttls;
use tokio_xmpp::rustls::ProtocolVersion;
use tokio_xmpp::rustls::pki_types::ServerName;
use tokio_xmpp::xmlstream::{
    PendingFeaturesRecv, ReadError, StreamHeader, Timeouts, XmppStream, XmppStreamElement,
    initiate_stream,
};

#[derive(Debug, Clone)]
pub struct StartTls(pub DnsConfig);

impl ServerConnector for StartTls {
    type Stream = BufStream<TlsStream<TcpStream>>;

    async fn connect(
        &self,
        jid: &Jid,
        ns: &'static str,
        timeouts: Timeouts,
    ) -> Result<(PendingFeaturesRecv<Self::Stream>, ChannelBinding), Error> {
        let header = || StreamHeader {
            to: Some(Cow::Borrowed(jid.domain().as_str())),
            from: None,
            id: None,
        };
        let tcp = BufStream::new(self.0.resolve().await?);
        let stream = initiate_stream(tcp, ns, header(), timeouts).await?;
        let (features, mut stream): (_, XmppStream<_>) = stream.recv_features().await?;
        if !features.can_starttls() {
            return Err(Error::Protocol(ProtocolError::NoTls));
        }
        stream
            .send(&XmppStreamElement::Starttls(starttls::Nonza::Request(
                starttls::Request,
            )))
            .await?;
        loop {
            match stream
                .next()
                .await
                .map(|v| v.and_then(|e| e.into_read_error()))
            {
                Some(Ok(XmppStreamElement::Starttls(starttls::Nonza::Proceed(_)))) => break,
                Some(Ok(_)) | Some(Err(ReadError::SoftTimeout)) => {}
                Some(Err(ReadError::HardError(e))) => return Err(e.into()),
                Some(Err(ReadError::ParseError(e))) => {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e).into());
                }
                None | Some(Err(ReadError::StreamFooterReceived)) => {
                    return Err(Error::Disconnected);
                }
            }
        }
        let tcp = stream.into_inner().into_inner();
        let name = ServerName::try_from(jid.domain().as_str().to_owned())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let tls = tokio_rustls::TlsConnector::from(Arc::new(sf_tls::client_config()))
            .connect(name, tcp)
            .await?;
        let (_, conn) = tls.get_ref();
        let binding = match conn.protocol_version() {
            Some(ProtocolVersion::TLSv1_3) => conn
                .export_keying_material(vec![0u8; 32], b"EXPORTER-Channel-Binding", None)
                .map(ChannelBinding::TlsExporter)
                .unwrap_or(ChannelBinding::None),
            _ => ChannelBinding::None,
        };
        let stream = initiate_stream(BufStream::new(tls), ns, header(), timeouts).await?;
        Ok((stream, binding))
    }
}
