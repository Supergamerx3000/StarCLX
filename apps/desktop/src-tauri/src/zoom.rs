//! Sperrt das Zoomen der Oberfläche.
//!
//! WebKitGTK vergrößert die Seite bei Zwei-Finger-Geste auf dem Touchpad und
//! bei Strg+Mausrad. Für eine Desktop-App ist das nur störend: die Fläche
//! lässt sich danach kaum zurücksetzen. Die Ereignisse werden abgefangen,
//! bevor WebKit sie sieht, in jedem Fenster der App.

use tauri::Runtime;
use tauri::plugin::{Builder, TauriPlugin};

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("nozoom")
        .on_webview_ready(|webview| {
            #[cfg(target_os = "linux")]
            if let Err(e) = webview.with_webview(block_zoom) {
                tracing::warn!(error = %e, "Zoomsperre nicht gesetzt");
            }
            #[cfg(not(target_os = "linux"))]
            let _ = webview;
        })
        .build()
}

#[cfg(target_os = "linux")]
fn block_zoom(webview: tauri::webview::PlatformWebview) {
    use gtk::gdk::{EventType, ModifierType};
    use gtk::glib::Propagation;
    use gtk::prelude::WidgetExt;
    use webkit2gtk::WebViewExt;

    let view = webview.inner();
    view.connect_event(|_, event| {
        let zoom = match event.event_type() {
            EventType::TouchpadPinch => true,
            EventType::Scroll => event
                .state()
                .is_some_and(|s| s.contains(ModifierType::CONTROL_MASK)),
            _ => false,
        };
        if zoom {
            Propagation::Stop
        } else {
            Propagation::Proceed
        }
    });
    // Falls doch einmal gezoomt wurde (z. B. über ein Tastenkürzel)
    view.connect_zoom_level_notify(|view| {
        if view.zoom_level() != 1.0 {
            view.set_zoom_level(1.0);
        }
    });
}
