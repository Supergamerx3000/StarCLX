# Architektur und Tech-Stack: STARFACE-Client für Linux

Stand: 30.09.2026, Geräte-Identität korrigiert am 02.10.2026. Grundlage: [windows-client-analyse.md](windows-client-analyse.md) und der erfolgreiche Durchstich auf Moes Rechner (gRPC, SIP/TLS mit SRTP über baresip, XMPP, Browser-Login).

Vorgabe von Moe: Cross-Platform wäre schön, **Linux hat Vorrang vor allem anderen.**

## 1. Entscheidung in einem Satz

**Kern in Rust, Oberfläche mit Tauri 2 (Web-UI mit Svelte + TypeScript), Softphone mit libbaresip eingebettet, gRPC mit tonic, Chat mit tokio-xmpp.** Damit läuft Linux zuerst und vollwertig, Windows und macOS kosten später nur Build- und Paketierungsaufwand, keinen zweiten Code.

## 2. Warum dieser Stack

Die Anlage erledigt die schwere Arbeit: Anrufsteuerung, Halten, Übergabe und Konferenz laufen per gRPC, das Softphone ist nur ein Audio-Endgerät mit Auto-Answer. Der Client ist deshalb im Kern ein **gRPC-Event-Client mit angeschlossenem SIP-Audio und XMPP-Chat**. Das bestimmt die Wahl:

| Baustein | Wahl | Begründung |
|---|---|---|
| Sprache Kern | **Rust** (tokio) | Viele parallele Langläufer-Streams (je Dienst ein `Subscribe…Events`, XMPP, SIP) sauber asynchron; speichersicher; ein Binary pro Plattform |
| gRPC | **tonic 0.14** + `tonic-build` aus den 95 `.proto` | Server-Streaming erstklassig, TLS über rustls, Bearer per Interceptor. Protos werden je App-Version neu extrahiert (`_protodump.py`) |
| Softphone | **libbaresip 4.x** per FFI (bindgen) | Im Durchstich mit genau dieser Anlage erprobt (TLS 5061, SRTP/SDES Pflicht, G.722/PCMA/PCMU). **BSD-Lizenz.** Module für PipeWire, Pulse, ALSA, aber auch WASAPI (Windows) und CoreAudio (macOS), dazu Opus, G.722, SRTP |
| Chat | **tokio-xmpp 6 + xmpp-parsers** | STARTTLS, SASL mit Token als Passwort, Roster, Carbons, MUC vorhanden. XEP-0136-Archiv (Openfire) als eigene kleine IQ-Schicht |
| Oberfläche | **Tauri 2** + **Svelte 5 / TypeScript** | Offizielle Plugins decken die Desktop-Integration fast 1:1 ab: `deep-link` (`starface-app://login`, `tel:`, `callto:`, `sip:`), `single-instance`, Tray, Benachrichtigungen, Autostart, `global-shortcut`, Updater. Gleiche UI auf allen Plattformen |
| Login | OAuth2 Code + PKCE im Systembrowser, `client_id=windows-app`, Redirect `starface-app://login`, Scope `pbx-login` | Live verifiziert. Die Client-ID ist nur der Name der OAuth-Freigabe für Desktop-Apps (eine eigene Linux-Client-ID bietet die Anlage nicht an) und bestimmt nicht den Gerätetyp. Refresh-Token (90 Tage) im **Secret Service** (gnome-keyring/KWallet) über die `keyring`-Crate, nie im Klartext |
| Softphone-Gerät | `RegisterSipDevice` mit der Geräte-ID des **Linux-Clients** `D4CC1516-EC90-42C7-8F70-B0853B173232` (Anlagen-Typ LinuxClient, 152) | Seit PR #5. Die Anlage legt dafür ein eigenes App-Telefon an und unterscheidet StarCLX so vom Windows-Client (`163C00A2-…`) desselben Benutzers. Gilt für die Desktop-App und `sfctl` (gemeinsame Konstante `sf_onehub::SIP_DEVICE_ID`); bei `sfctl` lässt sie sich zu Testzwecken mit `--sip-device-id` bzw. `SF_SIP_DEVICE_ID` überschreiben |
| Lokale Daten | SQLite über `rusqlite` mit SQLCipher-Feature | Chatverlauf und Caches (Kontakte, Journal) verschlüsselt, wie im Windows-Client |
| Linux-Spezifika | `ashpd` (xdg-desktop-portal) | Globale Hotkeys unter Wayland (GlobalShortcuts-Portal), Dateiauswahl, Hintergrund/Autostart in Flatpak |

## 3. Verworfene Alternativen

| Alternative | Warum nicht |
|---|---|
| **C++ mit Qt 6/QML + PJSIP + QXmpp** | Sehr nativ unter Linux und ebenfalls Cross-Platform. Aber **PJSIP ist GPLv2** (oder kommerziell) und zwingt das ganze Projekt unter GPL; C++ ist bei vielen parallelen Streams fehleranfälliger. Zweitbeste Wahl, falls die Web-UI unter Linux nicht überzeugt |
| **Python + PySide6** (Prototyp weiterbauen) | Schnellster Start, der Prototyp existiert. Aber Paketierung für Windows/macOS mühsam, Laufzeitfehler statt Compilerfehler bei 304 RPCs, Audio-Anbindung nur über Subprozess oder SWIG. Der Prototyp bleibt als **Referenz und Testwerkzeug** |
| **GTK4/libadwaita (Rust)** | Schönste GNOME-Integration, aber unter Windows/macOS schwach. Widerspricht dem Cross-Platform-Wunsch |
| **Electron** | Chromium im Paket (~150 MB+ RAM), bringt gegenüber Tauri nur bei WebKitGTK-Problemen etwas. Notausgang, falls WebKitGTK zickt |

## 4. Architektur

```
┌──────────────────────── Tauri-App (ein Prozess) ────────────────────────┐
│  Web-UI (Svelte/TS)   Workspaces: Anrufe, Journal, Kontakte, Chat, …    │
│        ▲  Events (typisiert, tauri-specta)   │ Commands                  │
│        │                                     ▼                          │
│  sf-core  ─ Sitzung, Zustand (Anrufe, Präsenz, Journal …), Event-Bus    │
│    ├─ sf-auth    OIDC/PKCE, Token-Refresh, Keyring                       │
│    ├─ sf-onehub  gRPC-Clients + Event-Streams, RegisterSipDevice (Linux)│
│    ├─ sf-sip     libbaresip: TLS-Registrierung, SRTP, Auto-Answer, Audio│
│    ├─ sf-chat    XMPP: Roster, Präsenz, 1:1, MUC, Carbons, Archiv       │
│    └─ sf-store   SQLCipher: Chatverlauf, Caches, Einstellungen          │
└──────────────────────────────────────────────────────────────────────────┘
      │ gRPC 9092 (TLS, Bearer)   │ SIP/TLS 5061 + SRTP   │ XMPP 5222 STARTTLS
      └──────────────────────── STARFACE-Anlage ────────────────────────────┘
```

Grundsätze:

1. **Anlage ist die Wahrheit.** Der Client hält nur einen Spiegel aus gRPC-Events; Aktionen gehen als gRPC-Aufruf raus, die UI reagiert auf das Event, nicht auf den eigenen Aufruf. So bleiben Tischtelefon, Softphone und andere Clients konsistent.
2. **Softphone ist dumm.** `sf-sip` kennt nur Registrieren, Annehmen, Auflegen, Stumm, DTMF, Gerätewahl. Anrufsteuerung (PlaceCall, Hold, Transfer, Konferenz) läuft ausschließlich über `CallService`. Ausgehende Anrufe: `PlaceCall` → Anlage ruft das Softphone → Auto-Answer.
3. **Kern ohne UI nutzbar.** Ein zweites Binary `sfctl` nutzt denselben Kern für Kommandozeile (wählen, Status, Fax-Batch) und für automatisierte Tests gegen die Testanlage.
4. **API-Drift abfangen.** Beim Start `SystemService.GetServerVersion`; bekannte Versionen sind getestet, unbekannte laufen mit Warnung. Protos je Windows-App-Version im Repo versioniert.

## 5. Projektgerüst (Cargo-Workspace)

```
starface-linux/
├── proto/                 # extrahierte .proto (Version im Ordnernamen), _protodump.py
├── crates/
│   ├── sf-proto/          # tonic-build, generierte Stubs
│   ├── sf-auth/
│   ├── sf-onehub/
│   ├── sf-sip/            # + baresip-sys (bindgen, statisch gelinkt)
│   ├── sf-chat/
│   ├── sf-store/
│   └── sf-core/
├── apps/
│   ├── desktop/           # Tauri 2 (src-tauri) + ui/ (Svelte 5, Vite)
│   └── sfctl/             # CLI
├── packaging/             # Flatpak-Manifest, .desktop, AppStream, deb
└── .github/workflows/     # Build, Tests, Flatpak/deb/AppImage
```

## 6. Paketierung

| Ziel | Format | Wann |
|---|---|---|
| Zorin/Ubuntu/Debian | `.deb` (Tauri-Bundler) | ab erstem Meilenstein |
| Alle Distros | **Flatpak** (GNOME-Runtime, WebKitGTK inklusive), später Flathub | ab Meilenstein 2 |
| Schnelltest | AppImage | nebenbei |
| Windows / macOS | MSI / DMG (Tauri-Bundler), baresip mit WASAPI/CoreAudio | nach Feature-Parität unter Linux, nur wenn gewünscht |

## 7. Meilensteine

1. **Anmelden:** Browser-Login, Token im Keyring, Refresh, Hauptfenster mit eigenem Profil (MeService), Tray.
2. **Telefonieren:** RegisterSipDevice, baresip-Registrierung, wählen (PlaceCall), eingehende Anrufe mit Benachrichtigung, Annehmen/Auflegen/Halten/Übergabe, Audio-Geräte, `tel:`-Links.
3. **Arbeiten:** Journal, Kontaktsuche und Rufnummernauflösung, Präsenz und Favoriten, Ruhe, Umleitungen.
4. **Chat:** 1:1, Gruppenchat, Verlauf (SQLCipher + Archiv), Dateien.
5. **Rest:** Voicemail, Fax (inkl. CUPS-Backend), Gruppen/Queues, Konferenzen, Aufzeichnungen, KI-Funktionen, NEON (im Browser), Headset-Tasten.

## 8. Bekannte Risiken

| Risiko | Gegenmaßnahme |
|---|---|
| gRPC-API intern und nicht öffentlich | Protos je Version extrahieren, Versionsprüfung, Integrationstests gegen die Testanlage |
| Anmeldung nutzt die OAuth-Client-ID `windows-app` | Technisch der einzige Weg, der funktioniert (eigene Loopback-Redirects werden abgelehnt). Das Softphone meldet sich trotzdem als Linux-Client an. Später bei STARFACE nach einer eigenen Client-ID fragen |
| WebKitGTK unter manchen GPU/Wayland-Kombis träge | UI schlank halten; Electron oder Qt als Ausweg, Kern bleibt gleich |
| Tray unter GNOME nur mit AppIndicator-Erweiterung | Zorin bringt sie mit; sonst Fenster statt Tray |
| Globale Hotkeys unter Wayland | GlobalShortcuts-Portal; unter X11 direkt |
| Markenrecht | Kein STARFACE-Logo, neutraler App-Name; „für STARFACE“ nur beschreibend |
