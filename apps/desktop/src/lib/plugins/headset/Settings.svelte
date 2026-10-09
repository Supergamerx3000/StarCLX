<script lang="ts">
  // Plugin Headset: Einstellungen und Test. Tasten und LEDs steuert Rust
  // (src-tauri/src/plugins/headset).
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import Icon from "../../Icon.svelte";
  import Toggle from "../../Toggle.svelte";
  import type { Prefs } from "../../prefs.svelte";
  import { t } from "../../i18n.svelte";

  let { draft = $bindable() }: { draft: Prefs } = $props();

  let headset = $state<{ devices: string[]; error: string | null }>({ devices: [], error: null });
  let testing = $state(false);

  onMount(() => {
    invoke<typeof headset>("headset_info").then((h) => (headset = h), () => {});
  });

  async function test() {
    testing = true;
    try {
      await invoke("headset_test");
    } finally {
      testing = false;
    }
  }
</script>

<section id="headset">
  <h3>{t("Headset-Tasten")}</h3>
  <div class="card">
    <Toggle bind:checked={draft.headset} label={t("Tasten am Headset verwenden (Jabra, Poly, EPOS)")} />
    <p class="small muted">{t("Gesprächstaste: annehmen und auflegen · Stummtaste: Mikrofon stumm · Das Headset klingelt bei Anrufen und zeigt Gespräch und Stummschaltung an.")}</p>
    <p class="muted">
      {#if headset.devices.length}
        {t("Angeschlossen:")} {headset.devices.join(", ")}
      {:else}
        {t("Kein Headset mit Telefonietasten angeschlossen.")}
      {/if}
    </p>
    {#if headset.error}<p class="notice">{headset.error}</p>{/if}
    <button class="play" onclick={test} disabled={testing || !draft.headset || !headset.devices.length}>
      <Icon name="headset" size={18} /> {testing ? t("Teste …") : t("Klingeln testen")}
    </button>
  </div>
</section>

<style>
  section { padding-top: 0.8rem; }
  h3 { font-size: 1.05rem; margin: 0.6rem 0 0.7rem; }
  .card { background: var(--panel); border-radius: 4px; padding: 0.7rem 1rem; display: flex; flex-direction: column; gap: 0.2rem; }
  .small { font-size: 0.85rem; margin: -0.3rem 0 0.6rem; }
  .muted { color: var(--muted); margin: 0.3rem 0; }
  .notice { color: var(--accent); margin: 0; flex: 1; }
  .play { align-self: flex-start; display: flex; align-items: center; gap: 0.4rem; margin-top: 0.4rem; }
</style>
