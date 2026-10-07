//! Module der Anlage (z. B. Zeitsteuerungen) für die Funktionstaste „Modul
//! aktivieren“: Liste mit Zustand, an- und abschalten, Änderungen verfolgen.

use std::time::Duration;

use serde::Serialize;
use sf_onehub::OneHub;
use sf_onehub::sf_proto::v1;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use v1::module as m;

const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Ein Modul mit Zustand
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Module {
    pub id: String,
    pub name: String,
    pub active: bool,
    pub read_only: bool,
}

fn module_of(x: m::Module) -> Option<Module> {
    Some(Module {
        id: x.module_id?.id,
        name: x.name,
        active: x.active,
        read_only: x.read_only,
    })
}

/// Alle Module, die der Benutzer sehen darf
pub async fn modules(hub: &OneHub) -> sf_onehub::Result<Vec<Module>> {
    Ok(hub
        .module()
        .get_modules(())
        .await?
        .into_inner()
        .modules
        .into_iter()
        .filter(|x| x.visible)
        .filter_map(module_of)
        .collect())
}

/// Schaltet ein Modul an oder ab.
pub async fn set_active(hub: &OneHub, id: &str, active: bool) -> sf_onehub::Result<()> {
    let module_id = Some(m::ModuleId { id: id.to_owned() });
    let mut svc = hub.module();
    if active {
        svc.activate_module(m::ActivateModuleRequest { module_id })
            .await?;
    } else {
        svc.deactivate_module(m::DeactivateModuleRequest { module_id })
            .await?;
    }
    Ok(())
}

/// Verfolgt die Module; jede Änderung schickt die vollständige Liste.
pub struct Modules(JoinHandle<()>);

impl Modules {
    pub fn start(hub: OneHub, updates: mpsc::UnboundedSender<Vec<Module>>) -> Self {
        Self(tokio::spawn(crate::reconnect::forever(
            "Modul-Ereignisse",
            MAX_BACKOFF,
            move || {
                let (hub, updates) = (hub.clone(), updates.clone());
                async move { watch(&hub, &updates).await }
            },
        )))
    }
}

impl Drop for Modules {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn watch(
    hub: &OneHub,
    updates: &mpsc::UnboundedSender<Vec<Module>>,
) -> sf_onehub::Result<()> {
    // Den Ereignis-Stream nebenher öffnen und nicht auf ihn warten: die
    // Anlage antwortet darauf womöglich erst mit dem ersten Ereignis (wie bei
    // der Präsenz). Die Liste soll sofort erscheinen.
    let mut svc = hub.module();
    let events = tokio::spawn(async move { svc.subscribe_module_events(()).await });
    tokio::task::yield_now().await;
    let mut list = modules(hub).await?;
    let _ = updates.send(list.clone());
    let mut stream = match events.await {
        Ok(r) => r?.into_inner(),
        Err(e) => {
            tracing::warn!(error = %e, "Modul-Ereignisse nicht abonniert");
            return Ok(());
        }
    };
    while let Some(ev) = stream.message().await? {
        use m::module_event_response::ModuleEvent as E;
        match ev.module_event {
            Some(E::ModuleChanged(c)) => {
                for x in c.modules.into_iter().filter_map(module_of) {
                    match list.iter_mut().find(|y| y.id == x.id) {
                        Some(y) => *y = x,
                        None => list.push(x),
                    }
                }
            }
            Some(E::ModuleDeleted(d)) => {
                let id = d.module_id.map(|i| i.id).unwrap_or_default();
                list.retain(|y| y.id != id);
            }
            // Sichtbarkeit geändert: neu lesen
            Some(E::VisibilityChanged(_)) => list = modules(hub).await?,
            None => continue,
        }
        let _ = updates.send(list.clone());
    }
    Ok(())
}
