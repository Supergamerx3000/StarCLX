<script lang="ts">
  import { connection } from "../../connection.svelte";
  // Voicemail-Ansage, Umleitungen und Parallelruf (iFMC). Alles liegt auf der
  // Anlage und wirkt sofort, unabhängig von „Speichern“.
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import Icon from "../../Icon.svelte";
  import Toggle from "../../Toggle.svelte";
  import { t } from "../../i18n.svelte";

  let { server }: { server: string } = $props();

  type Mailbox = { id: string; name: string };
  type Target = { number: string | null; mailbox: string | null };
  type Redirect = {
    id: string; kind: "always" | "busy" | "timeout" | "other"; called_number: string; group: boolean;
    enabled: boolean; target: Target; mailboxes: Mailbox[]; timeout_secs: number; last_number: string; read_only: boolean;
  };
  type Schedule = { days: number[]; from: string; to: string };
  type Fmc = { id: string; number: string; delay: number; enabled: boolean; confirm: boolean; schedules: Schedule[] };
  /** Bearbeitungsstand einer Umleitung: "mailbox:<id>" oder "number" */
  type Edit = { dest: string; number: string; timeout: number };

  const kinds = $derived({ always: t("Immer"), busy: t("Besetzt"), timeout: t("Zeitüberschreitung"), other: t("Sonstige") });
  const days = $derived([t("Mo"), t("Di"), t("Mi"), t("Do"), t("Fr"), t("Sa"), t("So")]);

  let redirects = $state<Redirect[]>([]);
  let edits = $state<Record<string, Edit>>({});
  let fmc = $state<Fmc[]>([]);
  let mailboxes = $state<Mailbox[]>([]);
  let mailbox = $state("");
  let error = $state("");
  let info = $state("");
  let busy = $state(false);
  let editing = $state<Fmc | null>(null);

  onMount(() => {
    load();
    const off = listen("reach-changed", () => load());
    return () => off.then((f) => f());
  });

  function editOf(r: Redirect): Edit {
    return {
      dest: r.target.mailbox ? `mailbox:${r.target.mailbox}` : "number",
      number: r.target.number ?? r.last_number ?? "",
      timeout: r.timeout_secs || 20,
    };
  }

  async function load() {
    try {
      const [r, f, m] = await Promise.all([
        invoke<Redirect[]>("redirects"),
        invoke<Fmc[]>("fmc_phones"),
        invoke<Mailbox[]>("mailboxes"),
      ]);
      redirects = r;
      edits = Object.fromEntries(r.map((x) => [x.id, editOf(x)]));
      fmc = f;
      mailboxes = m;
      if (!mailboxes.some((b) => b.id === mailbox)) mailbox = mailboxes[0]?.id ?? "";
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  async function act(cmd: string, args: Record<string, unknown>, done = "") {
    busy = true;
    error = info = "";
    try {
      await invoke(cmd, args);
      info = done;
      await load();
      return true;
    } catch (e) {
      error = String(e);
      return false;
    } finally {
      busy = false;
    }
  }

  const groups = $derived.by(() => {
    const map = new Map<string, Redirect[]>();
    for (const r of redirects) {
      const key = r.group ? t("Gruppe {name}", { name: r.called_number }) : r.called_number;
      map.set(key, [...(map.get(key) ?? []), r]);
    }
    return [...map];
  });

  function dirty(r: Redirect) {
    const e = edits[r.id], o = editOf(r);
    return e && (e.dest !== o.dest || (e.dest === "number" && e.number !== (r.target.number ?? "")) || (r.kind === "timeout" && e.timeout !== o.timeout));
  }

  function applyRedirect(r: Redirect) {
    const e = edits[r.id];
    const target = e.dest === "number" ? { number: e.number.trim(), mailbox: null } : { number: null, mailbox: e.dest.slice(8) };
    if (e.dest === "number" && !e.number.trim()) return void (error = t("Bitte eine Zielrufnummer eingeben."));
    act("redirect_update", { id: r.id, target, timeoutSecs: r.kind === "timeout" ? e.timeout : null }, t("Umleitung geändert."));
  }

  function newFmc() {
    editing = { id: "", number: "", delay: 0, enabled: true, confirm: false, schedules: [] };
  }

  async function saveFmc() {
    if (editing && (await act("fmc_save", { phone: $state.snapshot(editing) }, t("Gerät gespeichert.")))) editing = null;
  }

  function toggleDay(s: Schedule, d: number) {
    s.days = s.days.includes(d) ? s.days.filter((x) => x !== d) : [...s.days, d].sort();
  }

  const webApp = () => openUrl(server.replace(/\/+$/, ""));
</script>

<section id="voicemail">
  <h3>Voicemail</h3>
  <div class="card">
    {#if mailboxes.length > 1}
      <label class="row"><span>{t("Voicemail-Box")}</span>
        <select bind:value={mailbox}>{#each mailboxes as m}<option value={m.id}>{m.name}</option>{/each}</select>
      </label>
    {/if}
    <button class="play" disabled={busy || !mailbox} onclick={() => act("mailbox_record", { mailbox }, t("Die Anlage ruft das Softphone an."))}>
      <Icon name="record" size={18} /> {t("Voicemail-Ansage aufnehmen")}
    </button>
    <p class="small muted">
      {t("Die Anlage ruft dich an. Im Menü der Box: 0 = Abwesenheitsansage, 1 = Begrüssung, 3 = Namensansage aufnehmen.")}
      {#if !mailboxes.length}{t("Für diesen Benutzer ist keine Voicemail-Box eingerichtet.")}{/if}
    </p>
    <button class="link" onclick={webApp}>{t("Weitere Einstellungen: zur Web-App wechseln")}</button>
  </div>
</section>

<section id="redirects">
  <h3>{t("Umleitungen")}</h3>
  <div class="card">
    {#if !redirects.length}
      <p class="muted">{error ? "" : t("Keine Umleitungen verfügbar.")}</p>
    {/if}
    {#each groups as [title, list]}
      <h4>{title}</h4>
      {#each list as r (r.id)}
        {@const e = edits[r.id]}
        <div class="redirect" class:locked={r.read_only}>
          <Toggle checked={r.enabled} disabled={busy || r.read_only} label={kinds[r.kind]} onchange={(v: boolean) => act("redirect_enable", { id: r.id, enabled: v })} />
          {#if e}
            <div class="dest">
              <select bind:value={e.dest} disabled={r.read_only}>
                <option value="number">{t("Rufnummer")}</option>
                {#each r.mailboxes as m}<option value="mailbox:{m.id}">Voicemail: {m.name}</option>{/each}
              </select>
              {#if e.dest === "number"}
                <input type="text" bind:value={e.number} placeholder={t("Zielrufnummer")} disabled={r.read_only} />
              {/if}
              {#if r.kind === "timeout"}
                <label class="secs">{t("nach")} <input type="number" min="1" max="300" bind:value={e.timeout} disabled={r.read_only} /> s</label>
              {/if}
              {#if dirty(r)}<button class="primary" disabled={busy} onclick={() => applyRedirect(r)}>{t("Übernehmen")}</button>{/if}
            </div>
          {/if}
          {#if r.read_only}<p class="small muted">{t("Vom Administrator gesperrt.")}</p>{/if}
        </div>
      {/each}
    {/each}
  </div>
</section>

<section id="fmc">
  <h3>{t("Parallelruf (iFMC)")}</h3>
  <div class="card">
    <p class="small muted">{t("Weitere Geräte, z. B. das Handy, klingeln bei Anrufen mit. Call2Go braucht ein aktives Gerät.")}</p>
    {#each fmc as p (p.id)}
      <div class="fmc">
        <Toggle checked={p.enabled} disabled={busy} label={p.number} onchange={(v: boolean) => act("fmc_enable", { id: p.id, enabled: v })} />
        <span class="muted small">{[p.delay ? t("nach {n} s", { n: p.delay }) : t("sofort"), p.confirm && t("mit Tastendruck"), p.schedules.length && t("zeitgesteuert")].filter(Boolean).join(" · ")}</span>
        <button class="x" title={t("Bearbeiten")} onclick={() => (editing = structuredClone($state.snapshot(p)))}><Icon name="settings" size={18} /></button>
        <button class="x" title={t("Löschen")} disabled={busy} onclick={() => confirm(t("{number} entfernen?", { number: p.number })) && act("fmc_delete", { id: p.id })}><Icon name="trash" size={18} /></button>
      </div>
    {/each}
    {#if editing}
      <div class="editor">
        <label class="row"><span>{t("Rufnummer")}</span><input type="text" bind:value={editing.number} placeholder="+41 79 …" /></label>
        <label class="row"><span>{t("Verzögerung")}</span><span><input type="number" min="0" max="60" bind:value={editing.delay} /> s</span></label>
        <Toggle bind:checked={editing.confirm} label={t("Annahme per Tastendruck bestätigen")} />
        <h4>{t("Zeitsteuerung")}</h4>
        {#each editing.schedules as s, i}
          <div class="sched">
            {#each days as d, j}
              <button class="day" class:on={s.days.includes(j + 1)} onclick={() => toggleDay(s, j + 1)}>{d}</button>
            {/each}
            <input type="time" bind:value={s.from} /> – <input type="time" bind:value={s.to} />
            <button class="x" title={t("Entfernen")} onclick={() => editing?.schedules.splice(i, 1)}><Icon name="close" size={18} /></button>
          </div>
        {:else}
          <p class="small muted">{t("Ohne Zeitfenster klingelt das Gerät immer mit.")}</p>
        {/each}
        <button class="add" onclick={() => editing?.schedules.push({ days: [1, 2, 3, 4, 5], from: "08:00", to: "17:00" })}>{t("Zeitfenster hinzufügen")}</button>
        <div class="actions">
          <button class="primary" disabled={busy} onclick={saveFmc}>{editing.id ? t("Speichern") : t("Hinzufügen")}</button>
          <button onclick={() => (editing = null)}>{t("Abbrechen")}</button>
        </div>
      </div>
    {:else}
      <button class="add" onclick={newFmc}>{t("Gerät hinzufügen")}</button>
    {/if}
  </div>
</section>
{#if error && connection.online}<p class="notice">{error}</p>{/if}
{#if info}<p class="ok">{info}</p>{/if}

<style>
  section { padding-top: 0.8rem; }
  h3 { font-size: 1.05rem; margin: 0.6rem 0 0.7rem; }
  h4 { margin: 0.6rem 0 0.3rem; font-size: 0.98rem; }
  .card { background: var(--panel); border-radius: 4px; padding: 0.7rem 1rem; display: flex; flex-direction: column; gap: 0.3rem; }
  .muted { color: var(--muted); margin: 0.3rem 0; }
  .small { font-size: 0.85rem; }
  .notice { color: var(--accent-text); }
  .ok { color: var(--green); }
  .play, .add { align-self: flex-start; display: flex; align-items: center; gap: 0.4rem; }
  .link { align-self: flex-start; background: none; border: none; padding: 0; color: var(--accent-text); text-decoration: underline; }
  .row { display: grid; grid-template-columns: 8rem minmax(0, 16rem); align-items: center; gap: 0.8rem; }
  select, input[type="text"], input[type="number"], input[type="time"] {
    padding: 0.3rem 0.5rem; background: var(--panel-2); color: inherit; border: 1px solid var(--line); border-radius: 4px; font: inherit;
  }
  input[type="number"] { width: 4.5rem; }
  .redirect { padding: 0.3rem 0 0.5rem; border-bottom: 1px solid var(--line); }
  .redirect.locked { opacity: 0.7; }
  .dest { display: flex; flex-wrap: wrap; align-items: center; gap: 0.6rem; margin-left: 3.4rem; }
  .secs { display: flex; align-items: center; gap: 0.3rem; }
  .primary { background: var(--accent); border-color: var(--accent); color: var(--on-accent); font-weight: 600; padding: 0.3rem 0.8rem; }
  .fmc { display: grid; grid-template-columns: auto 1fr auto auto; align-items: center; gap: 0.6rem; border-bottom: 1px solid var(--line); }
  .x { background: none; border: none; padding: 0.2rem; color: var(--muted); display: grid; }
  .editor { display: flex; flex-direction: column; gap: 0.5rem; border: 1px solid var(--line); border-radius: 4px; padding: 0.7rem; margin-top: 0.4rem; }
  .sched { display: flex; flex-wrap: wrap; align-items: center; gap: 0.3rem; }
  .day { padding: 0.2rem 0.45rem; font-size: 0.85rem; }
  .day.on { background: var(--accent); border-color: var(--accent); color: var(--on-accent); }
  .actions { display: flex; gap: 0.6rem; }
</style>
