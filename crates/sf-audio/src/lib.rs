//! Audio neben dem Softphone: Geräteliste (PulseAudio-API, unter PipeWire
//! über pipewire-pulse), Klingeltöne und Testton auf einem gewählten Gerät,
//! Pegelanzeige fürs Mikrofon.
//!
//! Gerätenamen sind die Knotennamen von PipeWire bzw. PulseAudio; baresip
//! öffnet dasselbe Gerät mit `pipewire,<name>`.

pub mod tones;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use libpulse_binding as pulse;
use libpulse_simple_binding::Simple;
use pulse::callbacks::ListResult;
use pulse::context::{Context, FlagSet, State};
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::sample::{Format, Spec};
use pulse::stream::Direction;
use serde::Serialize;

/// Platzhalter in Gerätelisten für das Standardgerät des Systems
pub const DEFAULT_DEVICE: &str = "@default";
pub const RATE: u32 = 44_100;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Audiosystem nicht erreichbar: {0}")]
    Pulse(String),
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

/// Fragt die vorhandenen Ausgabe- und Eingabegeräte ab (ohne Monitore).
pub fn list() -> Result<Devices, Error> {
    let err = |e: &dyn std::fmt::Display| Error::Pulse(e.to_string());
    let mut ml = Mainloop::new().ok_or_else(|| Error::Pulse("Mainloop".into()))?;
    let mut ctx = Context::new(&ml, "STARFACE").ok_or_else(|| Error::Pulse("Kontext".into()))?;
    ctx.connect(None, FlagSet::NOAUTOSPAWN, None)
        .map_err(|e| err(&e))?;
    loop {
        match ml.iterate(true) {
            IterateResult::Success(_) => {}
            IterateResult::Quit(_) | IterateResult::Err(_) => {
                return Err(Error::Pulse("Verbindung abgebrochen".into()));
            }
        }
        match ctx.get_state() {
            State::Ready => break,
            State::Failed | State::Terminated => {
                return Err(Error::Pulse("Verbindung fehlgeschlagen".into()));
            }
            _ => {}
        }
    }

    let out = Rc::new(RefCell::new(Devices::default()));
    let pending = Rc::new(RefCell::new(2));
    {
        let (out, pending) = (out.clone(), pending.clone());
        ctx.introspect().get_sink_info_list(move |r| match r {
            ListResult::Item(i) => out.borrow_mut().speakers.push(Device {
                name: i.name.as_deref().unwrap_or_default().to_owned(),
                description: i.description.as_deref().unwrap_or_default().to_owned(),
            }),
            _ => *pending.borrow_mut() -= 1,
        });
    }
    {
        let (out, pending) = (out.clone(), pending.clone());
        ctx.introspect().get_source_info_list(move |r| match r {
            ListResult::Item(i) if i.monitor_of_sink.is_none() => {
                out.borrow_mut().microphones.push(Device {
                    name: i.name.as_deref().unwrap_or_default().to_owned(),
                    description: i.description.as_deref().unwrap_or_default().to_owned(),
                });
            }
            ListResult::Item(_) => {}
            _ => *pending.borrow_mut() -= 1,
        });
    }
    while *pending.borrow() > 0 {
        if let IterateResult::Quit(_) | IterateResult::Err(_) = ml.iterate(true) {
            break;
        }
    }
    ctx.disconnect();
    Ok(out.take())
}

/// Erstes bevorzugtes Gerät, das vorhanden ist. `None` heisst Standardgerät.
pub fn pick(preferred: &[String], present: &[Device]) -> Option<String> {
    preferred
        .iter()
        .find(|p| p.as_str() == DEFAULT_DEVICE || present.iter().any(|d| &d.name == *p))
        .filter(|p| p.as_str() != DEFAULT_DEVICE)
        .cloned()
}

fn spec(channels: u8) -> Spec {
    Spec {
        format: Format::S16le,
        channels,
        rate: RATE,
    }
}

/// Spielt Mono-PCM (16 Bit, [`RATE`]) ab, bis es fertig ist oder die
/// Wiedergabe fallen gelassen wird.
pub struct Playback {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Playback {
    pub fn start(device: Option<String>, samples: Arc<Vec<i16>>, repeat: bool) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            let simple = match Simple::new(
                None,
                "STARFACE",
                Direction::Playback,
                device.as_deref(),
                "Klingelton",
                &spec(1),
                None,
                None,
            ) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, ?device, "Wiedergabe nicht möglich");
                    return;
                }
            };
            // In Stücken von 50 ms schreiben, damit Stop schnell greift.
            let chunk = (RATE / 20) as usize;
            'outer: loop {
                for part in samples.chunks(chunk) {
                    if flag.load(Ordering::Relaxed) {
                        break 'outer;
                    }
                    let bytes: Vec<u8> = part.iter().flat_map(|s| s.to_le_bytes()).collect();
                    if simple.write(&bytes).is_err() {
                        break 'outer;
                    }
                }
                if !repeat {
                    let _ = simple.drain();
                    break;
                }
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Nicht blockieren: der Thread endet nach dem nächsten Stück selbst.
        drop(self.thread.take());
    }
}

/// Liest das Mikrofon und meldet etwa zehnmal pro Sekunde den Pegel (0..1).
pub struct MicMeter {
    stop: Arc<AtomicBool>,
}

impl MicMeter {
    pub fn start(device: Option<String>, mut level: impl FnMut(f32) + Send + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            let simple = match Simple::new(
                None,
                "STARFACE",
                Direction::Record,
                device.as_deref(),
                "Mikrofontest",
                &spec(1),
                None,
                None,
            ) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, ?device, "Mikrofon nicht geöffnet");
                    return;
                }
            };
            let mut buf = vec![0u8; (RATE / 10) as usize * 2];
            while !flag.load(Ordering::Relaxed) {
                if simple.read(&mut buf).is_err() {
                    break;
                }
                level(peak(&buf));
            }
        });
        Self { stop }
    }
}

impl Drop for MicMeter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Spitzenpegel von 16-Bit-PCM, auf 0..1 normiert
fn peak(bytes: &[u8]) -> f32 {
    bytes
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs())
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

fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
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
