<script lang="ts">
  import { connection } from "./connection.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import DeviceList, { DEFAULT, mergeOrder, type Device } from "./DeviceList.svelte";
  import Icon, { type IconName } from "./Icon.svelte";
  import FkeyEditor from "./plugins/fkeys/FkeyEditor.svelte";
  import Reach from "./plugins/reach/Reach.svelte";
  import Toggle from "./Toggle.svelte";
  import BusylightSettings from "./plugins/busylight/Settings.svelte";
  import HeadsetSettings from "./plugins/headset/Settings.svelte";
  import CallActionsSettings from "./plugins/callactions/Settings.svelte";
  import DoorCamSettings from "./plugins/doorcam/Settings.svelte";
  import { applyAccent, DEFAULT_ACCENT, type Hotkeys, loadPrefs, locked, prefs, savePrefs, type Prefs } from "./prefs.svelte";
  import { setLanguage, t } from "./i18n.svelte";
  import { avatarOf, fkeys } from "./plugins/fkeys/fkeys.svelte";

  type SignalingNumber = { id: string; number: string; suppressed: boolean; read_only: boolean; selected: boolean };

  let { onclose, onlogout, server = "", userId = "", displayName = "" }: {
    onclose: () => void; onlogout: () => void; server?: string; userId?: string; displayName?: string;
  } = $props();

  let draft = $state<Prefs | null>(null);
  /** Vom System gesperrt, nicht änderbar */
  const lk = (key: keyof Prefs) => locked.keys.includes(key);
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

  const sections: { id: string; icon: IconName; label: string }[] = $derived([
    { id: "softphone", icon: "call", label: "Softphone" },
    { id: "notifications", icon: "bell", label: t("Benachrichtigungen") },
    { id: "audio", icon: "headset", label: t("Audio") },
    { id: "headset", icon: "headset", label: t("Headset-Tasten") },
    { id: "ringtones", icon: "music", label: t("Klingeltöne") },
    { id: "signaling", icon: "numbers", label: t("Rufnummer signalisieren") },
    { id: "callmanager", icon: "forward", label: "Call Manager" },
    { id: "callactions", icon: "call2go", label: t("URL oder Programm bei Anruf") },
    { id: "doorcams", icon: "door", label: t("Türkameras") },
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
  ]);
  const personalSections: { id: string; icon: IconName; label: string }[] = $derived([
    { id: "appearance", icon: "workspace", label: t("Darstellung") },
    { id: "fkeys", icon: "dialpad", label: t("Funktionstasten") },
    { id: "hotkeys", icon: "dialpad", label: t("Hotkeys") },
    { id: "integration", icon: "call", label: t("Desktop-Integration") },
  ]);
  const accountSections: { id: string; icon: IconName; label: string }[] = $derived([
    { id: "account", icon: "account", label: t("Konto") },
    { id: "password", icon: "lock", label: t("Passwort") },
    { id: "log", icon: "list", label: t("Protokoll") },
  ]);
  type Tab = "phone" | "reach" | "chat" | "personal" | "account";
  /** Ein Reiter pro Bereich; die Unterpunkte springen innerhalb des Reiters */
  const tabs: { id: Tab; icon: IconName; label: string; items: { id: string; icon: IconName; label: string }[] }[] = $derived([
    { id: "phone", icon: "call", label: t("Telefonie"), items: sections },
    { id: "reach", icon: "forward", label: t("Erreichbarkeit"), items: reachSections },
    { id: "chat", icon: "chat", label: "Chat", items: chatSections },
    { id: "personal", icon: "workspace", label: t("Personalisierung"), items: personalSections },
    { id: "account", icon: "account", label: t("Konto"), items: accountSections },
  ]);
  let tab = $state<Tab>("phone");
  const hotkeyRows: { key: Exclude<keyof Hotkeys, "enabled">; label: string }[] = $derived([
    { key: "dial_selection", label: t("Markierte Rufnummer wählen") },
    { key: "dial_clipboard", label: t("Rufnummer aus Zwischenablage wählen") },
    { key: "answer", label: t("Softphone-Anruf annehmen") },
    { key: "hangup", label: t("Aktuellen Anruf beenden") },
    { key: "toggle_view", label: t("Ansicht umschalten") },
  ]);
  let desktop = $state({ wayland: false, gnome: false, portal: false, command: "" });
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

  const avatar = $derived(userId ? avatarOf(userId) : null);
  const initials = $derived(displayName.split(/\s+/).filter(Boolean).slice(0, 2).map((w) => w[0]).join("").toUpperCase() || "?");
  let avatarBusy = $state(false);
  let avatarError = $state("");

  /** Bild mittig quadratisch zuschneiden und als JPEG (256 px) liefern */
  function squareJpeg(src: string): Promise<string> {
    return new Promise((resolve, reject) => {
      const img = new Image();
      img.onload = () => {
        const side = Math.min(img.naturalWidth, img.naturalHeight);
        const size = Math.min(256, side);
        const canvas = document.createElement("canvas");
        canvas.width = canvas.height = size;
        const ctx = canvas.getContext("2d")!;
        ctx.fillStyle = "#fff"; // Transparenz gibt es in JPEG nicht
        ctx.fillRect(0, 0, size, size);
        ctx.drawImage(img, (img.naturalWidth - side) / 2, (img.naturalHeight - side) / 2, side, side, 0, 0, size, size);
        resolve(canvas.toDataURL("image/jpeg", 0.9).split(",")[1]);
      };
      img.onerror = () => reject(t("Die Datei ist kein lesbares Bild."));
      img.src = src;
    });
  }

  async function changeAvatar() {
    avatarError = "";
    try {
      const src = await invoke<string | null>("account_pick_avatar");
      if (!src) return;
      avatarBusy = true;
      await invoke("account_set_avatar", { jpeg: await squareJpeg(src) });
      delete fkeys.avatars[userId];
    } catch (e) {
      avatarError = t("Profilbild nicht geändert: {e}", { e: String(e) });
    } finally {
      avatarBusy = false;
    }
  }

  async function removeAvatar() {
    avatarError = "";
    avatarBusy = true;
    try {
      await invoke("account_delete_avatar");
      delete fkeys.avatars[userId];
    } catch (e) {
      avatarError = t("Profilbild nicht entfernt: {e}", { e: String(e) });
    } finally {
      avatarBusy = false;
    }
  }

  type PasswordChange = { kind: "changed" } | { kind: "wrong_current" } | { kind: "policy"; message: string; violations: string[] };
  let pw = $state({ current: "", next: "", repeat: "" });
  let pwBusy = $state(false);
  let pwError = $state<string[]>([]);
  let pwDone = $state(false);
  const pwMismatch = $derived(pw.repeat !== "" && pw.next !== pw.repeat);

  async function changePassword(e: SubmitEvent) {
    e.preventDefault();
    if (!pw.current || !pw.next || pw.next !== pw.repeat) return;
    pwBusy = true;
    pwError = [];
    pwDone = false;
    try {
      const r = await invoke<PasswordChange>("account_change_password", { current: pw.current, new: pw.next });
      if (r.kind === "changed") {
        pwDone = true;
        pw = { current: "", next: "", repeat: "" };
      } else if (r.kind === "wrong_current") {
        pwError = [t("Das aktuelle Passwort stimmt nicht.")];
      } else {
        pwError = [...(r.message ? [r.message] : []), ...r.violations];
      }
    } catch (err) {
      pwError = [t("Passwort nicht geändert: {e}", { e: String(err) })];
    } finally {
      pwBusy = false;
    }
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
    try {
      numbers = await invoke<SignalingNumber[]>("signaling_numbers");
      initialSignaling = signaling = numbers.find((n) => n.selected)?.id ?? "";
    } catch (e) {
      numbersError = String(e);
    }
  }

  async function exportLog() {
    try {
      const path = await invoke<string | null>("log_export");
      if (path) notice = t("Protokoll gespeichert: {path}", { path });
    } catch (e) {
      notice = String(e);
    }
  }

  async function openLogDir() {
    try {
      await invoke("log_open_dir");
    } catch (e) {
      notice = String(e);
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

  function select(id: Tab) {
    tab = id;
    content.scrollTo({ top: 0 });
  }

  function jump(id: string) {
    content.querySelector(`#${id}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  /** Schliessen ohne Speichern: probeweise gewählte Sprache und Farbe zurücknehmen */
  function cancel() {
    setLanguage(prefs.value?.language);
    applyAccent();
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
    {#each tabs as g}
      <button class="tab" class:active={tab === g.id} aria-current={tab === g.id ? "page" : undefined} onclick={() => select(g.id)}>
        <Icon name={g.icon} size={20} /><span>{g.label}</span>
      </button>
      {#if tab === g.id}
        {#each g.items as s}
          <button class="nav" onclick={() => jump(s.id)}><Icon name={s.icon} size={16} /><span>{s.label}</span></button>
        {/each}
      {/if}
    {/each}
  </nav>

  <div class="content" bind:this={content}>
    {#if !draft}
      <p class="muted">{t("Lade …")}</p>
    {:else}
      {#if locked.keys.length}
        <p class="muted locked">{t("Ausgegraute Einstellungen sind von der IT vorgegeben.")}</p>
      {/if}
      <!-- Alle Reiter bleiben gemountet, damit Eingaben beim Wechsel erhalten bleiben -->
      <div class="page" hidden={tab !== "phone"}>
      <section id="softphone">
        <h3>Softphone</h3>
        <div class="card">
          <Toggle bind:checked={draft.softphone} disabled={lk("softphone")} label={t("Softphone verwenden")} />
          <Toggle bind:checked={draft.primary_on_login} disabled={!draft.softphone || lk("primary_on_login")} label={t("Softphone bei der Anmeldung am Server als primäres Telefon auswählen")} />
          <Toggle bind:checked={draft.primary_on_answer} disabled={!draft.softphone || lk("primary_on_answer")} label={t("Bei Rufannahme das Softphone als primäres Telefon auswählen")} />
        </div>
      </section>

      <section id="notifications">
        <h3>{t("Benachrichtigungen")}</h3>
        <div class="card">
          <Toggle bind:checked={draft.notify_missed} disabled={lk("notify_missed")} label={t("Benachrichtigung über verpasste Anrufe anzeigen (ohne Gruppenanrufe)")} />
          <Toggle bind:checked={draft.notify_missed_group} disabled={lk("notify_missed_group")} label={t("Benachrichtigung bei verpassten Gruppenanrufen anzeigen")} />
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

      <HeadsetSettings bind:draft />

      <section id="ringtones">
        <h3>{t("Klingeltöne")}</h3>
        <div class="card">
          <Toggle bind:checked={draft.ringtone} disabled={lk("ringtone")} label={t("Klingelton verwenden")} />
          <div class="tones" class:off={!draft.ringtone}>
            <div class="tone head"><span>{t("Intern")}</span><span>{t("Extern")}</span></div>
            {#each [...builtinTones, ...draft.custom_ringtones] as tone (tone)}
              <div class="tone">
                <input type="radio" name="tone-int" value={tone} bind:group={draft.ringtone_internal} disabled={lk("ringtone_internal")} title={t("Interne Anrufe")} />
                <input type="radio" name="tone-ext" value={tone} bind:group={draft.ringtone_external} disabled={lk("ringtone_external")} title={t("Externe Anrufe")} />
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
          {#if numbersError && !connection.online}
            <p class="muted">{t("Keine Verbindung zur Anlage")}</p>
          {:else if numbersError}
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
          <Toggle bind:checked={draft.bring_to_front} disabled={lk("bring_to_front")} label={t("Beim Empfang eines Anrufs die App in den Vordergrund bringen")} />
        </div>
      </section>

      <CallActionsSettings bind:draft onnotice={(n) => (notice = n)} />
      <DoorCamSettings bind:draft />

      <BusylightSettings bind:draft />
      </div>

      <div class="page" hidden={tab !== "reach"}>
      <Reach {server} />
      </div>

      <div class="page" hidden={tab !== "chat"}>
      <section id="chat-notify">
        <h3>{t("Chat: Benachrichtigungen")}</h3>
        <div class="card">
          <Toggle bind:checked={draft.chat_notify} disabled={lk("chat_notify")} label={t("Benachrichtigung bei neuer Chatnachricht anzeigen")} />
          <Toggle bind:checked={draft.chat_sound} disabled={lk("chat_sound")} label={t("Ton bei neuer Chatnachricht abspielen")} />
        </div>
      </section>

      <section id="chat-files">
        <h3>{t("Dateien empfangen")}</h3>
        <div class="card">
          <p class="muted">{t("Empfangene Dateien speichern unter")}</p>
          <div class="path">
            <input type="text" bind:value={draft.download_dir} disabled={lk("download_dir")} placeholder={defaultDownloads || "Downloads"} />
            <button onclick={pickDownloadDir}>{t("Suchen")}</button>
          </div>
          <p class="small muted">{t("Leer lassen für den Standardordner. Hier landen Dateien, die du im Chat annimmst.")}</p>
        </div>
      </section>

      <section id="chat-status">
        <h3>{t("Status")}</h3>
        <div class="card">
          <Toggle bind:checked={draft.away_on_idle} disabled={lk("away_on_idle")} label={t("Bei Inaktivität (10 Minuten) Status auf „Abwesend“ setzen")} />
          <Toggle bind:checked={draft.away_on_screensaver} disabled={lk("away_on_screensaver")} label={t("Bei aktivem Bildschirmschoner Status auf „Abwesend“ setzen")} />
          <Toggle bind:checked={draft.away_on_lock} disabled={lk("away_on_lock")} label={t("Bei gesperrtem Bildschirm Status auf „Abwesend“ setzen")} />
          <label class="field">
            <span>{t("Statustext bei automatischer Abwesenheit")}</span>
            <input type="text" bind:value={draft.away_text} disabled={lk("away_text")} placeholder={t("z. B. Bin gleich zurück")} />
          </label>
          <label class="field">
            <span>{t("Statustext beim Abmelden")}</span>
            <input type="text" bind:value={draft.offline_text} disabled={lk("offline_text")} placeholder={t("z. B. Feierabend")} />
          </label>
        </div>
      </section>

      </div>

      <div class="page" hidden={tab !== "personal"}>
      <section id="appearance">
        <h3>{t("Darstellung")}</h3>
        <div class="card">
          <h4>{t("Erscheinungsbild")}</h4>
          {#each [["dark", t("Dunkel")], ["light", t("Hell")], ["system", t("System")]] as [value, label]}
            <label class="radio"><input type="radio" name="theme" {value} bind:group={draft.theme} disabled={lk("theme")} /> {label}</label>
          {/each}
          <hr />
          <!-- Wirkt sofort zur Vorschau; gespeichert wird mit „Speichern“ -->
          <div class="field">
            <span>{t("Akzentfarbe")}</span>
            <span class="accent">
              <input
                type="color"
                value={draft.accent || DEFAULT_ACCENT}
                disabled={lk("accent")}
                oninput={(e) => { if (draft) { draft.accent = e.currentTarget.value; applyAccent(draft.accent); } }}
              />
              <button disabled={lk("accent") || !draft.accent} onclick={() => { if (draft) { draft.accent = ""; applyAccent(""); } }}>{t("Zurücksetzen")}</button>
            </span>
          </div>
          <hr />
          <label class="field">
            <span>{t("Sprache")}</span>
            <!-- Wirkt sofort zur Vorschau; gespeichert wird mit „Speichern“ -->
            <select bind:value={draft.language} disabled={lk("language")} onchange={() => setLanguage(draft?.language)}>
              <option value="de">Deutsch</option>
              <option value="en">English</option>
              <option value="fr">Français</option>
              <option value="it">Italiano</option>
            </select>
          </label>
          <hr />
          <label class="field">
            <span>{t("Arbeitsbereich")}</span>
            <select bind:value={draft.workspace} disabled={lk("workspace")}>
              <option value="tabs">{t("Reiter")}</option>
              <option value="free">{t("Frei anordnen")}</option>
            </select>
          </label>
          {#if draft.workspace === "free"}
            <p class="small muted">{t("Kacheln an der Titelleiste verschieben und an der Ecke unten rechts in der Grösse ändern. Über die Leiste oben blendest du Kacheln ein und aus.")}</p>
            <button class="add" onclick={() => draft && (draft.workspace_tiles = null)}>{t("Anordnung zurücksetzen")}</button>
          {/if}
          <hr />
          <Toggle bind:checked={draft.autostart} disabled={lk("autostart")} label={t("Beim Anmelden am Rechner starten")} />
          <Toggle bind:checked={draft.start_minimized} disabled={lk("start_minimized")} label={t("Programm minimiert starten")} />
          <Toggle bind:checked={draft.minimize_to_tray} disabled={lk("minimize_to_tray")} label={t("Beim Minimieren nur als Symbol im Infobereich anzeigen")} />
          <Toggle bind:checked={draft.always_on_top} disabled={lk("always_on_top")} label={t("Immer im Vordergrund")} />
          {#if desktop.wayland && draft.always_on_top}
            <p class="small muted">{t("Unter Wayland bestimmt der Desktop, ob ein Fenster oben bleibt. Bei GNOME geht es über Alt+Leertaste → „Immer im Vordergrund“.")}</p>
          {/if}
        </div>
      </section>

      <section id="fkeys">
        <h3>{t("Funktionstasten")}</h3>
        <div class="card">
          <FkeyEditor bind:columns={draft.fkey_columns} />
          <p class="small muted">{t("Tasten werden sofort auf der Anlage gespeichert; die Spaltenzahl mit „Speichern“.")}</p>
        </div>
      </section>

      <section id="hotkeys">
        <h3>{t("Hotkeys")}</h3>
        <div class="card">
          {#if desktop.gnome}
            <Toggle bind:checked={draft.hotkeys.enabled} disabled={lk("hotkeys")} label={t("Tastenkürzel systemweit in GNOME eintragen")} />
            <p class="small muted">{t("Die Kürzel gelten dann in allen Programmen und überschreiben dort gleiche Kombinationen. Andere eigene Tastenkürzel bleiben unverändert.")}</p>
          {:else if desktop.portal}
            <Toggle bind:checked={draft.hotkeys.enabled} disabled={lk("hotkeys")} label={t("Tastenkürzel beim Desktop anmelden")} />
            <p class="small muted">{t("Beim ersten Mal fragt der Desktop nach, ob er die Kürzel übernehmen soll. Sie wirken, solange StarCLX läuft. Ändern lassen sie sich danach in den Systemeinstellungen des Desktops; die Tasten hier sind nur ein Vorschlag für neue Kürzel.")}</p>
            {#if draft.hotkeys.enabled}
              <button onclick={() => invoke("configure_hotkeys").catch((e) => (notice = String(e)))}>{t("In den Systemeinstellungen ändern …")}</button>
            {/if}
          {:else}
            <p class="muted">{t("Dieser Desktop erlaubt Programmen keine globalen Tastenkürzel. Lege in den Systemeinstellungen eine eigene Tastenkombination mit diesem Befehl an:")}</p>
            <code>{desktop.command}</code>
            <p class="small muted">{t("Aktionen:")} dial-selection, dial-clipboard, answer, hangup, toggle-view</p>
          {/if}
          <div class="keys" class:off={(desktop.gnome || desktop.portal) && !draft.hotkeys.enabled}>
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
          <Toggle bind:checked={draft.handle_tel_links} disabled={lk("handle_tel_links")} label={t("Rufnummern-Links (tel:, callto:, sip:) mit StarCLX öffnen")} />
          <p class="small muted">{t("Ein Klick auf eine Rufnummer im Browser oder Mailprogramm wählt sie mit dem Softphone.")}</p>
        </div>
      </section>

      </div>

      <div class="page" hidden={tab !== "account"}>
      <section id="account">
        <h3>{t("Konto")}</h3>
        <div class="card">
          <div class="profile">
            <div class="pic">{#if avatar}<img src={avatar} alt="" />{:else}{initials}{/if}</div>
            <div class="who">
              {#if displayName}<strong>{displayName}</strong>{/if}
              <div class="buttons">
                <button onclick={changeAvatar} disabled={avatarBusy || !userId}><Icon name="edit" size={18} /> {t("Profilbild ändern …")}</button>
                {#if avatar}<button onclick={removeAvatar} disabled={avatarBusy}><Icon name="trash" size={18} /> {t("Entfernen")}</button>{/if}
              </div>
            </div>
          </div>
          {#if avatarError}<p class="error">{avatarError}</p>{/if}
          <hr />
          <button class="logout" onclick={onlogout}><Icon name="logout" size={18} /> {t("Abmelden")}</button>
          {#if version}<p class="version">{t("Version {v}", { v: version })}</p>{/if}
        </div>
      </section>

      <section id="password">
        <h3>{t("Passwort ändern")}</h3>
        <form class="card" onsubmit={changePassword}>
          <label class="field">{t("Aktuelles Passwort")}
            <input type="password" autocomplete="current-password" bind:value={pw.current} />
          </label>
          <label class="field">{t("Neues Passwort")}
            <input type="password" autocomplete="new-password" bind:value={pw.next} />
          </label>
          <label class="field">{t("Neues Passwort wiederholen")}
            <input type="password" autocomplete="new-password" bind:value={pw.repeat} />
          </label>
          {#if pwMismatch}<p class="error">{t("Die beiden neuen Passwörter sind nicht gleich.")}</p>{/if}
          {#each pwError as line}<p class="error">{line}</p>{/each}
          {#if pwDone}<p class="ok">{t("Passwort geändert. Beim nächsten Anmelden gilt das neue Passwort.")}</p>{/if}
          <div class="buttons">
            <button type="submit" disabled={pwBusy || !pw.current || !pw.next || pw.next !== pw.repeat}><Icon name="lock" size={18} /> {pwBusy ? t("Ändere …") : t("Passwort ändern")}</button>
          </div>
        </form>
      </section>

      <section id="log">
        <h3>{t("Protokoll")}</h3>
        <div class="card">
          <p class="muted">{t("Bei einem Problem das Protokoll speichern und uns schicken, z. B. als Anhang an ein GitHub-Issue.")}</p>
          <div class="buttons">
            <button onclick={exportLog}><Icon name="folder" size={18} /> {t("Protokoll speichern …")}</button>
            <button onclick={openLogDir}>{t("Ordner öffnen")}</button>
          </div>
          <Toggle bind:checked={draft.verbose_log} disabled={lk("verbose_log")} label={t("Ausführlich protokollieren (Anruf- und Verbindungsdetails)")} />
          <p class="small muted">{t("Das Protokoll kann Namen und Rufnummern enthalten. Passwörter und Tokens stehen nicht darin.")}</p>
        </div>
      </section>
      </div>
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
  .tab { display: flex; align-items: center; gap: 0.6rem; background: none; border: none; border-radius: 4px; text-align: left; padding: 0.55rem 0.5rem; font-weight: 600; }
  .tab:hover { background: var(--panel-2); }
  .tab.active { background: var(--panel-2); color: var(--accent); }
  .nav { display: flex; align-items: center; gap: 0.6rem; background: none; border: none; border-radius: 4px; text-align: left; padding: 0.35rem 0.5rem 0.35rem 1.9rem; font-size: 0.92rem; color: var(--muted); }
  .nav:hover { background: var(--panel-2); color: inherit; }
  .page[hidden] { display: none; }
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
  .path { display: flex; gap: 0.6rem; max-width: 34rem; }
  .path input, .field input { flex: 1; padding: 0.35rem 0.5rem; background: var(--panel-2); color: inherit; border: 1px solid var(--line); border-radius: 4px; }
  .field { display: flex; flex-direction: column; gap: 0.3rem; margin-top: 0.6rem; max-width: 34rem; }
  .accent { display: flex; align-items: center; gap: 0.6rem; }
  .accent input { flex: none; width: 3rem; height: 2rem; padding: 0.1rem; background: var(--panel-2); border: 1px solid var(--line); border-radius: 4px; cursor: pointer; }
  .field select { padding: 0.35rem 0.5rem; background: var(--panel-2); color: inherit; border: 1px solid var(--line); border-radius: 4px; max-width: 16rem; }
  .keys { display: flex; flex-direction: column; margin-top: 0.4rem; }
  .keys.off { opacity: 0.5; }
  .keyrow { display: grid; grid-template-columns: minmax(0, 18rem) 12rem auto; align-items: center; gap: 0.8rem; padding: 0.3rem 0; border-bottom: 1px solid var(--line); }
  .key { padding: 0.3rem 0.6rem; text-align: center; }
  .key.rec { border-color: var(--accent); color: var(--accent); }
  code { background: var(--panel-2); padding: 0.4rem 0.6rem; border-radius: 4px; font-size: 0.85rem; overflow-wrap: anywhere; }
  .profile { display: flex; align-items: center; gap: 1rem; }
  .profile .pic {
    width: 4.5rem; height: 4.5rem; border-radius: 50%; flex: none; overflow: hidden;
    display: grid; place-items: center; background: var(--panel-2); font-size: 1.5rem; font-weight: 600;
  }
  .profile .pic img { width: 100%; height: 100%; object-fit: cover; }
  .who { display: flex; flex-direction: column; gap: 0.2rem; min-width: 0; }
  .error { color: var(--red); margin: 0.3rem 0; }
  .ok { color: var(--green); margin: 0.3rem 0; }
  .logout { align-self: flex-start; display: flex; align-items: center; gap: 0.5rem; }
  .buttons { display: flex; flex-wrap: wrap; gap: 0.6rem; margin: 0.4rem 0 0.6rem; }
  .buttons button { display: flex; align-items: center; gap: 0.4rem; }
  footer {
    grid-column: 1 / -1; display: flex; justify-content: flex-end; align-items: center; gap: 1rem;
    padding: 0.8rem 1.2rem; border-top: 1px solid var(--line); background: var(--bg);
  }
  footer button { min-width: 8rem; }
  .primary { background: var(--accent); border-color: var(--accent); color: #111; font-weight: 600; }
  @media (max-width: 700px) {
    .settings { grid-template-columns: 1fr; grid-template-rows: auto auto 1fr auto; }
    nav { flex-direction: row; padding: 0.4rem 0.6rem; gap: 0.3rem; overflow-x: auto; }
    .tab { white-space: nowrap; }
    .nav { display: none; }
  }
</style>
