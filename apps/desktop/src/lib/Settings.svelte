<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import DeviceList, { DEFAULT, mergeOrder, type Device } from "./DeviceList.svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import FkeyEditor from "./FkeyEditor.svelte";
  import Reach from "./Reach.svelte";
  import Toggle from "./Toggle.svelte";
  import { type CallAction, type Hotkeys, loadPrefs, prefs, savePrefs, type Prefs } from "./prefs.svelte";
  import { setLanguage, t } from "./i18n.svelte";

  type SignalingNumber = { id: string; number: string; suppressed: boolean; read_only: boolean; selected: boolean };

  let { onclose, onlogout, server = "" }: { onclose: () => void; onlogout: () => void; server?: string } = $props();

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

  const sections: { id: string; icon: IconName; label: string }[] = $derived([
    { id: "softphone", icon: "call", label: "Softphone" },
    { id: "notifications", icon: "bell", label: t("Benachrichtigungen") },
    { id: "audio", icon: "headset", label: t("Audio") },
    { id: "ringtones", icon: "music", label: t("Klingeltöne") },
    { id: "signaling", icon: "numbers", label: t("Rufnummer signalisieren") },
    { id: "callmanager", icon: "forward", label: "Call Manager" },
    { id: "callactions", icon: "call2go", label: t("URL oder Programm bei Anruf") },
    { id: "busylight", icon: "light", label: "Busylight" },
  ]);
  const chatSections: { id: string; icon: IconName; label: string }[] = $derived([
    { id: "chat-notify", icon: "bell", label: t("Benachrichtigungen") },
    { id: "chat-files", icon: "folder", label: t("Dateien empfangen") },
    { id: "chat-status", icon: "person", label: t("Status") },
  ]);
  let defaultDownloads = $state("");
  const reachSections: { id: string; icon: IconName; label: string }[] = $derived([
    { id: "voicemail", icon: "voicemail", label: "Voicemail" },
    { id: "redirects", icon: "forward", label: t("Umleitungen") },
    { id: "fmc", icon: "call2go", label: t("Parallelruf") },
    { id: "fkeys", icon: "dialpad", label: t("Funktionstasten") },
  ]);
  const personalSections: { id: string; icon: IconName; label: string }[] = $derived([
    { id: "appearance", icon: "workspace", label: t("Darstellung") },
    { id: "hotkeys", icon: "dialpad", label: t("Hotkeys") },
    { id: "integration", icon: "call", label: t("Desktop-Integration") },
  ]);
  const triggers: { value: CallAction["trigger"]; label: string }[] = $derived([
    { value: "ringing", label: t("Bei eingehendem Anruf (klingelt)") },
    { value: "answered", label: t("Bei Annahme") },
    { value: "outgoing", label: t("Bei ausgehendem Anruf") },
  ]);

  function addCallAction() {
    if (draft) draft.call_actions = [...draft.call_actions, { enabled: true, trigger: "ringing", filter: "", external_only: false, target: "" }];
  }

  /** Ziel mit einer Beispielnummer ausführen */
  async function testCallAction(target: string) {
    notice = "";
    try {
      await invoke("call_action_run", { target, number: "+41441234567" });
    } catch (e) {
      notice = String(e);
    }
  }
  const hotkeyRows: { key: Exclude<keyof Hotkeys, "enabled">; label: string }[] = $derived([
    { key: "dial_selection", label: t("Markierte Rufnummer wählen") },
    { key: "dial_clipboard", label: t("Rufnummer aus Zwischenablage wählen") },
    { key: "answer", label: t("Softphone-Anruf annehmen") },
    { key: "hangup", label: t("Aktuellen Anruf beenden") },
    { key: "toggle_view", label: t("Ansicht umschalten") },
  ]);
  let desktop = $state({ wayland: false, gnome: false, command: "" });
  let recording = $state<string | null>(null);

  /** `<Control><Shift>w` → `Strg+Umschalt+W` */
  function showAccel(a: string) {
    if (!a) return t("Keine");
    const names: Record<string, string> = { Control: t("Strg"), Shift: t("Umschalt"), Alt: "Alt", Super: "Super" };
    const mods = [...a.matchAll(/<(\w+)>/g)].map((m) => names[m[1]] ?? m[1]);
    const key = a.replace(/<\w+>/g, "");
    return [...mods, key.length === 1 ? key.toUpperCase() : key].join("+");
  }

  /** Tastendruck im GTK-Format aufnehmen; Esc bricht ab, Entf löscht. */
  function recordKey(e: KeyboardEvent, key: Exclude<keyof Hotkeys, "enabled">) {
    if (!draft) return;
    e.preventDefault();
    if (["Control", "Shift", "Alt", "Meta", "AltGraph"].includes(e.key)) return;
    if (e.key === "Escape") return void (recording = null);
    if (e.key === "Delete" || e.key === "Backspace") {
      draft.hotkeys[key] = "";
      return void (recording = null);
    }
    let name = e.code.startsWith("Key") ? e.code.slice(3).toLowerCase()
      : e.code.startsWith("Digit") ? e.code.slice(5)
      : /^F\d+$/.test(e.key) ? e.key
      : e.key.length === 1 ? e.key.toLowerCase() : e.key;
    const mods = (e.ctrlKey ? "<Control>" : "") + (e.shiftKey ? "<Shift>" : "") + (e.altKey ? "<Alt>" : "") + (e.metaKey ? "<Super>" : "");
    if (!mods && !/^F\d+$/.test(name)) return; // ohne Zusatztaste würde sie überall fehlen
    draft.hotkeys[key] = mods + name;
    recording = null;
  }

  let version = $state("");
  getVersion().then((v) => (version = v), () => {});

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
      notice = t("Klingelton nicht übernommen: {e}", { e: String(e) });
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
      notice = t("Audiogeräte nicht gelesen: {e}", { e: String(e) });
    }
    invoke<typeof desktop>("desktop_info").then((d) => (desktop = d), () => {});
    invoke<string>("default_download_dir").then((d) => (defaultDownloads = d), () => {});
    invoke<typeof busylight>("busylight_info").then((b) => (busylight = b), () => {});
    try {
      numbers = await invoke<SignalingNumber[]>("signaling_numbers");
      initialSignaling = signaling = numbers.find((n) => n.selected)?.id ?? "";
    } catch (e) {
      numbersError = String(e);
    }
  }

  async function pickDownloadDir() {
    if (!draft) return;
    try {
      const dir = await invoke<string | null>("pick_download_dir");
      if (dir) draft.download_dir = dir;
    } catch (e) {
      notice = String(e);
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

  /** Schliessen ohne Speichern: probeweise gewählte Sprache zurücknehmen */
  function cancel() {
    setLanguage(prefs.value?.language);
    onclose();
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
    <button class="icon" title={t("Zurück")} onclick={cancel}><Icon name="back" /></button>
    <h1>{t("Einstellungen")}</h1>
  </header>
  <nav>
    <h2>{t("Telefonie")}</h2>
    {#each sections as s}
      <button class="nav" onclick={() => jump(s.id)}><Icon name={s.icon} size={18} /><span>{s.label}</span></button>
    {/each}
    <h2>{t("Erreichbarkeit")}</h2>
    {#each reachSections as s}
      <button class="nav" onclick={() => jump(s.id)}><Icon name={s.icon} size={18} /><span>{s.label}</span></button>
    {/each}
    <h2>Chat</h2>
    {#each chatSections as s}
      <button class="nav" onclick={() => jump(s.id)}><Icon name={s.icon} size={18} /><span>{s.label}</span></button>
    {/each}
    <h2>{t("Personalisierung")}</h2>
    {#each personalSections as s}
      <button class="nav" onclick={() => jump(s.id)}><Icon name={s.icon} size={18} /><span>{s.label}</span></button>
    {/each}
    <h2>{t("Konto")}</h2>
    <button class="nav" onclick={() => jump("account")}><Icon name="account" size={18} /><span>{t("Konto")}</span></button>
  </nav>

  <div class="content" bind:this={content}>
    {#if !draft}
      <p class="muted">{t("Lade …")}</p>
    {:else}
      <section id="softphone">
        <h3>Softphone</h3>
        <div class="card">
          <Toggle bind:checked={draft.softphone} label={t("Softphone verwenden")} />
          <Toggle bind:checked={draft.primary_on_login} disabled={!draft.softphone} label={t("Softphone bei der Anmeldung am Server als primäres Telefon auswählen")} />
          <Toggle bind:checked={draft.primary_on_answer} disabled={!draft.softphone} label={t("Bei Rufannahme das Softphone als primäres Telefon auswählen")} />
        </div>
      </section>

      <section id="notifications">
        <h3>{t("Benachrichtigungen")}</h3>
        <div class="card">
          <Toggle bind:checked={draft.notify_missed} label={t("Benachrichtigung über verpasste Anrufe anzeigen (ohne Gruppenanrufe)")} />
          <Toggle bind:checked={draft.notify_missed_group} label={t("Benachrichtigung bei verpassten Gruppenanrufen anzeigen")} />
        </div>
      </section>

      <section id="audio">
        <h3>{t("Audio")}</h3>
        <p class="muted small">{t("Die Geräte werden in der aufgelisteten Reihenfolge verwendet: das erste angeschlossene Gerät gewinnt. Änderungen gelten nach dem Speichern, das Softphone startet dann neu.")}</p>
        <div class="card">
          <h4>{t("Lautsprecher")}</h4>
          <button class="play" onclick={() => preview(null)}><Icon name={playing === "@test" ? "pause" : "play"} size={18} /> {t("Testton abspielen")}</button>
          <DeviceList bind:order={draft.speakers} devices={devices.speakers} />
          <hr />
          <h4>{t("Mikrofon")}</h4>
          <div class="mic">
            <button class="play" onclick={toggleMic}>{micTesting ? t("Test beenden") : t("Mikrofon testen")}</button>
            <div class="meter"><div class="bar" style="width: {Math.round(micLevel * 100)}%"></div></div>
          </div>
          <DeviceList bind:order={draft.microphones} devices={devices.microphones} />
        </div>
      </section>

      <section id="ringtones">
        <h3>{t("Klingeltöne")}</h3>
        <div class="card">
          <Toggle bind:checked={draft.ringtone} label={t("Klingelton verwenden")} />
          <div class="tones" class:off={!draft.ringtone}>
            <div class="tone head"><span>{t("Intern")}</span><span>{t("Extern")}</span></div>
            {#each [...builtinTones, ...draft.custom_ringtones] as tone (tone)}
              <div class="tone">
                <input type="radio" name="tone-int" value={tone} bind:group={draft.ringtone_internal} title={t("Interne Anrufe")} />
                <input type="radio" name="tone-ext" value={tone} bind:group={draft.ringtone_external} title={t("Externe Anrufe")} />
                <button class="round" title={t("Anhören")} onclick={() => preview(tone)}><Icon name={playing === tone ? "pause" : "play"} size={16} /></button>
                <span class="tname">{fileName(tone)}</span>
                {#if draft.custom_ringtones.includes(tone)}
                  <button class="x" title={t("Entfernen")} onclick={() => removeRingtone(tone)}><Icon name="close" size={16} /></button>
                {/if}
              </div>
            {/each}
            <button class="add" onclick={addRingtone}>{t("Eigenen Klingelton hinzufügen (WAV)")}</button>
          </div>
          <hr />
          <h4>{t("Ausgabegerät zum Klingeln")}</h4>
          <DeviceList bind:order={draft.ring_devices} devices={devices.speakers} />
        </div>
      </section>

      <section id="signaling">
        <h3>{t("Rufnummer signalisieren")}</h3>
        <div class="card">
          {#if numbersError}
            <p class="notice">{t("Rufnummern nicht geladen: {e}", { e: numbersError })}</p>
          {:else if numbers.length === 0}
            <p class="muted">{t("Die Anlage bietet keine Auswahl an.")}</p>
          {:else}
            {#each numbers as n (n.id)}
              <label class="radio" class:disabled={n.read_only}>
                <input type="radio" name="signaling" value={n.id} bind:group={signaling} disabled={n.read_only} />
                <span>{n.suppressed ? t("Nummer unterdrücken") : n.number}</span>
              </label>
            {/each}
          {/if}
        </div>
      </section>

      <section id="callmanager">
        <h3>Call Manager</h3>
        <div class="card">
          <Toggle bind:checked={draft.bring_to_front} label={t("Beim Empfang eines Anrufs die App in den Vordergrund bringen")} />
        </div>
      </section>

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
                <button class="x" title={t("Entfernen")} onclick={() => draft && (draft.call_actions = draft.call_actions.filter((_, j) => j !== i))}><Icon name="trash" size={18} /></button>
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

      <section id="busylight">
        <h3>Busylight</h3>
        <div class="card">
          <Toggle bind:checked={draft.busylight} label={t("Kuando Busylight verwenden")} />
          <p class="small muted">{t("Grün: frei · Rot: im Gespräch · Rot blinkend: eingehender Anruf")}</p>
          <p class="muted">
            {#if busylight.devices.length}
              {t("Angeschlossen:")} {busylight.devices.length === 1 ? t("1 Gerät") : t("{n} Geräte", { n: busylight.devices.length })}
            {:else}
              {t("Kein Busylight angeschlossen.")}
            {/if}
          </p>
          {#if busylight.error}<p class="notice">{busylight.error}</p>{/if}
          <div class="bl" class:off={!draft.busylight}>
            <label>
              <span>{t("Ton bei Anruf")}</span>
              <select bind:value={draft.busylight_sound}>
                <option value="">{t("Kein Ton")}</option>
                {#each busylight.tones as tone}<option value={tone}>{tone}</option>{/each}
              </select>
            </label>
            <label>
              <span>{t("Lautstärke")}</span>
              <input type="range" min="0" max="100" step="5" bind:value={draft.busylight_volume} disabled={!draft.busylight_sound} />
              <span class="vol">{draft.busylight_volume} %</span>
            </label>
            <button class="play" onclick={testBusylight} disabled={blTesting || !busylight.devices.length}>
              <Icon name="light" size={18} /> {blTesting ? t("Teste …") : t("Testen")}
            </button>
          </div>
        </div>
      </section>

      <Reach {server} />

      <section id="fkeys">
        <h3>{t("Funktionstasten")}</h3>
        <div class="card">
          <FkeyEditor bind:columns={draft.fkey_columns} />
          <p class="small muted">{t("Tasten werden sofort auf der Anlage gespeichert; die Spaltenzahl mit „Speichern“.")}</p>
        </div>
      </section>

      <section id="chat-notify">
        <h3>{t("Chat: Benachrichtigungen")}</h3>
        <div class="card">
          <Toggle bind:checked={draft.chat_notify} label={t("Benachrichtigung bei neuer Chatnachricht anzeigen")} />
          <Toggle bind:checked={draft.chat_sound} label={t("Ton bei neuer Chatnachricht abspielen")} />
        </div>
      </section>

      <section id="chat-files">
        <h3>{t("Dateien empfangen")}</h3>
        <div class="card">
          <p class="muted">{t("Empfangene Dateien speichern unter")}</p>
          <div class="path">
            <input type="text" bind:value={draft.download_dir} placeholder={defaultDownloads || "Downloads"} />
            <button onclick={pickDownloadDir}>{t("Suchen")}</button>
          </div>
          <p class="small muted">{t("Leer lassen für den Standardordner. Der Dateiempfang im Chat folgt in einem späteren Schritt.")}</p>
        </div>
      </section>

      <section id="chat-status">
        <h3>{t("Status")}</h3>
        <div class="card">
          <Toggle bind:checked={draft.away_on_idle} label={t("Bei Inaktivität (10 Minuten) Status auf „Abwesend“ setzen")} />
          <Toggle bind:checked={draft.away_on_screensaver} label={t("Bei aktivem Bildschirmschoner Status auf „Abwesend“ setzen")} />
          <Toggle bind:checked={draft.away_on_lock} label={t("Bei gesperrtem Bildschirm Status auf „Abwesend“ setzen")} />
          <label class="field">
            <span>{t("Statustext bei automatischer Abwesenheit")}</span>
            <input type="text" bind:value={draft.away_text} placeholder={t("z. B. Bin gleich zurück")} />
          </label>
          <label class="field">
            <span>{t("Statustext beim Abmelden")}</span>
            <input type="text" bind:value={draft.offline_text} placeholder={t("z. B. Feierabend")} />
          </label>
        </div>
      </section>

      <section id="appearance">
        <h3>{t("Darstellung")}</h3>
        <div class="card">
          <h4>{t("Erscheinungsbild")}</h4>
          {#each [["dark", t("Dunkel")], ["light", t("Hell")], ["system", t("System")]] as [value, label]}
            <label class="radio"><input type="radio" name="theme" {value} bind:group={draft.theme} /> {label}</label>
          {/each}
          <hr />
          <label class="field">
            <span>{t("Sprache")}</span>
            <!-- Wirkt sofort zur Vorschau; gespeichert wird mit „Speichern“ -->
            <select bind:value={draft.language} onchange={() => setLanguage(draft?.language)}>
              <option value="de">Deutsch</option>
              <option value="en">English</option>
              <option value="fr">Français</option>
              <option value="it">Italiano</option>
            </select>
          </label>
          <hr />
          <label class="field">
            <span>{t("Arbeitsbereich")}</span>
            <select bind:value={draft.workspace}>
              <option value="tabs">{t("Reiter")}</option>
              <option value="free">{t("Frei anordnen")}</option>
            </select>
          </label>
          {#if draft.workspace === "free"}
            <p class="small muted">{t("Kacheln an der Titelleiste verschieben und an der Ecke unten rechts in der Grösse ändern. Über die Leiste oben blendest du Kacheln ein und aus.")}</p>
            <button class="add" onclick={() => draft && (draft.workspace_tiles = null)}>{t("Anordnung zurücksetzen")}</button>
          {/if}
          <hr />
          <Toggle bind:checked={draft.autostart} label={t("Beim Anmelden am Rechner starten")} />
          <Toggle bind:checked={draft.start_minimized} label={t("Programm minimiert starten")} />
          <Toggle bind:checked={draft.minimize_to_tray} label={t("Beim Minimieren nur als Symbol im Infobereich anzeigen")} />
          <Toggle bind:checked={draft.always_on_top} label={t("Immer im Vordergrund")} />
          {#if desktop.wayland && draft.always_on_top}
            <p class="small muted">{t("Unter Wayland bestimmt der Desktop, ob ein Fenster oben bleibt. Bei GNOME geht es über Alt+Leertaste → „Immer im Vordergrund“.")}</p>
          {/if}
        </div>
      </section>

      <section id="hotkeys">
        <h3>{t("Hotkeys")}</h3>
        <div class="card">
          {#if desktop.gnome}
            <Toggle bind:checked={draft.hotkeys.enabled} label={t("Tastenkürzel systemweit in GNOME eintragen")} />
            <p class="small muted">{t("Die Kürzel gelten dann in allen Programmen und überschreiben dort gleiche Kombinationen. Andere eigene Tastenkürzel bleiben unverändert.")}</p>
          {:else}
            <p class="muted">{t("Dieser Desktop erlaubt Programmen keine globalen Tastenkürzel. Lege in den Systemeinstellungen eine eigene Tastenkombination mit diesem Befehl an:")}</p>
            <code>{desktop.command}</code>
            <p class="small muted">{t("Aktionen:")} dial-selection, dial-clipboard, answer, hangup, toggle-view</p>
          {/if}
          <div class="keys" class:off={desktop.gnome && !draft.hotkeys.enabled}>
            {#each hotkeyRows as r}
              <div class="keyrow">
                <span>{r.label}</span>
                <button class="key" class:rec={recording === r.key} onclick={() => (recording = r.key)} onkeydown={(e) => recording === r.key && recordKey(e, r.key)} onblur={() => recording === r.key && (recording = null)}>
                  {recording === r.key ? t("Tasten drücken …") : showAccel(draft.hotkeys[r.key])}
                </button>
                <button class="x" title={t("Entfernen")} onclick={() => draft && (draft.hotkeys[r.key] = "")} disabled={!draft.hotkeys[r.key]}><Icon name="close" size={18} /></button>
              </div>
            {/each}
          </div>
        </div>
      </section>

      <section id="integration">
        <h3>{t("Desktop-Integration")}</h3>
        <div class="card">
          <Toggle bind:checked={draft.handle_tel_links} label={t("Rufnummern-Links (tel:, callto:, sip:) mit StarCLX öffnen")} />
          <p class="small muted">{t("Ein Klick auf eine Rufnummer im Browser oder Mailprogramm wählt sie mit dem Softphone.")}</p>
        </div>
      </section>

      <section id="account">
        <h3>{t("Konto")}</h3>
        <div class="card">
          <button class="logout" onclick={onlogout}><Icon name="logout" size={18} /> {t("Abmelden")}</button>
          {#if version}<p class="version">{t("Version {v}", { v: version })}</p>{/if}
        </div>
      </section>
    {/if}
  </div>

  <footer>
    {#if notice}<p class="notice">{notice}</p>{/if}
    <button class="primary" onclick={save} disabled={!draft || saving}>{saving ? t("Speichere …") : t("Speichern")}</button>
    <button onclick={cancel}>{t("Abbrechen")}</button>
  </footer>
</div>

<style>
  .version { color: var(--muted); font-size: 0.85rem; margin: 0.4rem 0 0; }
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
  .path { display: flex; gap: 0.6rem; max-width: 34rem; }
  .path input, .field input { flex: 1; padding: 0.35rem 0.5rem; background: var(--panel-2); color: inherit; border: 1px solid var(--line); border-radius: 4px; }
  .field { display: flex; flex-direction: column; gap: 0.3rem; margin-top: 0.6rem; max-width: 34rem; }
  .field select { padding: 0.35rem 0.5rem; background: var(--panel-2); color: inherit; border: 1px solid var(--line); border-radius: 4px; max-width: 16rem; }
  .keys { display: flex; flex-direction: column; margin-top: 0.4rem; }
  .keys.off { opacity: 0.5; }
  .keyrow { display: grid; grid-template-columns: minmax(0, 18rem) 12rem auto; align-items: center; gap: 0.8rem; padding: 0.3rem 0; border-bottom: 1px solid var(--line); }
  .key { padding: 0.3rem 0.6rem; text-align: center; }
  .key.rec { border-color: var(--accent); color: var(--accent); }
  code { background: var(--panel-2); padding: 0.4rem 0.6rem; border-radius: 4px; font-size: 0.85rem; overflow-wrap: anywhere; }
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
