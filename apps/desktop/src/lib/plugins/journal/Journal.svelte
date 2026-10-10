<script lang="ts">
  import { connection } from "../../connection.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Icon from "../../Icon.svelte";
  import { phone, run, canDial } from "../call/phone.svelte";
  import { contactEdit, newContact } from "../contacts/contactform.svelte";
  import ShareNote from "./ShareNote.svelte";
  import { searchable } from "../../numbers";
  import { locale, t } from "../../i18n.svelte";

  type Entry = {
    id: string;
    name: string;
    number: string;
    incoming: boolean;
    missed: boolean;
    start: number;
    duration_secs: number;
    group: string;
    answered_by: string;
    comment: string;
    called_back: boolean;
    voicemail: boolean;
  };

  let entries = $state<Entry[]>([]);
  let filter = $state<"all" | "missed" | "in" | "out">("all");
  let term = $state("");
  let error = $state("");
  /** Ausgewählter Eintrag, dessen Details aufgeklappt sind */
  let selected = $state<string | null>(null);
  let commentText = $state("");
  // Löschen braucht einen zweiten Klick
  let confirmDelete = $state<string | null>(null);

  const ready = $derived(canDial());

  onMount(() => {
    const offs = [
      listen<Entry[]>("journal", (e) => { entries = e.payload; error = ""; }),
      listen<string>("journal-error", (e) => (error = e.payload)),
    ];
    invoke<Entry[]>("journal_entries").then((e) => { if (e?.length) entries = e; });
    return () => offs.forEach((p) => p.then((off) => off()));
  });

  const shown = $derived(
    entries.map((e) => (e.name || !known[e.number] ? e : { ...e, name: known[e.number] })).filter((e) => {
      if (filter === "missed" && !(e.missed && e.incoming)) return false;
      if (filter === "in" && !e.incoming) return false;
      if (filter === "out" && e.incoming) return false;
      const q = term.trim().toLowerCase();
      return !q || e.name.toLowerCase().includes(q) || e.number.includes(q);
    }),
  );

  // Eigene Spalte für Gruppe, Voicemail und wer angenommen hat
  const hasExtra = $derived(shown.some((e) => e.group || e.voicemail || e.answered_by));

  // Nach Tagen gruppiert
  const days = $derived.by(() => {
    const groups: { label: string; items: Entry[] }[] = [];
    for (const e of shown) {
      const label = dayLabel(e.start);
      if (groups.at(-1)?.label !== label) groups.push({ label, items: [] });
      groups.at(-1)!.items.push(e);
    }
    return groups;
  });

  function dayLabel(ms: number) {
    const d = new Date(ms);
    const today = new Date();
    const yesterday = new Date(Date.now() - 86400000);
    if (d.toDateString() === today.toDateString()) return t("Heute");
    if (d.toDateString() === yesterday.toDateString()) return t("Gestern");
    return d.toLocaleDateString(locale(), { weekday: "long", day: "numeric", month: "long", year: "numeric" });
  }
  const time = (ms: number) => new Date(ms).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });
  function dur(s: number) {
    if (!s) return "";
    const m = Math.floor(s / 60);
    return m ? `${m}:${String(s % 60).padStart(2, "0")} min` : `${s} s`;
  }

  /** Anruf, der gerade weitergegeben wird */
  let sharing = $state<Entry | null>(null);

  function shareText(e: Entry) {
    const who = [e.name, e.number].filter(Boolean).join(", ") || t("Unbekannt");
    const head = e.missed && e.incoming ? t("Verpasster Anruf von {who}", { who })
      : e.incoming ? t("Eingehender Anruf von {who}", { who }) : t("Ausgehender Anruf an {who}", { who });
    const when = new Date(e.start).toLocaleString(locale(), { dateStyle: "medium", timeStyle: "short" });
    const length = e.duration_secs ? t(", Dauer {dauer}", { dauer: dur(e.duration_secs) }) : "";
    return [t("Gesprächsnotiz"), head, t("Zeit: {when}", { when }) + length, e.comment && t("Notiz: {text}", { text: e.comment })]
      .filter(Boolean)
      .join("\n");
  }

  /** Kurze interne Nummern (Parkplatz "00", Kurzwahlen) gehören nicht ins Adressbuch */
  const external = (n: string) => n.replace(/\D/g, "").length >= 5;

  /** Namen aus dem Adressbuch für Nummern, die die Anlage nicht aufgelöst
   *  hat (alte Einträge löst sie nicht nach); "" = nicht gefunden */
  let known = $state<Record<string, string>>({});
  $effect(() => {
    for (const e of entries) {
      if (e.name || !external(e.number) || e.number in known) continue;
      known[e.number] = "";
      // Die Kontaktsuche findet 0041… nicht, die letzten Ziffern schon
      invoke<{ name: string; numbers: { number: string }[] }[]>("contacts_search", { term: searchable(e.number) })
        .then((hits) => {
          const digits = (s: string) => s.replace(/\D/g, "").slice(-9);
          const hit = hits?.find((h) => h.numbers.some((n) => digits(n.number) === digits(e.number)));
          known[e.number] = hit?.name || "";
        })
        .catch(() => {});
    }
  });
  // Nach dem Anlegen eines Kontakts neu prüfen
  let seenChange = contactEdit.changed;
  $effect(() => {
    if (contactEdit.changed !== seenChange) {
      seenChange = contactEdit.changed;
      known = {};
    }
  });

  async function act(action: string, id: string, text?: string) {
    try {
      await invoke("journal_action", { action, id, text });
    } catch (e) {
      error = String(e);
    }
  }

  function select(e: Entry) {
    if (selected === e.id) {
      selected = null;
      return;
    }
    selected = e.id;
    commentText = e.comment;
  }
  async function saveComment(event: Event, id: string) {
    event.preventDefault();
    await act("comment", id, commentText);
  }

  function status(e: Entry) {
    if (e.incoming) return e.missed ? t("Eingehend, verpasst") : t("Eingehend, angenommen");
    return e.missed ? t("Ausgehend, nicht erreicht") : t("Ausgehend, verbunden");
  }
  const fullDate = (ms: number) =>
    new Date(ms).toLocaleString(locale(), { weekday: "short", day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" });

  const filters = $derived([
    { id: "all", label: t("Alle") },
    { id: "missed", label: t("Verpasst") },
    { id: "in", label: t("Eingehend") },
    { id: "out", label: t("Ausgehend") },
  ] as const);
</script>

<div class="journal">
  <div class="bar">
    {#each filters as f}
      <button class="chip" class:active={filter === f.id} onclick={() => (filter = f.id)}>{f.label}</button>
    {/each}
    <label class="filter">
      <Icon name="search" size={18} />
      <input bind:value={term} placeholder={t("Name oder Nummer")} />
    </label>
  </div>
  {#if error && connection.online}<p class="error">{t("Rufliste: {e}", { e: error })}</p>{/if}
  <div class="list" class:extra={hasExtra}>
    {#each days as day (day.label)}
      <h3>{day.label}</h3>
      {#each day.items as e (e.id)}
        <div class="row" class:missed={e.missed && e.incoming} class:selected={selected === e.id}>
          <button class="open" title={t("Details")} aria-expanded={selected === e.id} onclick={() => select(e)}>
          <span class="dir" title={e.missed ? t("Verpasst") : e.incoming ? t("Eingehend") : t("Ausgehend")}>
            <Icon name={e.missed && e.incoming ? "missed" : e.incoming ? "incoming" : "outgoing"} size={20} />
          </span>
          <!-- Wie in der STARFACE-App: oben der Kontakt (sonst „---“), darunter nur die Nummer -->
          <span class="who">
            <strong>{e.name || "---"}</strong>
            <small>{e.number || t("Unbekannt")}</small>
            {#if e.comment && selected !== e.id}<span class="note">📝 {e.comment}</span>{/if}
          </span>
          <span class="time">{time(e.start)}</span>
          <span class="dur">{dur(e.duration_secs)}</span>
          {#if hasExtra}
            <!-- Gruppe als Chip, darunter wer angenommen hat -->
            <span class="extra">
              {#if e.group}<span class="tag" title={t("Gruppe {name}", { name: e.group })}>{e.group}</span>{/if}
              {#if e.voicemail}<span class="tag">Voicemail</span>{/if}
              {#if e.answered_by}<span class="by" title={t("angenommen von {name}", { name: e.answered_by })}><Icon name="person" size={14} />{e.answered_by}</span>{/if}
            </span>
          {/if}
          </button>
          <!-- Eigene Spalte, damit „zurückgerufen“ immer sichtbar ist -->
          <span class="cb">
            {#if e.missed && e.incoming}
              <button
                class="icon"
                class:done={e.called_back}
                title={e.called_back ? t("Als nicht zurückgerufen markieren") : t("Als zurückgerufen markieren")}
                onclick={() => act(e.called_back ? "not_called_back" : "called_back", e.id)}
              >✓</button>
            {/if}
          </span>
          <div class="acts">
            {#if external(e.number) && !e.name && !known[e.number]}
              <button class="icon" title={t("Ins Adressbuch übernehmen")} onclick={() => newContact({ number: e.number })}><Icon name="person" size={18} /></button>
            {/if}
            <button class="icon" title={t("Notiz")} onclick={() => selected !== e.id && select(e)}>✎</button>
            <button class="icon" title={t("Weitergeben (Chat oder E-Mail)")} onclick={() => (sharing = e)}><Icon name="send" size={18} /></button>
            <button
              class="icon"
              class:warn={confirmDelete === e.id}
              title={confirmDelete === e.id ? t("Nochmals klicken zum Löschen") : t("Löschen")}
              onclick={() => (confirmDelete === e.id ? act("delete", e.id) : (confirmDelete = e.id))}
              onblur={() => confirmDelete === e.id && (confirmDelete = null)}
            ><Icon name="trash" size={18} /></button>
            {#if e.number}
              <button class="call" title={t("Anrufen")} disabled={!ready} onclick={() => run("phone_dial", { number: e.number })}><Icon name="call" size={18} /></button>
            {/if}
          </div>
        </div>
        {#if selected === e.id}
          <dl class="details">
            <dt>{t("Anrufende Person")}</dt><dd>{e.name || "---"}</dd>
            <dt>{t("Rufnummer")}</dt><dd>{e.number || "---"}</dd>
            <dt>{t("Anrufstatus")}</dt><dd>{status(e)}{#if e.voicemail} · Voicemail{/if}</dd>
            <dt>{t("Datum, Zeit")}</dt><dd>{fullDate(e.start)}</dd>
            <dt>{t("Dauer")}</dt><dd>{dur(e.duration_secs) || "---"}</dd>
            <dt>{t("Gruppe")}</dt><dd>{e.group || "---"}</dd>
            <dt>{t("Angenommen von")}</dt><dd>{e.answered_by || "---"}</dd>
            <dt>{t("Zurückgerufen")}</dt>
            <dd>
              <label class="check">
                <input type="checkbox" checked={e.called_back} onchange={(ev) => act(ev.currentTarget.checked ? "called_back" : "not_called_back", e.id)} />
                {e.called_back ? t("Ja") : t("Nein")}
              </label>
            </dd>
            <dt>{t("Kommentar")}</dt>
            <dd>
              <form class="comment" onsubmit={(ev) => saveComment(ev, e.id)}>
                <textarea bind:value={commentText} rows="2" placeholder={t("Notiz")}></textarea>
                <button type="submit" disabled={commentText.trim() === e.comment.trim()}>{t("Speichern")}</button>
              </form>
            </dd>
          </dl>
        {/if}
      {/each}
    {:else}
      <p class="muted">{entries.length ? t("Keine passenden Einträge.") : t("Die Rufliste ist leer.")}</p>
    {/each}
  </div>
</div>

{#if sharing}
  <ShareNote text={shareText(sharing)} subject={t("Anruf {who}", { who: sharing.name || sharing.number })} onclose={() => (sharing = null)} />
{/if}

<style>
  .journal { display: flex; flex-direction: column; height: 100%; min-height: 0; gap: 0.5rem; }
  .bar { display: flex; align-items: center; gap: 0.3rem; flex-wrap: wrap; }
  .chip { padding: 0.3rem 0.8rem; border-radius: 999px; font-size: 0.9rem; background: var(--panel); }
  .chip.active { background: var(--accent); color: var(--on-accent); border-color: var(--accent); }
  .filter { margin-left: auto; min-width: 0; display: flex; align-items: center; gap: 0.4rem; padding: 0 0.6rem; background: var(--bar-2); border: 1px solid var(--line); border-radius: 999px; }
  .filter input { border: none; background: none; outline: none; padding: 0.4rem 0; width: 12rem; min-width: 0; text-overflow: ellipsis; }
  .list { flex: 1; overflow: auto; border-radius: 4px; padding: 0.2rem 0.8rem 0.8rem; }
  h3 { font-size: 0.85rem; color: var(--muted); font-weight: 600; margin: 0.9rem 0 0.3rem; text-transform: uppercase; letter-spacing: 0.04em; }
  /* Wie in der STARFACE-App: ruhige Zeilen, Aktionen erst beim Überfahren */
  .row { position: relative; display: flex; align-items: center; gap: 0.4rem; padding: 0.45rem 0.5rem; border: 1px solid transparent; border-radius: 6px; }
  .row + .row { margin-top: 0.15rem; }
  .row:hover { border-color: var(--accent); }
  .row.selected { background: var(--accent-soft); }
  /* Feste Spalten wie eine Tabelle: Richtung | Name | Uhrzeit | Dauer */
  .open {
    flex: 1; min-width: 0; display: grid; grid-template-columns: 1.5rem minmax(0, 1fr) 3.2rem 5.2rem; align-items: center; gap: 0.8rem;
    padding: 0; background: none; border: none; color: inherit; text-align: left; cursor: pointer;
  }
  .dir { display: grid; color: var(--green); }
  .row:not(.missed) .dir { color: var(--muted); }
  .row.missed .dir, .row.missed .who strong { color: #ff6b6b; }
  .who { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .who strong, .who small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .who small { color: var(--muted); }
  .time, .dur { text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .dur { color: var(--muted); font-size: 0.85rem; }
  .cb { flex: none; width: 2rem; display: grid; place-items: center; }
  /* Spalte für Gruppe/Voicemail nur, wenn eine Zeile sie braucht */
  .list.extra .open { grid-template-columns: 1.5rem minmax(0, 1fr) 3.2rem 5.2rem minmax(0, 9rem); }
  .extra { min-width: 0; display: flex; flex-direction: column; align-items: flex-start; gap: 0.15rem; }
  .tag { max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding: 0 0.5rem; border: 1px solid var(--muted); border-radius: 999px; font-size: 0.78rem; }
  .by { max-width: 100%; display: inline-flex; align-items: center; gap: 0.2rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 0.78rem; color: var(--muted); }
  /* Aktionen schweben beim Überfahren rechts über der Zeile, ohne Platz zu belegen */
  .acts {
    position: absolute; right: 0.4rem; top: 50%; transform: translateY(-50%); display: none; align-items: center; gap: 0.25rem;
    padding: 0.15rem 0.25rem; background: var(--panel); border-radius: 999px; box-shadow: 0 2px 8px #0005;
  }
  .row:hover .acts, .row:focus-within .acts, .row.selected .acts { display: flex; }
  .icon { width: 2rem; height: 2rem; padding: 0; display: grid; place-items: center; background: none; border: 1px solid transparent; border-radius: 50%; color: var(--muted); }
  .icon:hover { border-color: var(--line); color: var(--text); }
  .icon.warn { color: #fff; background: var(--red); }
  .icon.done { color: var(--green); border-color: var(--green); }
  /* Anrufen: zurückhaltend, grün erst direkt unter der Maus */
  .call { width: 2.1rem; height: 2.1rem; padding: 0; border-radius: 50%; display: grid; place-items: center; background: var(--panel-2); border: none; color: var(--text); }
  .call:hover:not(:disabled) { background: var(--green); color: #fff; }
  .call:disabled { opacity: 0.4; }
  .note { padding: 0.1rem 0; color: var(--text); font-size: 0.85rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .details { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 0.35rem 1rem; margin: 0; padding: 0.6rem 0.8rem 0.8rem 2.6rem; background: var(--accent-soft); }
  .details dt { color: var(--muted); font-size: 0.9rem; }
  .details dd { margin: 0; overflow-wrap: anywhere; }
  .check { display: inline-flex; align-items: center; gap: 0.4rem; }
  .comment { display: flex; gap: 0.3rem; align-items: flex-start; }
  .comment textarea { flex: 1; padding: 0.3rem 0.5rem; resize: vertical; font: inherit; }
  .comment button { padding: 0.3rem 0.6rem; }
  .muted { color: var(--muted); }
  .error { color: var(--accent-text); margin: 0; }
</style>
