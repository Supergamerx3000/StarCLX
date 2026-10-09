// Benutzereinstellungen (lokal gespeichert, siehe settings.rs).
import { invoke } from "@tauri-apps/api/core";
import { setLanguage } from "./i18n.svelte";

export type Prefs = {
  softphone: boolean;
  primary_on_login: boolean;
  primary_on_answer: boolean;
  notify_missed: boolean;
  notify_missed_group: boolean;
  speakers: string[];
  microphones: string[];
  ring_devices: string[];
  ringtone: boolean;
  ringtone_internal: string;
  ringtone_external: string;
  custom_ringtones: string[];
  bring_to_front: boolean;
  busylight: boolean;
  busylight_sound: string;
  busylight_volume: number;
  chat_notify: boolean;
  chat_sound: boolean;
  download_dir: string;
  away_on_idle: boolean;
  away_on_screensaver: boolean;
  away_on_lock: boolean;
  away_text: string;
  offline_text: string;
  /** Selbst gewählter Chat-Status: "available", "away" oder "dnd" */
  chat_availability: string;
  chat_text: string;
  /** Gespeicherte eigene Status */
  chat_presets: { availability: string; text: string }[];
  theme: "system" | "dark" | "light";
  language: string;
  start_minimized: boolean;
  autostart: boolean;
  handle_tel_links: boolean;
  call_actions: CallAction[];
  /** Angelegte Türkameras (Plugin src-tauri/src/plugins/doorcam) */
  door_cams: DoorCam[];
  /** Landesvorwahl ohne "+", z. B. "41" */
  default_country_code: string;
  minimize_to_tray: boolean;
  always_on_top: boolean;
  hotkeys: Hotkeys;
  fkey_columns: number;
  workspace: "tabs" | "free";
  workspace_tiles: Tile[] | null;
  /** Ausführliches Protokoll (Anruf- und Verbindungsdetails) */
  verbose_log: boolean;
};

/** Kachel im freien Arbeitsbereich: Lage in Rasterzellen (12 Spalten) */
export type Tile = { id: string; x: number; y: number; w: number; h: number; visible: boolean };

/** URL oder Programm bei Anruf (Plugin src-tauri/src/plugins/callactions) */
export type CallAction = {
  enabled: boolean;
  trigger: "ringing" | "answered" | "outgoing";
  /** Platzhalter auf die Nummer, z. B. "+41*"; leer heisst alle */
  filter: string;
  external_only: boolean;
  /** URL (mit "://") oder Befehlszeile */
  target: string;
};

/** Türkamera: Name und URL (rtsp://, MJPEG oder Einzelbild, Zugangsdaten in der URL) */
export type DoorCam = { name: string; url: string };

export type Hotkeys = {
  enabled: boolean;
  dial_selection: string;
  dial_clipboard: string;
  answer: string;
  hangup: string;
  toggle_view: string;
};

const darkQuery = typeof window !== "undefined" ? window.matchMedia("(prefers-color-scheme: dark)") : null;

/** Setzt das Erscheinungsbild; "system" folgt der Einstellung des Desktops. */
export function applyTheme(theme: Prefs["theme"] = prefs.value?.theme ?? "system") {
  const dark = theme === "dark" || (theme === "system" && (darkQuery?.matches ?? true));
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}
darkQuery?.addEventListener("change", () => applyTheme());

export const prefs = $state({ value: null as Prefs | null });

export async function loadPrefs() {
  prefs.value = await invoke<Prefs>("get_prefs");
  applyTheme(prefs.value.theme);
  setLanguage(prefs.value.language);
  return prefs.value;
}

export async function savePrefs(p: Prefs) {
  try {
    await invoke("save_prefs", { prefs: p });
  } finally {
    prefs.value = $state.snapshot(p) as Prefs;
    applyTheme(p.theme);
    setLanguage(p.language);
  }
}
