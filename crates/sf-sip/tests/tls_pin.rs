//! SIP über TLS mit einem Zertifikat, das die normale Prüfung nicht besteht
//! (selbstsigniert, auf einen anderen Namen): ohne Bestätigung kein Anruf,
//! mit bestätigtem Fingerabdruck schon.

use std::time::Duration;

use sf_sip::{Config, SipEvent, Softphone};
use tokio::sync::mpsc::UnboundedReceiver;

/// Wartet auf einen eingehenden Anruf; `false` nach Fehler oder Zeitablauf.
async fn incoming(rx: &mut UnboundedReceiver<SipEvent>) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(SipEvent::Incoming { .. })) => return true,
            Ok(Some(SipEvent::Closed { .. } | SipEvent::Error { .. })) | Err(_) | Ok(None) => {
                return false;
            }
            Ok(Some(ev)) => eprintln!("{ev:?}"),
        }
    }
}

async fn call_over_tls(ip: std::net::IpAddr, cert: &std::path::Path, pins: Vec<String>) -> bool {
    let port = std::net::UdpSocket::bind((ip, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config = Config {
        audio_player: "aubridge,loop".into(),
        audio_source: "ausine,440".into(),
        sip_listen: Some(format!("{ip}:{port}")),
        verify_server: true,
        ca_file: None,
        trusted_fingerprints: pins,
        extra: format!(
            "sip_certificate {}\nausrc_srate 16000\nausrc_channels 1\n",
            cert.display()
        ),
    };
    let (phone, mut rx) = Softphone::start(&config, "sf-sip-test").unwrap();
    let alice = format!("sip:alice@{ip}");
    phone
        .add_aor(&format!(
            "<{alice}>;regint=0;mediaenc=srtp-mand;answermode=manual"
        ))
        .unwrap();
    phone
        .add_aor(&format!(
            "<sip:bob@{ip}>;regint=0;mediaenc=srtp-mand;answermode=manual"
        ))
        .unwrap();
    // baresip lauscht für TLS auf dem Port nach dem von sip_listen.
    phone
        .connect(&alice, &format!("sip:bob@{ip}:{};transport=tls", port + 1))
        .unwrap();
    let got = incoming(&mut rx).await;
    let _ = phone.hangup(None);
    got
}

#[tokio::test]
async fn untrusted_certificate_only_with_confirmed_fingerprint() {
    let probe = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
    probe.connect("192.0.2.1:9").unwrap();
    let ip = probe.local_addr().unwrap().ip();

    let cert = rcgen::generate_simple_self_signed(vec!["pbx.invalid".into()]).unwrap();
    let fingerprint = sf_tls::fingerprint(cert.cert.der());
    let pem = std::env::temp_dir().join(format!("sf-sip-pin-{}.pem", std::process::id()));
    std::fs::write(
        &pem,
        format!("{}{}", cert.cert.pem(), cert.signing_key.serialize_pem()),
    )
    .unwrap();

    let without = call_over_tls(ip, &pem, Vec::new()).await;
    let other = call_over_tls(ip, &pem, vec![sf_tls::fingerprint(b"anderes")]).await;
    let with = call_over_tls(ip, &pem, vec![fingerprint.to_lowercase()]).await;
    let _ = std::fs::remove_file(&pem);

    assert!(!without, "ungeprüftes Zertifikat angenommen");
    assert!(!other, "falscher Fingerabdruck angenommen");
    assert!(with, "bestätigtes Zertifikat abgelehnt");
}
