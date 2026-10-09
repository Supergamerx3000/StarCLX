// Gemeinsamer Stand des Telefons für alle Komponenten.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { t } from "../../i18n.svelte";

export type Phase = "setup" | "ringing" | "ringback" | "connected" | "held" | "other";
export type Call = {
  id: string;
  phase: Phase;
  incoming: boolean;
  internal: boolean;
  remote_name: string;
  remote_number: string;
  local_name: string;
  local_number: string;
  consultation_of: string | null;
  recording: boolean;
  connected_since: number | null;
  /** Anruf einer Türsprechstelle mit Kamera */
  door_cam: boolean;
  /** Tür lässt sich per DTMF öffnen */
  door_open: boolean;
};
export type PhoneStatus = {
  state: "off" | "starting" | "ready" | "error";
  detail: string;
  calls: Call[];
  muted: boolean;
  /** Rückruf bei Besetzt: "available", "active" oder "" */
  callback: string;
  /** Anrufsteuerung über die Anlage verfügbar, auch ohne Softphone */
  control: boolean;
  /** ID des Softphones an der Anlage, leer ohne Softphone */
  softphone_id: string;
};

export const phone = $state({
  status: { state: "off", detail: "", calls: [], muted: false, callback: "", control: false, softphone_id: "" } as PhoneStatus,
  notice: "",
  now: Date.now(),
});

let started = false;

/** Einmal beim Start: Ereignisse abonnieren und den aktuellen Stand holen. */
export function initPhone() {
  if (started) return;
  started = true;
  listen<PhoneStatus>("phone", (e) => (phone.status = e.payload));
  listen<string>("phone-error", (e) => (phone.notice = e.payload));
  invoke<PhoneStatus>("phone_status").then((s) => (phone.status = s));
  setInterval(() => (phone.now = Date.now()), 1000);
}

/** Führt einen Befehl aus; Fehler landen als Hinweis im Call Manager. */
export async function run(cmd: string, args: Record<string, unknown> = {}) {
  phone.notice = "";
  try {
    await invoke(cmd, args);
    return true;
  } catch (e) {
    phone.notice = String(e);
    return false;
  }
}

export const action = (action: string, callId: string, number?: string) =>
  run("phone_action", { action, callId, number });

export function who(c: Call) {
  return c.remote_name || c.remote_number || t("Unbekannt");
}

export function duration(c: Call, now: number) {
  if (!c.connected_since) return "";
  const s = Math.max(0, Math.floor((now - c.connected_since) / 1000));
  const h = Math.floor(s / 3600);
  const mm = String(Math.floor((s % 3600) / 60)).padStart(2, "0");
  const ss = String(s % 60).padStart(2, "0");
  return h ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

/** Wählen geht, sobald die Anrufsteuerung läuft; ohne Softphone über das
 *  Telefon aus „Wählen über“ bzw. das primäre Telefon. */
export const canDial = () => phone.status.control;
/** Ton, Annehmen und Stummschalten gibt es nur am Softphone. */
export const hasSoftphone = () => phone.status.state === "ready";

export const isRingingIn = (c: Call) => c.incoming && c.phase === "ringing";
