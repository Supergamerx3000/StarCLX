//! Rufliste (Journal der Anlage): Laden, Ereignisse verfolgen, verpasste
//! Anrufe melden.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use sf_onehub::OneHub;
use sf_onehub::sf_proto::v1;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use v1::journal::journal_entry_extension::Extension;

/// So viele Einträge hält die Liste.
const LIMIT: usize = 200;
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub number: String,
    pub incoming: bool,
    pub missed: bool,
    /// Unix-Zeit in Millisekunden
    pub start: i64,
    pub duration_secs: i64,
    /// Bei Gruppenanrufen der Gruppenname
    pub group: String,
    /// Wer den Gruppenanruf angenommen hat
    pub answered_by: String,
    pub comment: String,
    pub called_back: bool,
    pub voicemail: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JournalEvent {
    /// Vollständige Liste, neueste zuerst
    Entries {
        entries: Vec<Entry>,
    },
    /// Ein neu eingetroffener verpasster Anruf (für Benachrichtigungen)
    Missed {
        entry: Entry,
    },
    Error {
        message: String,
    },
}

pub struct Journal {
    hub: OneHub,
    entries: Arc<Mutex<Vec<Entry>>>,
    task: JoinHandle<()>,
}

impl Journal {
    /// Lädt die Rufliste und verfolgt Änderungen im Hintergrund.
    pub fn start(hub: OneHub, events: mpsc::UnboundedSender<JournalEvent>) -> Self {
        let entries = Arc::new(Mutex::new(Vec::new()));
        let task = {
            let hub = hub.clone();
            let entries = entries.clone();
            tokio::spawn(async move {
                let mut backoff = Duration::from_secs(1);
                loop {
                    match session(&hub, &entries, &events).await {
                        Ok(()) => backoff = Duration::from_secs(1),
                        Err(e) => {
                            tracing::warn!(error = %e, "Rufliste unterbrochen");
                            let _ = events.send(JournalEvent::Error {
                                message: e.to_string(),
                            });
                        }
                    }
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
            })
        };
        Self { hub, entries, task }
    }

    pub fn entries(&self) -> Vec<Entry> {
        self.entries.lock().unwrap().clone()
    }

    pub async fn delete(&self, id: &str) -> sf_onehub::Result<()> {
        self.hub
            .journal()
            .delete_journal_entry(v1::journal::DeleteJournalEntryRequest {
                journal_entry_id: Some(entry_id(id)),
            })
            .await?;
        Ok(())
    }

    pub async fn set_called_back(&self, id: &str, called_back: bool) -> sf_onehub::Result<()> {
        self.hub
            .journal()
            .set_journal_entry_called_back(v1::journal::SetCalledBackRequest {
                journal_entry_id: Some(entry_id(id)),
                called_back,
            })
            .await?;
        Ok(())
    }

    pub async fn set_comment(&self, id: &str, comment: &str) -> sf_onehub::Result<()> {
        let mut journal = self.hub.journal();
        if comment.trim().is_empty() {
            journal
                .clear_journal_entry_comment(v1::journal::ClearCommentRequest {
                    journal_entry_id: Some(entry_id(id)),
                })
                .await?;
        } else {
            journal
                .set_journal_entry_comment(v1::journal::SetCommentRequest {
                    journal_entry_id: Some(entry_id(id)),
                    comment: comment.trim().to_owned(),
                })
                .await?;
        }
        Ok(())
    }
}

impl Drop for Journal {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn entry_id(id: &str) -> v1::journal::JournalEntryId {
    v1::journal::JournalEntryId { id: id.to_owned() }
}

async fn session(
    hub: &OneHub,
    entries: &Arc<Mutex<Vec<Entry>>>,
    events: &mpsc::UnboundedSender<JournalEvent>,
) -> sf_onehub::Result<()> {
    let req = v1::journal::GetJournalRequest {
        journal_filter: None,
        journal_entry_order_by: v1::journal::JournalEntryOrderBy::StartTime as i32,
        order_direction: v1::types::OrderDirection::Descending as i32,
        limit: LIMIT as i32,
        offset: 0,
    };
    let list = hub
        .journal()
        .get_journal(req)
        .await?
        .into_inner()
        .journal_entries;
    *entries.lock().unwrap() = list.iter().map(view_of).collect();
    publish(entries, events);

    let mut stream = hub
        .journal()
        .subscribe_journal_events(())
        .await?
        .into_inner();
    while let Some(ev) = stream.message().await? {
        let Some(ev) = ev.journal_event else {
            continue;
        };
        if let Some(missed) = apply(&mut entries.lock().unwrap(), ev) {
            let _ = events.send(JournalEvent::Missed { entry: missed });
        }
        publish(entries, events);
    }
    Ok(())
}

fn publish(entries: &Arc<Mutex<Vec<Entry>>>, events: &mpsc::UnboundedSender<JournalEvent>) {
    let entries = entries.lock().unwrap().clone();
    let _ = events.send(JournalEvent::Entries { entries });
}

/// Wendet ein Ereignis an. Gibt einen neuen verpassten Anruf zurück.
fn apply(
    entries: &mut Vec<Entry>,
    ev: v1::journal::journal_event_response::JournalEvent,
) -> Option<Entry> {
    use v1::journal::journal_event_response::JournalEvent as E;
    let id_of = |id: &Option<v1::journal::JournalEntryId>| {
        id.as_ref().map(|i| i.id.clone()).unwrap_or_default()
    };
    match ev {
        E::JournalEntryCreated(e) => {
            let entry = view_of(&e.journal_entry?);
            entries.retain(|x| x.id != entry.id);
            let pos = entries
                .iter()
                .position(|x| x.start <= entry.start)
                .unwrap_or(entries.len());
            entries.insert(pos, entry.clone());
            entries.truncate(LIMIT);
            (entry.missed && entry.incoming).then_some(entry)
        }
        E::JournalEntryDeleted(e) => {
            let id = id_of(&e.journal_entry_id);
            entries.retain(|x| x.id != id);
            None
        }
        E::JournalEntryCommentUpdated(e) => {
            let id = id_of(&e.journal_entry_id);
            if let Some(x) = entries.iter_mut().find(|x| x.id == id) {
                x.comment = e.comment.map(|c| c.comment).unwrap_or_default();
            }
            None
        }
        E::JournalEntryCalledBackChanged(e) => {
            let id = id_of(&e.journal_entry_id);
            if let Some(x) = entries.iter_mut().find(|x| x.id == id) {
                x.called_back = e.called_back.is_some_and(|c| c.called_back);
            }
            None
        }
        E::JournalEntryVoicemailTranscriptionEvent(_) => None,
    }
}

fn millis(ts: &Option<prost_types::Timestamp>) -> i64 {
    ts.as_ref()
        .map_or(0, |t| t.seconds * 1000 + i64::from(t.nanos) / 1_000_000)
}

fn view_of(e: &v1::journal::JournalEntry) -> Entry {
    let remote = e.remote_participant.clone().unwrap_or_default();
    let mut comment = String::new();
    let mut called_back = false;
    for ext in e.extensions.iter().filter_map(|x| x.extension.as_ref()) {
        match ext {
            Extension::Comment(c) => comment.clone_from(&c.comment),
            Extension::CallBack(c) => called_back = c.called_back,
            _ => {}
        }
    }
    let group = e.group_call_info.clone().unwrap_or_default();
    Entry {
        id: e
            .journal_entry_id
            .as_ref()
            .map(|i| i.id.clone())
            .unwrap_or_default(),
        name: remote.name,
        number: remote.number,
        incoming: e.call_direction == v1::types::CallDirection::Inbound as i32,
        missed: e.answer_type == v1::journal::AnswerType::Missed as i32,
        start: millis(&e.start_time),
        duration_secs: e.duration.as_ref().map_or(0, |d| d.seconds),
        group: group.group_name,
        answered_by: group.group_call_answered_by,
        comment,
        called_back,
        voicemail: e.voicemail_id.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use v1::journal::journal_event_response::JournalEvent as E;

    fn raw(id: &str, secs: i64, missed: bool) -> v1::journal::JournalEntry {
        v1::journal::JournalEntry {
            journal_entry_id: Some(entry_id(id)),
            remote_participant: Some(v1::types::RemoteParticipant {
                number: "12".into(),
                name: "Claude".into(),
                user_id: None,
            }),
            start_time: Some(prost_types::Timestamp {
                seconds: secs,
                nanos: 0,
            }),
            call_direction: v1::types::CallDirection::Inbound as i32,
            answer_type: if missed {
                v1::journal::AnswerType::Missed
            } else {
                v1::journal::AnswerType::Answered
            } as i32,
            ..Default::default()
        }
    }

    fn created(e: v1::journal::JournalEntry) -> E {
        E::JournalEntryCreated(v1::journal::JournalEntryCreatedEvent {
            journal_entry: Some(e),
        })
    }

    #[test]
    fn events_keep_list_sorted_and_report_missed() {
        let mut list = vec![
            view_of(&raw("a", 300, false)),
            view_of(&raw("b", 100, false)),
        ];
        assert_eq!(apply(&mut list, created(raw("c", 200, false))), None);
        let missed = apply(&mut list, created(raw("d", 400, true))).unwrap();
        assert_eq!(missed.id, "d");
        assert_eq!(missed.start, 400_000);
        let ids: Vec<_> = list.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["d", "a", "c", "b"]);

        apply(
            &mut list,
            E::JournalEntryCalledBackChanged(v1::journal::JournalEntryCalledBackChangedEvent {
                journal_entry_id: Some(entry_id("d")),
                called_back: Some(v1::journal::JournalEntryCalledBack {
                    called_back: true,
                    ..Default::default()
                }),
            }),
        );
        assert!(list[0].called_back);

        apply(
            &mut list,
            E::JournalEntryDeleted(v1::journal::JournalEntryDeletedEvent {
                journal_entry_id: Some(entry_id("a")),
            }),
        );
        assert_eq!(list.len(), 3);
    }
}
