//! Kontakte anlegen, bearbeiten und löschen. Welche Felder ein Kontakt hat,
//! gibt die Anlage über `GetScheme` vor (Blöcke mit Attributen); das
//! Formular zeigt genau diese Felder und schreibt die Werte in dieselbe
//! Struktur zurück.

use serde::{Deserialize, Serialize};
use sf_onehub::OneHub;
use sf_onehub::sf_proto::v1;
use v1::contact::{ContactAttribute, ContactBlock, ContactDisplayKey as Key};

/// Ein Eingabefeld des Formulars.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Field {
    /// Name des Blocks im Schema (zum Zurückschreiben)
    pub block: String,
    /// Überschrift der Gruppe
    pub group: String,
    /// Name des Attributs im Schema (zum Zurückschreiben)
    pub name: String,
    /// `ContactDisplayKey`, damit die Oberfläche z. B. Nummernfelder erkennt
    pub key: i32,
    pub label: String,
    pub value: String,
}

/// Alle Blöcke des Schemas in Anzeigereihenfolge, mit Überschrift
async fn scheme(hub: &OneHub) -> sf_onehub::Result<Vec<(String, ContactBlock)>> {
    let s = hub.contact().get_scheme(()).await?.into_inner();
    Ok(s.summary_block
        .map(|b| ("Kontakt".to_owned(), b))
        .into_iter()
        .chain(s.phone_numbers_block.map(|b| ("Rufnummern".to_owned(), b)))
        .chain(s.detail_blocks.into_iter().map(|b| (detail_label(&b), b)))
        .collect())
}

fn blocks_of(scheme: Vec<(String, ContactBlock)>) -> Vec<ContactBlock> {
    scheme.into_iter().map(|(_, b)| b).collect()
}

/// Ohne Nachname oder Firma lehnt die Anlage einen Kontakt ab
/// ("Internal error"). Gibt es keines der beiden Felder, reicht irgendein Wert.
pub fn missing_name(values: &[Field]) -> bool {
    let named = |k: Key| values.iter().any(|f| f.key == k as i32);
    let has = |k: Key| {
        values
            .iter()
            .any(|f| f.key == k as i32 && !f.value.trim().is_empty())
    };
    if named(Key::Surname) || named(Key::Company) {
        !has(Key::Surname) && !has(Key::Company)
    } else {
        values.iter().all(|f| f.value.trim().is_empty())
    }
}

/// Leeres Formular für einen neuen Kontakt
pub async fn empty(hub: &OneHub) -> sf_onehub::Result<Vec<Field>> {
    Ok(fields(&scheme(hub).await?, &[]))
}

/// Formular mit den Werten eines bestehenden Kontakts
pub async fn load(hub: &OneHub, id: &str) -> sf_onehub::Result<Vec<Field>> {
    let contact = get(hub, id).await?;
    Ok(fields(&scheme(hub).await?, &contact.blocks))
}

pub async fn create(hub: &OneHub, folder: &str, values: &[Field]) -> sf_onehub::Result<()> {
    let mut blocks = blocks_of(scheme(hub).await?);
    apply(&mut blocks, &[], values);
    // Leere Felder und Blöcke gar nicht erst schicken
    for b in &mut blocks {
        b.attributes.retain(|a| !a.value.trim().is_empty());
    }
    blocks.retain(|b| !b.attributes.is_empty());
    hub.contact()
        .create_contact(v1::contact::CreateContactRequest {
            blocks,
            folder_id: Some(v1::contact::FolderId { id: folder.into() }),
        })
        .await?;
    Ok(())
}

pub async fn update(hub: &OneHub, id: &str, values: &[Field]) -> sf_onehub::Result<()> {
    let mut contact = get(hub, id).await?;
    let scheme = blocks_of(scheme(hub).await?);
    apply(&mut contact.blocks, &scheme, values);
    hub.contact()
        .update_contact(v1::contact::UpdateContactRequest {
            contact: Some(contact),
        })
        .await?;
    Ok(())
}

pub async fn delete(hub: &OneHub, id: &str) -> sf_onehub::Result<()> {
    hub.contact()
        .delete_contact(v1::contact::DeleteContactRequest {
            contact_id: Some(v1::types::ContactId { id: id.into() }),
        })
        .await?;
    Ok(())
}

async fn get(hub: &OneHub, id: &str) -> sf_onehub::Result<v1::contact::Contact> {
    Ok(hub
        .contact()
        .get_contact(v1::contact::GetContactRequest {
            contact_id: Some(v1::types::ContactId { id: id.into() }),
        })
        .await?
        .into_inner()
        .contact
        .unwrap_or_default())
}

/// Felder aus dem Schema, gefüllt mit den Werten aus `current`. Dasselbe
/// Attribut kann in mehreren Blöcken stehen (z. B. `phone` in den
/// Rufnummern und in einem Detailblock); es erscheint nur einmal.
fn fields(scheme: &[(String, ContactBlock)], current: &[ContactBlock]) -> Vec<Field> {
    let mut out: Vec<Field> = Vec::new();
    for (group, block) in scheme {
        for attr in &block.attributes {
            if out.iter().any(|f| f.name == attr.name) {
                continue;
            }
            let value = current
                .iter()
                .flat_map(|b| &b.attributes)
                .filter(|a| a.name == attr.name)
                .map(|a| a.value.trim())
                .find(|v| !v.is_empty())
                .unwrap_or_default()
                .to_owned();
            out.push(Field {
                block: block.name.clone(),
                group: group.clone(),
                name: attr.name.clone(),
                key: attr.display_key,
                label: attr_label(attr),
                value,
            });
        }
    }
    out
}

/// Schreibt die Werte in alle Blöcke, die das Attribut enthalten; fehlt es
/// überall, kommt es mit seinem Block aus dem Schema dazu.
fn apply(blocks: &mut Vec<ContactBlock>, scheme: &[ContactBlock], values: &[Field]) {
    for f in values {
        let value = f.value.trim().to_owned();
        let mut found = false;
        for a in blocks
            .iter_mut()
            .flat_map(|b| b.attributes.iter_mut())
            .filter(|a| a.name == f.name)
        {
            a.value.clone_from(&value);
            found = true;
        }
        if found || value.is_empty() {
            continue;
        }
        let block = match blocks.iter().position(|b| b.name == f.block) {
            Some(i) => &mut blocks[i],
            None => {
                let Some(template) = scheme.iter().find(|b| b.name == f.block) else {
                    continue;
                };
                blocks.push(ContactBlock {
                    attributes: Vec::new(),
                    ..template.clone()
                });
                blocks.last_mut().unwrap()
            }
        };
        let template = scheme
            .iter()
            .filter(|b| b.name == f.block)
            .flat_map(|b| &b.attributes)
            .find(|a| a.name == f.name);
        block.attributes.push(ContactAttribute {
            value,
            ..template.cloned().unwrap_or_else(|| ContactAttribute {
                name: f.name.clone(),
                display_key: f.key,
                ..Default::default()
            })
        });
    }
}

/// Übersetzungsschlüssel wie `de.vertico…` sind keine Anzeigetexte
fn readable(s: &str) -> Option<&str> {
    let s = s.trim();
    (!s.is_empty() && (s.contains(' ') || !s.contains('.'))).then_some(s)
}

fn detail_label(block: &ContactBlock) -> String {
    readable(&block.name)
        .map(capitalize)
        .unwrap_or_else(|| "Weitere Angaben".into())
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

fn attr_label(a: &ContactAttribute) -> String {
    let fixed = match Key::try_from(a.display_key).unwrap_or(Key::Unspecified) {
        Key::Name => "Vorname",
        Key::Surname => "Nachname",
        Key::Salutation => "Anrede",
        Key::Title => "Titel",
        Key::Email => "E-Mail",
        Key::Country => "Land",
        Key::City => "Ort",
        Key::State => "Kanton/Bundesland",
        Key::PostalCode => "PLZ",
        Key::Street => "Strasse",
        Key::Url => "Webseite",
        Key::Company => "Firma",
        Key::Messenger => "Messenger",
        Key::Birthday => "Geburtstag",
        Key::Note => "Notiz",
        Key::JobTitle => "Position",
        Key::PhoneNumber => "Telefon",
        Key::PrivatePhoneNumber => "Privat",
        Key::OfficePhoneNumber => "Büro",
        Key::MobilePhoneNumber => "Mobil",
        Key::FaxNumber => "Fax",
        Key::Description => "Beschreibung",
        Key::UserDefined | Key::Unspecified => "",
    };
    readable(&a.i18n_display_name)
        .or((!fixed.is_empty()).then_some(fixed))
        .or_else(|| readable(&a.name))
        .unwrap_or(a.name.as_str())
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(name: &str, key: Key, value: &str) -> ContactAttribute {
        ContactAttribute {
            name: name.into(),
            display_key: key as i32,
            value: value.into(),
            ..Default::default()
        }
    }

    fn block(name: &str, attrs: Vec<ContactAttribute>) -> ContactBlock {
        ContactBlock {
            name: name.into(),
            attributes: attrs,
            ..Default::default()
        }
    }

    fn scheme() -> Vec<ContactBlock> {
        vec![
            block(
                "contact",
                vec![
                    attr("firstname", Key::Name, ""),
                    attr("familyname", Key::Surname, ""),
                    attr("company", Key::Company, ""),
                ],
            ),
            block(
                "telephone",
                vec![
                    attr("phone", Key::PhoneNumber, ""),
                    attr("mobile", Key::MobilePhoneNumber, ""),
                ],
            ),
            block("address", vec![attr("city", Key::City, "")]),
        ]
    }

    fn labeled(blocks: Vec<ContactBlock>) -> Vec<(String, ContactBlock)> {
        let names = ["Kontakt", "Rufnummern"];
        blocks
            .into_iter()
            .enumerate()
            .map(|(i, b)| {
                let label = names
                    .get(i)
                    .map_or_else(|| detail_label(&b), |n| (*n).into());
                (label, b)
            })
            .collect()
    }

    #[test]
    fn form_follows_scheme_with_current_values() {
        let current = vec![block(
            "telephone",
            vec![attr("mobile", Key::MobilePhoneNumber, "079")],
        )];
        let f = fields(&labeled(scheme()), &current);
        assert_eq!(f.len(), 6);
        assert_eq!(f[0].label, "Vorname");
        assert_eq!(f[0].group, "Kontakt");
        assert_eq!(f[4].group, "Rufnummern");
        assert_eq!(f[4].value, "079");
        assert_eq!(f[5].group, "Address");
        assert_eq!(f[5].label, "Ort");
    }

    #[test]
    fn apply_fills_and_adds_missing_parts() {
        let mut f = fields(&labeled(scheme()), &[]);
        f[1].value = " Muster ".into();
        f[3].value = "+41 44 000 00 00".into();
        // Bestehender Kontakt hat nur den Kontakt-Block
        let mut blocks = vec![block("contact", vec![attr("firstname", Key::Name, "Max")])];
        apply(&mut blocks, &scheme(), &f);
        assert_eq!(blocks.len(), 2);
        let contact = &blocks[0].attributes;
        // Leerer Vorname im Formular löscht den Wert
        assert_eq!(contact[0].value, "");
        assert_eq!(contact[1].name, "familyname");
        assert_eq!(contact[1].value, "Muster");
        assert_eq!(blocks[1].name, "telephone");
        assert_eq!(blocks[1].attributes.len(), 1);
        assert_eq!(blocks[1].attributes[0].value, "+41 44 000 00 00");
    }

    #[test]
    fn translation_keys_are_not_labels() {
        let mut a = attr("x", Key::Email, "");
        a.i18n_display_name = "de.vertico.contact.email".into();
        assert_eq!(attr_label(&a), "E-Mail");
        a.i18n_display_name = "E-Mail geschäftlich".into();
        assert_eq!(attr_label(&a), "E-Mail geschäftlich");
        let u = attr("Kundennummer", Key::UserDefined, "");
        assert_eq!(attr_label(&u), "Kundennummer");
    }

    #[test]
    fn same_attribute_in_two_blocks_shows_once_and_writes_both() {
        let mut s = scheme();
        s.push(block(
            "details",
            vec![
                attr("phone", Key::PhoneNumber, ""),
                attr("fax", Key::FaxNumber, ""),
            ],
        ));
        let mut f = fields(&labeled(s.clone()), &[]);
        assert_eq!(f.iter().filter(|x| x.name == "phone").count(), 1);
        assert_eq!(f.len(), 7);
        let phone = f.iter_mut().find(|x| x.name == "phone").unwrap();
        assert_eq!(phone.group, "Rufnummern");
        phone.value = "0041765101286".into();
        let mut blocks = s.clone();
        apply(&mut blocks, &s, &f);
        let phones: Vec<_> = blocks
            .iter()
            .flat_map(|b| &b.attributes)
            .filter(|a| a.name == "phone")
            .map(|a| a.value.as_str())
            .collect();
        assert_eq!(phones, ["0041765101286", "0041765101286"]);
        // Wert aus dem zweiten Block wird beim Laden gefunden
        let current = vec![block(
            "details",
            vec![attr("phone", Key::PhoneNumber, "044")],
        )];
        let f = fields(&labeled(s), &current);
        assert_eq!(f.iter().find(|x| x.name == "phone").unwrap().value, "044");
    }

    #[test]
    fn surname_or_company_required() {
        let mut f = fields(&labeled(scheme()), &[]);
        assert!(missing_name(&f));
        f[3].value = "079".into();
        assert!(missing_name(&f));
        f[2].value = "Muster AG".into();
        assert!(!missing_name(&f));
        let other = vec![Field {
            block: "x".into(),
            group: "x".into(),
            name: "note".into(),
            key: Key::Note as i32,
            label: "Notiz".into(),
            value: "".into(),
        }];
        assert!(missing_name(&other));
    }
}
