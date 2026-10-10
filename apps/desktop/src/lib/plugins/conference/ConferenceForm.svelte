<script lang="ts">
  // Konferenz anlegen oder bearbeiten: Name, Termin, Wiederholung und
  // Teilnehmer (aus dem Adressbuch oder frei eingegeben).
  import { invoke } from "@tauri-apps/api/core";
  import { untrack } from "svelte";
  import Icon from "../../Icon.svelte";
  import type { Contact } from "../contacts/contacts";
  import { conferenceEdit, conferences, loadConferences, type Participant, type Recurrence } from "./conference.svelte";
  import { t } from "../../i18n.svelte";

  let { me }: { me: { user_id: string; display_name: string } } = $props();

  const owner = untrack(() => me);
  const isNew = !conferenceEdit.id;
  const existing = conferences.list.find((c) => c.id === conferenceEdit.id);

  /** Nächste volle Stunde */
  function nextHour() {
    const d = new Date();
    d.setHours(d.getHours() + 1, 0, 0, 0);
    return d.getTime();
  }
  /** Für <input type="datetime-local">: Ortszeit ohne Zeitzone */
  function local(ms: number) {
    const d = new Date(ms - new Date(ms).getTimezoneOffset() * 60_000);
    return d.toISOString().slice(0, 16);
  }

  let name = $state(existing?.name ?? "");
  let start = $state(local(existing?.start ?? nextHour()));
  let recurrence = $state<Recurrence>(existing?.recurrence ?? "once");
  // Wer anlegt, moderiert und wird beim Start angerufen
  let participants = $state<Participant[]>(
    existing ? structuredClone($state.snapshot(existing.participants)) : [
      { id: "", user_id: owner.user_id, name: owner.display_name, number: "", email: "", moderator: true, call_on_start: true },
    ],
  );
  let busy = $state(false);
  let error = $state("");

  const repeats: { id: Recurrence; label: string }[] = $derived([
    { id: "once", label: t("Einmalig") },
    { id: "daily", label: t("Täglich") },
    { id: "weekly", label: t("Wöchentlich") },
    { id: "monthly", label: t("Monatlich") },
  ]);

  // Teilnehmer suchen wie im Wählfeld
  let term = $state("");
  let hits = $state<Contact[]>([]);
  let seq = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  function search() {
    clearTimeout(timer);
    const q = term.trim();
    if (q.length < 2) return void (hits = []);
    const my = ++seq;
    timer = setTimeout(async () => {
      try {
        const r = await invoke<Contact[]>("contacts_search", { term: q });
        if (my === seq) hits = r;
      } catch {
        if (my === seq) hits = [];
      }
    }, 250);
  }

  function add(p: Omit<Participant, "id" | "moderator" | "call_on_start">) {
    if (p.user_id && participants.some((x) => x.user_id === p.user_id)) return;
    participants.push({ ...p, id: "", moderator: false, call_on_start: !!p.user_id || !!p.number });
    term = "";
    hits = [];
  }

  function addContact(c: Contact, number: string) {
    add({ user_id: c.user_id, name: c.name, number, email: c.email });
  }

  /** Eingabe ohne Treffer: als Nummer oder E-Mail-Adresse übernehmen */
  function addTyped() {
    const v = term.trim();
    if (!v) return;
    if (v.includes("@")) add({ user_id: null, name: v, number: "", email: v });
    else add({ user_id: null, name: v, number: v, email: "" });
  }

  const named = $derived(!!name.trim() && !!start);

  function close() {
    conferenceEdit.id = null;
  }

  async function save() {
    busy = true;
    error = "";
    try {
      await invoke("conference_save", {
        draft: {
          id: existing?.id ?? null,
          name,
          start: new Date(start).getTime(),
          recurrence,
          participants: $state.snapshot(participants),
        },
      });
      await loadConferences();
      close();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && close()} />
<div class="scrim" role="presentation" onclick={close}></div>
<div class="dialog" role="dialog" aria-label={isNew ? t("Neue Konferenz") : t("Konferenz bearbeiten")}>
  <h4>{isNew ? t("Neue Konferenz") : t("Konferenz bearbeiten")}</h4>
  <div class="body">
    <label class="row"><span>{t("Name")}</span><input type="text" bind:value={name} /></label>
    <label class="row"><span>{t("Beginn")}</span><input type="datetime-local" bind:value={start} /></label>
    <label class="row"><span>{t("Wiederholung")}</span>
      <select bind:value={recurrence}>
        {#each repeats as r}<option value={r.id}>{r.label}</option>{/each}
      </select>
    </label>

    <h5>{t("Teilnehmer")}</h5>
    <div class="people">
      {#each participants as p, i}
        <div class="person">
          <div class="who">
            <input type="text" bind:value={p.name} placeholder={t("Name")} />
            {#if !p.user_id}
              <div class="contact">
                <input type="text" bind:value={p.number} placeholder={t("Nummer")} />
                <input type="email" bind:value={p.email} placeholder={t("E-Mail")} />
              </div>
            {/if}
          </div>
          <label class="flag" title={t("Darf die Konferenz steuern")}><input type="checkbox" bind:checked={p.moderator} />{t("Moderator")}</label>
          <label class="flag" title={t("Die Anlage ruft beim Start an")}><input type="checkbox" bind:checked={p.call_on_start} disabled={!p.user_id && !p.number.trim()} />{t("Anrufen")}</label>
          <button class="icon" title={t("Entfernen")} onclick={() => participants.splice(i, 1)}><Icon name="close" size={18} /></button>
        </div>
      {/each}
    </div>
    <form class="add" onsubmit={(e) => { e.preventDefault(); if (hits[0]?.numbers[0]) addContact(hits[0], hits[0].numbers[0].number); else addTyped(); }}>
      <input type="text" bind:value={term} oninput={search} placeholder={t("Teilnehmer suchen oder Nummer/E-Mail eingeben")} />
      {#if hits.length}
        <div class="hits">
          {#each hits as c}
            {#each c.numbers.length ? c.numbers : [{ label: "", number: "" }] as n}
              <button type="button" class="hit" onclick={() => addContact(c, n.number)}>
                <strong>{c.name}</strong><small>{[n.label, n.number].filter(Boolean).join(" ")}</small>
              </button>
            {/each}
          {/each}
        </div>
      {/if}
    </form>
  </div>
  {#if error}<p class="notice">{error}</p>{/if}
  <div class="actions">
    <span class="spacer"></span>
    <button onclick={close}>{t("Abbrechen")}</button>
    <button class="primary" disabled={busy || !named} onclick={save}>{isNew ? t("Anlegen") : t("Speichern")}</button>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: #0007; z-index: 40; }
  .dialog {
    position: fixed; z-index: 41; left: 50%; top: 50%; transform: translate(-50%, -50%); width: min(38rem, 94vw); max-height: 88vh;
    background: var(--panel); border: 1px solid var(--line); border-radius: 8px; padding: 1rem; display: flex; flex-direction: column; gap: 0.6rem;
  }
  .dialog h4 { margin: 0; }
  .body { overflow: auto; display: flex; flex-direction: column; gap: 0.45rem; padding-right: 0.3rem; }
  h5 { margin: 0.5rem 0 0; font-size: 0.8rem; color: var(--muted); font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; }
  .row { display: grid; grid-template-columns: 8rem minmax(0, 1fr); align-items: center; gap: 0.6rem; }
  .row span { color: var(--muted); font-size: 0.9rem; }
  .people { display: flex; flex-direction: column; }
  .person { display: flex; align-items: center; gap: 0.5rem; padding: 0.3rem 0; border-top: 1px solid var(--line); }
  .who { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 0.25rem; }
  .contact { display: flex; gap: 0.25rem; }
  .contact input { flex: 1; min-width: 0; }
  .flag { display: flex; align-items: center; gap: 0.2rem; font-size: 0.85rem; color: var(--muted); white-space: nowrap; }
  .add { position: relative; }
  .add input { width: 100%; box-sizing: border-box; }
  .hits { display: flex; flex-direction: column; border: 1px solid var(--line); border-radius: 4px; margin-top: 0.2rem; max-height: 12rem; overflow: auto; }
  .hit { display: flex; justify-content: space-between; gap: 0.6rem; background: none; border: none; border-radius: 0; text-align: left; }
  .hit:hover { background: var(--line); }
  .hit small { color: var(--muted); }
  .icon { width: 2rem; height: 2rem; padding: 0; display: grid; place-items: center; background: none; border: 1px solid transparent; border-radius: 50%; color: var(--muted); }
  .icon:hover { border-color: var(--line); color: var(--text); }
  .actions { display: flex; gap: 0.6rem; margin-top: 0.2rem; }
  .spacer { flex: 1; }
  .primary { background: var(--accent); border-color: var(--accent); color: var(--on-accent); font-weight: 600; }
  .notice { color: var(--accent); margin: 0; }
</style>
