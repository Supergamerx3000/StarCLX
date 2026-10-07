//! Wartezeit vor dem nächsten Verbindungsversuch.
//!
//! Die STARFACE begrenzt ab der nächsten Version die Anfragen pro Client
//! (Rate-Limits). Damit StarCLX dort nicht anschlägt:
//! - Die Wartezeit wächst exponentiell und wird erst zurückgesetzt, wenn eine
//!   Verbindung eine Weile stabil lief. Schliesst die Anlage einen Stream
//!   sofort wieder, verbindet der Client also nicht im Sekundentakt neu.
//! - Ein Zufallsanteil verteilt die Versuche, damit nach einem Ausfall oder
//!   dem Standby nicht alle Streams und Clients gleichzeitig anklopfen.
//! - Nennt die Anlage selbst eine Wartezeit (`Retry-After`), gilt mindestens
//!   diese.

use std::hash::{BuildHasher, RandomState};
use std::time::Duration;

/// Ab dieser Laufzeit gilt eine Verbindung als stabil; danach beginnt die
/// Wartezeit wieder beim Minimum.
pub const STABLE_AFTER: Duration = Duration::from_secs(60);

/// Wartezeit bei einem Rate-Limit, wenn die Anlage keine nennt.
pub const LIMITED_DEFAULT: Duration = Duration::from_secs(60);

/// Obergrenze für eine von der Anlage genannte Wartezeit.
const HINT_MAX: Duration = Duration::from_secs(15 * 60);

/// Anteil des Zufalls an der Wartezeit (±20 %)
const JITTER: f64 = 0.2;

#[derive(Debug, Clone)]
pub struct Backoff {
    min: Duration,
    max: Duration,
    next: Duration,
}

impl Backoff {
    pub fn new(min: Duration, max: Duration) -> Self {
        Self {
            min,
            max,
            next: min,
        }
    }

    /// Wartezeit nach einem Versuch, der `ran` lang lief. `hint` ist eine
    /// Vorgabe der Anlage (Rate-Limit), falls sie eine gemacht hat.
    pub fn next(&mut self, ran: Duration, hint: Option<Duration>) -> Duration {
        if ran >= STABLE_AFTER {
            self.next = self.min;
        }
        let base = self.next;
        self.next = (self.next * 2).min(self.max);
        let wait = jitter(base, random_unit());
        match hint {
            // Bis zu 10 % obendrauf, damit nicht alle exakt nach Ablauf kommen
            Some(h) => {
                let h = h.min(HINT_MAX);
                wait.max(h + h.mul_f64(0.1 * random_unit()))
            }
            None => wait,
        }
    }

    /// Nach einem erfolgreichen Versuch, der nicht als Verbindung zählt
    /// (z. B. eine Anmeldung), wieder beim Minimum beginnen.
    pub fn reset(&mut self) {
        self.next = self.min;
    }
}

/// `d` um bis zu ±[`JITTER`] verschoben; `unit` liegt in [0, 1).
fn jitter(d: Duration, unit: f64) -> Duration {
    d.mul_f64(1.0 - JITTER + 2.0 * JITTER * unit)
}

/// Zufallszahl in [0, 1) ohne eigene Abhängigkeit: Jeder `RandomState` ist
/// zufällig initialisiert.
fn random_unit() -> f64 {
    (RandomState::new().hash_one(0u8) >> 11) as f64 / (1u64 << 53) as f64
}

/// Liest `Retry-After` als Sekunden. Die Datumsform kommt bei der STARFACE
/// nicht vor; dann gilt [`LIMITED_DEFAULT`].
pub fn parse_retry_after(value: Option<&str>) -> Duration {
    value
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(LIMITED_DEFAULT)
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: fn(u64) -> Duration = Duration::from_secs;

    #[test]
    fn jitter_stays_within_bounds() {
        assert_eq!(jitter(S(10), 0.0), Duration::from_secs(8));
        assert_eq!(jitter(S(10), 0.5), S(10));
        assert!(jitter(S(10), 0.999_999) < S(12));
        for _ in 0..1000 {
            let u = random_unit();
            assert!((0.0..1.0).contains(&u));
        }
    }

    #[test]
    fn grows_on_quick_failures_and_resets_after_stable_run() {
        let mut b = Backoff::new(S(1), S(30));
        let waits: Vec<_> = (0..7).map(|_| b.next(Duration::ZERO, None)).collect();
        let bounds = [1, 2, 4, 8, 16, 30, 30];
        for (w, base) in waits.iter().zip(bounds) {
            assert!(
                *w >= S(base).mul_f64(0.8) && *w <= S(base).mul_f64(1.2),
                "{w:?} vs {base}"
            );
        }
        // Ein sauber, aber sofort beendeter Stream zählt nicht als stabil
        assert!(b.next(S(2), None) >= S(24));
        // Nach einer stabilen Verbindung wieder kurz
        assert!(b.next(STABLE_AFTER, None) <= Duration::from_millis(1200));
    }

    #[test]
    fn hint_from_pbx_wins_but_is_capped() {
        let mut b = Backoff::new(S(1), S(30));
        assert!(b.next(Duration::ZERO, Some(S(120))) >= S(120));
        let capped = b.next(Duration::ZERO, Some(S(24 * 3600)));
        assert!(capped >= HINT_MAX && capped <= HINT_MAX.mul_f64(1.1));
    }

    #[test]
    fn retry_after() {
        assert_eq!(parse_retry_after(Some(" 30 ")), S(30));
        assert_eq!(
            parse_retry_after(Some("Wed, 21 Oct 2026 07:28:00 GMT")),
            LIMITED_DEFAULT
        );
        assert_eq!(parse_retry_after(None), LIMITED_DEFAULT);
    }
}
