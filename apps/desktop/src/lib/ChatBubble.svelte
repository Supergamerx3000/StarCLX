<script lang="ts">
  // Chat-Status als Sprechblase wie in der STARFACE-App: grün mit Haken
  // (verfügbar), rot mit Kreis (abwesend, offline), rot mit Kreuz (nicht stören).
  let { state }: { state: string } = $props();
</script>

<svg class="bubble {state}" viewBox="0 0 14 13" aria-hidden="true">
  <path class="body" d="M1 1h12v8.5H6.5L3.5 12V9.5H1z" />
  {#if state === "available"}
    <path class="mark" d="M4.2 5.2l2 2 3.8-3.8" />
  {:else if state === "dnd"}
    <path class="mark" d="M4.8 3.2l4.4 4.2M9.2 3.2L4.8 7.4" />
  {:else}
    <circle class="mark" cx="7" cy="5.25" r="2.1" />
  {/if}
</svg>

<style>
  .bubble { width: 100%; height: 100%; overflow: visible; display: block; }
  .body { stroke: #fff; stroke-width: 1; stroke-linejoin: round; fill: var(--red); }
  .available .body { fill: var(--green); }
  .mark { fill: none; stroke: #fff; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
</style>
