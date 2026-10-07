<script lang="ts">
  import { portal } from "../../portal";
  // Anruf mit Notiz an einen Kollegen weitergeben: per Chat direkt an einen
  // STARFACE-Benutzer oder per E-Mail über das Mailprogramm.
  import { invoke } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { untrack } from "svelte";
  import { chat } from "../chat/chat.svelte";
  import { t } from "../../i18n.svelte";

  let { text: initial, subject, onclose }: { text: string; subject: string; onclose: () => void } = $props();

  let text = $state(untrack(() => initial));
  let peer = $state("");
  let filter = $state("");
  let busy = $state(false);
  let error = $state("");

  const people = $derived(
    chat.status.contacts
      .filter((c) => !filter || c.name.toLowerCase().includes(filter.toLowerCase()))
      .sort((a, b) => a.name.localeCompare(b.name)),
  );

  async function sendChat() {
    busy = true;
    error = "";
    try {
      await invoke("chat_send", { peer, body: text });
      onclose();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function sendMail() {
    error = "";
    try {
      await openUrl(`mailto:?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(text)}`);
      onclose();
    } catch (e) {
      error = t("Mailprogramm nicht geöffnet: {e}", { e: String(e) });
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<div class="layer" use:portal>
<div class="scrim" role="presentation" onclick={onclose}></div>
<div class="dialog" role="dialog" aria-label={t("Anruf weitergeben")}>
  <h4>{t("Anruf weitergeben")}</h4>
  <textarea bind:value={text} rows="6"></textarea>
  <h5>{t("Per Chat an")}</h5>
  {#if chat.status.online}
    <input type="text" bind:value={filter} placeholder={t("Kollegen suchen")} />
    <div class="people">
      {#each people as c (c.jid)}
        <button class:active={peer === c.jid} aria-pressed={peer === c.jid} onclick={() => (peer = c.jid)}>{peer === c.jid ? "✓ " : ""}{c.name}</button>
      {:else}
        <p class="muted">{t("Keine Kollegen gefunden.")}</p>
      {/each}
    </div>
  {:else}
    <p class="muted">{t("Chat ist nicht verbunden.")}</p>
  {/if}
  {#if error}<p class="notice">{error}</p>{/if}
  <div class="actions">
    <button onclick={sendMail}>{t("Per E-Mail …")}</button>
    <span class="spacer"></span>
    <button onclick={onclose}>{t("Abbrechen")}</button>
    <button class="primary" disabled={busy || !peer || !text.trim()} onclick={sendChat}>{t("Per Chat senden")}</button>
  </div>
</div>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: #0007; z-index: 40; }
  .dialog {
    position: fixed; z-index: 41; left: 50%; top: 50%; transform: translate(-50%, -50%); width: min(30rem, 92vw); max-height: 88vh;
    background: var(--panel); border: 1px solid var(--line); border-radius: 8px; padding: 1rem; display: flex; flex-direction: column; gap: 0.5rem;
  }
  .dialog h4 { margin: 0; }
  h5 { margin: 0.4rem 0 0; font-size: 0.8rem; color: var(--muted); font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; }
  textarea { resize: vertical; font: inherit; padding: 0.5rem; }
  .people { display: flex; flex-wrap: wrap; gap: 0.3rem; max-height: 9rem; overflow: auto; }
  .people button { padding: 0.25rem 0.7rem; border-radius: 999px; font-size: 0.9rem; }
  .people button.active, .people button.active:hover { background: var(--accent); border-color: var(--accent); color: #111; }
  .actions { display: flex; gap: 0.6rem; margin-top: 0.3rem; }
  .spacer { flex: 1; }
  .primary { background: var(--accent); border-color: var(--accent); color: #111; font-weight: 600; }
  .muted { color: var(--muted); margin: 0; }
  .notice { color: var(--accent); margin: 0; }
</style>
