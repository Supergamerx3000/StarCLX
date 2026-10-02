# Änderungen

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
