# sfctl

Kommandozeile für die STARFACE, auf demselben Kern wie die Desktop-App. Gedacht für Skripte,
Cronjobs und Monitoring: Chat-Nachrichten senden, den eigenen Status setzen, anrufen und
Ansagen abspielen.

Fertige Dateien gibt es bei jedem [Release](https://github.com/crazmoe/StarCLX/releases)
(`sfctl-<version>-linux-x86_64`). Aus dem Quelltext: `cargo build --release -p sfctl`.

## Anmelden

Die Zugangsdaten kommen aus Umgebungsvariablen (oder den gleichnamigen Optionen):

```sh
export SF_SERVER=https://anlage.example.com
export SF_USER=1004 SF_PASSWORD=…    # Password-Grant
# oder: export SF_TOKEN=…            # fertiges Access-Token
```

Der Password-Grant braucht auf der Anlage das Recht „API access with Password Grant“.
Für Skripte empfiehlt sich ein eigener Benutzer, z. B. „Monitoring“.

## Befehle

| Befehl | Zweck |
|---|---|
| `version` | Version der Anlage |
| `me` | eigenes Profil mit Rufnummern |
| `phones` | eigene Telefone, `*` markiert das Primärtelefon |
| `call <nummer>` | Anruf über das eigene Telefon starten |
| `chat contacts` | Chat-Kontakte mit Status |
| `chat send <an> <text>` | Chat-Nachricht senden |
| `status` | eigenen Status anzeigen oder setzen |
| `say <nummer> <text>` | anrufen und eine Ansage abspielen |
| `softphone` | App-Telefon anmelden (Test) |

`sfctl <befehl> --help` zeigt alle Optionen.

## Rückgabewerte

| Code | Bedeutung |
|---|---|
| 0 | erfolgreich (bei `say`: angenommen bzw. bestätigt) |
| 1 | Fehler (Anmeldung, Netz, unbekannter Kontakt …) |
| 2 | `say`: nicht angenommen |
| 3 | `say`: angenommen, aber nicht bestätigt |

## Chat

```sh
sfctl chat contacts
sfctl chat send "Anna Muster" "Backup fehlgeschlagen"
sfctl chat send anna "Kurz: Server neu gestartet"
df -h / | sfctl chat send anna -          # Text von der Standardeingabe
```

Als Empfänger gehen die Jabber-ID (`anna@pbx`), der Benutzername vor dem @ oder ein
eindeutiger Teil des Namens. Ist der Name nicht eindeutig, nennt `sfctl` die Treffer.

## Status

```sh
sfctl status                              # anzeigen
sfctl status --dnd on --text "Im Meeting"
sfctl status --dnd off --text ""          # zurücksetzen
```

Läuft gleichzeitig die Desktop-App, setzt sie ihren eigenen Statustext über den Chat und kann
den von `sfctl` überschreiben.

## Ansage mit Bestätigung

`say` meldet ein App-Telefon an, lässt die Anlage die Nummer wählen und spielt die Ansage ab,
sobald das Gegenüber abnimmt.

```sh
sfctl say 0791234567 "Das Backup ist fehlgeschlagen"
sfctl say --confirm 1 0791234567 "Server down"
sfctl say --confirm "#" --repeat 5 12 "Bitte Serverraum prüfen"
sfctl say 12 --wav ansage.wav             # eigene Aufnahme statt Sprachausgabe
```

- `--confirm <taste>` (0–9, `*` oder `#`, Letztere in Anführungszeichen): wiederholt die
  Ansage, bis die Taste gedrückt wird, und hängt „Bitte bestätigen Sie mit der Taste …“ an
  (`--no-hint` lässt den Satz weg).
- `--repeat` (Standard 3), `--pause` (1 s), `--confirm-wait` (10 s nach der letzten Ansage),
  `--ring-timeout` (45 s).
- `--insecure-sip`, wenn die Anlage ein selbstsigniertes Zertifikat hat.

Ohne `--confirm` gilt auch eine Mailbox, die abnimmt, als angenommen.

### Sprachausgabe

Standard ist `espeak-ng` (`sudo apt install espeak-ng`). Mit `--tts` oder `SF_TTS` lässt sich
ein anderes Programm einsetzen. Der Befehl läuft über `sh -c`, bekommt den Text in `$TEXT` und
schreibt eine WAV-Datei (16-bit-PCM) nach `$WAV`.

[Piper](https://github.com/OHF-Voice/piper1-gpl) klingt deutlich natürlicher und läuft
ebenfalls offline:

```sh
pipx install piper-tts                     # oder in einer venv: pip install piper-tts
mkdir -p ~/.local/share/piper && cd ~/.local/share/piper
python3 -m piper.download_voices de_DE-thorsten-high   # mit pipx: ~/.local/share/pipx/venvs/piper-tts/bin/python
export SF_TTS='printf %s "$TEXT" | piper -m "$HOME/.local/share/piper/de_DE-thorsten-high.onnx" -f "$WAV"'
```

Deutsche Stimmen sind u. a. `de_DE-thorsten-high` (männlich), `de_DE-kerstin-low` und
`de_DE-ramona-low` (weiblich); Hörproben unter https://rhasspy.github.io/piper-samples/.

## Beispiele für Skripte

Alarm, bis jemand bestätigt, sonst die nächste Person:

```sh
#!/bin/sh
for nr in 0791111111 0792222222; do
    sfctl say --confirm 1 "$nr" "Achtung: $1" && exit 0
done
sfctl chat send technik "Alarm unbestätigt: $1"
exit 1
```

Backup-Meldung per Cron:

```sh
0 3 * * * /usr/local/bin/backup.sh || sfctl chat send anna "Backup auf $(hostname) fehlgeschlagen"
```

Status während einer Wartung:

```sh
sfctl status --dnd on --text "Wartung bis 18 Uhr"
./wartung.sh
sfctl status --dnd off --text ""
```
