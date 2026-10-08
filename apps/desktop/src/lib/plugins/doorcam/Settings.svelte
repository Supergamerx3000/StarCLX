<script lang="ts">
  // Plugin Türkamera: Kameras mit Name und URL in den Einstellungen.
  import DoorCamView from "./DoorCamView.svelte";
  import Icon from "../../Icon.svelte";
  import type { Prefs } from "../../prefs.svelte";
  import { t } from "../../i18n.svelte";

  let { draft = $bindable() }: { draft: Prefs } = $props();

  /** Kamera in der Vorschau, mit der URL beim Klick (nicht bei jedem Tastendruck neu verbinden) */
  let preview = $state<{ index: number; url: string } | null>(null);

  function add() {
    draft.door_cams = [...draft.door_cams, { name: "", url: "" }];
  }

  function remove(i: number) {
    draft.door_cams = draft.door_cams.filter((_, j) => j !== i);
    preview = null;
  }

  function togglePreview(i: number) {
    const url = draft.door_cams[i].url.trim();
    preview = preview?.index === i && preview.url === url ? null : { index: i, url };
  }
</script>

<section id="doorcams">
  <h3>{t("Türkameras")}</h3>
  <div class="card">
    <p class="small muted">{t("Ruft eine Türsprechstelle mit Kamera an, zeigt der Call Manager ihr Bild automatisch und bietet „Tür öffnen“ an. Kamera und Türöffner-Code kommen aus der Anlage. Hier legst du zusätzlich Kameras an, die du jederzeit als Kachel „Türkamera“ siehst.")}</p>
    {#each draft.door_cams as cam, i}
      <div class="rule">
        <div class="rulehead">
          <input type="text" class="name" bind:value={cam.name} placeholder={t("Name, z. B. Haupteingang")} />
          <button onclick={() => togglePreview(i)} disabled={!cam.url.trim()}>{t("Vorschau")}</button>
          <button class="x" title={t("Entfernen")} onclick={() => remove(i)}><Icon name="trash" size={18} /></button>
        </div>
        <input type="text" class="target" bind:value={cam.url} placeholder={t("rtsp://kamera/live oder https://benutzer:passwort@kamera/video.mjpg")} />
        {#if preview?.index === i}
          {#key preview.url}<div class="preview"><DoorCamView url={preview.url} /></div>{/key}
        {/if}
      </div>
    {/each}
    <button class="add" onclick={add}>{t("Kamera hinzufügen")}</button>
    <p class="small muted hint">{t("Möglich sind RTSP-Ströme mit H.264 (URL beginnt mit rtsp://), Motion JPEG (URL endet auf mjpg oder enthält motionjpeg, mjpg, stream= oder fps=) und Einzelbilder, die laufend neu geladen werden. Zugangsdaten gehören in die URL (https://benutzer:passwort@kamera/…). Für RTSP muss ffmpeg installiert sein.")}</p>
  </div>
</section>

<style>
  section { padding-top: 0.8rem; }
  h3 { font-size: 1.05rem; margin: 0.6rem 0 0.7rem; }
  .card { background: var(--panel); border-radius: 4px; padding: 0.7rem 1rem; display: flex; flex-direction: column; gap: 0.2rem; }
  .small { font-size: 0.85rem; }
  .muted { color: var(--muted); margin: 0.3rem 0; }
  .x { background: none; border: none; padding: 0.2rem; color: var(--muted); display: grid; }
  .add { align-self: flex-start; margin-top: 0.7rem; }
  .rule { display: flex; flex-direction: column; gap: 0.4rem; padding: 0.5rem 0; border-bottom: 1px solid var(--line); }
  .rulehead { display: flex; flex-wrap: wrap; align-items: center; gap: 0.6rem; }
  .rule input[type="text"] { padding: 0.3rem 0.5rem; background: var(--panel-2); color: inherit; border: 1px solid var(--line); border-radius: 4px; }
  .rule .name { flex: 1; min-width: 12rem; }
  .rule .target { width: 100%; box-sizing: border-box; }
  .preview { max-width: 32rem; }
  .hint { margin-top: 0.6rem; }
</style>
