<script lang="ts">
  // Menü am Profilbild, aufgebaut wie in der STARFACE-App: Bild und Name,
  // darunter Ruhe, Chat-Status, primäres Telefon, signalisierte Rufnummer
  // und die eigenen Umleitungen (Immer); Untermenüs öffnen sich daneben.
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import ChatBubble from "./ChatBubble.svelte";
  import Icon from "./Icon.svelte";
  import OwnStatus from "./OwnStatus.svelte";
  import { avatarOf, fkeys, loadFkeys, loadSignaling, ownChat, type Redirect, type SignalingNumber } from "./plugins/fkeys/fkeys.svelte";
  import { phone } from "./plugins/call/phone.svelte";
  import { prefs } from "./prefs.svelte";
  import { t } from "./i18n.svelte";

  type Session = { server: string; server_version: string; display_name: string; user_id: string };
  type PhoneView = { id: string; name: string; primary: boolean };

  let { session, onsettings, onlogout }: { session: Session; onsettings: () => void; onlogout: () => void } = $props();

  let sub = $state<"" | "chat" | "phone" | "number" | "redirect">("");
  let phones = $state<PhoneView[]>([]);
  let numbers = $state<SignalingNumber[]>([]);
  let redirects = $state<Redirect[]>([]);
  let notice = $state("");
  let license = $state<string | null>(null);

  const avatar = $derived(session.user_id ? avatarOf(session.user_id) : null);
  const initials = $derived(session.display_name.split(/\s+/).filter(Boolean).slice(0, 2).map((w) => w[0]).join("").toUpperCase() || "?");
  const dnd = $derived(fkeys.presence[session.user_id]?.dnd ?? false);
  // Eigener Status von der Anlage, auch wenn ihn ein anderer Client gesetzt hat
  const own = $derived(ownChat(session.user_id));
  const chat = $derived(own.availability);
  const chatLabel = $derived(own.text || ({ available: t("Verfügbar"), away: t("Abwesend"), dnd: t("Bitte nicht stören") } as Record<string, string>)[chat]);
  // Lizenztyp aus der REST-API; unbekannte Werte so, wie die Anlage sie liefert
  const licenseLabel = $derived(license ? ({ USER: "User", USERLIGHT: "User Light" } as Record<string, string>)[license] ?? license : "");
  const primary = $derived(phones.find((p) => p.primary));
  const signaling = $derived(numbers.find((n) => n.selected));
  const always = $derived(redirects.filter((r) => r.kind === "always" && !r.group));
  const activeRedirects = $derived(always.filter((r) => r.enabled).length);
  const stateText = $derived({
    off: t("Softphone aus"),
    starting: t("Softphone startet …"),
    ready: t("Softphone bereit"),
    error: t("Softphone nicht angemeldet"),
  } as Record<string, string>);

  async function run(f: () => Promise<unknown>) {
    try {
      await f();
      notice = "";
    } catch (e) {
      notice = String(e);
    }
  }
  const loadPhones = () => run(async () => (phones = await invoke<PhoneView[]>("phones")));
  const loadNumbers = () => run(async () => (numbers = await invoke<SignalingNumber[]>("signaling_numbers")));
  const loadRedirects = () => run(async () => (redirects = await invoke<Redirect[]>("redirects")));

  onMount(() => {
    // Ruhe kommt aus der Präsenz; die läuft erst, wenn die Tasten geladen sind.
    if (!fkeys.loaded) loadFkeys();
    loadPhones();
    loadNumbers();
    loadRedirects();
    // Ohne Lizenzangabe bleibt die Zeile einfach weg
    invoke<string | null>("account_license").then((l) => (license = l)).catch(() => {});
    // Änderungen aus anderen Clients nachziehen, solange das Menü offen ist
    const off = [
      listen("me-signaling", () => loadNumbers()),
      listen("me-phones", () => loadPhones()),
      listen("reach-changed", () => loadRedirects()),
    ];
    return () => off.forEach((p) => p.then((f) => f()));
  });

  // Untermenü auf Höhe der angeklickten Zeile, wie in der STARFACE-App
  let subTop = $state(0);
  const toggle = (name: typeof sub, e: MouseEvent) => {
    subTop = (e.currentTarget as HTMLElement).offsetTop;
    sub = sub === name ? "" : name;
  };
  const trustSipCert = () =>
    run(() => invoke("phone_trust_sip_certificate", { fingerprint: phone.status.sip_certificate }));
  const setDnd = () => run(() => invoke("fkey_dnd", { enabled: !dnd }));
  const setPrimary = (id: string) => run(async () => { await invoke("set_primary_phone", { id }); await loadPhones(); });
  const setNumber = (id: string) => run(async () => { await invoke("set_signaling_number", { id }); await loadNumbers(); await loadSignaling(); });
  const setRedirect = (r: Redirect) => run(async () => { await invoke("redirect_enable", { id: r.id, enabled: !r.enabled }); await loadRedirects(); });
  const setAll = (enabled: boolean) =>
    run(async () => {
      for (const r of always.filter((r) => r.enabled !== enabled)) await invoke("redirect_enable", { id: r.id, enabled });
      await loadRedirects();
    });
  // Telefone ohne das technische „SIP/“ davor, wie in der STARFACE-App
  const phoneName = (p: PhoneView) => p.name.replace(/^SIP\//, "");
  const numberLabel = (n: SignalingNumber) => (n.suppressed ? t("Rufnummer unterdrücken") : n.number);
</script>

<div class="menu">
  <div class="head">
    <span class="big">
      {#if avatar}<img src={avatar} alt="" />{:else}{initials}{/if}
      <span class="bub"><ChatBubble state={chat} /></span>
      {#if dnd}<span class="dndb" title={t("Bitte nicht stören")}><Icon name="dnd" size={22} /></span>{/if}
      {#if activeRedirects}<span class="rdb" title={t("Umleitung aktiv")}><Icon name="redirect" size={15} /></span>{/if}
    </span>
    <strong>{session.display_name}</strong>
    <span class="muted">STARFACE {session.server_version} · {session.server.replace(/^https?:\/\//, "")}</span>
    {#if licenseLabel}<span class="muted">{t("Lizenz")}: {licenseLabel}</span>{/if}
    <span class="muted state"><span class="reg {phone.status.state}"></span>{stateText[phone.status.state]}</span>
    {#if phone.status.state === "error" && phone.status.detail}<span class="notice">{t(phone.status.detail)}</span>{/if}
    {#if phone.status.state === "error" && phone.status.sip_certificate}
      <code class="fp">{phone.status.sip_certificate}</code>
      <button class="certok" onclick={trustSipCert}>{t("SIP-Zertifikat bestätigen")}</button>
    {/if}
  </div>

  <button class="row" class:on={dnd} onclick={setDnd}>
    <span class="ic dnd"><Icon name="dnd" size={22} /></span>
    <span class="lbl">{t("Bitte nicht stören")}</span>
    {#if dnd}<span class="pill">{t("an")}</span>{/if}
  </button>
  <button class="row" class:open={sub === "chat"} onclick={(e) => toggle("chat", e)}>
    <span class="ic"><span class="bubble"><ChatBubble state={chat} /></span></span>
    <span class="lbl">{chatLabel}</span><span class="more"><Icon name="chevron" size={20} /></span>
  </button>
  <button class="row" class:open={sub === "phone"} onclick={(e) => toggle("phone", e)}>
    <span class="ic"><Icon name="headset" size={20} /></span>
    <span class="lbl">{primary ? phoneName(primary) : t("Primäres Telefon")}</span><span class="more"><Icon name="chevron" size={20} /></span>
  </button>
  <button class="row" class:open={sub === "number"} onclick={(e) => toggle("number", e)}>
    <span class="ic"><Icon name="eye" size={20} /></span>
    <span class="lbl">{signaling ? numberLabel(signaling) : t("Rufnummer signalisieren")}</span><span class="more"><Icon name="chevron" size={20} /></span>
  </button>
  <button class="row" class:open={sub === "redirect"} onclick={(e) => toggle("redirect", e)}>
    <span class="ic"><Icon name="redirect" size={20} /></span>
    <span class="lbl">{t("Umleitungen")}</span>
    {#if activeRedirects}<span class="count">{activeRedirects}</span>{/if}
    <span class="more"><Icon name="chevron" size={20} /></span>
  </button>
  {#if notice}<span class="notice">{notice}</span>{/if}

  <hr />
  <button class="row plain" onclick={onsettings}><span class="ic"><Icon name="settings" size={20} /></span><span class="lbl">{t("Einstellungen")}</span></button>
  <button class="row plain" onclick={onlogout}><span class="ic"><Icon name="logout" size={20} /></span><span class="lbl">{t("Abmelden")}</span></button>

  {#if sub}
    <div class="sub" style="top: {subTop}px">
      {#if sub === "chat"}
        <OwnStatus userId={session.user_id} />
      {:else if sub === "phone"}
        <span class="title">{t("Primäres Telefon auswählen")}</span>
        {#each phones as p (p.id)}
          <button class="opt" class:sel={p.primary} onclick={() => setPrimary(p.id)}>{phoneName(p)}{#if p.primary}<span class="tick"><Icon name="check" size={16} /></span>{/if}</button>
        {:else}<span class="muted pad">{t("Keine Telefone")}</span>{/each}
        <span class="muted pad">{t("Beim Wählen ruft die Anlage zuerst dieses Telefon an. Annehmen geht nur am Softphone.")}</span>
      {:else if sub === "number"}
        <span class="title">{t("Rufnummer signalisieren")}</span>
        {#each numbers as n (n.id)}
          <button class="opt" class:sel={n.selected} disabled={n.read_only} onclick={() => setNumber(n.id)}>
            {#if n.group}<span class="grp"><Icon name="groups" size={16} /> {n.group}:</span>{/if}{numberLabel(n)}{#if n.selected}<span class="tick"><Icon name="check" size={16} /></span>{/if}</button>
        {/each}
      {:else if sub === "redirect"}
        <span class="title">{t("Umleitung: Immer")}</span>
        {#if always.length}
          <button class="all" onclick={() => setAll(!always.every((r) => r.enabled))}>
            {always.every((r) => r.enabled) ? t("Alle deaktivieren") : t("Alle aktivieren")}
          </button>
        {/if}
        {#each always as r (r.id)}
          <button class="opt" class:sel={r.enabled} onclick={() => setRedirect(r)}>
            <span class="rd"><span>{r.called_number}</span><small class="muted">→ {r.target.number || r.target.mailbox || t("kein Ziel")}</small></span>
            <span class="switch" class:on={r.enabled}></span>
          </button>
        {:else}<span class="muted pad">{t("Keine Umleitungen")}</span>{/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .menu {
    position: absolute; left: 0; top: calc(100% + 0.4rem); z-index: 15; width: 19rem;
    background: var(--panel); border: 1px solid var(--line); border-radius: 8px; box-shadow: 0 8px 24px #000a;
    display: flex; flex-direction: column; gap: 0.3rem; padding: 0.6rem;
  }
  .head { display: flex; flex-direction: column; align-items: center; gap: 0.15rem; padding: 0.4rem 0 0.7rem; text-align: center; }
  .big {
    position: relative; width: 4.6rem; height: 4.6rem; border-radius: 50%; margin-bottom: 0.4rem;
    display: grid; place-items: center; font-size: 1.4rem; font-weight: 600; color: #fff;
    background: linear-gradient(135deg, #3a7bd5, #00a37a); border: 3px solid var(--green);
  }
  .big img { width: 100%; height: 100%; border-radius: 50%; object-fit: cover; }
  .bub { position: absolute; right: -0.4rem; top: -0.2rem; width: 1.6rem; height: 1.45rem; }
  .dndb { position: absolute; left: -0.3rem; top: -0.2rem; display: grid; color: var(--red); background: #fff; border-radius: 50%; }
  .rdb { position: absolute; left: -0.2rem; bottom: 0; width: 1.45rem; height: 1.45rem; border-radius: 50%; display: grid; place-items: center; background: #111; color: #fff; border: 2px solid #fff; }
  .count { min-width: 1.2rem; height: 1.2rem; padding: 0 0.3rem; border-radius: 999px; display: grid; place-items: center; font-size: 0.75rem; font-weight: 700; background: var(--accent); color: #111; }
  .grp { display: inline-flex; align-items: center; gap: 0.25rem; font-weight: 600; margin-right: 0.3rem; }
  .head strong { font-size: 1.05rem; }
  .muted { color: var(--muted); font-size: 0.82rem; }
  .state { display: flex; align-items: center; gap: 0.35rem; }
  .reg { display: inline-block; width: 0.6rem; height: 0.6rem; border-radius: 50%; background: #777; }
  .reg.ready { background: var(--green); }
  .reg.starting { background: var(--accent); }
  .reg.error { background: var(--red); }
  .row {
    display: flex; align-items: center; gap: 0.7rem; text-align: left; padding: 0.55rem 0.6rem;
    background: var(--panel-2); border: 1px solid var(--line); border-radius: 5px;
  }
  .row:hover, .row.open { border-color: var(--accent); }
  .row.plain { background: none; border-color: transparent; }
  .row.plain:hover { background: var(--panel-2); }
  .ic { width: 1.4rem; display: grid; place-items: center; flex: none; color: var(--muted); }
  .ic.dnd { color: #9aa0a6; }
  .row.on .ic.dnd { color: var(--red); }
  .bubble { width: 1.2rem; height: 1.1rem; }
  .lbl { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
  .more { display: grid; transform: rotate(-90deg); color: var(--muted); }
  .pill { font-size: 0.72rem; font-weight: 700; padding: 0.05rem 0.45rem; border-radius: 999px; background: var(--red); color: #fff; }
  hr { border: none; border-top: 1px solid var(--line); margin: 0.3rem 0 0; width: 100%; }
  .notice { color: var(--accent); font-size: 0.85rem; padding: 0 0.3rem; }
  .fp { font-size: 0.72rem; word-break: break-all; padding: 0 0.3rem; color: var(--muted); }
  .certok { align-self: center; }

  .sub {
    position: absolute; left: calc(100% + 0.4rem); width: 18rem;
    background: var(--panel); border: 1px solid var(--line); border-radius: 8px; box-shadow: 0 8px 24px #000a;
    display: flex; flex-direction: column; padding: 0.5rem;
  }
  .title { font-weight: 600; padding: 0.3rem 0.6rem 0.5rem; }
  .opt { display: flex; align-items: center; gap: 0.6rem; background: none; border: none; text-align: left; padding: 0.5rem 0.6rem; border-radius: 4px; }
  .opt:hover:not(:disabled) { background: var(--panel-2); }
  .opt.sel { font-weight: 600; }
  .tick { margin-left: auto; color: var(--accent); display: grid; }
  .pad { padding: 0.4rem 0.6rem; }
  .all { margin: 0 0.4rem 0.5rem; padding: 0.45rem; font-weight: 600; }
  .rd { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .rd small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .switch { position: relative; width: 2.1rem; height: 1.15rem; border-radius: 999px; background: var(--line); flex: none; }
  .switch::after { content: ""; position: absolute; left: 0.15rem; top: 0.15rem; width: 0.85rem; height: 0.85rem; border-radius: 50%; background: #fff; transition: left 0.15s; }
  .switch.on { background: var(--green); }
  .switch.on::after { left: 1.1rem; }
</style>
