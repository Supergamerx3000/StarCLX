//! Anmeldefenster in der App statt im Systembrowser.
//!
//! Browser wie Firefox lehnen selbstsignierte Zertifikate unter einem
//! Hostnamen mit HSTS ab. Das eigene Fenster lässt Zertifikate zu, die der
//! Benutzer im Client bestätigt hat, und fängt die Weiterleitung auf
//! `starface-app://login` direkt ab.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "login";

pub fn open(app: &AppHandle, url: &url::Url) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.close();
    }
    let handle = app.clone();
    // Erst leer öffnen: die Zertifikatsfreigabe muss stehen, bevor die
    // Anmeldeseite lädt.
    let window = WebviewWindowBuilder::new(
        app,
        LABEL,
        WebviewUrl::External("about:blank".parse().unwrap()),
    )
    .title("STARFACE – Anmelden")
    .inner_size(480.0, 680.0)
    .on_navigation(move |url| {
        if !url.as_str().starts_with(sf_auth::REDIRECT_URI) {
            return true;
        }
        crate::handle_urls(&handle, vec![url.to_string()]);
        let handle = handle.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(window) = handle.get_webview_window(LABEL) {
                let _ = window.close();
            }
        });
        false
    })
    .build()?;
    #[cfg(target_os = "linux")]
    window.with_webview(allow_confirmed_certificates)?;
    window.navigate(url.clone())
}

/// Lässt WebKit ein vom Benutzer bestätigtes Zertifikat annehmen und lädt
/// die Seite neu.
#[cfg(target_os = "linux")]
fn allow_confirmed_certificates(webview: tauri::webview::PlatformWebview) {
    use webkit2gtk::gio::prelude::TlsCertificateExt;
    use webkit2gtk::{WebContextExt, WebViewExt};

    webview
        .inner()
        .connect_load_failed_with_tls_errors(|view, uri, cert, _| {
            let Some(der) = cert.certificate() else {
                return false;
            };
            if !sf_tls::is_confirmed_fingerprint(&sf_tls::fingerprint(&der)) {
                return false;
            }
            let Some(host) = url::Url::parse(uri)
                .ok()
                .and_then(|u| u.host_str().map(|h| h.trim_matches(['[', ']']).to_owned()))
            else {
                return false;
            };
            if let Some(context) = view.context() {
                context.allow_tls_certificate_for_host(cert, &host);
                view.load_uri(uri);
                return true;
            }
            false
        });
}
