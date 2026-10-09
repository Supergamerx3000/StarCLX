//! Geplante Konferenzen der Anlage (Konferenzraum mit Teilnehmern, Termin
//! und Wiederholung): Liste, Anlegen, Ändern, Löschen, Starten und
//! Ereignisse für Änderungen.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use sf_onehub::OneHub;
use sf_onehub::sf_proto::v1;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use v1::types::{ConferenceState as State, Recurrence};

const LIMIT: i32 = 200;
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Participant {
    /// Leer bei neuen Teilnehmern
    #[serde(default)]
    pub id: String,
    /// Gesetzt, wenn der Teilnehmer ein Benutzer der Anlage ist
    #[serde(default)]
    pub user_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub number: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub moderator: bool,
    /// Die Anlage ruft ihn beim Start der Konferenz an
    #[serde(default)]
    pub call_on_start: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Conference {
    pub id: String,
    pub name: String,
    /// Der Benutzer darf sie ändern und starten
    pub moderator: bool,
    /// "planned", "active" oder "concluded"
    pub state: &'static str,
    /// Unix-Zeit in Millisekunden
    pub start: i64,
    /// "once", "daily", "weekly" oder "monthly"
    pub recurrence: &'static str,
    pub participants: Vec<Participant>,
}

/// Neue oder geänderte Konferenz aus der Oberfläche
#[derive(Debug, Clone, Deserialize)]
pub struct Draft {
    /// `None` legt eine neue an
    pub id: Option<String>,
    pub name: String,
    /// Unix-Zeit in Millisekunden
    pub start: i64,
    pub recurrence: String,
    pub participants: Vec<Participant>,
}

#[derive(Debug, Clone, Serialize)]
pub enum ConferenceEvent {
    /// Etwas hat sich geändert; neu laden
    Changed,
}

fn state_name(s: i32) -> &'static str {
    match State::try_from(s) {
        Ok(State::Active) => "active",
        Ok(State::Concluded) => "concluded",
        _ => "planned",
    }
}

fn recurrence_name(r: i32) -> &'static str {
    match Recurrence::try_from(r) {
        Ok(Recurrence::Daily) => "daily",
        Ok(Recurrence::Weekly) => "weekly",
        Ok(Recurrence::Monthly) => "monthly",
        _ => "once",
    }
}

pub fn recurrence_of(name: &str) -> Option<Recurrence> {
    match name {
        "once" => Some(Recurrence::Once),
        "daily" => Some(Recurrence::Daily),
        "weekly" => Some(Recurrence::Weekly),
        "monthly" => Some(Recurrence::Monthly),
        _ => None,
    }
}

fn millis(t: Option<prost_types::Timestamp>) -> i64 {
    t.map_or(0, |t| t.seconds * 1000 + i64::from(t.nanos) / 1_000_000)
}

fn timestamp(ms: i64) -> prost_types::Timestamp {
    prost_types::Timestamp {
        seconds: ms.div_euclid(1000),
        nanos: (ms.rem_euclid(1000) * 1_000_000) as i32,
    }
}

fn participant_view(p: v1::types::ConferenceParticipant) -> Participant {
    Participant {
        id: p.participant_id.map(|i| i.id).unwrap_or_default(),
        user_id: p.user_id.map(|u| u.id).filter(|u| !u.is_empty()),
        name: p.display_name,
        number: p.phone_number,
        email: p.email_address,
        moderator: p.is_moderator,
        call_on_start: p.call_on_start,
    }
}

fn participant_proto(p: Participant) -> v1::types::ConferenceParticipant {
    v1::types::ConferenceParticipant {
        participant_id: (!p.id.is_empty())
            .then_some(v1::types::ConferenceParticipantId { id: p.id }),
        user_id: p
            .user_id
            .filter(|u| !u.is_empty())
            .map(|id| v1::types::UserId { id }),
        display_name: p.name.trim().to_owned(),
        phone_number: p.number.trim().to_owned(),
        is_moderator: p.moderator,
        call_on_start: p.call_on_start,
        email_address: p.email.trim().to_owned(),
    }
}

fn view_of(c: v1::types::Conference) -> Option<Conference> {
    Some(Conference {
        id: c.conference_id?.id,
        name: c.name,
        moderator: c.is_moderator,
        state: state_name(c.conference_state),
        start: millis(c.start_time),
        recurrence: recurrence_name(c.recurrence),
        participants: c.participants.into_iter().map(participant_view).collect(),
    })
}

fn conference_id(id: &str) -> Option<v1::types::ConferenceId> {
    Some(v1::types::ConferenceId { id: id.to_owned() })
}

/// Eigene Konferenzen, nach Beginn sortiert.
pub async fn list(hub: &OneHub) -> sf_onehub::Result<Vec<Conference>> {
    let list = hub
        .conference()
        .get_conferences(v1::conference::GetConferencesRequest {
            limit: LIMIT,
            offset: 0,
            order_by: v1::conference::ConferenceOrderBy::StartTime as i32,
            order_direction: v1::types::OrderDirection::Ascending as i32,
        })
        .await?
        .into_inner()
        .conferences;
    Ok(list.into_iter().filter_map(view_of).collect())
}

/// Ungültige Eingaben, bevor sie zur Anlage gehen
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    Name,
    Recurrence,
    /// Ein Teilnehmer ohne Benutzer, Nummer und E-Mail
    Participant,
}

/// Legt eine Konferenz an oder ändert sie.
pub async fn save(hub: &OneHub, draft: Draft) -> Result<(), SaveError> {
    let name = draft.name.trim().to_owned();
    if name.is_empty() {
        return Err(SaveError::Invalid(Invalid::Name));
    }
    let recurrence =
        recurrence_of(&draft.recurrence).ok_or(SaveError::Invalid(Invalid::Recurrence))? as i32;
    if draft.participants.iter().any(|p| {
        p.user_id.as_deref().is_none_or(str::is_empty)
            && p.number.trim().is_empty()
            && p.email.trim().is_empty()
    }) {
        return Err(SaveError::Invalid(Invalid::Participant));
    }
    let mut participants = draft.participants;
    for p in &mut participants {
        complete(hub, p).await;
    }
    let participants = participants.into_iter().map(participant_proto).collect();
    let start_time = Some(timestamp(draft.start));
    let mut svc = hub.conference();
    match draft.id.filter(|id| !id.is_empty()) {
        Some(id) => {
            svc.update_conference(v1::conference::UpdateConferenceRequest {
                conference_id: conference_id(&id),
                name,
                start_time,
                recurrence,
                conference_participants: participants,
            })
            .await
        }
        None => {
            svc.create_conference(v1::conference::CreateConferenceRequest {
                name,
                start_time,
                recurrence,
                conference_participants: participants,
            })
            .await
        }
    }
    .map_err(|s| SaveError::Hub(s.into()))?;
    Ok(())
}

/// Ergänzt bei Benutzern der Anlage interne Nummer und E-Mail-Adresse. Ohne
/// Nummer bricht die Anlage das Anlegen mit „Internal error“ ab; die
/// Windows-App schickt sie deshalb immer mit.
async fn complete(hub: &OneHub, p: &mut Participant) {
    let Some(user_id) = p.user_id.clone().filter(|u| !u.is_empty()) else {
        return;
    };
    if !p.number.trim().is_empty() && !p.email.trim().is_empty() {
        return;
    }
    let user = match hub
        .user()
        .get_user(v1::types::GetUserRequest {
            user_id: Some(v1::types::UserId { id: user_id }),
        })
        .await
    {
        Ok(r) => r.into_inner().user.unwrap_or_default(),
        Err(e) => {
            tracing::warn!(error = %e, "Benutzer für Konferenzteilnehmer nicht gelesen");
            return;
        }
    };
    if p.number.trim().is_empty() {
        p.number = internal_number(&user);
    }
    if p.email.trim().is_empty() {
        p.email = user.email;
    }
}

/// Interne Hauptnummer des Benutzers, sonst seine erste interne Nummer
fn internal_number(user: &v1::types::User) -> String {
    use v1::types::phone_number::Number;
    let internal =
        |n: &&v1::types::PhoneNumber| matches!(n.number, Some(Number::InternalNumber(_)));
    let primary = user
        .primary_internal_phone_number_id
        .as_ref()
        .and_then(|id| {
            user.phone_numbers
                .iter()
                .find(|n| n.phone_number_id.as_ref() == Some(id))
        });
    primary
        .or_else(|| user.phone_numbers.iter().find(internal))
        .map(crate::account::format_number)
        .unwrap_or_default()
}

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("ungültige Eingabe: {0:?}")]
    Invalid(Invalid),
    #[error(transparent)]
    Hub(#[from] sf_onehub::Error),
}

pub async fn delete(hub: &OneHub, id: &str) -> sf_onehub::Result<()> {
    hub.conference()
        .delete_conference(v1::conference::DeleteConferenceRequest {
            conference_id: conference_id(id),
        })
        .await?;
    Ok(())
}

/// Startet die Konferenz jetzt. Die Anlage holt `phone_id` hinein und ruft
/// die Teilnehmer mit „beim Start anrufen“ an.
pub async fn start(hub: &OneHub, id: &str, phone_id: &str) -> sf_onehub::Result<()> {
    hub.conference()
        .start_conference(v1::conference::StartConferenceRequest {
            conference_id: conference_id(id),
            phone_id: Some(v1::types::PhoneId {
                id: phone_id.to_owned(),
            }),
        })
        .await?;
    Ok(())
}

/// Verfolgt Konferenz-Ereignisse, bis es gedroppt wird.
pub struct Watcher(JoinHandle<()>);

impl Watcher {
    pub fn start(hub: OneHub, events: mpsc::UnboundedSender<ConferenceEvent>) -> Self {
        Self(tokio::spawn(crate::reconnect::forever(
            "Konferenz-Ereignisse",
            MAX_BACKOFF,
            move || {
                let (hub, events) = (hub.clone(), events.clone());
                async move { watch(&hub, &events).await }
            },
        )))
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn watch(
    hub: &OneHub,
    events: &mpsc::UnboundedSender<ConferenceEvent>,
) -> sf_onehub::Result<()> {
    let mut stream = hub
        .conference()
        .subscribe_conference_events(())
        .await?
        .into_inner();
    // Nach einem Neuverbinden kann etwas verpasst worden sein
    let _ = events.send(ConferenceEvent::Changed);
    while stream.message().await?.is_some() {
        let _ = events.send(ConferenceEvent::Changed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(state_name(State::Active as i32), "active");
        assert_eq!(state_name(0), "planned");
        assert_eq!(recurrence_name(Recurrence::Weekly as i32), "weekly");
        assert_eq!(recurrence_name(0), "once");
        assert_eq!(recurrence_of("monthly"), Some(Recurrence::Monthly));
        assert_eq!(recurrence_of("yearly"), None);
    }

    #[test]
    fn time_round_trip() {
        for ms in [0, 1_760_000_000_123, -1] {
            assert_eq!(millis(Some(timestamp(ms))), ms);
        }
    }

    #[test]
    fn participant_round_trip() {
        let p = Participant {
            id: String::new(),
            user_id: Some("42".into()),
            name: " Star Claude ".into(),
            number: "12".into(),
            email: String::new(),
            moderator: true,
            call_on_start: true,
        };
        let proto = participant_proto(p);
        assert!(proto.participant_id.is_none());
        assert_eq!(proto.display_name, "Star Claude");
        let back = participant_view(proto);
        assert_eq!(back.user_id.as_deref(), Some("42"));
        assert!(back.moderator && back.call_on_start);
    }

    #[test]
    fn primary_internal_number_wins() {
        use v1::types::phone_number::Number;
        let num = |id: &str, n: Number| v1::types::PhoneNumber {
            phone_number_id: Some(v1::types::PhoneNumberId { id: id.into() }),
            is_fax: false,
            number: Some(n),
        };
        let internal = |e: &str| {
            Number::InternalNumber(v1::types::InternalNumber {
                extension: e.into(),
            })
        };
        let mut user = v1::types::User {
            phone_numbers: vec![
                num(
                    "1",
                    Number::InternationalNumber(v1::types::InternationalNumber {
                        country_code: "41".into(),
                        national_destination_code: "71".into(),
                        subscriber_number: "7271616".into(),
                    }),
                ),
                num("2", internal("11")),
                num("3", internal("12")),
            ],
            ..Default::default()
        };
        assert_eq!(internal_number(&user), "11");
        user.primary_internal_phone_number_id = Some(v1::types::PhoneNumberId { id: "3".into() });
        assert_eq!(internal_number(&user), "12");
        assert_eq!(internal_number(&v1::types::User::default()), "");
    }

    #[test]
    fn view() {
        let c = view_of(v1::types::Conference {
            conference_id: conference_id("5"),
            name: "Teamrunde".into(),
            is_moderator: true,
            conference_state: State::Planned as i32,
            start_time: Some(timestamp(60_000)),
            recurrence: Recurrence::Weekly as i32,
            participants: vec![v1::types::ConferenceParticipant {
                display_name: "Extern".into(),
                phone_number: "+41441234567".into(),
                ..Default::default()
            }],
        })
        .unwrap();
        assert_eq!(
            (c.id.as_str(), c.state, c.recurrence, c.start),
            ("5", "planned", "weekly", 60_000)
        );
        assert_eq!(c.participants[0].user_id, None);
    }
}
