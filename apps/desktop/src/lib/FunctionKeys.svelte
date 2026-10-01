<script lang="ts">
  // Arbeitsbereich „Funktionstasten“: Tasten der Anlage zum Auslösen.
  import { onMount } from "svelte";
  import FkeyTile from "./FkeyTile.svelte";
  import { fkeys, loadFkeys, press } from "./fkeys.svelte";
  import { prefs } from "./prefs.svelte";

  onMount(() => { loadFkeys(); });
  const columns = $derived(prefs.value?.fkey_columns ?? 3);
</script>

<div class="fk">
  {#if fkeys.error}<p class="error">{fkeys.error}</p>{/if}
  {#if fkeys.notice}<p class="error">{fkeys.notice}</p>{/if}
  <div class="grid" style="grid-template-columns: repeat({columns}, minmax(0, 1fr))">
    {#each fkeys.keys as k (k.id)}
      <FkeyTile key={k} onclick={() => press(k)} />
    {/each}
  </div>
  {#if fkeys.loaded && !fkeys.keys.length}
    <p class="muted">Noch keine Funktionstasten. Anlegen lassen sie sich unter Einstellungen → Funktionstasten oder auf der Anlage.</p>
  {/if}
</div>

<style>
  .fk { height: 100%; overflow: auto; display: flex; flex-direction: column; gap: 0.5rem; }
  .grid { display: grid; gap: 0.5rem; max-width: 60rem; }
  .muted { color: var(--muted); }
  .error { color: var(--accent); margin: 0; }
</style>
