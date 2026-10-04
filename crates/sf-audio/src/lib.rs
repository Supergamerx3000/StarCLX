//! Audio neben dem Softphone: Geräteliste, Klingeltöne und Testton auf
//! einem gewählten Gerät, Pegelanzeige fürs Mikrofon.
//!
//! Linux: PulseAudio-API (unter PipeWire über pipewire-pulse); Gerätenamen
//! sind die Knotennamen, baresip öffnet dasselbe Gerät mit `pipewire,<name>`.
//! macOS und Windows: cpal; baresip öffnet dasselbe Gerät mit
//! `coreaudio,<name>` bzw. `wasapi,<Endpunkt-ID>`.

pub mod tones;

#[cfg(any(target_os = "macos", windows))]
mod native;
#[cfg(target_os = "linux")]
mod pulse;

#[cfg(any(target_os = "macos", windows))]
pub use native::{MicMeter, Playback, list};
#[cfg(target_os = "linux")]
pub use pulse::{MicMeter, Playback, list};

use std::time::Duration;

use serde::Serialize;

/// Platzhalter in Gerätelisten für das Standardgerät des Systems
pub const DEFAULT_DEVICE: &str = "@default";
pub const RATE: u32 = 44_100;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Audiosystem nicht erreichbar: {0}")]
    Pulse(String),
    #[error("Audiogerät: {0}")]
    Device(String),
    #[error("WAV-Datei: {0}")]
    Wav(#[from] hound::Error),
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Devices {
    pub speakers: Vec<Device>,
    pub microphones: Vec<Device>,
}

/// Erstes bevorzugtes Gerät, das vorhanden ist. `None` heisst Standardgerät.
pub fn pick(preferred: &[String], present: &[Device]) -> Option<String> {
    preferred
        .iter()
        .find(|p| p.as_str() == DEFAULT_DEVICE || present.iter().any(|d| &d.name == *p))
        .filter(|p| p.as_str() != DEFAULT_DEVICE)
        .cloned()
}

/// Spitzenpegel von 16-Bit-PCM, auf 0..1 normiert
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn peak(bytes: &[u8]) -> f32 {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b).unsigned_abs())
        .max()
        .map_or(0.0, |m| f32::from(m) / 32768.0)
}

/// Lädt eine WAV-Datei als Mono-PCM mit [`RATE`] (einfache Umrechnung).
pub fn load_wav(path: &std::path::Path) -> Result<Vec<i16>, Error> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels.max(1));
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()?
        }
    };
    let mono: Vec<f32> = raw
        .chunks(channels)
        .map(|c| c.iter().sum::<f32>() / c.len() as f32)
        .collect();
    Ok(resample(&mono, spec.sample_rate, RATE)
        .into_iter()
        .map(|v| (v.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect())
}

pub(crate) fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = f64::from(from) / f64::from(to);
    let len = (input.len() as f64 / ratio) as usize;
    (0..len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let a = pos.floor() as usize;
            let b = (a + 1).min(input.len() - 1);
            let t = (pos - a as f64) as f32;
            input[a] * (1.0 - t) + input[b] * t
        })
        .collect()
}

/// Dauer von PCM in [`RATE`]
pub fn duration(samples: &[i16]) -> Duration {
    Duration::from_secs_f64(samples.len() as f64 / f64::from(RATE))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str) -> Device {
        Device {
            name: name.into(),
            description: String::new(),
        }
    }

    #[test]
    fn pick_takes_first_present_device() {
        let present = [dev("usb-headset"), dev("speakers")];
        let pref = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            pick(&pref(&["jabra", "speakers"]), &present).as_deref(),
            Some("speakers")
        );
        assert_eq!(pick(&pref(&["@default", "speakers"]), &present), None);
        assert_eq!(pick(&pref(&["jabra"]), &present), None);
        assert_eq!(pick(&[], &present), None);
    }

    #[test]
    fn wav_roundtrip_mono_and_resample() {
        let path = std::env::temp_dir().join(format!("sf-audio-test-{}.wav", std::process::id()));
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 22_050,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..22_050 {
            w.write_sample(16_384i16).unwrap();
            w.write_sample(16_384i16).unwrap();
        }
        w.finalize().unwrap();
        let pcm = load_wav(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert!(
            (pcm.len() as i64 - i64::from(RATE)).abs() < 4,
            "{}",
            pcm.len()
        );
        assert!((pcm[100] - 16_383).abs() < 3);
    }

    #[test]
    fn peak_level() {
        let bytes: Vec<u8> = [0i16, -16_384, 100]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        assert!((peak(&bytes) - 0.5).abs() < 0.001);
    }
}
