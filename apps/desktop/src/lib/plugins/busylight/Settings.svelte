<script lang="ts">
  // Plugin Busylight: Einstellungen und Test. Das Licht steuert Rust
  // (src-tauri/src/plugins/busylight).
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import Icon from "../../Icon.svelte";
  import Toggle from "../../Toggle.svelte";
  import type { Prefs } from "../../prefs.svelte";
  import { t } from "../../i18n.svelte";

  let { draft = $bindable() }: { draft: Prefs } = $props();

  let busylight = $state<{ devices: string[]; error: string | null; tones: string[] }>({ devices: [], error: null, tones: [] });
  let blTesting = $state(false);

  onMount(() => {
    invoke<typeof busylight>("busylight_info").then((b) => (busylight = b), () => {});
  });

  async function testBusylight() {
    blTesting = true;
    try {
      await invoke("busylight_test", { sound: draft.busylight_sound, volume: draft.busylight_volume });
    } finally {
      blTesting = false;
    }
  }
</script>

<section id="busylight">
  <h3>Busylight</h3>
  <div class="card">
    <Toggle bind:checked={draft.busylight} label={t("Kuando Busylight verwenden")} />
    <p class="small muted">{t("Grün: frei · Rot: im Gespräch · Rot blinkend: eingehender Anruf")}</p>
    <p class="muted">
      {#if busylight.devices.length}
        {t("Angeschlossen:")} {busylight.devices.length === 1 ? t("1 Gerät") : t("{n} Geräte", { n: busylight.devices.length })}
      {:else}
        {t("Kein Busylight angeschlossen.")}
      {/if}
    </p>
    {#if busylight.error}<p class="notice">{busylight.error}</p>{/if}
    <div class="bl" class:off={!draft.busylight}>
      <label>
        <span>{t("Ton bei Anruf")}</span>
        <select bind:value={draft.busylight_sound}>
          <option value="">{t("Kein Ton")}</option>
          {#each busylight.tones as tone}<option value={tone}>{tone}</option>{/each}
        </select>
      </label>
      <label>
        <span>{t("Lautstärke")}</span>
        <input type="range" min="0" max="100" step="5" bind:value={draft.busylight_volume} disabled={!draft.busylight_sound} />
        <span class="vol">{draft.busylight_volume} %</span>
      </label>
      <button class="play" onclick={testBusylight} disabled={blTesting || !busylight.devices.length}>
        <Icon name="light" size={18} /> {blTesting ? t("Teste …") : t("Testen")}
      </button>
    </div>
  </div>
</section>

<style>
  section { padding-top: 0.8rem; }
  h3 { font-size: 1.05rem; margin: 0.6rem 0 0.7rem; }
  .card { background: var(--panel); border-radius: 4px; padding: 0.7rem 1rem; display: flex; flex-direction: column; gap: 0.2rem; }
  .small { font-size: 0.85rem; margin: -0.3rem 0 0.6rem; }
  .muted { color: var(--muted); margin: 0.3rem 0; }
  .notice { color: var(--accent-text); margin: 0; flex: 1; }
  .play { align-self: flex-start; display: flex; align-items: center; gap: 0.4rem; }
  .bl { display: flex; flex-direction: column; gap: 0.6rem; margin-top: 0.4rem; }
  .bl.off { opacity: 0.5; }
  .bl label { display: grid; grid-template-columns: 8rem minmax(0, 16rem) auto; align-items: center; gap: 0.8rem; }
  .bl select { padding: 0.3rem; background: var(--panel-2); color: inherit; border: 1px solid var(--line); border-radius: 4px; }
  .bl input[type="range"] { accent-color: var(--accent); }
  .vol { color: var(--muted); font-size: 0.85rem; }
</style>
