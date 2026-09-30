<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import CallManager from "$lib/CallManager.svelte";
  import Icon, { type IconName } from "$lib/Icon.svelte";
  import { initPhone, phone, run } from "$lib/phone.svelte";

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
    ];
    initPhone();
    restore();
    return () => offs.forEach((p) => p.then((off) => off()));
  });

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

  async function logout() {
    try {
      await invoke("logout");
      notice = "Abgemeldet";
    } catch (e) {
      notice = `Abmelden unvollständig: ${e}`;
    }
    session = null;
    phase = "login";
  }

  let dialNumber = $state("");
  let dialing = $state(false);

  async function dial(event: Event) {
    event.preventDefault();
    if (!dialNumber.trim() || dialing) return;
    dialing = true;
    if (await run("phone_dial", { number: dialNumber })) dialNumber = "";
    dialing = false;
  }

  function initials(name: string) {
    return name.split(/\s+/).filter(Boolean).slice(0, 2).map((w) => w[0]).join("").toUpperCase() || "?";
  }

  const nav: { icon: IconName; label: string; ready: boolean }[] = [
    { icon: "call", label: "Anrufe", ready: false },
    { icon: "chat", label: "Chat", ready: false },
    { icon: "meetings", label: "Meetings", ready: false },
    { icon: "contacts", label: "Kontakte", ready: false },
    { icon: "overview", label: "Übersicht", ready: false },
    { icon: "list", label: "Listen", ready: false },
    { icon: "workspace", label: "Workspace", ready: true },
  ];
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
      <div class="me" title="{session.display_name} · STARFACE {session.server_version} · {session.server}">
        {initials(session.display_name)}
        <span class="reg {phone.status.state}" title={phone.status.detail || stateText[phone.status.state]}></span>
      </div>
      <form class="dial" onsubmit={dial}>
        <label class="search">
          <Icon name="search" size={20} />
          <input bind:value={dialNumber} placeholder="Name/Nummer eingeben" inputmode="tel" autocomplete="off" />
        </label>
        <button
          class="dialbtn"
          class:armed={dialNumber.trim() && phone.status.state === "ready"}
          type="submit"
          title="Anrufen"
          disabled={phone.status.state !== "ready" || dialing || !dialNumber.trim()}
        >
          <Icon name="call" />
        </button>
      </form>
      <div class="spacer"></div>
      <CallManager />
      <div class="brand"><span class="star">✱</span> STARFACE</div>
    </header>

    <nav class="rail">
      {#each nav as item}
        <button class="navitem" class:active={item.ready} disabled={!item.ready} title={item.ready ? item.label : `${item.label} (folgt)`}>
          <Icon name={item.icon} size={26} />
          <span>{item.label}</span>
        </button>
      {/each}
      <div class="spacer"></div>
      <button class="navitem small" title="Abmelden" onclick={logout}>
        <Icon name="logout" size={22} />
      </button>
    </nav>

    <main class="work">
      <section class="tile">
        <h2>Willkommen, {session.display_name}</h2>
        <p class="state"><span class="reg {phone.status.state}"></span>{stateText[phone.status.state]}{#if phone.status.state === "error" && phone.status.detail}: {phone.status.detail}{/if}</p>
        <p>Nummer oben eingeben und mit dem Hörer wählen. Gespräche erscheinen im Call Manager oben rechts.</p>
        <p class="muted">Favoriten, Adressbuch, Rufliste und Chat folgen als Nächstes.</p>
        {#if phone.notice}<p class="notice">{phone.notice}</p>{/if}
        {#if notice}<p class="notice">{notice}</p>{/if}
      </section>
    </main>
  </div>
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
  :global(body) { margin: 0; }
  :global(input), :global(button) {
    font: inherit; padding: 0.55rem 0.8rem; border-radius: 6px; border: 1px solid var(--line);
    background: var(--bar-2); color: inherit;
  }
  :global(button) { cursor: pointer; }
  :global(button:disabled) { cursor: default; }

  .shell {
    display: grid; grid-template-columns: 5.5rem 1fr; grid-template-rows: auto 1fr;
    height: 100vh; overflow: hidden;
  }
  .top {
    grid-column: 1 / -1; display: flex; align-items: center; gap: 0.8rem;
    padding: 0.5rem 1rem 0.5rem 0.6rem; background: var(--bar);
  }
  .me {
    position: relative; width: 3rem; height: 3rem; border-radius: 50%; flex: none;
    display: grid; place-items: center; font-weight: 600;
    background: linear-gradient(135deg, #3a7bd5, #00a37a); border: 3px solid var(--green);
  }
  .reg { display: inline-block; width: 0.7rem; height: 0.7rem; border-radius: 50%; background: #777; }
  .me .reg { position: absolute; right: -2px; top: -2px; border: 2px solid var(--bar); }
  .reg.ready { background: var(--green); }
  .reg.starting { background: var(--accent); }
  .reg.error { background: var(--red); }
  .dial { display: flex; align-items: center; gap: 0.5rem; }
  .search {
    display: flex; align-items: center; gap: 0.5rem; padding: 0 0.9rem;
    background: var(--bar-2); border: 1px solid var(--line); border-radius: 999px; width: 17rem;
  }
  .search:focus-within { border-color: var(--accent); }
  .search input { border: none; background: none; padding: 0.55rem 0; flex: 1; outline: none; }
  .dialbtn {
    width: 2.6rem; height: 2.6rem; padding: 0; border-radius: 50%; display: grid; place-items: center;
    background: var(--panel-2); border: none; color: var(--text);
  }
  .dialbtn.armed { background: var(--green); color: #fff; }
  .spacer { flex: 1; }
  .brand { font-weight: 700; letter-spacing: 0.12em; color: var(--accent); white-space: nowrap; }
  .brand .star { display: inline-grid; place-items: center; width: 1.6rem; height: 1.6rem; background: var(--accent); color: #fff; border-radius: 5px; letter-spacing: 0; margin-right: 0.3rem; }
  .brand.big { font-size: 1.6rem; margin-bottom: 1.5rem; }

  .rail { display: flex; flex-direction: column; gap: 0.2rem; background: var(--bar); padding: 0.4rem 0.35rem; }
  .navitem {
    display: flex; flex-direction: column; align-items: center; gap: 0.2rem; padding: 0.55rem 0.2rem;
    background: none; border: none; border-radius: 4px; font-size: 0.78rem; color: var(--text);
  }
  .navitem:disabled { opacity: 0.4; }
  .navitem.active { background: var(--accent); }
  .navitem.small { opacity: 0.8; }

  .work { overflow: auto; padding: 0.4rem; }
  .tile { background: var(--panel); border-radius: 4px; padding: 1rem 1.25rem; max-width: 40rem; }
  .tile h2 { margin: 0 0 0.5rem; font-size: 1.2rem; }
  .state { display: flex; align-items: center; gap: 0.5rem; }
  .muted { color: var(--muted); }
  .notice { color: var(--accent); }

  .login { max-width: 26rem; margin: 15vh auto 0; padding: 0 1rem; }
  .login form { display: flex; flex-direction: column; gap: 0.75rem; }
  .primary { background: var(--accent); border-color: var(--accent); color: #111; font-weight: 600; }
</style>
