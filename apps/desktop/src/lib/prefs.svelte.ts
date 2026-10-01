// Benutzereinstellungen (lokal gespeichert, siehe settings.rs).
import { invoke } from "@tauri-apps/api/core";

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
  theme: "system" | "dark" | "light";
  language: string;
  start_minimized: boolean;
  minimize_to_tray: boolean;
  always_on_top: boolean;
  hotkeys: Hotkeys;
  fkey_columns: number;
};

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
  return prefs.value;
}

export async function savePrefs(p: Prefs) {
  try {
    await invoke("save_prefs", { prefs: p });
  } finally {
    prefs.value = $state.snapshot(p) as Prefs;
    applyTheme(p.theme);
  }
}
