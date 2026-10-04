//! Refresh-Token im Schlüsselbund des Systems (unter Linux Secret Service,
//! also gnome-keyring oder KWallet; unter macOS der Anmelde-Schlüsselbund,
//! unter Windows die Anmeldeinformationsverwaltung).
//!
//! Die Aufrufe blockieren; aus async-Code heraus über
//! `tokio::task::spawn_blocking` verwenden.

use keyring_core::Entry;

use crate::Result;

const SERVICE: &str = "starface-linuxclient";

/// Richtet den Schlüsselbund des Systems ein. Einmal beim Start aufrufen.
#[cfg(target_os = "linux")]
pub fn init_system_store() -> Result<()> {
    keyring_core::set_default_store(zbus_secret_service_keyring_store::Store::new()?);
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn init_system_store() -> Result<()> {
    keyring_core::set_default_store(apple_native_keyring_store::keychain::Store::new()?);
    Ok(())
}

#[cfg(windows)]
pub fn init_system_store() -> Result<()> {
    keyring_core::set_default_store(windows_native_keyring_store::Store::new()?);
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn init_system_store() -> Result<()> {
    Err(keyring_core::Error::NoDefaultStore.into())
}

fn entry(server: &str) -> Result<Entry> {
    Ok(Entry::new(SERVICE, server)?)
}

pub fn save_refresh_token(server: &str, token: &str) -> Result<()> {
    entry(server)?.set_password(token)?;
    Ok(())
}

/// `None`, wenn für diese Anlage nichts gespeichert ist.
pub fn load_refresh_token(server: &str) -> Result<Option<String>> {
    match entry(server)?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring_core::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn delete_refresh_token(server: &str) -> Result<()> {
    match entry(server)?.delete_credential() {
        Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_mock_store() {
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        let server = "https://pbx.example.com";
        assert_eq!(load_refresh_token(server).unwrap(), None);
        save_refresh_token(server, "rt-1").unwrap();
        save_refresh_token(server, "rt-2").unwrap();
        assert_eq!(load_refresh_token(server).unwrap().as_deref(), Some("rt-2"));
        delete_refresh_token(server).unwrap();
        delete_refresh_token(server).unwrap();
        assert_eq!(load_refresh_token(server).unwrap(), None);
    }
}
