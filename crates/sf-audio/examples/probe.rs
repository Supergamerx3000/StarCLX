//! Prüft das Audiosystem: Geräte auflisten, Testton, eine Sekunde Mikrofonpegel.
use std::sync::Arc;

fn main() {
    let devices = sf_audio::list().expect("Geräte");
    println!("{devices:#?}");
    let speaker = devices.speakers.first().map(|d| d.name.clone());
    let play = sf_audio::Playback::start(speaker, Arc::new(sf_audio::tones::test_tone()), false);
    let ring = sf_audio::Playback::start(None, Arc::new(sf_audio::tones::render("Marimba")), true);
    let mic = std::env::args()
        .nth(1)
        .or_else(|| devices.microphones.first().map(|d| d.name.clone()));
    let meter = sf_audio::MicMeter::start(mic, |l| println!("Pegel {l:.3}"));
    std::thread::sleep(std::time::Duration::from_millis(1200));
    drop((play, ring, meter));
    std::thread::sleep(std::time::Duration::from_millis(200));
    println!("fertig");
}
