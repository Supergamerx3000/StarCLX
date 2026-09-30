//! Gegen den echten Secret Service. Braucht einen entsperrten Schlüsselbund:
//! `cargo test -p sf-auth --test system_keyring -- --ignored`

// Jeder Test nutzt einen eigenen Eintrag; die Tests laufen parallel.
fn round_trip(server: &str) {
    sf_auth::secret::init_system_store().unwrap();
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

#[test]
#[ignore = "braucht einen laufenden Secret Service"]
fn round_trip_with_system_keyring() {
    round_trip("https://sf-auth-test.invalid");
}

/// So ruft die App den Schlüsselbund auf: aus einem tokio-Runtime heraus.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "braucht einen laufenden Secret Service"]
async fn round_trip_from_tokio_runtime() {
    tokio::task::spawn_blocking(|| round_trip("https://sf-auth-test-rt.invalid"))
        .await
        .unwrap();
}
