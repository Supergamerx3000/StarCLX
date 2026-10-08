<script lang="ts">
  // Bild einer Türkamera. Die Bilder holt das Backend (plugins/doorcam) und
  // schickt sie über einen Kanal; beim Entfernen der Ansicht hört es auf.
  import { Channel, invoke } from "@tauri-apps/api/core";
  import { t } from "../../i18n.svelte";

  /** Kamera der anrufenden Türsprechstelle (`callId`) oder angelegte Kamera (`url`) */
  let { callId = null, url = null }: { callId?: string | null; url?: string | null } = $props();

  type DoorCamEvent = { kind: "frame"; data: string } | { kind: "error"; message: string };

  let src = $state("");
  let error = $state("");

  $effect(() => {
    const args = { callId, url };
    let id: number | null = null;
    let gone = false;
    src = "";
    error = "";
    const channel = new Channel<DoorCamEvent>();
    channel.onmessage = (m) => {
      if (gone) return;
      if (m.kind === "frame") {
        src = `data:image/jpeg;base64,${m.data}`;
        error = "";
      } else {
        error = m.message;
      }
    };
    invoke<number>("doorcam_watch", { ...args, channel })
      .then((n) => (gone ? invoke("doorcam_stop", { id: n }) : (id = n)))
      .catch((e) => (error = String(e)));
    return () => {
      gone = true;
      if (id !== null) invoke("doorcam_stop", { id }).catch(() => {});
    };
  });
</script>

<div class="cam">
  {#if src}<img {src} alt={t("Türkamera")} />{/if}
  {#if !src || error}
    <p class:error={!!error}>{error || t("Kamera wird verbunden …")}</p>
  {/if}
</div>

<style>
  /* Feste Fläche (16:9, im Reiter bzw. in der Kachel ausgefüllt); das Bild
     passt sich ein, statt die Fläche mit seiner eigenen Grösse aufzublähen */
  .cam {
    position: relative; display: grid; place-items: center; aspect-ratio: 16 / 9; min-height: 6rem;
    background: #000; border-radius: 6px; overflow: hidden;
  }
  img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain; }
  p {
    position: relative; margin: 0; padding: 0.4rem 0.7rem; font-size: 0.85rem; color: #ddd; text-align: center;
  }
  /* Fehler über dem letzten Bild einblenden */
  img + p { position: absolute; left: 0; right: 0; bottom: 0; background: #000b; }
  .error { color: #ff8a80; }
</style>
