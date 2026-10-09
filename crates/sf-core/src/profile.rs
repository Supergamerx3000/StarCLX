//! Eigenes Konto über die REST-API der Anlage: Benutzerbild, Passwort und
//! Lizenztyp.
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Me {
    id: i64,
    #[serde(default)]
    license_type: Option<String>,
}

impl Rest {
    /// Eigener Benutzer aus `/rest/users/me`
    async fn me(&self) -> Result<Me, BoxError> {
        let resp = check(
            self.req(reqwest::Method::GET, "/rest/users/me")?
                .send()
                .await?,
        )
        .await?;
        Ok(resp.json::<Me>().await?)
    }

    /// Eigene REST-User-ID
    async fn me_id(&self) -> Result<i64, BoxError> {
        Ok(self.me().await?.id)
    }

    /// Lizenztyp des eigenen Kontos, etwa `USER` oder `USERLIGHT`;
    /// `None`, wenn die Anlage keinen liefert.
    pub async fn license_type(&self) -> Result<Option<String>, BoxError> {
        Ok(self.me().await?.license_type.filter(|l| !l.is_empty()))
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
    fn me_reads_license_type() {
        let me: Me =
            serde_json::from_str(r#"{"id":7,"login":"moe","licenseType":"USERLIGHT"}"#).unwrap();
        assert_eq!(me.id, 7);
        assert_eq!(me.license_type.as_deref(), Some("USERLIGHT"));
        let me: Me = serde_json::from_str(r#"{"id":7}"#).unwrap();
        assert!(me.license_type.is_none());
    }

    #[test]
    fn outcome_serializes_with_kind() {
        assert_eq!(
            serde_json::to_value(PasswordChange::WrongCurrent).unwrap(),
            serde_json::json!({ "kind": "wrong_current" })
        );
    }
}
