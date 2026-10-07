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
curl -fsSL https://crazmoe.github.io/StarCLX/starclx.gpg | sudo tee /usr/share/keyrings/starclx.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/starclx.gpg] https://crazmoe.github.io/StarCLX/deb stable main" \
  | sudo tee /etc/apt/sources.list.d/starclx.list
sudo apt update && sudo apt install starclx
```

Fedora (und andere RPM-Distributionen mit dnf):

```sh
sudo curl -fsSL -o /etc/yum.repos.d/starclx.repo https://crazmoe.github.io/StarCLX/starclx.repo
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

### Flatpak

Jeder Build legt zusätzlich das Artefakt `starclx-<version>-flatpak` ab, Releases enthalten
`starclx_<version>_x86_64.flatpak`. Installieren (holt die GNOME-Laufzeit von Flathub dazu):

```sh
flatpak install --user ./starclx_<version>_x86_64.flatpak
flatpak run ch.crazmoe.StarCLX
```

Selbst bauen aus einem `.deb` (ohne Argument das neueste Release):
`packaging/flatpak/build.sh [starclx_<version>_amd64.deb]`.

Das Flatpak hat dieselben Funktionen wie das `.deb` und teilt mit ihm Einstellungen und
bestätigte Zertifikate. Es darf dafür Programme auf dem System starten (Programm bei Anruf,
GNOME-Tastenkürzel, `tel:`-Links als Standard), die Sandbox schützt also kaum mehr als beim `.deb`.
Unterschiede:

- **Autostart** läuft über die Desktop-Umgebung; GNOME fragt beim Einschalten einmal nach.
- **`tel:`-Links abschalten** geht nicht in StarCLX, ein anderes Programm wählt man in den
  Systemeinstellungen unter Standardanwendungen.
- **Busylight** braucht die udev-Regel auf dem System, einmalig:
  `flatpak run --command=cat ch.crazmoe.StarCLX /app/share/starclx/60-starclx-busylight.rules | sudo tee /etc/udev/rules.d/60-starclx-busylight.rules`
- Eine eigene Firmen-CA, die nur im System hinterlegt ist, kann im Flatpak fehlen;
  selbstsignierte Zertifikate lokaler Anlagen bestätigt man wie gewohnt in StarCLX.
- Updates kommen nicht automatisch, sondern mit dem nächsten `.flatpak`.

### Flathub

`packaging/flathub` ist die Variante für Flathub (App-ID `io.github.crazmoe.StarCLX`). Sie wird
ohne Netz aus dem Quellcode gebaut und hat weniger Rechte als das eigene Flatpak: Programme bei
Anruf starten und GNOME-Tastenkürzel automatisch eintragen geht dort nicht (URLs bei Anruf,
Autostart und `tel:`-Links schon). Die CI baut sie bei jedem Lauf; das Artefakt `starclx-flathub`
enthält das Paket und unter `submission/` die Dateien für das Flathub-Repo.

Einreichen (einmalig):

1. Release-Tag anlegen und `packaging/flathub/prepare.sh --submission v<version>` ausführen
   (oder `submission/` aus dem Artefakt des Release-Builds nehmen).
2. `github.com/flathub/flathub` forken, Branch vom Branch `new-pr` anlegen, den Inhalt von
   `submission/` hineinlegen und `git submodule add https://github.com/flathub/shared-modules.git`.
3. Pull Request gegen `new-pr` öffnen; die Prüfer von Flathub melden sich dort.

Danach liegt die App in einem eigenen Repo `github.com/flathub/io.github.crazmoe.StarCLX`; neue
Versionen: dort Tag, Commit und die neu erzeugten `*-sources.json` per Pull Request eintragen.

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

## Lizenz

MIT, siehe [LICENSE](LICENSE).
