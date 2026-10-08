//! Prüft eine Türkamera ohne App: `cargo run -p sf-doorcam --example probe -- <URL> [Bilder]`
//! Zeigt Art der Quelle, Grösse der Bilder und Bildrate; speichert das
//! erste Bild als `doorcam.jpg`.

use std::time::Instant;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let url = args.next().expect("URL der Kamera angeben");
    let count: usize = args.next().and_then(|n| n.parse().ok()).unwrap_or(20);
    println!("Art: {:?}", sf_doorcam::kind_of(&url));
    let start = Instant::now();
    let mut n = 0;
    let result = sf_doorcam::watch(&url, &sf_doorcam::Options::default(), |frame| {
        if n == 0 {
            std::fs::write("doorcam.jpg", &frame).expect("doorcam.jpg schreiben");
        }
        n += 1;
        println!(
            "Bild {n}: {} Bytes nach {:.1?}",
            frame.len(),
            start.elapsed()
        );
        n < count
    })
    .await;
    if let Err(e) = result {
        eprintln!("Fehler: {e}");
        std::process::exit(1);
    }
    let fps = n as f64 / start.elapsed().as_secs_f64();
    println!("{n} Bilder, {fps:.1} pro Sekunde");
}
