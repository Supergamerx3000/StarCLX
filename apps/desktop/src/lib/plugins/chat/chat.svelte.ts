// Gemeinsamer Stand des Chats (auch für das Ungelesen-Abzeichen am Reiter).
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type ChatContact = { jid: string; name: string; show: string; status: string };
export type ChatMessage = { id: string; peer: string; outgoing: boolean; body: string; ts: number };
export type ChatStatus = { online: boolean; detail: string; own: string; contacts: ChatContact[] };

export const chat = $state({
  status: { online: false, detail: "", own: "", contacts: [] } as ChatStatus,
  /** Letzte Nachricht je Gespräch */
  last: {} as Record<string, ChatMessage>,
  unread: {} as Record<string, number>,
  /** Offenes Gespräch und sein Verlauf */
  open: "" as string,
  messages: [] as ChatMessage[],
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
  invoke<ChatStatus>("chat_status").then((s) => (chat.status = s));
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

export function nameOf(jid: string) {
  return chat.status.contacts.find((c) => c.jid === jid)?.name || jid.split("@")[0];
}
