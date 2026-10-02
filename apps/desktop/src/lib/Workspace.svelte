<script lang="ts" module>
  import type { Tile } from "./prefs.svelte";

  /** Rasterspalten der Fläche; Zeilen sind ROW Pixel hoch */
  export const COLS = 12;
  const ROW = 40;

  export const DEFAULT_TILES: Tile[] = [
    { id: "fkeys", x: 0, y: 0, w: 6, h: 8, visible: true },
    { id: "journal", x: 6, y: 0, w: 6, h: 8, visible: true },
    { id: "contacts", x: 0, y: 8, w: 4, h: 8, visible: true },
    { id: "chat", x: 4, y: 8, w: 4, h: 8, visible: true },
    { id: "voicemail", x: 8, y: 8, w: 4, h: 8, visible: true },
  ];

  /** Gespeicherte Kacheln, ergänzt um neue und ohne unbekannte */
  export function tilesOf(saved: Tile[] | null | undefined): Tile[] {
    const list = (saved ?? []).filter((t) => DEFAULT_TILES.some((d) => d.id === t.id));
    for (const d of DEFAULT_TILES) if (!list.some((t) => t.id === d.id)) list.push({ ...d });
    return list.map((t) => ({ ...t }));
  }
</script>

<script lang="ts">
  // Freier Arbeitsbereich: Im Bearbeiten-Modus Kacheln an der Titelleiste
  // verschieben, an der Ecke in der Grösse ändern; sie rasten am Raster ein.
  // Ausserhalb des Bearbeiten-Modus ist die Anordnung fixiert. Ziehen läuft über
  // Zeigerereignisse (HTML5-Drag&Drop funktioniert im Linux-Fenster nicht).
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";

  let {
    tiles = $bindable(),
    editing = false,
    meta,
    badge,
    body,
    onchange,
  }: {
    tiles: Tile[];
    editing?: boolean;
    meta: Record<string, { icon: IconName; label: string }>;
    badge?: (id: string) => number;
    body: Snippet<[string]>;
    onchange?: () => void;
  } = $props();

  let width = $state(0);
  const col = $derived(width / COLS);
  /** Zuletzt angefasste Kachel liegt oben */
  let order = $state<string[]>([]);
  let drag: { id: string; mode: "move" | "size"; sx: number; sy: number; start: Tile } | null = null;

  const z = (id: string) => order.indexOf(id) + 1;
  const height = $derived(Math.max(...tiles.filter((t) => t.visible).map((t) => t.y + t.h), 1) * ROW + ROW * 4);

  function start(e: PointerEvent, t: Tile, mode: "move" | "size") {
    if (!editing || e.button !== 0 || (mode === "move" && (e.target as HTMLElement).closest("button"))) return;
    e.preventDefault();
    order = [...order.filter((x) => x !== t.id), t.id];
    drag = { id: t.id, mode, sx: e.clientX, sy: e.clientY, start: { ...t } };
  }

  function onmove(e: PointerEvent) {
    if (!drag || !col) return;
    const t = tiles.find((x) => x.id === drag?.id);
    if (!t) return;
    const dx = Math.round((e.clientX - drag.sx) / col);
    const dy = Math.round((e.clientY - drag.sy) / ROW);
    if (drag.mode === "move") {
      t.x = Math.min(Math.max(0, drag.start.x + dx), COLS - t.w);
      t.y = Math.max(0, drag.start.y + dy);
    } else {
      t.w = Math.min(Math.max(2, drag.start.w + dx), COLS - t.x);
      t.h = Math.max(3, drag.start.h + dy);
    }
  }

  const overlaps = (a: Tile, b: Tile) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

  /** Überdeckte Kacheln unter die bewegte schieben (und deren Nachbarn weiter) */
  function pushDown(moved: Tile) {
    const fixed = [moved];
    for (let guard = 0; guard < 50; guard++) {
      const hit = tiles.find((t) => t.visible && !fixed.includes(t) && fixed.some((f) => overlaps(f, t)));
      if (!hit) return;
      const blocker = fixed.filter((f) => overlaps(f, hit));
      hit.y = Math.max(...blocker.map((f) => f.y + f.h));
      fixed.push(hit);
    }
  }

  function onup() {
    if (!drag) return;
    const t = tiles.find((x) => x.id === drag?.id);
    const s = drag.start;
    drag = null;
    if (t && (t.x !== s.x || t.y !== s.y || t.w !== s.w || t.h !== s.h)) {
      pushDown(t);
      onchange?.();
    }
  }

  function hide(t: Tile) {
    t.visible = false;
    onchange?.();
  }
</script>

<svelte:window onpointermove={onmove} onpointerup={onup} onpointercancel={onup} />

<div class="area" class:editing bind:clientWidth={width} style="height: {height}px">
  {#each tiles.filter((t) => t.visible) as t (t.id)}
    <section
      class="tile"
      data-tile={t.id}
      style="left: {t.x * col}px; top: {t.y * ROW}px; width: {t.w * col}px; height: {t.h * ROW}px; z-index: {z(t.id)}"
    >
      <header role="toolbar" tabindex="-1" onpointerdown={(e) => start(e, t, "move")}>
        <Icon name={meta[t.id].icon} size={16} />
        <span>{meta[t.id].label}</span>
        {#if badge?.(t.id)}<b class="unread">{badge(t.id)}</b>{/if}
        {#if editing}<button class="x" title="Ausblenden" onclick={() => hide(t)}><Icon name="close" size={16} /></button>{/if}
      </header>
      <div class="body">{@render body(t.id)}</div>
      {#if editing}<span class="grip" role="separator" aria-label="Grösse ändern" onpointerdown={(e) => start(e, t, "size")}></span>{/if}
    </section>
  {/each}
</div>

<style>
  .area { position: relative; min-height: 100%; }
  .tile {
    position: absolute; box-sizing: border-box; padding: 0.25rem; display: flex; flex-direction: column;
    transition: left 0.08s, top 0.08s, width 0.08s, height 0.08s;
  }
  .tile > * { min-width: 0; }
  header {
    display: flex; align-items: center; gap: 0.45rem; padding: 0.3rem 0.6rem; user-select: none;
    background: var(--bar-2); border: 1px solid var(--line); border-bottom: none; border-radius: 6px 6px 0 0; font-weight: 600; font-size: 0.9rem;
  }
  header span { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .editing header { cursor: grab; touch-action: none; }
  .editing .tile .body { outline: 1px dashed var(--accent); outline-offset: -1px; }
  .unread { background: var(--accent); color: #111; border-radius: 999px; padding: 0 0.45rem; font-size: 0.78rem; }
  .x { background: none; border: none; padding: 0.1rem; color: var(--muted); display: grid; }
  .body { flex: 1; min-height: 0; overflow: auto; background: var(--bg); border: 1px solid var(--line); border-radius: 0 0 6px 6px; padding: 0.4rem; }
  .grip {
    position: absolute; right: 0.25rem; bottom: 0.25rem; width: 0.9rem; height: 0.9rem; cursor: nwse-resize; touch-action: none;
    background: linear-gradient(135deg, transparent 50%, var(--muted) 50%, var(--muted) 60%, transparent 60%, transparent 75%, var(--muted) 75%, var(--muted) 85%, transparent 85%);
  }
</style>
