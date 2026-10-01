<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import CallManager from "$lib/CallManager.svelte";
  import Contacts from "$lib/Contacts.svelte";
  import DialSearch from "$lib/DialSearch.svelte";
  import Journal from "$lib/Journal.svelte";
  import Chat from "$lib/Chat.svelte";
  import { initChat, unreadTotal } from "$lib/chat.svelte";
  import Icon, { type IconName } from "$lib/Icon.svelte";
  import Settings from "$lib/Settings.svelte";
  import { initPhone, phone, run, isRingingIn } from "$lib/phone.svelte";
  import { loadPrefs } from "$lib/prefs.svelte";

  type SessionInfo = { server: string; server_version: string; display_name: string };

  let server = $state("");
  let phase = $state<"restoring" | "login" | "waiting" | "session">("restoring");
  let notice = $state("");
  let session = $state<SessionInfo | null>(null);

  onMount(() => {
    const offs = [
      listen<SessionInfo>("session", (e) => { session = e.payload; notice = ""; phase = "session"; }),
      listen<string>("login-error", (e) => { notice = e.payload; phase = "login"; }),
      listen<string>("logged-out", (e) => { session = null; notice = e.payload; phase = "login"; }),
      listen<{ action: string; text: string | null }>("hotkey", (e) => hotkey(e.payload.action, e.payload.text)),
    ];
    initPhone();
    initChat();
    loadPrefs().catch(() => {});
    restore();
    return () => offs.forEach((p) => p.then((off) => off()));
  });

  /** Tastenkürzel aus GNOME (`starface-desktop --action …`) */
  function hotkey(action: string, text: string | null) {
    const calls = phone.status.calls;
    switch (action) {
      case "dial-selection":
      case "dial-clipboard": {
        const number = (text ?? "").replace(/[^\d+*#]/g, "");
        if (number) run("phone_dial", { number });
        else phone.notice = action === "dial-selection" ? "Keine Rufnummer markiert" : "Keine Rufnummer in der Zwischenablage";
        break;
      }
      case "answer": {
        const c = calls.find(isRingingIn);
        if (c) run("phone_answer", { callId: c.id });
        break;
      }
      case "hangup": {
        const order = ["connected", "ringback", "setup", "held"];
        const c = order.map((p) => calls.find((c) => c.phase === p)).find(Boolean);
        if (c) run("phone_hangup", { callId: c.id });
        break;
      }
      case "toggle-view":
        tab = tabs[(tabs.findIndex((t) => t.id === tab) + 1) % tabs.length].id;
        break;
    }
  }

  async function restore() {
    server = (await invoke<string | null>("last_server")) ?? "";
    try {
      const info = await invoke<SessionInfo | null>("restore_session");
      if (info) { session = info; phase = "session"; return; }
    } catch (e) {
      notice = `Automatische Anmeldung fehlgeschlagen: ${e}`;
    }
    phase = "login";
  }

  async function login(event: Event) {
    event.preventDefault();
    notice = "";
    phase = "waiting";
    try {
      await invoke("start_login", { server });
    } catch (e) {
      notice = String(e);
      phase = "login";
    }
  }

  let menuOpen = $state(false);
  let settingsOpen = $state(false);
  type Tab = "journal" | "contacts" | "chat";
  let tab = $state<Tab>("journal");
  const tabs: { id: Tab; icon: IconName; label: string }[] = [
    { id: "journal", icon: "history", label: "Rufliste" },
    { id: "contacts", icon: "contacts", label: "Adressbuch" },
    { id: "chat", icon: "chat", label: "Chat" },
  ];

  async function logout() {
    menuOpen = false;
    settingsOpen = false;
    try {
      await invoke("logout");
      notice = "Abgemeldet";
    } catch (e) {
      notice = `Abmelden unvollständig: ${e}`;
    }
    session = null;
    phase = "login";
  }

  function initials(name: string) {
    return name.split(/\s+/).filter(Boolean).slice(0, 2).map((w) => w[0]).join("").toUpperCase() || "?";
  }

  const stateText = {
    off: "Softphone aus",
    starting: "Softphone startet …",
    ready: "Softphone bereit",
    error: "Softphone nicht angemeldet",
  } as const;
</script>

{#if phase === "session" && session}
  <div class="shell">
    <header class="top">
      <div class="menuwrap">
        <button class="me" title="{session.display_name} · {stateText[phone.status.state]}" onclick={() => (menuOpen = !menuOpen)} aria-expanded={menuOpen}>
          {initials(session.display_name)}
          <span class="reg {phone.status.state}"></span>
        </button>
        {#if menuOpen}
          <button class="scrim" aria-label="Menü schliessen" onclick={() => (menuOpen = false)}></button>
          <div class="menu">
            <div class="who">
              <strong>{session.display_name}</strong>
              <span class="muted">STARFACE {session.server_version}</span>
              <span class="muted">{session.server}</span>
              <span class="state"><span class="reg {phone.status.state}"></span>{stateText[phone.status.state]}</span>
              {#if phone.status.state === "error" && phone.status.detail}<span class="notice">{phone.status.detail}</span>{/if}
            </div>
            <button class="item" onclick={() => { menuOpen = false; settingsOpen = true; }}><Icon name="settings" size={20} /> Einstellungen</button>
            <button class="item" onclick={logout}><Icon name="logout" size={20} /> Abmelden</button>
          </div>
        {/if}
      </div>
      <DialSearch />
      <div class="spacer"></div>
      <CallManager />
      <div class="brand"><span class="star">✱</span> STARFACE</div>
    </header>

    <nav class="tabs">
      {#each tabs as t}
        <button class="tab" class:active={tab === t.id} onclick={() => (tab = t.id)}>
          <Icon name={t.icon} size={20} /><span>{t.label}</span>
          {#if t.id === "chat" && unreadTotal()}<span class="unread">{unreadTotal()}</span>{/if}
        </button>
      {/each}
    </nav>

    <main class="work">
      {#if phone.status.state === "error" || (phone.status.state === "off" && phone.status.detail)}
        <p class="banner"><span class="reg {phone.status.state}"></span>{stateText[phone.status.state]}{#if phone.status.detail}: {phone.status.detail}{/if}</p>
      {/if}
      {#if notice}<p class="banner">{notice}</p>{/if}
      {#if tab === "journal"}
        <Journal />
      {:else if tab === "contacts"}
        <Contacts />
      {:else}
        <Chat />
      {/if}
    </main>
  </div>
  {#if settingsOpen}
    <Settings onclose={() => (settingsOpen = false)} onlogout={logout} />
  {/if}
{:else}
<main class="login">
  <div class="brand big"><span class="star">✱</span> STARFACE</div>
  {#if phase === "restoring"}
    <p>Verbinde …</p>
  {:else}
    <h1>Anmelden</h1>
    <form onsubmit={login}>
      <input placeholder="https://anlage.example.com" bind:value={server} required />
      <button class="primary" type="submit" disabled={phase === "waiting"}>
        {phase === "waiting" ? "Warte auf Browser …" : "Im Browser anmelden"}
      </button>
    </form>
  {/if}
  {#if notice}<p class="notice">{notice}</p>{/if}
</main>
{/if}

<style>
  :global(:root) {
    font-family: "Segoe UI", system-ui, sans-serif;
    color-scheme: dark;
    --bg: #1f2226;
    --bar: #16181b;
    --bar-2: #2a2d31;
    --panel: #2b2f34;
    --panel-2: #373b41;
    --line: #3d4248;
    --text: #e8eaed;
    --muted: #a3a9b0;
    --accent: #f5a31a;
    --accent-soft: #f5a31a33;
    --green: #5cb82b;
    --red: #d9262d;
    color: var(--text);
    background: var(--bg);
  }
  :global(:root[data-theme="light"]) {
    color-scheme: light;
    --bg: #eef0f3;
    --bar: #ffffff;
    --bar-2: #e4e7eb;
    --panel: #ffffff;
    --panel-2: #e6e9ed;
    --line: #d3d8de;
    --text: #1f2226;
    --muted: #5f6670;
    --accent-soft: #f5a31a40;
  }
  :global(body) { margin: 0; }
  :global(input), :global(button) {
    font: inherit; padding: 0.55rem 0.8rem; border-radius: 6px; border: 1px solid var(--line);
    background: var(--bar-2); color: inherit;
  }
  :global(button) { cursor: pointer; }
  :global(button:disabled) { cursor: default; }

  .shell {
    display: grid; grid-template-rows: auto auto 1fr;
    height: 100vh; overflow: hidden;
  }
  .top {
    display: flex; align-items: center; gap: 0.8rem;
    padding: 0.5rem 1rem 0.5rem 0.6rem; background: var(--bar);
  }
  .menuwrap { position: relative; }
  .me {
    position: relative; padding: 0; color: #fff; cursor: pointer; width: 3rem; height: 3rem; border-radius: 50%; flex: none;
    display: grid; place-items: center; font-weight: 600;
    background: linear-gradient(135deg, #3a7bd5, #00a37a); border: 3px solid var(--green);
  }
  .reg { display: inline-block; width: 0.7rem; height: 0.7rem; border-radius: 50%; background: #777; }
  .me .reg { position: absolute; right: -2px; top: -2px; border: 2px solid var(--bar); }
  .reg.ready { background: var(--green); }
  .reg.starting { background: var(--accent); }
  .reg.error { background: var(--red); }
  .spacer { flex: 1; }
  .brand { font-weight: 700; letter-spacing: 0.12em; color: var(--accent); white-space: nowrap; }
  .brand .star { display: inline-grid; place-items: center; width: 1.6rem; height: 1.6rem; background: var(--accent); color: #fff; border-radius: 5px; letter-spacing: 0; margin-right: 0.3rem; }
  .brand.big { font-size: 1.6rem; margin-bottom: 1.5rem; }

  .scrim { position: fixed; inset: 0; z-index: 14; background: transparent; border: none; padding: 0; cursor: default; }
  .menu {
    position: absolute; left: 0; top: calc(100% + 0.4rem); z-index: 15; min-width: 17rem;
    background: var(--panel); border: 1px solid var(--line); border-radius: 8px; box-shadow: 0 8px 24px #000a;
    display: flex; flex-direction: column; padding: 0.4rem;
  }
  .menu .who { display: flex; flex-direction: column; gap: 0.15rem; padding: 0.5rem 0.6rem 0.7rem; border-bottom: 1px solid var(--line); margin-bottom: 0.3rem; font-size: 0.9rem; }
  .menu .who strong { font-size: 1rem; }
  .item { display: flex; align-items: center; gap: 0.7rem; background: none; border: none; text-align: left; padding: 0.55rem 0.6rem; }
  .item:hover { background: var(--panel-2); }
  .tabs { display: flex; gap: 0.2rem; padding: 0 0.6rem; background: var(--bar); border-top: 1px solid var(--bar-2); }
  .tab { display: flex; align-items: center; gap: 0.45rem; background: none; border: none; border-bottom: 3px solid transparent; border-radius: 0; padding: 0.55rem 0.9rem; color: var(--muted); }
  .unread { background: var(--accent); color: #111; border-radius: 999px; padding: 0 0.45rem; font-size: 0.78rem; font-weight: 700; }
  .tab.active { color: var(--text); border-bottom-color: var(--accent); }
  .banner { display: flex; align-items: center; gap: 0.5rem; margin: 0 0 0.4rem; padding: 0.5rem 0.8rem; background: var(--panel); border-left: 3px solid var(--accent); }
  .work { overflow: auto; padding: 0.5rem; display: flex; flex-direction: column; min-height: 0; }
  .state { display: flex; align-items: center; gap: 0.5rem; }
  .muted { color: var(--muted); }
  .notice { color: var(--accent); }

  .login { max-width: 26rem; margin: 15vh auto 0; padding: 0 1rem; }
  .login form { display: flex; flex-direction: column; gap: 0.75rem; }
  .primary { background: var(--accent); border-color: var(--accent); color: #111; font-weight: 600; }
</style>
