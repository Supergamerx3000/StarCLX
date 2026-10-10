<script lang="ts">
  import logo from "$lib/assets/logo.png";
  import ContactForm from "$lib/plugins/contacts/ContactForm.svelte";
  import { contactEdit } from "$lib/plugins/contacts/contactform.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import CallManager from "$lib/plugins/call/CallManager.svelte";
  import Contacts from "$lib/plugins/contacts/Contacts.svelte";
  import DialSearch from "$lib/DialSearch.svelte";
  import Journal from "$lib/plugins/journal/Journal.svelte";
  import Chat from "$lib/plugins/chat/Chat.svelte";
  import Voicemail from "$lib/plugins/voicemail/Voicemail.svelte";
  import FunctionKeys from "$lib/plugins/fkeys/FunctionKeys.svelte";
  import DoorCams from "$lib/plugins/doorcam/DoorCams.svelte";
  import Conferences from "$lib/plugins/conference/Conferences.svelte";
  import ConferenceForm from "$lib/plugins/conference/ConferenceForm.svelte";
  import { activeConferences, conferenceEdit, conferences, initConferences, loadConferences } from "$lib/plugins/conference/conference.svelte";
  import { initVoicemail, loadVoicemails, unheard, voicemail } from "$lib/plugins/voicemail/voicemail.svelte";
  import { initChat, unreadTotal } from "$lib/plugins/chat/chat.svelte";
  import Icon, { type IconName } from "$lib/Icon.svelte";
  import Settings from "$lib/Settings.svelte";
  import MeMenu from "$lib/MeMenu.svelte";
  import { avatarOf, ownChat, resetFkeys } from "$lib/plugins/fkeys/fkeys.svelte";
  import ChatBubble from "$lib/ChatBubble.svelte";
  import { canDial, initPhone, phone, run, isRingingIn } from "$lib/plugins/call/phone.svelte";
  import { loadPrefs, prefs, savePrefs, type Tile } from "$lib/prefs.svelte";
  import Workspace, { tilesOf } from "$lib/Workspace.svelte";
  import { t } from "$lib/i18n.svelte";

  type SessionInfo = { server: string; server_version: string; display_name: string; user_id: string };
  type UntrustedCert = { host: string; port: number; fingerprint: string; reason: string };
  type Account = { server: string; user_id: string; display_name: string; active: boolean };

  let server = $state("");
  let phase = $state<"restoring" | "login" | "waiting" | "session">("restoring");
  let notice = $state("");
  let session = $state<SessionInfo | null>(null);
  /** Zertifikat der Anlage, das der Benutzer bestätigen muss */
  let untrusted = $state<UntrustedCert | null>(null);
  /** Gespeicherte Konten für die Anmeldeseite */
  let accounts = $state<Account[]>([]);
  /** Weiteres Konto hinzufügen: der Login fragt immer nach den Zugangsdaten */
  let adding = $state(false);

  /** Daten des bisherigen Kontos verwerfen */
  function clearSessionData() {
    session = null;
    voicemail.list = [];
    conferences.list = [];
    conferenceEdit.id = null;
    resetFkeys();
    menuOpen = false;
    settingsOpen = false;
  }

  function showLogin(text: string) {
    clearSessionData();
    notice = text;
    phase = "login";
    loadAccounts();
  }

  async function loadAccounts() {
    accounts = await invoke<Account[]>("accounts").catch((): Account[] => []);
  }

  async function switchAccount(a: Account) {
    notice = "";
    try {
      await invoke("switch_account", { server: a.server, userId: a.user_id });
    } catch (e) {
      notice = String(e);
    }
  }

  async function forgetAccount(a: Account) {
    try {
      await invoke("forget_account", { server: a.server, userId: a.user_id });
    } catch (e) {
      notice = String(e);
    }
    if (a.active) showLogin(t("Abgemeldet"));
    else loadAccounts();
  }

  /** Laufendes Konto trennen (bleibt gespeichert) und ein weiteres anmelden */
  async function addAccount() {
    try {
      await invoke("disconnect");
    } catch (e) {
      notice = String(e);
      menuOpen = false;
      return;
    }
    if (!serverLocked) server = "";
    adding = true;
    showLogin("");
  }

  const accountHost = (a: Account) => a.server.replace(/^https?:\/\//, "");

  onMount(() => {
    const offs = [
      listen<SessionInfo>("session", (e) => { session = e.payload; notice = ""; adding = false; phase = "session"; loadVoicemails(); loadConferences(); }),
      listen<string>("login-error", (e) => { notice = e.payload; phase = "login"; }),
      listen<string>("logged-out", (e) => showLogin(e.payload)),
      // Kontowechsel: erst trennen, dann das andere Konto verbinden
      listen("switching", () => { clearSessionData(); notice = ""; phase = "restoring"; }),
      listen<{ server: string; notice: string }>("account-login", (e) => { server = e.payload.server; adding = false; showLogin(e.payload.notice); }),
      listen<string>("switch-error", (e) => { notice = e.payload; }),
      listen<{ action: string; text: string | null }>("hotkey", (e) => hotkey(e.payload.action, e.payload.text)),
      listen("dial-request", takeDialRequest),
      // Nach dem Standby: Voicemails neu holen (Funktionstasten und
      // Umleitungen hören selbst auf "resumed" bzw. "reach-changed")
      listen("resumed", () => { if (session) { loadVoicemails(); loadConferences(); } }),
    ];
    initPhone();
    takeDialRequest();
    initChat();
    initVoicemail();
    initConferences();
    loadPrefs().catch(() => {});
    restore();
    return () => offs.forEach((p) => p.then((off) => off()));
  });

  /** Tastenkürzel aus GNOME (`starclx --action …`) */
  function hotkey(action: string, text: string | null) {
    const calls = phone.status.calls;
    switch (action) {
      case "dial-selection":
      case "dial-clipboard": {
        const number = (text ?? "").replace(/[^\d+*#]/g, "");
        if (number) run("phone_dial", { number });
        else phone.notice = action === "dial-selection" ? t("Keine Rufnummer markiert") : t("Keine Rufnummer in der Zwischenablage");
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
        tab = tabs[(tabs.findIndex((x) => x.id === tab) + 1) % tabs.length].id;
        break;
    }
  }

  /** Rufnummer aus einem tel:-, callto:- oder sip:-Link; wartet aufs Softphone */
  let dialPending = $state<{ number: string; until: number } | null>(null);

  async function takeDialRequest() {
    const number = await invoke<string | null>("take_dial_request").catch(() => null);
    if (number === null) return;
    if (number) dialPending = { number, until: Date.now() + 60_000 };
    else phone.notice = t("Der Link enthält keine Rufnummer");
  }

  $effect(() => {
    if (!dialPending) return;
    if (canDial()) {
      run("phone_dial", { number: dialPending.number });
      dialPending = null;
    } else if (phone.now > dialPending.until) {
      phone.notice = t("Softphone nicht bereit, {number} nicht gewählt", { number: dialPending.number });
      dialPending = null;
    }
  });

  async function restore() {
    server = (await invoke<string | null>("last_server")) ?? "";
    serverLocked = (await invoke<string[]>("locked_prefs").catch((): string[] => [])).includes("server");
    try {
      const info = await invoke<SessionInfo | null>("restore_session");
      if (info) { session = info; phase = "session"; loadVoicemails(); loadConferences(); return; }
    } catch (e) {
      notice = t("Automatische Anmeldung fehlgeschlagen: {e}", { e: String(e) });
    }
    phase = "login";
    loadAccounts();
  }

  /** Anlage vom System vorgegeben (/etc/xdg/starclxrc) */
  let serverLocked = $state(false);

  /** Anmeldung im Systembrowser statt im eigenen Fenster */
  let viaBrowser = $state(false);

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
      await invoke("start_login", { server, browser: viaBrowser, fresh: adding });
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
  type Tab = "journal" | "voicemail" | "contacts" | "chat" | "fkeys" | "conference" | "doorcam";
  let tab = $state<Tab>("journal");
  const allTabs: { id: Tab; icon: IconName; label: string }[] = $derived([
    { id: "journal", icon: "history", label: t("Rufliste") },
    { id: "voicemail", icon: "voicemail", label: "Voicemail" },
    { id: "contacts", icon: "contacts", label: t("Adressbuch") },
    { id: "chat", icon: "chat", label: "Chat" },
    { id: "fkeys", icon: "dialpad", label: t("Funktionstasten") },
    { id: "conference", icon: "meetings", label: t("Konferenzen") },
    { id: "doorcam", icon: "videocam", label: t("Türkamera") },
  ]);
  /** Türkamera nur mit angelegten Kameras anbieten */
  const hasDoorCams = $derived(!!prefs.value?.door_cams?.some((c) => c.url.trim()));
  const tabs = $derived(allTabs.filter((x) => x.id !== "doorcam" || hasDoorCams));
  $effect(() => {
    if (tab === "doorcam" && !hasDoorCams) tab = "journal";
  });

  const meta = $derived(Object.fromEntries(allTabs.map((x) => [x.id, x])));
  const free = $derived(prefs.value?.workspace === "free");
  let tiles = $state<Tile[]>([]);
  /** Zähler, die sonst am Reiter stehen, für die Kachel-Titelleiste */
  const badge = (id: string) => (id === "chat" ? unreadTotal() : id === "voicemail" ? unheard() : id === "conference" ? activeConferences() : 0);

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
    const tile = tiles.find((x) => x.id === id);
    if (tile) tile.visible = !tile.visible;
  }

  async function logout() {
    menuOpen = false;
    settingsOpen = false;
    try {
      await invoke("logout");
      notice = t("Abgemeldet");
    } catch (e) {
      notice = t("Abmelden unvollständig: {e}", { e: String(e) });
    }
    showLogin(notice);
  }

  function initials(name: string) {
    return name.split(/\s+/).filter(Boolean).slice(0, 2).map((w) => w[0]).join("").toUpperCase() || "?";
  }

  const stateText = $derived({
    off: t("Softphone aus"),
    starting: t("Softphone startet …"),
    ready: t("Softphone bereit"),
    error: t("Softphone nicht angemeldet"),
  });
</script>

{#if phase === "session" && session}
  <div class="shell">
    <header class="top">
      <div class="menuwrap">
        <button class="me" title="{session.display_name} · {stateText[phone.status.state]}" onclick={() => (menuOpen = !menuOpen)} aria-expanded={menuOpen}>
          {#if session.user_id && avatarOf(session.user_id)}<img class="pic" src={avatarOf(session.user_id)} alt="" />{:else}{initials(session.display_name)}{/if}
          <span class="reg {phone.status.state}"></span>
          <span class="mychat"><ChatBubble state={ownChat(session.user_id).availability} /></span>
        </button>
        {#if menuOpen}
          <button class="scrim" aria-label={t("Menü schliessen")} onclick={() => (menuOpen = false)}></button>
          <MeMenu {session} onsettings={() => { menuOpen = false; settingsOpen = true; }} onlogout={logout} onswitch={switchAccount} onadd={addAccount} />
        {/if}
      </div>
      <DialSearch />
      <div class="spacer"></div>
      {#if free && !editing}
        <button class="arrange" title={t("Anordnung bearbeiten")} onclick={editStart}><Icon name="edit" size={20} /></button>
      {/if}
      <CallManager />
      <div class="brand"><img src={logo} alt="" /> StarCLX</div>
    </header>

    {#if !free || editing}
    <nav class="tabs">
      {#each tabs as tb}
        <button
          class="tab"
          class:active={free ? tiles.find((x) => x.id === tb.id)?.visible : tab === tb.id}
          title={free && editing ? t("Kachel ein- oder ausblenden") : undefined}
          onclick={() => tabClick(tb.id)}
        >
          <Icon name={tb.icon} size={20} /><span>{tb.label}</span>
          {#if tb.id === "chat" && unreadTotal()}<span class="unread">{unreadTotal()}</span>{/if}
          {#if tb.id === "voicemail" && unheard()}<span class="unread">{unheard()}</span>{/if}
          {#if tb.id === "conference" && activeConferences()}<span class="unread">{activeConferences()}</span>{/if}
        </button>
      {/each}
      {#if free}
        <span class="spacer"></span>
        {#if editing}
          <button class="tab" title={t("Standardanordnung")} onclick={() => (tiles = tilesOf(null))}><span>{t("Standard")}</span></button>
          <button class="tab" title={t("Änderungen verwerfen")} onclick={editCancel}><Icon name="close" size={20} /><span>{t("Abbrechen")}</span></button>
          <button class="tab lock on" title={t("Anordnung speichern und fixieren")} onclick={editDone}><Icon name="check" size={20} /><span>{t("Fertig")}</span></button>
        {/if}
      {/if}
    </nav>
    {/if}

    <main class="work">
      {#if phone.status.state === "error" || (phone.status.state === "off" && phone.status.detail)}
        <p class="banner"><span class="reg {phone.status.state}"></span>{stateText[phone.status.state]}{#if phone.status.detail}: {t(phone.status.detail)}{/if}</p>
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
        {:else if id === "doorcam"}
          <DoorCams />
        {:else if id === "conference"}
          <Conferences />
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
  {#if contactEdit.open}
    {#key contactEdit.id + contactEdit.number}<ContactForm />{/key}
  {/if}
  {#if conferenceEdit.id !== null}
    {#key conferenceEdit.id}<ConferenceForm me={session} />{/key}
  {/if}
  {#if settingsOpen}
    <Settings onclose={() => (settingsOpen = false)} onlogout={logout} server={session.server} userId={session.user_id} displayName={session.display_name} />
  {/if}
{:else}
<main class="login">
  <div class="brand big"><img src={logo} alt="" /> StarCLX</div>
  {#if phase === "restoring"}
    <p>{t("Verbinde …")}</p>
  {:else}
    <h1>{adding ? t("Konto hinzufügen") : t("Anmelden")}</h1>
    {#if accounts.length}
      <div class="accounts">
        <span class="muted">{t("Gespeicherte Konten")}</span>
        {#each accounts as a (a.server + a.user_id)}
          <div class="account">
            <button class="pick" onclick={() => switchAccount(a)}>
              <strong>{a.display_name || accountHost(a)}</strong>
              {#if a.display_name}<small class="muted">{accountHost(a)}</small>{/if}
            </button>
            <button class="drop" title={t("Konto entfernen")} aria-label={t("Konto entfernen")} onclick={() => forgetAccount(a)}><Icon name="close" size={18} /></button>
          </div>
        {/each}
        <span class="muted">{t("Oder an einer Anlage anmelden:")}</span>
      </div>
    {/if}
    <form onsubmit={login}>
      <input placeholder={t("https://anlage.example.com")} bind:value={server} readonly={serverLocked} required />
      <!-- Bleibt klickbar: schliesst der Benutzer das Anmeldefenster, kann er neu beginnen. -->
      <button class="primary" type="submit" onclick={() => (viaBrowser = false)}>
        {phase === "waiting" ? t("Warte auf Anmeldung …") : t("Anmelden")}
      </button>
      <button type="submit" class="link" onclick={() => (viaBrowser = true)}>
        {t("Stattdessen im Browser anmelden")}
      </button>
    </form>
    {#if untrusted}
      <div class="cert" role="alertdialog" aria-labelledby="cert-title">
        <h2 id="cert-title">{t("Zertifikat nicht vertrauenswürdig")}</h2>
        <p>
          {t("Das Zertifikat von")} <b>{untrusted.host}:{untrusted.port}</b> {t("ist nicht von einer bekannten Stelle ausgestellt oder passt nicht zur Adresse ({reason}). Das ist bei lokalen Anlagen mit selbstsigniertem Zertifikat üblich.", { reason: untrusted.reason })}
        </p>
        <p class="muted">{t("Fingerabdruck (SHA-256), mit dem Zertifikat der Anlage vergleichen:")}</p>
        <code>{untrusted.fingerprint}</code>
        <div class="actions">
          <button onclick={() => (untrusted = null)}>{t("Abbrechen")}</button>
          <button class="primary" onclick={trustCertificate}>{t("Vertrauen und anmelden")}</button>
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
  /* Wie in der STARFACE-App: Status-Sprechblase oben rechts; der Punkt unten rechts zeigt das Softphone */
  .me .reg { position: absolute; right: -1px; bottom: -1px; width: 0.6rem; height: 0.6rem; border: 2px solid var(--bar); }
  .me .pic { width: 100%; height: 100%; border-radius: 50%; object-fit: cover; }
  .me .mychat { position: absolute; right: -0.4rem; top: -0.25rem; width: 1.2rem; height: 1.1rem; }
  .reg.ready { background: var(--green); }
  .reg.starting { background: var(--accent); }
  .reg.error { background: var(--red); }
  .spacer { flex: 1; }
  .brand { font-weight: 700; letter-spacing: 0.12em; color: var(--accent); white-space: nowrap; }
  .brand { display: flex; align-items: center; gap: 0.4rem; letter-spacing: 0.06em; }
  .brand img { width: 1.6em; height: 1.6em; }
  .brand.big { font-size: 1.6rem; margin-bottom: 1.5rem; }

  .accounts { display: flex; flex-direction: column; gap: 0.4rem; margin-bottom: 0.8rem; }
  .accounts .muted { color: var(--muted); font-size: 0.85rem; }
  .account { display: flex; gap: 0.3rem; }
  .account .pick { flex: 1; min-width: 0; display: flex; flex-direction: column; align-items: flex-start; text-align: left; padding: 0.5rem 0.7rem; }
  .account .pick small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100%; }
  .account .drop { display: grid; place-items: center; padding: 0 0.5rem; }
  .scrim { position: fixed; inset: 0; z-index: 14; background: transparent; border: none; padding: 0; cursor: default; }
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
  .link { background: none; border: none; color: var(--muted); text-decoration: underline; padding: 0; }
  .cert { margin-top: 1rem; padding: 0.75rem 1rem; border: 1px solid var(--accent); border-radius: 6px; background: var(--panel); }
  .cert h2 { margin: 0 0 0.5rem; font-size: 1rem; }
  .cert code { display: block; font-size: 0.8rem; word-break: break-all; }
  .cert .actions { display: flex; justify-content: flex-end; gap: 0.5rem; margin-top: 0.75rem; }
</style>
