<script lang="ts">
  // Arbeitsbereich „Funktionstasten“: Tasten der Anlage zum Auslösen.
  import { onMount } from "svelte";
  import FkeyTile from "./FkeyTile.svelte";
  import { fkeys, keyAt, loadFkeys, press } from "./fkeys.svelte";
  import { prefs } from "./prefs.svelte";
  import { t } from "./i18n.svelte";

  onMount(() => { loadFkeys(); });
  const columns = $derived(prefs.value?.fkey_columns ?? 3);
</script>

<div class="fk">
  {#if fkeys.error}<p class="error">{t(fkeys.error)}</p>{/if}
  {#if fkeys.notice}<p class="error">{fkeys.notice}</p>{/if}
  <div class="grid" style="grid-template-columns: repeat({columns}, minmax(0, 1fr))">
    {#each fkeys.order as _, i}
      {@const k = keyAt(i)}
      {#if k}<FkeyTile key={k} onclick={() => press(k)} />{:else}<div class="gap"></div>{/if}
    {/each}
  </div>
  {#if fkeys.loaded && !fkeys.keys.length}
    <p class="muted">{t("Noch keine Funktionstasten. Anlegen lassen sie sich unter Einstellungen → Funktionstasten oder auf der Anlage.")}</p>
  {/if}
</div>

<style>
  .fk { height: 100%; overflow: auto; display: flex; flex-direction: column; gap: 0.5rem; }
  .grid { display: grid; gap: 0.5rem; max-width: 60rem; }
  .gap { min-height: 3.2rem; }
  .muted { color: var(--muted); }
  .error { color: var(--accent); margin: 0; }
</style>
