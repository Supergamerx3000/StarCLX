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

### Paketquelle (empfohlen, mit automatischen Updates)

Debian, Ubuntu und Abkömmlinge:

```sh
curl -fsSL https://supergamerx3000.github.io/StarCLX/starclx.gpg | sudo tee /usr/share/keyrings/starclx.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/starclx.gpg] https://supergamerx3000.github.io/StarCLX/deb stable main" \
  | sudo tee /etc/apt/sources.list.d/starclx.list
sudo apt update && sudo apt install starclx
```

Fedora (und andere RPM-Distributionen mit dnf):

```sh
sudo curl -fsSL -o /etc/yum.repos.d/starclx.repo https://supergamerx3000.github.io/StarCLX/starclx.repo
sudo dnf install starclx
```

Die Quellen sind signiert; neue Versionen kommen mit dem normalen System-Update.

### Einzelne Pakete

Fertige Pakete gibt es auch unter [Releases](../../releases): `.deb`, `.rpm`, AppImage, `.dmg`
(macOS) und Installer (Windows).

```sh
sudo apt install ./starclx_<version>_amd64.deb
sudo dnf install ./starclx-<version>-1.x86_64.rpm
```

oder ohne Installation das AppImage ausführbar machen und starten. Das Paket ersetzt ältere Builds
unter dem Namen `starface-linuxclient` und registriert die Link-Handler `starface-app://`
(Rückkehr vom Browser-Login) sowie `tel:`, `callto:` und `sip:`.

Zwischenstände: Jeder Build auf `main` legt unter **Actions** das Artefakt
`starclx-<version>-linux-x86_64` ab (Version `1.0.0+<Laufnummer>`).

### macOS und Windows

StarCLX läuft auch unter macOS (Apple Silicon) und Windows (x64) und lässt sich **neben der
offiziellen STARFACE-App** verwenden:

- Eigenes Telefon auf der Anlage: StarCLX meldet sich mit der Geräte-ID des Linux-Clients an,
  die offizielle App mit ihrer eigenen. Beide klingeln parallel.
- Eigener Eintrag im Schlüsselbund bzw. in der Windows-Anmeldeinformationsverwaltung
  (`starface-linuxclient`) und eigene App-Kennung.
- Die Anmeldung läuft immer im eigenen Fenster; `starface-app://` gehört der offiziellen App und
  wird von StarCLX dort nicht registriert. „Im Browser anmelden“ fehlt deshalb.
- `tel:`, `callto:` und `sip:` sind angemeldet; welche App sie öffnet, legt das System fest.
- Systemweite Tastenkürzel meldet die App selbst an (Einstellungen → Hotkeys). „Markierte
  Rufnummer wählen“ nimmt dort die Zwischenablage, eine Markierung wie unter Linux gibt es nicht.
- Selbstsignierte Zertifikate lassen sich wie unter Linux einmalig bestätigen.
- G.722 kommt hier aus der mitgelieferten libg722 statt aus spandsp.

Fertige Pakete für alle drei Systeme gibt es unter [Releases](../../releases); jeder Build auf
`main` legt sie außerdem unter **Actions** als Artefakte ab.

Selbst bauen unter macOS (Homebrew):

```sh
brew install cmake openssl@3
git submodule update --init --recursive
cd apps/desktop && npm install && npm run tauri build
```

Unter Windows (Visual Studio mit C++, CMake, Rust, Node 22):

```powershell
vcpkg install openssl:x64-windows-static-md
$env:OPENSSL_DIR = "<vcpkg>\installed\x64-windows-static-md"
git submodule update --init --recursive
cd apps/desktop; npm install; npm run tauri build
```

Beim ersten Anruf fragt macOS nach dem Mikrofon.

Die Mac-Builds sind nur ad-hoc signiert (keine Apple-Developer-ID). Nach dem Download einmalig
in Systemeinstellungen → Datenschutz & Sicherheit „Trotzdem öffnen“ wählen, oder im Terminal:
`xattr -dr com.apple.quarantine /Applications/StarCLX.app`

### Voraussetzungen auf der Anlage

- STARFACE 10 mit erreichbarem OneHub-Port 9092 und SIP/TLS
- Telefon-Typ des Benutzers: „UCC Client for Linux“ (wird bei der ersten Anmeldung angelegt)
- Für Call2Go eine hinterlegte iFMC-Nummer

## Neue Version veröffentlichen

Version in `Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json` und `apps/desktop/package.json`
anheben, Abschnitt in `CHANGELOG.md` ergänzen und mergen. Der erste Build auf `main` mit der neuen
Version legt das GitHub-Release `v<Version>` mit Paketen und dem CHANGELOG-Abschnitt an. Ein
Tag `v<Version>` von Hand bewirkt dasselbe.

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
| `crates/sf-audio` | Audiogeräte, Klingeltöne, Testton (PulseAudio/PipeWire; macOS/Windows über cpal) |
| `crates/sf-chat` | XMPP-Chat |
| `crates/sf-busylight` | Kuando Busylight (Linux hidraw, macOS/Windows hidapi) |
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
