//! Gegen den echten Secret Service. Braucht einen entsperrten Schlüsselbund:
//! `cargo test -p sf-auth --test system_keyring -- --ignored`

#[test]
#[ignore = "braucht einen laufenden Secret Service"]
fn round_trip_with_system_keyring() {
    sf_auth::secret::init_system_store().unwrap();
    let server = "https://sf-auth-test.invalid";
    sf_auth::secret::save_refresh_token(server, "rt-test").unwrap();
    assert_eq!(
        sf_auth::secret::load_refresh_token(server)
            .unwrap()
            .as_deref(),
        Some("rt-test")
    );
    sf_auth::secret::delete_refresh_token(server).unwrap();
    assert_eq!(sf_auth::secret::load_refresh_token(server).unwrap(), None);
}

/// So ruft die App den Schlüsselbund auf: aus einem tokio-Runtime heraus.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "braucht einen laufenden Secret Service"]
async fn round_trip_from_tokio_runtime() {
    tokio::task::spawn_blocking(round_trip_with_system_keyring)
        .await
        .unwrap();
}
