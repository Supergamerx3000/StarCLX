<script lang="ts" module>
  export type Device = { name: string; description: string };
  export const DEFAULT = "@default";

  /** Gespeicherte Reihenfolge plus neu gefundene Geräte am Ende */
  export function mergeOrder(order: string[], devices: Device[]) {
    const out = order.length ? [...order] : [DEFAULT];
    if (!out.includes(DEFAULT)) out.unshift(DEFAULT);
    for (const d of devices) if (!out.includes(d.name)) out.push(d.name);
    return out;
  }
</script>

<script lang="ts">
  import Icon from "./Icon.svelte";
  import { t } from "./i18n.svelte.js";

  let { order = $bindable([]), devices }: { order: string[]; devices: Device[] } = $props();
  let selected = $state(0);

  const label = (name: string) =>
    name === DEFAULT ? t("Systemstandard") : (devices.find((d) => d.name === name)?.description || name);
  const present = (name: string) => name === DEFAULT || devices.some((d) => d.name === name);
  // Das Gerät, das tatsächlich benutzt wird
  const active = $derived(order.find(present));

  function move(delta: number) {
    const to = selected + delta;
    if (to < 0 || to >= order.length) return;
    const next = [...order];
    [next[selected], next[to]] = [next[to], next[selected]];
    order = next;
    selected = to;
  }
  function remove() {
    if (order[selected] === DEFAULT) return;
    order = order.filter((_, i) => i !== selected);
    selected = Math.min(selected, order.length - 1);
  }
</script>

<div class="list" role="listbox">
  {#each order as name, i (name)}
    <button class="row" class:sel={i === selected} class:absent={!present(name)} onclick={() => (selected = i)} role="option" aria-selected={i === selected}>
      <span>{label(name)}</span>
      {#if !present(name)}<small>{t("nicht verbunden")}</small>{/if}
      {#if name === active}<small class="act">{t("wird verwendet")}</small>{/if}
    </button>
  {/each}
</div>
<div class="acts">
  <button onclick={remove} disabled={order[selected] === DEFAULT}>{t("Löschen")}</button>
  <span class="spacer"></span>
  <button onclick={() => move(1)} disabled={selected >= order.length - 1}><Icon name="down" size={18} /> {t("Nach unten")}</button>
  <button onclick={() => move(-1)} disabled={selected <= 0}><Icon name="up" size={18} /> {t("Nach oben")}</button>
</div>

<style>
  .list { display: flex; flex-direction: column; gap: 2px; margin: 0.4rem 0; }
  .row { display: flex; align-items: center; gap: 0.6rem; text-align: left; border-radius: 0; border: 1px solid transparent; background: var(--panel-2); padding: 0.55rem 0.8rem; }
  .row.absent { background: var(--bar-2); color: var(--muted); }
  .row.sel { border-color: var(--accent); background: #1c1e21; }
  small { font-size: 0.78rem; color: var(--muted); }
  .act { margin-left: auto; color: var(--green); }
  .acts { display: flex; gap: 0.5rem; align-items: center; }
  .acts button { display: flex; align-items: center; gap: 0.35rem; }
  .spacer { flex: 1; }
</style>
