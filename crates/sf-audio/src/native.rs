//! macOS (CoreAudio) und Windows (WASAPI) über cpal.
//!
//! Der Gerätename ist das, womit baresip das Gerät öffnet: unter macOS der
//! CoreAudio-Name (Modul coreaudio), unter Windows die Endpunkt-ID von
//! WASAPI (Modul wasapi). Angezeigt wird der lesbare Name.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::{Device, Devices, Error, RATE};

fn label(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|desc| desc.name().to_owned())
}

#[cfg(target_os = "macos")]
fn device_name(d: &cpal::Device) -> Option<String> {
    label(d)
}

#[cfg(windows)]
fn device_name(d: &cpal::Device) -> Option<String> {
    d.id().ok().map(|id| id.id().to_owned())
}

fn devices(input: bool) -> Result<Vec<Device>, Error> {
    let host = cpal::default_host();
    let list: Vec<cpal::Device> = if input {
        host.input_devices()
            .map_err(|e| Error::Device(e.to_string()))?
            .collect()
    } else {
        host.output_devices()
            .map_err(|e| Error::Device(e.to_string()))?
            .collect()
    };
    Ok(list
        .iter()
        .filter_map(|d| {
            let name = device_name(d)?;
            Some(Device {
                description: label(d).unwrap_or_else(|| name.clone()),
                name,
            })
        })
        .collect())
}

/// Fragt die vorhandenen Ausgabe- und Eingabegeräte ab.
pub fn list() -> Result<Devices, Error> {
    Ok(Devices {
        speakers: devices(false)?,
        microphones: devices(true)?,
    })
}

/// Gerät nach Namen, sonst das Standardgerät
fn open(name: Option<&str>, input: bool) -> Option<cpal::Device> {
    let host = cpal::default_host();
    let found = name.and_then(|name| {
        let mut all: Box<dyn Iterator<Item = cpal::Device>> = if input {
            Box::new(host.input_devices().ok()?)
        } else {
            Box::new(host.output_devices().ok()?)
        };
        all.find(|d| device_name(d).as_deref() == Some(name))
    });
    found.or_else(|| {
        if input {
            host.default_input_device()
        } else {
            host.default_output_device()
        }
    })
}

/// Spielt Mono-PCM (16 Bit, [`RATE`]) ab, bis es fertig ist oder die
/// Wiedergabe fallen gelassen wird.
pub struct Playback {
    stop: Arc<AtomicBool>,
}

impl Playback {
    pub fn start(device: Option<String>, samples: Arc<Vec<i16>>, repeat: bool) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            if let Err(e) = play(device.as_deref(), &samples, repeat, &flag) {
                tracing::warn!(error = %e, ?device, "Wiedergabe nicht möglich");
            }
        });
        Self { stop }
    }
}

fn play(
    device: Option<&str>,
    samples: &[i16],
    repeat: bool,
    stop: &AtomicBool,
) -> Result<(), Error> {
    if samples.is_empty() {
        return Ok(());
    }
    let err = |e: &dyn std::fmt::Display| Error::Device(e.to_string());
    let dev = open(device, false).ok_or_else(|| Error::Device("kein Ausgabegerät".into()))?;
    let config = dev.default_output_config().map_err(|e| err(&e))?.config();
    let channels = usize::from(config.channels.max(1));
    let mono: Vec<f32> = samples.iter().map(|s| f32::from(*s) / 32768.0).collect();
    let pcm: Arc<Vec<f32>> = Arc::new(crate::resample(&mono, RATE, config.sample_rate));
    let pos = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let stream = {
        let (pcm, pos, done) = (pcm.clone(), pos.clone(), done.clone());
        dev.build_output_stream::<f32, _, _>(
            config,
            move |out, _| {
                let mut p = pos.load(Ordering::Relaxed);
                for frame in out.chunks_mut(channels) {
                    if p >= pcm.len() {
                        if !repeat {
                            done.store(true, Ordering::Relaxed);
                            frame.fill(0.0);
                            continue;
                        }
                        p = 0;
                    }
                    frame.fill(pcm[p]);
                    p += 1;
                }
                pos.store(p, Ordering::Relaxed);
            },
            |e| tracing::warn!(error = %e, "Wiedergabe gestört"),
            None,
        )
        .map_err(|e| err(&e))?
    };
    stream.play().map_err(|e| err(&e))?;
    while !stop.load(Ordering::Relaxed) && !done.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(50));
    }
    if done.load(Ordering::Relaxed) {
        // Puffer des Geräts auslaufen lassen
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
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
            let err = |e: &dyn std::fmt::Display| Error::Device(e.to_string());
            let mut run = || -> Result<(), Error> {
                let dev = open(device.as_deref(), true)
                    .ok_or_else(|| Error::Device("kein Mikrofon".into()))?;
                let config = dev.default_input_config().map_err(|e| err(&e))?.config();
                // Spitzenpegel seit der letzten Meldung, als f32-Bits
                let peak = Arc::new(AtomicU32::new(0));
                let sink = peak.clone();
                let stream = dev
                    .build_input_stream::<f32, _, _>(
                        config,
                        move |data, _| {
                            let m = data.iter().fold(0f32, |m, s| m.max(s.abs()));
                            let old = f32::from_bits(sink.load(Ordering::Relaxed));
                            if m > old {
                                sink.store(m.to_bits(), Ordering::Relaxed);
                            }
                        },
                        |e| tracing::warn!(error = %e, "Mikrofon gestört"),
                        None,
                    )
                    .map_err(|e| err(&e))?;
                stream.play().map_err(|e| err(&e))?;
                while !flag.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(100));
                    level(f32::from_bits(peak.swap(0, Ordering::Relaxed)).min(1.0));
                }
                Ok(())
            };
            if let Err(e) = run() {
                tracing::warn!(error = %e, ?device, "Mikrofon nicht geöffnet");
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
