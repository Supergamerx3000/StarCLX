// Erreichbarkeit der Anlage. Ist sie weg (z. B. VPN getrennt), zeigt das
// Hauptfenster einen einzigen Hinweis; die Fehler der Module bleiben
// ausgeblendet und stehen nur im Protokoll. Sobald die Anlage wieder antwortet,
// lädt alles neu (Ereignis "resumed").
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { phone } from "./plugins/call/phone.svelte";
import { fkeys } from "./plugins/fkeys/fkeys.svelte";

export const connection = $state({ online: true });

let started = false;

function set(online: boolean) {
  if (connection.online === online) return;
  connection.online = online;
  // Hinweise aus der Zeit des Abbruchs sind danach überholt
  phone.notice = "";
  fkeys.notice = "";
}

/** Einmal je Fenster: Ereignis abonnieren und den aktuellen Stand holen. */
export function initConnection() {
  if (started) return;
  started = true;
  listen<boolean>("connection", (e) => set(e.payload));
  invoke<boolean>("connection_online").then(set).catch(() => {});
}
