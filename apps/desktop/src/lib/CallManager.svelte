<script lang="ts">
  import CallCard from "./CallCard.svelte";
  import Icon from "./Icon.svelte";
  import { isRingingIn, phone } from "./phone.svelte.js";
  import { t } from "./i18n.svelte.js";

  let open = $state(false);
  const calls = $derived(phone.status.calls);
  const ringing = $derived(calls.some(isRingingIn));

  // Bei neuen Anrufen aufklappen, ohne Anrufe zuklappen.
  let lastCount = 0;
  $effect(() => {
    if (calls.length > lastCount) open = true;
    if (calls.length === 0) open = false;
    lastCount = calls.length;
  });

</script>

<div class="cm">
  <button class="toggle" class:ringing onclick={() => (open = !open)} aria-expanded={open}>
    <span>Call Manager</span>
    <span class="count">{calls.length}</span>
    <span class="chev" class:open><Icon name="chevron" size={20} /></span>
  </button>
  {#if open}
    <div class="drop">
      {#each calls as call (call.id)}
        <CallCard {call} />
      {:else}
        <p class="empty">{t("Keine Gespräche")}</p>
      {/each}
      {#if phone.notice}
        <p class="notice">{phone.notice}</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .cm { position: relative; }
  .toggle {
    display: flex; align-items: center; gap: 0.75rem; min-width: 15rem;
    background: var(--bar-2); border: 1px solid var(--line); border-radius: 999px; padding: 0.45rem 0.6rem 0.45rem 1.1rem;
  }
  .toggle.ringing { border-color: var(--green); animation: pulse 1s infinite alternate; }
  @keyframes pulse { to { box-shadow: 0 0 0 4px #5cb82b55; } }
  .count { margin-left: auto; background: var(--panel-2); border-radius: 999px; padding: 0 0.55rem; font-size: 0.85rem; }
  .chev { display: grid; transition: transform 0.15s; transform: rotate(180deg); }
  .chev.open { transform: none; }
  .drop {
    position: absolute; right: 0; top: calc(100% + 0.4rem); z-index: 10; width: 26rem; max-height: calc(100vh - 5rem); overflow: auto;
    display: flex; flex-direction: column; gap: 1.2rem; padding: 0.9rem;
    background: var(--panel); border: 1px solid var(--line); border-radius: 8px; box-shadow: 0 8px 24px #000a;
  }
  .empty { margin: 0; color: var(--muted); }
  .notice { margin: 0; color: var(--accent); font-size: 0.9rem; }
</style>
