<script lang="ts">
  import { connection } from "../../connection.svelte";
  // Geplante Konferenzen: Liste mit Termin und Teilnehmern, starten,
  // bearbeiten, löschen. Das Formular liegt in ConferenceForm.
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import Icon from "../../Icon.svelte";
  import { phone } from "../call/phone.svelte";
  import { conferenceEdit, conferences, loadConferences, type Conference } from "./conference.svelte";
  import { locale, t } from "../../i18n.svelte";

  type View = "upcoming" | "done";
  let view = $state<View>("upcoming");
  let notice = $state("");
  let busy = $state<string | null>(null);
  let confirmDelete = $state<string | null>(null);

  const ready = $derived(phone.status.state === "ready");
  const shown = $derived(conferences.list.filter((c) => (c.state === "concluded") === (view === "done")));
  const count = (v: View) => conferences.list.filter((c) => (c.state === "concluded") === (v === "done")).length;

  const repeats: Record<Conference["recurrence"], string> = $derived({
    once: "",
    daily: t("täglich"),
    weekly: t("wöchentlich"),
    monthly: t("monatlich"),
  });

  onMount(() => {
    loadConferences();
  });

  async function act(cmd: string, id: string) {
    notice = "";
    busy = id;
    try {
      await invoke(cmd, { id });
      await loadConferences();
    } catch (e) {
      notice = String(e);
    } finally {
      busy = null;
      confirmDelete = null;
    }
  }

  function when(ms: number) {
    const d = new Date(ms);
    const time = d.toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });
    const day = d.toLocaleDateString(locale(), { weekday: "short", day: "numeric", month: "short", year: "numeric" });
    return `${day}, ${time}`;
  }

  function who(c: Conference) {
    const names = c.participants.map((p) => p.name || p.number || p.email);
    return names.length > 3 ? `${names.slice(0, 3).join(", ")} +${names.length - 3}` : names.join(", ");
  }
</script>

<div class="conf">
  <div class="bar">
    <button class="chip" class:active={view === "upcoming"} onclick={() => (view = "upcoming")}>
      {count("upcoming") ? `${t("Geplant")} (${count("upcoming")})` : t("Geplant")}
    </button>
    <button class="chip" class:active={view === "done"} onclick={() => (view = "done")}>{t("Beendet")}</button>
    <span class="spacer"></span>
    <button class="new" onclick={() => (conferenceEdit.id = "")}>+ {t("Neue Konferenz")}</button>
  </div>
  {#if conferences.error && connection.online}<p class="error">{conferences.error}</p>{/if}
  {#if notice && connection.online}<p class="error">{notice}</p>{/if}
  <div class="list">
    {#each shown as c (c.id)}
      <div class="row" class:live={c.state === "active"}>
        <div class="what">
          <strong>{c.name}</strong>
          <small>
            {#if c.state === "active"}<span class="badge">{t("Läuft")}</span>{/if}
            {when(c.start)}{#if repeats[c.recurrence]} · {repeats[c.recurrence]}{/if}
          </small>
          {#if c.participants.length}<small class="who" title={c.participants.map((p) => p.name || p.number || p.email).join(", ")}>{who(c)}</small>{/if}
        </div>
        <div class="acts">
          {#if c.moderator}
            <button class="icon" title={t("Bearbeiten")} onclick={() => (conferenceEdit.id = c.id)}><Icon name="edit" size={18} /></button>
            <button
              class="icon"
              class:warn={confirmDelete === c.id}
              title={confirmDelete === c.id ? t("Nochmals klicken zum Löschen") : t("Löschen")}
              disabled={busy === c.id}
              onclick={() => (confirmDelete === c.id ? act("conference_delete", c.id) : (confirmDelete = c.id))}
              onblur={() => confirmDelete === c.id && (confirmDelete = null)}
            ><Icon name="trash" size={18} /></button>
            {#if c.state !== "concluded"}
              <button
                class="call"
                title={ready ? (c.state === "active" ? t("Beitreten") : t("Jetzt starten")) : t("Das Softphone ist nicht aktiv.")}
                disabled={!ready || busy === c.id}
                onclick={() => act("conference_start", c.id)}
              ><Icon name="call" size={18} /></button>
            {/if}
          {/if}
        </div>
      </div>
    {:else}
      <p class="muted">{view === "done" ? t("Keine beendeten Konferenzen.") : t("Keine geplanten Konferenzen.")}</p>
    {/each}
  </div>
</div>

<style>
  .conf { display: flex; flex-direction: column; height: 100%; min-height: 0; gap: 0.5rem; }
  .bar { display: flex; align-items: center; gap: 0.3rem; flex-wrap: wrap; }
  .spacer { flex: 1; }
  .chip { padding: 0.3rem 0.8rem; border-radius: 999px; font-size: 0.9rem; background: var(--panel); }
  .chip.active { background: var(--accent); color: #111; border-color: var(--accent); }
  .new { font-size: 0.9rem; }
  .list { flex: 1; overflow: auto; background: var(--panel); border-radius: 4px; padding: 0.2rem 0.8rem 0.8rem; }
  .row { display: flex; align-items: center; gap: 0.8rem; padding: 0.45rem 0.3rem; border-top: 1px solid var(--line); }
  .row.live strong { color: var(--green); }
  .what { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .what small { color: var(--muted); }
  .who { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .badge { background: var(--green); color: #fff; border-radius: 999px; padding: 0 0.45rem; margin-right: 0.3rem; font-size: 0.75rem; }
  .acts { display: flex; align-items: center; gap: 0.25rem; }
  .icon { width: 2rem; height: 2rem; padding: 0; display: grid; place-items: center; background: none; border: 1px solid transparent; border-radius: 50%; color: var(--muted); }
  .icon:hover:not(:disabled) { border-color: var(--line); color: var(--text); }
  .icon:disabled { opacity: 0.4; }
  .icon.warn { color: #fff; background: var(--red); }
  .call { width: 2.1rem; height: 2.1rem; padding: 0; border-radius: 50%; display: grid; place-items: center; background: var(--green); border: none; color: #fff; }
  .call:disabled { opacity: 0.4; }
  .muted { color: var(--muted); }
  .error { color: var(--accent); margin: 0; }
</style>
