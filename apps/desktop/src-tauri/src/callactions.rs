//! URL oder Programm bei Anruf (wie „URL/Programm bei Anruf“ im
//! Windows-Client). Die Oberfläche meldet, wann ein Anruf klingelt,
//! angenommen wird oder abgeht; hier werden die passenden Regeln ausgeführt.
//!
//! Die Rufnummer kommt aus dem Netz. Sie wird deshalb auf Ziffern, `+`, `*`
//! und `#` reduziert, in URLs kodiert und bei Programmen erst nach dem
//! Zerlegen der Befehlszeile in einzelne Argumente eingesetzt; eine Shell
//! ist nie beteiligt.

use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::i18n::{t, tf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CallAction {
    pub enabled: bool,
    /// "ringing", "answered" oder "outgoing"
    pub trigger: String,
    /// Platzhalter auf die Nummer, z. B. `+41*`; leer heisst alle
    pub filter: String,
    /// Nur bei externen Anrufen
    pub external_only: bool,
    /// URL (mit `://`) oder Befehlszeile
    pub target: String,
}

impl Default for CallAction {
    fn default() -> Self {
        Self {
            enabled: true,
            trigger: "ringing".into(),
            filter: String::new(),
            external_only: false,
            target: String::new(),
        }
    }
}

/// Nur Ziffern, `+`, `*` und `#`
pub fn sanitize(number: &str) -> String {
    number
        .chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '+' | '*' | '#'))
        .collect()
}

/// Kurze Nummern (unter fünf Ziffern) sind interne Durchwahlen.
pub fn is_internal(number: &str) -> bool {
    number.chars().filter(char::is_ascii_digit).count() < 5
}

/// +E.164: `00…` → `+…`, `0…` → `+<Land>…`, `+…` bleibt.
pub fn canonical(number: &str, country: &str) -> String {
    if is_internal(number) || number.starts_with('+') {
        number.to_owned()
    } else if let Some(rest) = number.strip_prefix("00") {
        format!("+{rest}")
    } else if let Some(rest) = number.strip_prefix('0') {
        format!("+{country}{rest}")
    } else {
        number.to_owned()
    }
}

/// Nationales Format: eigenes Land mit `0`, sonst `00` statt `+`.
pub fn national(number: &str, country: &str) -> String {
    if is_internal(number) {
        return number.to_owned();
    }
    let c = canonical(number, country);
    if let Some(rest) = c.strip_prefix(&format!("+{country}")) {
        format!("0{rest}")
    } else if let Some(rest) = c.strip_prefix('+') {
        format!("00{rest}")
    } else {
        c
    }
}

/// Platzhaltervergleich: `*` beliebig viele Zeichen, `?` genau eins.
pub fn wildcard(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().filter(|c| !c.is_whitespace()).collect();
    let s: Vec<char> = text.chars().collect();
    let (mut pi, mut si) = (0, 0);
    let mut back: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) && p[pi] != '*' {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            back = Some((pi, si));
            pi += 1;
        } else if let Some((bp, bs)) = back {
            pi = bp + 1;
            si = bs + 1;
            back = Some((bp, bs + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// Greift die Regel für diesen Anruf? Der Filter darf auf die Nummer wie
/// empfangen, im nationalen oder im +E.164-Format passen.
pub fn matches(rule: &CallAction, number: &str, internal: bool, country: &str) -> bool {
    let n = sanitize(number);
    if rule.external_only && (internal || is_internal(&n)) {
        return false;
    }
    rule.filter.trim().is_empty()
        || [n.clone(), national(&n, country), canonical(&n, country)]
            .iter()
            .any(|v| wildcard(&rule.filter, v))
}

/// Befehlszeile wie in einer Shell in Argumente zerlegen: Leerraum trennt,
/// '…' wörtlich, "…" mit `\"` und `\\`, ausserhalb maskiert `\` das nächste
/// Zeichen. Variablen, Umleitungen usw. gibt es nicht.
pub fn split_args(line: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => cur.push(c),
                        None => return Err(t("Anführungszeichen nicht geschlossen").into()),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\')) => cur.push(c),
                            Some(c) => {
                                cur.push('\\');
                                cur.push(c);
                            }
                            None => return Err(t("Anführungszeichen nicht geschlossen").into()),
                        },
                        Some(c) => cur.push(c),
                        None => return Err(t("Anführungszeichen nicht geschlossen").into()),
                    }
                }
            }
            '\\' => {
                started = true;
                if let Some(c) = chars.next() {
                    cur.push(c);
                }
            }
            c if c.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                started = true;
                cur.push(c);
            }
        }
    }
    if started {
        args.push(cur);
    }
    Ok(args)
}

/// Prozentkodierung für URLs; nur unreservierte Zeichen bleiben.
pub fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Werte der Variablen `$(callerid)`, `$(calleridNational)`, `$(calleridCanonical)`
fn variables(number: &str, country: &str) -> [(&'static str, String); 3] {
    let n = sanitize(number);
    [
        ("$(calleridNational)", national(&n, country)),
        ("$(calleridCanonical)", canonical(&n, country)),
        ("$(callerid)", n),
    ]
}

fn substitute(s: &str, vars: &[(&str, String)], encode: bool) -> String {
    vars.iter().fold(s.to_owned(), |s, (k, v)| {
        s.replace(k, &if encode { url_encode(v) } else { v.clone() })
    })
}

pub fn is_url(target: &str) -> bool {
    target.contains("://")
}

/// URL mit kodierten Werten
pub fn expand_url(target: &str, number: &str, country: &str) -> String {
    substitute(target.trim(), &variables(number, country), true)
}

/// Argumente des Programms; erst zerlegen, dann einsetzen.
pub fn expand_argv(target: &str, number: &str, country: &str) -> Result<Vec<String>, String> {
    let vars = variables(number, country);
    let argv: Vec<String> = split_args(target)?
        .iter()
        .map(|a| substitute(a, &vars, false))
        .collect();
    if argv.first().is_none_or(String::is_empty) {
        return Err(t("Kein Programm angegeben").into());
    }
    Ok(argv)
}

/// Öffnet die URL im Standardbrowser bzw. startet das Programm.
pub fn run(app: &AppHandle, target: &str, number: &str, country: &str) -> Result<(), String> {
    if target.trim().is_empty() {
        return Err(t("Kein Programm angegeben").into());
    }
    if is_url(target) {
        return app
            .opener()
            .open_url(expand_url(target, number, country), None::<&str>)
            .map_err(|e| e.to_string());
    }
    let argv = expand_argv(target, number, country)?;
    let mut child = crate::flatpak::host_command(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| tf("Programm nicht gestartet: {e}", &[("e", &e.to_string())]))?;
    // Auf das Ende warten, damit kein Zombie-Prozess bleibt
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Einzelnes Ziel ausführen (Schaltfläche „Testen“)
#[tauri::command]
pub fn call_action_run(app: AppHandle, target: String, number: String) -> Result<(), String> {
    let country = crate::settings::load(&app).prefs.default_country_code;
    run(
        &app,
        &target,
        &number,
        country.trim().trim_start_matches('+'),
    )
}

/// Alle aktiven Regeln für `trigger` ausführen, deren Filter passt.
#[tauri::command]
pub fn call_actions_fire(
    app: AppHandle,
    trigger: String,
    number: String,
    internal: bool,
) -> Result<(), String> {
    let prefs = crate::settings::load(&app).prefs;
    let country = prefs.default_country_code.trim().trim_start_matches('+');
    let errors: Vec<String> = prefs
        .call_actions
        .iter()
        .filter(|r| r.enabled && r.trigger == trigger && !r.target.trim().is_empty())
        .filter(|r| matches(r, &number, internal, country))
        .filter_map(|r| run(&app, &r.target, &number, country).err())
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        tracing::warn!(?errors, "Aktion bei Anruf fehlgeschlagen");
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formats() {
        assert_eq!(canonical("0441234567", "41"), "+41441234567");
        assert_eq!(canonical("0049301234", "41"), "+49301234");
        assert_eq!(canonical("+41441234567", "41"), "+41441234567");
        assert_eq!(canonical("123", "41"), "123");
        assert_eq!(national("+41441234567", "41"), "0441234567");
        assert_eq!(national("0049301234", "41"), "0049301234");
        assert_eq!(national("+49301234", "41"), "0049301234");
        assert_eq!(national("0441234567", "41"), "0441234567");
        assert_eq!(national("12", "41"), "12");
        assert!(is_internal("1234") && !is_internal("12345"));
    }

    #[test]
    fn wildcard_filter() {
        assert!(wildcard("+41*", "+41441234567"));
        assert!(wildcard("0*", "0441234567"));
        assert!(!wildcard("0*", "+41441234567"));
        assert!(wildcard("*567", "0441234567"));
        assert!(wildcard("044?234*", "0441234567"));
        assert!(wildcard("*", ""));
        assert!(!wildcard("12", "123"));
        let rule = |filter: &str, external_only| CallAction {
            filter: filter.into(),
            external_only,
            ..CallAction::default()
        };
        assert!(matches(&rule("+41*", false), "0441234567", false, "41"));
        assert!(matches(&rule("", false), "12", true, "41"));
        assert!(!matches(&rule("", true), "12", false, "41"));
        assert!(!matches(&rule("", true), "0441234567", true, "41"));
        assert!(!matches(&rule("+49*", false), "0441234567", false, "41"));
    }

    #[test]
    fn argv_split() {
        assert_eq!(
            split_args(r#"/usr/bin/crm --nr "a b" 'c d' e\ f"#).unwrap(),
            vec!["/usr/bin/crm", "--nr", "a b", "c d", "e f"]
        );
        assert_eq!(split_args(r#"x "" y"#).unwrap(), vec!["x", "", "y"]);
        assert!(split_args("x 'offen").is_err());
        assert!(split_args("").unwrap().is_empty());
    }

    #[test]
    fn url_values_are_encoded() {
        assert_eq!(url_encode("+41#*"), "%2B41%23%2A");
        assert_eq!(
            expand_url(
                "https://crm.example/suche?nr=$(calleridCanonical)&n=$(calleridNational)&r=$(callerid)",
                "044 123 45 67",
                "41"
            ),
            "https://crm.example/suche?nr=%2B41441234567&n=0441234567&r=0441234567"
        );
        assert_eq!(
            expand_url("https://x/?q=$(callerid)", "1&a=b", "41"),
            "https://x/?q=1"
        );
    }

    #[test]
    fn number_cannot_inject() {
        let argv = expand_argv("notify-send 'Anruf' $(callerid)", "123;rm -rf ~", "41").unwrap();
        assert_eq!(argv, vec!["notify-send", "Anruf", "123"]);
        let argv = expand_argv(
            "prog --nr=$(calleridCanonical)",
            "0441234567 $(x) `id`",
            "41",
        )
        .unwrap();
        assert_eq!(argv, vec!["prog", "--nr=+41441234567"]);
        assert!(expand_argv("$(callerid)", "", "41").is_err());
    }
}
