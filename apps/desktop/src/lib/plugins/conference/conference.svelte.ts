// Geplante Konferenzen der Anlage, gemeinsam für Reiter und Formular.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Recurrence = "once" | "daily" | "weekly" | "monthly";

export type Participant = {
  id: string;
  user_id: string | null;
  name: string;
  number: string;
  email: string;
  moderator: boolean;
  call_on_start: boolean;
};

export type Conference = {
  id: string;
  name: string;
  moderator: boolean;
  state: "planned" | "active" | "concluded";
  start: number;
  recurrence: Recurrence;
  participants: Participant[];
};

export const conferences = $state({ list: [] as Conference[], error: "" });

/** Offenes Formular: `null` zu, `""` neue Konferenz, sonst deren ID */
export const conferenceEdit = $state({ id: null as string | null });

export async function loadConferences() {
  try {
    conferences.list = await invoke<Conference[]>("conferences");
    conferences.error = "";
  } catch (e) {
    conferences.error = String(e);
  }
}

let started = false;

/** Einmal beim Start: bei jeder Änderung auf der Anlage neu laden. */
export function initConferences() {
  if (started) return;
  started = true;
  listen("conferences-changed", () => loadConferences());
}

/** Laufende Konferenzen, für den Zähler am Reiter */
export const activeConferences = () => conferences.list.filter((c) => c.state === "active").length;
