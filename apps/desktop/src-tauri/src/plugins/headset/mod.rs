//! Plugin Headset: Gesprächs- und Stummtaste von USB-Headsets (Jabra, Poly,
//! EPOS) nehmen an, legen auf und schalten stumm. Bei einem Anruf klingelt
//! das Headset, Gesprächs- und Stumm-LED folgen dem Softphone.

use std::sync::Mutex;

use serde::Serialize;
use sf_core::phone::{CallPhase, CallView};
use sf_headset::{Button, Headset, State};
use tauri::{AppHandle, Emitter, Manager};

use crate::bus::{self, Event};
use crate::plugins::call;
use crate::settings;

#[derive(Default)]
pub struct HeadsetState {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    headset: Option<Headset>,
    lines: Vec<Line>,
    muted: bool,
    /// Test aus den Einstellungen läuft; Anrufzustand wird danach gesetzt
    testing: bool,
}

/// Was das Headset von einem Anruf wissen muss
#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    id: String,
    /// Klingelt bei uns
    ringing: bool,
    held: bool,
}

impl From<&CallView> for Line {
    fn from(c: &CallView) -> Self {
        Self {
            id: c.id.clone(),
            ringing: c.incoming && c.phase == CallPhase::Ringing,
            held: c.phase == CallPhase::Held,
        }
    }
}

fn state_of(lines: &[Line], muted: bool) -> State {
    State {
        ringing: lines.iter().any(|l| l.ringing),
        off_hook: lines.iter().any(|l| !l.ringing),
        muted,
        held: !lines.is_empty() && lines.iter().all(|l| l.held),
    }
}

/// Was ein Tastendruck bei diesen Anrufen bewirkt
#[derive(Debug, PartialEq, Eq)]
enum Action {
    Answer(String),
    Hangup(String),
    Mute(bool),
    Nothing,
}

fn action_for(button: Button, lines: &[Line], muted: bool) -> Action {
    let off_hook = lines.iter().any(|l| !l.ringing);
    // Laufendes Gespräch (oder Wählen); gehaltene bleiben stehen
    let active = lines.iter().find(|l| !l.ringing && !l.held);
    let ringing = lines.iter().find(|l| l.ringing);
    match button {
        Button::HookOff | Button::HookToggle if !off_hook => match ringing {
            Some(l) => Action::Answer(l.id.clone()),
            None => Action::Nothing,
        },
        Button::HookOn | Button::HookToggle => match active {
            Some(l) => Action::Hangup(l.id.clone()),
            None => Action::Nothing,
        },
        Button::Mute if off_hook => Action::Mute(!muted),
        _ => Action::Nothing,
    }
}

pub fn start(app: &AppHandle) {
    bus::listen(app, "headset", |app, event| {
        match event {
            Event::PhoneStopped => {
                let state = app.state::<HeadsetState>();
                let mut inner = state.inner.lock().unwrap();
                inner.lines.clear();
                inner.muted = false;
            }
            Event::Calls { calls, muted } => {
                let state = app.state::<HeadsetState>();
                let mut inner = state.inner.lock().unwrap();
                inner.lines = calls.iter().map(Line::from).collect();
                inner.muted = muted;
            }
            Event::PhoneReady | Event::PrefsSaved => {}
        }
        refresh(app);
    });
    refresh(app);
}

/// Headset ein- bzw. ausschalten und den Anrufzustand übertragen
fn refresh(app: &AppHandle) {
    let state = app.state::<HeadsetState>();
    let mut inner = state.inner.lock().unwrap();
    if !settings::load(app).prefs.headset {
        inner.headset.take(); // schaltet Klingeln und LEDs aus
        return;
    }
    if inner.testing {
        return;
    }
    let s = state_of(&inner.lines, inner.muted);
    inner.headset.get_or_insert_with(|| open(app)).set(s);
}

fn open(app: &AppHandle) -> Headset {
    let app = app.clone();
    Headset::start(move |button| {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { pressed(&app, button).await });
    })
}

async fn pressed(app: &AppHandle, button: Button) {
    let action = {
        let state = app.state::<HeadsetState>();
        let inner = state.inner.lock().unwrap();
        action_for(button, &inner.lines, inner.muted)
    };
    tracing::debug!(?button, ?action, "Headset-Taste");
    let result = match action {
        Action::Answer(id) => call::answer(app, &id).await,
        Action::Hangup(id) => call::hangup(app, &id).await,
        Action::Mute(m) => call::set_mute(app, m).await,
        Action::Nothing => Ok(()),
    };
    if let Err(e) = result {
        let _ = app.emit("phone-error", e);
    }
    // Ohne Wirkung meint das Headset sonst, es sei im Gespräch
    refresh(app);
}

#[derive(Serialize)]
pub struct HeadsetInfo {
    devices: Vec<String>,
    error: Option<String>,
}

#[tauri::command]
pub async fn headset_info() -> HeadsetInfo {
    let status = tauri::async_runtime::spawn_blocking(sf_headset::probe)
        .await
        .unwrap_or_default();
    HeadsetInfo {
        devices: status.devices,
        error: status.error,
    }
}

/// Lässt das Headset drei Sekunden klingeln.
#[tauri::command]
pub async fn headset_test(app: AppHandle) {
    {
        let state = app.state::<HeadsetState>();
        let mut inner = state.inner.lock().unwrap();
        inner.testing = true;
        inner.headset.get_or_insert_with(|| open(&app)).set(State {
            ringing: true,
            ..State::default()
        });
    }
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    app.state::<HeadsetState>().inner.lock().unwrap().testing = false;
    refresh(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str, ringing: bool, held: bool) -> Line {
        Line {
            id: id.into(),
            ringing,
            held,
        }
    }

    #[test]
    fn answer_ringing_call() {
        let lines = [line("a", true, false)];
        assert_eq!(
            action_for(Button::HookOff, &lines, false),
            Action::Answer("a".into())
        );
        assert_eq!(
            action_for(Button::HookToggle, &lines, false),
            Action::Answer("a".into())
        );
        // Stumm ohne Gespräch und Auflegen beim Klingeln tun nichts
        assert_eq!(action_for(Button::Mute, &lines, false), Action::Nothing);
        assert_eq!(action_for(Button::HookOn, &lines, false), Action::Nothing);
        let s = state_of(&lines, false);
        assert!(s.ringing && !s.off_hook);
    }

    #[test]
    fn hang_up_active_not_held_or_waiting() {
        let lines = [
            line("held", false, true),
            line("talk", false, false),
            line("wait", true, false),
        ];
        assert_eq!(
            action_for(Button::HookOn, &lines, false),
            Action::Hangup("talk".into())
        );
        assert_eq!(
            action_for(Button::HookToggle, &lines, false),
            Action::Hangup("talk".into())
        );
        // Abheben im Gespräch nimmt den Anklopfer nicht an
        assert_eq!(action_for(Button::HookOff, &lines, false), Action::Nothing);
        assert_eq!(action_for(Button::Mute, &lines, true), Action::Mute(false));
        let only_held = [line("held", false, true)];
        assert_eq!(
            action_for(Button::HookOn, &only_held, false),
            Action::Nothing
        );
        assert!(state_of(&only_held, false).held);
    }

    #[test]
    fn idle() {
        assert_eq!(action_for(Button::HookOff, &[], false), Action::Nothing);
        assert_eq!(
            state_of(&[], true),
            State {
                muted: true,
                ..State::default()
            }
        );
    }
}
