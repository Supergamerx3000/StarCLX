<script lang="ts">
  // Plugin „URL oder Programm bei Anruf“: Regeln in den Einstellungen.
  // Ausgeführt werden sie in Rust (src-tauri/src/plugins/callactions).
  import { invoke } from "@tauri-apps/api/core";
  import Icon from "../../Icon.svelte";
  import Toggle from "../../Toggle.svelte";
  import type { CallAction, Prefs } from "../../prefs.svelte";
  import { t } from "../../i18n.svelte";

  let { draft = $bindable(), onnotice }: { draft: Prefs; onnotice: (notice: string) => void } = $props();

  const triggers: { value: CallAction["trigger"]; label: string }[] = $derived([
    { value: "ringing", label: t("Bei eingehendem Anruf (klingelt)") },
    { value: "answered", label: t("Bei Annahme") },
    { value: "outgoing", label: t("Bei ausgehendem Anruf") },
  ]);

  function addCallAction() {
    draft.call_actions = [...draft.call_actions, { enabled: true, trigger: "ringing", filter: "", external_only: false, target: "" }];
  }

  /** Ziel mit einer Beispielnummer ausführen */
  async function testCallAction(target: string) {
    onnotice("");
    try {
      await invoke("call_action_run", { target, number: "+41441234567" });
    } catch (e) {
      onnotice(String(e));
    }
  }
</script>

<section id="callactions">
  <h3>{t("URL oder Programm bei Anruf")}</h3>
  <div class="card">
    {#each draft.call_actions as rule, i}
      <div class="rule">
        <div class="rulehead">
          <Toggle bind:checked={rule.enabled} label={t("Aktiv")} />
          <select bind:value={rule.trigger}>
            {#each triggers as tr}<option value={tr.value}>{tr.label}</option>{/each}
          </select>
          <input type="text" class="filter" bind:value={rule.filter} placeholder={t("z. B. +41* (leer = alle)")} />
          <label class="check"><input type="checkbox" bind:checked={rule.external_only} /> {t("nur externe")}</label>
          <button onclick={() => testCallAction(rule.target)} disabled={!rule.target.trim()}>{t("Testen")}</button>
          <button class="x" title={t("Entfernen")} onclick={() => (draft.call_actions = draft.call_actions.filter((_, j) => j !== i))}><Icon name="trash" size={18} /></button>
        </div>
        <input type="text" class="target" bind:value={rule.target} placeholder={t("https://crm.example/suche?nr=$(calleridCanonical) oder Programm")} />
      </div>
    {/each}
    <button class="add" onclick={addCallAction}>{t("Regel hinzufügen")}</button>
    <p class="small muted hint">{t("Variablen: $(callerid) = Nummer wie empfangen, $(calleridNational) = nationales Format, $(calleridCanonical) = internationales Format (+41…). Ziele mit „://“ öffnen im Browser, alles andere wird als Programm ohne Shell gestartet. „Testen“ verwendet +41441234567.")}</p>
    <label class="field">
      <span>{t("Eigene Landesvorwahl")}</span>
      <span class="cc">+<input type="text" inputmode="numeric" bind:value={draft.default_country_code} placeholder="41" /></span>
    </label>
  </div>
</section>

<style>
  section { padding-top: 0.8rem; }
  h3 { font-size: 1.05rem; margin: 0.6rem 0 0.7rem; }
  .card { background: var(--panel); border-radius: 4px; padding: 0.7rem 1rem; display: flex; flex-direction: column; gap: 0.2rem; }
  .small { font-size: 0.85rem; margin: -0.3rem 0 0.6rem; }
  .muted { color: var(--muted); margin: 0.3rem 0; }
  .x { background: none; border: none; padding: 0.2rem; color: var(--muted); display: grid; }
  .add { align-self: flex-start; margin-top: 0.7rem; }
  .field { display: flex; flex-direction: column; gap: 0.3rem; margin-top: 0.6rem; max-width: 34rem; }
  .rule { display: flex; flex-direction: column; gap: 0.4rem; padding: 0.5rem 0; border-bottom: 1px solid var(--line); }
  .rulehead { display: flex; flex-wrap: wrap; align-items: center; gap: 0.6rem; }
  .rule select, .rule input[type="text"], .cc input { padding: 0.3rem 0.5rem; background: var(--panel-2); color: inherit; border: 1px solid var(--line); border-radius: 4px; }
  .rule .filter { width: 13rem; }
  .rule .target { width: 100%; box-sizing: border-box; }
  .check { display: flex; align-items: center; gap: 0.4rem; cursor: pointer; }
  .check input { accent-color: var(--accent); }
  .hint { margin-top: 0.6rem; }
  .cc { display: flex; align-items: center; gap: 0.3rem; }
  .cc input { width: 4rem; }
</style>
