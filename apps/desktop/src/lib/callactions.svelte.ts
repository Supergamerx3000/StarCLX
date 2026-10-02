// URL oder Programm bei Anruf: erkennt Klingeln, Annahme und abgehende
// Anrufe; Filter und Ausführung erledigt call_actions_fire in Rust.
import { invoke } from "@tauri-apps/api/core";
import { phone, type Call } from "./phone.svelte";
import { prefs, type CallAction } from "./prefs.svelte";

/** Bereits ausgelöste Ereignisse je Anruf-ID */
const fired = new Map<string, Set<CallAction["trigger"]>>();
let started = false;

function triggerOf(c: Call): CallAction["trigger"] | null {
  if (c.incoming) return c.phase === "ringing" ? "ringing" : c.phase === "connected" ? "answered" : null;
  return ["setup", "ringback", "connected"].includes(c.phase) ? "outgoing" : null;
}

function check(calls: Call[]) {
  for (const id of fired.keys()) if (!calls.some((c) => c.id === id)) fired.delete(id);
  const rules = prefs.value?.call_actions ?? [];
  for (const c of calls) {
    const trigger = triggerOf(c);
    if (!trigger) continue;
    const done = fired.get(c.id) ?? new Set();
    fired.set(c.id, done);
    if (done.has(trigger)) continue;
    done.add(trigger);
    if (!rules.some((r) => r.enabled && r.trigger === trigger && r.target.trim())) continue;
    invoke("call_actions_fire", { trigger, number: c.remote_number, internal: c.internal }).catch(
      (e) => (phone.notice = String(e)),
    );
  }
}

/** Einmal beim Start aus +page.svelte */
export function initCallActions() {
  if (started) return;
  started = true;
  $effect.root(() => {
    $effect(() => check(phone.status.calls));
  });
}
