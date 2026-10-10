<script lang="ts">
  import { connection } from "../../connection.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import Icon from "../../Icon.svelte";
  import { phone, run, canDial } from "../call/phone.svelte";
  import { loadVoicemails, voicemail, type Voicemail } from "./voicemail.svelte";
  import { locale, t } from "../../i18n.svelte";

  type Folder = Voicemail["folder"];
  const folders: { id: Folder; label: string }[] = $derived([
    { id: "inbox", label: t("Neu") },
    { id: "old", label: t("Alt") },
    { id: "private", label: t("Privat") },
  ]);

  let folder = $state<Folder>("inbox");
  let notice = $state("");
  let playing = $state<string | null>(null);
  let loading = $state<string | null>(null);
  let confirmDelete = $state<string | null>(null);
  let audio: HTMLAudioElement | null = null;
  let url = "";

  const ready = $derived(canDial());
  const shown = $derived(voicemail.list.filter((v) => v.folder === folder));
  const count = (f: Folder) => voicemail.list.filter((v) => v.folder === f).length;

  onMount(() => {
    loadVoicemails();
    return stop;
  });

  function stop() {
    audio?.pause();
    audio = null;
    if (url) URL.revokeObjectURL(url);
    url = "";
    playing = null;
  }

  async function act(cmd: string, args: Record<string, unknown>) {
    notice = "";
    try {
      await invoke(cmd, args);
      await loadVoicemails();
      return true;
    } catch (e) {
      notice = String(e);
      return false;
    }
  }

  async function play(v: Voicemail) {
    if (playing === v.id) return stop();
    stop();
    loading = v.id;
    notice = "";
    try {
      const data = await invoke<ArrayBuffer>("voicemail_audio", { id: v.id });
      url = URL.createObjectURL(new Blob([data], { type: "audio/wav" }));
      audio = new Audio(url);
      audio.onended = stop;
      await audio.play();
      playing = v.id;
      // Die Anlage merkt sich das Abhören nicht selbst
      if (v.folder === "inbox") act("voicemail_move", { id: v.id, folder: "old" });
    } catch (e) {
      notice = t("Abspielen fehlgeschlagen: {e}", { e: String(e) });
      stop();
    } finally {
      loading = null;
    }
  }

  async function save(v: Voicemail) {
    const d = new Date(v.start);
    const stamp = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}_${String(d.getHours()).padStart(2, "0")}${String(d.getMinutes()).padStart(2, "0")}`;
    const name = `Voicemail_${(v.name || v.number || t("Unbekannt")).replace(/[^\p{L}\d+-]+/gu, "_")}_${stamp}`;
    notice = "";
    try {
      if (await invoke<boolean>("voicemail_save", { id: v.id, name })) notice = t("Gespeichert.");
    } catch (e) {
      notice = String(e);
    }
  }

  function when(ms: number) {
    const d = new Date(ms);
    const today = new Date();
    const time = d.toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });
    return d.toDateString() === today.toDateString() ? time : `${d.toLocaleDateString(locale())} ${time}`;
  }

  const dur = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
</script>

<div class="vm">
  <div class="bar">
    {#each folders as f}
      <button class="chip" class:active={folder === f.id} onclick={() => (folder = f.id)}>
        {count(f.id) ? `${f.label} (${count(f.id)})` : f.label}
      </button>
    {/each}
  </div>
  {#if voicemail.error && connection.online}<p class="error">{voicemail.error}</p>{/if}
  {#if notice && connection.online}<p class="error">{notice}</p>{/if}
  <div class="list">
    {#each shown as v (v.id)}
      <div class="row" class:new={v.folder === "inbox"}>
        <button class="playbtn" title={playing === v.id ? t("Stopp") : t("Abspielen")} disabled={loading === v.id} onclick={() => play(v)}>
          <Icon name={playing === v.id ? "pause" : "play"} size={20} />
        </button>
        <div class="who">
          <strong>{v.name || v.number || t("Unbekannt")}</strong>
          <small>{[v.name && v.number, v.mailbox, v.group && t("Gruppe")].filter(Boolean).join(" · ")}</small>
        </div>
        <span class="when">{when(v.start)}<small>{dur(v.duration_secs)}</small></span>
        <div class="acts">
          <button class="icon" title={t("Über das Softphone abhören")} disabled={!ready} onclick={() => act("voicemail_via_phone", { id: v.id })}><Icon name="headset" size={18} /></button>
          <button class="icon" title={t("Als Datei speichern")} onclick={() => save(v)}><Icon name="down" size={18} /></button>
          {#if v.folder === "inbox"}
            <button class="icon" title={t("Als gehört markieren")} onclick={() => act("voicemail_move", { id: v.id, folder: "old" })}>✓</button>
          {:else}
            <button class="icon" title={t("Als neu markieren")} onclick={() => act("voicemail_move", { id: v.id, folder: "inbox" })}>●</button>
          {/if}
          {#if v.folder !== "private"}
            <button class="icon" title={t("Nach Privat verschieben")} onclick={() => act("voicemail_move", { id: v.id, folder: "private" })}><Icon name="person" size={18} /></button>
          {/if}
          <button
            class="icon"
            class:warn={confirmDelete === v.id}
            title={confirmDelete === v.id ? t("Nochmals klicken zum Löschen") : t("Löschen")}
            onclick={() => (confirmDelete === v.id ? act("voicemail_delete", { id: v.id }) : (confirmDelete = v.id))}
            onblur={() => confirmDelete === v.id && (confirmDelete = null)}
          ><Icon name="trash" size={18} /></button>
          {#if v.number}
            <button class="call" title={t("Zurückrufen")} disabled={!ready} onclick={() => run("phone_dial", { number: v.number })}><Icon name="call" size={18} /></button>
          {/if}
        </div>
      </div>
    {:else}
      <p class="muted">{t("Keine Nachrichten in „{ordner}“.", { ordner: folders.find((f) => f.id === folder)?.label ?? "" })}</p>
    {/each}
  </div>
</div>

<style>
  .vm { display: flex; flex-direction: column; height: 100%; min-height: 0; gap: 0.5rem; }
  .bar { display: flex; align-items: center; gap: 0.3rem; flex-wrap: wrap; }
  .chip { padding: 0.3rem 0.8rem; border-radius: 999px; font-size: 0.9rem; background: var(--panel); }
  .chip.active { background: var(--accent); color: var(--on-accent); border-color: var(--accent); }
  .list { flex: 1; overflow: auto; border-radius: 4px; padding: 0.2rem 0.8rem 0.8rem; }
  .row { display: flex; align-items: center; gap: 0.8rem; padding: 0.45rem 0.3rem; border-top: 1px solid var(--line); }
  .row.new .who strong { color: var(--accent); }
  .playbtn { width: 2.3rem; height: 2.3rem; padding: 0; border-radius: 50%; display: grid; place-items: center; }
  .who { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .who small { color: var(--muted); }
  .when { display: flex; flex-direction: column; align-items: flex-end; font-variant-numeric: tabular-nums; }
  .when small { color: var(--muted); font-size: 0.75rem; }
  .acts { display: flex; align-items: center; gap: 0.25rem; }
  .icon { width: 2rem; height: 2rem; padding: 0; display: grid; place-items: center; background: none; border: 1px solid transparent; border-radius: 50%; color: var(--muted); }
  .icon:hover:not(:disabled) { border-color: var(--line); color: var(--text); }
  .icon:disabled { opacity: 0.4; }
  .icon.warn { color: #fff; background: var(--red); }
  .call { width: 2.1rem; height: 2.1rem; padding: 0; border-radius: 50%; display: grid; place-items: center; background: var(--green); border: none; color: #fff; }
  .call:disabled { opacity: 0.4; }
  .muted { color: var(--muted); }
  .error { color: var(--accent); margin: 0; }
</style>
