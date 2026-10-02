// Funktionstasten: Daten von der Anlage, Zustände und Auslösen.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { action, phone, run, type Call } from "./phone.svelte";
import { t } from "./i18n.svelte";

export type FunctionKey = {
  functionKeyType: string;
  id: string;
  accountId: string;
  valid: boolean;
  name: string;
  position: number;
  blfAccountId: number | null;
  directCallTargetnumber: string | null;
  redirectNumberIds: number[];
  forwardTarget: string | null;
  forwardTargetType: string | null;
  forwardType: string | null;
  groupIds: number[];
  poNumber: string | null;
  displayNumberId: number | null;
  activateModuleIds: string[];
  addressbookRequest: string | null;
  addressBookFolderName: string | null;
  callListRequest: string | null;
  dtmf: string | null;
  genericURL: string | null;
};
export type Account = { account_id: number; user_ids: string[]; name: string; number: string };
type Keys = { set_id: string; set_name: string; account_id: string; keys: FunctionKey[]; order: string[]; accounts: Account[]; me: string };
type UserState = { telephony: string; dnd: boolean };
export type Redirect = {
  id: string; kind: string; called_number: string; called_number_id: string; group: boolean; enabled: boolean;
  target: { number: string | null; mailbox: string | null }; mailboxes: { id: string; name: string }[];
};
export type SignalingNumber = { id: string; number: string; suppressed: boolean; read_only: boolean; selected: boolean };

/** Gruppe in der Typenliste; "desk" = nur auf Tischtelefonen */
type Group = "fav" | "fn" | "desk";
/** Tastentypen; als Funktion, damit die Bezeichnungen der Sprache folgen */
export const types = (): { type: string; label: string; group: Group; usable: boolean }[] => [
  { type: "BUSYLAMPFIELD", label: t("Besetztlampenfeld"), group: "fav", usable: true },
  { type: "QUICKDIAL", label: t("Direktwahl"), group: "fav", usable: true },
  { type: "GROUPLOGIN", label: t("Gruppe An-/Abmelden"), group: "fn", usable: false },
  { type: "DONOTDISTURB", label: t("Ruhe"), group: "fn", usable: true },
  { type: "COMPLETIONOFCALLSTOBUSYSUBSCRIBER", label: t("Rückruf bei Besetzt"), group: "fn", usable: false },
  { type: "SIGNALNUMBER", label: t("Rufnummer anzeigen"), group: "fn", usable: true },
  { type: "FORWARD", label: t("Umleitung (Art)"), group: "fn", usable: true },
  { type: "FORWARDNUMBER", label: t("Umleitung (Rufnummer)"), group: "fn", usable: true },
  { type: "FORWARDTOTARGET", label: t("Umleitung auf Ziel"), group: "fn", usable: true },
  { type: "PARKANDORBIT", label: t("Parken"), group: "fn", usable: true },
  { type: "MODULEACTIVATION", label: t("Modul aktivieren"), group: "fn", usable: false },
  { type: "ADDRESSBOOK", label: t("Telefonmenü Adressbuch"), group: "desk", usable: false },
  { type: "PHONECALLLIST", label: t("Telefonmenü Rufliste"), group: "desk", usable: false },
  { type: "PHONEGENERICURL", label: t("Telefonbasierte URL"), group: "desk", usable: false },
  { type: "PHONEDTMF", label: "DTMF", group: "desk", usable: true },
  { type: "SEPARATOR", label: t("Leere Taste"), group: "desk", usable: false },
];
export const typeInfo = (type: string) => types().find((x) => x.type === type) ?? { type, label: type, group: "fn" as Group, usable: false };

export const fkeys = $state({
  setId: "",
  setName: "",
  accountId: "",
  keys: [] as FunctionKey[],
  /** Platzbelegung: Tasten-ID je Platz, "" = leerer Platz */
  order: [] as string[],
  /** Parkplätze, auf denen dieser Client ein Gespräch geparkt hat */
  parked: {} as Record<string, boolean>,
  accounts: [] as Account[],
  presence: {} as Record<string, UserState>,
  redirects: [] as Redirect[],
  me: "",
  error: "",
  notice: "",
  loaded: false,
});

let started = false;

export async function loadFkeys() {
  if (!started) {
    started = true;
    listen<Record<string, UserState>>("fkey-presence", (e) => (fkeys.presence = e.payload));
    listen("reach-changed", () => loadRedirects());
  }
  try {
    const k = await invoke<Keys>("fkeys_load");
    fkeys.setId = k.set_id;
    fkeys.setName = k.set_name;
    fkeys.accountId = k.account_id;
    fkeys.keys = k.keys;
    fkeys.order = k.order;
    fkeys.accounts = k.accounts;
    fkeys.me = k.me;
    fkeys.error = "";
    fkeys.loaded = true;
    fkeys.presence = await invoke<Record<string, UserState>>("fkey_presence");
  } catch (e) {
    fkeys.error = String(e);
  }
  loadRedirects();
}

async function loadRedirects() {
  try {
    fkeys.redirects = await invoke<Redirect[]>("redirects");
  } catch {
    // Umleitungen sind für die Anzeige nicht zwingend
  }
}

export const account = (k: FunctionKey) => fkeys.accounts.find((a) => a.account_id === k.blfAccountId);

/** Taste auf einem Platz, oder undefined für einen leeren Platz */
export const keyAt = (slot: number) => {
  const id = fkeys.order[slot];
  return id ? fkeys.keys.find((k) => k.id === id) : undefined;
};

/** Legt `id` auf `slot`. Ist der Platz belegt, wird die Taste davor
 *  eingeschoben: eine verschobene Taste lässt dabei keine Lücke zurück, eine
 *  neue schiebt die folgenden Tasten bis zur nächsten Lücke weiter. */
export function placeAt(order: string[], id: string, slot: number): string[] {
  const o = [...order];
  const from = o.indexOf(id);
  while (o.length <= slot) o.push("");
  if (!o[slot] || o[slot] === id) {
    if (from >= 0) o[from] = "";
    o[slot] = id;
  } else if (from >= 0) {
    o.splice(from, 1);
    o.splice(slot, 0, id);
  } else {
    o.splice(slot, 0, id);
    const gap = o.indexOf("", slot + 1);
    if (gap >= 0) o.splice(gap, 1);
  }
  while (o.length && !o[o.length - 1]) o.pop();
  return o;
}

/** Neue Platzbelegung an die Anlage schicken */
export async function saveOrder(order: string[]) {
  fkeys.order = order;
  await invoke("fkeys_reorder", { set: fkeys.setId, name: fkeys.setName, order, keys: $state.snapshot(fkeys.keys) });
}

const stateOf = (a: Account | undefined) => {
  for (const id of a?.user_ids ?? []) if (fkeys.presence[id]) return fkeys.presence[id];
};

const ownDnd = () => fkeys.presence[fkeys.me]?.dnd ?? false;

/** Umleitungen, die eine Taste schaltet */
export function redirectsOf(k: FunctionKey): Redirect[] {
  const own = fkeys.redirects.filter((r) => !r.group);
  switch (k.functionKeyType) {
    case "FORWARD":
      return own.filter((r) => r.kind === (k.forwardType ?? "ALWAYS").toLowerCase());
    case "FORWARDNUMBER":
    case "FORWARDTOTARGET":
      return fkeys.redirects.filter((r) => r.kind === "always" && k.redirectNumberIds.map(String).includes(r.called_number_id));
    default:
      return [];
  }
}

/** Zustand für die Farbe: "on", "busy", "ringing", "free", "off" oder "" */
export function keyState(k: FunctionKey): string {
  switch (k.functionKeyType) {
    case "BUSYLAMPFIELD": {
      const s = stateOf(account(k));
      if (!s) return "";
      return s.telephony === "ringing" ? "ringing" : s.telephony === "active" ? "busy" : s.telephony === "unavailable" ? "off" : "free";
    }
    case "DONOTDISTURB":
      return ownDnd() ? "on" : "";
    case "FORWARD":
    case "FORWARDNUMBER":
    case "FORWARDTOTARGET":
      return redirectsOf(k).some((r) => r.enabled) ? "on" : "";
    case "PARKANDORBIT":
      return fkeys.parked[k.poNumber ?? ""] ? "parked" : "";
    default:
      return "";
  }
}

export function keyTitle(k: FunctionKey) {
  return k.name || typeInfo(k.functionKeyType).label;
}

const activeCall = () => phone.status.calls.find((c) => c.phase === "connected");

async function call(cmd: string, args: Record<string, unknown>) {
  fkeys.notice = "";
  try {
    await invoke(cmd, args);
    return true;
  } catch (e) {
    fkeys.notice = String(e);
    return false;
  }
}

/** Taste gedrückt */
export async function press(k: FunctionKey) {
  fkeys.notice = "";
  switch (k.functionKeyType) {
    case "BUSYLAMPFIELD": {
      // Klingelt es beim Kollegen, Anruf heranholen, sonst ihn anrufen
      const a = account(k);
      if (!a) return void (fkeys.notice = t("Für diese Taste ist kein Benutzer hinterlegt."));
      const s = stateOf(a);
      if (s?.telephony === "ringing" && a.user_ids.length) {
        await call("fkey_grab", { userId: a.user_ids[0] });
        return;
      }
      if (a.number) run("phone_dial", { number: a.number });
      else fkeys.notice = t("{name} hat keine Rufnummer.", { name: a.name });
      return;
    }
    case "QUICKDIAL":
      if (k.directCallTargetnumber) run("phone_dial", { number: k.directCallTargetnumber });
      return;
    case "DONOTDISTURB":
      return call("fkey_dnd", { enabled: !ownDnd() });
    case "SIGNALNUMBER": {
      const list = await invoke<SignalingNumber[]>("signaling_numbers").catch(() => []);
      const n = k.displayNumberId === 0 ? list.find((x) => x.suppressed) : list.find((x) => x.id === String(k.displayNumberId));
      if (!n) return void (fkeys.notice = t("Diese Rufnummer ist nicht mehr wählbar."));
      return call("set_signaling_number", { id: n.id });
    }
    case "FORWARD":
    case "FORWARDNUMBER":
    case "FORWARDTOTARGET": {
      const list = redirectsOf(k);
      if (!list.length) return void (fkeys.notice = t("Keine passende Umleitung gefunden."));
      const enable = !list.some((r) => r.enabled);
      for (const r of list) {
        if (enable && k.functionKeyType === "FORWARDTOTARGET") {
          const target = k.forwardTargetType === "VOICEMAIL"
            ? { number: null, mailbox: r.mailboxes[0]?.id ?? null }
            : { number: k.forwardTarget, mailbox: null };
          await call("redirect_update", { id: r.id, target, timeoutSecs: null });
        }
        await call("redirect_enable", { id: r.id, enabled: enable });
      }
      return loadRedirects();
    }
    case "PARKANDORBIT": {
      // Mit Gespräch parken, ohne Gespräch das geparkte zurückholen
      const number = k.poNumber ?? "";
      const c = activeCall();
      if (c) {
        if (await call("fkey_park", { callId: c.id, number })) fkeys.parked[number] = true;
      } else if (await call("fkey_park", { callId: null, number })) {
        fkeys.parked[number] = false;
      } else if (!fkeys.parked[number]) {
        fkeys.notice = t("Auf Platz {n} ist kein Gespräch geparkt.", { n: number });
      }
      return;
    }
    case "PHONEDTMF": {
      const c = activeCall();
      if (!c) return void (fkeys.notice = t("Tastentöne gehen nur während eines Gesprächs."));
      return call("phone_dtmf", { callId: c.id, digits: k.dtmf ?? "" });
    }
    default:
      fkeys.notice = t("„{label}“ lässt sich im Linux-Client noch nicht auslösen.", { label: typeInfo(k.functionKeyType).label });
  }
}

/** Neue, leere Taste eines Typs mit den Vorgaben wie in Windows */
export function blank(type: string): FunctionKey {
  return {
    functionKeyType: type, id: "", accountId: fkeys.accountId, valid: true, name: "", position: fkeys.keys.length,
    blfAccountId: null, directCallTargetnumber: null, redirectNumberIds: [], forwardTarget: null, forwardTargetType: null,
    forwardType: type === "FORWARD" ? "ALWAYS" : null, groupIds: [], poNumber: type === "PARKANDORBIT" ? "00" : null,
    displayNumberId: null, activateModuleIds: [], addressbookRequest: type === "ADDRESSBOOK" ? "CONTACTLIST" : null,
    addressBookFolderName: null, callListRequest: type === "PHONECALLLIST" ? "INCOMING" : null, dtmf: null, genericURL: null,
  };
}

// ── Gespräch auf ein Besetztlampenfeld ziehen ──────────────────────────────
// Ablegen startet eine Rückfrage zum Kollegen; Auflegen dieser Rückfrage
// vermittelt dann das erste Gespräch an ihn.

/** Laufendes Ziehen eines Gesprächs (für Vorschau und Hervorhebung) */
export const callDrag = $state({ call: null as Call | null, x: 0, y: 0, over: "" });
/** Erste Gespräche, deren Rückfrage per Besetztlampenfeld gestartet wurde */
export const blfTransfers = $state<Record<string, boolean>>({});

let pending: { call: Call; x: number; y: number } | null = null;

/** BLF-Taste unter dem Zeiger (Elemente mit data-fkey) */
function blfAt(x: number, y: number) {
  const id = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-fkey]")?.dataset.fkey;
  const k = fkeys.keys.find((k) => k.id === id);
  return k?.functionKeyType === "BUSYLAMPFIELD" ? k : undefined;
}

function onMove(e: PointerEvent) {
  if (!pending) return;
  if (!callDrag.call) {
    if (Math.hypot(e.clientX - pending.x, e.clientY - pending.y) < 8) return;
    callDrag.call = pending.call;
  }
  callDrag.x = e.clientX;
  callDrag.y = e.clientY;
  callDrag.over = blfAt(e.clientX, e.clientY)?.id ?? "";
}

function onUp(e: PointerEvent) {
  const call = callDrag.call;
  stopCallDrag();
  if (!call) return;
  const k = blfAt(e.clientX, e.clientY);
  if (k) transferTo(call, k);
}

function stopCallDrag() {
  pending = null;
  callDrag.call = null;
  callDrag.over = "";
  window.removeEventListener("pointermove", onMove);
  window.removeEventListener("pointerup", onUp);
  window.removeEventListener("pointercancel", stopCallDrag);
}

/** Auf der Anrufkarte gedrückt: Ziehen vorbereiten */
export function startCallDrag(e: PointerEvent, call: Call) {
  if (e.button !== 0 || (call.phase !== "connected" && call.phase !== "held") || call.consultation_of) return;
  if ((e.target as HTMLElement).closest("button, input")) return;
  e.preventDefault(); // keine Textauswahl beim Ziehen
  pending = { call, x: e.clientX, y: e.clientY };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  window.addEventListener("pointercancel", stopCallDrag);
}

/** Rückfrage zum Kollegen der Taste; Auflegen vermittelt danach */
export async function transferTo(call: Call, k: FunctionKey) {
  const a = account(k);
  if (!a?.number) return void (phone.notice = t("Für diese Taste ist keine Rufnummer bekannt."));
  if (await action("consult", call.id, a.number)) blfTransfers[call.id] = true;
}

/** Soll Auflegen dieses Gesprächs vermitteln? */
export const transfersOnHangup = (c: Call) =>
  !!c.consultation_of && !!blfTransfers[c.consultation_of] && phone.status.calls.some((x) => x.id === c.consultation_of);

/** Rückfrage abbrechen und zum ersten Gespräch zurück */
export async function backToFirst(c: Call) {
  const first = c.consultation_of;
  if (first) delete blfTransfers[first];
  await run("phone_hangup", { callId: c.id });
  if (first && phone.status.calls.some((x) => x.id === first && x.phase === "held")) {
    await run("phone_hold", { callId: first, hold: false });
  }
}
