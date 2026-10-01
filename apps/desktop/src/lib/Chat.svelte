<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount, tick } from "svelte";
  import Icon from "./Icon.svelte";
  import { chat, nameOf, openConversation, type ChatContact } from "./chat.svelte";
  import { initials } from "./contacts";

  let term = $state("");
  let draft = $state("");
  let error = $state("");
  let scroller = $state<HTMLElement>();

  onMount(() => {
    chat.visible = true;
    if (chat.open) chat.unread[chat.open] = 0;
    return () => (chat.visible = false);
  });

  const showText: Record<string, string> = {
    online: "Online", chat: "Online", away: "Abwesend", xa: "Länger abwesend", dnd: "Nicht stören", offline: "Offline",
  };

  // Kontakte mit Gesprächen zuerst (neueste oben), dann alphabetisch
  const list = $derived.by(() => {
    const t = term.trim().toLowerCase();
    const known = new Map<string, ChatContact>(chat.status.contacts.map((c) => [c.jid, c]));
    for (const peer of Object.keys(chat.last)) {
      if (!known.has(peer)) known.set(peer, { jid: peer, name: peer.split("@")[0], show: "offline", status: "" });
    }
    return [...known.values()]
      .filter((c) => !t || c.name.toLowerCase().includes(t) || c.jid.includes(t))
      .sort((a, b) => (chat.last[b.jid]?.ts ?? 0) - (chat.last[a.jid]?.ts ?? 0) || a.name.localeCompare(b.name));
  });

  $effect(() => {
    // Bei neuen Nachrichten nach unten scrollen
    chat.messages.length;
    tick().then(() => scroller?.scrollTo({ top: scroller.scrollHeight }));
  });

  async function send(event: Event) {
    event.preventDefault();
    const body = draft.trim();
    if (!body || !chat.open) return;
    try {
      await invoke("chat_send", { peer: chat.open, body });
      draft = "";
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) send(e);
  }

  const time = (ms: number) => {
    const d = new Date(ms);
    return d.toDateString() === new Date().toDateString()
      ? d.toLocaleTimeString("de-CH", { hour: "2-digit", minute: "2-digit" })
      : d.toLocaleString("de-CH", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });
  };
  const openContact = $derived(chat.status.contacts.find((c) => c.jid === chat.open));
</script>

<div class="chat">
  <aside>
    <div class="state" class:on={chat.status.online}>
      <span class="dot"></span>{chat.status.online ? "Chat verbunden" : chat.status.detail || "Chat nicht verbunden"}
    </div>
    <label class="filter">
      <Icon name="search" size={18} />
      <input bind:value={term} placeholder="Kontakt suchen" />
    </label>
    <div class="contacts">
      {#each list as c (c.jid)}
        <button class="contact" class:active={c.jid === chat.open} onclick={() => openConversation(c.jid)}>
          <span class="av">{initials(c.name)}<span class="presence {c.show}" title={showText[c.show] ?? c.show}></span></span>
          <span class="txt">
            <strong>{c.name}</strong>
            <small>{chat.last[c.jid]?.body ?? (c.status || showText[c.show] || "")}</small>
          </span>
          {#if chat.unread[c.jid]}<span class="badge">{chat.unread[c.jid]}</span>{/if}
        </button>
      {:else}
        <p class="muted">{chat.status.online ? "Keine Kontakte." : ""}</p>
      {/each}
    </div>
  </aside>

  <section class="conv">
    {#if chat.open}
      <header>
        <span class="av">{initials(nameOf(chat.open))}<span class="presence {openContact?.show ?? 'offline'}"></span></span>
        <div>
          <strong>{nameOf(chat.open)}</strong>
          <small>{openContact?.status || showText[openContact?.show ?? "offline"]}</small>
        </div>
      </header>
      <div class="messages" bind:this={scroller}>
        {#each chat.messages as m (m.id)}
          <div class="msg" class:out={m.outgoing}>
            <div class="bubble">{m.body}</div>
            <small>{time(m.ts)}</small>
          </div>
        {:else}
          <p class="muted center">Noch keine Nachrichten.</p>
        {/each}
      </div>
      <form class="compose" onsubmit={send}>
        <textarea bind:value={draft} {onkeydown} rows="2" placeholder={chat.status.online ? "Nachricht schreiben (Enter sendet, Umschalt+Enter neue Zeile)" : "Chat nicht verbunden"} disabled={!chat.status.online}></textarea>
        <button class="send" type="submit" title="Senden" disabled={!draft.trim() || !chat.status.online}><Icon name="send" size={20} /></button>
      </form>
      {#if error}<p class="error">{error}</p>{/if}
    {:else}
      <p class="muted center">Kontakt auswählen, um zu chatten.</p>
    {/if}
  </section>
</div>

<style>
  .chat { display: grid; grid-template-columns: minmax(15rem, 20rem) 1fr; gap: 0.5rem; height: 100%; min-height: 0; }
  aside { display: flex; flex-direction: column; min-height: 0; background: var(--panel); border-radius: 4px; padding: 0.5rem; gap: 0.4rem; }
  .state { display: flex; align-items: center; gap: 0.45rem; font-size: 0.85rem; color: var(--muted); padding: 0 0.3rem; }
  .state .dot { width: 0.6rem; height: 0.6rem; border-radius: 50%; background: #777; }
  .state.on .dot { background: var(--green); }
  .filter { display: flex; align-items: center; gap: 0.4rem; padding: 0 0.6rem; background: var(--bar-2); border: 1px solid var(--line); border-radius: 999px; }
  .filter input { border: none; background: none; outline: none; flex: 1; padding: 0.4rem 0; }
  .contacts { flex: 1; overflow: auto; display: flex; flex-direction: column; gap: 0.1rem; }
  .contact { display: flex; align-items: center; gap: 0.6rem; text-align: left; background: none; border: none; padding: 0.4rem; }
  .contact:hover { background: var(--panel-2); }
  .contact.active { background: var(--accent-soft); }
  .txt { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .txt strong, .txt small { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .txt small { color: var(--muted); }
  .av { position: relative; flex: none; width: 2.3rem; height: 2.3rem; border-radius: 50%; display: grid; place-items: center; background: var(--panel-2); font-size: 0.8rem; font-weight: 600; }
  .presence { position: absolute; right: -1px; bottom: -1px; width: 0.7rem; height: 0.7rem; border-radius: 50%; border: 2px solid var(--panel); background: #777; }
  .presence.online, .presence.chat { background: var(--green); }
  .presence.away, .presence.xa { background: var(--accent); }
  .presence.dnd { background: var(--red); }
  .badge { background: var(--accent); color: #111; border-radius: 999px; padding: 0 0.45rem; font-size: 0.8rem; font-weight: 700; }
  .conv { display: flex; flex-direction: column; min-height: 0; background: var(--panel); border-radius: 4px; }
  header { display: flex; align-items: center; gap: 0.7rem; padding: 0.6rem 0.9rem; border-bottom: 1px solid var(--line); }
  header div { display: flex; flex-direction: column; }
  header small { color: var(--muted); }
  .messages { flex: 1; overflow: auto; padding: 0.8rem 1rem; display: flex; flex-direction: column; gap: 0.5rem; }
  .msg { display: flex; flex-direction: column; align-items: flex-start; max-width: 75%; }
  .msg.out { align-self: flex-end; align-items: flex-end; }
  .bubble { background: var(--panel-2); padding: 0.45rem 0.75rem; border-radius: 12px 12px 12px 3px; white-space: pre-wrap; overflow-wrap: anywhere; }
  .msg.out .bubble { background: #7a5410; border-radius: 12px 12px 3px 12px; }
  .msg small { color: var(--muted); font-size: 0.72rem; margin: 0.1rem 0.3rem; }
  .compose { display: flex; gap: 0.5rem; align-items: flex-end; padding: 0.6rem; border-top: 1px solid var(--line); }
  textarea { flex: 1; resize: none; font: inherit; padding: 0.5rem 0.7rem; border-radius: 8px; border: 1px solid var(--line); background: var(--bar-2); color: inherit; }
  .send { width: 2.6rem; height: 2.6rem; padding: 0; border-radius: 50%; display: grid; place-items: center; background: var(--accent); border: none; color: #111; }
  .send:disabled { opacity: 0.4; }
  .muted { color: var(--muted); }
  .center { text-align: center; margin: auto; }
  .error { color: var(--accent); margin: 0 0.8rem 0.5rem; }
</style>
