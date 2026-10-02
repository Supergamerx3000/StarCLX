# StarCLX

StarCLX ist ein freier Desktop-Client für STARFACE-Telefonanlagen, mit Linux als Hauptplattform.
Er spricht dieselben Schnittstellen wie die STARFACE App für Windows (OneHub-gRPC, SIP/TLS mit SRTP,
XMPP-Chat) und läuft mit STARFACE ab Version 10.

Nicht mit der STARFACE GmbH verbunden. „STARFACE“ wird nur beschreibend verwendet.

## Funktionen

- **Anmelden** im Browser (OAuth2 mit PKCE), Sitzung bleibt im Schlüsselbund; selbstsignierte
  Zertifikate lokaler Anlagen lassen sich einmalig bestätigen
- **Softphone** (SIP/TLS, SRTP) mit Call Manager: annehmen, halten, stumm, Ziffernblock, Rückfrage,
  verbinden, Konferenz, Umleiten, Call2Go
- **Suche und Adressbuch**: Suche beim Tippen über alle Adressbücher, Kontakte anlegen, bearbeiten
  und löschen, unbekannte Nummern aus der Rufliste übernehmen
- **Rufliste** mit Filtern, Notizen, „zurückgerufen“, Benachrichtigung bei verpassten Anrufen;
  Anruf mit Notiz per Chat oder E-Mail an Kollegen weitergeben
- **Chat** mit Kollegen (Präsenz, Verlauf, Dateien, Abwesenheit bei Inaktivität)
- **Voicemail** abhören und verwalten
- **Funktionstasten** (BLF, Kurzwahl, Heranholen, Parken, Gruppen, Ruhe …) anzeigen, nutzen und bearbeiten
- **Erreichbarkeit**: Umleitungen, iFMC, signalisierte Rufnummer
- **Arbeitsbereich** als Reiter oder frei angeordnete Kacheln
- **Einstellungen**: Audiogeräte, Klingeltöne, Busylight (Kuando), Erscheinungsbild,
  Sprache (Deutsch, English, Français, Italiano)
- **Desktop-Integration**: Symbol im Infobereich, Autostart, Schnellwahl-Fenster,
  Tastenkürzel (markierte Nummer wählen, annehmen, auflegen), `tel:`/`callto:`/`sip:`-Links,
  URL oder Programm bei Anruf

Was sich seit der letzten Version geändert hat, steht in [CHANGELOG.md](CHANGELOG.md).

## Installieren

Fertige Pakete gibt es unter [Releases](https://github.com/crazmoe/StarCLX/releases):

```sh
sudo apt install ./starclx_<version>_amd64.deb
```

oder ohne Installation das AppImage ausführbar machen und starten. Das Paket ersetzt ältere Builds
unter dem Namen `starface-linuxclient` und registriert die Link-Handler `starface-app://`
(Rückkehr vom Browser-Login) sowie `tel:`, `callto:` und `sip:`.

Zwischenstände: Jeder Build auf `main` legt unter **Actions** das Artefakt
`starclx-<version>-linux-x86_64` ab (Version `1.0.0+<Laufnummer>`).

### Voraussetzungen auf der Anlage

- STARFACE 10 mit erreichbarem OneHub-Port 9092 und SIP/TLS
- Telefon-Typ des Benutzers: „UCC Client for Linux“ (wird bei der ersten Anmeldung angelegt)
- Für Call2Go eine hinterlegte iFMC-Nummer

## Neue Version veröffentlichen

1. Version in `Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json` und `apps/desktop/package.json`
   anheben, Abschnitt in `CHANGELOG.md` ergänzen, mergen.
2. Tag setzen: `git tag v1.2.3 && git push origin v1.2.3`. Die CI baut die Pakete und legt das
   GitHub-Release mit dem CHANGELOG-Abschnitt an.

## Aufbau

| Pfad | Inhalt |
|---|---|
| `proto/` | Aus der STARFACE App für Windows rekonstruierte OneHub-Protos (Version in `proto/VERSION`) |
| `crates/sf-proto` | Daraus generierte gRPC-Client-Stubs |
| `crates/sf-auth` | OAuth2-Login (Authorization Code + PKCE, Refresh), Schlüsselbund |
| `crates/sf-onehub` | Verbindung zur OneHub-API (Port 9092) mit Bearer-Token |
| `crates/sf-tls` | TLS-Konfiguration inkl. bestätigter Zertifikate |
| `crates/sf-core` | Sitzung, Adressbuch, Rufliste, Funktionstasten, Erreichbarkeit, Voicemail |
| `crates/sf-sip` | Softphone auf Basis von libbaresip |
| `crates/sf-audio` | Audiogeräte, Klingeltöne, Testton (PulseAudio/PipeWire) |
| `crates/sf-chat` | XMPP-Chat |
| `crates/sf-busylight` | Kuando Busylight über hidraw |
| `apps/desktop` | Desktop-App: Tauri 2, Oberfläche in Svelte 5 / TypeScript |
| `apps/sfctl` | Kommandozeile für Tests und Skripte |

## Entwickeln

Voraussetzungen: Rust (stable), Node 22, unter Debian/Ubuntu
`libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev`
(vollständige Liste im Schritt „Systempakete“ von `.github/workflows/ci.yml`).

```sh
cargo test --workspace
cd apps/desktop && npm install && npm run tauri dev
```

Texte der Oberfläche laufen über `t()` (deutscher Text als Schlüssel); Übersetzungen stehen in
`apps/desktop/src/lib/i18n/`. `npm run i18n:check` meldet fehlende Einträge.

`sfctl` gegen eine Anlage (Password-Grant, braucht das Recht „API access with Password Grant“):

```sh
export SF_SERVER=https://anlage.example.com SF_USER=… SF_PASSWORD=…
cargo run -p sfctl -- version
cargo run -p sfctl -- phones
cargo run -p sfctl -- call 12
```
