//! Funktionstasten. OneHub kennt sie nicht; verwaltet werden sie über die
//! REST-API der Anlage (`/rest/functionkeysets`), mit demselben Token.
//! Anlage und Client bearbeiten denselben Tastensatz.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sf_onehub::OneHub;
use sf_onehub::sf_proto::v1;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Eine Taste, wie sie die REST-API liefert und erwartet.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct FunctionKey {
    pub function_key_type: String,
    pub id: String,
    pub account_id: String,
    pub valid: bool,
    pub name: String,
    pub position: i32,
    pub blf_account_id: Option<i32>,
    pub direct_call_targetnumber: Option<String>,
    pub redirect_number_ids: Vec<i32>,
    pub forward_target: Option<String>,
    pub forward_target_type: Option<String>,
    pub forward_type: Option<String>,
    pub group_ids: Vec<i32>,
    pub po_number: Option<String>,
    pub display_number_id: Option<i32>,
    pub activate_module_ids: Vec<String>,
    pub addressbook_request: Option<String>,
    pub address_book_folder_name: Option<String>,
    pub call_list_request: Option<String>,
    pub dtmf: Option<String>,
    #[serde(rename = "genericURL")]
    pub generic_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeySet {
    id: String,
    #[serde(default)]
    key_order: Vec<String>,
}

/// Ein User der Anlage, für das Besetztlampenfeld
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Account {
    pub account_id: i32,
    /// Entspricht der OneHub-User-ID (für die Präsenz)
    pub uuid: String,
    pub name: String,
    pub number: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Keys {
    pub set_id: String,
    /// Eigene REST-Account-ID (für neue Tasten)
    pub account_id: String,
    pub keys: Vec<FunctionKey>,
    pub accounts: Vec<Account>,
    /// Eigene OneHub-User-ID (für den Ruhe-Zustand); setzt der Aufrufer
    pub me: String,
}

/// Zugang zur REST-API mit dem aktuellen Token der Sitzung.
pub struct Rest {
    base: url::Url,
    token: String,
    http: reqwest::Client,
}

impl Rest {
    pub fn new(server: &str, hub: &OneHub) -> Result<Self, BoxError> {
        Ok(Self {
            base: url::Url::parse(server)?,
            token: hub.token().get(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?,
        })
    }

    fn req(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, BoxError> {
        Ok(self
            .http
            .request(method, self.base.join(path)?)
            .bearer_auth(&self.token)
            .header("X-Version", "2"))
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, BoxError> {
        Ok(check(self.req(reqwest::Method::GET, path)?.send().await?)
            .await?
            .json()
            .await?)
    }

    /// Tastensatz, eigene Account-ID, Tasten in Platzreihenfolge und User
    pub async fn load(&self) -> Result<Keys, BoxError> {
        let sets: Vec<KeySet> = self.get("/rest/functionkeysets").await?;
        let set = sets
            .into_iter()
            .next()
            .ok_or("Kein Tastensatz auf der Anlage")?;
        let mut keys: Vec<FunctionKey> = self
            .get(&format!("/rest/functionkeysets/{}", set.id))
            .await?;
        order(&mut keys, &set.key_order);
        #[derive(Deserialize)]
        struct Me {
            id: i64,
        }
        let me: Me = self.get("/rest/users/me").await?;
        let defaults: serde_json::Value = self.get("/rest/functionkeysets/edit/defaults").await?;
        Ok(Keys {
            set_id: set.id,
            account_id: me.id.to_string(),
            keys,
            accounts: accounts(&defaults),
            me: String::new(),
        })
    }

    /// Legt eine Taste an (ohne `id`) oder ändert sie.
    pub async fn save(&self, set: &str, key: &FunctionKey) -> Result<(), BoxError> {
        let (method, path) = if key.id.is_empty() {
            (
                reqwest::Method::POST,
                format!("/rest/functionkeysets/{set}"),
            )
        } else {
            (
                reqwest::Method::PUT,
                format!("/rest/functionkeysets/{set}/{}", key.id),
            )
        };
        check(self.req(method, &path)?.json(key).send().await?).await?;
        Ok(())
    }

    pub async fn delete(&self, set: &str, id: &str) -> Result<(), BoxError> {
        let path = format!("/rest/functionkeysets/{set}/{id}");
        check(self.req(reqwest::Method::DELETE, &path)?.send().await?).await?;
        Ok(())
    }

    /// Speichert die Reihenfolge (alle Tasten mit neuer `position`).
    pub async fn reorder(&self, set: &str, keys: &[FunctionKey]) -> Result<(), BoxError> {
        let keys: Vec<FunctionKey> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| FunctionKey {
                position: i as i32,
                ..k.clone()
            })
            .collect();
        let path = format!("/rest/functionkeysets/{set}");
        check(
            self.req(reqwest::Method::PUT, &path)?
                .json(&keys)
                .send()
                .await?,
        )
        .await?;
        Ok(())
    }
}

/// Fehlertext der Anlage mitgeben, statt nur den Statuscode
async fn check(resp: reqwest::Response) -> Result<reqwest::Response, BoxError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    let detail: String = body.chars().take(300).collect();
    Err(format!("Anlage antwortet {status}: {detail}").into())
}

fn order(keys: &mut [FunctionKey], key_order: &[String]) {
    let rank = |k: &FunctionKey| {
        key_order
            .iter()
            .position(|id| *id == k.id)
            .unwrap_or(usize::MAX)
    };
    keys.sort_by_key(|k| (rank(k), k.position));
}

fn accounts(defaults: &serde_json::Value) -> Vec<Account> {
    let list = defaults
        .pointer("/editFunctionKeyBusyLampField/availableAccounts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    list.iter()
        .filter_map(|a| {
            Some(Account {
                account_id: a.get("accountId")?.as_i64()? as i32,
                uuid: a.get("uuid")?.as_str()?.to_owned(),
                name: a
                    .get("displayInformation")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
                number: a
                    .get("primaryInternalPhoneNumber")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_owned(),
            })
        })
        .collect()
}

/// Telefonie- und Ruhe-Zustand eines Users für die Tastenfarben
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct UserState {
    /// "available", "ringing", "active", "unavailable" oder ""
    pub telephony: &'static str,
    pub dnd: bool,
}

fn telephony(t: i32) -> &'static str {
    use v1::presence::TelephonyState as T;
    match T::try_from(t) {
        Ok(T::Available) => "available",
        Ok(T::Ringing) => "ringing",
        Ok(T::Active) => "active",
        Ok(T::Unavailable) => "unavailable",
        _ => "",
    }
}

/// Verfolgt die Präsenz der angegebenen User (OneHub-IDs). Jede Änderung
/// schickt den vollständigen Stand.
pub struct Presence(JoinHandle<()>);

impl Presence {
    pub fn start(
        hub: OneHub,
        users: Vec<String>,
        updates: mpsc::UnboundedSender<HashMap<String, UserState>>,
    ) -> Self {
        Self(tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            loop {
                match watch(&hub, &users, &updates).await {
                    Ok(()) => backoff = Duration::from_secs(1),
                    Err(e) => tracing::warn!(error = %e, "Präsenz unterbrochen"),
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }))
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn watch(
    hub: &OneHub,
    users: &[String],
    updates: &mpsc::UnboundedSender<HashMap<String, UserState>>,
) -> sf_onehub::Result<()> {
    use v1::presence::presence_event_response::PresenceEvent as E;
    use v1::presence::presence_state::PresenceState as S;
    let mut svc = hub.presence();
    let mut stream = svc.subscribe_presence_events(()).await?.into_inner();
    let initial = svc
        .subscribe_presence_states(v1::presence::SubscribePresenceStatesRequest {
            presence_ids: Some(
                v1::presence::subscribe_presence_states_request::PresenceIds::UserIdList(
                    v1::presence::UserIdList {
                        user_ids: users
                            .iter()
                            .map(|id| v1::types::UserId { id: id.clone() })
                            .collect(),
                    },
                ),
            ),
            return_presence_states: true,
        })
        .await?
        .into_inner();
    let mut states: HashMap<String, UserState> = HashMap::new();
    let apply_state = |states: &mut HashMap<String, UserState>, s: v1::presence::PresenceState| {
        if let Some(S::UserPresenceState(u)) = s.presence_state
            && let Some(id) = u.user_id
        {
            states.insert(
                id.id,
                UserState {
                    telephony: telephony(u.telephony_state),
                    dnd: u.dnd_enabled,
                },
            );
        }
    };
    for s in initial.presence_states {
        apply_state(&mut states, s);
    }
    let _ = updates.send(states.clone());
    while let Some(ev) = stream.message().await? {
        let user = |id: Option<v1::types::UserId>| id.map(|i| i.id).unwrap_or_default();
        match ev.presence_event {
            Some(E::PresenceStateSubscribed(s)) => {
                if let Some(s) = s.presence_state {
                    apply_state(&mut states, s);
                }
            }
            Some(E::TelephonyStateChanged(t)) => {
                use v1::presence::telephony_state_changed_event::Target;
                if let Some(Target::UserId(id)) = t.target {
                    states.entry(id.id).or_default().telephony = telephony(t.telephony_state);
                }
            }
            Some(E::DoNotDisturbStatusChanged(d)) => {
                states.entry(user(d.user_id)).or_default().dnd = d.dnd_enabled;
            }
            _ => continue,
        }
        let _ = updates.send(states.clone());
    }
    Ok(())
}

pub async fn set_dnd(hub: &OneHub, enabled: bool) -> sf_onehub::Result<()> {
    hub.me()
        .set_do_not_disturb(v1::me::SetDoNotDisturbRequest { enabled })
        .await?;
    Ok(())
}

/// Parkt das Gespräch `call_id` auf dem Platz `number`.
pub async fn park(hub: &OneHub, call_id: &str, number: &str) -> sf_onehub::Result<()> {
    hub.call()
        .park_and_orbit(v1::call::ParkAndOrbitRequest {
            number: number.to_owned(),
            call_id: Some(v1::types::CallId {
                id: call_id.to_owned(),
            }),
            phone_id: None,
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SET: &str = r#"[{"functionKeyType":"BUSYLAMPFIELD","id":"1000","accountId":"1003","valid":true,"name":"Claude2, Star2","position":1,"blfAccountId":1004,"directCallTargetnumber":null,"redirectNumberIds":[],"forwardTarget":null,"forwardTargetType":null,"forwardType":null,"groupIds":[],"poNumber":null,"displayNumberId":null,"activateModuleIds":[],"addressbookRequest":null,"addressBookFolderName":null,"callListRequest":null,"dtmf":null,"genericURL":null},
      {"functionKeyType":"PHONEGENERICURL","id":"1013","accountId":"1003","valid":true,"name":"URL","position":0,"genericURL":"https://claude.ai"}]"#;

    #[test]
    fn parses_and_orders_keys() {
        let mut keys: Vec<FunctionKey> = serde_json::from_str(SET).unwrap();
        assert_eq!(keys[0].blf_account_id, Some(1004));
        assert_eq!(keys[1].generic_url.as_deref(), Some("https://claude.ai"));
        order(&mut keys, &["1000".into(), "1013".into()]);
        assert_eq!(keys[0].id, "1000");
        order(&mut keys, &[]);
        assert_eq!(keys[0].id, "1013");
    }

    #[test]
    fn serializes_rest_field_names() {
        let k = FunctionKey {
            function_key_type: "QUICKDIAL".into(),
            direct_call_targetnumber: Some("12".into()),
            ..Default::default()
        };
        let v = serde_json::to_value(&k).unwrap();
        assert_eq!(v["functionKeyType"], "QUICKDIAL");
        assert_eq!(v["directCallTargetnumber"], "12");
        assert!(v.get("genericURL").is_some());
    }

    #[test]
    fn blf_accounts_from_defaults() {
        let d = serde_json::json!({"editFunctionKeyBusyLampField": {"availableAccounts": [
            {"uuid": "9d58", "accountId": 1003, "displayInformation": "Claude, Star", "primaryInternalPhoneNumber": "11"}
        ]}});
        assert_eq!(
            accounts(&d),
            vec![Account {
                account_id: 1003,
                uuid: "9d58".into(),
                name: "Claude, Star".into(),
                number: "11".into()
            }]
        );
    }
}
