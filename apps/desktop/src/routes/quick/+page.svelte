<script lang="ts">
  // Schnellwahl aus dem Tray: Rufnummernfeld mit Adressbuchsuche und nur die
  // Besetztlampenfelder. Klick ruft an bzw. holt einen klingelnden Anruf heran.
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import DialSearch from "$lib/DialSearch.svelte";
  import FkeyTile from "$lib/FkeyTile.svelte";
  import { fkeys, loadFkeys, press } from "$lib/fkeys.svelte";
  import { initPhone, phone } from "$lib/phone.svelte";
  import { loadPrefs } from "$lib/prefs.svelte";

  const blfs = $derived(fkeys.keys.filter((k) => k.functionKeyType === "BUSYLAMPFIELD"));

  onMount(() => {
    initPhone();
    loadPrefs().catch(() => {});
    loadFkeys();
    const reload = () => document.visibilityState === "visible" && loadFkeys();
    document.addEventListener("visibilitychange", reload);
    return () => document.removeEventListener("visibilitychange", reload);
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") invoke("quick_hide");
  }
</script>

<svelte:window {onkeydown} />

<main class="quick">
  <DialSearch />
  {#if phone.notice}<p class="notice">{phone.notice}</p>{/if}
  {#if fkeys.notice}<p class="notice">{fkeys.notice}</p>{/if}
  <div class="list">
    {#each blfs as k (k.id)}
      <FkeyTile key={k} onclick={() => press(k)} />
    {:else}
      <p class="muted">{fkeys.error || (fkeys.loaded ? "Keine Besetztlampenfelder eingerichtet." : "Lade …")}</p>
    {/each}
  </div>
</main>

<style>
  :global(body) { margin: 0; background: var(--bg); }
  .quick { height: 100vh; box-sizing: border-box; padding: 0.7rem; display: flex; flex-direction: column; gap: 0.6rem; }
  .list { flex: 1; overflow: auto; display: flex; flex-direction: column; gap: 0.4rem; }
  .muted { color: var(--muted); }
  .notice { color: var(--accent); margin: 0; font-size: 0.85rem; }
</style>
