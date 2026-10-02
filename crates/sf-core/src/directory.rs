//! Adressbücher der Anlage: schnelle Suche beim Tippen und Blättern pro
//! Adressbuch.

use serde::Serialize;
use sf_onehub::OneHub;
use sf_onehub::sf_proto::v1;
use v1::contact::ContactDisplayKey as Key;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Number {
    /// z. B. "Mobil", "Büro", "Intern"
    pub label: String,
    pub number: String,
}

/// Ein Kontakt, wie ihn die Oberfläche zeigt.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ContactView {
    pub id: String,
    pub name: String,
    pub company: String,
    pub numbers: Vec<Number>,
    pub email: String,
    /// Gesetzt, wenn der Kontakt ein Benutzer der Anlage ist
    pub user_id: Option<String>,
    pub editable: bool,
    pub deletable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    /// Darf der Benutzer hier Kontakte anlegen?
    pub writable: bool,
    /// Das eigene, private Adressbuch
    pub private: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub contacts: Vec<ContactView>,
    pub total: i32,
}

/// Suche über alle Adressbücher, für die Trefferliste unter dem Wählfeld.
pub async fn search(hub: &OneHub, term: &str, limit: i32) -> sf_onehub::Result<Vec<ContactView>> {
    let req = v1::contact::phonenumber::GetSimpleContactsRequest {
        search_term: term.trim().to_owned(),
        order_by: v1::types::ContactOrderBy::LastName as i32,
        order_direction: v1::types::OrderDirection::Ascending as i32,
        limit,
        offset: 0,
    };
    let contacts = hub
        .simple_contact()
        .get_simple_contacts(req)
        .await?
        .into_inner()
        .contacts;
    Ok(contacts.into_iter().map(simple_view).collect())
}

/// Adressbücher des Benutzers. Die Ordner kommen über gRPC; die auf der
/// Anlage konfigurierten Anzeigenamen (Alias) liefert nur REST
/// (`/rest/contacts/tags`, gleiche IDs). Fehlt REST, gelten die eingebauten
/// Namen.
pub async fn folders(hub: &OneHub, server: &str) -> sf_onehub::Result<Vec<Folder>> {
    let folders = hub.contact().get_folders(()).await?.into_inner().folders;
    let aliases = match folder_aliases(server, &hub.token().get()).await {
        Ok(a) => a,
        Err(e) => {
            tracing::debug!(error = %e, "Adressbuch-Namen über REST nicht gelesen");
            Default::default()
        }
    };
    Ok(folders
        .into_iter()
        .filter_map(|f| {
            let id = f.folder_id.as_ref()?.id.clone();
            let alias = aliases.get(&id).map(String::as_str);
            Some(Folder {
                name: display_name(&f.folder_name, f.folder_type(), alias),
                id,
                writable: f.is_writable,
                private: f.folder_type() == v1::contact::FolderType::Private,
            })
        })
        .collect())
}

#[derive(serde::Deserialize)]
struct Tag {
    id: String,
    #[serde(default)]
    alias: String,
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

async fn folder_aliases(
    server: &str,
    token: &str,
) -> Result<std::collections::HashMap<String, String>, BoxError> {
    let url = url::Url::parse(server)?.join("/rest/contacts/tags")?;
    let tags: Vec<Tag> = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .tls_backend_preconfigured(sf_tls::client_config())
        .build()?
        .get(url)
        .bearer_auth(token)
        .header("X-Version", "2")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(tags.into_iter().map(|t| (t.id, t.alias)).collect())
}

/// Alias von der Anlage, sonst der eingebaute bzw. gelieferte Name
fn display_name(name: &str, kind: v1::contact::FolderType, alias: Option<&str>) -> String {
    match alias.map(str::trim) {
        Some(a) if !a.is_empty() && !a.starts_with("de.vertico.") => a.to_owned(),
        _ => folder_label(name, kind),
    }
}

/// Anzeigename eines Adressbuchs. Für die eingebauten Ordner schickt die
/// Anlage nur einen Übersetzungsschlüssel wie
/// `de.vertico.starface.addressbook.folder.all`; die Namen entsprechen der
/// deutschen Oberfläche der Anlage. Eigene Ordner behalten ihren Namen.
fn folder_label(name: &str, kind: v1::contact::FolderType) -> String {
    use v1::contact::FolderType as T;
    let Some(key) = name.strip_prefix("de.vertico.starface.addressbook.folder.") else {
        return name.to_owned();
    };
    match (key, kind) {
        ("all", _) => "Zentral",
        ("users", _) => "Benutzer",
        ("private", _) => "Privat",
        (_, T::Public) => "Zentral",
        (_, T::Users) => "Benutzer",
        (_, T::Private) => "Privat",
        _ => return key.to_owned(),
    }
    .to_owned()
}

/// Eine Seite eines Adressbuchs, optional gefiltert.
pub async fn list(
    hub: &OneHub,
    folder: &str,
    term: &str,
    offset: i32,
    limit: i32,
) -> sf_onehub::Result<Page> {
    let req = v1::contact::GetContactsRequest {
        search_term: term.trim().to_owned(),
        folder_id: Some(v1::contact::FolderId {
            id: folder.to_owned(),
        }),
        order_by: v1::types::ContactOrderBy::LastName as i32,
        order_direction: v1::types::OrderDirection::Ascending as i32,
        limit,
        offset,
    };
    let res = hub.contact().get_contacts(req).await?.into_inner();
    Ok(Page {
        contacts: res.contacts.iter().map(contact_view).collect(),
        total: res.total_count,
    })
}

fn join_name(first: &str, last: &str) -> String {
    format!("{} {}", first.trim(), last.trim())
        .trim()
        .to_owned()
}

fn push_number(numbers: &mut Vec<Number>, label: &str, number: &str) {
    let number = number.trim();
    if !number.is_empty() && !numbers.iter().any(|n| n.number == number) {
        numbers.push(Number {
            label: label.to_owned(),
            number: number.to_owned(),
        });
    }
}

fn simple_view(c: v1::contact::phonenumber::SimpleContact) -> ContactView {
    use v1::contact::phonenumber::PhoneNumberType as T;
    let mut numbers = Vec::new();
    push_number(&mut numbers, "Intern", &c.internal_phone_number);
    for n in &c.phone_numbers {
        let label = match T::try_from(n.r#type).unwrap_or(T::Unspecified) {
            T::Mobile => "Mobil",
            T::Home => "Privat",
            T::Fax => continue,
            T::Phone | T::Unspecified => "Telefon",
        };
        push_number(&mut numbers, label, &n.number);
    }
    push_number(&mut numbers, "Extern", &c.external_phone_number);
    ContactView {
        id: c.id.map(|i| i.id).unwrap_or_default(),
        name: join_name(&c.first_name, &c.last_name),
        company: c.company,
        numbers,
        email: String::new(),
        user_id: None,
        editable: false,
        deletable: false,
    }
}

fn contact_view(c: &v1::contact::Contact) -> ContactView {
    let attrs = || c.blocks.iter().flat_map(|b| b.attributes.iter());
    let first = |key: Key| {
        attrs()
            .find(|a| a.display_key == key as i32 && !a.value.trim().is_empty())
            .map(|a| a.value.trim().to_owned())
            .unwrap_or_default()
    };
    let mut numbers = Vec::new();
    push_number(&mut numbers, "Intern", &c.internal_number);
    for a in attrs() {
        let label = match Key::try_from(a.display_key).unwrap_or(Key::Unspecified) {
            Key::PhoneNumber => "Telefon",
            Key::OfficePhoneNumber => "Büro",
            Key::MobilePhoneNumber => "Mobil",
            Key::PrivatePhoneNumber => "Privat",
            _ => continue,
        };
        // Übersetzungsschlüssel (de.vertico.…label_telephonenumber) sind
        // keine Anzeigetexte
        let custom = a.i18n_display_name.trim();
        let label = if custom.is_empty() || (custom.contains('.') && !custom.contains(' ')) {
            label
        } else {
            custom
        };
        push_number(&mut numbers, label, &a.value);
    }
    push_number(&mut numbers, "Extern", &c.external_number);
    let company = first(Key::Company);
    let mut name = join_name(&first(Key::Name), &first(Key::Surname));
    if name.is_empty() {
        name.clone_from(&company);
    }
    ContactView {
        id: c
            .contact_id
            .as_ref()
            .map(|i| i.id.clone())
            .unwrap_or_default(),
        name,
        company,
        numbers,
        email: first(Key::Email),
        user_id: c
            .user_id
            .as_ref()
            .map(|u| u.id.clone())
            .filter(|u| !u.is_empty()),
        editable: c.is_editable,
        deletable: c.is_deletable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(key: Key, value: &str) -> v1::contact::ContactAttribute {
        v1::contact::ContactAttribute {
            display_key: key as i32,
            value: value.into(),
            ..Default::default()
        }
    }

    #[test]
    fn contact_view_collects_name_and_numbers() {
        let c = v1::contact::Contact {
            contact_id: Some(v1::types::ContactId { id: "c1".into() }),
            internal_number: "12".into(),
            blocks: vec![v1::contact::ContactBlock {
                attributes: vec![
                    attr(Key::Name, "Claude"),
                    attr(Key::Surname, "Star"),
                    attr(Key::Company, "Muster AG"),
                    attr(Key::MobilePhoneNumber, "+41 79 000 00 00"),
                    attr(Key::OfficePhoneNumber, "12"),
                    v1::contact::ContactAttribute {
                        i18n_display_name:
                            "de.vertico.starface.addressbook.phone.label_privatetelephonenumber"
                                .into(),
                        ..attr(Key::PrivatePhoneNumber, "+41 81 000 00 00")
                    },
                    attr(Key::Email, "c@example.com"),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let v = contact_view(&c);
        assert_eq!(v.name, "Claude Star");
        assert_eq!(v.company, "Muster AG");
        assert_eq!(v.email, "c@example.com");
        // Doppelte Nummer (12) nur einmal
        assert_eq!(
            v.numbers,
            vec![
                Number {
                    label: "Intern".into(),
                    number: "12".into()
                },
                Number {
                    label: "Mobil".into(),
                    number: "+41 79 000 00 00".into()
                },
                Number {
                    label: "Privat".into(),
                    number: "+41 81 000 00 00".into()
                },
            ]
        );
    }

    #[test]
    fn builtin_folders_get_readable_names() {
        use v1::contact::FolderType as T;
        let p = "de.vertico.starface.addressbook.folder.";
        assert_eq!(folder_label(&format!("{p}all"), T::Public), "Zentral");
        assert_eq!(folder_label(&format!("{p}users"), T::Users), "Benutzer");
        assert_eq!(folder_label(&format!("{p}private"), T::Private), "Privat");
        assert_eq!(folder_label(&format!("{p}neu"), T::Users), "Benutzer");
        assert_eq!(folder_label(&format!("{p}neu"), T::Custom), "neu");
        assert_eq!(
            folder_label("SelectLine Mitarbeiter", T::Custom),
            "SelectLine Mitarbeiter"
        );
    }

    #[test]
    fn alias_from_pbx_wins() {
        use v1::contact::FolderType as T;
        let all = "de.vertico.starface.addressbook.folder.all";
        assert_eq!(display_name(all, T::Public, Some("Zentrale")), "Zentrale");
        assert_eq!(display_name(all, T::Public, Some(" ")), "Zentral");
        assert_eq!(display_name(all, T::Public, Some(all)), "Zentral");
        assert_eq!(display_name(all, T::Public, None), "Zentral");
        assert_eq!(display_name("Firma", T::Number, Some("Firma")), "Firma");
    }

    #[test]
    fn company_only_contact_uses_company_as_name() {
        let c = v1::contact::Contact {
            blocks: vec![v1::contact::ContactBlock {
                attributes: vec![attr(Key::Company, "Muster AG")],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(contact_view(&c).name, "Muster AG");
    }
}
