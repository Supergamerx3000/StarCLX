<script lang="ts">
  import { connection } from "../../connection.svelte";
  import { splitter } from "../../splitter";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { onMount, tick } from "svelte";
  import Icon from "../../Icon.svelte";
  import { acceptsFiles, chat, createRoom, fileSize, leaveRoom, nameOf, openConversation, roomOf, sendFiles, type ChatMessage, type ChatRoom, type ChatTransfer } from "./chat.svelte";
  import { initials } from "../contacts/contacts";
  import { numberParts } from "../../numbers";
  import { canDial, phone, run } from "../call/phone.svelte";
  import { locale, t } from "../../i18n.svelte";

  let term = $state("");
  let draft = $state("");
  let error = $state("");
  let scroller = $state<HTMLElement>();
  let conv = $state<HTMLElement>();
  /** Dateien werden über das Gespräch gezogen */
  let dropping = $state(false);
  /** Neuen Gruppenchat zusammenstellen: Thema und ausgewählte Kontakte */
  let picking = $state(false);
  let subject = $state("");
  let picked = $state<Record<string, boolean>>({});

  onMount(() => {
    chat.visible = true;
    if (chat.open) chat.unread[chat.open] = 0;
    // Drag & Drop aus dem Dateimanager: Tauri liefert die Pfade
    const unlisten = getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "leave") return void (dropping = false);
      const over = canDrop() && overConv(p.position.x, p.position.y);
      if (p.type === "drop") {
        dropping = false;
        if (over) send_files(p.paths);
      } else dropping = over;
    });
    return () => {
      chat.visible = false;
      unlisten.then((f) => f());
    };
  });

  const canDrop = () => !!chat.open && chat.status.online && !roomOf(chat.open) && acceptsFiles(chat.open);

  function overConv(x: number, y: number) {
    const r = conv?.getBoundingClientRect();
    if (!r) return false;
    const [cx, cy] = [x / devicePixelRatio, y / devicePixelRatio];
    return cx >= r.left && cx <= r.right && cy >= r.top && cy <= r.bottom;
  }

  async function send_files(paths: string[]) {
    try {
      await sendFiles(paths);
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  async function pickFiles() {
    try {
      await send_files(await invoke<string[]>("chat_pick_files"));
    } catch (e) {
      error = String(e);
    }
  }

  async function transferAction(cmd: string, id: string) {
    try {
      await invoke(cmd, { id });
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  // Nachrichten und Dateien des offenen Gesprächs in zeitlicher Folge
  type Item = { kind: "msg"; m: ChatMessage } | { kind: "file"; f: ChatTransfer };
  const items = $derived.by(() => {
    const files: Item[] = Object.values(chat.transfers)
      .filter((f) => f.peer === chat.open)
      .map((f) => ({ kind: "file", f }));
    const msgs: Item[] = chat.messages.map((m) => ({ kind: "msg", m }));
    const ts = (i: Item) => (i.kind === "msg" ? i.m.ts : i.f.ts);
    return [...msgs, ...files].sort((a, b) => ts(a) - ts(b));
  });

  function fileState(f: ChatTransfer) {
    switch (f.state) {
      case "offered": return t("Möchte dir eine Datei senden");
      case "waiting": return t("Wartet auf Annahme …");
      case "running": return t("{done} von {size}", { done: fileSize(f.done), size: fileSize(f.size) });
      case "done": return f.outgoing ? t("Gesendet") : t("Gespeichert");
      case "declined": return t("Abgelehnt");
      case "cancelled": return f.error ? t(f.error) : t("Abgebrochen");
      case "failed": return t("Fehlgeschlagen: {e}", { e: t(f.error) });
    }
  }

  const showText: Record<string, string> = $derived({
    online: t("Online"), chat: t("Online"), away: t("Abwesend"), xa: t("Länger abwesend"), dnd: t("Nicht stören"), offline: t("Offline"),
  });

  // Kontakte und Gruppenchats mit Gesprächen zuerst (neueste oben), dann alphabetisch
  type Entry = { jid: string; name: string; show: string; status: string; room?: ChatRoom };
  const list = $derived.by(() => {
    const q = term.trim().toLowerCase();
    const known = new Map<string, Entry>(chat.status.contacts.map((c) => [c.jid, c]));
    for (const r of chat.status.rooms) {
      known.set(r.jid, { jid: r.jid, name: r.name || t("Gruppenchat"), show: r.joined ? "online" : "offline", status: "", room: r });
    }
    for (const peer of Object.keys(chat.last)) {
      // Verlassene Gruppenchats nicht als Kontakt zeigen
      if (!known.has(peer) && !peer.includes("@chatrooms.")) known.set(peer, { jid: peer, name: peer.split("@")[0], show: "offline", status: "" });
    }
    return [...known.values()]
      .filter((c) => !q || c.name.toLowerCase().includes(q) || c.jid.includes(q))
      .sort((a, b) => (chat.last[b.jid]?.ts ?? 0) - (chat.last[a.jid]?.ts ?? 0) || a.name.localeCompare(b.name));
  });

  $effect(() => {
    // Bei neuen Nachrichten und Dateien nach unten scrollen
    items.length;
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
      ? d.toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" })
      : d.toLocaleString(locale(), { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });
  };
  const openContact = $derived(chat.status.contacts.find((c) => c.jid === chat.open));
  const openRoom = $derived(roomOf(chat.open));
  const filesOk = $derived(!!chat.open && !openRoom && acceptsFiles(chat.open));
  // Wer im Gruppenchat anwesend ist
  const memberLine = $derived(
    !openRoom
      ? ""
      : !openRoom.joined
        ? t("Verbinde …")
        : openRoom.members.length
          ? openRoom.members.map((m) => m.name || nameOf(m.jid)).join(", ")
          : t("Sonst niemand anwesend"),
  );
  // Kontakte zur Auswahl für einen neuen Gruppenchat
  const pickable = $derived(
    chat.status.contacts
      .filter((c) => !term.trim() || c.name.toLowerCase().includes(term.trim().toLowerCase()))
      .sort((a, b) => a.name.localeCompare(b.name)),
  );
  const pickedJids = $derived(Object.keys(picked).filter((j) => picked[j]));

  function startPicking() {
    picking = !picking;
    subject = "";
    picked = {};
  }

  async function startRoom() {
    try {
      await createRoom(subject, pickedJids);
      picking = false;
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  async function leave() {
    if (!openRoom) return;
    try {
      await leaveRoom(openRoom.jid);
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  /** Vorschau der letzten Nachricht, im Gruppenchat mit Absender */
  function preview(jid: string) {
    const m = chat.last[jid];
    if (!m) return "";
    const who = m.outgoing ? "" : m.sender_name || (m.sender ? nameOf(m.sender) : "");
    return who ? `${who}: ${m.body}` : m.body;
  }
  // Angemeldete Geräte, z. B. „STARFACE Windows · unbekannter Client“
  const clientNames = $derived(
    (openContact?.clients ?? []).map((k) => k.name || t("unbekannter Client")).join(" · "),
  );
  const attachTitle = $derived(
    !chat.status.online
      ? t("Chat nicht verbunden")
      : openRoom
        ? t("Dateien gehen nur an einzelne Kontakte")
        : filesOk
        ? t("Datei senden (oder hierher ziehen)")
        : (openContact?.clients.length ?? 0)
          ? t("Der Client des Kontakts kann keine Dateien empfangen")
          : t("Kontakt ist nicht online"),
  );
</script>

<div class="chat">
  <aside>
    <div class="state" class:on={chat.status.online}>
      <span class="dot"></span>{chat.status.online ? t("Chat verbunden") : (connection.online && chat.status.detail) || t("Chat nicht verbunden")}
    </div>
    <div class="tools">
      <label class="filter">
        <Icon name="search" size={18} />
        <input bind:value={term} placeholder={t("Kontakt suchen")} />
      </label>
      <button class="newroom" class:on={picking} title={t("Gruppenchat starten")} onclick={startPicking} disabled={!chat.status.online}><Icon name="groups" size={18} /></button>
    </div>
    {#if picking}
      <div class="pick">
        <input bind:value={subject} placeholder={t("Thema (optional)")} />
        <div class="contacts">
          {#each pickable as c (c.jid)}
            <label class="contact">
              <input type="checkbox" bind:checked={picked[c.jid]} />
              <span class="av">{initials(c.name)}<span class="presence {c.show}" title={showText[c.show] ?? c.show}></span></span>
              <span class="txt"><strong>{c.name}</strong></span>
            </label>
          {:else}
            <p class="muted">{t("Keine Kontakte.")}</p>
          {/each}
        </div>
        <div class="pickactions">
          <button onclick={() => (picking = false)}>{t("Abbrechen")}</button>
          <button class="primary" onclick={startRoom} disabled={!pickedJids.length || !chat.status.online}>{t("Gruppenchat starten")}</button>
        </div>
      </div>
    {:else}
      <div class="contacts">
        {#each list as c (c.jid)}
          <button class="contact" class:active={c.jid === chat.open} onclick={() => openConversation(c.jid)}>
            {#if c.room}
              <span class="av room" title={t("Gruppenchat")}><Icon name="groups" size={20} /></span>
            {:else}
              <span class="av">{initials(c.name)}<span class="presence {c.show}" title={showText[c.show] ?? c.show}></span></span>
            {/if}
            <span class="txt">
              <strong>{c.name}</strong>
              <small>{preview(c.jid) || (c.room ? t("Gruppenchat") : c.status || showText[c.show] || "")}</small>
            </span>
            {#if chat.unread[c.jid]}<span class="badge">{chat.unread[c.jid]}</span>{/if}
          </button>
        {:else}
          <p class="muted">{chat.status.online ? t("Keine Kontakte.") : ""}</p>
        {/each}
      </div>
    {/if}
  </aside>
  <div class="divider" role="separator" aria-orientation="vertical" use:splitter={"chat"}></div>

  <section class="conv" class:dropping bind:this={conv}>
    {#if chat.open}
      <header>
        {#if openRoom}
          <span class="av room"><Icon name="groups" size={20} /></span>
          <div class="who">
            <strong>{openRoom.name || t("Gruppenchat")}</strong>
            <small class="members" title={memberLine}>{memberLine}</small>
          </div>
          {#if !openRoom.group}<button class="leave" title={t("Gruppenchat verlassen")} onclick={leave}>{t("Verlassen")}</button>{/if}
        {:else}
          <span class="av">{initials(nameOf(chat.open))}<span class="presence {openContact?.show ?? 'offline'}"></span></span>
          <div class="who">
            <strong>{nameOf(chat.open)}</strong>
            <small>{openContact?.status || showText[openContact?.show ?? "offline"]}</small>
            {#if clientNames}<small class="clients" title={(openContact?.clients ?? []).map((k) => k.resource).join("\n")}>{clientNames}</small>{/if}
          </div>
        {/if}
      </header>
      <div class="messages" bind:this={scroller}>
        {#each items as item (item.kind === "msg" ? item.m.id : `file-${item.f.id}`)}
          {#if item.kind === "msg"}
            {@const m = item.m}
            <div class="msg" class:out={m.outgoing}>
              {#if openRoom && !m.outgoing}<small class="from">{m.sender_name || nameOf(m.sender ?? "")}</small>{/if}
              <div class="bubble">{#each numberParts(m.body) as p}{#if p.number}<button class="num" title={t("Anrufen")} disabled={!canDial()} onclick={() => run("phone_dial", { number: p.number })}>{p.text}</button>{:else}{p.text}{/if}{/each}</div>
              <small>{time(m.ts)}</small>
            </div>
          {:else}
            {@const f = item.f}
            <div class="msg" class:out={f.outgoing}>
              <div class="bubble file" class:failed={f.state === "failed"}>
                <Icon name="file" size={30} />
                <div class="finfo">
                  <strong title={f.name}>{f.name}</strong>
                  <small>{fileSize(f.size)} · {fileState(f)}</small>
                  {#if f.state === "running"}<progress max={f.size || 1} value={f.done}></progress>{/if}
                  <div class="factions">
                    {#if f.state === "offered"}
                      <button class="primary" onclick={() => transferAction("chat_accept_file", f.id)}>{t("Speichern")}</button>
                      <button onclick={() => transferAction("chat_decline_file", f.id)}>{t("Ablehnen")}</button>
                    {:else if f.state === "waiting" || f.state === "running"}
                      <button onclick={() => transferAction("chat_cancel_file", f.id)}>{t("Abbrechen")}</button>
                    {:else if f.state === "done"}
                      <button onclick={() => transferAction("chat_open_file", f.id)}>{t("Öffnen")}</button>
                      <button onclick={() => transferAction("chat_show_file", f.id)}>{t("Im Ordner zeigen")}</button>
                    {/if}
                  </div>
                </div>
              </div>
              <small>{time(f.ts)}</small>
            </div>
          {/if}
        {:else}
          <p class="muted center">{t("Noch keine Nachrichten.")}</p>
        {/each}
      </div>
      <form class="compose" onsubmit={send}>
        <button class="attach" type="button" title={attachTitle} onclick={pickFiles} disabled={!chat.status.online || !filesOk}><Icon name="attach" size={20} /></button>
        <textarea bind:value={draft} {onkeydown} rows="2" placeholder={chat.status.online ? t("Nachricht schreiben (Enter sendet, Umschalt+Enter neue Zeile)") : t("Chat nicht verbunden")} disabled={!chat.status.online}></textarea>
        <button class="send" type="submit" title={t("Senden")} disabled={!draft.trim() || !chat.status.online}><Icon name="send" size={20} /></button>
      </form>
      {#if error && connection.online}<p class="error">{error}</p>{/if}
      {#if dropping}<div class="drop">{t("Loslassen, um an {name} zu senden", { name: nameOf(chat.open) })}</div>{/if}
    {:else}
      <p class="muted center">{t("Kontakt auswählen, um zu chatten.")}</p>
    {/if}
  </section>
</div>

<style>
  .chat { display: grid; grid-template-columns: min(var(--split, 18rem), calc(100% - 12rem)) 0.5rem minmax(0, 1fr); height: 100%; min-height: 0; }
  .divider { cursor: col-resize; touch-action: none; position: relative; }
  .divider::after { content: ""; position: absolute; inset: 0 3px; border-radius: 2px; }
  .divider:hover::after, .divider:global(.dragging)::after { background: var(--accent); }
  aside { display: flex; flex-direction: column; min-height: 0; background: var(--panel); border-radius: 4px; padding: 0.5rem; gap: 0.4rem; }
  .state { display: flex; align-items: center; gap: 0.45rem; font-size: 0.85rem; color: var(--muted); padding: 0 0.3rem; }
  .state .dot { width: 0.6rem; height: 0.6rem; border-radius: 50%; background: #777; }
  .state.on .dot { background: var(--green); }
  .tools { display: flex; gap: 0.4rem; align-items: center; }
  .tools .filter { flex: 1; min-width: 0; }
  .newroom { width: 2.1rem; height: 2.1rem; padding: 0; border-radius: 50%; display: grid; place-items: center; flex: none; background: none; border: 1px solid var(--line); color: inherit; }
  .newroom.on { background: var(--accent-soft); }
  .newroom:disabled { opacity: 0.4; }
  .pick { flex: 1; min-height: 0; display: flex; flex-direction: column; gap: 0.4rem; }
  .pick > input { padding: 0.4rem 0.6rem; border-radius: 6px; border: 1px solid var(--line); background: var(--bar-2); color: inherit; }
  .pick .contact { cursor: pointer; }
  .pickactions { display: flex; gap: 0.4rem; justify-content: flex-end; flex-wrap: wrap; }
  .pickactions .primary { background: var(--accent); color: var(--on-accent); border-color: var(--accent); }
  .pickactions .primary:disabled { opacity: 0.4; }
  .av.room { color: var(--muted); }
  .leave { margin-left: auto; padding: 0.2rem 0.7rem; font-size: 0.85rem; }
  header .who { min-width: 0; }
  header .members { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .msg .from { margin: 0 0.3rem 0.1rem; font-weight: 600; }
  .filter { display: flex; align-items: center; gap: 0.4rem; padding: 0 0.6rem; background: var(--bar-2); border: 1px solid var(--line); border-radius: 999px; }
  .filter input { border: none; background: none; outline: none; flex: 1; min-width: 0; text-overflow: ellipsis; padding: 0.4rem 0; }
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
  .badge { background: var(--accent); color: var(--on-accent); border-radius: 999px; padding: 0 0.45rem; font-size: 0.8rem; font-weight: 700; }
  .conv { position: relative; display: flex; flex-direction: column; min-height: 0; background: var(--panel); border-radius: 4px; }
  .conv.dropping { outline: 2px dashed var(--accent); outline-offset: -4px; }
  .drop { position: absolute; inset: 0; display: grid; place-items: center; background: color-mix(in srgb, var(--panel) 80%, transparent); font-weight: 600; pointer-events: none; border-radius: 4px; }
  .bubble.file { display: flex; gap: 0.6rem; align-items: flex-start; white-space: normal; min-width: 14rem; }
  .bubble.file.failed { opacity: 0.75; }
  .finfo { display: flex; flex-direction: column; gap: 0.25rem; min-width: 0; }
  .finfo strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .finfo small { color: var(--muted); margin: 0; font-size: 0.8rem; }
  .msg.out .finfo small { color: inherit; opacity: 0.8; }
  .finfo progress { width: 100%; accent-color: var(--accent); }
  .factions { display: flex; gap: 0.4rem; flex-wrap: wrap; }
  .factions:empty { display: none; }
  .factions button { padding: 0.2rem 0.7rem; font-size: 0.85rem; }
  .factions .primary { background: var(--accent); color: var(--on-accent); border-color: var(--accent); }
  .attach { width: 2.6rem; height: 2.6rem; padding: 0; border-radius: 50%; display: grid; place-items: center; background: none; border: 1px solid var(--line); color: inherit; }
  .attach:disabled { opacity: 0.4; }
  header { display: flex; align-items: center; gap: 0.7rem; padding: 0.6rem 0.9rem; border-bottom: 1px solid var(--line); }
  header div { display: flex; flex-direction: column; }
  header small { color: var(--muted); }
  header .clients { font-size: 0.75rem; }
  .messages { flex: 1; overflow: auto; padding: 0.8rem 1rem; display: flex; flex-direction: column; gap: 0.5rem; }
  .msg { display: flex; flex-direction: column; align-items: flex-start; max-width: 75%; }
  .msg.out { align-self: flex-end; align-items: flex-end; }
  .bubble { background: var(--panel-2); padding: 0.45rem 0.75rem; border-radius: 12px 12px 12px 3px; white-space: pre-wrap; overflow-wrap: anywhere; }
  .num { display: inline; padding: 0; border: none; background: none; color: inherit; font: inherit; text-decoration: underline; cursor: pointer; }
  .num:disabled { cursor: default; text-decoration: none; }
  .msg.out .bubble { background: #7a5410; border-radius: 12px 12px 3px 12px; }
  .msg small { color: var(--muted); font-size: 0.72rem; margin: 0.1rem 0.3rem; }
  .compose { display: flex; gap: 0.5rem; align-items: flex-end; padding: 0.6rem; border-top: 1px solid var(--line); }
  textarea { flex: 1; resize: none; font: inherit; padding: 0.5rem 0.7rem; border-radius: 8px; border: 1px solid var(--line); background: var(--bar-2); color: inherit; }
  .send { width: 2.6rem; height: 2.6rem; padding: 0; border-radius: 50%; display: grid; place-items: center; background: var(--accent); border: none; color: var(--on-accent); }
  .send:disabled { opacity: 0.4; }
  .muted { color: var(--muted); }
  .center { text-align: center; margin: auto; }
  .error { color: var(--accent); margin: 0 0.8rem 0.5rem; }
</style>
