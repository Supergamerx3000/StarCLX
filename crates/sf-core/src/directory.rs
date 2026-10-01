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
}

#[derive(Debug, Clone, Serialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
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

pub async fn folders(hub: &OneHub) -> sf_onehub::Result<Vec<Folder>> {
    let folders = hub.contact().get_folders(()).await?.into_inner().folders;
    Ok(folders
        .into_iter()
        .filter_map(|f| {
            Some(Folder {
                id: f.folder_id?.id,
                name: f.folder_name,
            })
        })
        .collect())
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
        let label = if a.i18n_display_name.trim().is_empty() {
            label
        } else {
            a.i18n_display_name.trim()
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
            ]
        );
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
