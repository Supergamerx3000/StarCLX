//! Linux: PulseAudio-API (unter PipeWire über pipewire-pulse).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use libpulse_binding as pulse;
use libpulse_simple_binding::Simple;
use pulse::callbacks::ListResult;
use pulse::context::{Context, FlagSet, State};
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::sample::{Format, Spec};
use pulse::stream::Direction;

use crate::{Device, Devices, Error, RATE, peak};

/// Fragt die vorhandenen Ausgabe- und Eingabegeräte ab (ohne Monitore).
pub fn list() -> Result<Devices, Error> {
    let err = |e: &dyn std::fmt::Display| Error::Pulse(e.to_string());
    let mut ml = Mainloop::new().ok_or_else(|| Error::Pulse("Mainloop".into()))?;
    let mut ctx = Context::new(&ml, "StarCLX").ok_or_else(|| Error::Pulse("Kontext".into()))?;
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
                "StarCLX",
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
                "StarCLX",
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
