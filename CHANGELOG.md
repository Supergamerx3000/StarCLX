# Änderungen

## 1.3.0 – 2026-10-07

### Funktionstasten wie in der STARFACE-App
- Besetztlampenfeld mit Benutzerbild (sonst Initialen), Telefonzustand als Fläche (grün frei, rot
  im Gespräch, grau ohne Telefon oder bei Ruhe), Ruhe, Chat-Status, Umleitung und Statustext;
  auch Gruppen bekommen ihren Zustand
- Ruhe, Modul aktivieren, Umleitungen, Rufnummer anzeigen und Gruppe An-/Abmelden als Schalter;
  Umleitungen „teilaktiv“, wenn nur manche an sind
- Neu: Modul aktivieren, Rückruf bei Besetzt und „Anonym“ (Rufnummer unterdrücken); die aktive
  Rufnummer abschalten unterdrückt sie, Anonym abschalten zeigt wieder die Durchwahl
- Direktwahl wie ein Kontakt; Tasten nur für Tischtelefone werden ausgeblendet
- Die Tasten bekamen vorher keine Zustände, weil die Präsenz auf einen Stream wartete, den die
  Anlage erst nach dem Abo öffnet

### Eigener Status und Menü am Profilbild
- Menü wie in der STARFACE-App: Bitte nicht stören, Chat-Status, primäres Telefon,
  signalisierte Rufnummer und Immer-Umleitungen
- Chat-Status Verfügbar, Abwesend, Bitte nicht stören und gespeicherte eigene Status mit Symbol
  und Text; was in einem anderen Client gesetzt wird, übernimmt StarCLX
- Rufnummer, Telefone, Bild und Rechte werden live nachgezogen, auch wenn sie anderswo geändert
  werden

### Oberfläche
- Schrift Inter wird mitgeliefert (vorher je nach Distribution z. B. DejaVu Sans), etwas
  kompakter

## 1.2.1 – 2026-10-07

### Flatpak
- Eigene Flatpak-Quelle: `flatpak install --user https://crazmoe.github.io/StarCLX/flatpak/starclx.flatpakref`,
  danach kommen Updates mit `flatpak update`
- Die .flatpak-Datei hängt jetzt am Release

## 1.2.0 – 2026-10-07

### Flatpak und Flathub
- StarCLX gibt es zusätzlich als Flatpak (`starclx-<Version>.flatpak` am Release); es verhält
  sich wie das .deb-Paket, nur die udev-Regel für Headsets muss man selbst anlegen
- Flathub-Variante `io.github.crazmoe.StarCLX`, aus dem Quellcode gebaut; dort lassen sich bei
  Anruf nur URLs öffnen, keine Programme starten, und Tastenkürzel trägt man selbst ein
- Im Flatpak läuft der Autostart über das Hintergrund-Portal der Desktop-Umgebung
- StarCLX steht unter der MIT-Lizenz

## 1.1.1 – 2026-10-06

### Chat
- Der Chat trennt sich an Cloud-Anlagen nicht mehr alle fünf Minuten: nach 60 Sekunden Stille
  fragt StarCLX per Ping nach, statt die Verbindung unbemerkt kappen zu lassen

### Anmeldung
- Ohne Schlüsselbund (kein gnome-keyring/KWallet) ist Anmelden und Abmelden jetzt möglich; die
  Anmeldung gilt dann bis zum Beenden
- Ohne Schlüsselbund steht der Hinweis darauf nur noch einmal im Protokoll statt bei jeder
  Erneuerung der Anmeldung

## 1.1.0 – 2026-10-05

### Anmeldung an Cloud-Anlagen
Anlagen an den STARFACE-Cloud-Diensten (Login über den zentralen STARFACE-Login, auch mit
Microsoft o. ä. dahinter) ließen sich nicht anmelden.

- Login-Konfiguration der Anlage (`/rpc/oauth/login-config`) wie in der STARFACE-App: Token mit
  `resource=edgenode://…` für genau diese Anlage, gRPC über das Cloud-Gateway
- Beim zentralen STARFACE-Login passende Scopes statt `pbx-login`
- Softphone an Cloud-Anlagen: SIP-Zertifikat der STARFACE-CA wird angenommen
- Lehnt die Anlage die Anmeldung ab, steht jetzt ihr Grund in der Meldung

### Softphone und Chat
- Benutzer ohne das Recht „Autoprovisionierung“ (`uci_autoprovisioning`) bekommen trotzdem ein
  Softphone: StarCLX nimmt dann das App-Telefon der Windows-App; fehlt auch das, sagt die Meldung,
  welches Recht der Administrator freischalten muss
- Chat und Softphone lösen den Namen der Anlage über das System auf wie die Anmeldung. Die
  eigenen DNS-Clients fanden manche Anlagen nicht („NXDomain“ im Chat, „destination address
  required“ beim Softphone)

### Verbindung nach Standby
Blieb StarCLX über Nacht offen und wurde der Rechner zugeklappt, fehlten danach Funktionstasten,
neue Anrufe, Rufliste und Voicemail, weil tote Verbindungen nicht bemerkt wurden.

- Verbindung zur Anlage mit Keepalive (HTTP/2 und TCP): abgebrochene Verbindungen fallen nach
  spätestens etwa 30 Sekunden auf und werden neu aufgebaut
- Aufwachen aus dem Standby wird erkannt: Anmeldung (Token) sofort erneuern, Softphone neu
  verbinden und registrieren, Funktionstasten, Voicemail und Umleitungen neu laden
- Funktionstasten versuchen es nach einem Ladefehler selbst erneut, statt leer zu bleiben

### Protokoll für Fehlerberichte
- Die App schreibt ein Protokoll in ihren Log-Ordner (`~/.local/share/ch.crazmoe.starface-linuxclient/logs`)
- Einstellungen → Protokoll: speichern (mit Version, Distribution, Kernel und Desktop) und uns
  schicken, Ordner öffnen, auf Wunsch ausführlich mit Anruf- und Verbindungsdetails

### Call Manager
- Anrufkarte wie in der STARFACE-App: Karte und Aktionen (Rückfrage, Konferenz, Extras) in einer
  gemeinsamen Schale statt einer verrutschten Leiste

### Pakete
- RPM-Paket für Fedora, openSUSE und andere RPM-Distributionen
- Signierte Paketquellen für apt (Debian/Ubuntu) und dnf (Fedora) auf GitHub Pages, Updates
  kommen mit dem normalen System-Update
- Pakete starten auch unter Debian 12 und Ubuntu 22.04 (gebaut mit älterer glibc)

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
