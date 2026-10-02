<script lang="ts">
  import { splitter } from "./splitter";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import { phone, run } from "./phone.svelte";
  import { initials, type Contact, type Folder } from "./contacts";
  import { contactEdit, editContact, newContact } from "./contactform.svelte";

  let folders = $state<Folder[]>([]);
  let folder = $state("");
  let term = $state("");
  let contacts = $state<Contact[]>([]);
  let total = $state(0);
  let loading = $state(false);
  let error = $state("");
  let selected = $state<Contact | null>(null);
  let seq = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const ready = $derived(phone.status.state === "ready");

  onMount(async () => {
    try {
      folders = await invoke<Folder[]>("contacts_folders");
      if (folders.length) pick(folders[0].id);
    } catch (e) {
      error = String(e);
    }
  });

  const writable = $derived(folders.find((f) => f.id === folder)?.writable ?? false);

  // Nach Anlegen, Ändern oder Löschen die Liste neu laden
  let seen = contactEdit.changed;
  $effect(() => {
    if (contactEdit.changed === seen) return;
    seen = contactEdit.changed;
    const id = selected?.id;
    load(true).then(() => (selected = contacts.find((c) => c.id === id) ?? null));
  });

  function pick(id: string) {
    folder = id;
    selected = null;
    load(true);
  }

  async function load(reset: boolean) {
    if (!folder) return;
    const my = ++seq;
    loading = true;
    try {
      const page = await invoke<{ contacts: Contact[]; total: number }>("contacts_list", {
        folder,
        term,
        offset: reset ? 0 : contacts.length,
      });
      if (my !== seq) return;
      contacts = reset ? page.contacts : [...contacts, ...page.contacts];
      total = page.total;
      error = "";
    } catch (e) {
      if (my === seq) error = String(e);
    } finally {
      if (my === seq) loading = false;
    }
  }

  function oninput() {
    clearTimeout(timer);
    timer = setTimeout(() => load(true), 250);
  }
</script>

<div class="book">
  <nav class="folders">
    {#each folders as f (f.id)}
      <button class:active={f.id === folder} onclick={() => pick(f.id)}>{f.name}</button>
    {/each}
    {#if folders.some((f) => f.writable)}
      <span class="spacer"></span>
      <button class="add" title="Kontakt anlegen" onclick={() => newContact({ folder: writable ? folder : "" })}><Icon name="person" size={18} /> Neuer Kontakt</button>
    {/if}
  </nav>
  <div class="cols">
    <div class="list">
      <label class="filter">
        <Icon name="search" size={18} />
        <input bind:value={term} {oninput} placeholder="Im Adressbuch suchen" />
      </label>
      {#if error}<p class="muted">{error}</p>{/if}
      {#each contacts as c (c.id)}
        <button class="row" class:active={selected?.id === c.id} onclick={() => (selected = c)}>
          <span class="av">{initials(c.name)}</span>
          <span class="txt">
            <strong>{c.name || "Ohne Namen"}</strong>
            <small>{c.company && c.company !== c.name ? c.company : (c.numbers[0]?.number ?? "")}</small>
          </span>
        </button>
      {:else}
        {#if !loading && !error}<p class="muted">Keine Kontakte gefunden.</p>{/if}
      {/each}
      {#if contacts.length < total}
        <button class="more" onclick={() => load(false)} disabled={loading}>{loading ? "Lade …" : `Weitere laden (${total - contacts.length})`}</button>
      {/if}
    </div>
    <div class="divider" role="separator" aria-orientation="vertical" use:splitter={"contacts"}></div>
    <div class="detail">
      {#if selected}
        <div class="head">
          <span class="av big">{initials(selected.name)}</span>
          <div>
            <h3>{selected.name || "Ohne Namen"}</h3>
            {#if selected.company && selected.company !== selected.name}<p class="muted">{selected.company}</p>{/if}
          </div>
          {#if selected.editable}
            <span class="spacer"></span>
            <button class="edit" title="Kontakt bearbeiten" onclick={() => selected && editContact(selected.id)}><Icon name="edit" size={18} /> Bearbeiten</button>
          {/if}
        </div>
        {#each selected.numbers as n}
          <div class="num">
            <span class="lbl">{n.label}</span>
            <span class="val">{n.number}</span>
            <button class="call" disabled={!ready} title="Anrufen" onclick={() => run("phone_dial", { number: n.number })}><Icon name="call" size={18} /></button>
          </div>
        {/each}
        {#if selected.email}
          <div class="num"><span class="lbl">E-Mail</span><span class="val">{selected.email}</span></div>
        {/if}
      {:else}
        <p class="muted">Kontakt auswählen.</p>
      {/if}
    </div>
  </div>
</div>

<style>
  .book { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .folders { display: flex; gap: 0.3rem; flex-wrap: wrap; padding-bottom: 0.5rem; }
  .folders button { padding: 0.3rem 0.8rem; border-radius: 999px; font-size: 0.9rem; background: var(--panel); }
  .spacer { flex: 1; }
  .folders .add, .edit { display: flex; align-items: center; gap: 0.35rem; }
  .folders button.active { background: var(--accent); color: #111; border-color: var(--accent); }
  .cols { flex: 1; min-height: 0; display: grid; grid-template-columns: min(var(--split, 20rem), calc(100% - 12rem)) 0.5rem minmax(0, 1fr); }
  .divider { cursor: col-resize; touch-action: none; position: relative; }
  .divider::after { content: ""; position: absolute; inset: 0 3px; border-radius: 2px; }
  .divider:hover::after, .divider:global(.dragging)::after { background: var(--accent); }

  .list { overflow: auto; background: var(--panel); border-radius: 4px; padding: 0.4rem; display: flex; flex-direction: column; gap: 0.1rem; }
  .filter { display: flex; align-items: center; gap: 0.4rem; padding: 0 0.6rem; margin-bottom: 0.3rem; background: var(--bar-2); border: 1px solid var(--line); border-radius: 999px; }
  .filter input { border: none; background: none; flex: 1; outline: none; padding: 0.45rem 0; }
  .row { display: flex; align-items: center; gap: 0.6rem; text-align: left; background: none; border: none; padding: 0.4rem 0.5rem; }
  .row:hover { background: var(--panel-2); }
  .row.active { background: var(--accent-soft); }
  .txt { display: flex; flex-direction: column; min-width: 0; }
  .txt strong, .txt small { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .txt small { color: var(--muted); }
  .av { flex: none; width: 2.2rem; height: 2.2rem; border-radius: 50%; display: grid; place-items: center; background: var(--panel-2); font-size: 0.8rem; font-weight: 600; }
  .av.big { width: 3.4rem; height: 3.4rem; font-size: 1.1rem; }
  .more { margin-top: 0.4rem; }
  .detail { overflow: auto; background: var(--panel); border-radius: 4px; padding: 1rem 1.2rem; }
  .head { display: flex; align-items: center; gap: 0.9rem; margin-bottom: 0.8rem; }
  .head h3 { margin: 0; font-size: 1.15rem; }
  .head p { margin: 0.1rem 0 0; }
  .num { display: flex; align-items: center; gap: 0.8rem; padding: 0.45rem 0; border-top: 1px solid var(--line); }
  .lbl { flex: 0 0 6rem; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--muted); font-size: 0.9rem; }
  .val { flex: 1; color: inherit; }
  .call { width: 2.2rem; height: 2.2rem; padding: 0; border-radius: 50%; display: grid; place-items: center; background: var(--green); border: none; color: #fff; }
  .call:disabled { opacity: 0.4; }
  .muted { color: var(--muted); }
</style>
