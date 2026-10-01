//! Erreichbarkeit: Umleitungen, Parallelruf (iFMC) und die Voicemail-Ansage.
//! Alles liegt auf der Anlage; Änderungen wirken sofort.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use sf_onehub::OneHub;
use sf_onehub::sf_proto::v1;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use v1::types::redirect_target::RedirectTarget as Target;

const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Mailbox {
    pub id: String,
    pub name: String,
}

/// Ziel einer Umleitung: genau eins von beiden ist gesetzt.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RedirectTarget {
    pub number: Option<String>,
    pub mailbox: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Redirect {
    pub id: String,
    /// "always", "busy" oder "timeout"
    pub kind: &'static str,
    /// Umgeleitete Rufnummer (eigene oder Gruppennummer)
    pub called_number: String,
    pub group: bool,
    pub enabled: bool,
    pub target: RedirectTarget,
    /// Wählbare Voicemail-Boxen
    pub mailboxes: Vec<Mailbox>,
    /// Nur bei "timeout": Sekunden bis zur Umleitung
    pub timeout_secs: i64,
    /// Zuletzt benutzte Rufnummer (Vorschlag beim Umschalten)
    pub last_number: String,
    pub read_only: bool,
}

fn mailbox(m: v1::types::Mailbox) -> Option<Mailbox> {
    Some(Mailbox {
        id: m.mailbox_id?.id,
        name: m.name,
    })
}

fn target_of(t: Option<v1::types::RedirectTarget>) -> RedirectTarget {
    match t.and_then(|t| t.redirect_target) {
        Some(Target::PhoneNumber(n)) => RedirectTarget {
            number: Some(n),
            mailbox: None,
        },
        Some(Target::Mailbox(m)) => RedirectTarget {
            number: None,
            mailbox: m.mailbox_id.map(|i| i.id),
        },
        None => RedirectTarget::default(),
    }
}

fn kind_of(t: i32) -> &'static str {
    match v1::types::RedirectType::try_from(t) {
        Ok(v1::types::RedirectType::Always) => "always",
        Ok(v1::types::RedirectType::Busy) => "busy",
        Ok(v1::types::RedirectType::Timeout) => "timeout",
        _ => "other",
    }
}

fn view_of(r: v1::redirect::RedirectResponse) -> Option<Redirect> {
    let read_only = r.read_only;
    let visible = r.visible;
    let r = r.redirect?;
    if !visible {
        return None;
    }
    Some(Redirect {
        id: r.redirect_id?.id,
        kind: kind_of(r.redirect_type),
        called_number: r.called_number,
        group: r.group_id.is_some_and(|g| !g.id.is_empty()),
        enabled: r.enabled,
        target: target_of(r.redirect_target),
        mailboxes: r.mailboxes.into_iter().filter_map(mailbox).collect(),
        timeout_secs: r.timeout.map_or(0, |d| d.seconds),
        last_number: r.last_destination_number,
        read_only,
    })
}

/// Sortiert: eigene vor Gruppen, dann nach Nummer und Immer/Besetzt/Zeit.
fn order(list: &mut [Redirect]) {
    let rank = |k: &str| match k {
        "always" => 0,
        "busy" => 1,
        "timeout" => 2,
        _ => 3,
    };
    list.sort_by(|a, b| {
        (a.group, &a.called_number, rank(a.kind)).cmp(&(b.group, &b.called_number, rank(b.kind)))
    });
}

pub async fn redirects(hub: &OneHub) -> sf_onehub::Result<Vec<Redirect>> {
    let list = hub
        .redirect()
        .get_redirects(v1::redirect::GetRedirectsRequest::default())
        .await?
        .into_inner()
        .redirects;
    let mut list: Vec<Redirect> = list.into_iter().filter_map(view_of).collect();
    order(&mut list);
    Ok(list)
}

fn redirect_id(id: &str) -> Option<v1::types::RedirectId> {
    Some(v1::types::RedirectId { id: id.to_owned() })
}

pub async fn set_redirect_enabled(hub: &OneHub, id: &str, enabled: bool) -> sf_onehub::Result<()> {
    let mut svc = hub.redirect();
    if enabled {
        svc.enable_redirect(v1::redirect::EnableRedirectRequest {
            redirect_id: redirect_id(id),
        })
        .await?;
    } else {
        svc.disable_redirect(v1::redirect::DisableRedirectRequest {
            redirect_id: redirect_id(id),
        })
        .await?;
    }
    Ok(())
}

/// Ändert Ziel und (bei Zeitüberschreitung) die Wartezeit.
pub async fn update_redirect(
    hub: &OneHub,
    id: &str,
    target: &RedirectTarget,
    timeout_secs: Option<i64>,
) -> sf_onehub::Result<()> {
    let target = match (&target.mailbox, &target.number) {
        (Some(m), _) => Target::Mailbox(v1::types::Mailbox {
            mailbox_id: Some(v1::types::MailboxId { id: m.clone() }),
            name: String::new(),
        }),
        (None, n) => Target::PhoneNumber(n.clone().unwrap_or_default().trim().to_owned()),
    };
    hub.redirect()
        .update_redirect(v1::redirect::UpdateRedirectRequest {
            redirect_id: redirect_id(id),
            redirect_target: Some(v1::types::RedirectTarget {
                redirect_target: Some(target),
            }),
            timeout: timeout_secs.map(|s| prost_types::Duration {
                seconds: s,
                nanos: 0,
            }),
        })
        .await?;
    Ok(())
}

/// Zeitfenster eines Parallelrufs, Zeiten als "HH:MM"
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Schedule {
    /// 1 = Montag … 7 = Sonntag
    pub days: Vec<i32>,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FmcPhone {
    /// Leer beim Anlegen
    #[serde(default)]
    pub id: String,
    pub number: String,
    /// Sekunden, bis das Gerät mitklingelt
    pub delay: i32,
    #[serde(default)]
    pub enabled: bool,
    /// Annahme per Tastendruck bestätigen
    pub confirm: bool,
    #[serde(default)]
    pub schedules: Vec<Schedule>,
}

fn hhmm(t: Option<v1::types::Time>) -> String {
    let t = t.unwrap_or_default();
    format!("{:02}:{:02}", t.hours, t.minutes)
}

fn time(s: &str) -> Option<v1::types::Time> {
    let (h, m) = s.split_once(':')?;
    Some(v1::types::Time {
        hours: h.trim().parse().ok().filter(|h| (0..24).contains(h))?,
        minutes: m.trim().parse().ok().filter(|m| (0..60).contains(m))?,
        seconds: 0,
    })
}

fn schedule_of(s: v1::fmcphone::FmcPhoneSchedule) -> Schedule {
    Schedule {
        days: s.weekday,
        from: hhmm(s.from),
        to: hhmm(s.to),
    }
}

fn schedules_to(list: &[Schedule]) -> Vec<v1::fmcphone::FmcPhoneSchedule> {
    list.iter()
        .map(|s| v1::fmcphone::FmcPhoneSchedule {
            weekday: s.days.clone(),
            from: time(&s.from),
            to: time(&s.to),
        })
        .collect()
}

fn fmc_view(p: v1::fmcphone::FmcPhone) -> Option<FmcPhone> {
    Some(FmcPhone {
        id: p.fmc_phone_id?.id,
        number: p.number,
        delay: p.delay,
        enabled: p.enabled,
        confirm: p.confirm_by_keypress,
        schedules: p.fmc_phone_schedules.into_iter().map(schedule_of).collect(),
    })
}

fn fmc_id(id: &str) -> Option<v1::fmcphone::FmcPhoneId> {
    Some(v1::fmcphone::FmcPhoneId { id: id.to_owned() })
}

pub async fn fmc_phones(hub: &OneHub) -> sf_onehub::Result<Vec<FmcPhone>> {
    Ok(hub
        .fmc_phone()
        .get_fmc_phones(())
        .await?
        .into_inner()
        .fmc_phones
        .into_iter()
        .filter_map(fmc_view)
        .collect())
}

/// Legt ein Gerät an (ohne `id`) oder ändert es.
pub async fn save_fmc_phone(hub: &OneHub, p: &FmcPhone) -> sf_onehub::Result<()> {
    let mut svc = hub.fmc_phone();
    if p.id.is_empty() {
        svc.create_fmc_phone(v1::fmcphone::CreateFmcPhoneRequest {
            fmc_phone: Some(v1::fmcphone::FmcPhone {
                fmc_phone_id: None,
                number: p.number.trim().to_owned(),
                delay: p.delay,
                enabled: true,
                confirm_by_keypress: p.confirm,
                fmc_phone_schedules: schedules_to(&p.schedules),
            }),
        })
        .await?;
    } else {
        svc.update_fmc_phone(v1::fmcphone::UpdateFmcPhoneRequest {
            fmc_phone_id: fmc_id(&p.id),
            number: p.number.trim().to_owned(),
            delay: p.delay,
            confirm_by_keypress: p.confirm,
            fmc_phone_schedules: schedules_to(&p.schedules),
        })
        .await?;
    }
    Ok(())
}

pub async fn set_fmc_enabled(hub: &OneHub, id: &str, enabled: bool) -> sf_onehub::Result<()> {
    let mut svc = hub.fmc_phone();
    if enabled {
        svc.enable_fmc_phone(v1::fmcphone::EnableFmcPhoneRequest {
            fmc_phone_id: fmc_id(id),
        })
        .await?;
    } else {
        svc.disable_fmc_phone(v1::fmcphone::DisableFmcPhoneRequest {
            fmc_phone_id: fmc_id(id),
        })
        .await?;
    }
    Ok(())
}

pub async fn delete_fmc_phone(hub: &OneHub, id: &str) -> sf_onehub::Result<()> {
    hub.fmc_phone()
        .delete_fmc_phone(v1::fmcphone::DeleteFmcPhoneRequest {
            fmc_phone_id: fmc_id(id),
        })
        .await?;
    Ok(())
}

/// Eigene Voicemail-Boxen
pub async fn mailboxes(hub: &OneHub) -> sf_onehub::Result<Vec<Mailbox>> {
    Ok(hub
        .voicemail()
        .get_mailboxes(())
        .await?
        .into_inner()
        .mailboxes
        .into_iter()
        .filter_map(mailbox)
        .collect())
}

/// Die Anlage ruft `phone_id` an und verbindet mit dem Menü der Box; dort
/// nimmt man die Ansagen auf (0 = Abwesenheit, 1 = Begrüssung, 3 = Name …).
pub async fn call_mailbox(hub: &OneHub, mailbox_id: &str, phone_id: &str) -> sf_onehub::Result<()> {
    hub.voicemail()
        .call_mailbox_via_phone(v1::voicemail::CallMailboxViaPhoneRequest {
            mailbox_id: Some(v1::types::MailboxId {
                id: mailbox_id.to_owned(),
            }),
            phone_id: Some(v1::types::PhoneId {
                id: phone_id.to_owned(),
            }),
        })
        .await?;
    Ok(())
}

/// Meldet jede Änderung an Umleitungen oder Parallelruf, damit die
/// Oberfläche neu lädt. Läuft bis zum Drop.
pub struct Watcher {
    tasks: Vec<JoinHandle<()>>,
}

impl Watcher {
    pub fn start(hub: OneHub, changed: mpsc::UnboundedSender<()>) -> Self {
        let redirects = {
            let (hub, changed) = (hub.clone(), changed.clone());
            tokio::spawn(retry("Umleitungen", move || {
                let (hub, changed) = (hub.clone(), changed.clone());
                async move {
                    let mut s = hub
                        .redirect()
                        .subscribe_redirect_events(())
                        .await?
                        .into_inner();
                    while s.message().await?.is_some() {
                        let _ = changed.send(());
                    }
                    Ok(())
                }
            }))
        };
        let fmc = tokio::spawn(retry("Parallelruf", move || {
            let (hub, changed) = (hub.clone(), changed.clone());
            async move {
                let mut s = hub
                    .fmc_phone()
                    .subscribe_fmc_phone_events(())
                    .await?
                    .into_inner();
                while s.message().await?.is_some() {
                    let _ = changed.send(());
                }
                Ok(())
            }
        }));
        Self {
            tasks: vec![redirects, fmc],
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.tasks.iter().for_each(JoinHandle::abort);
    }
}

async fn retry<F, Fut>(what: &'static str, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = sf_onehub::Result<()>>,
{
    let mut backoff = Duration::from_secs(1);
    loop {
        match f().await {
            Ok(()) => backoff = Duration::from_secs(1),
            Err(e) => tracing::warn!(error = %e, "{what}: Ereignisse unterbrochen"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times() {
        assert_eq!(hhmm(time("7:05")), "07:05");
        assert_eq!(time("24:00"), None);
        assert_eq!(time("abc"), None);
    }

    #[test]
    fn targets() {
        let t = target_of(Some(v1::types::RedirectTarget {
            redirect_target: Some(Target::PhoneNumber("12".into())),
        }));
        assert_eq!(t.number.as_deref(), Some("12"));
        assert_eq!(target_of(None), RedirectTarget::default());
    }

    #[test]
    fn hidden_redirects_dropped_and_sorted() {
        let r =
            |id: &str, t: v1::types::RedirectType, visible: bool| v1::redirect::RedirectResponse {
                redirect: Some(v1::types::Redirect {
                    redirect_id: Some(v1::types::RedirectId { id: id.into() }),
                    called_number: "11".into(),
                    redirect_type: t as i32,
                    ..Default::default()
                }),
                visible,
                read_only: true,
            };
        let mut list: Vec<Redirect> = [
            r("t", v1::types::RedirectType::Timeout, true),
            r("a", v1::types::RedirectType::Always, true),
            r("x", v1::types::RedirectType::Busy, false),
        ]
        .into_iter()
        .filter_map(view_of)
        .collect();
        order(&mut list);
        assert_eq!(
            list.iter().map(|r| r.kind).collect::<Vec<_>>(),
            ["always", "timeout"]
        );
        assert!(list[0].read_only);
    }
}
