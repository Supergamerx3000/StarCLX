// Gemeinsamer Stand des Chats (auch für das Ungelesen-Abzeichen am Reiter).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { locale } from "../../i18n.svelte";

export type ChatClient = { resource: string; name: string; files: boolean | null };
export type ChatContact = { jid: string; name: string; show: string; status: string; clients: ChatClient[] };
/** Bei Gruppenchats ist `peer` der Raum und `sender` der Absender */
export type ChatMessage = { id: string; peer: string; outgoing: boolean; body: string; ts: number; sender?: string; sender_name?: string };
export type ChatMember = { jid: string; name: string };
/** Gruppenchat: Raum einer Gruppe der Anlage (`group`) oder spontan gestartet */
export type ChatRoom = { jid: string; name: string; group: boolean; joined: boolean; members: ChatMember[] };
export type ChatStatus = { online: boolean; detail: string; own: string; contacts: ChatContact[]; rooms: ChatRoom[] };
export type TransferState = "offered" | "waiting" | "running" | "done" | "declined" | "cancelled" | "failed";
export type ChatTransfer = {
  id: string;
  peer: string;
  outgoing: boolean;
  name: string;
  size: number;
  done: number;
  state: TransferState;
  path: string;
  error: string;
  ts: number;
};

export const chat = $state({
  status: { online: false, detail: "", own: "", contacts: [], rooms: [] } as ChatStatus,
  /** Letzte Nachricht je Gespräch */
  last: {} as Record<string, ChatMessage>,
  unread: {} as Record<string, number>,
  /** Offenes Gespräch und sein Verlauf */
  open: "" as string,
  messages: [] as ChatMessage[],
  /** Dateiübertragungen dieser Sitzung */
  transfers: {} as Record<string, ChatTransfer>,
  /** Chat-Reiter sichtbar */
  visible: false,
});

let started = false;

export function initChat() {
  if (started) return;
  started = true;
  listen<ChatStatus>("chat-status", (e) => {
    chat.status = e.payload;
    if (e.payload.online) loadRecent();
  });
  listen<ChatMessage>("chat-message", (e) => {
    const m = e.payload;
    chat.last[m.peer] = m;
    if (m.peer === chat.open) {
      if (!chat.messages.some((x) => x.id === m.id)) chat.messages = [...chat.messages, m];
    }
    if (!m.outgoing && !(chat.visible && m.peer === chat.open)) {
      chat.unread[m.peer] = (chat.unread[m.peer] ?? 0) + 1;
    }
  });
  listen<[string, ChatMessage[]]>("chat-history", (e) => {
    const [peer, messages] = e.payload;
    if (peer === chat.open) chat.messages = messages;
    if (messages.length) chat.last[peer] = messages[messages.length - 1];
  });
  listen<ChatTransfer>("chat-transfer", (e) => {
    const tr = e.payload;
    const isNew = !chat.transfers[tr.id];
    chat.transfers[tr.id] = tr;
    if (isNew && tr.state === "offered" && !(chat.visible && tr.peer === chat.open)) {
      chat.unread[tr.peer] = (chat.unread[tr.peer] ?? 0) + 1;
    }
  });
  invoke<ChatStatus>("chat_status").then((s) => (chat.status = s));
  invoke<ChatTransfer[]>("chat_transfers").then((list) => {
    for (const tr of list) chat.transfers[tr.id] = tr;
  });
  loadRecent();
}

async function loadRecent() {
  try {
    for (const m of await invoke<ChatMessage[]>("chat_recent")) chat.last[m.peer] = m;
  } catch {
    // noch nicht verbunden
  }
}

export async function openConversation(peer: string) {
  chat.open = peer;
  chat.unread[peer] = 0;
  try {
    chat.messages = await invoke<ChatMessage[]>("chat_conversation", { peer });
  } catch (e) {
    chat.messages = [];
    chat.status.detail = String(e);
  }
}

export const unreadTotal = () => Object.values(chat.unread).reduce((a, b) => a + b, 0);

export const roomOf = (jid: string) => chat.status.rooms.find((r) => r.jid === jid);

export function nameOf(jid: string) {
  return chat.status.contacts.find((c) => c.jid === jid)?.name || roomOf(jid)?.name || jid.split("@")[0];
}

/** Spontanen Gruppenchat starten und gleich öffnen */
export async function createRoom(subject: string, members: string[]) {
  const room = await invoke<string>("chat_create_room", { subject, members });
  await openConversation(room);
}

export async function leaveRoom(room: string) {
  await invoke("chat_leave_room", { room });
  delete chat.last[room];
  delete chat.unread[room];
  if (chat.open === room) {
    chat.open = "";
    chat.messages = [];
  }
}

/** Ein Client des Kontakts nimmt Dateien an */
export function acceptsFiles(jid: string) {
  return !!chat.status.contacts.find((c) => c.jid === jid)?.clients.some((k) => k.files === true);
}

/** Dateien an das offene Gespräch senden */
export async function sendFiles(paths: string[]) {
  if (!chat.open || !paths.length) return;
  await invoke("chat_send_files", { peer: chat.open, paths });
}

export function fileSize(bytes: number) {
  const units = ["B", "KB", "MB", "GB"];
  let n = bytes;
  let i = 0;
  while (n >= 1024 && i < units.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toLocaleString(locale(), { maximumFractionDigits: i ? 1 : 0 })} ${units[i]}`;
}
