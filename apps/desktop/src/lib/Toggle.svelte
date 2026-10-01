<script lang="ts">
  let {
    checked = $bindable(false),
    label,
    disabled = false,
    onchange,
  }: { checked?: boolean; label: string; disabled?: boolean; onchange?: (checked: boolean) => void } = $props();
</script>

<label class="toggle" class:disabled>
  <input type="checkbox" bind:checked {disabled} onchange={() => onchange?.(checked)} />
  <span class="track"><span class="knob">{#if checked}✓{/if}</span></span>
  <span>{label}</span>
</label>

<style>
  .toggle { display: flex; align-items: center; gap: 0.8rem; cursor: pointer; padding: 0.35rem 0; }
  .toggle.disabled { opacity: 0.5; cursor: default; }
  input { position: absolute; opacity: 0; pointer-events: none; }
  .track {
    flex: none; width: 2.3rem; height: 1.25rem; border-radius: 999px; background: var(--panel-2);
    border: 1px solid var(--line); position: relative; transition: background 0.15s;
  }
  .knob {
    position: absolute; top: 1px; left: 1px; width: 1.05rem; height: 1.05rem; border-radius: 50%;
    background: #fff; transition: left 0.15s; display: grid; place-items: center;
    font-size: 0.7rem; color: var(--accent); font-weight: 700;
  }
  input:checked + .track { background: var(--accent); border-color: var(--accent); }
  input:checked + .track .knob { left: calc(100% - 1.15rem); }
  input:focus-visible + .track { outline: 2px solid var(--accent); outline-offset: 2px; }
</style>
