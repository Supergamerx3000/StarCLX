//! Türkameras: holt die Bilder einer Türsprechstelle mit Kamera als
//! einzelne JPEG-Bilder, wie die STARFACE App für Windows (SfTechGrp.DoorCam).
//!
//! Die Art der Quelle ergibt sich wie dort aus der URL:
//! - `rtsp://…` ist ein RTSP-Strom (H.264). Ihn entpackt `ffmpeg` zu JPEG.
//! - Endet die URL auf `mjpg` oder enthält sie `motionjpeg`, `mjpg`,
//!   `stream=` oder `fps=` (Gross-/Kleinschreibung egal), ist es ein
//!   Motion-JPEG-Strom über HTTP(S).
//! - Alles andere gilt als Einzelbild, das regelmässig neu geholt wird.
//!
//! Zugangsdaten stehen in der URL (`https://benutzer:passwort@kamera/…`)
//! und gehen per Basic-Authentifizierung an die Kamera.

use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;
use url::Url;

/// Ohne neues Bild so lange gilt die Kamera als stumm.
const STALL: Duration = Duration::from_secs(20);
/// Abstand zwischen zwei Einzelbildern
const STILL_INTERVAL: Duration = Duration::from_millis(500);
/// Bilder pro Sekunde, die ffmpeg aus RTSP liefert
const RTSP_FPS: u32 = 8;
/// Grösstes Bild; was darüber geht, wird verworfen.
const MAX_FRAME: usize = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ungültige Kamera-URL: {0}")]
    Url(String),
    #[error("Kamera nicht erreichbar: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Kamera antwortet mit {0}")]
    Status(reqwest::StatusCode),
    #[error("Kamera liefert kein Bild")]
    NoImage,
    #[error("Kamera sendet keine Bilder mehr")]
    Stalled,
    #[error("Für RTSP-Kameras wird ffmpeg benötigt")]
    NoFfmpeg,
    #[error("ffmpeg: {0}")]
    Ffmpeg(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Rtsp,
    Mjpeg,
    Still,
}

/// Art der Quelle, wie im Windows-Client aus der URL erkannt
pub fn kind_of(url: &str) -> Kind {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("rtsp://") {
        Kind::Rtsp
    } else if lower.ends_with("mjpg")
        || ["motionjpeg", "mjpg", "stream=", "fps="]
            .iter()
            .any(|p| lower.contains(p))
    {
        Kind::Mjpeg
    } else {
        Kind::Still
    }
}

/// Wie RTSP-Ströme entpackt werden
#[derive(Debug, Clone)]
pub struct Options {
    /// Befehl für ffmpeg samt Vorspann, z. B. `["ffmpeg"]` oder
    /// `["flatpak-spawn", "--host", "ffmpeg"]`. Leer: kein RTSP.
    pub ffmpeg: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            ffmpeg: vec!["ffmpeg".into()],
        }
    }
}

/// Holt Bilder, bis `on_frame` `false` liefert (dann `Ok`) oder die Kamera
/// ausfällt. Jedes Bild ist ein vollständiges JPEG.
pub async fn watch(
    url: &str,
    options: &Options,
    mut on_frame: impl FnMut(Vec<u8>) -> bool,
) -> Result<()> {
    match kind_of(url) {
        Kind::Rtsp => watch_rtsp(url.trim(), options, &mut on_frame).await,
        Kind::Mjpeg => watch_mjpeg(url, &mut on_frame).await,
        Kind::Still => watch_still(url, &mut on_frame).await,
    }
}

/// URL ohne Zugangsdaten und die Zugangsdaten selbst
fn split_credentials(url: &str) -> Result<(Url, Option<(String, String)>)> {
    let mut url = Url::parse(url.trim()).map_err(|e| Error::Url(e.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::Url(url.scheme().to_owned()));
    }
    let decode = |s: &str| {
        percent_encoding::percent_decode_str(s)
            .decode_utf8_lossy()
            .into_owned()
    };
    let creds = (!url.username().is_empty()).then(|| {
        (
            decode(url.username()),
            decode(url.password().unwrap_or_default()),
        )
    });
    // Kann bei http(s) nicht scheitern
    let _ = url.set_username("");
    let _ = url.set_password(None);
    Ok((url, creds))
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("starclx/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(STALL)
        .tls_backend_preconfigured(sf_tls::client_config())
        .build()?)
}

async fn get(client: &reqwest::Client, url: &str) -> Result<reqwest::Response> {
    let (url, creds) = split_credentials(url)?;
    let mut req = client.get(url);
    if let Some((user, password)) = creds {
        req = req.basic_auth(user, Some(password));
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        return Err(Error::Status(resp.status()));
    }
    Ok(resp)
}

async fn watch_mjpeg(url: &str, on_frame: &mut impl FnMut(Vec<u8>) -> bool) -> Result<()> {
    let client = http_client()?;
    let mut resp = get(&client, url).await?;
    let mut frames = JpegSplitter::default();
    let mut any = false;
    while let Some(chunk) = resp.chunk().await? {
        for frame in frames.push(&chunk) {
            any = true;
            if !on_frame(frame) {
                return Ok(());
            }
        }
    }
    Err(if any { Error::Stalled } else { Error::NoImage })
}

async fn watch_still(url: &str, on_frame: &mut impl FnMut(Vec<u8>) -> bool) -> Result<()> {
    let client = http_client()?;
    loop {
        let body = get(&client, url).await?.bytes().await?;
        let frame = JpegSplitter::default()
            .push(&body)
            .into_iter()
            .next()
            .ok_or(Error::NoImage)?;
        if !on_frame(frame) {
            return Ok(());
        }
        tokio::time::sleep(STILL_INTERVAL).await;
    }
}

async fn watch_rtsp(
    url: &str,
    options: &Options,
    on_frame: &mut impl FnMut(Vec<u8>) -> bool,
) -> Result<()> {
    let (program, prefix) = options.ffmpeg.split_first().ok_or(Error::NoFfmpeg)?;
    let fps = format!("fps={RTSP_FPS}");
    let mut child = Command::new(program)
        .args(prefix)
        .args(["-nostdin", "-hide_banner", "-loglevel", "error"])
        .args(["-fflags", "nobuffer", "-rtsp_transport", "tcp"])
        .args(["-i", url, "-an", "-vf", &fps])
        .args(["-f", "image2pipe", "-c:v", "mjpeg", "-q:v", "5", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NoFfmpeg,
            _ => Error::Ffmpeg(e.to_string()),
        })?;
    let mut stdout = child.stdout.take().expect("stdout ist umgeleitet");
    let mut stderr = child.stderr.take().expect("stderr ist umgeleitet");
    // Fehlermeldungen nebenher lesen, damit ffmpeg nie daran hängt
    let errors = tokio::spawn(async move {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text).await;
        text
    });

    let mut frames = JpegSplitter::default();
    let mut buf = vec![0; 64 * 1024];
    let result = loop {
        let n = match tokio::time::timeout(STALL, stdout.read(&mut buf)).await {
            Err(_) => break Err(Error::Stalled),
            Ok(Err(e)) => break Err(Error::Ffmpeg(e.to_string())),
            Ok(Ok(0)) => break Err(Error::Stalled),
            Ok(Ok(n)) => n,
        };
        if frames.push(&buf[..n]).into_iter().any(|f| !on_frame(f)) {
            break Ok(());
        }
    };
    // Geschlossene Pipe beendet auch ein über flatpak-spawn gestartetes ffmpeg.
    drop(stdout);
    let _ = child.start_kill();
    let _ = child.wait().await;
    match result {
        Err(Error::Stalled) => {
            let text = errors.await.unwrap_or_default();
            match text.lines().rev().find(|l| !l.trim().is_empty()) {
                Some(line) => Err(Error::Ffmpeg(line.trim().to_owned())),
                None => Err(Error::Stalled),
            }
        }
        other => other,
    }
}

/// Zerlegt einen Bytestrom in einzelne JPEG-Bilder. Folgt dazu dem Aufbau
/// der Datei (Segmente mit Länge, dann Bilddaten bis zum Endmarker), damit
/// eingebettete Vorschaubilder (EXIF) ein Bild nicht vorzeitig beenden.
/// Rahmen wie die Multipart-Grenzen von MJPEG werden dabei übersprungen.
#[derive(Default)]
pub struct JpegSplitter {
    buf: Vec<u8>,
}

impl JpegSplitter {
    /// Hängt Daten an und liefert alle Bilder, die damit vollständig sind.
    pub fn push(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        self.buf.extend_from_slice(data);
        let mut out = Vec::new();
        loop {
            let Some(start) = self.buf.windows(2).position(|w| w == [0xFF, 0xD8]) else {
                // Ein einzelnes 0xFF am Ende kann der Anfang des nächsten sein.
                let keep = usize::from(self.buf.last() == Some(&0xFF));
                self.buf.drain(..self.buf.len() - keep);
                break;
            };
            self.buf.drain(..start);
            match frame_end(&self.buf) {
                Ok(Some(end)) => out.push(self.buf.drain(..end).collect()),
                Ok(None) => {
                    if self.buf.len() > MAX_FRAME {
                        self.buf.clear();
                    }
                    break;
                }
                // Kein gültiges Bild: hinter diesem Anfang neu suchen
                Err(()) => {
                    self.buf.drain(..2);
                }
            }
        }
        out
    }
}

/// Ende des JPEG-Bildes, das bei `buf[0]` (Marker SOI) beginnt.
/// `Ok(None)`: noch unvollständig, `Err`: kein gültiges JPEG.
fn frame_end(buf: &[u8]) -> std::result::Result<Option<usize>, ()> {
    let mut p = 2;
    loop {
        if p >= buf.len() {
            return Ok(None);
        }
        if buf[p] != 0xFF {
            return Err(());
        }
        // Füllbytes 0xFF vor dem Marker
        while p + 1 < buf.len() && buf[p + 1] == 0xFF {
            p += 1;
        }
        let Some(&marker) = buf.get(p + 1) else {
            return Ok(None);
        };
        match marker {
            0xD9 => return Ok(Some(p + 2)),
            0xD8 | 0x00 => return Err(()),
            // Marker ohne Länge
            0x01 | 0xD0..=0xD7 => p += 2,
            _ => {
                let Some(len) = buf.get(p + 2..p + 4) else {
                    return Ok(None);
                };
                let len = usize::from(u16::from_be_bytes([len[0], len[1]]));
                if len < 2 {
                    return Err(());
                }
                p += 2 + len;
                if marker == 0xDA {
                    // Bilddaten bis zum nächsten echten Marker; 0xFF 0x00 ist
                    // ein maskiertes 0xFF, 0xFF 0xD0–D7 ein Restart-Marker.
                    loop {
                        let Some(q) = buf.get(p..).and_then(|b| b.iter().position(|&b| b == 0xFF))
                        else {
                            return Ok(None);
                        };
                        p += q;
                        match buf.get(p + 1) {
                            None => return Ok(None),
                            Some(0x00 | 0xD0..=0xD7) => p += 2,
                            Some(0xFF) => p += 1,
                            Some(_) => break,
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kleines, gültig aufgebautes JPEG (Inhalt ist egal)
    fn jpeg(tag: u8) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8];
        // APP0 mit Länge 6
        v.extend([0xFF, 0xE0, 0x00, 0x06, b'J', b'F', b'I', tag]);
        // SOS mit Länge 4, dann Bilddaten mit maskiertem 0xFF und Restart
        v.extend([0xFF, 0xDA, 0x00, 0x04, 0x01, 0x02]);
        v.extend([0x10, 0xFF, 0x00, 0x20, 0xFF, 0xD3, 0x30, tag]);
        v.extend([0xFF, 0xD9]);
        v
    }

    #[test]
    fn detects_kind_like_windows() {
        assert_eq!(kind_of("rtsp://cam/live"), Kind::Rtsp);
        assert_eq!(kind_of("RTSP://cam/live"), Kind::Rtsp);
        assert_eq!(kind_of("http://cam/video.MJPG"), Kind::Mjpeg);
        assert_eq!(kind_of("http://cam/cgi?stream=1"), Kind::Mjpeg);
        assert_eq!(kind_of("http://cam/api/MotionJPEG"), Kind::Mjpeg);
        assert_eq!(kind_of("http://cam/video?fps=5"), Kind::Mjpeg);
        assert_eq!(kind_of("http://cam/snapshot.jpg"), Kind::Still);
    }

    #[test]
    fn credentials_leave_the_url() {
        let (url, creds) = split_credentials("https://ad%40min:p%3Ass@cam.local/a.mjpg").unwrap();
        assert_eq!(url.as_str(), "https://cam.local/a.mjpg");
        assert_eq!(creds, Some(("ad@min".into(), "p:ss".into())));
        let (_, creds) = split_credentials("http://cam/a.jpg").unwrap();
        assert_eq!(creds, None);
        assert!(split_credentials("ftp://cam/a.jpg").is_err());
    }

    #[test]
    fn splits_multipart_stream_in_pieces() {
        let mut stream = Vec::new();
        for tag in [1, 2, 3] {
            stream.extend(b"--boundary\r\nContent-Type: image/jpeg\r\n\r\n");
            stream.extend(jpeg(tag));
            stream.extend(b"\r\n");
        }
        // In kleinen, ungeraden Stücken anliefern
        let mut s = JpegSplitter::default();
        let frames: Vec<_> = stream.chunks(5).flat_map(|c| s.push(c)).collect();
        assert_eq!(frames, vec![jpeg(1), jpeg(2), jpeg(3)]);
    }

    #[test]
    fn embedded_thumbnail_does_not_end_frame() {
        let mut v = vec![0xFF, 0xD8];
        // APP1 mit einem eingebetteten Mini-JPEG (SOI … EOI)
        let thumb = [0xFF, 0xD8, 0xFF, 0xD9];
        v.extend([0xFF, 0xE1, 0x00, (2 + thumb.len()) as u8]);
        v.extend(thumb);
        v.extend([0xFF, 0xDA, 0x00, 0x02, 0x55, 0xFF, 0xD9]);
        let frames = JpegSplitter::default().push(&v);
        assert_eq!(frames, vec![v]);
    }

    #[test]
    fn skips_garbage_and_resyncs() {
        let mut stream = vec![0xFF, 0xD8, 0x12, 0x34];
        stream.extend(jpeg(7));
        let frames = JpegSplitter::default().push(&stream);
        assert_eq!(frames, vec![jpeg(7)]);
    }

    #[tokio::test]
    async fn reads_still_image_over_http() {
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let body = jpeg(9);
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut req = vec![0; 2048];
            let n = sock.read(&mut req).await.unwrap();
            let req = String::from_utf8_lossy(&req[..n]).to_ascii_lowercase();
            // "user:pw" in Base64
            assert!(req.contains("authorization: basic dxnlcjpwdw=="), "{req}");
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            sock.write_all(head.as_bytes()).await.unwrap();
            sock.write_all(&body).await.unwrap();
        });
        let mut got = Vec::new();
        watch(
            &format!("http://user:pw@{addr}/snapshot.jpg"),
            &Options::default(),
            |f| {
                got.push(f);
                false
            },
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(got, vec![jpeg(9)]);
    }
}
