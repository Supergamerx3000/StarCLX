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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeySet {
    id: String,
    #[serde(default)]
    name: String,
    /// Tasten-IDs in Platzreihenfolge; `""` ist ein leerer Platz
    #[serde(default)]
    key_order: Vec<String>,
}

/// Ein User der Anlage, für das Besetztlampenfeld
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Account {
    pub account_id: i32,
    /// OneHub-IDs dieses Users (für Präsenz und Heranholen); füllt der
    /// Aufrufer über [`user_ids`]
    pub user_ids: Vec<String>,
    pub name: String,
    pub number: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Keys {
    pub set_id: String,
    pub set_name: String,
    /// Eigene REST-Account-ID (für neue Tasten)
    pub account_id: String,
    pub keys: Vec<FunctionKey>,
    /// Platzbelegung: Tasten-ID je Platz, `""` für einen leeren Platz
    pub order: Vec<String>,
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

    async fn send_json(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &impl Serialize,
    ) -> Result<(), BoxError> {
        check(self.req(method, path)?.json(body).send().await?).await?;
        Ok(())
    }

    /// Tastensatz, eigene Account-ID, Tasten mit Platzbelegung und User
    pub async fn load(&self) -> Result<Keys, BoxError> {
        let sets: Vec<KeySet> = self.get("/rest/functionkeysets").await?;
        let set = sets
            .into_iter()
            .next()
            .ok_or("Kein Tastensatz auf der Anlage")?;
        let keys: Vec<FunctionKey> = self
            .get(&format!("/rest/functionkeysets/{}", set.id))
            .await?;
        #[derive(Deserialize)]
        struct Me {
            id: i64,
        }
        let me: Me = self.get("/rest/users/me").await?;
        let defaults: serde_json::Value = self.get("/rest/functionkeysets/edit/defaults").await?;
        let mut accounts = accounts(&defaults);
        // Bereits belegte User fehlen in den Vorgaben; die Bearbeitungsform
        // der Taste nennt sie samt Nummer.
        for k in keys
            .iter()
            .filter(|k| k.function_key_type == "BUSYLAMPFIELD")
        {
            let path = format!("/rest/functionkeysets/{}/edit/{}", set.id, k.id);
            match self.get::<serde_json::Value>(&path).await {
                Ok(edit) => {
                    if let Some(a) = blf_account(&edit)
                        && !accounts.iter().any(|x| x.account_id == a.account_id)
                    {
                        accounts.push(a);
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, key = %k.id, "Besetztlampenfeld nicht gelesen")
                }
            }
        }
        accounts.sort_by_key(|a| a.name.to_lowercase());
        Ok(Keys {
            order: slot_order(&set.key_order, &keys),
            set_id: set.id,
            set_name: set.name,
            account_id: me.id.to_string(),
            keys,
            accounts,
            me: String::new(),
        })
    }

    /// Legt eine Taste an (ohne `id`) oder ändert sie. Das Besetztlampenfeld
    /// geht in der Bearbeitungsform an die Anlage; im flachen Format
    /// übernimmt sie den Besitzer statt des gewählten Users.
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
        if let Some(edit) = edit_form(key) {
            match self.send_json(method.clone(), &path, &edit).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    tracing::warn!(error = %e, "Bearbeitungsform abgelehnt, versuche flaches Format")
                }
            }
        }
        self.send_json(method, &path, key).await
    }

    pub async fn delete(&self, set: &str, id: &str) -> Result<(), BoxError> {
        let path = format!("/rest/functionkeysets/{set}/{id}");
        check(self.req(reqwest::Method::DELETE, &path)?.send().await?).await?;
        Ok(())
    }

    /// Speichert die Platzbelegung (`""` = leerer Platz) im Tastensatz.
    pub async fn reorder(
        &self,
        set: &str,
        name: &str,
        order: &[String],
        keys: &[FunctionKey],
    ) -> Result<(), BoxError> {
        let path = format!("/rest/functionkeysets/{set}");
        let body = KeySet {
            id: set.to_owned(),
            name: name.to_owned(),
            key_order: trim_gaps(order),
        };
        let Err(e) = self.send_json(reqwest::Method::PUT, &path, &body).await else {
            return Ok(());
        };
        // Laut REST-Doku eine Liste der Tasten mit neuer Position
        tracing::warn!(error = %e, "keyOrder abgelehnt, versuche Tastenliste");
        let list: Vec<FunctionKey> = order
            .iter()
            .enumerate()
            .filter_map(|(i, id)| {
                let k = keys.iter().find(|k| !id.is_empty() && k.id == *id)?;
                Some(FunctionKey {
                    position: i as i32,
                    ..k.clone()
                })
            })
            .collect();
        self.send_json(reqwest::Method::PUT, &path, &list).await
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

/// Platzbelegung aus `keyOrder`; Tasten, die dort fehlen, kommen ans Ende.
fn slot_order(key_order: &[String], keys: &[FunctionKey]) -> Vec<String> {
    let mut order: Vec<String> = key_order
        .iter()
        .map(|id| {
            if keys.iter().any(|k| k.id == *id) {
                id.clone()
            } else {
                String::new()
            }
        })
        .collect();
    let mut rest: Vec<&FunctionKey> = keys.iter().filter(|k| !order.contains(&k.id)).collect();
    rest.sort_by_key(|k| k.position);
    order.extend(rest.into_iter().map(|k| k.id.clone()));
    trim_gaps(&order)
}

/// Leere Plätze am Ende braucht die Anlage nicht.
fn trim_gaps(order: &[String]) -> Vec<String> {
    let end = order
        .iter()
        .rposition(|id| !id.is_empty())
        .map_or(0, |i| i + 1);
    order[..end].to_vec()
}

/// Bearbeitungsform für Typen, deren flaches Format die Anlage falsch übernimmt
fn edit_form(k: &FunctionKey) -> Option<serde_json::Value> {
    match k.function_key_type.as_str() {
        "BUSYLAMPFIELD" => Some(serde_json::json!({
            "editFunctionKeyBusyLampField": {
                "name": k.name,
                "blfDisplayInformation": k.name,
                "blfAccountId": k.blf_account_id?,
                "number": k.direct_call_targetnumber.clone().unwrap_or_default(),
            }
        })),
        _ => None,
    }
}

fn str_of(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned()
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
                user_ids: Vec::new(),
                name: str_of(a, "displayInformation"),
                number: str_of(a, "primaryInternalPhoneNumber"),
            })
        })
        .collect()
}

/// Der gewählte User aus der Bearbeitungsform eines Besetztlampenfelds
fn blf_account(edit: &serde_json::Value) -> Option<Account> {
    let b = edit.get("editFunctionKeyBusyLampField")?;
    Some(Account {
        account_id: b.get("blfAccountId")?.as_i64()? as i32,
        user_ids: Vec::new(),
        name: str_of(b, "blfDisplayInformation"),
        number: str_of(b, "number"),
    })
}

/// OneHub-IDs zu REST-Account-IDs (für Präsenz und Heranholen)
pub async fn user_ids(
    hub: &OneHub,
    account_ids: &[i32],
) -> sf_onehub::Result<HashMap<i32, Vec<String>>> {
    use v1::useridlookup::user_identifier::Identifier;
    let resp = hub
        .user_id_lookup()
        .batch_get_user_identifiers(v1::useridlookup::BatchGetUserIdentifiersRequest {
            user_identifiers: account_ids
                .iter()
                .map(|id| v1::useridlookup::UserIdentifier {
                    identifier: Some(Identifier::AccountId(id.to_string())),
                })
                .collect(),
        })
        .await?
        .into_inner();
    Ok(resp
        .user_identifiers_list
        .into_iter()
        .filter_map(|u| {
            let account = u.account_id.parse().ok()?;
            let mut ids: Vec<String> = [u.user_id, u.one_hub_user_id]
                .into_iter()
                .flatten()
                .map(|i| i.id)
                .filter(|i| !i.is_empty())
                .collect();
            ids.dedup();
            Some((account, ids))
        })
        .collect())
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

/// Parkt das Gespräch `call_id` auf dem Platz `number`. Ohne `call_id`
/// holt die Anlage das dort geparkte Gespräch auf `phone_id` zurück.
pub async fn park(
    hub: &OneHub,
    call_id: Option<&str>,
    number: &str,
    phone_id: Option<&str>,
) -> sf_onehub::Result<()> {
    hub.call()
        .park_and_orbit(v1::call::ParkAndOrbitRequest {
            number: number.to_owned(),
            call_id: call_id.map(|id| v1::types::CallId { id: id.to_owned() }),
            phone_id: phone_id.map(|id| v1::types::PhoneId { id: id.to_owned() }),
        })
        .await?;
    Ok(())
}

/// Holt einen Anruf heran, der beim User `user_id` klingelt.
pub async fn grab(hub: &OneHub, user_id: &str, phone_id: Option<&str>) -> sf_onehub::Result<()> {
    hub.call()
        .grab_call(v1::call::GrabCallRequest {
            target: Some(v1::call::grab_call_request::Target::UserId(
                v1::types::UserId {
                    id: user_id.to_owned(),
                },
            )),
            phone_id: phone_id.map(|id| v1::types::PhoneId { id: id.to_owned() }),
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SET: &str = r#"[{"functionKeyType":"BUSYLAMPFIELD","id":"1000","accountId":"1003","valid":true,"name":"Claude2, Star2","position":1,"blfAccountId":1004,"directCallTargetnumber":null,"redirectNumberIds":[],"forwardTarget":null,"forwardTargetType":null,"forwardType":null,"groupIds":[],"poNumber":null,"displayNumberId":null,"activateModuleIds":[],"addressbookRequest":null,"addressBookFolderName":null,"callListRequest":null,"dtmf":null,"genericURL":null},
      {"functionKeyType":"PHONEGENERICURL","id":"1013","accountId":"1003","valid":true,"name":"URL","position":0,"genericURL":"https://claude.ai"}]"#;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_keys_and_keeps_gaps() {
        let keys: Vec<FunctionKey> = serde_json::from_str(SET).unwrap();
        assert_eq!(keys[0].blf_account_id, Some(1004));
        assert_eq!(keys[1].generic_url.as_deref(), Some("https://claude.ai"));
        // Lücke bleibt, verschwundene Taste wird zur Lücke
        assert_eq!(
            slot_order(&ids(&["1000", "", "999", "1013"]), &keys),
            ids(&["1000", "", "", "1013"])
        );
        // Fehlt die Taste in keyOrder, kommt sie ans Ende
        assert_eq!(slot_order(&ids(&["1000"]), &keys), ids(&["1000", "1013"]));
        assert_eq!(slot_order(&[], &keys), ids(&["1013", "1000"]));
        assert_eq!(trim_gaps(&ids(&["1000", "", ""])), ids(&["1000"]));
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
        let set = serde_json::to_value(KeySet {
            id: "0".into(),
            name: "default".into(),
            key_order: ids(&["1", ""]),
        })
        .unwrap();
        assert_eq!(
            set,
            serde_json::json!({"id": "0", "name": "default", "keyOrder": ["1", ""]})
        );
    }

    #[test]
    fn blf_goes_out_in_edit_form() {
        let k = FunctionKey {
            function_key_type: "BUSYLAMPFIELD".into(),
            name: "System, Cloud".into(),
            blf_account_id: Some(1000),
            direct_call_targetnumber: Some("10".into()),
            ..Default::default()
        };
        let v = edit_form(&k).unwrap();
        assert_eq!(v["editFunctionKeyBusyLampField"]["blfAccountId"], 1000);
        assert_eq!(v["editFunctionKeyBusyLampField"]["number"], "10");
        assert!(
            edit_form(&FunctionKey {
                function_key_type: "QUICKDIAL".into(),
                ..Default::default()
            })
            .is_none()
        );
    }

    #[test]
    fn blf_accounts() {
        let d = serde_json::json!({"editFunctionKeyBusyLampField": {"availableAccounts": [
            {"uuid": "9d58", "accountId": 1003, "displayInformation": "Claude, Star", "primaryInternalPhoneNumber": "11"}
        ]}});
        assert_eq!(accounts(&d)[0].number, "11");
        let e = serde_json::json!({"editFunctionKeyBusyLampField": {"name": "x", "blfDisplayInformation": "Claude2, Star2", "blfAccountId": 1004, "number": "12"}});
        let a = blf_account(&e).unwrap();
        assert_eq!(
            (a.account_id, a.number.as_str(), a.name.as_str()),
            (1004, "12", "Claude2, Star2")
        );
    }
}
