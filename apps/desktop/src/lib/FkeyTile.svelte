<script lang="ts">
  // Eine Funktionstaste mit Zustandsfarbe; im Arbeitsbereich und im Editor.
  import { account, callDrag, keyState, keyTitle, typeInfo, type FunctionKey } from "./fkeys.svelte";
  import { t } from "./i18n.svelte";

  let { key, onclick, disabled = false, editor = false }: { key: FunctionKey; onclick?: () => void; disabled?: boolean; editor?: boolean } = $props();

  // Leere Taste: im Betrieb nur die Fläche, ohne Text und nicht anklickbar
  const blank = $derived(key.functionKeyType === "SEPARATOR" && !editor);

  const info = $derived(typeInfo(key.functionKeyType));
  const state = $derived(keyState(key));
  const sub = $derived.by(() => {
    switch (key.functionKeyType) {
      case "BUSYLAMPFIELD": return account(key)?.number ?? "";
      case "QUICKDIAL": return key.directCallTargetnumber ?? "";
      case "PARKANDORBIT": return key.poNumber ? t("Platz {n}", { n: key.poNumber }) : "";
      default: return key.name ? info.label : "";
    }
  });
  const stateText: Record<string, string> = $derived({ on: t("aktiv"), busy: t("im Gespräch"), ringing: t("klingelt"), free: t("frei"), off: t("nicht erreichbar"), parked: t("Gespräch geparkt") });
</script>

<button
  class="tile {state}"
  class:sep={key.functionKeyType === "SEPARATOR" && editor}
  class:blank
  class:unusable={!info.usable && !blank}
  class:target={callDrag.call && callDrag.over === key.id}
  data-fkey={key.id}
  title={blank ? undefined : [info.label, stateText[state]].filter(Boolean).join(" · ")}
  disabled={disabled || blank}
  {onclick}
>
  <span class="lamp"></span>
  {#if !blank}<span class="txt"><strong>{keyTitle(key)}</strong>{#if sub}<small>{sub}</small>{/if}</span>{/if}
</button>

<style>
  .tile { width: 100%; min-height: 3.2rem; display: flex; align-items: center; gap: 0.6rem; text-align: left; padding: 0.4rem 0.6rem; background: var(--panel-2); border: 1px solid var(--line); border-radius: 6px; }
  .tile:hover:not(:disabled) { border-color: var(--accent); }
  .lamp { width: 0.7rem; height: 0.7rem; border-radius: 50%; flex: none; background: var(--line); }
  .free .lamp { background: var(--green); }
  .busy .lamp, .on .lamp { background: var(--red); }
  .on .lamp { background: var(--accent); }
  .ringing .lamp { background: var(--red); animation: blink 0.6s steps(2) infinite; }
  .parked .lamp { background: var(--accent); animation: blink 0.6s steps(2) infinite; }
  .off .lamp { background: transparent; border: 1px solid var(--muted); }
  .txt { min-width: 0; display: flex; flex-direction: column; }
  .txt strong { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .txt small { color: var(--muted); font-size: 0.78rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .unusable { opacity: 0.55; }
  .tile.target { border-color: var(--accent); outline: 2px solid var(--accent); }
  .sep { background: none; border-style: dashed; }
  .sep .lamp { visibility: hidden; }
  .blank { cursor: default; }
  .blank .lamp { visibility: hidden; }
  @keyframes blink { to { opacity: 0.2; } }
</style>
