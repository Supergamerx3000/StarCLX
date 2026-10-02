<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import CallManager from "$lib/CallManager.svelte";
  import Contacts from "$lib/Contacts.svelte";
  import DialSearch from "$lib/DialSearch.svelte";
  import Journal from "$lib/Journal.svelte";
  import Chat from "$lib/Chat.svelte";
  import Voicemail from "$lib/Voicemail.svelte";
  import FunctionKeys from "$lib/FunctionKeys.svelte";
  import { initVoicemail, loadVoicemails, unheard, voicemail } from "$lib/voicemail.svelte";
  import { initChat, unreadTotal } from "$lib/chat.svelte";
  import Icon, { type IconName } from "$lib/Icon.svelte";
  import Settings from "$lib/Settings.svelte";
  import { initPhone, phone, run, isRingingIn } from "$lib/phone.svelte";
  import { loadPrefs, prefs, savePrefs, type Tile } from "$lib/prefs.svelte";
  import Workspace, { tilesOf } from "$lib/Workspace.svelte";

  type SessionInfo = { server: string; server_version: string; display_name: string };
  type UntrustedCert = { host: string; port: number; fingerprint: string; reason: string };

  let server = $state("");
  let phase = $state<"restoring" | "login" | "waiting" | "session">("restoring");
  let notice = $state("");
  let session = $state<SessionInfo | null>(null);
  /** Zertifikat der Anlage, das der Benutzer bestätigen muss */
  let untrusted = $state<UntrustedCert | null>(null);

  onMount(() => {
    const offs = [
      listen<SessionInfo>("session", (e) => { session = e.payload; notice = ""; phase = "session"; loadVoicemails(); }),
      listen<string>("login-error", (e) => { notice = e.payload; phase = "login"; }),
      listen<string>("logged-out", (e) => { session = null; notice = e.payload; phase = "login"; voicemail.list = []; }),
      listen<{ action: string; text: string | null }>("hotkey", (e) => hotkey(e.payload.action, e.payload.text)),
    ];
    initPhone();
    initChat();
    initVoicemail();
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
      if (info) { session = info; phase = "session"; loadVoicemails(); return; }
    } catch (e) {
      notice = `Automatische Anmeldung fehlgeschlagen: ${e}`;
    }
    phase = "login";
  }

  async function login(event?: Event) {
    event?.preventDefault();
    notice = "";
    untrusted = null;
    phase = "waiting";
    try {
      const cert = await invoke<UntrustedCert | null>("check_certificate", { server });
      if (cert) {
        untrusted = cert;
        phase = "login";
        return;
      }
      await invoke("start_login", { server });
    } catch (e) {
      notice = String(e);
      phase = "login";
    }
  }

  async function trustCertificate() {
    if (!untrusted) return;
    await invoke("trust_certificate", { server, fingerprint: untrusted.fingerprint });
    // Weiter prüfen: gRPC kann ein anderes Zertifikat haben als der Web-Port.
    login();
  }

  let menuOpen = $state(false);
  let settingsOpen = $state(false);
  type Tab = "journal" | "voicemail" | "contacts" | "chat" | "fkeys";
  let tab = $state<Tab>("journal");
  const tabs: { id: Tab; icon: IconName; label: string }[] = [
    { id: "journal", icon: "history", label: "Rufliste" },
    { id: "voicemail", icon: "voicemail", label: "Voicemail" },
    { id: "contacts", icon: "contacts", label: "Adressbuch" },
    { id: "chat", icon: "chat", label: "Chat" },
    { id: "fkeys", icon: "dialpad", label: "Funktionstasten" },
  ];

  const meta = Object.fromEntries(tabs.map((t) => [t.id, t]));
  const free = $derived(prefs.value?.workspace === "free");
  let tiles = $state<Tile[]>([]);
  /** Zähler, die sonst am Reiter stehen, für die Kachel-Titelleiste */
  const badge = (id: string) => (id === "chat" ? unreadTotal() : id === "voicemail" ? unheard() : 0);

  /** Anordnung entsperrt; beim Start immer fixiert */
  let editing = $state(false);
  /** Stand vor dem Bearbeiten, für "Abbrechen" */
  let before: Tile[] = [];

  function editStart() {
    before = $state.snapshot(tiles);
    editing = true;
  }
  function editDone() {
    editing = false;
    saveLayout();
  }
  function editCancel() {
    tiles = before.map((t) => ({ ...t }));
    editing = false;
  }
  $effect(() => {
    tiles = tilesOf(prefs.value?.workspace_tiles);
  });

  /** Anordnung der Kacheln sichern */
  function saveLayout() {
    if (prefs.value) savePrefs({ ...$state.snapshot(prefs.value), workspace_tiles: $state.snapshot(tiles) }).catch(() => {});
  }

  function tabClick(id: Tab) {
    if (!free) return void (tab = id);
    const t = tiles.find((t) => t.id === id);
    if (t) t.visible = !t.visible;
  }

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
      {#if free && !editing}
        <button class="arrange" title="Anordnung bearbeiten" onclick={editStart}><Icon name="edit" size={20} /></button>
      {/if}
      <CallManager />
      <div class="brand"><span class="star">✱</span> STARFACE</div>
    </header>

    {#if !free || editing}
    <nav class="tabs">
      {#each tabs as t}
        <button
          class="tab"
          class:active={free ? tiles.find((x) => x.id === t.id)?.visible : tab === t.id}
          title={free && editing ? "Kachel ein- oder ausblenden" : undefined}
          onclick={() => tabClick(t.id)}
        >
          <Icon name={t.icon} size={20} /><span>{t.label}</span>
          {#if t.id === "chat" && unreadTotal()}<span class="unread">{unreadTotal()}</span>{/if}
          {#if t.id === "voicemail" && unheard()}<span class="unread">{unheard()}</span>{/if}
        </button>
      {/each}
      {#if free}
        <span class="spacer"></span>
        {#if editing}
          <button class="tab" title="Standardanordnung" onclick={() => (tiles = tilesOf(null))}><span>Standard</span></button>
          <button class="tab" title="Änderungen verwerfen" onclick={editCancel}><Icon name="close" size={20} /><span>Abbrechen</span></button>
          <button class="tab lock on" title="Anordnung speichern und fixieren" onclick={editDone}><Icon name="check" size={20} /><span>Fertig</span></button>
        {/if}
      {/if}
    </nav>
    {/if}

    <main class="work">
      {#if phone.status.state === "error" || (phone.status.state === "off" && phone.status.detail)}
        <p class="banner"><span class="reg {phone.status.state}"></span>{stateText[phone.status.state]}{#if phone.status.detail}: {phone.status.detail}{/if}</p>
      {/if}
      {#if notice}<p class="banner">{notice}</p>{/if}
      {#snippet view(id: string)}
        {#if id === "journal"}
          <Journal />
        {:else if id === "fkeys"}
          <FunctionKeys />
        {:else if id === "voicemail"}
          <Voicemail />
        {:else if id === "contacts"}
          <Contacts />
        {:else}
          <Chat />
        {/if}
      {/snippet}
      {#if free}
        <Workspace bind:tiles {editing} {meta} {badge} body={view} />
      {:else}
        {@render view(tab)}
      {/if}
    </main>
  </div>
  {#if settingsOpen}
    <Settings onclose={() => (settingsOpen = false)} onlogout={logout} server={session.server} />
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
    {#if untrusted}
      <div class="cert" role="alertdialog" aria-labelledby="cert-title">
        <h2 id="cert-title">Zertifikat nicht vertrauenswürdig</h2>
        <p>
          Das Zertifikat von <b>{untrusted.host}:{untrusted.port}</b> ist nicht von einer bekannten
          Stelle ausgestellt oder passt nicht zur Adresse ({untrusted.reason}). Das ist bei lokalen
          Anlagen mit selbstsigniertem Zertifikat üblich.
        </p>
        <p class="muted">Fingerabdruck (SHA-256), mit dem Zertifikat der Anlage vergleichen:</p>
        <code>{untrusted.fingerprint}</code>
        <div class="actions">
          <button onclick={() => (untrusted = null)}>Abbrechen</button>
          <button class="primary" onclick={trustCertificate}>Vertrauen und anmelden</button>
        </div>
      </div>
    {/if}
  {/if}
  {#if notice}<p class="notice">{notice}</p>{/if}
</main>
{/if}

<style>
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
  .arrange { display: grid; place-items: center; width: 2.4rem; height: 2.4rem; border-radius: 50%; padding: 0; color: var(--muted); }
  .tab.lock.on { color: #111; background: var(--accent); border-radius: 6px 6px 0 0; }
  .tab.active { color: var(--text); border-bottom-color: var(--accent); }
  .banner { display: flex; align-items: center; gap: 0.5rem; margin: 0 0 0.4rem; padding: 0.5rem 0.8rem; background: var(--panel); border-left: 3px solid var(--accent); }
  .work { overflow: auto; padding: 0.5rem; display: flex; flex-direction: column; min-height: 0; }
  .state { display: flex; align-items: center; gap: 0.5rem; }
  .muted { color: var(--muted); }
  .notice { color: var(--accent); }

  .login { max-width: 26rem; margin: 15vh auto 0; padding: 0 1rem; }
  .login form { display: flex; flex-direction: column; gap: 0.75rem; }
  .primary { background: var(--accent); border-color: var(--accent); color: #111; font-weight: 600; }
  .cert { margin-top: 1rem; padding: 0.75rem 1rem; border: 1px solid var(--accent); border-radius: 6px; background: var(--panel); }
  .cert h2 { margin: 0 0 0.5rem; font-size: 1rem; }
  .cert code { display: block; font-size: 0.8rem; word-break: break-all; }
  .cert .actions { display: flex; justify-content: flex-end; gap: 0.5rem; margin-top: 0.75rem; }
</style>
