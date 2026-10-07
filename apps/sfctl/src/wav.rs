//! WAV-Dateien für Ansagen: liest 16-bit-PCM in beliebiger Rate und
//! Kanalzahl und schreibt sie als 16 kHz mono, passend zur festen Quellrate
//! des Softphones bei `sfctl say`.

use anyhow::{Result, bail};

pub const RATE: u32 = 16_000;

/// Wandelt eine WAV-Datei (16-bit-PCM) in 16 kHz mono um.
pub fn to_16k_mono(bytes: &[u8]) -> Result<Vec<u8>> {
    let (rate, channels, samples) = parse(bytes)?;
    let mono: Vec<f32> = samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().map(|&s| f32::from(s)).sum::<f32>() / channels as f32)
        .collect();
    Ok(write(&resample(&mono, rate, RATE)))
}

fn parse(bytes: &[u8]) -> Result<(u32, usize, Vec<i16>)> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        bail!("keine WAV-Datei");
    }
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at =
        |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let mut fmt = None;
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32_at(pos + 4) as usize;
        let body = pos + 8;
        // Manche Programme (z. B. beim Schreiben in eine Pipe) lassen die
        // Länge des Datenblocks offen; dann gilt der Rest der Datei.
        let end = body.saturating_add(len).min(bytes.len());
        match id {
            b"fmt " if len >= 16 && end >= body + 16 => {
                fmt = Some((
                    u16_at(body),
                    u16_at(body + 2),
                    u32_at(body + 4),
                    u16_at(body + 14),
                ));
            }
            b"data" => {
                let Some((format, channels, rate, bits)) = fmt else {
                    bail!("WAV ohne Formatangabe");
                };
                // 1 = PCM, 0xFFFE = WAVE_FORMAT_EXTENSIBLE (bei 16 bit PCM)
                if !(format == 1 || format == 0xFFFE) || bits != 16 {
                    bail!("nur 16-bit-PCM-WAV wird unterstützt (Format {format}, {bits} bit)");
                }
                if channels == 0 || rate == 0 {
                    bail!("ungültige WAV-Datei");
                }
                let samples = bytes[body..end]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&b| i16::from_le_bytes(b))
                    .collect();
                return Ok((rate, usize::from(channels), samples));
            }
            _ => {}
        }
        pos = body + len + (len & 1);
    }
    bail!("WAV ohne Audiodaten")
}

/// Lineare Interpolation; für Sprache am Telefon genügt das.
fn resample(input: &[f32], from: u32, to: u32) -> Vec<i16> {
    if input.is_empty() {
        return Vec::new();
    }
    let n = (input.len() as u64 * u64::from(to) / u64::from(from)) as usize;
    let step = f64::from(from) / f64::from(to);
    (0..n)
        .map(|i| {
            let x = i as f64 * step;
            let j = x as usize;
            let frac = (x - j as f64) as f32;
            let a = input[j.min(input.len() - 1)];
            let b = input[(j + 1).min(input.len() - 1)];
            (a + (b - a) * frac).round().clamp(-32768.0, 32767.0) as i16
        })
        .collect()
}

/// Schreibt 16 kHz mono 16 bit.
pub fn write(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + samples.len() * 2);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&RATE.to_le_bytes());
    w.extend_from_slice(&(RATE * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        w.extend_from_slice(&s.to_le_bytes());
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data_len).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&channels.to_le_bytes());
        w.extend_from_slice(&rate.to_le_bytes());
        w.extend_from_slice(&(rate * 2 * u32::from(channels)).to_le_bytes());
        w.extend_from_slice(&(2 * channels).to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            w.extend_from_slice(&s.to_le_bytes());
        }
        w
    }

    #[test]
    fn keeps_16k_mono() {
        let input = wav(16_000, 1, &[0, 100, -100, 32767]);
        assert_eq!(to_16k_mono(&input).unwrap(), input);
    }

    #[test]
    fn downmixes_and_resamples() {
        // 32 kHz stereo, 8 Frames → 4 Abtastwerte bei 16 kHz
        let frames: Vec<i16> = (0..8).flat_map(|i| [i * 100, i * 100 + 50]).collect();
        let out = to_16k_mono(&wav(32_000, 2, &frames)).unwrap();
        let (rate, ch, samples) = parse(&out).unwrap();
        assert_eq!((rate, ch), (16_000, 1));
        assert_eq!(samples, vec![25, 225, 425, 625]);
    }

    #[test]
    fn rejects_other_formats() {
        let mut input = wav(16_000, 1, &[0; 4]);
        input[34] = 8; // 8 bit
        assert!(to_16k_mono(&input).is_err());
        assert!(to_16k_mono(b"kein wav").is_err());
    }
}
