<script lang="ts">
  // Funktionstasten-Editor wie im Windows-Client: links das Raster, rechts
  // die Tastentypen. Änderungen gehen sofort an die Anlage.
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import FkeyTile from "./FkeyTile.svelte";
  import Icon from "./Icon.svelte";
  import { TYPES, blank, fkeys, keyAt, loadFkeys, placeAt, saveOrder, typeInfo, type FunctionKey, type SignalingNumber } from "./fkeys.svelte";

  let { columns = $bindable(3) }: { columns: number } = $props();

  let editing = $state<FunctionKey | null>(null);
  let busy = $state(false);
  let error = $state("");
  // Ziehen mit Zeigerereignissen: HTML5-Drag&Drop kommt im Tauri-Fenster
  // unter Linux nicht an (das Fenster fängt Drops für Dateien ab).
  type Drag = { kind: "type"; type: string; label: string } | { kind: "key"; id: string; label: string };
  let pending: { drag: Drag; x: number; y: number } | null = null;
  let dragging = $state<Drag | null>(null);
  let ghost = $state({ x: 0, y: 0 });
  /** Nach einem Ziehen den folgenden Klick verschlucken */
  let swallowClick = false;
  /** Platz, auf den eine neue Taste nach dem Anlegen kommt */
  let target: number | null = null;
  let over = $state<number | null>(null);
  let signaling = $state<SignalingNumber[]>([]);

  onMount(() => {
    loadFkeys();
    invoke<SignalingNumber[]>("signaling_numbers").then((s) => (signaling = s), () => {});
  });

  const slots = $derived(Math.max(24, Math.ceil((fkeys.order.length + 1) / columns) * columns));
  const groups = [
    { id: "fav", label: "Favoriten" },
    { id: "fn", label: "Funktionstasten" },
    { id: "desk", label: "Nur auf Tischtelefonen verfügbar" },
  ] as const;
  const numbers = $derived(
    [...new Map(fkeys.redirects.filter((r) => r.called_number_id).map((r) => [r.called_number_id, r.called_number])).entries()],
  );

  async function act(cmd: string, args: Record<string, unknown>) {
    busy = true;
    error = "";
    try {
      await invoke(cmd, args);
      await loadFkeys();
      return true;
    } catch (e) {
      error = String(e);
      return false;
    } finally {
      busy = false;
    }
  }

  /** Name wie in Windows, wenn keiner eingegeben ist */
  function defaultName(k: FunctionKey) {
    const label = typeInfo(k.functionKeyType).label;
    switch (k.functionKeyType) {
      case "BUSYLAMPFIELD": return fkeys.accounts.find((a) => a.account_id === k.blfAccountId)?.name ?? label;
      case "QUICKDIAL": return k.directCallTargetnumber ?? label;
      case "FORWARD": return `Umleitung [${{ ALWAYS: "Immer", BUSY: "Besetzt", TIMEOUT: "Zeitüberschreitung" }[k.forwardType ?? "ALWAYS"]}]`;
      case "PARKANDORBIT": return `P+O[${k.poNumber ?? ""}]`;
      case "PHONEDTMF": return `Tastentöne[${k.dtmf ?? ""}]`;
      case "SEPARATOR": return "";
      default: return label;
    }
  }

  function validate(k: FunctionKey) {
    switch (k.functionKeyType) {
      case "BUSYLAMPFIELD": return k.blfAccountId ? "" : "Bitte einen Benutzer wählen.";
      case "QUICKDIAL": return k.directCallTargetnumber?.trim() ? "" : "Bitte eine Rufnummer eingeben.";
      case "FORWARDNUMBER":
      case "FORWARDTOTARGET":
        if (!k.redirectNumberIds.length) return "Bitte mindestens eine Rufnummer wählen.";
        return k.functionKeyType === "FORWARDTOTARGET" && k.forwardTargetType === "PHONENUMBER" && !k.forwardTarget?.trim() ? "Bitte ein Ziel eingeben." : "";
      case "SIGNALNUMBER": return k.displayNumberId === null ? "Bitte eine Rufnummer wählen." : "";
      case "PHONEDTMF": return k.dtmf?.trim() ? "" : "Bitte Tastentöne eingeben.";
      case "PHONEGENERICURL": return k.genericURL?.trim() ? "" : "Bitte eine URL eingeben.";
      default: return "";
    }
  }

  async function save() {
    if (!editing) return;
    const k = $state.snapshot(editing) as FunctionKey;
    error = validate(k);
    if (error) return;
    if (k.functionKeyType === "FORWARDTOTARGET" && k.forwardTargetType === "VOICEMAIL") k.forwardTarget = `destination:${fkeys.accountId}`;
    if (!k.name.trim()) k.name = defaultName(k);
    // Die Bearbeitungsform der Anlage erwartet beim BLF die Rufnummer
    if (k.functionKeyType === "BUSYLAMPFIELD") {
      k.directCallTargetnumber = fkeys.accounts.find((a) => a.account_id === k.blfAccountId)?.number ?? null;
    }
    const before = new Set(fkeys.keys.map((x) => x.id));
    const slot = k.id ? null : target;
    if (!(await act("fkey_save", { set: fkeys.setId, key: k }))) return;
    editing = null;
    target = null;
    // Neue Taste auf den Platz legen, auf den sie gezogen wurde
    const added = fkeys.keys.find((x) => !before.has(x.id));
    if (added && slot !== null) await reorder(placeAt(fkeys.order, added.id, slot));
  }

  async function reorder(order: string[]) {
    busy = true;
    error = "";
    try {
      await saveOrder(order);
    } catch (e) {
      error = String(e);
    }
    await loadFkeys();
    busy = false;
  }

  async function remove(k: FunctionKey) {
    if (await act("fkey_delete", { set: fkeys.setId, id: k.id })) editing = null;
  }

  function startDrag(e: PointerEvent, drag: Drag) {
    if (e.button !== 0 || busy) return;
    pending = { drag, x: e.clientX, y: e.clientY };
  }

  function slotAt(x: number, y: number) {
    const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-slot]");
    return el ? Number(el.dataset.slot) : null;
  }

  function onMove(e: PointerEvent) {
    if (!pending) return;
    if (!dragging) {
      if (Math.hypot(e.clientX - pending.x, e.clientY - pending.y) < 6) return;
      dragging = pending.drag;
    }
    e.preventDefault();
    ghost = { x: e.clientX, y: e.clientY };
    over = slotAt(e.clientX, e.clientY);
  }

  function onUp(e: PointerEvent) {
    const d = dragging;
    pending = null;
    dragging = null;
    over = null;
    if (!d) return;
    swallowClick = true;
    setTimeout(() => (swallowClick = false), 0);
    const slot = slotAt(e.clientX, e.clientY);
    if (slot === null) return;
    if (d.kind === "type") {
      target = slot;
      editing = blank(d.type);
    } else if (fkeys.order[slot] !== d.id) {
      reorder(placeAt(fkeys.order, d.id, slot));
    }
  }

  function cancelDrag() {
    pending = null;
    dragging = null;
    over = null;
  }

  /** Klick nur, wenn nicht gerade gezogen wurde */
  const click = (f: () => void) => () => {
    if (!swallowClick) f();
  };

  const toggleNumber = (id: number) => {
    if (!editing) return;
    const ids = editing.redirectNumberIds;
    editing.redirectNumberIds = ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id];
  };
</script>

<svelte:window onpointermove={onMove} onpointerup={onUp} onpointercancel={cancelDrag} onblur={cancelDrag} />

{#if dragging}
  <div class="ghost" style="left: {ghost.x}px; top: {ghost.y}px">{dragging.label}</div>
{/if}

<div class="editor" class:dragging={!!dragging}>
  <div class="left">
    <label class="cols">Anzahl der Spalten
      <select bind:value={columns}>{#each [1, 2, 3, 4, 5, 6] as n}<option value={n}>{n}</option>{/each}</select>
    </label>
    <div class="grid" style="grid-template-columns: repeat({columns}, minmax(0, 1fr))">
      {#each Array(slots) as _, i}
        {@const k = keyAt(i)}
        <div
          class="slot"
          class:over={over === i}
          role="listitem"
          data-slot={i}
        >
          <span class="num">{String(i + 1).padStart(2, "0")}</span>
          {#if k}
            <div
              class="keywrap"
              class:lifted={dragging?.kind === "key" && dragging.id === k.id}
              role="button"
              tabindex="-1"
              onpointerdown={(e) => startDrag(e, { kind: "key", id: k.id, label: k.name || typeInfo(k.functionKeyType).label })}
            >
              <FkeyTile key={k} editor onclick={click(() => (editing = structuredClone($state.snapshot(k)) as FunctionKey))} />
            </div>
          {:else}
            <div class="empty"></div>
          {/if}
        </div>
      {/each}
    </div>
    <p class="small muted">Typ von rechts auf einen Platz ziehen oder anklicken. Tasten lassen sich im Raster verschieben: auf einen leeren Platz legen oder auf eine Taste ziehen, um sie davor einzuschieben. Ein Klick öffnet sie zum Bearbeiten.</p>
  </div>

  <div class="types">
    <h4>Funktionstastentypen</h4>
    {#each groups as g}
      <h5>{g.label}</h5>
      {#each TYPES.filter((t) => t.group === g.id) as t}
        <button
          class="type"
          class:unusable={!t.usable}
          onpointerdown={(e) => startDrag(e, { kind: "type", type: t.type, label: t.label })}
          onclick={click(() => { target = null; editing = blank(t.type); })}
        >{t.label}</button>
      {/each}
    {/each}
  </div>
</div>
{#if fkeys.error}<p class="notice">{fkeys.error}</p>{/if}
{#if error && !editing}<p class="notice">{error}</p>{/if}

{#if editing}
  {@const k = editing}
  <div class="scrim" role="presentation" onclick={() => (editing = null)}></div>
  <div class="dialog" role="dialog" aria-label="Funktionstaste bearbeiten">
    <h4>{typeInfo(k.functionKeyType).label}{k.id ? "" : " hinzufügen"}</h4>
    {#if !typeInfo(k.functionKeyType).usable}
      <p class="small muted">Diese Taste wirkt auf Tischtelefonen; im Linux-Client wird sie nur angezeigt.</p>
    {/if}
    {#if k.functionKeyType !== "SEPARATOR"}
      <label class="row"><span>Bezeichnung</span><input type="text" bind:value={k.name} placeholder={defaultName(k)} /></label>
    {/if}
    {#if k.functionKeyType === "BUSYLAMPFIELD"}
      <label class="row"><span>Benutzer</span>
        <select bind:value={k.blfAccountId}>
          <option value={null}>Bitte wählen …</option>
          {#each fkeys.accounts as a}<option value={a.account_id}>{a.name} ({a.number})</option>{/each}
        </select>
      </label>
    {:else if k.functionKeyType === "QUICKDIAL"}
      <label class="row"><span>Rufnummer</span><input type="text" bind:value={k.directCallTargetnumber} /></label>
    {:else if k.functionKeyType === "FORWARD"}
      <label class="row"><span>Art</span>
        <select bind:value={k.forwardType}>
          <option value="ALWAYS">Immer</option><option value="BUSY">Besetzt</option><option value="TIMEOUT">Zeitüberschreitung</option>
        </select>
      </label>
    {:else if k.functionKeyType === "FORWARDNUMBER" || k.functionKeyType === "FORWARDTOTARGET"}
      <div class="row"><span>Rufnummern</span>
        <span class="checks">
          {#each numbers as [id, n]}
            <label><input type="checkbox" checked={k.redirectNumberIds.includes(Number(id))} onchange={() => toggleNumber(Number(id))} /> {n}</label>
          {:else}<span class="muted small">Keine Rufnummern gefunden.</span>{/each}
        </span>
      </div>
      {#if k.functionKeyType === "FORWARDTOTARGET"}
        <label class="row"><span>Ziel</span>
          <select bind:value={k.forwardTargetType}>
            <option value="VOICEMAIL">Voicemail</option><option value="PHONENUMBER">Rufnummer</option>
          </select>
        </label>
        {#if k.forwardTargetType === "PHONENUMBER"}
          <label class="row"><span>Zielrufnummer</span><input type="text" bind:value={k.forwardTarget} /></label>
        {/if}
      {/if}
    {:else if k.functionKeyType === "SIGNALNUMBER"}
      <label class="row"><span>Rufnummer</span>
        <select bind:value={k.displayNumberId}>
          <option value={null}>Bitte wählen …</option>
          {#each signaling as s}<option value={s.suppressed ? 0 : Number(s.id)}>{s.suppressed ? "Nummer unterdrücken" : s.number}</option>{/each}
        </select>
      </label>
    {:else if k.functionKeyType === "PARKANDORBIT"}
      <label class="row"><span>Parkplatz</span><input type="text" bind:value={k.poNumber} /></label>
    {:else if k.functionKeyType === "PHONEDTMF"}
      <label class="row"><span>Tastentöne</span><input type="text" bind:value={k.dtmf} placeholder="z. B. 12345" /></label>
    {:else if k.functionKeyType === "PHONEGENERICURL"}
      <label class="row"><span>URL</span><input type="text" bind:value={k.genericURL} placeholder="https://…" /></label>
    {:else if k.functionKeyType === "ADDRESSBOOK"}
      <label class="row"><span>Anzeige</span>
        <select bind:value={k.addressbookRequest}><option value="CONTACTLIST">Kontaktliste</option><option value="CONTACTSEARCH">Kontaktsuche</option></select>
      </label>
      <label class="row"><span>Adressbuch</span><input type="text" bind:value={k.addressBookFolderName} /></label>
    {:else if k.functionKeyType === "PHONECALLLIST"}
      <label class="row"><span>Liste</span>
        <select bind:value={k.callListRequest}><option value="INCOMING">Eingehend</option><option value="OUTGOING">Ausgehend</option><option value="MISSED">Verpasst</option></select>
      </label>
    {:else if k.functionKeyType === "GROUPLOGIN" || k.functionKeyType === "MODULEACTIVATION"}
      <p class="small muted">Die Auswahl der {k.functionKeyType === "GROUPLOGIN" ? "Gruppen" : "Module"} folgt später; bis dahin bitte in der Web-App einstellen.</p>
    {/if}
    {#if error}<p class="notice">{error}</p>{/if}
    <div class="actions">
      {#if k.id}<button class="danger" disabled={busy} onclick={() => editing && remove(editing)}><Icon name="trash" size={18} /> Entfernen</button>{/if}
      <span class="spacer"></span>
      <button onclick={() => (editing = null)}>Abbrechen</button>
      <button class="primary" disabled={busy} onclick={save}>{k.id ? "Speichern" : "Hinzufügen"}</button>
    </div>
  </div>
{/if}

<style>
  .editor { display: grid; grid-template-columns: minmax(0, 1fr) 14rem; gap: 1rem; }
  .left { display: flex; flex-direction: column; gap: 0.5rem; min-width: 0; }
  .cols { display: flex; align-items: center; gap: 0.6rem; }
  select, input[type="text"] { padding: 0.3rem 0.5rem; background: var(--panel-2); color: inherit; border: 1px solid var(--line); border-radius: 4px; font: inherit; }
  .grid { display: grid; gap: 0.4rem; }
  .slot { display: grid; grid-template-columns: 1.6rem 1fr; align-items: center; gap: 0.3rem; border-radius: 6px; }
  .slot.over { outline: 2px solid var(--accent); }
  .dragging, .dragging * { cursor: grabbing !important; user-select: none; }
  .keywrap { touch-action: none; }
  .keywrap.lifted { opacity: 0.35; }
  .ghost {
    position: fixed; z-index: 40; pointer-events: none; transform: translate(-50%, -50%);
    max-width: 14rem; padding: 0.35rem 0.7rem; border-radius: 6px; background: var(--panel-2);
    border: 1px solid var(--accent); box-shadow: 0 4px 14px #0006; font-size: 0.9rem; font-weight: 600;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .num { color: var(--muted); font-size: 0.8rem; font-variant-numeric: tabular-nums; text-align: right; }
  .empty { min-height: 3.2rem; border: 1px dashed var(--line); border-radius: 6px; }
  .keywrap { min-width: 0; }
  .types { display: flex; flex-direction: column; gap: 0.25rem; }
  .types h4 { margin: 0 0 0.2rem; }
  .types h5 { margin: 0.6rem 0 0.1rem; font-size: 0.8rem; color: var(--muted); font-weight: 600; }
  .type { text-align: left; padding: 0.35rem 0.6rem; cursor: grab; touch-action: none; }
  .type.unusable { opacity: 0.55; }
  .small { font-size: 0.85rem; }
  .muted { color: var(--muted); margin: 0.2rem 0; }
  .notice { color: var(--accent); margin: 0.3rem 0; }
  .scrim { position: fixed; inset: 0; background: #0007; z-index: 30; }
  .dialog {
    position: fixed; z-index: 31; left: 50%; top: 50%; transform: translate(-50%, -50%); width: min(30rem, 92vw);
    background: var(--panel); border: 1px solid var(--line); border-radius: 8px; padding: 1rem; display: flex; flex-direction: column; gap: 0.6rem;
  }
  .dialog h4 { margin: 0 0 0.2rem; }
  .row { display: grid; grid-template-columns: 7.5rem minmax(0, 1fr); align-items: center; gap: 0.6rem; }
  .checks { display: flex; flex-wrap: wrap; gap: 0.3rem 0.9rem; }
  .actions { display: flex; gap: 0.6rem; margin-top: 0.4rem; }
  .spacer { flex: 1; }
  .primary { background: var(--accent); border-color: var(--accent); color: #111; font-weight: 600; }
  .danger { display: flex; align-items: center; gap: 0.3rem; color: var(--red); }
  @media (max-width: 800px) { .editor { grid-template-columns: 1fr; } }
</style>
