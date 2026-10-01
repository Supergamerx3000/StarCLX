<script lang="ts">
  // Gemeinsame Farben und Grundstile für alle Fenster (Hauptfenster, Schnellwahl)
  let { children } = $props();
</script>

{@render children()}

<style>
  :global(:root) {
    font-family: "Segoe UI", system-ui, sans-serif;
    color-scheme: dark;
    --bg: #1f2226;
    --bar: #16181b;
    --bar-2: #2a2d31;
    --panel: #2b2f34;
    --panel-2: #373b41;
    --line: #3d4248;
    --text: #e8eaed;
    --muted: #a3a9b0;
    --accent: #f5a31a;
    --accent-soft: #f5a31a33;
    --green: #5cb82b;
    --red: #d9262d;
    color: var(--text);
    background: var(--bg);
  }
  :global(:root[data-theme="light"]) {
    color-scheme: light;
    --bg: #eef0f3;
    --bar: #ffffff;
    --bar-2: #e4e7eb;
    --panel: #ffffff;
    --panel-2: #e6e9ed;
    --line: #d3d8de;
    --text: #1f2226;
    --muted: #5f6670;
    --accent-soft: #f5a31a40;
  }
  :global(body) { margin: 0; }
  :global(input), :global(button) {
    font: inherit; padding: 0.55rem 0.8rem; border-radius: 6px; border: 1px solid var(--line);
    background: var(--bar-2); color: inherit;
  }
  :global(button) { cursor: pointer; }
  :global(button:disabled) { cursor: default; }
</style>
