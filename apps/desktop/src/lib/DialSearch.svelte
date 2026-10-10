<script lang="ts">
  import { connection } from "./connection.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import Icon from "./Icon.svelte";
  import { phone, run, canDial } from "./plugins/call/phone.svelte";
  import type { Contact } from "./plugins/contacts/contacts";
  import { t } from "./i18n.svelte";

  let text = $state("");
  let results = $state<Contact[]>([]);
  let open = $state(false);
  let active = $state(-1);
  let dialing = $state(false);
  let searchError = $state("");
  let seq = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  // Alle wählbaren Nummern der Treffer, für die Pfeiltasten
  const flat = $derived(results.flatMap((c) => c.numbers.map((n) => ({ contact: c, ...n }))));
  const ready = $derived(canDial());

  function oninput() {
    clearTimeout(timer);
    active = -1;
    const term = text.trim();
    if (term.length < 2) {
      results = [];
      searchError = "";
      return;
    }
    const my = ++seq;
    timer = setTimeout(async () => {
      try {
        const r = await invoke<Contact[]>("contacts_search", { term });
        if (my === seq) {
          results = r;
          searchError = "";
          open = true;
        }
      } catch (e) {
        if (my === seq) searchError = String(e);
      }
    }, 250);
  }

  async function dial(number: string) {
    if (!number.trim() || dialing) return;
    dialing = true;
    if (await run("phone_dial", { number })) {
      text = "";
      results = [];
      open = false;
    }
    dialing = false;
  }

  function submit(event: Event) {
    event.preventDefault();
    if (active >= 0 && flat[active]) return dial(flat[active].number);
    // Mit Buchstaben im Feld die erste Nummer des ersten Treffers wählen
    if (/[a-zA-ZäöüÄÖÜ]/.test(text) && flat.length) return dial(flat[0].number);
    return dial(text);
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown" && flat.length) {
      e.preventDefault();
      open = true;
      active = (active + 1) % flat.length;
    } else if (e.key === "ArrowUp" && flat.length) {
      e.preventDefault();
      active = active <= 0 ? flat.length - 1 : active - 1;
    } else if (e.key === "Escape") {
      open = false;
      active = -1;
    }
  }
</script>

<form class="dial" onsubmit={submit}>
  <label class="search">
    <Icon name="search" size={20} />
    <input
      bind:value={text}
      {oninput}
      {onkeydown}
      onfocus={() => (open = true)}
      onblur={() => setTimeout(() => (open = false), 150)}
      placeholder={t("Name/Nummer eingeben")}
      autocomplete="off"
      spellcheck="false"
    />
  </label>
  <button class="dialbtn" class:armed={text.trim() && ready} type="submit" title={t("Anrufen")} disabled={!ready || dialing || !text.trim()}>
    <Icon name="call" />
  </button>
  {#if open && text.trim().length >= 2 && (results.length || (searchError && connection.online))}
    <div class="results">
      {#if searchError && connection.online}<p class="err">{t("Suche nicht möglich: {e}", { e: searchError })}</p>{/if}
      {#each results as c (c.id + c.name)}
        <div class="hit">
          <div class="who">
            <strong>{c.name || c.company || t("Ohne Namen")}</strong>
            {#if c.company && c.company !== c.name}<span>{c.company}</span>{/if}
          </div>
          <div class="nums">
            {#each c.numbers as n}
              {@const i = flat.findIndex((f) => f.contact === c && f.number === n.number)}
              <button type="button" class="num" class:active={i === active} disabled={!ready} onmousedown={(e) => e.preventDefault()} onclick={() => dial(n.number)} title={t("{label} anrufen", { label: t(n.label) })}>
                <Icon name="call" size={14} /><span class="lbl">{t(n.label)}</span>{n.number}
              </button>
            {:else}
              <span class="none">{t("Keine Nummer")}</span>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</form>

<style>
  .dial { position: relative; display: flex; align-items: center; gap: 0.5rem; }
  .search {
    display: flex; align-items: center; gap: 0.5rem; padding: 0 0.9rem;
    background: var(--bar-2); border: 1px solid var(--line); border-radius: 999px; width: 20rem; flex: 0 1 auto; min-width: 0;
  }
  .search:focus-within { border-color: var(--accent); }
  .search input { border: none; background: none; padding: 0.55rem 0; flex: 1; min-width: 0; outline: none; }
  .dialbtn {
    width: 2.6rem; height: 2.6rem; flex: none; padding: 0; border-radius: 50%; display: grid; place-items: center;
    background: var(--panel-2); border: none; color: var(--text);
  }
  .dialbtn.armed { background: var(--green); color: #fff; }
  .results {
    position: absolute; left: 0; top: calc(100% + 0.4rem); z-index: 12; width: min(30rem, calc(100vw - 1.5rem)); max-height: 70vh; overflow: auto;
    background: var(--panel); border: 1px solid var(--line); border-radius: 8px; box-shadow: 0 8px 24px #000a; padding: 0.3rem;
  }
  .hit { display: flex; flex-direction: column; gap: 0.3rem; padding: 0.5rem 0.6rem; border-bottom: 1px solid var(--line); }
  .hit:last-child { border-bottom: none; }
  .who { display: flex; gap: 0.6rem; align-items: baseline; }
  .who span { color: var(--muted); font-size: 0.85rem; }
  .nums { display: flex; flex-wrap: wrap; gap: 0.35rem; }
  .num { display: flex; align-items: center; gap: 0.35rem; padding: 0.25rem 0.6rem; border-radius: 999px; font-size: 0.85rem; background: var(--panel-2); border: 1px solid transparent; }
  .num:hover:not(:disabled), .num.active { border-color: var(--accent); }
  .lbl { color: var(--muted); }
  .none, .err { color: var(--muted); font-size: 0.85rem; margin: 0.3rem 0.6rem; }
</style>
