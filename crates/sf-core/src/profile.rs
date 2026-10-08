//! Eigenes Konto über die REST-API der Anlage: Benutzerbild und Passwort.
//! OneHub kann das Passwort nicht ändern; beides geht deshalb über
//! `/rest/users/{id}` mit dem Token der Sitzung.

use serde::{Deserialize, Serialize};

use crate::fkeys::{Rest, check};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Ergebnis einer Passwortänderung
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PasswordChange {
    Changed,
    /// Die Anlage hat das aktuelle Passwort abgelehnt
    WrongCurrent,
    /// Das neue Passwort verletzt die Passwortrichtlinie der Anlage
    /// (Texte kommen lokalisiert von der Anlage)
    Policy {
        message: String,
        violations: Vec<String>,
    },
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct PolicyCheck {
    policy_message: String,
    violations: Vec<String>,
}

impl Rest {
    /// Eigene REST-User-ID
    async fn me_id(&self) -> Result<i64, BoxError> {
        #[derive(Deserialize)]
        struct Me {
            id: i64,
        }
        let resp = check(
            self.req(reqwest::Method::GET, "/rest/users/me")?
                .send()
                .await?,
        )
        .await?;
        Ok(resp.json::<Me>().await?.id)
    }

    /// Setzt das eigene Benutzerbild (PNG, JPEG oder GIF).
    pub async fn set_avatar(&self, image: Vec<u8>, mime: &str) -> Result<(), BoxError> {
        let path = format!("/rest/users/{}/avatar", self.me_id().await?);
        let req = self
            .req(reqwest::Method::PUT, &path)?
            .header(reqwest::header::CONTENT_TYPE, mime)
            .body(image);
        check(req.send().await?).await?;
        Ok(())
    }

    /// Entfernt das eigene Benutzerbild.
    pub async fn delete_avatar(&self) -> Result<(), BoxError> {
        let path = format!("/rest/users/{}/avatar", self.me_id().await?);
        check(self.req(reqwest::Method::DELETE, &path)?.send().await?).await?;
        Ok(())
    }

    /// Ändert das eigene Passwort. Prüft das neue zuerst gegen die
    /// Passwortrichtlinie, damit die Anlage sagen kann, was fehlt.
    pub async fn change_password(
        &self,
        current: &str,
        new: &str,
    ) -> Result<PasswordChange, BoxError> {
        let resp = self
            .req(reqwest::Method::PUT, "/rest/users/password-policy-check")?
            .json(&serde_json::json!({ "password": new }))
            .send()
            .await?;
        let policy: PolicyCheck = check(resp).await?.json().await?;
        if !policy.violations.is_empty() {
            return Ok(PasswordChange::Policy {
                message: policy.policy_message,
                violations: policy.violations,
            });
        }
        let path = format!("/rest/users/{}/password", self.me_id().await?);
        let resp = self
            .req(reqwest::Method::POST, &path)?
            .json(&update_body(current, new))
            .send()
            .await?;
        // Laut API heisst 401 hier: aktuelles Passwort falsch
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Ok(PasswordChange::WrongCurrent);
        }
        check(resp).await?;
        Ok(PasswordChange::Changed)
    }
}

fn update_body(current: &str, new: &str) -> serde_json::Value {
    serde_json::json!({
        "currentPassword": current,
        "newPassword": new,
        "sendCredentialsEmail": false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_body_uses_api_field_names() {
        assert_eq!(
            update_body("alt", "neu"),
            serde_json::json!({ "currentPassword": "alt", "newPassword": "neu", "sendCredentialsEmail": false })
        );
    }

    #[test]
    fn policy_check_tolerates_missing_fields() {
        let p: PolicyCheck = serde_json::from_str(r#"{"violations":["Zu kurz"]}"#).unwrap();
        assert_eq!(p.violations, vec!["Zu kurz"]);
        assert!(p.policy_message.is_empty());
    }

    #[test]
    fn outcome_serializes_with_kind() {
        assert_eq!(
            serde_json::to_value(PasswordChange::WrongCurrent).unwrap(),
            serde_json::json!({ "kind": "wrong_current" })
        );
    }
}
