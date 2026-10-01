<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import DeviceList, { DEFAULT, mergeOrder, type Device } from "./DeviceList.svelte";
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
  let devices = $state<{ speakers: Device[]; microphones: Device[] }>({ speakers: [], microphones: [] });
  let builtinTones = $state<string[]>([]);
  let micLevel = $state(0);
  let micTesting = $state(false);
  let playing = $state<string | null>(null);
  let busylight = $state<{ devices: string[]; error: string | null; tones: string[] }>({ devices: [], error: null, tones: [] });
  let blTesting = $state(false);

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

  onMount(() => {
    const off = listen<number>("mic-level", (e) => (micLevel = e.payload));
    init();
    return () => {
      invoke("audio_stop");
      off.then((f) => f());
    };
  });

  const firstPresent = (order: string[], devs: Device[]) =>
    order.find((n) => n === DEFAULT || devs.some((d) => d.name === n)) ?? DEFAULT;
  const fileName = (path: string) => path.split("/").pop() ?? path;

  async function preview(name: string | null) {
    if (!draft) return;
    const key = name ?? "@test";
    if (playing === key) {
      playing = null;
      return invoke("audio_stop");
    }
    playing = key;
    const order = name ? draft.ring_devices : draft.speakers;
    await invoke("audio_preview", { name, device: firstPresent(order, devices.speakers) });
    setTimeout(() => playing === key && (playing = null), name ? 3000 : 1000);
  }

  async function toggleMic() {
    if (!draft) return;
    micTesting = !micTesting;
    micLevel = 0;
    if (micTesting) await invoke("mic_test", { device: firstPresent(draft.microphones, devices.microphones) });
    else await invoke("audio_stop");
  }

  async function addRingtone() {
    if (!draft) return;
    try {
      const path = await invoke<string | null>("pick_ringtone");
      if (path && !draft.custom_ringtones.includes(path)) draft.custom_ringtones = [...draft.custom_ringtones, path];
    } catch (e) {
      notice = `Klingelton nicht übernommen: ${e}`;
    }
  }

  function removeRingtone(path: string) {
    if (!draft) return;
    draft.custom_ringtones = draft.custom_ringtones.filter((p) => p !== path);
    if (draft.ringtone_internal === path) draft.ringtone_internal = builtinTones[0];
    if (draft.ringtone_external === path) draft.ringtone_external = builtinTones[0];
  }

  async function init() {
    draft = $state.snapshot(await loadPrefs());
    try {
      const info = await invoke<{ devices: typeof devices; ringtones: string[] }>("audio_info");
      devices = info.devices;
      builtinTones = info.ringtones;
      draft.speakers = mergeOrder(draft.speakers, devices.speakers);
      draft.microphones = mergeOrder(draft.microphones, devices.microphones);
      draft.ring_devices = mergeOrder(draft.ring_devices, devices.speakers);
    } catch (e) {
      notice = `Audiogeräte nicht gelesen: ${e}`;
    }
    invoke<typeof busylight>("busylight_info").then((b) => (busylight = b), () => {});
    try {
      numbers = await invoke<SignalingNumber[]>("signaling_numbers");
      initialSignaling = signaling = numbers.find((n) => n.selected)?.id ?? "";
    } catch (e) {
      numbersError = String(e);
    }
  }

  async function testBusylight() {
    if (!draft) return;
    blTesting = true;
    try {
      await invoke("busylight_test", { sound: draft.busylight_sound, volume: draft.busylight_volume });
    } finally {
      blTesting = false;
    }
  }

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
        <p class="muted small">Die Geräte werden in der aufgelisteten Reihenfolge verwendet: das erste angeschlossene Gerät gewinnt. Änderungen gelten nach dem Speichern, das Softphone startet dann neu.</p>
        <div class="card">
          <h4>Lautsprecher</h4>
          <button class="play" onclick={() => preview(null)}><Icon name={playing === "@test" ? "pause" : "play"} size={18} /> Testton abspielen</button>
          <DeviceList bind:order={draft.speakers} devices={devices.speakers} />
          <hr />
          <h4>Mikrofon</h4>
          <div class="mic">
            <button class="play" onclick={toggleMic}>{micTesting ? "Test beenden" : "Mikrofon testen"}</button>
            <div class="meter"><div class="bar" style="width: {Math.round(micLevel * 100)}%"></div></div>
          </div>
          <DeviceList bind:order={draft.microphones} devices={devices.microphones} />
        </div>
      </section>

      <section id="ringtones">
        <h3>Klingeltöne</h3>
        <div class="card">
          <Toggle bind:checked={draft.ringtone} label="Klingelton verwenden" />
          <div class="tones" class:off={!draft.ringtone}>
            <div class="tone head"><span>Intern</span><span>Extern</span></div>
            {#each [...builtinTones, ...draft.custom_ringtones] as t (t)}
              <div class="tone">
                <input type="radio" name="tone-int" value={t} bind:group={draft.ringtone_internal} title="Interne Anrufe" />
                <input type="radio" name="tone-ext" value={t} bind:group={draft.ringtone_external} title="Externe Anrufe" />
                <button class="round" title="Anhören" onclick={() => preview(t)}><Icon name={playing === t ? "pause" : "play"} size={16} /></button>
                <span class="tname">{fileName(t)}</span>
                {#if draft.custom_ringtones.includes(t)}
                  <button class="x" title="Entfernen" onclick={() => removeRingtone(t)}><Icon name="close" size={16} /></button>
                {/if}
              </div>
            {/each}
            <button class="add" onclick={addRingtone}>Eigenen Klingelton hinzufügen (WAV)</button>
          </div>
          <hr />
          <h4>Ausgabegerät zum Klingeln</h4>
          <DeviceList bind:order={draft.ring_devices} devices={devices.speakers} />
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
          <Toggle bind:checked={draft.busylight} label="Kuando Busylight verwenden" />
          <p class="small muted">Grün: frei · Rot: im Gespräch · Rot blinkend: eingehender Anruf</p>
          <p class="muted">
            {#if busylight.devices.length}
              Angeschlossen: {busylight.devices.length === 1 ? "1 Gerät" : `${busylight.devices.length} Geräte`}
            {:else}
              Kein Busylight angeschlossen.
            {/if}
          </p>
          {#if busylight.error}<p class="notice">{busylight.error}</p>{/if}
          <div class="bl" class:off={!draft.busylight}>
            <label>
              <span>Ton bei Anruf</span>
              <select bind:value={draft.busylight_sound}>
                <option value="">Kein Ton</option>
                {#each busylight.tones as t}<option value={t}>{t}</option>{/each}
              </select>
            </label>
            <label>
              <span>Lautstärke</span>
              <input type="range" min="0" max="100" step="5" bind:value={draft.busylight_volume} disabled={!draft.busylight_sound} />
              <span class="vol">{draft.busylight_volume} %</span>
            </label>
            <button class="play" onclick={testBusylight} disabled={blTesting || !busylight.devices.length}>
              <Icon name="light" size={18} /> {blTesting ? "Teste …" : "Testen"}
            </button>
          </div>
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
  h4 { margin: 0.3rem 0; font-size: 0.98rem; }
  hr { border: none; border-top: 1px solid var(--line); margin: 0.8rem 0 0.4rem; width: 100%; }
  .small { font-size: 0.85rem; margin: -0.3rem 0 0.6rem; }
  .play { align-self: flex-start; display: flex; align-items: center; gap: 0.4rem; }
  .mic { display: flex; align-items: center; gap: 1rem; }
  .meter { flex: 1; max-width: 18rem; height: 0.4rem; background: var(--panel-2); border-radius: 999px; overflow: hidden; }
  .bar { height: 100%; background: var(--green); transition: width 0.08s; }
  .tones { display: flex; flex-direction: column; }
  .tones.off { opacity: 0.5; }
  .tone { display: grid; grid-template-columns: 3.2rem 3.2rem 2.4rem 1fr auto; align-items: center; padding: 0.3rem 0; border-bottom: 1px solid var(--line); }
  .tone.head { font-size: 0.8rem; color: var(--muted); border: none; padding-bottom: 0; }
  .tone input { accent-color: var(--accent); width: 1.1rem; height: 1.1rem; margin: 0 0 0 0.6rem; }
  .round { width: 1.9rem; height: 1.9rem; padding: 0; border-radius: 50%; display: grid; place-items: center; }
  .tname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .x { background: none; border: none; padding: 0.2rem; color: var(--muted); display: grid; }
  .add { align-self: flex-start; margin-top: 0.7rem; }
  .radio { display: flex; align-items: center; gap: 0.7rem; padding: 0.35rem 0; cursor: pointer; }
  .radio.disabled { opacity: 0.5; }
  .radio input { accent-color: var(--accent); width: 1.1rem; height: 1.1rem; margin: 0; }
  .muted { color: var(--muted); margin: 0.3rem 0; }
  .notice { color: var(--accent); margin: 0; flex: 1; }
  .bl { display: flex; flex-direction: column; gap: 0.6rem; margin-top: 0.4rem; }
  .bl.off { opacity: 0.5; }
  .bl label { display: grid; grid-template-columns: 8rem minmax(0, 16rem) auto; align-items: center; gap: 0.8rem; }
  .bl select { padding: 0.3rem; background: var(--panel-2); color: inherit; border: 1px solid var(--line); border-radius: 4px; }
  .bl input[type="range"] { accent-color: var(--accent); }
  .vol { color: var(--muted); font-size: 0.85rem; }
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
