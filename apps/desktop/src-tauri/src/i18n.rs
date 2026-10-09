//! Übersetzung der Texte, die die App selbst anzeigt (Tray, Fenstertitel,
//! Benachrichtigungen, Fehlermeldungen). Schlüssel ist der deutsche Text,
//! fehlt eine Übersetzung, bleibt er stehen. Die Oberfläche übersetzt ihre
//! Texte selbst (src/lib/i18n).

use std::sync::atomic::{AtomicUsize, Ordering};

/// Sprachen in der Reihenfolge der Spalten in [`TEXTS`]; 0 = Deutsch
const LANGS: [&str; 4] = ["de", "en", "fr", "it"];

static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// Deutsch → Englisch, Französisch, Italienisch
const TEXTS: &[(&str, [&str; 3])] = &[
    // Module
    (
        "Modul nicht gefunden oder nicht freigegeben",
        [
            "Module not found or not shared with you",
            "Module introuvable ou non partagé",
            "Modulo non trovato o non condiviso",
        ],
    ),
    (
        "Dieses Modul darf nicht geschaltet werden",
        [
            "This module may not be switched",
            "Ce module ne peut pas être commuté",
            "Questo modulo non può essere commutato",
        ],
    ),
    (
        "Keine Berechtigung, Module zu schalten",
        [
            "No permission to switch modules",
            "Pas d’autorisation pour commuter les modules",
            "Nessuna autorizzazione per commutare i moduli",
        ],
    ),
    // Protokoll
    (
        "Protokoll speichern",
        ["Save log", "Enregistrer le journal", "Salva registro"],
    ),
    ("Textdatei", ["Text file", "Fichier texte", "File di testo"]),
    // Dateien im Chat
    (
        "Datei angeboten: {name}",
        [
            "File offered: {name}",
            "Fichier proposé : {name}",
            "File offerto: {name}",
        ],
    ),
    (
        "Dateien senden",
        ["Send files", "Envoyer des fichiers", "Invia file"],
    ),
    (
        "Kein Ordner für empfangene Dateien",
        [
            "No folder for received files",
            "Aucun dossier pour les fichiers reçus",
            "Nessuna cartella per i file ricevuti",
        ],
    ),
    (
        "Datei nicht gefunden",
        ["File not found", "Fichier introuvable", "File non trovato"],
    ),
    // Tray und Fenster
    ("Öffnen", ["Open", "Ouvrir", "Apri"]),
    (
        "Schnellwahl",
        ["Quick dial", "Numérotation rapide", "Selezione rapida"],
    ),
    ("Abmelden", ["Sign out", "Se déconnecter", "Disconnetti"]),
    ("Beenden", ["Quit", "Quitter", "Esci"]),
    (
        "StarCLX: abgemeldet",
        [
            "StarCLX: signed out",
            "StarCLX : déconnecté",
            "StarCLX: disconnesso",
        ],
    ),
    (
        "StarCLX Schnellwahl",
        [
            "StarCLX Quick dial",
            "StarCLX Numérotation rapide",
            "StarCLX Selezione rapida",
        ],
    ),
    (
        "StarCLX – Anmelden",
        [
            "StarCLX – Sign in",
            "StarCLX – Connexion",
            "StarCLX – Accesso",
        ],
    ),
    ("Abgemeldet", ["Signed out", "Déconnecté", "Disconnesso"]),
    (
        "Sitzung beendet: {reason}",
        [
            "Session ended: {reason}",
            "Session terminée : {reason}",
            "Sessione terminata: {reason}",
        ],
    ),
    // Benachrichtigungen
    ("Unbekannt", ["Unknown", "Inconnu", "Sconosciuto"]),
    (
        "Verpasster Anruf",
        ["Missed call", "Appel manqué", "Chiamata persa"],
    ),
    (
        "{who} über Gruppe {group}",
        [
            "{who} via group {group}",
            "{who} via le groupe {group}",
            "{who} tramite il gruppo {group}",
        ],
    ),
    (
        "Neue Voicemail",
        [
            "New voicemail",
            "Nouveau message vocal",
            "Nuovo messaggio vocale",
        ],
    ),
    ("von {who}", ["from {who}", "de {who}", "da {who}"]),
    // Dateiauswahl
    (
        "Klingelton auswählen",
        ["Select ringtone", "Choisir une sonnerie", "Scegli suoneria"],
    ),
    (
        "Ordner für empfangene Dateien",
        [
            "Folder for received files",
            "Dossier pour les fichiers reçus",
            "Cartella per i file ricevuti",
        ],
    ),
    (
        "Voicemail speichern",
        [
            "Save voicemail",
            "Enregistrer le message vocal",
            "Salva messaggio vocale",
        ],
    ),
    // Tastenkürzel (Namen in GNOME)
    (
        "Markierte Rufnummer wählen",
        [
            "Dial selected number",
            "Composer le numéro sélectionné",
            "Componi il numero selezionato",
        ],
    ),
    (
        "Rufnummer aus Zwischenablage wählen",
        [
            "Dial number from clipboard",
            "Composer le numéro du presse-papiers",
            "Componi il numero dagli appunti",
        ],
    ),
    (
        "Softphone-Anruf annehmen",
        [
            "Answer softphone call",
            "Prendre l'appel du softphone",
            "Rispondi alla chiamata softphone",
        ],
    ),
    (
        "Aktuellen Anruf beenden",
        [
            "End current call",
            "Terminer l'appel en cours",
            "Termina la chiamata in corso",
        ],
    ),
    (
        "Ansicht umschalten",
        ["Switch view", "Changer de vue", "Cambia vista"],
    ),
    // Status und Fehler
    (
        "Verbinde …",
        ["Connecting …", "Connexion …", "Connessione …"],
    ),
    (
        "Kein Chat-Konto auf der Anlage",
        [
            "No chat account on the phone system",
            "Aucun compte de chat sur l'installation",
            "Nessun account chat sul centralino",
        ],
    ),
    (
        "Chat nicht verfügbar: {e}",
        [
            "Chat not available: {e}",
            "Chat indisponible : {e}",
            "Chat non disponibile: {e}",
        ],
    ),
    (
        "Chat ist nicht verbunden",
        [
            "Chat is not connected",
            "Le chat n'est pas connecté",
            "La chat non è connessa",
        ],
    ),
    (
        "Softphone in den Einstellungen ausgeschaltet",
        [
            "Softphone switched off in the settings",
            "Softphone désactivé dans les paramètres",
            "Softphone disattivato nelle impostazioni",
        ],
    ),
    // Türkamera
    (
        "Keine Türkamera zu diesem Anruf",
        [
            "No door camera for this call",
            "Pas de caméra de porte pour cet appel",
            "Nessuna telecamera porta per questa chiamata",
        ],
    ),
    (
        "Keine Kamera-URL",
        [
            "No camera URL",
            "Pas d'URL de caméra",
            "Nessun URL della telecamera",
        ],
    ),
    (
        "Ungültige Kamera-URL: {detail}",
        [
            "Invalid camera URL: {detail}",
            "URL de caméra invalide : {detail}",
            "URL della telecamera non valido: {detail}",
        ],
    ),
    (
        "Kamera nicht erreichbar: {detail}",
        [
            "Camera not reachable: {detail}",
            "Caméra injoignable : {detail}",
            "Telecamera non raggiungibile: {detail}",
        ],
    ),
    (
        "Kamera antwortet mit {detail}",
        [
            "Camera responds with {detail}",
            "La caméra répond {detail}",
            "La telecamera risponde con {detail}",
        ],
    ),
    (
        "Kamera liefert kein Bild",
        [
            "Camera delivers no image",
            "La caméra ne fournit pas d'image",
            "La telecamera non fornisce immagini",
        ],
    ),
    (
        "Kamera sendet keine Bilder mehr",
        [
            "Camera stopped sending images",
            "La caméra n'envoie plus d'images",
            "La telecamera non invia più immagini",
        ],
    ),
    (
        "Für RTSP-Kameras wird ffmpeg benötigt",
        [
            "RTSP cameras need ffmpeg",
            "Les caméras RTSP nécessitent ffmpeg",
            "Le telecamere RTSP richiedono ffmpeg",
        ],
    ),
    (
        "ffmpeg: {detail}",
        ["ffmpeg: {detail}", "ffmpeg : {detail}", "ffmpeg: {detail}"],
    ),
    (
        "Anrufsteuerung ist nicht bereit",
        [
            "Call control is not ready",
            "Le contrôle des appels n'est pas prêt",
            "Il controllo chiamate non è pronto",
        ],
    ),
    (
        "Das Softphone ist nicht aktiv.",
        [
            "The softphone is not active.",
            "Le softphone n'est pas actif.",
            "Il softphone non è attivo.",
        ],
    ),
    (
        "Keine Nummer",
        ["No number", "Aucun numéro", "Nessun numero"],
    ),
    (
        "Nicht angemeldet",
        ["Not signed in", "Non connecté", "Non connesso"],
    ),
    (
        "Kein Login ausstehend",
        [
            "No sign-in pending",
            "Aucune connexion en attente",
            "Nessun accesso in sospeso",
        ],
    ),
    (
        "Antwort der Anlage enthält keinen gültigen Code",
        [
            "The phone system's response contains no valid code",
            "La réponse de l'installation ne contient aucun code valide",
            "La risposta del centralino non contiene un codice valido",
        ],
    ),
    (
        "Server-Adresse ohne Hostname",
        [
            "Server address without host name",
            "Adresse du serveur sans nom d'hôte",
            "Indirizzo del server senza nome host",
        ],
    ),
    (
        "Unbekannte Aktion {action}",
        [
            "Unknown action {action}",
            "Action inconnue {action}",
            "Azione sconosciuta {action}",
        ],
    ),
    (
        "Unbekannter Ordner",
        ["Unknown folder", "Dossier inconnu", "Cartella sconosciuta"],
    ),
    (
        "Nicht gespeichert: {e}",
        ["Not saved: {e}", "Non enregistré : {e}", "Non salvato: {e}"],
    ),
    (
        "Bitte Nachname oder Firma ausfüllen.",
        [
            "Please enter a last name or company.",
            "Veuillez saisir un nom ou une entreprise.",
            "Inserisci un cognome o un'azienda.",
        ],
    ),
    (
        "Bitte eine Rufnummer eingeben.",
        [
            "Please enter a phone number.",
            "Veuillez saisir un numéro de téléphone.",
            "Inserisci un numero di telefono.",
        ],
    ),
    (
        "Gespeichert, aber Tastenkürzel nicht eingetragen: {e}",
        [
            "Saved, but keyboard shortcuts not registered: {e}",
            "Enregistré, mais raccourcis clavier non inscrits : {e}",
            "Salvato, ma scorciatoie da tastiera non registrate: {e}",
        ],
    ),
    (
        "Gespeichert. Das Softphone übernimmt die Änderung nach dem Gespräch beim nächsten Start.",
        [
            "Saved. The softphone will apply the change at its next start after the call.",
            "Enregistré. Le softphone appliquera la modification à son prochain démarrage après l'appel.",
            "Salvato. Il softphone applicherà la modifica al prossimo avvio dopo la chiamata.",
        ],
    ),
    (
        "Gruppe nicht gefunden oder kein Mitglied",
        [
            "Group not found or not a member",
            "Groupe introuvable ou pas membre",
            "Gruppo non trovato o non membro",
        ],
    ),
    (
        "Die Anmeldung in „{name}“ lässt sich nicht ändern",
        [
            "Membership in “{name}” cannot be changed",
            "L’inscription dans « {name} » ne peut pas être modifiée",
            "L’accesso a «{name}» non può essere modificato",
        ],
    ),
    (
        "Die Anlage hat die Anmeldung abgelehnt: {e}",
        [
            "The PBX rejected the sign-in: {e}",
            "Le PBX a refusé la connexion : {e}",
            "Il centralino ha rifiutato l’accesso: {e}",
        ],
    ),
    (
        "Die Antwort gehört zu einem älteren Anmeldeversuch. Bitte erneut anmelden.",
        [
            "The response belongs to an earlier sign-in attempt. Please sign in again.",
            "La réponse appartient à une tentative de connexion précédente. Veuillez vous reconnecter.",
            "La risposta appartiene a un tentativo di accesso precedente. Accedi di nuovo.",
        ],
    ),
    (
        "Dem Benutzer fehlt in der Anlage das Recht für App-Telefone (uci_autoprovisioning). Der Administrator kann es unter Benutzer → Rechte freischalten.",
        [
            "The user lacks the permission for app phones on the PBX (uci_autoprovisioning). An administrator can grant it under Users → Permissions.",
            "L’utilisateur n’a pas le droit pour les téléphones d’application sur le PBX (uci_autoprovisioning). Un administrateur peut l’accorder sous Utilisateurs → Droits.",
            "L’utente non ha il permesso per i telefoni app sul centralino (uci_autoprovisioning). Un amministratore può concederlo in Utenti → Permessi.",
        ],
    ),
    (
        "Das SIP-Zertifikat der Anlage ist unbekannt oder hat sich geändert. Bestätige es nur, wenn du den Fingerabdruck kennst.",
        [
            "The PBX's SIP certificate is unknown or has changed. Only confirm it if you recognise the fingerprint.",
            "Le certificat SIP du PBX est inconnu ou a changé. Ne le confirmez que si vous reconnaissez l’empreinte.",
            "Il certificato SIP del centralino è sconosciuto o è cambiato. Confermalo solo se riconosci l’impronta.",
        ],
    ),
    (
        "Kein Zertifikat zu bestätigen",
        [
            "No certificate to confirm",
            "Aucun certificat à confirmer",
            "Nessun certificato da confermare",
        ],
    ),
    (
        "Tastenkürzel lassen sich nur unter GNOME automatisch eintragen.",
        [
            "Keyboard shortcuts can only be registered automatically under GNOME.",
            "Les raccourcis clavier ne peuvent être inscrits automatiquement que sous GNOME.",
            "Le scorciatoie da tastiera si possono registrare automaticamente solo con GNOME.",
        ],
    ),
    (
        "gsettings nicht ausführbar: {e}",
        [
            "gsettings cannot be run: {e}",
            "Impossible d'exécuter gsettings : {e}",
            "Impossibile eseguire gsettings: {e}",
        ],
    ),
    (
        "Gespeichert, aber Autostart nicht eingerichtet: {e}",
        [
            "Saved, but autostart not set up: {e}",
            "Enregistré, mais démarrage automatique non configuré : {e}",
            "Salvato, ma avvio automatico non configurato: {e}",
        ],
    ),
    (
        "StarCLX beim Anmelden starten",
        [
            "Start StarCLX at login",
            "Démarrer StarCLX à l'ouverture de session",
            "Avvia StarCLX all'accesso",
        ],
    ),
    (
        "Das Flatpak von Flathub kann keine Programme starten, nur URLs öffnen.",
        [
            "The Flathub Flatpak cannot start programs, only open URLs.",
            "Le Flatpak de Flathub ne peut pas lancer de programmes, seulement ouvrir des URL.",
            "Il Flatpak di Flathub non può avviare programmi, solo aprire URL.",
        ],
    ),
    (
        "Im Flatpak von Flathub lassen sich Tastenkürzel nicht automatisch eintragen. Lege den angezeigten Befehl in den Systemeinstellungen selbst auf eine Taste.",
        [
            "Keyboard shortcuts cannot be registered automatically in the Flathub Flatpak. Assign the command shown to a key in the system settings yourself.",
            "Dans le Flatpak de Flathub, les raccourcis clavier ne peuvent pas être inscrits automatiquement. Attribuez vous-même la commande affichée à une touche dans les paramètres du système.",
            "Nel Flatpak di Flathub le scorciatoie da tastiera non si possono registrare automaticamente. Assegna tu il comando mostrato a un tasto nelle impostazioni di sistema.",
        ],
    ),
    // URL oder Programm bei Anruf
    (
        "Anführungszeichen nicht geschlossen",
        [
            "Unclosed quotation mark",
            "Guillemet non fermé",
            "Virgolette non chiuse",
        ],
    ),
    (
        "Kein Programm angegeben",
        [
            "No program specified",
            "Aucun programme indiqué",
            "Nessun programma indicato",
        ],
    ),
    (
        "Bitte eine Zielrufnummer eingeben.",
        [
            "Please enter a destination number.",
            "Veuillez saisir un numéro de destination.",
            "Inserisci un numero di destinazione.",
        ],
    ),
    (
        "Profilbild auswählen",
        [
            "Choose profile picture",
            "Choisir une photo de profil",
            "Scegli immagine del profilo",
        ],
    ),
    (
        "Programm nicht gestartet: {e}",
        [
            "Program not started: {e}",
            "Programme non démarré : {e}",
            "Programma non avviato: {e}",
        ],
    ),
];

/// Sprache setzen ("de", "en", "fr", "it"); Unbekanntes heisst Deutsch.
pub fn set_language(lang: &str) {
    CURRENT.store(
        LANGS.iter().position(|l| *l == lang).unwrap_or(0),
        Ordering::Relaxed,
    );
}

/// Deutschen Text in die eingestellte Sprache übersetzen.
pub fn t(de: &'static str) -> &'static str {
    tr(LANGS[CURRENT.load(Ordering::Relaxed)], de)
}

/// Deutschen Text in `lang` übersetzen.
pub fn tr(lang: &str, de: &'static str) -> &'static str {
    let Some(col) = LANGS.iter().position(|l| *l == lang).filter(|&i| i > 0) else {
        return de;
    };
    TEXTS
        .iter()
        .find(|(k, _)| *k == de)
        .map_or(de, |(_, v)| v[col - 1])
}

/// Wie [`t`], ersetzt danach `{name}` durch die Werte.
pub fn tf(de: &'static str, vars: &[(&str, &str)]) -> String {
    vars.iter().fold(t(de).to_owned(), |s, (k, v)| {
        s.replace(&format!("{{{k}}}"), v)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_and_falls_back() {
        assert_eq!(tr("en", "Beenden"), "Quit");
        assert_eq!(tr("it", "Öffnen"), "Apri");
        assert_eq!(tr("de", "Beenden"), "Beenden");
        assert_eq!(tr("xx", "Beenden"), "Beenden");
        assert_eq!(tr("fr", "Gibt es nicht"), "Gibt es nicht");
    }

    #[test]
    fn placeholders_survive_translation() {
        for (de, all) in TEXTS {
            let marks = |s: &str| s.matches('{').count();
            for v in all {
                assert_eq!(marks(de), marks(v), "{de} → {v}");
            }
        }
        assert!(
            TEXTS
                .iter()
                .all(|(de, _)| TEXTS.iter().filter(|(k, _)| k == de).count() == 1)
        );
    }
}
