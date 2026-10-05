<script lang="ts">
  import Icon, { type IconName } from "./Icon.svelte";
  import { backToFirst, callDrag, startCallDrag, transfersOnHangup } from "./fkeys.svelte";
  import { action, duration, isRingingIn, phone, run, who, type Call } from "./phone.svelte";
  import { t } from "./i18n.svelte";

  let { call }: { call: Call } = $props();

  let tab = $state<"consult" | "conference" | "extras" | null>(null);
  let forwarding = $state(false);
  let target = $state("");
  let keypad = $state(false);

  const ringingIn = $derived(isRingingIn(call));
  const connected = $derived(call.phase === "connected");
  const held = $derived(call.phase === "held");
  // Anrufe, die mit diesem zusammenhängen (Rückfrage bzw. gehaltenes Gespräch)
  const partners = $derived(
    phone.status.calls.filter(
      (c) => c.id !== call.id && (c.consultation_of === call.id || call.consultation_of === c.id),
    ),
  );
  const transfers = $derived(transfersOnHangup(call));
  const others = $derived(phone.status.calls.filter((c) => c.id !== call.id));

  const title = $derived(
    ringingIn ? t("Eingehender Anruf")
    : held ? t("Gehaltenes Gespräch")
    : connected ? t("Aktives Gespräch")
    : t("Ausgehender Anruf"),
  );
  const sub = $derived(
    call.phase === "setup" ? t("Verbinde …")
    : call.phase === "ringback" ? t("Klingelt …")
    : "",
  );

  async function submitTarget(event: Event, act: "forward" | "consult") {
    event.preventDefault();
    if (!target.trim()) return;
    if (await action(act, call.id, target)) {
      target = "";
      forwarding = false;
    }
  }

  const keys = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "*", "0", "#"];
</script>

<article class="call">
  <header>
    <Icon name={call.incoming ? "incoming" : "outgoing"} size={18} />
    <span>{title}</span>
    {#if call.recording}<span class="rec">● {t("Aufnahme")}</span>{/if}
    <span class="time">{duration(call, phone.now)}</span>
  </header>

  <!-- Karte und Aktionsleiste liegen wie im Original in einer gemeinsamen Schale -->
  <div class="shell">
  <!-- Gespräch lässt sich auf ein Besetztlampenfeld ziehen (Vermitteln) -->
  <div class="pill" class:draggable={(connected || held) && !call.consultation_of} role="group" onpointerdown={(e) => startCallDrag(e, call)}>
    <div class="avatar"><Icon name="person" size={34} /></div>
    <div class="who">
      <strong>{who(call)}</strong>
      {#if call.remote_name && call.remote_number}<span>{call.remote_number}</span>{/if}
      {#if sub}<span class="sub">{sub}</span>{/if}
      {#if transfers}<span class="sub">{t("Auflegen vermittelt das gehaltene Gespräch")}</span>{/if}
      {#if call.local_number || call.local_name}
        <small><b>{call.local_number}</b> {call.local_name}</small>
      {/if}
    </div>
    {#if connected}
      <button
        class="mute"
        class:on={phone.status.muted}
        title={phone.status.muted ? t("Mikrofon einschalten") : t("Stumm schalten")}
        onclick={() => run("phone_mute", { muted: !phone.status.muted })}
      >
        <Icon name={phone.status.muted ? "micOff" : "mic"} size={18} />
      </button>
    {/if}
    {#if transfers}
      <button class="back" title={t("Rückfrage beenden, zurück zum ersten Gespräch")} onclick={() => backToFirst(call)}>{t("Zurück")}</button>
      <button class="round red" class:big={!ringingIn} title={t("Auflegen und vermitteln")} onclick={() => action("transfer_consultation", call.id)}>
        <Icon name="hangup" size={30} />
      </button>
    {:else}
      <button class="round red" class:big={!ringingIn} title={ringingIn ? t("Ablehnen") : t("Auflegen")} onclick={() => run("phone_hangup", { callId: call.id })}>
        <Icon name="hangup" size={ringingIn ? 24 : 30} />
      </button>
    {/if}
    {#if ringingIn}
      <button class="round green big" title={t("Annehmen")} onclick={() => run("phone_answer", { callId: call.id })}>
        <Icon name="call" size={32} />
      </button>
    {/if}
  </div>

  {#snippet tabButton(id: "consult" | "conference" | "extras", icon: IconName | null, label: string)}
    <button class="tab" class:active={tab === id} onclick={() => (tab = tab === id ? null : id)}>
      {#if icon}<Icon name={icon} />{:else}<b>R</b>{/if}
      <span>{label}</span>
    </button>
  {/snippet}

  {#if ringingIn}
    <nav class="tabs">
      <button class="tab" class:active={forwarding} onclick={() => (forwarding = !forwarding)}>
        <Icon name="forward" /><span>{t("Umleiten")}</span>
      </button>
      <button class="tab" onclick={() => action("voicemail", call.id)}>
        <Icon name="voicemail" /><span>Voicemail</span>
      </button>
    </nav>
  {:else if connected}
    <nav class="tabs">
      {@render tabButton("consult", null, t("Rückfrage"))}
      {@render tabButton("conference", "group", t("Konferenz"))}
      {@render tabButton("extras", "more", t("Extras"))}
    </nav>
  {:else if held}
    <nav class="tabs">
      <button class="tab" onclick={() => run("phone_hold", { callId: call.id, hold: false })}>
        <Icon name="play" /><span>{t("Fortsetzen")}</span>
      </button>
    </nav>
  {/if}
  </div>

  {#if ringingIn}
    {#if forwarding}
      <form class="target" onsubmit={(e) => submitTarget(e, "forward")}>
        <input bind:value={target} placeholder={t("Umleiten an Nummer")} inputmode="tel" />
        <button class="go" type="submit" disabled={!target.trim()}><Icon name="forward" size={20} /></button>
      </form>
    {/if}
  {:else if connected}
    {#if tab === "consult"}
      <div class="panel">
        <button class="row" onclick={() => run("phone_hold", { callId: call.id, hold: true })}>
          <Icon name="pause" /><span>{t("Anruf halten")}</span>
        </button>
        {#each partners as p (p.id)}
          <button class="row accent" onclick={() => action("transfer_consultation", p.consultation_of ? p.id : call.id)}>
            <Icon name="forward" /><span>{t("Verbinden mit {name}", { name: who(p) })}</span>
          </button>
        {/each}
        <form class="target" onsubmit={(e) => submitTarget(e, "consult")}>
          <input bind:value={target} placeholder={t("Rückfrage an Nummer")} inputmode="tel" />
          <button class="go" type="submit" disabled={!target.trim()}><Icon name="call" size={20} /></button>
        </form>
      </div>
    {:else if tab === "conference"}
      <div class="panel">
        {#if others.length}
          <button class="row accent" onclick={() => action("conference", call.id, others.map((c) => c.id).join(","))}>
            <Icon name="group" /><span>{t("Konferenz starten mit {names}", { names: others.map(who).join(", ") })}</span>
          </button>
        {:else}
          <p class="hint">{t("Zuerst einen weiteren Teilnehmer per Rückfrage anrufen, dann hier die Konferenz starten.")}</p>
        {/if}
        <form class="target" onsubmit={(e) => submitTarget(e, "consult")}>
          <input bind:value={target} placeholder={t("Teilnehmer anrufen")} inputmode="tel" />
          <button class="go" type="submit" disabled={!target.trim()}><Icon name="call" size={20} /></button>
        </form>
      </div>
    {:else if tab === "extras"}
      <div class="panel">
        <button class="row" onclick={() => (keypad = !keypad)}>
          <Icon name="dialpad" /><span>{t("Ziffernblock")}</span><span class="chev" class:open={keypad}><Icon name="chevron" size={20} /></span>
        </button>
        {#if keypad}
          <div class="keypad">
            {#each keys as k}
              <button onclick={() => run("phone_dtmf", { callId: call.id, digits: k })}>{k}</button>
            {/each}
          </div>
        {/if}
        <button class="row" onclick={() => action("record", call.id)}>
          <span class="dot"><Icon name="record" /></span><span>{call.recording ? t("Aufnahme beenden") : t("Aufnahme starten")}</span>
        </button>
        <button class="row" onclick={() => action("switch_phone", call.id)}>
          <Icon name="call2go" /><span>{t("Rufweitergabe/Call2Go")}</span>
        </button>
        <button class="row" disabled title={t("Folgt mit dem Adressbuch")}>
          <Icon name="contacts" /><span>{t("Kontakt hinzufügen")}</span>
        </button>
      </div>
    {/if}
  {/if}
</article>

{#if callDrag.call?.id === call.id}
  <div class="ghost" style="left: {callDrag.x}px; top: {callDrag.y}px">
    <Icon name="forward" size={16} /> {who(call)}{callDrag.over ? "" : ` … ${t("auf Besetztlampenfeld ziehen")}`}
  </div>
{/if}

<style>
  .pill.draggable { cursor: grab; touch-action: none; }
  .back { padding: 0.3rem 0.7rem; border-radius: 999px; }
  .ghost {
    position: fixed; z-index: 50; pointer-events: none; transform: translate(12px, 12px);
    display: flex; align-items: center; gap: 0.4rem; padding: 0.35rem 0.7rem; border-radius: 6px;
    background: var(--panel-2); border: 1px solid var(--accent); box-shadow: 0 4px 14px #0006;
    font-size: 0.9rem; font-weight: 600; white-space: nowrap;
  }
  .call { display: flex; flex-direction: column; gap: 0.5rem; }
  header { display: flex; align-items: center; gap: 0.4rem; font-size: 0.85rem; color: var(--muted); }
  header .time { margin-left: auto; font-variant-numeric: tabular-nums; }
  .rec { color: var(--red); }
  .shell {
    display: flex; flex-direction: column;
    padding: 0.4rem 0.4rem 0.3rem; border-radius: 2.6rem 2.6rem 1.8rem 1.8rem;
    background: linear-gradient(#cfd1d4, #b7babe);
    box-shadow: 0 1px 0 #fff8 inset, 0 2px 8px #0005;
  }
  .pill {
    display: flex; align-items: center; gap: 0.75rem;
    padding: 0.35rem 0.35rem 0.35rem 0.4rem; border-radius: 999px;
    background: #fff; color: #202326;
    box-shadow: 0 2px 5px #0004;
  }
  .avatar {
    flex: none; width: 3.6rem; height: 3.6rem; border-radius: 50%;
    display: grid; place-items: center; background: #d9dbde; color: #4a4f55;
    border: 2px solid #9a9ea3; overflow: hidden;
  }
  .who { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .who strong { font-size: 1.05rem; }
  .who span, .who small { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .who span { color: #555b62; }
  .who small { align-self: flex-start; max-width: 100%; border-top: 1px solid #b9bdc2; margin-top: 0.3rem; padding-top: 0.25rem; padding-right: 2rem; font-size: 0.8rem; color: #555b62; }
  .who small b { color: #202326; font-weight: 500; margin-right: 0.2rem; }
  .sub { color: #555b62; font-size: 0.9rem; }
  .round {
    flex: none; display: grid; place-items: center; border: none; border-radius: 50%;
    width: 3rem; height: 3rem; padding: 0; color: #fff; cursor: pointer;
    box-shadow: 0 2px 4px #0005;
  }
  .round.big { width: 4rem; height: 4rem; }
  /* Grauer Ring wie beim Original */
  .red { background: radial-gradient(circle at 50% 30%, #f03a3a, #b3060c); box-shadow: 0 0 0 3px #b3b6ba, 0 2px 4px #0005; }
  .green { background: radial-gradient(circle at 50% 30%, #8fe04a, #3a9a12); box-shadow: 0 0 0 3px #b3b6ba, 0 2px 4px #0005; }
  .mute { background: none; border: none; color: #5d636a; padding: 0.3rem; border-radius: 50%; cursor: pointer; }
  .mute.on { color: #fff; background: var(--red); }
  .tabs { display: flex; justify-content: space-around; padding: 0.35rem 0.6rem 0.1rem; }
  .tab {
    flex: 1; display: flex; flex-direction: column; align-items: center; gap: 0.15rem;
    background: none; border: 2px solid transparent; border-radius: 4px; color: #3b4045;
    padding: 0.3rem 0.4rem; font-size: 0.8rem; cursor: pointer;
  }
  .tab:hover { background: #fff3; }
  .tab b { font-size: 1.3rem; line-height: 24px; }
  .tab.active { border-color: var(--accent); background: #fff3; }
  .panel { display: flex; flex-direction: column; gap: 0.4rem; padding-top: 0.3rem; }
  .row {
    display: flex; align-items: center; gap: 0.75rem; text-align: left;
    background: var(--panel-2); border: none; color: inherit; padding: 0.6rem 0.8rem; border-radius: 4px; cursor: pointer;
  }
  .row:hover:not(:disabled) { background: var(--accent-soft); }
  .row.accent { background: var(--accent); color: #111; }
  .row:disabled { opacity: 0.45; cursor: default; }
  .row .chev { margin-left: auto; transition: transform 0.15s; }
  .row .chev.open { transform: rotate(180deg); }
  .dot { color: var(--red); display: grid; }
  .target { display: flex; gap: 0.4rem; }
  .target input { flex: 1; border-radius: 999px; }
  .go { border-radius: 50%; width: 2.6rem; height: 2.6rem; padding: 0; display: grid; place-items: center; background: var(--green); border: none; color: #fff; }
  .go:disabled { opacity: 0.4; }
  .keypad { display: grid; grid-template-columns: repeat(3, 1fr); gap: 0.35rem; }
  .keypad button { font-size: 1.15rem; padding: 0.5rem; }
  .hint { margin: 0.2rem 0; font-size: 0.85rem; color: var(--muted); }
</style>
