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

pub(crate) type BoxError = Box<dyn std::error::Error + Send + Sync>;

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
    /// Eine Gruppe statt eines Users; `user_ids` enthält dann die
    /// OneHub-Gruppen-ID (siehe [`group_ids`])
    pub group: bool,
}

/// Gruppe, die eine Taste „Gruppe An-/Abmelden“ schalten kann
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GroupChoice {
    /// Konto-ID der Gruppe, wie sie in `groupIds` der Taste steht
    pub id: i32,
    pub name: String,
}

/// Modul, das eine Taste „Modul aktivieren“ schalten kann
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModuleChoice {
    /// ID des Moduls, wie sie in `activateModuleIds` der Taste steht
    pub id: String,
    pub name: String,
}

/// Wählbare Gruppen und Module aus den Vorgaben der Anlage
#[derive(Debug, Clone, Default)]
struct Choices {
    groups: Vec<GroupChoice>,
    modules: Vec<ModuleChoice>,
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
    /// Wählbare Gruppen für „Gruppe An-/Abmelden“
    pub group_choices: Vec<GroupChoice>,
    /// Wählbare Module für „Modul aktivieren“
    pub module_choices: Vec<ModuleChoice>,
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
                .tls_backend_preconfigured(sf_tls::client_config())
                .build()?,
        })
    }

    pub(crate) fn req(
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

    pub(crate) async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, BoxError> {
        Ok(check(self.req(reqwest::Method::GET, path)?.send().await?)
            .await?
            .json()
            .await?)
    }

    pub(crate) async fn send_json(
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
            group_choices: group_choices(&defaults),
            module_choices: module_choices(&defaults),
            me: String::new(),
        })
    }

    /// Legt eine Taste an (ohne `id`) oder ändert sie. Das Besetztlampenfeld
    /// geht in der Bearbeitungsform an die Anlage; im flachen Format
    /// übernimmt sie den Besitzer statt des gewählten Users.
    pub async fn save(&self, set: &str, key: &FunctionKey) -> Result<(), BoxError> {
        // Die Formen für Gruppen und Module nennen alle wählbaren mit Namen
        let choices = if matches!(
            key.function_key_type.as_str(),
            "GROUPLOGIN" | "MODULEACTIVATION"
        ) {
            match self
                .get::<serde_json::Value>("/rest/functionkeysets/edit/defaults")
                .await
            {
                Ok(d) => Choices {
                    groups: group_choices(&d),
                    modules: module_choices(&d),
                },
                Err(e) => {
                    tracing::warn!(error = %e, "Vorgaben für die Taste nicht gelesen");
                    Choices::default()
                }
            }
        } else {
            Choices::default()
        };
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
        if let Some(edit) = edit_form(key, &choices) {
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
pub(crate) async fn check(resp: reqwest::Response) -> Result<reqwest::Response, BoxError> {
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
fn edit_form(k: &FunctionKey, choices: &Choices) -> Option<serde_json::Value> {
    let groups = &choices.groups;
    let modules = &choices.modules;
    match k.function_key_type.as_str() {
        // Wie die Web-App: alle Gruppen, die gewählten mit `activated`
        "GROUPLOGIN" if !groups.is_empty() => Some(serde_json::json!({
            "editFunctionKeyGroupLogin": {
                "name": k.name,
                "editFunctionKeyGlGroupSettings": groups
                    .iter()
                    .map(|g| serde_json::json!({
                        "groupId": g.id,
                        "groupname": g.name,
                        "activated": k.group_ids.contains(&g.id),
                    }))
                    .collect::<Vec<_>>(),
            }
        })),
        "MODULEACTIVATION" if !modules.is_empty() => Some(serde_json::json!({
            "editFunctionKeyModuleActivation": {
                "name": k.name,
                "editFunctionKeyMaModuleSettings": modules
                    .iter()
                    .map(|m| serde_json::json!({
                        "moduleId": m.id,
                        "name": m.name,
                        "activated": k.activate_module_ids.contains(&m.id),
                    }))
                    .collect::<Vec<_>>(),
            }
        })),
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
                group: false,
            })
        })
        .collect()
}

/// Gruppen aus den Vorgaben der Gruppen-Taste, nach Namen sortiert
fn group_choices(defaults: &serde_json::Value) -> Vec<GroupChoice> {
    let mut list: Vec<GroupChoice> = defaults
        .pointer("/editFunctionKeyGroupLogin/editFunctionKeyGlGroupSettings")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|g| {
            Some(GroupChoice {
                id: g.get("groupId")?.as_i64()? as i32,
                name: str_of(g, "groupname"),
            })
        })
        .collect();
    list.sort_by_key(|g| g.name.to_lowercase());
    list
}

/// Module aus den Vorgaben der Modul-Taste, nach Namen sortiert
fn module_choices(defaults: &serde_json::Value) -> Vec<ModuleChoice> {
    let mut list: Vec<ModuleChoice> = defaults
        .pointer("/editFunctionKeyModuleActivation/editFunctionKeyMaModuleSettings")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some(ModuleChoice {
                id: m.get("moduleId")?.as_str()?.to_owned(),
                name: str_of(m, "name"),
            })
        })
        .collect();
    list.sort_by_key(|m| m.name.to_lowercase());
    list
}

/// Der gewählte User aus der Bearbeitungsform eines Besetztlampenfelds
fn blf_account(edit: &serde_json::Value) -> Option<Account> {
    let b = edit.get("editFunctionKeyBusyLampField")?;
    Some(Account {
        account_id: b.get("blfAccountId")?.as_i64()? as i32,
        user_ids: Vec::new(),
        name: str_of(b, "blfDisplayInformation"),
        number: str_of(b, "number"),
        group: false,
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

/// OneHub-Gruppen-IDs zu Konten, hinter denen kein User steht (Gruppen im
/// Besetztlampenfeld). Gesucht wird über die Nummer, sonst den Namen; die
/// Konto-ID steht je nach Version in der Gruppen-ID oder in `logon_id`.
pub async fn group_ids(
    hub: &OneHub,
    accounts: &[Account],
) -> sf_onehub::Result<HashMap<i32, String>> {
    use v1::sfpbx::group::GroupSearchType as T;
    let mut svc = hub.group();
    let mut found = HashMap::new();
    for a in accounts {
        let key = a.account_id.to_string();
        for (term, kind) in [(&a.number, T::PhoneNumber), (&a.name, T::Name)] {
            if term.is_empty() {
                continue;
            }
            let groups = svc
                .search_groups(v1::sfpbx::group::SearchGroupsRequest {
                    search_term: term.clone(),
                    group_search_types: vec![kind as i32],
                })
                .await?
                .into_inner()
                .groups;
            let hit = groups.iter().find_map(|g| {
                let id = g.group_id.as_ref()?.id.clone();
                let same_number = g
                    .phone_numbers
                    .iter()
                    .any(|n| crate::account::format_number(n) == a.number);
                (id == key || g.logon_id == key || same_number).then_some(id)
            });
            if let Some(id) = hit {
                found.insert(a.account_id, id);
                break;
            }
        }
    }
    Ok(found)
}

/// Präsenz eines Users für die Funktionstasten: Telefon, Ruhe, Chat und
/// Umleitung, wie die Anlage sie meldet.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct UserState {
    /// "available", "ringing", "active", "unavailable" oder ""
    pub telephony: &'static str,
    pub dnd: bool,
    /// Chat: "available", "away", "dnd", "offline" oder "" (kein Chat)
    pub chat: &'static str,
    /// Selbst gesetzter Statustext
    pub chat_message: String,
    /// Eine Umleitung ist aktiv
    pub redirect: bool,
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

fn chat(c: i32) -> &'static str {
    use v1::presence::ChatState as C;
    match C::try_from(c) {
        Ok(C::Available | C::FreeForChat) => "available",
        Ok(C::Away | C::ExtendedAway) => "away",
        Ok(C::DoNotDisturb) => "dnd",
        Ok(C::Unavailable) => "offline",
        _ => "",
    }
}

fn user_state(u: v1::presence::UserPresenceState) -> UserState {
    UserState {
        telephony: telephony(u.telephony_state),
        dnd: u.dnd_enabled,
        chat: chat(u.chat_state),
        chat_message: u.chat_state_message,
        redirect: u.redirect_enabled,
    }
}

/// Aktueller Zustand eines einzelnen Users, wie ihn die Anlage gerade kennt
pub async fn current_state(hub: &OneHub, user_id: &str) -> sf_onehub::Result<Option<UserState>> {
    use v1::presence::presence_state::PresenceState as S;
    let resp = hub
        .presence()
        .subscribe_presence_states(v1::presence::SubscribePresenceStatesRequest {
            presence_ids: Some(
                v1::presence::subscribe_presence_states_request::PresenceIds::UserIdList(
                    v1::presence::UserIdList {
                        user_ids: vec![v1::types::UserId {
                            id: user_id.to_owned(),
                        }],
                    },
                ),
            ),
            return_presence_states: true,
        })
        .await?
        .into_inner();
    Ok(resp
        .presence_states
        .into_iter()
        .find_map(|s| match s.presence_state {
            Some(S::UserPresenceState(u)) => Some(user_state(u)),
            _ => None,
        }))
}

/// Verfolgt die Präsenz der angegebenen User (OneHub-IDs). Jede Änderung
/// schickt den vollständigen Stand.
pub struct Presence(JoinHandle<()>);

impl Presence {
    pub fn start(
        hub: OneHub,
        users: Vec<String>,
        groups: Vec<String>,
        updates: mpsc::UnboundedSender<HashMap<String, UserState>>,
    ) -> Self {
        Self(tokio::spawn(crate::reconnect::forever(
            "Präsenz",
            MAX_BACKOFF,
            move || {
                let (hub, users, groups, updates) =
                    (hub.clone(), users.clone(), groups.clone(), updates.clone());
                async move { watch(&hub, &users, &groups, &updates).await }
            },
        )))
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
    groups: &[String],
    updates: &mpsc::UnboundedSender<HashMap<String, UserState>>,
) -> sf_onehub::Result<()> {
    use v1::presence::presence_event_response::PresenceEvent as E;
    use v1::presence::presence_state::PresenceState as S;
    let mut svc = hub.presence();
    // Den Ereignis-Stream nebenher öffnen: Die Anlage antwortet darauf erst
    // mit dem ersten Ereignis. Darauf zu warten hielt auch die Anfangszustände
    // auf, die Tasten blieben ohne Zustand.
    let mut events_svc = svc.clone();
    let events = tokio::spawn(async move { events_svc.subscribe_presence_events(()).await });
    tokio::task::yield_now().await;
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
    let mut initial = initial.presence_states;
    if !groups.is_empty() {
        let g = svc
            .subscribe_presence_states(v1::presence::SubscribePresenceStatesRequest {
                presence_ids: Some(
                    v1::presence::subscribe_presence_states_request::PresenceIds::GroupIdList(
                        v1::presence::GroupIdList {
                            group_ids: groups
                                .iter()
                                .map(|id| v1::types::GroupId { id: id.clone() })
                                .collect(),
                        },
                    ),
                ),
                return_presence_states: true,
            })
            .await?
            .into_inner();
        initial.extend(g.presence_states);
    }
    let mut states: HashMap<String, UserState> = HashMap::new();
    let apply_state = |states: &mut HashMap<String, UserState>, s: v1::presence::PresenceState| {
        match s.presence_state {
            Some(S::UserPresenceState(u)) => {
                if let Some(id) = u.user_id.clone() {
                    states.insert(id.id, user_state(u));
                }
            }
            // Gruppen haben nur Telefon und Umleitung
            Some(S::GroupPresenceState(g)) => {
                if let Some(id) = g.group_id {
                    states.insert(
                        id.id,
                        UserState {
                            telephony: telephony(g.telephony_state),
                            redirect: g.redirect_enabled,
                            ..Default::default()
                        },
                    );
                }
            }
            None => {}
        }
    };
    tracing::debug!(
        users = users.len(),
        groups = groups.len(),
        initial = initial.len(),
        "Präsenz abonniert"
    );
    for s in initial {
        apply_state(&mut states, s);
    }
    tracing::debug!(?states, "Präsenz Anfangsstand");
    let _ = updates.send(states.clone());
    let mut stream = match events.await {
        Ok(r) => r?.into_inner(),
        // Aufgabe abgebrochen: neuer Versuch über Presence::start
        Err(e) => {
            tracing::warn!(error = %e, "Präsenz-Ereignisse nicht abonniert");
            return Ok(());
        }
    };
    while let Some(ev) = stream.message().await? {
        tracing::debug!(?ev, "Präsenz-Ereignis");
        let user = |id: Option<v1::types::UserId>| id.map(|i| i.id).unwrap_or_default();
        match ev.presence_event {
            Some(E::PresenceStateSubscribed(s)) => {
                if let Some(s) = s.presence_state {
                    apply_state(&mut states, s);
                }
            }
            Some(E::TelephonyStateChanged(t)) => {
                use v1::presence::telephony_state_changed_event::Target;
                let id = match t.target {
                    Some(Target::UserId(id)) => id.id,
                    Some(Target::GroupId(id)) => id.id,
                    None => continue,
                };
                states.entry(id).or_default().telephony = telephony(t.telephony_state);
            }
            Some(E::DoNotDisturbStatusChanged(d)) => {
                states.entry(user(d.user_id)).or_default().dnd = d.dnd_enabled;
            }
            Some(E::ChatStateChanged(c)) => {
                states.entry(user(c.user_id)).or_default().chat = chat(c.chat_state);
            }
            Some(E::ChatStateMessageChanged(c)) => {
                states.entry(user(c.user_id)).or_default().chat_message = c.chat_state_message;
            }
            Some(E::RedirectStatusChanged(r)) => {
                use v1::presence::redirect_status_changed_event::Target;
                let id = match r.target {
                    Some(Target::UserId(id)) => id.id,
                    Some(Target::GroupId(id)) => id.id,
                    None => continue,
                };
                states.entry(id).or_default().redirect = r.redirect_enabled;
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
        let v = edit_form(&k, &Choices::default()).unwrap();
        assert_eq!(v["editFunctionKeyBusyLampField"]["blfAccountId"], 1000);
        assert_eq!(v["editFunctionKeyBusyLampField"]["number"], "10");
        assert!(
            edit_form(
                &FunctionKey {
                    function_key_type: "QUICKDIAL".into(),
                    ..Default::default()
                },
                &Choices::default()
            )
            .is_none()
        );
    }

    #[test]
    fn group_login_goes_out_with_all_groups() {
        let d = serde_json::json!({"editFunctionKeyGroupLogin": {"name": "", "editFunctionKeyGlGroupSettings": [
            {"groupId": 1007, "groupname": "Zentrale", "activated": false},
            {"groupId": 1008, "groupname": "Support", "activated": false}
        ]}});
        let groups = group_choices(&d);
        assert_eq!(
            groups
                .iter()
                .map(|g| (g.id, g.name.as_str()))
                .collect::<Vec<_>>(),
            [(1008, "Support"), (1007, "Zentrale")]
        );
        let k = FunctionKey {
            function_key_type: "GROUPLOGIN".into(),
            name: "Gruppe[Support]".into(),
            group_ids: vec![1008],
            ..Default::default()
        };
        let choices = Choices {
            groups,
            ..Default::default()
        };
        let v = edit_form(&k, &choices).unwrap();
        let s = &v["editFunctionKeyGroupLogin"]["editFunctionKeyGlGroupSettings"];
        assert_eq!(v["editFunctionKeyGroupLogin"]["name"], "Gruppe[Support]");
        assert_eq!(
            s[0],
            serde_json::json!({"groupId": 1008, "groupname": "Support", "activated": true})
        );
        assert_eq!(s[1]["activated"], false);
        // Ohne Vorgaben geht die Taste im flachen Format hinaus
        assert!(edit_form(&k, &Choices::default()).is_none());
    }

    #[test]
    fn module_activation_goes_out_with_all_modules() {
        let d = serde_json::json!({"editFunctionKeyModuleActivation": {"name": "", "editFunctionKeyMaModuleSettings": [
            {"moduleId": "m-2", "name": "Zeitsteuerung", "activated": false},
            {"moduleId": "m-1", "name": "Nachtschaltung", "activated": false}
        ]}});
        let choices = Choices {
            modules: module_choices(&d),
            ..Default::default()
        };
        assert_eq!(choices.modules[0].id, "m-1");
        let k = FunctionKey {
            function_key_type: "MODULEACTIVATION".into(),
            name: "Nacht".into(),
            activate_module_ids: vec!["m-2".into()],
            ..Default::default()
        };
        let v = edit_form(&k, &choices).unwrap();
        let s = &v["editFunctionKeyModuleActivation"]["editFunctionKeyMaModuleSettings"];
        assert_eq!(
            s[0],
            serde_json::json!({"moduleId": "m-1", "name": "Nachtschaltung", "activated": false})
        );
        assert_eq!(s[1]["activated"], true);
        assert!(edit_form(&k, &Choices::default()).is_none());
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
