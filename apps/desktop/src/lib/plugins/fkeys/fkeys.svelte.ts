// Funktionstasten: Daten von der Anlage, Zustände und Auslösen.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { action, phone, run, type Call } from "../call/phone.svelte";
import { t } from "../../i18n.svelte";
import { prefs } from "../../prefs.svelte";
import { searchable } from "../../numbers";

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
/** Konto im Besetztlampenfeld; bei einer Gruppe enthält user_ids die Gruppen-ID */
export type Account = { account_id: number; user_ids: string[]; name: string; number: string; group: boolean };
/** Gruppe, die „Gruppe An-/Abmelden“ schalten kann; id wie in groupIds */
export type GroupChoice = { id: number; name: string };
type Keys = { set_id: string; set_name: string; account_id: string; keys: FunctionKey[]; order: string[]; accounts: Account[]; group_choices: GroupChoice[]; me: string };
/** Präsenz eines Users; chat: "available", "away", "dnd", "offline" oder "" (kein Chat) */
export type UserState = { telephony: string; dnd: boolean; chat: string; chat_message: string; redirect: boolean };
export type Redirect = {
  id: string; kind: string; called_number: string; called_number_id: string; group: boolean; enabled: boolean;
  target: { number: string | null; mailbox: string | null }; mailboxes: { id: string; name: string }[];
  timeout_secs: number; last_number: string;
};
export type Module = { id: string; name: string; active: boolean; read_only: boolean };
export type SignalingNumber = { id: string; number: string; suppressed: boolean; read_only: boolean; selected: boolean; group: string };

/** Gruppe in der Typenliste; "desk" = nur auf Tischtelefonen */
type Group = "fav" | "fn" | "desk";
/** Tastentypen; als Funktion, damit die Bezeichnungen der Sprache folgen */
export const types = (): { type: string; label: string; group: Group; usable: boolean }[] => [
  { type: "BUSYLAMPFIELD", label: t("Besetztlampenfeld"), group: "fav", usable: true },
  { type: "QUICKDIAL", label: t("Direktwahl"), group: "fav", usable: true },
  { type: "GROUPLOGIN", label: t("Gruppe An-/Abmelden"), group: "fn", usable: true },
  { type: "DONOTDISTURB", label: t("Ruhe"), group: "fn", usable: true },
  { type: "COMPLETIONOFCALLSTOBUSYSUBSCRIBER", label: t("Rückruf bei Besetzt"), group: "fn", usable: true },
  { type: "SIGNALNUMBER", label: t("Rufnummer anzeigen"), group: "fn", usable: true },
  { type: "FORWARD", label: t("Umleitung (Art)"), group: "fn", usable: true },
  { type: "FORWARDNUMBER", label: t("Umleitung (Rufnummer)"), group: "fn", usable: true },
  { type: "FORWARDTOTARGET", label: t("Umleitung auf Ziel"), group: "fn", usable: true },
  { type: "PARKANDORBIT", label: t("Parken"), group: "fn", usable: true },
  { type: "MODULEACTIVATION", label: t("Modul aktivieren"), group: "fn", usable: true },
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
  /** Benutzerbilder als data:-URL; null = keins, undefined = noch nicht geladen */
  avatars: {} as Record<string, string | null>,
  redirects: [] as Redirect[],
  /** „Umleitung (Art)“-Tasten, die beim Einschalten nach dem Ziel fragen */
  askKeys: [] as string[],
  /** Umleitungen, die gerade über eine solche Taste programmiert sind */
  programmed: [] as string[],
  /** Offene Zielabfrage */
  program: null as Program | null,
  /** Wählbare signalisierte Rufnummern, für „Rufnummer anzeigen“ */
  signaling: [] as SignalingNumber[],
  groups: [] as Membership[],
  /** Wählbare Gruppen für den Editor */
  groupChoices: [] as GroupChoice[],
  /** Module der Anlage für „Modul aktivieren“ */
  modules: [] as Module[],
  me: "",
  error: "",
  notice: "",
  loaded: false,
});

/** Zielabfrage einer Umleitungstaste: Nummer und bei Zeitüberschreitung die Wartezeit */
export type Program = { key: FunctionKey; ids: string[]; number: string; timeout: number | null };

/** Gruppe, in der man Mitglied ist (für „Gruppe An-/Abmelden“) */
export type Membership = { id: string; name: string; logon_id: string; logged_on: boolean; read_only: boolean };

/** Namen der gewählten Gruppen einer Taste */
export const groupNames = (k: FunctionKey) => fkeys.groupChoices.filter((g) => k.groupIds.includes(g.id)).map((g) => g.name);

/** Gruppen einer Taste: über die IDs, sonst über die Namen der gewählten
 *  Gruppen, sonst über den Namen in `Gruppe[Name]` */
function groupsOf(k: FunctionKey): Membership[] {
  const ids = k.groupIds.map(String);
  const byId = fkeys.groups.filter((g) => ids.includes(g.id) || ids.includes(g.logon_id));
  if (byId.length) return byId;
  const names = groupNames(k);
  const byName = fkeys.groups.filter((g) => names.includes(g.name));
  if (byName.length) return byName;
  const name = /\[(.*)\]$/.exec(k.name)?.[1] ?? k.name;
  return fkeys.groups.filter((g) => g.name === name);
}

let started = false;
let retry: ReturnType<typeof setTimeout> | undefined;
/** Neuer Versuch nach einem Fehler, z. B. wenn nach dem Aufwachen das Netz
 *  noch nicht da ist; danach alle 30 s, bis es klappt. */
const RETRY_MS = 10_000;
let retryMs = RETRY_MS;

export async function loadFkeys() {
  if (!started) {
    started = true;
    listen<Record<string, UserState>>("fkey-presence", (e) => (fkeys.presence = e.payload));
    listen<Membership[]>("fkey-groups", (e) => (fkeys.groups = e.payload));
    listen<Module[]>("fkey-modules", (e) => (fkeys.modules = e.payload));
    // Anderswo umgestellt (z. B. in der STARFACE-App)
    listen("me-signaling", () => loadSignaling());
    listen("me-avatar", () => { delete fkeys.avatars[fkeys.me]; });
    // Rechte geändert: Tasten und Zustände neu laden, wie die STARFACE-App
    listen("me-permission", () => loadFkeys());
    listen("reach-changed", () => loadRedirects());
    // Nach dem Standby neu laden (Token, Präsenz und Tasten frisch holen)
    listen("resumed", () => loadFkeys());
  }
  clearTimeout(retry);
  retry = undefined;
  try {
    const k = await invoke<Keys>("fkeys_load");
    fkeys.setId = k.set_id;
    fkeys.setName = k.set_name;
    fkeys.accountId = k.account_id;
    fkeys.keys = k.keys;
    fkeys.order = k.order;
    fkeys.accounts = k.accounts;
    fkeys.groupChoices = k.group_choices;
    fkeys.me = k.me;
    fkeys.error = "";
    fkeys.loaded = true;
    fkeys.presence = await invoke<Record<string, UserState>>("fkey_presence");
    fkeys.groups = await invoke<Membership[]>("fkey_groups");
    fkeys.modules = await invoke<Module[]>("fkey_modules");
    retryMs = RETRY_MS;
  } catch (e) {
    // Bisherige Tasten bleiben stehen; nur der Fehler wird angezeigt.
    fkeys.error = String(e);
    retry = setTimeout(() => loadFkeys(), retryMs);
    retryMs = Math.min(retryMs * 3, 30_000);
  }
  loadRedirects();
  loadSignaling();
}

/** Signalisierte Rufnummern neu holen (Zustand der Rufnummer-Tasten) */
export async function loadSignaling() {
  fkeys.signaling = await invoke<SignalingNumber[]>("signaling_numbers").catch(() => fkeys.signaling);
}

/** Nummer einer „Rufnummer anzeigen“-Taste; ohne Nummer (0) = unterdrücken */
const numberOf = (k: FunctionKey) =>
  !k.displayNumberId ? fkeys.signaling.find((x) => x.suppressed) : fkeys.signaling.find((x) => x.id === String(k.displayNumberId));


async function loadRedirects() {
  try {
    const o = await invoke<{ ask: string[]; programmed: string[] }>("fkey_redirect_options");
    fkeys.askKeys = o.ask;
    fkeys.programmed = o.programmed;
    fkeys.redirects = await invoke<Redirect[]>("redirects");
  } catch {
    // Umleitungen sind für die Anzeige nicht zwingend
  }
}

/** „Umleitung (Art)“ mit Zielabfrage: fragt beim Einschalten nach der Nummer
 *  und stellt beim Ausschalten wieder her, was vorher eingestellt war. */
export const asksTarget = (k: FunctionKey) => k.functionKeyType === "FORWARD" && fkeys.askKeys.includes(k.id);

export async function setAsksTarget(keyId: string, ask: boolean) {
  await invoke("fkey_set_ask", { keyId, ask });
  await loadRedirects();
}

/** Zielabfrage bestätigt: Umleitungen auf die Nummer umstellen und einschalten */
export async function programRedirect() {
  const p = fkeys.program;
  if (!p) return;
  if (!p.number.trim()) return void (fkeys.notice = t("Bitte eine Zielrufnummer eingeben."));
  fkeys.program = null;
  await call("fkey_redirect_program", { ids: p.ids, number: p.number, timeoutSecs: p.timeout });
  return loadRedirects();
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

/** User hinter einer BLF-Taste: OneHub-ID und Präsenz */
export function blfUser(k: FunctionKey): { id: string; group: boolean; state: UserState | undefined } | undefined {
  const a = account(k);
  const id = a?.user_ids.find((u) => fkeys.presence[u]) ?? a?.user_ids[0];
  return id ? { id, group: a?.group ?? false, state: fkeys.presence[id] } : undefined;
}

const loadingAvatars = new Set<string>();
/** Benutzerbild; lädt es beim ersten Zugriff nach (null = keins) */
export function avatarOf(userId: string): string | null {
  const url = fkeys.avatars[userId];
  if (url !== undefined) return url;
  if (!loadingAvatars.has(userId)) {
    loadingAvatars.add(userId);
    invoke<string | null>("fkey_avatar", { userId })
      .then((u) => (fkeys.avatars[userId] = u))
      .catch(() => (fkeys.avatars[userId] = null))
      .finally(() => loadingAvatars.delete(userId));
  }
  return null;
}

/** Eigener Chat-Status: wie ihn die Anlage meldet (auch von anderen Clients
 *  gesetzt), sonst die letzte eigene Wahl */
export function ownChat(userId: string): { availability: string; text: string } {
  const s = fkeys.presence[userId];
  if (s?.chat && s.chat !== "offline") return { availability: s.chat, text: s.chat_message };
  return { availability: prefs.value?.chat_availability || "available", text: prefs.value?.chat_text ?? "" };
}

/** Statuszeile wie in der STARFACE-App: eigener Text, sonst der Chat-Zustand */
export function chatText(s: UserState | undefined): string {
  if (!s?.chat) return "";
  if (s.chat_message) return s.chat_message;
  return ({ available: t("Verfügbar"), away: t("Abwesend"), dnd: t("Bitte nicht stören"), offline: t("Offline") } as Record<string, string>)[s.chat] ?? "";
}

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

/** Ob eine Umleitung zum Ziel einer „Umleitung auf Ziel“-Taste führt. Die
 *  Anlage schreibt die Nummer nicht immer so wie in der Taste (+41…, 0041…,
 *  076…), daher über die letzten Ziffern vergleichen. */
function sameTarget(k: FunctionKey, r: Redirect): boolean {
  if (k.forwardTargetType === "VOICEMAIL") return !!r.target.mailbox;
  const want = searchable(k.forwardTarget ?? "");
  return !!want && searchable(r.target.number ?? "") === want;
}

/** Umleitung zählt für die Taste als an: Taste mit festem Ziel nur, wenn sie
 *  auch dorthin führt; sonst teilen sich mehrere Tasten für dieselbe Nummer
 *  denselben Zustand. */
function activeFor(k: FunctionKey, r: Redirect): boolean {
  // Mit Zielabfrage an, solange die Taste die Umleitung programmiert hat
  if (asksTarget(k)) return r.enabled && fkeys.programmed.includes(r.id);
  return r.enabled && (k.functionKeyType !== "FORWARDTOTARGET" || sameTarget(k, r));
}

/** Zustand für die Farbe: "on", "partial", "busy", "ringing", "dnd", "free", "off", "none" oder "" */
export function keyState(k: FunctionKey): string {
  switch (k.functionKeyType) {
    case "BUSYLAMPFIELD": {
      const s = stateOf(account(k));
      if (!s) return "";
      // Gespräch geht vor Ruhe, damit man sieht, dass telefoniert wird
      if (s.telephony === "ringing") return "ringing";
      if (s.telephony === "active") return "busy";
      if (s.dnd) return "dnd";
      return s.telephony === "unavailable" ? "off" : "free";
    }
    case "DONOTDISTURB":
      return ownDnd() ? "on" : "";
    case "MODULEACTIVATION": {
      // an = alle aktiv; ohne passendes Modul (z. B. kein Recht) "none"
      const list = fkeys.modules.filter((m) => k.activateModuleIds.includes(m.id));
      return !list.length ? "none" : list.every((m) => m.active) ? "on" : "";
    }
    case "GROUPLOGIN": {
      // Schalter wie in der STARFACE-App: an = in einer der Gruppen angemeldet
      const list = groupsOf(k);
      return !list.length ? "none" : list.some((g) => g.logged_on) ? "on" : "";
    }
    case "FORWARD":
    case "FORWARDNUMBER":
    case "FORWARDTOTARGET": {
      // Wie in der STARFACE-App: teilaktiv, wenn nur manche Umleitungen an sind
      const list = redirectsOf(k);
      const on = list.filter((r) => activeFor(k, r)).length;
      return !on ? "" : on === list.length ? "on" : "partial";
    }
    case "COMPLETIONOFCALLSTOBUSYSUBSCRIBER":
      // blinkt, solange die Anlage den Rückruf anbietet; an, wenn aktiv
      return phone.status.callback === "active" ? "on" : phone.status.callback === "available" ? "ringing" : "";
    case "SIGNALNUMBER": {
      const n = numberOf(k);
      return !n ? "none" : n.selected ? "on" : "";
    }
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
    case "GROUPLOGIN":
      return call("fkey_group_toggle", { groupIds: k.groupIds, groupNames: groupNames(k), keyName: k.name });
    case "MODULEACTIVATION":
      return call("fkey_module_toggle", { moduleIds: k.activateModuleIds });
    case "COMPLETIONOFCALLSTOBUSYSUBSCRIBER":
      return call("phone_callback", {});
    case "SIGNALNUMBER": {
      await loadSignaling();
      const n = numberOf(k);
      if (!n) return void (fkeys.notice = t("Diese Rufnummer ist nicht mehr wählbar."));
      // Wie in der STARFACE-App: Die aktive Nummer abschalten heißt
      // unterdrücken; Anonym abschalten zeigt wieder die eigene Durchwahl.
      const anonymous = fkeys.signaling.find((x) => x.suppressed);
      const own = fkeys.signaling.find((x) => !x.suppressed && !x.group) ?? fkeys.signaling.find((x) => !x.suppressed);
      const target = !n.selected ? n : n.suppressed ? own : anonymous;
      if (!target) return;
      if (target.selected) return;
      await call("set_signaling_number", { id: target.id });
      return loadSignaling();
    }
    case "FORWARD":
    case "FORWARDNUMBER":
    case "FORWARDTOTARGET": {
      const list = redirectsOf(k);
      if (!list.length) return void (fkeys.notice = t("Keine passende Umleitung gefunden."));
      if (asksTarget(k)) {
        // Aus: alles zurückstellen, was die Taste programmiert hat
        const programmed = list.filter((r) => fkeys.programmed.includes(r.id)).map((r) => r.id);
        if (list.some((r) => activeFor(k, r))) {
          await call("fkey_redirect_restore", { ids: programmed });
          return loadRedirects();
        }
        const timeout = list.find((r) => r.kind === "timeout");
        fkeys.program = {
          key: k,
          ids: list.map((r) => r.id),
          number: list.find((r) => r.last_number)?.last_number ?? "",
          timeout: timeout ? timeout.timeout_secs || 20 : null,
        };
        return;
      }
      // Führt eine Umleitung woandershin, schaltet eine Taste mit festem
      // Ziel sie auf ihr Ziel um, statt sie abzuschalten.
      const enable = !list.some((r) => activeFor(k, r));
      for (const r of list) {
        if (enable && k.functionKeyType === "FORWARDTOTARGET" && !sameTarget(k, r)) {
          const target = k.forwardTargetType === "VOICEMAIL"
            ? { number: null, mailbox: r.mailboxes[0]?.id ?? null }
            : { number: k.forwardTarget, mailbox: null };
          await call("redirect_update", { id: r.id, target, timeoutSecs: null });
        }
        if (enable ? !r.enabled : activeFor(k, r)) await call("redirect_enable", { id: r.id, enabled: enable });
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
const FORWARD_TYPES = ["ALWAYS", "BUSY", "TIMEOUT"];

/** Arten, die schon eine „Umleitung (Art)“-Taste hat (außer `exceptId`).
 *  Die Anlage lehnt eine zweite Taste derselben Art ab. */
export function usedForwardTypes(exceptId = ""): string[] {
  return fkeys.keys
    .filter((k) => k.functionKeyType === "FORWARD" && k.id !== exceptId)
    .map((k) => k.forwardType ?? "ALWAYS");
}

/** Keine Art mehr frei: „Umleitung (Art)“ lässt sich nicht mehr anlegen */
export const forwardTypesExhausted = () => FORWARD_TYPES.every((x) => usedForwardTypes().includes(x));

export function blank(type: string): FunctionKey {
  return {
    functionKeyType: type, id: "", accountId: fkeys.accountId, valid: true, name: "", position: fkeys.keys.length,
    blfAccountId: null, directCallTargetnumber: null, redirectNumberIds: [], forwardTarget: null, forwardTargetType: null,
    forwardType: type === "FORWARD" ? (FORWARD_TYPES.find((x) => !usedForwardTypes().includes(x)) ?? "ALWAYS") : null, groupIds: [], poNumber: type === "PARKANDORBIT" ? "00" : null,
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
