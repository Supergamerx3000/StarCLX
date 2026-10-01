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
