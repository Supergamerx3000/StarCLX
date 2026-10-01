<script lang="ts">
  // Eine Funktionstaste mit Zustandsfarbe; im Arbeitsbereich und im Editor.
  import { account, keyState, keyTitle, typeInfo, type FunctionKey } from "./fkeys.svelte";

  let { key, onclick, disabled = false }: { key: FunctionKey; onclick?: () => void; disabled?: boolean } = $props();

  const info = $derived(typeInfo(key.functionKeyType));
  const state = $derived(keyState(key));
  const sub = $derived.by(() => {
    switch (key.functionKeyType) {
      case "BUSYLAMPFIELD": return account(key)?.number ?? "";
      case "QUICKDIAL": return key.directCallTargetnumber ?? "";
      case "PARKANDORBIT": return key.poNumber ? `Platz ${key.poNumber}` : "";
      default: return key.name ? info.label : "";
    }
  });
  const stateText: Record<string, string> = { on: "aktiv", busy: "im Gespräch", ringing: "klingelt", free: "frei", off: "nicht erreichbar" };
</script>

<button
  class="tile {state}"
  class:sep={key.functionKeyType === "SEPARATOR"}
  class:unusable={!info.usable}
  title={[info.label, stateText[state]].filter(Boolean).join(" · ")}
  {disabled}
  {onclick}
>
  <span class="lamp"></span>
  <span class="txt"><strong>{keyTitle(key)}</strong>{#if sub}<small>{sub}</small>{/if}</span>
</button>

<style>
  .tile { width: 100%; min-height: 3.2rem; display: flex; align-items: center; gap: 0.6rem; text-align: left; padding: 0.4rem 0.6rem; background: var(--panel-2); border: 1px solid var(--line); border-radius: 6px; }
  .tile:hover:not(:disabled) { border-color: var(--accent); }
  .lamp { width: 0.7rem; height: 0.7rem; border-radius: 50%; flex: none; background: var(--line); }
  .free .lamp { background: var(--green); }
  .busy .lamp, .on .lamp { background: var(--red); }
  .on .lamp { background: var(--accent); }
  .ringing .lamp { background: var(--red); animation: blink 0.6s steps(2) infinite; }
  .off .lamp { background: transparent; border: 1px solid var(--muted); }
  .txt { min-width: 0; display: flex; flex-direction: column; }
  .txt strong { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .txt small { color: var(--muted); font-size: 0.78rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .unusable { opacity: 0.55; }
  .sep { background: none; border-style: dashed; }
  .sep .lamp { visibility: hidden; }
  @keyframes blink { to { opacity: 0.2; } }
</style>
