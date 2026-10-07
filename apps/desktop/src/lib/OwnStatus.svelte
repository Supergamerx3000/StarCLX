<script lang="ts">
  // Eigener Chat-Status wie in der STARFACE-App: oben ein personalisierter
  // Status (Symbol und Text), darunter die festen Status und die gespeicherten.
  // Der aktuelle Status kommt von der Anlage, auch wenn ihn ein anderer Client
  // gesetzt hat.
  import { invoke } from "@tauri-apps/api/core";
  import ChatBubble from "./ChatBubble.svelte";
  import Icon from "./Icon.svelte";
  import { ownChat } from "./fkeys.svelte";
  import { prefs } from "./prefs.svelte";
  import { t } from "./i18n.svelte";

  type Preset = { availability: string; text: string };

  let { userId }: { userId: string } = $props();

  const current = $derived(ownChat(userId));
  const presets = $derived(prefs.value?.chat_presets ?? []);
  let icon = $state("available");
  let text = $state("");
  let picking = $state(false);
  let notice = $state("");

  const fixed = $derived([
    { availability: "available", text: "", label: t("Verfügbar") },
    { availability: "away", text: "", label: t("Abwesend") },
    { availability: "dnd", text: "", label: t("Bitte nicht stören") },
  ]);
  const isCurrent = (p: Preset) => current.availability === p.availability && current.text === p.text;

  async function set(availability: string, statusText: string) {
    try {
      const list = await invoke<Preset[]>("chat_set_own", { availability, text: statusText });
      if (prefs.value) {
        prefs.value.chat_availability = availability;
        prefs.value.chat_text = statusText.trim();
        prefs.value.chat_presets = list;
      }
      notice = "";
    } catch (e) {
      notice = String(e);
    }
  }

  async function remove(p: Preset) {
    try {
      const list = await invoke<Preset[]>("chat_delete_preset", { availability: p.availability, text: p.text });
      if (prefs.value) prefs.value.chat_presets = list;
    } catch (e) {
      notice = String(e);
    }
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    if (!text.trim()) return;
    await set(icon, text);
    text = "";
  }
</script>

<div class="own">
  <span class="title">{t("Personalisierten Status festlegen")}</span>
  <form class="custom" onsubmit={submit}>
    <span class="pickwrap">
      <button type="button" class="pick" title={t("Symbol wählen")} onclick={() => (picking = !picking)}>
        <span class="icon"><ChatBubble state={icon} /></span><span class="chev"><Icon name="chevron" size={16} /></span>
      </button>
      {#if picking}
        <span class="icons">
          {#each ["available", "away", "dnd"] as a (a)}
            <button type="button" class="icon" onclick={() => { icon = a; picking = false; }}><ChatBubble state={a} /></button>
          {/each}
        </span>
      {/if}
    </span>
    <input bind:value={text} placeholder={t("Eigener Statustext")} maxlength="120" />
    <button type="submit" class="ok" title={t("Statustext setzen")} disabled={!text.trim()}><Icon name="check" size={18} /></button>
  </form>
  <hr />
  {#each fixed as p (p.availability)}
    <button class="opt" class:sel={isCurrent(p)} onclick={() => set(p.availability, "")}>
      <span class="icon"><ChatBubble state={p.availability} /></span>{p.label}
    </button>
  {/each}
  {#each presets as p (p.availability + p.text)}
    <div class="opt preset" class:sel={isCurrent(p)}>
      <button class="choose" onclick={() => set(p.availability, p.text)}>
        <span class="icon"><ChatBubble state={p.availability} /></span>{p.text}
      </button>
      <button class="del" title={t("Löschen")} onclick={() => remove(p)}><Icon name="trash" size={18} /></button>
    </div>
  {/each}
  {#if notice}<span class="notice">{notice}</span>{/if}
</div>

<style>
  .own { display: flex; flex-direction: column; gap: 0.1rem; }
  .title { font-weight: 600; padding: 0.3rem 0.6rem 0.5rem; }
  .custom { display: flex; gap: 0.4rem; padding: 0 0.6rem; }
  .pickwrap { position: relative; }
  .pick { display: flex; align-items: center; gap: 0.2rem; padding: 0.3rem 0.4rem; height: 100%; }
  .chev { display: grid; color: var(--muted); }
  .icons {
    position: absolute; left: 0; top: calc(100% + 0.2rem); z-index: 2; display: flex; flex-direction: column; gap: 0.2rem;
    padding: 0.35rem; background: var(--panel); border: 1px solid var(--line); border-radius: 6px; box-shadow: 0 4px 12px #0006;
  }
  .icons .icon { padding: 0.15rem; background: none; border: none; width: 1.6rem; height: 1.5rem; }
  .custom input { flex: 1; min-width: 0; padding: 0.35rem 0.5rem; background: var(--panel-2); color: inherit; border: 1px solid var(--line); border-radius: 4px; }
  .ok { display: grid; place-items: center; padding: 0.3rem 0.45rem; }
  hr { border: none; border-top: 1px solid var(--line); margin: 0.6rem 0.6rem 0.3rem; }
  .opt { display: flex; align-items: center; gap: 0.7rem; background: none; border: none; text-align: left; padding: 0.45rem 0.6rem; border-radius: 4px; }
  .opt:hover { background: var(--panel-2); }
  .opt.sel { font-weight: 700; }
  .preset { padding: 0; }
  .choose { flex: 1; display: flex; align-items: center; gap: 0.7rem; background: none; border: none; text-align: left; padding: 0.45rem 0.6rem; font-weight: inherit; }
  .del { background: none; border: none; color: var(--muted); display: grid; padding: 0.3rem 0.5rem; }
  .del:hover { color: var(--red); }
  .icon { width: 1.15rem; height: 1.05rem; flex: none; display: block; }
  .notice { color: var(--accent); font-size: 0.85rem; padding: 0.2rem 0.6rem; }
</style>
