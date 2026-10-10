<script lang="ts">
  import { connection } from "../../connection.svelte";
  // Arbeitsbereich „Funktionstasten“: Tasten der Anlage zum Auslösen.
  import { onMount } from "svelte";
  import FkeyTile from "./FkeyTile.svelte";
  import { fkeys, keyAt, keyTitle, loadFkeys, press, programRedirect, typeInfo } from "./fkeys.svelte";
  import { portal } from "../../portal";
  import { prefs } from "../../prefs.svelte";
  import { t } from "../../i18n.svelte";

  onMount(() => { loadFkeys(); });
  const columns = $derived(prefs.value?.fkey_columns ?? 3);
  // Freie Plätze aus der Anordnung zählen nicht: die Tasten stehen dicht in
  // ihrer Reihenfolge. Abstand schafft nur eine „Leere Taste“. Tasten nur für
  // Tischtelefone (Telefonmenü, DTMF …) zeigt der Client wie die STARFACE-App
  // nicht.
  const shown = $derived(
    fkeys.order
      .map((_, i) => keyAt(i))
      .filter((k) => k !== undefined)
      .filter((k) => typeInfo(k.functionKeyType).group !== "desk" || k.functionKeyType === "SEPARATOR"),
  );
</script>

<div class="fk">
  {#if fkeys.error && connection.online}<p class="error">{t(fkeys.error)}</p>{/if}
  {#if fkeys.notice && connection.online}<p class="error">{fkeys.notice}</p>{/if}
  <div class="grid" style="grid-template-columns: repeat({columns}, minmax(0, 1fr))">
    {#each shown as k (k.id)}
      <FkeyTile key={k} onclick={() => press(k)} />
    {/each}
  </div>
  {#if fkeys.loaded && !fkeys.keys.length}
    <p class="muted">{t("Noch keine Funktionstasten. Anlegen lassen sie sich unter Einstellungen → Funktionstasten oder auf der Anlage.")}</p>
  {/if}
</div>

{#if fkeys.program}
  {@const p = fkeys.program}
  <div class="layer" use:portal>
    <div class="scrim" role="presentation" onclick={() => (fkeys.program = null)}></div>
    <div class="dialog" role="dialog" aria-label={keyTitle(p.key)}><form onsubmit={(e) => { e.preventDefault(); programRedirect(); }}>
      <h4>{keyTitle(p.key)}</h4>
      <!-- svelte-ignore a11y_autofocus -->
      <label class="row"><span>{t("Zielrufnummer")}</span><input type="text" bind:value={p.number} autofocus /></label>
      {#if p.timeout !== null}
        <label class="row"><span>{t("Umleiten nach")}</span><span><input type="number" min="1" max="300" bind:value={p.timeout} /> s</span></label>
      {/if}
      <p class="small muted">{t("Beim Ausschalten gelten wieder die vorherigen Einstellungen der Umleitung.")}</p>
      <div class="actions">
        <span class="spacer"></span>
        <button type="button" onclick={() => (fkeys.program = null)}>{t("Abbrechen")}</button>
        <button type="submit" class="primary">{t("Einschalten")}</button>
      </div>
    </form></div>
  </div>
{/if}

<style>
  .fk { height: 100%; overflow: auto; display: flex; flex-direction: column; gap: 0.5rem; }
  .grid { display: grid; gap: 0.5rem; max-width: 60rem; }
  .muted { color: var(--muted); }
  .error { color: var(--accent-text); margin: 0; }
  .small { font-size: 0.85rem; margin: 0; }
  .scrim { position: fixed; inset: 0; background: #0007; z-index: 30; }
  .dialog {
    position: fixed; z-index: 31; left: 50%; top: 50%; transform: translate(-50%, -50%); width: min(26rem, 92vw);
    background: var(--panel); border: 1px solid var(--line); border-radius: 8px; padding: 1rem; display: flex; flex-direction: column; gap: 0.6rem;
  }
  form { display: contents; }
  .dialog h4 { margin: 0 0 0.2rem; }
  .row { display: grid; grid-template-columns: 7.5rem minmax(0, 1fr); align-items: center; gap: 0.6rem; }
  input { padding: 0.3rem 0.5rem; background: var(--panel-2); color: inherit; border: 1px solid var(--line); border-radius: 4px; font: inherit; }
  input[type="number"] { width: 5rem; }
  .actions { display: flex; gap: 0.6rem; margin-top: 0.4rem; }
  .spacer { flex: 1; }
  .primary { background: var(--accent); border-color: var(--accent); color: var(--on-accent); font-weight: 600; }
</style>
