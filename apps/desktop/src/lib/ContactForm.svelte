<script lang="ts">
  // Kontakt anlegen oder bearbeiten. Die Felder gibt die Anlage vor.
  import { invoke } from "@tauri-apps/api/core";
  import Icon from "./Icon.svelte";
  import type { Folder } from "./contacts";
  import { contactEdit, PHONE_KEYS, type ContactField } from "./contactform.svelte";

  let fields = $state<ContactField[]>([]);
  let folders = $state<Folder[]>([]);
  let folder = $state("");
  let loading = $state(true);
  let busy = $state(false);
  let error = $state("");
  let confirmDelete = $state(false);

  const isNew = !contactEdit.id;
  const groups = $derived([...new Set(fields.map((f) => f.group))]);

  (async () => {
    try {
      fields = await invoke<ContactField[]>("contact_form", { id: contactEdit.id });
      if (isNew) {
        folders = (await invoke<Folder[]>("contacts_folders")).filter((f) => f.writable);
        folder = (folders.find((f) => f.id === contactEdit.folder) ?? folders.find((f) => f.private) ?? folders[0])?.id ?? "";
        if (contactEdit.number) {
          const slot = fields.find((f) => PHONE_KEYS.includes(f.key));
          if (slot) slot.value = contactEdit.number;
        }
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  })();

  const SURNAME = 2;
  /** Cursor gleich in den Nachnamen, z. B. bei Übernahme aus der Rufliste */
  function focusIf(node: HTMLInputElement, on: boolean) {
    if (on) setTimeout(() => node.focus());
  }

  const SURNAME_OR_COMPANY = [2, 12];
  /** Ohne Nachname oder Firma lehnt die Anlage den Kontakt ab */
  const named = $derived(
    fields.some((f) => SURNAME_OR_COMPANY.includes(f.key))
      ? fields.some((f) => SURNAME_OR_COMPANY.includes(f.key) && f.value.trim())
      : fields.some((f) => f.value.trim()),
  );

  function close() {
    contactEdit.open = false;
  }

  async function save() {
    busy = true;
    error = "";
    try {
      await invoke("contact_save", { id: contactEdit.id, folder, fields: $state.snapshot(fields) });
      contactEdit.changed++;
      close();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function remove() {
    if (!confirmDelete) return void (confirmDelete = true);
    busy = true;
    try {
      await invoke("contact_delete", { id: contactEdit.id });
      contactEdit.changed++;
      close();
    } catch (e) {
      error = String(e);
      confirmDelete = false;
    } finally {
      busy = false;
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && close()} />

<div class="scrim" role="presentation" onclick={close}></div>
<div class="dialog" role="dialog" aria-label={isNew ? "Neuer Kontakt" : "Kontakt bearbeiten"}>
  <h4>{isNew ? "Neuer Kontakt" : "Kontakt bearbeiten"}</h4>
  {#if loading}
    <p class="muted">Lade …</p>
  {:else}
    <div class="body">
      {#if isNew}
        <label class="row"><span>Adressbuch</span>
          <select bind:value={folder}>
            {#each folders as f}<option value={f.id}>{f.name}</option>{/each}
          </select>
        </label>
        {#if !folders.length}<p class="notice">Du darfst in keinem Adressbuch Kontakte anlegen.</p>{/if}
      {/if}
      {#each groups as g}
        <h5>{g}</h5>
        {#each fields.filter((f) => f.group === g) as f}
          <label class="row"><span>{f.label}</span><input type="text" bind:value={f.value} use:focusIf={isNew && f.key === SURNAME} /></label>
        {/each}
      {/each}
    </div>
  {/if}
  {#if error}<p class="notice">{error}</p>{:else if !loading && !named}<p class="muted hint">Nachname oder Firma ausfüllen.</p>{/if}
  <div class="actions">
    {#if !isNew}
      <button class="danger" disabled={busy || loading} onclick={remove}><Icon name="trash" size={18} /> {confirmDelete ? "Wirklich löschen?" : "Löschen"}</button>
    {/if}
    <span class="spacer"></span>
    <button onclick={close}>Abbrechen</button>
    <button class="primary" disabled={busy || loading || !named || (isNew && !folder)} onclick={save}>{isNew ? "Anlegen" : "Speichern"}</button>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: #0007; z-index: 40; }
  .dialog {
    position: fixed; z-index: 41; left: 50%; top: 50%; transform: translate(-50%, -50%); width: min(32rem, 92vw); max-height: 88vh;
    background: var(--panel); border: 1px solid var(--line); border-radius: 8px; padding: 1rem; display: flex; flex-direction: column; gap: 0.6rem;
  }
  .dialog h4 { margin: 0; }
  .body { overflow: auto; display: flex; flex-direction: column; gap: 0.45rem; padding-right: 0.3rem; }
  h5 { margin: 0.5rem 0 0; font-size: 0.8rem; color: var(--muted); font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; }
  .hint { margin: 0; font-size: 0.85rem; }
  .row { display: grid; grid-template-columns: 8rem minmax(0, 1fr); align-items: center; gap: 0.6rem; }
  .row span { color: var(--muted); font-size: 0.9rem; }
  .actions { display: flex; gap: 0.6rem; margin-top: 0.2rem; }
  .spacer { flex: 1; }
  .primary { background: var(--accent); border-color: var(--accent); color: #111; font-weight: 600; }
  .danger { display: flex; align-items: center; gap: 0.3rem; color: var(--red); }
  .muted { color: var(--muted); }
  .notice { color: var(--accent); margin: 0; }
</style>
