// Voicemail-Nachrichten der Anlage, gemeinsam für Reiter und Zähler.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Voicemail = {
  id: string;
  folder: "inbox" | "old" | "private";
  name: string;
  number: string;
  mailbox: string;
  start: number;
  duration_secs: number;
  group: boolean;
};

export const voicemail = $state({ list: [] as Voicemail[], error: "" });

export async function loadVoicemails() {
  try {
    voicemail.list = await invoke<Voicemail[]>("voicemails");
    voicemail.error = "";
  } catch (e) {
    voicemail.error = String(e);
  }
}

let started = false;

/** Einmal beim Start: bei jeder Änderung auf der Anlage neu laden. */
export function initVoicemail() {
  if (started) return;
  started = true;
  listen("voicemail-changed", () => loadVoicemails());
}

export const unheard = () => voicemail.list.filter((v) => v.folder === "inbox").length;
