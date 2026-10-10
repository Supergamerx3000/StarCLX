<script lang="ts">
  // Gemeinsame Farben und Grundstile für alle Fenster (Hauptfenster, Schnellwahl)
  // Inter wird mitgeliefert: unter Linux gäbe es sonst je nach Distribution
  // eine andere, oft sehr breite Schrift (z. B. DejaVu Sans).
  import "@fontsource-variable/inter";
  let { children } = $props();
</script>

{@render children()}

<style>
  :global(:root) {
    font-family: "Inter Variable", "Segoe UI", system-ui, sans-serif;
    /* Kompakt wie die STARFACE-App; alle rem-Maße skalieren mit */
    font-size: 15px;
    color-scheme: dark;
    --bg: #1f2226;
    --bar: #16181b;
    --bar-2: #2a2d31;
    /* Kacheln im Arbeitsbereich: heller als der Hintergrund, dunkler als Listen */
    --tile: #26292d;
    --panel: #2b2f34;
    --panel-2: #373b41;
    --line: #3d4248;
    --text: #e8eaed;
    --muted: #a3a9b0;
    --accent: #f5a31a;
    --accent-soft: #f5a31a33;
    /* Text auf Akzent-Flächen; bei eigener Akzentfarbe hell oder dunkel je nach Farbe */
    --on-accent: #111;
    /* Akzentfarbe als Schrift; bei eigener Farbe so angepasst, dass sie auf dem Hintergrund lesbar bleibt */
    --accent-text: var(--accent);
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
    --tile: #f7f8fa;
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
