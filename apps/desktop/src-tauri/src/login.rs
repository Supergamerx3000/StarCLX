//! Anmeldefenster in der App statt im Systembrowser.
//!
//! Browser wie Firefox lehnen selbstsignierte Zertifikate unter einem
//! Hostnamen mit HSTS ab. Das eigene Fenster lässt Zertifikate zu, die der
//! Benutzer im Client bestätigt hat (WebKitGTK, WKWebView, WebView2), und
//! fängt die Weiterleitung auf `starface-app://login` direkt ab.

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
    .title(crate::i18n::t("StarCLX – Anmelden"))
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
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
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

/// WKWebView: wrys Navigation-Delegate beantwortet keine
/// Zertifikatsabfragen. Die Methode wird zur Laufzeit ergänzt; danach wird
/// der Delegate neu gesetzt, weil WebKit beim Setzen nachsieht, welche
/// Methoden er kennt.
#[cfg(target_os = "macos")]
fn allow_confirmed_certificates(webview: tauri::webview::PlatformWebview) {
    mac::install(webview.inner().cast());
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{c_long, c_void};
    use std::ptr::null_mut;

    use block2::Block;
    use objc2::encode::{Encode, Encoding, RefEncode};
    use objc2::runtime::{AnyClass, AnyObject, Sel};
    use objc2::{class, msg_send, sel};

    /// SecTrustRef mit passender Objective-C-Kodierung
    #[repr(transparent)]
    #[derive(Clone, Copy)]
    struct SecTrust(*mut c_void);

    // SAFETY: Zeiger auf die opake Struktur __SecTrust
    unsafe impl Encode for SecTrust {
        const ENCODING: Encoding = Encoding::Pointer(&Encoding::Struct("__SecTrust", &[]));
    }
    // SAFETY: wie oben
    unsafe impl RefEncode for SecTrust {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
    }

    type CFTypeRef = *const c_void;

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecTrustCopyCertificateChain(trust: *mut c_void) -> CFTypeRef;
        fn SecCertificateCopyData(cert: CFTypeRef) -> CFTypeRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFArrayGetCount(array: CFTypeRef) -> c_long;
        fn CFArrayGetValueAtIndex(array: CFTypeRef, index: c_long) -> CFTypeRef;
        fn CFDataGetBytePtr(data: CFTypeRef) -> *const u8;
        fn CFDataGetLength(data: CFTypeRef) -> c_long;
        fn CFRelease(obj: CFTypeRef);
    }

    /// NSURLSessionAuthChallengeDisposition
    const USE_CREDENTIAL: isize = 0;
    const DEFAULT_HANDLING: isize = 1;

    /// DER des Serverzertifikats (erstes Glied der Kette)
    fn leaf_der(trust: SecTrust) -> Option<Vec<u8>> {
        // SAFETY: Copy-Ergebnisse werden freigegeben; Get-Werte nicht.
        unsafe {
            let chain = SecTrustCopyCertificateChain(trust.0);
            if chain.is_null() {
                return None;
            }
            let der = (CFArrayGetCount(chain) > 0).then(|| {
                let data = SecCertificateCopyData(CFArrayGetValueAtIndex(chain, 0));
                let bytes = std::slice::from_raw_parts(
                    CFDataGetBytePtr(data),
                    usize::try_from(CFDataGetLength(data)).unwrap_or(0),
                )
                .to_vec();
                CFRelease(data);
                bytes
            });
            CFRelease(chain);
            der
        }
    }

    extern "C-unwind" fn did_receive_challenge(
        _this: &AnyObject,
        _cmd: Sel,
        _webview: *mut AnyObject,
        challenge: *mut AnyObject,
        handler: &Block<dyn Fn(isize, *mut AnyObject)>,
    ) {
        // SAFETY: WebKit ruft mit gültiger Abfrage auf; serverTrust ist nil,
        // wenn es keine Serverzertifikatsprüfung ist.
        unsafe {
            let space: *mut AnyObject = msg_send![challenge, protectionSpace];
            let trust: SecTrust = msg_send![space, serverTrust];
            let confirmed = !trust.0.is_null()
                && leaf_der(trust).is_some_and(|der| {
                    sf_tls::is_confirmed_fingerprint(&sf_tls::fingerprint(&der))
                });
            if confirmed {
                let credential: *mut AnyObject =
                    msg_send![class!(NSURLCredential), credentialForTrust: trust];
                handler.call((USE_CREDENTIAL, credential));
            } else {
                handler.call((DEFAULT_HANDLING, null_mut()));
            }
        }
    }

    pub fn install(webview: *mut AnyObject) {
        if webview.is_null() {
            return;
        }
        // SAFETY: webview ist der WKWebView des Fensters; die Methode wird
        // nur ergänzt, wenn die Klasse sie noch nicht hat.
        unsafe {
            let delegate: *mut AnyObject = msg_send![webview, navigationDelegate];
            if delegate.is_null() {
                return;
            }
            let cls: *const AnyClass = objc2::ffi::object_getClass(delegate);
            let selector = sel!(webView:didReceiveAuthenticationChallenge:completionHandler:);
            let imp: unsafe extern "C-unwind" fn() = std::mem::transmute(
                did_receive_challenge
                    as extern "C-unwind" fn(
                        &AnyObject,
                        Sel,
                        *mut AnyObject,
                        *mut AnyObject,
                        &Block<dyn Fn(isize, *mut AnyObject)>,
                    ),
            );
            objc2::ffi::class_addMethod(cls.cast_mut(), selector, imp, c"v@:@@@?".as_ptr());
            let _: () = msg_send![webview, setNavigationDelegate: delegate];
        }
    }
}

/// WebView2: Zertifikatsfehler melden sich als ServerCertificateErrorDetected;
/// bestätigte Zertifikate werden für diese Sitzung zugelassen.
#[cfg(windows)]
fn allow_confirmed_certificates(webview: tauri::webview::PlatformWebview) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_ALWAYS_ALLOW,
        COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_DEFAULT, ICoreWebView2_14,
    };
    use webview2_com::{ServerCertificateErrorDetectedEventHandler, take_pwstr};
    use windows_core::{Interface, PWSTR};

    let result = (|| -> windows_core::Result<()> {
        // SAFETY: COM-Aufrufe auf dem Controller des Fensters
        unsafe {
            let core = webview
                .controller()
                .CoreWebView2()?
                .cast::<ICoreWebView2_14>()?;
            let handler =
                ServerCertificateErrorDetectedEventHandler::create(Box::new(|_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut pem = PWSTR::null();
                    args.ServerCertificate()?.ToPemEncoding(&mut pem)?;
                    let confirmed = sf_tls::fingerprint_pem(&take_pwstr(pem))
                        .is_some_and(|fp| sf_tls::is_confirmed_fingerprint(&fp));
                    args.SetAction(if confirmed {
                        COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_ALWAYS_ALLOW
                    } else {
                        COREWEBVIEW2_SERVER_CERTIFICATE_ERROR_ACTION_DEFAULT
                    })
                }));
            let mut token = 0i64;
            core.add_ServerCertificateErrorDetected(&handler, &mut token)
        }
    })();
    if let Err(e) = result {
        tracing::warn!(error = %e, "Zertifikatsfreigabe im Anmeldefenster nicht eingerichtet");
    }
}
