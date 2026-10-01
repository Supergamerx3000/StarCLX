//! Eingebaute Klingeltöne, synthetisch erzeugt (keine fremden Audiodateien).
//! Jeder Ton ist ein Muster inklusive Pause und wird in Schleife gespielt.

use std::f32::consts::TAU;

use crate::RATE;

/// Namen in Anzeigereihenfolge
pub const NAMES: &[&str] = &[
    "Klassisch",
    "Dezent",
    "Arpeggio",
    "Ding-Dong",
    "Marimba",
    "Digital",
];

/// Rendert den Ton `name`; unbekannte Namen ergeben "Klassisch".
pub fn render(name: &str) -> Vec<i16> {
    let mut buf = vec![0.0f32; (RATE as f32 * 3.0) as usize];
    match name {
        "Dezent" => {
            bell(&mut buf, 0.0, 880.0, 1.2, 0.35);
            bell(&mut buf, 0.15, 1318.5, 1.2, 0.25);
        }
        "Arpeggio" => {
            for (i, f) in [523.25, 659.25, 783.99, 1046.5, 783.99, 659.25]
                .iter()
                .enumerate()
            {
                pluck(&mut buf, i as f32 * 0.14, *f, 0.35, 0.4);
            }
        }
        "Ding-Dong" => {
            bell(&mut buf, 0.0, 659.25, 1.0, 0.45);
            bell(&mut buf, 0.55, 523.25, 1.3, 0.45);
        }
        "Marimba" => {
            for (i, f) in [784.0, 988.0, 784.0, 1175.0, 988.0].iter().enumerate() {
                marimba(&mut buf, i as f32 * 0.18, *f, 0.5);
            }
        }
        "Digital" => {
            for k in 0..2 {
                for i in 0..4 {
                    beep(
                        &mut buf,
                        k as f32 * 0.7 + i as f32 * 0.12,
                        1400.0,
                        0.07,
                        0.3,
                    );
                }
            }
        }
        _ => {
            // Klassisches Doppelklingeln: 2 × 0.4 s, Mischton mit Tremolo
            for start in [0.0, 0.6] {
                ring(&mut buf, start, 0.4);
            }
        }
    }
    to_pcm(&buf)
}

/// Kurzer Testton für die Lautsprecherprüfung (1 s, 440 Hz)
pub fn test_tone() -> Vec<i16> {
    let mut buf = vec![0.0f32; RATE as usize];
    beep(&mut buf, 0.0, 440.0, 0.9, 0.4);
    to_pcm(&buf)
}

fn to_pcm(buf: &[f32]) -> Vec<i16> {
    buf.iter()
        .map(|v| (v.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect()
}

fn add(buf: &mut [f32], start: f32, len: f32, mut f: impl FnMut(f32) -> f32) {
    let s = (start * RATE as f32) as usize;
    let n = (len * RATE as f32) as usize;
    for i in 0..n {
        if let Some(x) = buf.get_mut(s + i) {
            *x += f(i as f32 / RATE as f32);
        }
    }
}

fn ring(buf: &mut [f32], start: f32, len: f32) {
    add(buf, start, len, |t| {
        let env = (t / 0.01).min(1.0) * ((len - t) / 0.02).clamp(0.0, 1.0);
        let trem = 0.75 + 0.25 * (TAU * 20.0 * t).sin();
        0.22 * env * trem * ((TAU * 440.0 * t).sin() + (TAU * 480.0 * t).sin())
    });
}

fn bell(buf: &mut [f32], start: f32, f: f32, len: f32, amp: f32) {
    add(buf, start, len, |t| {
        let env = (t / 0.005).min(1.0) * (-t * 3.5).exp();
        amp * env
            * ((TAU * f * t).sin()
                + 0.4 * (TAU * f * 2.0 * t).sin()
                + 0.2 * (TAU * f * 3.01 * t).sin())
            / 1.6
    });
}

fn pluck(buf: &mut [f32], start: f32, f: f32, len: f32, amp: f32) {
    add(buf, start, len, |t| {
        let env = (t / 0.004).min(1.0) * (-t * 9.0).exp();
        amp * env * ((TAU * f * t).sin() + 0.3 * (TAU * f * 2.0 * t).sin())
    });
}

fn marimba(buf: &mut [f32], start: f32, f: f32, amp: f32) {
    add(buf, start, 0.5, |t| {
        let env = (t / 0.003).min(1.0) * (-t * 11.0).exp();
        amp * env * ((TAU * f * t).sin() + 0.25 * (TAU * f * 4.0 * t).sin() * (-t * 30.0).exp())
    });
}

fn beep(buf: &mut [f32], start: f32, f: f32, len: f32, amp: f32) {
    add(buf, start, len, |t| {
        let env = (t / 0.005).min(1.0) * ((len - t) / 0.005).clamp(0.0, 1.0);
        amp * env * (TAU * f * t).sin()
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tones_render_without_clipping() {
        for name in NAMES {
            let pcm = render(name);
            assert_eq!(pcm.len(), (RATE * 3) as usize, "{name}");
            let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap();
            assert!(peak > 3000, "{name} zu leise: {peak}");
            assert!(peak < 32767, "{name} übersteuert");
        }
        assert!(!test_tone().is_empty());
    }
}
