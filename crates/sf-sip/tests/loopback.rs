//! Zwei Konten im selben baresip rufen sich über die eigene IP an, mit SRTP
//! als Pflicht, Sinuston als Quelle und ohne echtes Audiogerät.

use std::time::Duration;

use sf_sip::{Config, SipEvent, Softphone};
use tokio::sync::mpsc::UnboundedReceiver;

async fn next(
    rx: &mut UnboundedReceiver<SipEvent>,
    what: &str,
    pred: impl Fn(&SipEvent) -> bool,
) -> SipEvent {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let ev = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("Zeitüberschreitung beim Warten auf {what}"))
            .expect("Kanal geschlossen");
        eprintln!("{ev:?}");
        if pred(&ev) {
            return ev;
        }
    }
}

#[tokio::test]
async fn call_between_two_local_accounts_with_srtp() {
    // baresip nutzt keine Loopback-Adressen; daher die eigene Netzadresse.
    let probe = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
    probe.connect("192.0.2.1:9").unwrap();
    let ip = probe.local_addr().unwrap().ip();
    let port = std::net::UdpSocket::bind((ip, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config = Config {
        audio_player: "aubridge,loop".into(),
        audio_source: "ausine,440".into(),
        sip_listen: Some(format!("{ip}:{port}")),
        verify_server: false,
        ca_file: None,
        extra: "ausrc_srate 16000\nausrc_channels 1\n".into(),
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

    phone
        .connect(&alice, &format!("sip:bob@{ip}:{port}"))
        .unwrap();

    let SipEvent::Incoming { call, .. } = next(&mut rx, "eingehenden Anruf", |e| {
        matches!(e, SipEvent::Incoming { .. })
    })
    .await
    else {
        unreachable!()
    };
    assert!(call.peer_uri.contains("alice"), "{call:?}");
    phone.answer(&call.call_id).unwrap();

    // SRTP-Aushandlung und Verbindungsaufbau kommen in beliebiger Reihenfolge.
    let (mut srtp, mut established) = (false, false);
    while !(srtp && established) {
        match next(&mut rx, "SRTP und Verbindung", |e| {
            matches!(
                e,
                SipEvent::MediaEncryption { .. } | SipEvent::Established { .. }
            )
        })
        .await
        {
            SipEvent::MediaEncryption { info, .. } => srtp |= info.contains("AES_CM_128_HMAC_SHA1"),
            SipEvent::Established { call_id } => established |= call_id == call.call_id,
            _ => unreachable!(),
        }
    }

    // Ansage aus einer WAV-Datei: meldet das Dateiende.
    let wav = std::env::temp_dir().join(format!("sf-sip-test-{}.wav", std::process::id()));
    std::fs::write(&wav, silent_wav(16000, 300)).unwrap();
    phone
        .set_source(&call.call_id, "aufile", wav.to_str().unwrap())
        .unwrap();
    next(&mut rx, "Dateiende", |e| {
        matches!(e, SipEvent::EndOfFile { .. })
    })
    .await;
    let _ = std::fs::remove_file(&wav);

    phone.set_mute(&call.call_id, true).unwrap();
    phone.send_dtmf(&call.call_id, "12#").unwrap();
    // Die Gegenseite (im selben Prozess) empfängt die Tasten.
    let SipEvent::Dtmf { key, .. } =
        next(&mut rx, "Taste", |e| matches!(e, SipEvent::Dtmf { .. })).await
    else {
        unreachable!()
    };
    assert_eq!(key, '1');
    phone.hangup(Some(&call.call_id)).unwrap();
    next(&mut rx, "Auflegen", |e| {
        matches!(e, SipEvent::Closed { .. })
    })
    .await;

    drop(phone);
    // Nach dem Beenden lässt sich baresip erneut starten.
    let (again, _rx) = Softphone::start(&config, "sf-sip-test").unwrap();
    drop(again);
}

/// WAV-Datei mit Stille: 16 bit, mono
fn silent_wav(rate: u32, millis: u32) -> Vec<u8> {
    let data_len = rate * millis / 1000 * 2;
    let mut w = Vec::new();
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    w.resize(w.len() + data_len as usize, 0);
    w
}
