//! Gruppen der Anlage: Mitgliedschaften abfragen, an- und abmelden und den
//! Anmeldestatus verfolgen (Funktionstaste „Gruppe An-/Abmelden“).

use std::time::Duration;

use serde::Serialize;
use sf_onehub::OneHub;
use sf_onehub::sf_proto::v1;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use v1::sfpbx::group as g;

const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// Gruppe, in der der Benutzer Mitglied ist
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Membership {
    /// OneHub-ID der Gruppe
    pub id: String,
    pub name: String,
    /// Kennung der Gruppe in der Anlage (z. B. die Konto-ID)
    pub logon_id: String,
    pub logged_on: bool,
    pub read_only: bool,
}

fn group_id(id: &str) -> Option<v1::types::GroupId> {
    Some(v1::types::GroupId { id: id.to_owned() })
}

/// Mitgliedschaften mit Anmeldestatus; `logon_id` wird je Gruppe ergänzt.
pub async fn memberships(hub: &OneHub) -> sf_onehub::Result<Vec<Membership>> {
    let mut svc = hub.group();
    let list = svc
        .get_group_memberships(())
        .await?
        .into_inner()
        .group_memberships;
    let mut out = Vec::with_capacity(list.len());
    for m in list {
        let id = m.group_id.map(|g| g.id).unwrap_or_default();
        let logon_id = match svc
            .get_group(g::GetGroupRequest {
                group_id: group_id(&id),
            })
            .await
        {
            Ok(r) => r.into_inner().group.map(|g| g.logon_id).unwrap_or_default(),
            Err(e) => {
                tracing::debug!(error = %e, %id, "Gruppe nicht gelesen");
                String::new()
            }
        };
        out.push(Membership {
            id,
            name: m.name,
            logon_id,
            logged_on: m.logged_on,
            read_only: m.read_only,
        });
    }
    Ok(out)
}

/// An- (`on = true`) oder abmelden
pub async fn set_logged_on(hub: &OneHub, id: &str, on: bool) -> sf_onehub::Result<()> {
    let mut svc = hub.group();
    if on {
        svc.log_on_to_group(g::LogOnToGroupRequest {
            group_id: group_id(id),
        })
        .await?;
    } else {
        svc.log_off_from_group(g::LogOffFromGroupRequest {
            group_id: group_id(id),
        })
        .await?;
    }
    Ok(())
}

/// Hält die Mitgliedschaften aktuell; jede Änderung schickt den vollen Stand.
pub struct Groups(JoinHandle<()>);

impl Groups {
    pub fn start(hub: OneHub, updates: mpsc::UnboundedSender<Vec<Membership>>) -> Self {
        Self(tokio::spawn(crate::reconnect::forever(
            "Gruppen-Ereignisse",
            MAX_BACKOFF,
            move || {
                let (hub, updates) = (hub.clone(), updates.clone());
                async move { watch(&hub, &updates).await }
            },
        )))
    }
}

impl Drop for Groups {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn watch(
    hub: &OneHub,
    updates: &mpsc::UnboundedSender<Vec<Membership>>,
) -> sf_onehub::Result<()> {
    // Erst abonnieren, dann lesen: so geht keine Änderung dazwischen verloren.
    let mut stream = hub.group().subscribe_group_events(()).await?.into_inner();
    let mut list = memberships(hub).await?;
    let _ = updates.send(list.clone());
    while let Some(ev) = stream.message().await? {
        use g::group_event_response::GroupEvents as E;
        let set = |list: &mut Vec<Membership>, id: Option<v1::types::GroupId>, on: bool| {
            let id = id.map(|g| g.id).unwrap_or_default();
            for m in list.iter_mut().filter(|m| m.id == id) {
                m.logged_on = on;
            }
        };
        match ev.group_events {
            Some(E::LoggedOn(e)) => set(&mut list, e.group_id, true),
            Some(E::LoggedOff(e)) => set(&mut list, e.group_id, false),
            Some(E::GroupNameChanged(e)) => {
                let id = e.group_id.map(|g| g.id).unwrap_or_default();
                for m in list.iter_mut().filter(|m| m.id == id) {
                    m.name.clone_from(&e.name);
                }
            }
            Some(E::VisibilityChanged(e)) => {
                let id = e.group_id.map(|g| g.id).unwrap_or_default();
                for m in list.iter_mut().filter(|m| m.id == id) {
                    m.read_only = e.is_read_only;
                }
            }
            // Mitgliedschaft dazu oder weg: neu lesen
            Some(E::AddedToGroup(_) | E::RemovedFromGroup(_)) => list = memberships(hub).await?,
            None => continue,
        }
        let _ = updates.send(list.clone());
    }
    Ok(())
}

/// Passt eine Gruppe zur ID aus der Funktionstaste? Die Taste speichert die
/// Konto-ID der Anlage; je nach Version steht sie in der OneHub-ID oder in
/// `logon_id`.
pub fn matches(m: &Membership, key_group_id: i32) -> bool {
    let k = key_group_id.to_string();
    m.id == k || m.logon_id == k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_ids_match_onehub_or_logon_id() {
        let m = Membership {
            id: "abc".into(),
            logon_id: "4711".into(),
            ..Default::default()
        };
        assert!(matches(&m, 4711));
        assert!(!matches(&m, 12));
        let n = Membership {
            id: "12".into(),
            ..Default::default()
        };
        assert!(matches(&n, 12));
    }
}
