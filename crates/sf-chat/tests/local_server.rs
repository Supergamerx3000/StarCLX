//! Gegen einen lokalen XMPP-Server mit STARTTLS (z. B. Prosody), weil die
//! Testanlage nicht immer erreichbar ist:
//! `SF_CHAT_TEST_HOST=pbx.test cargo test -p sf-chat -- --ignored`
//! Erwartet die Konten alice und bob mit den Passwörtern token-a und token-b.

use std::time::Duration;

use sf_chat::{Chat, ChatEvent};
use tokio::sync::mpsc::UnboundedReceiver;

async fn wait(
    rx: &mut UnboundedReceiver<ChatEvent>,
    what: &str,
    pred: impl Fn(&ChatEvent) -> bool,
) -> ChatEvent {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let ev = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .unwrap_or_else(|_| panic!("Zeitüberschreitung: {what}"))
            .expect("Kanal zu");
        eprintln!("{what}: {ev:?}");
        if pred(&ev) {
            return ev;
        }
    }
}

#[tokio::test]
#[ignore = "braucht einen lokalen XMPP-Server"]
async fn two_users_exchange_messages() {
    let host = std::env::var("SF_CHAT_TEST_HOST").expect("SF_CHAT_TEST_HOST");
    let (atx, mut arx) = tokio::sync::mpsc::unbounded_channel();
    let (btx, mut brx) = tokio::sync::mpsc::unbounded_channel();
    let alice = Chat::start(
        &format!("alice@{host}"),
        &host,
        || "token-a".into(),
        None,
        atx,
    )
    .unwrap();
    let bob = Chat::start(
        &format!("bob@{host}"),
        &host,
        || "token-b".into(),
        None,
        btx,
    )
    .unwrap();
    let online = |e: &ChatEvent| matches!(e, ChatEvent::State { online: true, .. });
    wait(&mut arx, "alice online", online).await;
    wait(&mut brx, "bob online", online).await;

    bob.send(&format!("alice@{host}"), "Hoi Alice");
    let ChatEvent::Message { message, notify } = wait(&mut arx, "alice empfängt", |e| {
        matches!(e, ChatEvent::Message { .. })
    })
    .await
    else {
        unreachable!()
    };
    assert!(notify);
    assert!(!message.outgoing);
    assert_eq!(message.peer, format!("bob@{host}"));
    assert_eq!(message.body, "Hoi Alice");

    alice.send(&format!("bob@{host}"), "Grüezi Bob");
    let ChatEvent::Message { message, .. } = wait(
        &mut brx,
        "bob empfängt",
        |e| matches!(e, ChatEvent::Message { message, .. } if !message.outgoing),
    )
    .await
    else {
        unreachable!()
    };
    assert_eq!(message.body, "Grüezi Bob");

    // Verlauf bei alice: empfangen und gesendet
    let conv = alice.conversation(&format!("bob@{host}"));
    assert_eq!(conv.len(), 2, "{conv:?}");
    assert!(conv[1].outgoing);
    assert_eq!(alice.recent()[0].body, "Grüezi Bob");
}

#[tokio::test]
#[ignore = "braucht einen lokalen XMPP-Server"]
async fn file_goes_from_bob_to_alice() {
    let host = std::env::var("SF_CHAT_TEST_HOST").expect("SF_CHAT_TEST_HOST");
    let dir = std::env::temp_dir().join(format!("sf-chat-server-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("Bericht.pdf");
    let data: Vec<u8> = (0..300_000u32).map(|i| (i * 13) as u8).collect();
    std::fs::write(&src, &data).unwrap();

    let (atx, mut arx) = tokio::sync::mpsc::unbounded_channel();
    let (btx, mut brx) = tokio::sync::mpsc::unbounded_channel();
    let alice = Chat::start(
        &format!("alice@{host}"),
        &host,
        || "token-a".into(),
        None,
        atx,
    )
    .unwrap();
    let bob = Chat::start(
        &format!("bob@{host}"),
        &host,
        || "token-b".into(),
        None,
        btx,
    )
    .unwrap();
    let online = |e: &ChatEvent| matches!(e, ChatEvent::State { online: true, .. });
    wait(&mut arx, "alice online", online).await;
    wait(&mut brx, "bob online", online).await;
    // Bob muss Alices Client kennen, der Dateien annimmt
    wait(&mut brx, "bob sieht alice", |e| {
        matches!(e, ChatEvent::Roster { contacts } if contacts.iter().any(|c| c.jid.starts_with("alice@") && c.clients.iter().any(|k| k.files == Some(true))))
    })
    .await;

    bob.send_file(&format!("alice@{host}"), src);
    let ChatEvent::Transfer { transfer } = wait(&mut arx, "alice bekommt Angebot", |e| {
        matches!(e, ChatEvent::Transfer { transfer } if transfer.state == sf_chat::TransferState::Offered)
    })
    .await
    else {
        unreachable!()
    };
    alice.accept_file(&transfer.id, dir.join("in"));
    let ChatEvent::Transfer { transfer } = wait(
        &mut arx,
        "alice fertig",
        |e| matches!(e, ChatEvent::Transfer { transfer } if !transfer.state.active()),
    )
    .await
    else {
        unreachable!()
    };
    assert_eq!(transfer.state, sf_chat::TransferState::Done, "{transfer:?}");
    assert_eq!(std::fs::read(&transfer.path).unwrap(), data);
    wait(&mut brx, "bob fertig", |e| {
        matches!(e, ChatEvent::Transfer { transfer } if transfer.state == sf_chat::TransferState::Done)
    })
    .await;
    // Sauber abmelden, sonst hält der Server die Sitzungen noch eine Weile
    alice.shutdown("").await;
    bob.shutdown("").await;
    let _ = std::fs::remove_dir_all(dir);
}
