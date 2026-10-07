//! Benutzereinstellungen, die auf der Anlage liegen: signalisierte Rufnummer
//! und primäres Telefon.

use std::time::Duration;

use serde::Serialize;
use sf_onehub::OneHub;
use sf_onehub::sf_proto::v1;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// ID, unter der die Anlage „Rufnummer unterdrücken“ führt (auch in den
/// Funktionstasten „Rufnummer anzeigen“)
pub const SUPPRESSED_ID: &str = "0";

/// Eine Rufnummer, die bei ausgehenden Anrufen gezeigt werden kann.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SignalingNumber {
    pub id: String,
    /// Lesbare Nummer, leer bei unterdrückter Nummer
    pub number: String,
    /// Diese Wahl unterdrückt die Nummer
    pub suppressed: bool,
    pub read_only: bool,
    pub selected: bool,
    /// Gruppe, zu der die Nummer gehört (z. B. „Zentrale“), sonst leer
    pub group: String,
}

/// Formatiert eine Nummer der Anlage, z. B. `+41 71 7271616` oder `12`.
pub fn format_number(n: &v1::types::PhoneNumber) -> String {
    use v1::types::phone_number::Number;
    match &n.number {
        Some(Number::InternationalNumber(i)) => [
            format!("+{}", i.country_code.trim_start_matches('+')),
            i.national_destination_code.clone(),
            i.subscriber_number.clone(),
        ]
        .into_iter()
        .filter(|p| !p.is_empty() && p != "+")
        .collect::<Vec<_>>()
        .join(" "),
        Some(Number::InternalNumber(i)) => i.extension.clone(),
        None => String::new(),
    }
}

/// Wählbare signalisierte Rufnummern; die aktuelle ist `selected`.
pub async fn signaling_numbers(hub: &OneHub) -> sf_onehub::Result<Vec<SignalingNumber>> {
    let mut me = hub.me();
    let list = me
        .get_signaling_phone_numbers(())
        .await?
        .into_inner()
        .signaling_phone_numbers;
    tracing::debug!(?list, "Signalisierte Rufnummern");
    let config = me.get_phone_numbers_config(()).await?.into_inner();
    tracing::debug!(?config, "Rufnummern-Einstellung");
    // Unterdrückt: keine signalisierte Nummer, eine unsichtbare oder die 0
    let suppressed_now = config.signaling_phone_number.as_ref().is_none_or(|s| {
        !s.visible
            || s.phone_number
                .as_ref()
                .and_then(|n| n.phone_number_id.as_ref())
                .is_none_or(|i| i.id == SUPPRESSED_ID)
    });
    let current = config
        .signaling_phone_number
        .and_then(|s| s.phone_number)
        .and_then(|n| n.phone_number_id)
        .map(|id| id.id)
        .filter(|_| !suppressed_now);
    // Gruppennamen wie in der STARFACE-App („Zentrale: +49 …“)
    let mut groups = std::collections::HashMap::new();
    let mut svc = hub.group();
    for g in list.iter().filter_map(|s| s.group_id.as_ref()) {
        if groups.contains_key(&g.id) {
            continue;
        }
        let name = svc
            .get_group(v1::sfpbx::group::GetGroupRequest {
                group_id: Some(g.clone()),
            })
            .await
            .ok()
            .and_then(|r| r.into_inner().group)
            .map(|g| g.name)
            .unwrap_or_default();
        groups.insert(g.id.clone(), name);
    }
    let mut out: Vec<SignalingNumber> = list
        .into_iter()
        .filter_map(|s| {
            let n = s.phone_number?;
            let id = n.phone_number_id.as_ref()?.id.clone();
            let number = format_number(&n);
            Some(SignalingNumber {
                selected: current.as_deref() == Some(id.as_str()),
                suppressed: number.is_empty(),
                id,
                number,
                read_only: s.read_only,
                group: s
                    .group_id
                    .and_then(|g| groups.get(&g.id).cloned())
                    .unwrap_or_default(),
            })
        })
        .collect();
    // Eigene Nummern zuerst, dann die der Gruppen
    out.sort_by_key(|n| (n.suppressed, !n.group.is_empty()));
    // Wie in der STARFACE-App als eigene Wahl am Ende; die Anlage führt dafür
    // keinen Eintrag in der Liste, gewählt wird es mit der ID 0.
    if !out.iter().any(|n| n.suppressed) {
        out.push(SignalingNumber {
            id: SUPPRESSED_ID.into(),
            number: String::new(),
            suppressed: true,
            read_only: false,
            selected: suppressed_now,
            group: String::new(),
        });
    }
    Ok(out)
}

/// Wählt die signalisierte Nummer; [`SUPPRESSED_ID`] unterdrückt sie.
pub async fn set_signaling_number(hub: &OneHub, id: &str) -> sf_onehub::Result<()> {
    hub.me()
        .set_signaling_phone_number(v1::me::SetSignalingPhoneNumberRequest {
            phone_number_id: Some(v1::types::PhoneNumberId { id: id.to_owned() }),
        })
        .await?;
    Ok(())
}

/// Änderung an den eigenen Einstellungen, auch von anderen Clients
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeChange {
    /// Signalisierte Nummer oder ihre Sichtbarkeit
    Signaling,
    /// Primäres Telefon, Telefon dazu oder weg
    Phones,
    Avatar,
    /// Ein Recht wurde erteilt oder entzogen
    Permission,
}

/// Verfolgt die eigenen Einstellungen (MeService-Ereignisse).
pub struct MeEvents(JoinHandle<()>);

impl MeEvents {
    pub fn start(hub: OneHub, changes: mpsc::UnboundedSender<MeChange>) -> Self {
        Self(tokio::spawn(crate::reconnect::forever(
            "Eigene Ereignisse",
            Duration::from_secs(30),
            move || {
                let (hub, changes) = (hub.clone(), changes.clone());
                async move { watch_me(&hub, &changes).await }
            },
        )))
    }
}

impl Drop for MeEvents {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn watch_me(
    hub: &OneHub,
    changes: &mpsc::UnboundedSender<MeChange>,
) -> sf_onehub::Result<()> {
    use v1::me::me_event_response::MeEvents as E;
    let mut stream = hub.me().subscribe_me_events(()).await?.into_inner();
    while let Some(ev) = stream.message().await? {
        tracing::debug!(?ev, "Eigenes Ereignis");
        let change = match ev.me_events {
            Some(
                E::SignalingPhoneNumberChanged(_) | E::SignalingPhoneNumberVisibilityChanged(_),
            ) => MeChange::Signaling,
            Some(E::PrimaryPhoneChanged(_) | E::PhoneAdded(_) | E::PhoneRemoved(_)) => {
                MeChange::Phones
            }
            Some(E::AvatarChanged(_)) => MeChange::Avatar,
            Some(E::PermissionChanged(_)) => MeChange::Permission,
            None => continue,
        };
        let _ = changes.send(change);
    }
    Ok(())
}

/// Ein Telefon des Benutzers (Tischtelefon, Softphones der Apps)
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PhoneView {
    pub id: String,
    pub name: String,
    pub primary: bool,
}

/// Eigene Telefone, das primäre markiert
pub async fn phones(hub: &OneHub) -> sf_onehub::Result<Vec<PhoneView>> {
    let mut me = hub.me();
    let list = me.get_phones(()).await?.into_inner().phones;
    let primary = me
        .get_primary_phone(())
        .await?
        .into_inner()
        .phone
        .and_then(|p| p.phone_id)
        .map(|i| i.id)
        .unwrap_or_default();
    Ok(list
        .into_iter()
        .filter_map(|p| {
            let id = p.phone_id?.id;
            Some(PhoneView {
                primary: id == primary,
                id,
                name: p.name,
            })
        })
        .collect())
}

pub async fn set_primary_phone(hub: &OneHub, phone_id: &str) -> sf_onehub::Result<()> {
    hub.me()
        .set_primary_phone(v1::me::SetPrimaryPhoneRequest {
            phone_id: Some(v1::types::PhoneId {
                id: phone_id.to_owned(),
            }),
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use v1::types::phone_number::Number;

    #[test]
    fn formats_numbers() {
        let intl = v1::types::PhoneNumber {
            phone_number_id: None,
            is_fax: false,
            number: Some(Number::InternationalNumber(
                v1::types::InternationalNumber {
                    country_code: "41".into(),
                    national_destination_code: "71".into(),
                    subscriber_number: "7271616".into(),
                },
            )),
        };
        assert_eq!(format_number(&intl), "+41 71 7271616");
        let int = v1::types::PhoneNumber {
            number: Some(Number::InternalNumber(v1::types::InternalNumber {
                extension: "12".into(),
            })),
            ..intl.clone()
        };
        assert_eq!(format_number(&int), "12");
        let none = v1::types::PhoneNumber {
            number: None,
            ..intl
        };
        assert_eq!(format_number(&none), "");
    }
}
