<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import Toggle from "./Toggle.svelte";
  import { loadPrefs, savePrefs, type Prefs } from "./prefs.svelte";

  type SignalingNumber = { id: string; number: string; suppressed: boolean; read_only: boolean; selected: boolean };

  let { onclose, onlogout }: { onclose: () => void; onlogout: () => void } = $props();

  let draft = $state<Prefs | null>(null);
  let numbers = $state<SignalingNumber[]>([]);
  let numbersError = $state("");
  let signaling = $state("");
  let initialSignaling = "";
  let saving = $state(false);
  let notice = $state("");
  let content: HTMLElement;

  const sections: { id: string; icon: IconName; label: string }[] = [
    { id: "softphone", icon: "call", label: "Softphone" },
    { id: "notifications", icon: "bell", label: "Benachrichtigungen" },
    { id: "audio", icon: "headset", label: "Audio" },
    { id: "ringtones", icon: "music", label: "Klingeltöne" },
    { id: "signaling", icon: "numbers", label: "Rufnummer signalisieren" },
    { id: "callmanager", icon: "forward", label: "Call Manager" },
    { id: "busylight", icon: "light", label: "Busylight" },
    { id: "account", icon: "account", label: "Konto" },
  ];

  onMount(async () => {
    draft = $state.snapshot(await loadPrefs());
    try {
      numbers = await invoke<SignalingNumber[]>("signaling_numbers");
      initialSignaling = signaling = numbers.find((n) => n.selected)?.id ?? "";
    } catch (e) {
      numbersError = String(e);
    }
  });

  function jump(id: string) {
    content.querySelector(`#${id}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  async function save() {
    if (!draft) return;
    saving = true;
    notice = "";
    try {
      if (signaling && signaling !== initialSignaling) {
        await invoke("set_signaling_number", { id: signaling });
        initialSignaling = signaling;
      }
      await savePrefs(draft);
      onclose();
    } catch (e) {
      notice = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="settings">
  <header>
    <button class="icon" title="Zurück" onclick={onclose}><Icon name="back" /></button>
    <h1>Einstellungen</h1>
  </header>
  <nav>
    <h2>Telefonie</h2>
    {#each sections as s}
      <button class="nav" onclick={() => jump(s.id)}><Icon name={s.icon} size={18} /><span>{s.label}</span></button>
    {/each}
  </nav>

  <div class="content" bind:this={content}>
    {#if !draft}
      <p class="muted">Lade …</p>
    {:else}
      <section id="softphone">
        <h3>Softphone</h3>
        <div class="card">
          <Toggle bind:checked={draft.softphone} label="Softphone verwenden" />
          <Toggle bind:checked={draft.primary_on_login} disabled={!draft.softphone} label="Softphone bei der Anmeldung am Server als primäres Telefon auswählen" />
          <Toggle bind:checked={draft.primary_on_answer} disabled={!draft.softphone} label="Bei Rufannahme das Softphone als primäres Telefon auswählen" />
        </div>
      </section>

      <section id="notifications">
        <h3>Benachrichtigungen</h3>
        <div class="card">
          <Toggle bind:checked={draft.notify_missed} label="Benachrichtigung über verpasste Anrufe anzeigen (ohne Gruppenanrufe)" />
          <Toggle bind:checked={draft.notify_missed_group} label="Benachrichtigung bei verpassten Gruppenanrufen anzeigen" />
        </div>
      </section>

      <section id="audio">
        <h3>Audio</h3>
        <div class="card">
          <p class="muted">Das Softphone nutzt die Standardgeräte des Systems (PipeWire). Die Geräteauswahl mit Reihenfolge folgt.</p>
        </div>
      </section>

      <section id="ringtones">
        <h3>Klingeltöne</h3>
        <div class="card">
          <Toggle bind:checked={draft.ringtone} label="Klingelton verwenden" />
          <p class="muted">Auswahl für intern und extern sowie eigene Klingeltöne folgen.</p>
        </div>
      </section>

      <section id="signaling">
        <h3>Rufnummer signalisieren</h3>
        <div class="card">
          {#if numbersError}
            <p class="notice">Rufnummern nicht geladen: {numbersError}</p>
          {:else if numbers.length === 0}
            <p class="muted">Die Anlage bietet keine Auswahl an.</p>
          {:else}
            {#each numbers as n (n.id)}
              <label class="radio" class:disabled={n.read_only}>
                <input type="radio" name="signaling" value={n.id} bind:group={signaling} disabled={n.read_only} />
                <span>{n.suppressed ? "Nummer unterdrücken" : n.number}</span>
              </label>
            {/each}
          {/if}
        </div>
      </section>

      <section id="callmanager">
        <h3>Call Manager</h3>
        <div class="card">
          <Toggle bind:checked={draft.bring_to_front} label="Beim Empfang eines Anrufs die App in den Vordergrund bringen" />
        </div>
      </section>

      <section id="busylight">
        <h3>Busylight</h3>
        <div class="card">
          <p class="muted">Unterstützung für Kuando Busylight folgt.</p>
        </div>
      </section>

      <section id="account">
        <h3>Konto</h3>
        <div class="card">
          <button class="logout" onclick={onlogout}><Icon name="logout" size={18} /> Abmelden</button>
        </div>
      </section>
    {/if}
  </div>

  <footer>
    {#if notice}<p class="notice">{notice}</p>{/if}
    <button class="primary" onclick={save} disabled={!draft || saving}>{saving ? "Speichere …" : "Speichern"}</button>
    <button onclick={onclose}>Abbrechen</button>
  </footer>
</div>

<style>
  .settings {
    position: fixed; inset: 0; z-index: 20; background: var(--bg);
    display: grid; grid-template-columns: 15rem 1fr; grid-template-rows: auto 1fr auto;
  }
  header { grid-column: 1 / -1; display: flex; align-items: center; gap: 0.5rem; padding: 0.4rem 0.6rem; background: var(--bar); }
  header h1 { flex: 1; text-align: center; margin: 0; font-size: 1rem; font-weight: 600; padding-right: 2.5rem; }
  .icon { background: none; border: none; padding: 0.3rem; display: grid; }
  nav { background: var(--panel); padding: 1rem 0.8rem; display: flex; flex-direction: column; gap: 0.1rem; overflow: auto; }
  nav h2 { font-size: 1.05rem; margin: 0.4rem 0.5rem 0.9rem; }
  .nav { display: flex; align-items: center; gap: 0.6rem; background: none; border: none; text-align: left; padding: 0.45rem 0.5rem; }
  .nav:hover { background: var(--panel-2); }
  .content { overflow: auto; padding: 0.5rem 1.2rem 2rem; scroll-behavior: smooth; }
  section { padding-top: 0.8rem; }
  h3 { font-size: 1.05rem; margin: 0.6rem 0 0.7rem; }
  .card { background: var(--panel); border-radius: 4px; padding: 0.7rem 1rem; display: flex; flex-direction: column; gap: 0.2rem; }
  .radio { display: flex; align-items: center; gap: 0.7rem; padding: 0.35rem 0; cursor: pointer; }
  .radio.disabled { opacity: 0.5; }
  .radio input { accent-color: var(--accent); width: 1.1rem; height: 1.1rem; margin: 0; }
  .muted { color: var(--muted); margin: 0.3rem 0; }
  .notice { color: var(--accent); margin: 0; flex: 1; }
  .logout { align-self: flex-start; display: flex; align-items: center; gap: 0.5rem; }
  footer {
    grid-column: 1 / -1; display: flex; justify-content: flex-end; align-items: center; gap: 1rem;
    padding: 0.8rem 1.2rem; border-top: 1px solid var(--line); background: var(--bg);
  }
  footer button { min-width: 8rem; }
  .primary { background: var(--accent); border-color: var(--accent); color: #111; font-weight: 600; }
  @media (max-width: 700px) {
    .settings { grid-template-columns: 1fr; }
    nav { display: none; }
  }
</style>
