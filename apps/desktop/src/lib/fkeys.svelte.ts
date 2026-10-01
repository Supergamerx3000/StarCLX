// Funktionstasten: Daten von der Anlage, Zustände und Auslösen.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { phone, run } from "./phone.svelte";

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
export type Account = { account_id: number; uuid: string; name: string; number: string };
type Keys = { set_id: string; account_id: string; keys: FunctionKey[]; accounts: Account[]; me: string };
type UserState = { telephony: string; dnd: boolean };
export type Redirect = {
  id: string; kind: string; called_number: string; called_number_id: string; group: boolean; enabled: boolean;
  target: { number: string | null; mailbox: string | null }; mailboxes: { id: string; name: string }[];
};
export type SignalingNumber = { id: string; number: string; suppressed: boolean; read_only: boolean; selected: boolean };

/** Gruppe in der Typenliste; "desk" = nur auf Tischtelefonen */
type Group = "fav" | "fn" | "desk";
export const TYPES: { type: string; label: string; group: Group; usable: boolean }[] = [
  { type: "BUSYLAMPFIELD", label: "Besetztlampenfeld", group: "fav", usable: true },
  { type: "QUICKDIAL", label: "Direktwahl", group: "fav", usable: true },
  { type: "GROUPLOGIN", label: "Gruppe An-/Abmelden", group: "fn", usable: false },
  { type: "DONOTDISTURB", label: "Ruhe", group: "fn", usable: true },
  { type: "COMPLETIONOFCALLSTOBUSYSUBSCRIBER", label: "Rückruf bei Besetzt", group: "fn", usable: false },
  { type: "SIGNALNUMBER", label: "Rufnummer anzeigen", group: "fn", usable: true },
  { type: "FORWARD", label: "Umleitung (Art)", group: "fn", usable: true },
  { type: "FORWARDNUMBER", label: "Umleitung (Rufnummer)", group: "fn", usable: true },
  { type: "FORWARDTOTARGET", label: "Umleitung auf Ziel", group: "fn", usable: true },
  { type: "PARKANDORBIT", label: "Parken", group: "fn", usable: true },
  { type: "MODULEACTIVATION", label: "Modul aktivieren", group: "fn", usable: false },
  { type: "ADDRESSBOOK", label: "Telefonmenü Adressbuch", group: "desk", usable: false },
  { type: "PHONECALLLIST", label: "Telefonmenü Rufliste", group: "desk", usable: false },
  { type: "PHONEGENERICURL", label: "Telefonbasierte URL", group: "desk", usable: false },
  { type: "PHONEDTMF", label: "DTMF", group: "desk", usable: true },
  { type: "SEPARATOR", label: "Leere Taste", group: "desk", usable: false },
];
export const typeInfo = (t: string) => TYPES.find((x) => x.type === t) ?? { type: t, label: t, group: "fn" as Group, usable: false };

export const fkeys = $state({
  setId: "",
  accountId: "",
  keys: [] as FunctionKey[],
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
    fkeys.accountId = k.account_id;
    fkeys.keys = k.keys;
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
      const s = fkeys.presence[account(k)?.uuid ?? ""];
      if (!s) return "";
      return s.telephony === "ringing" ? "ringing" : s.telephony === "active" ? "busy" : s.telephony === "unavailable" ? "off" : "free";
    }
    case "DONOTDISTURB":
      return ownDnd() ? "on" : "";
    case "FORWARD":
    case "FORWARDNUMBER":
    case "FORWARDTOTARGET":
      return redirectsOf(k).some((r) => r.enabled) ? "on" : "";
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
  } catch (e) {
    fkeys.notice = String(e);
  }
}

/** Taste gedrückt */
export async function press(k: FunctionKey) {
  fkeys.notice = "";
  switch (k.functionKeyType) {
    case "BUSYLAMPFIELD": {
      const a = account(k);
      if (a?.number) run("phone_dial", { number: a.number });
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
      if (!n) return void (fkeys.notice = "Diese Rufnummer ist nicht mehr wählbar.");
      return call("set_signaling_number", { id: n.id });
    }
    case "FORWARD":
    case "FORWARDNUMBER":
    case "FORWARDTOTARGET": {
      const list = redirectsOf(k);
      if (!list.length) return void (fkeys.notice = "Keine passende Umleitung gefunden.");
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
      const c = activeCall();
      if (!c) return void (fkeys.notice = "Kein Gespräch zum Parken.");
      return call("fkey_park", { callId: c.id, number: k.poNumber ?? "" });
    }
    case "PHONEDTMF": {
      const c = activeCall();
      if (!c) return void (fkeys.notice = "Tastentöne gehen nur während eines Gesprächs.");
      return call("phone_dtmf", { callId: c.id, digits: k.dtmf ?? "" });
    }
    default:
      fkeys.notice = `„${typeInfo(k.functionKeyType).label}“ lässt sich im Linux-Client noch nicht auslösen.`;
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
