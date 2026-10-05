# Änderungen

## 1.2.2 – 2026-10-05

### Pakete
- RPM-Paket für Fedora, openSUSE und andere RPM-Distributionen
- Signierte Paketquellen für apt (Debian/Ubuntu) und dnf (Fedora) auf GitHub Pages, Updates
  kommen mit dem normalen System-Update

## 1.2.1 – 2026-10-05

### Autostart
- macOS: der Autostart startet das App-Bundle über `open` wie der Finder. Läuft StarCLX noch aus
  der DMG oder direkt aus „Downloads“ (zufälliger Pfad durch macOS), kommt ein Hinweis, die App
  erst nach „Programme“ zu verschieben, statt eines Eintrags, der nach dem Neustart ins Leere geht
- Tests für alle Plattformen: Linux (Autostart-Datei), macOS (gültige Property-List), Windows
  (Registry-Eintrag anlegen, lesen, löschen)

## 1.2.0 – 2026-10-05

### Funktionstasten
- „Gruppe An-/Abmelden“ funktioniert: ein Klick meldet in der Gruppe an bzw. ab, die Lampe
  zeigt den Stand (grün angemeldet, leer abgemeldet) und folgt Änderungen auch von anderen
  Telefonen oder Clients

## 1.1.1 – 2026-10-05

### Verbindung nach Standby
Blieb StarCLX über Nacht offen und wurde der Rechner zugeklappt, fehlten danach Funktionstasten,
neue Anrufe, Rufliste und Voicemail, weil tote Verbindungen nicht bemerkt wurden.

- Verbindung zur Anlage mit Keepalive (HTTP/2 und TCP): abgebrochene Verbindungen fallen nach
  spätestens etwa 30 Sekunden auf und werden neu aufgebaut
- Aufwachen aus dem Standby wird erkannt: Anmeldung (Token) sofort erneuern, Softphone neu
  verbinden und registrieren, Funktionstasten, Voicemail und Umleitungen neu laden
- Funktionstasten versuchen es nach einem Ladefehler selbst erneut, statt leer zu bleiben

## 1.1.0 – 2026-10-05

StarCLX läuft jetzt auch unter macOS und Windows, neben der offiziellen STARFACE-App.

### macOS und Windows
- Softphone über CoreAudio bzw. WASAPI, G.722 über die mitgelieferte libg722
- Audiogeräte, Klingeltöne und Mikrofontest über cpal
- Anmeldung im Schlüsselbund bzw. in der Windows-Anmeldeinformationsverwaltung
- Eigenes Telefon auf der Anlage (Geräte-ID des Linux-Clients); `starface-app://` bleibt der
  offiziellen App, die Anmeldung läuft immer im eigenen Fenster
- Bestätigte selbstsignierte Zertifikate auch im Anmeldefenster (WKWebView, WebView2)
- Systemweite Tastenkürzel, Autostart, automatische Abwesenheit, Busylight über hidapi
- Pakete: `.dmg` (Apple Silicon, ad-hoc signiert) und Installer (`.exe`) für Windows x64

### Für alle
- Call Manager: Anrufkarte wie in der STARFACE-App, Karte und Aktionen (Rückfrage, Konferenz,
  Extras) in einer gemeinsamen Schale statt verrutschter Leiste
- Imports der Svelte-Module eindeutig (`….svelte.js`), damit der Build auch auf Dateisystemen
  ohne Groß-/Kleinschreibung klappt

## 1.0.0 – 2026-10-02

Erste vollständige Version von StarCLX, dem Linux-Client für STARFACE-Telefonanlagen.

### Telefonie
- Softphone (SIP/TLS, SRTP) mit Call Manager: annehmen, ablehnen, halten, stumm, Ziffernblock,
  Rückfrage, verbinden, Konferenz, Umleiten, Call2Go
- Signalisierte Rufnummer wählen, Softphone als primäres Telefon
- Erreichbarkeit: Umleitungen (immer, besetzt, keine Antwort), iFMC
- Voicemail abhören, als gehört markieren, löschen
- Funktionstasten anzeigen, auslösen und bearbeiten (BLF mit Heranholen, Parken, Kurzwahl, Gruppen …)

### Kontakte und Rufliste
- Suche beim Tippen über alle Adressbücher, Adressbuch-Reiter mit den Namen der Anlage
- Kontakte anlegen, bearbeiten und löschen; Felder gibt die Anlage vor
- Rufliste mit Filtern, Notizen, „zurückgerufen“ und Benachrichtigung bei verpassten Anrufen
- Unbekannte Nummern aus der Rufliste ins Adressbuch übernehmen; nachträglich eingetragene
  Kontakte erscheinen mit Namen
- Anruf mit Notiz per Chat oder E-Mail an Kollegen weitergeben; Rufnummern im Chat anklickbar

### Chat
- Chat mit Kollegen über XMPP: Präsenz, Verlauf, Dateien, automatische Abwesenheit

### Oberfläche und Desktop
- Arbeitsbereich als Reiter oder frei anordenbare, sperrbare Kacheln
- Sprachen: Deutsch, English, Français, Italiano
- Hell, dunkel oder wie das System; Symbol im Infobereich; Schnellwahl-Fenster
- Tastenkürzel über GNOME, `tel:`/`callto:`/`sip:`-Links, Autostart, URL oder Programm bei Anruf
- Audiogeräte, Klingeltöne, Kuando Busylight

### Anmeldung
- Browser-Login (OAuth2 mit PKCE), Sitzung im Schlüsselbund
- Selbstsignierte Zertifikate lokaler Anlagen einmalig bestätigen; SIP per IP-Adresse
