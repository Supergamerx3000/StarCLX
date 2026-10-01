//! Voicemail-Nachrichten auf der Anlage: Liste, Herunterladen, Verschieben,
//! Löschen und Ereignisse für neue Nachrichten.

use std::time::Duration;

use serde::Serialize;
use sf_onehub::OneHub;
use sf_onehub::sf_proto::v1;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use v1::types::VoicemailFolder as Folder;

const LIMIT: i32 = 200;
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Voicemail {
    pub id: String,
    /// "inbox" (neu), "old" oder "private"
    pub folder: &'static str,
    pub name: String,
    pub number: String,
    pub mailbox: String,
    /// Unix-Zeit in Millisekunden
    pub start: i64,
    pub duration_secs: i64,
    pub group: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VoicemailEvent {
    /// Etwas hat sich geändert; neu laden
    Changed,
    /// Eine neue Nachricht (für die Benachrichtigung)
    Created { voicemail: Voicemail },
}

fn folder_name(f: i32) -> &'static str {
    match Folder::try_from(f) {
        Ok(Folder::Old) => "old",
        Ok(Folder::Private) => "private",
        _ => "inbox",
    }
}

pub fn folder_of(name: &str) -> Option<Folder> {
    match name {
        "inbox" => Some(Folder::Inbox),
        "old" => Some(Folder::Old),
        "private" => Some(Folder::Private),
        _ => None,
    }
}

fn view_of(v: v1::types::Voicemail) -> Option<Voicemail> {
    let remote = v.remote_participant.unwrap_or_default();
    Some(Voicemail {
        id: v.voicemail_id?.id,
        folder: folder_name(v.voicemail_folder),
        name: remote.name,
        number: remote.number,
        mailbox: v.mailbox.map(|m| m.name).unwrap_or_default(),
        start: v
            .start_time
            .map_or(0, |t| t.seconds * 1000 + i64::from(t.nanos) / 1_000_000),
        duration_secs: v.duration.map_or(0, |d| d.seconds),
        group: v.group_info.is_some(),
    })
}

fn voicemail_id(id: &str) -> Option<v1::types::VoicemailId> {
    Some(v1::types::VoicemailId { id: id.to_owned() })
}

/// Alle Nachrichten aller eigenen Boxen, neueste zuerst.
pub async fn list(hub: &OneHub) -> sf_onehub::Result<Vec<Voicemail>> {
    let list = hub
        .voicemail()
        .get_voicemails(v1::voicemail::GetVoicemailsRequest {
            order_by: v1::voicemail::VoicemailOrderBy::StartTime as i32,
            order_direction: v1::types::OrderDirection::Descending as i32,
            limit: LIMIT,
            ..Default::default()
        })
        .await?
        .into_inner()
        .voicemails;
    Ok(list.into_iter().filter_map(view_of).collect())
}

/// Lädt die Aufnahme (WAV). Markiert die Nachricht nicht als gehört.
pub async fn download(hub: &OneHub, id: &str) -> sf_onehub::Result<Vec<u8>> {
    let mut stream = hub
        .voicemail()
        .download_voicemail_file(v1::voicemail::DownloadVoicemailRequest {
            voicemail_id: voicemail_id(id),
        })
        .await?
        .into_inner();
    let mut data = Vec::new();
    while let Some(chunk) = stream.message().await? {
        if let Some(c) = chunk.file_chunk {
            data.extend_from_slice(&c.data);
        }
    }
    Ok(data)
}

pub async fn move_to(hub: &OneHub, id: &str, folder: Folder) -> sf_onehub::Result<()> {
    hub.voicemail()
        .move_voicemail(v1::voicemail::MoveVoicemailRequest {
            voicemail_id: voicemail_id(id),
            destination_folder: folder as i32,
        })
        .await?;
    Ok(())
}

pub async fn delete(hub: &OneHub, id: &str) -> sf_onehub::Result<()> {
    hub.voicemail()
        .delete_voicemail(v1::voicemail::DeleteVoicemailRequest {
            voicemail_id: voicemail_id(id),
        })
        .await?;
    Ok(())
}

/// Die Anlage ruft `phone_id` an und spielt die Nachricht ab.
pub async fn play_via_phone(hub: &OneHub, id: &str, phone_id: &str) -> sf_onehub::Result<()> {
    hub.voicemail()
        .call_voicemail_via_phone(v1::voicemail::CallVoicemailViaPhoneRequest {
            voicemail_id: voicemail_id(id),
            phone_id: Some(v1::types::PhoneId {
                id: phone_id.to_owned(),
            }),
        })
        .await?;
    Ok(())
}

/// Verfolgt Voicemail-Ereignisse, bis es gedroppt wird.
pub struct Watcher(JoinHandle<()>);

impl Watcher {
    pub fn start(hub: OneHub, events: mpsc::UnboundedSender<VoicemailEvent>) -> Self {
        Self(tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            loop {
                match watch(&hub, &events).await {
                    Ok(()) => backoff = Duration::from_secs(1),
                    Err(e) => tracing::warn!(error = %e, "Voicemail-Ereignisse unterbrochen"),
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }))
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn watch(
    hub: &OneHub,
    events: &mpsc::UnboundedSender<VoicemailEvent>,
) -> sf_onehub::Result<()> {
    use v1::voicemail::voicemail_event_response::VoicemailEvent as E;
    let mut stream = hub
        .voicemail()
        .subscribe_voicemail_events(())
        .await?
        .into_inner();
    // Nach einem Neuverbinden kann etwas verpasst worden sein
    let _ = events.send(VoicemailEvent::Changed);
    while let Some(ev) = stream.message().await? {
        if let Some(E::VoicemailCreated(c)) = ev.voicemail_event
            && let Some(v) = c.voicemail.and_then(view_of)
        {
            let _ = events.send(VoicemailEvent::Created { voicemail: v });
        }
        let _ = events.send(VoicemailEvent::Changed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders() {
        assert_eq!(folder_name(Folder::Inbox as i32), "inbox");
        assert_eq!(folder_name(Folder::Old as i32), "old");
        assert_eq!(folder_name(0), "inbox");
        assert_eq!(folder_of("private"), Some(Folder::Private));
        assert_eq!(folder_of("x"), None);
    }

    #[test]
    fn view() {
        let v = view_of(v1::types::Voicemail {
            voicemail_id: Some(v1::types::VoicemailId { id: "7".into() }),
            voicemail_folder: Folder::Inbox as i32,
            remote_participant: Some(v1::types::RemoteParticipant {
                number: "12".into(),
                name: "Star2 Claude2".into(),
                user_id: None,
            }),
            duration: Some(prost_types::Duration {
                seconds: 6,
                nanos: 0,
            }),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            (v.number.as_str(), v.duration_secs, v.folder),
            ("12", 6, "inbox")
        );
    }
}
