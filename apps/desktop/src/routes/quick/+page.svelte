<script lang="ts">
  // Schnellwahl aus dem Tray: Rufnummernfeld mit Adressbuchsuche und nur die
  // Besetztlampenfelder. Klick ruft an bzw. holt einen klingelnden Anruf heran.
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import DialSearch from "$lib/DialSearch.svelte";
  import FkeyTile from "$lib/plugins/fkeys/FkeyTile.svelte";
  import { fkeys, loadFkeys, press, resetFkeys } from "$lib/plugins/fkeys/fkeys.svelte";
  import { initPhone, phone } from "$lib/plugins/call/phone.svelte";
  import { loadPrefs } from "$lib/prefs.svelte";
  import { t } from "$lib/i18n.svelte";

  const blfs = $derived(fkeys.keys.filter((k) => k.functionKeyType === "BUSYLAMPFIELD"));

  onMount(() => {
    initPhone();
    loadPrefs().catch(() => {});
    loadFkeys();
    // Beim Einblenden neu laden; so gilt auch eine inzwischen geänderte Sprache.
    const reload = () => {
      if (document.visibilityState !== "visible") return;
      loadFkeys();
      loadPrefs().catch(() => {});
    };
    document.addEventListener("visibilitychange", reload);
    // Kontowechsel: Tasten des alten Kontos weg, die des neuen laden
    const offs = [listen("switching", resetFkeys), listen("logged-out", resetFkeys), listen("session", () => loadFkeys())];
    return () => {
      document.removeEventListener("visibilitychange", reload);
      offs.forEach((p) => p.then((off) => off()));
    };
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
      <p class="muted">{fkeys.error || (fkeys.loaded ? t("Keine Besetztlampenfelder eingerichtet.") : t("Lade …"))}</p>
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
