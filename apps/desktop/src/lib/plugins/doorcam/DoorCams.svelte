<script lang="ts">
  // Kachel bzw. Reiter „Türkamera“: zeigt eine der angelegten Kameras.
  import DoorCamView from "./DoorCamView.svelte";
  import { prefs } from "../../prefs.svelte";
  import { t } from "../../i18n.svelte";

  const cams = $derived((prefs.value?.door_cams ?? []).filter((c) => c.url.trim()));
  let selected = $state(0);
  const index = $derived(Math.min(selected, Math.max(cams.length - 1, 0)));
  const cam = $derived(cams[index]);
</script>

<div class="wrap">
  {#if cams.length > 1}
    <nav>
      {#each cams as c, i}
        <button class:active={i === index} onclick={() => (selected = i)}>{c.name || t("Kamera {n}", { n: String(i + 1) })}</button>
      {/each}
    </nav>
  {/if}
  {#if cam}
    {#key cam.url}<div class="view"><DoorCamView url={cam.url} /></div>{/key}
  {:else}
    <p class="empty">{t("Noch keine Türkamera angelegt. Kameras fügst du in den Einstellungen unter Telefonie → Türkameras hinzu.")}</p>
  {/if}
</div>

<style>
  .wrap { flex: 1; display: flex; flex-direction: column; gap: 0.5rem; height: 100%; min-height: 0; }
  nav { display: flex; flex-wrap: wrap; gap: 0.4rem; }
  nav button { border-radius: 999px; padding: 0.25rem 0.8rem; }
  nav button.active { background: var(--accent); color: var(--on-accent); border-color: var(--accent); }
  .view { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .view :global(.cam) { flex: 1; min-height: 0; aspect-ratio: auto; }
  .empty { color: var(--muted); margin: 0.5rem; }
</style>
