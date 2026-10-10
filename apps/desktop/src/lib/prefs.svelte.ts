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
  /** Headset-Tasten (Plugin src-tauri/src/plugins/headset) */
  headset: boolean;
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
  /** Akzentfarbe (#rrggbb); leer = Orange */
  accent: string;
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
function applyTheme(theme: Prefs["theme"] = prefs.value?.theme ?? "system") {
  const dark = theme === "dark" || (theme === "system" && (darkQuery?.matches ?? true));
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  applyAccent();
}
darkQuery?.addEventListener("change", () => applyTheme());

/** Standard-Akzentfarbe (Orange wie in der STARFACE-App) */
export const DEFAULT_ACCENT = "#f5a31a";

/** Setzt die Akzentfarbe; ohne eigene Farbe gelten die Werte aus dem Layout. */
export function applyAccent(accent = prefs.value?.accent ?? "") {
  const style = document.documentElement.style;
  if (!/^#[0-9a-f]{6}$/i.test(accent)) {
    style.removeProperty("--accent");
    style.removeProperty("--accent-soft");
    style.removeProperty("--on-accent");
    style.removeProperty("--accent-text");
    return;
  }
  // Wie im Layout: im hellen Erscheinungsbild etwas deckender
  const light = document.documentElement.dataset.theme === "light";
  style.setProperty("--accent", accent);
  style.setProperty("--accent-soft", accent + (light ? "40" : "33"));
  style.setProperty("--on-accent", readableOn(accent));
  style.setProperty("--accent-text", readableText(accent, light));
}

const rgb = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
const hex = (c: number[]) => "#" + c.map((v) => Math.round(v).toString(16).padStart(2, "0")).join("");

/** Relative Helligkeit nach WCAG */
function luminance(c: number[]) {
  const lin = (v: number) => ((v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
  return 0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2]);
}
const contrast = (a: number, b: number) => (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);

/** Akzentfarbe als Schrift: zu Weiss (dunkles Erscheinungsbild) bzw. Schwarz
 *  hin mischen, bis sie auf den Flächen (--panel-2) gut lesbar ist (4.5:1) */
function readableText(accent: string, light: boolean) {
  const bg = luminance(rgb(light ? "#e6e9ed" : "#373b41"));
  const target = light ? [0, 0, 0] : [255, 255, 255];
  const c = rgb(accent);
  for (let k = 0; k <= 1; k += 0.05) {
    const mixed = c.map((v, i) => v + (target[i] - v) * k);
    if (contrast(luminance(mixed), bg) >= 4.5) return hex(mixed);
  }
  return hex(target);
}

/** Dunkler oder heller Text, je nachdem welcher auf `accent` besser lesbar ist */
function readableOn(accent: string) {
  const l = luminance(rgb(accent));
  return contrast(l, luminance([17, 17, 17])) >= contrast(l, 1) ? "#111" : "#fff";
}

export const prefs = $state({ value: null as Prefs | null });

/** Vom System gesperrte Einstellungen (/etc/xdg/starclxrc, siehe policy.rs) */
export const locked = $state({ keys: [] as string[] });

export async function loadPrefs() {
  prefs.value = await invoke<Prefs>("get_prefs");
  locked.keys = await invoke<string[]>("locked_prefs").catch((): string[] => []);
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
