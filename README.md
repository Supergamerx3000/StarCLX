# StarCLX

StarCLX ist ein freier Desktop-Client für STARFACE-Telefonanlagen, mit Linux als Hauptplattform.

Nicht mit der STARFACE GmbH verbunden. „STARFACE“ wird nur beschreibend verwendet.

## Installieren (ohne selbst zu kompilieren)

Jeder Build auf GitHub erzeugt fertige Pakete. Unter **Actions** den neuesten Lauf öffnen, das Artefakt
`starclx-<version>-linux-x86_64` herunterladen und entpacken:

```sh
sudo apt install ./starclx_*_amd64.deb
```

Das Paket registriert den Link-Handler `starface-app://`, über den der Browser-Login zur App zurückkehrt.

## Aufbau

| Pfad | Inhalt |
|---|---|
| `proto/` | Aus der STARFACE App für Windows rekonstruierte OneHub-Protos (Version in `proto/VERSION`) |
| `crates/sf-proto` | Daraus generierte gRPC-Client-Stubs |
| `crates/sf-auth` | OAuth2-Login (Authorization Code + PKCE, Refresh) |
| `crates/sf-onehub` | Verbindung zur OneHub-API (Port 9092) mit Bearer-Token |
| `crates/sf-core` | Sitzung: Refresh-Token im Schlüsselbund, automatische Token-Erneuerung |
| `apps/desktop` | Desktop-App: Tauri 2, Oberfläche in Svelte 5 / TypeScript |
| `apps/sfctl` | Kommandozeile für Tests und Skripte |

Geplant: `sf-sip` (Softphone mit libbaresip), `sf-chat` (XMPP), `sf-store` (SQLCipher).

## Entwickeln

Voraussetzungen: Rust (stable), Node 22, unter Debian/Ubuntu
`libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev`.

```sh
cargo test --workspace
cd apps/desktop && npm install && npm run tauri dev
```

`sfctl` gegen eine Anlage (Password-Grant, braucht das Recht „API access with Password Grant“):

```sh
export SF_SERVER=https://anlage.example.com SF_USER=… SF_PASSWORD=…
cargo run -p sfctl -- version
cargo run -p sfctl -- phones
cargo run -p sfctl -- call 12
```
