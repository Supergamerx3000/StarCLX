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
    (
        "Softphone ist nicht bereit",
        [
            "Softphone is not ready",
            "Le softphone n'est pas prêt",
            "Il softphone non è pronto",
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
        "Belegt oder ungültig: {keys}",
        [
            "Already taken or invalid: {keys}",
            "Déjà utilisé ou invalide : {keys}",
            "Già in uso o non valido: {keys}",
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
