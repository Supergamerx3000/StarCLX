<script lang="ts">
  import Icon, { type IconName } from "./Icon.svelte";
  import { action, duration, isRingingIn, phone, run, who, type Call } from "./phone.svelte";

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
  const others = $derived(phone.status.calls.filter((c) => c.id !== call.id));

  const title = $derived(
    ringingIn ? "Eingehender Anruf"
    : held ? "Gehaltenes Gespräch"
    : connected ? "Aktives Gespräch"
    : "Ausgehender Anruf",
  );
  const sub = $derived(
    call.phase === "setup" ? "Verbinde …"
    : call.phase === "ringback" ? "Klingelt …"
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
    {#if call.recording}<span class="rec">● Aufnahme</span>{/if}
    <span class="time">{duration(call, phone.now)}</span>
  </header>

  <div class="pill">
    <div class="avatar"><Icon name="person" size={34} /></div>
    <div class="who">
      <strong>{who(call)}</strong>
      {#if call.remote_name && call.remote_number}<span>{call.remote_number}</span>{/if}
      {#if sub}<span class="sub">{sub}</span>{/if}
      {#if call.local_number || call.local_name}
        <small>{call.local_number} {call.local_name}</small>
      {/if}
    </div>
    {#if connected}
      <button
        class="mute"
        class:on={phone.status.muted}
        title={phone.status.muted ? "Mikrofon einschalten" : "Stumm schalten"}
        onclick={() => run("phone_mute", { muted: !phone.status.muted })}
      >
        <Icon name={phone.status.muted ? "micOff" : "mic"} size={18} />
      </button>
    {/if}
    <button class="round red" class:big={!ringingIn} title={ringingIn ? "Ablehnen" : "Auflegen"} onclick={() => run("phone_hangup", { callId: call.id })}>
      <Icon name="hangup" size={ringingIn ? 24 : 30} />
    </button>
    {#if ringingIn}
      <button class="round green big" title="Annehmen" onclick={() => run("phone_answer", { callId: call.id })}>
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
        <Icon name="forward" /><span>Umleiten</span>
      </button>
      <button class="tab" onclick={() => action("voicemail", call.id)}>
        <Icon name="voicemail" /><span>Voicemail</span>
      </button>
    </nav>
    {#if forwarding}
      <form class="target" onsubmit={(e) => submitTarget(e, "forward")}>
        <input bind:value={target} placeholder="Umleiten an Nummer" inputmode="tel" />
        <button class="go" type="submit" disabled={!target.trim()}><Icon name="forward" size={20} /></button>
      </form>
    {/if}
  {:else if connected}
    <nav class="tabs">
      {@render tabButton("consult", null, "Rückfrage")}
      {@render tabButton("conference", "group", "Konferenz")}
      {@render tabButton("extras", "more", "Extras")}
    </nav>
    {#if tab === "consult"}
      <div class="panel">
        <button class="row" onclick={() => run("phone_hold", { callId: call.id, hold: true })}>
          <Icon name="pause" /><span>Anruf halten</span>
        </button>
        {#each partners as p (p.id)}
          <button class="row accent" onclick={() => action("transfer_consultation", p.consultation_of ? p.id : call.id)}>
            <Icon name="forward" /><span>Verbinden mit {who(p)}</span>
          </button>
        {/each}
        <form class="target" onsubmit={(e) => submitTarget(e, "consult")}>
          <input bind:value={target} placeholder="Rückfrage an Nummer" inputmode="tel" />
          <button class="go" type="submit" disabled={!target.trim()}><Icon name="call" size={20} /></button>
        </form>
      </div>
    {:else if tab === "conference"}
      <div class="panel">
        {#if others.length}
          <button class="row accent" onclick={() => action("conference", call.id, others.map((c) => c.id).join(","))}>
            <Icon name="group" /><span>Konferenz starten mit {others.map(who).join(", ")}</span>
          </button>
        {:else}
          <p class="hint">Zuerst einen weiteren Teilnehmer per Rückfrage anrufen, dann hier die Konferenz starten.</p>
        {/if}
        <form class="target" onsubmit={(e) => submitTarget(e, "consult")}>
          <input bind:value={target} placeholder="Teilnehmer anrufen" inputmode="tel" />
          <button class="go" type="submit" disabled={!target.trim()}><Icon name="call" size={20} /></button>
        </form>
      </div>
    {:else if tab === "extras"}
      <div class="panel">
        <button class="row" onclick={() => (keypad = !keypad)}>
          <Icon name="dialpad" /><span>Ziffernblock</span><span class="chev" class:open={keypad}><Icon name="chevron" size={20} /></span>
        </button>
        {#if keypad}
          <div class="keypad">
            {#each keys as k}
              <button onclick={() => run("phone_dtmf", { callId: call.id, digits: k })}>{k}</button>
            {/each}
          </div>
        {/if}
        <button class="row" onclick={() => action("record", call.id)}>
          <span class="dot"><Icon name="record" /></span><span>{call.recording ? "Aufnahme beenden" : "Aufnahme starten"}</span>
        </button>
        <button class="row" onclick={() => action("switch_phone", call.id)}>
          <Icon name="call2go" /><span>Rufweitergabe/Call2Go</span>
        </button>
        <button class="row" disabled title="Folgt mit dem Adressbuch">
          <Icon name="contacts" /><span>Kontakt hinzufügen</span>
        </button>
      </div>
    {/if}
  {:else if held}
    <nav class="tabs">
      <button class="tab" onclick={() => run("phone_hold", { callId: call.id, hold: false })}>
        <Icon name="play" /><span>Fortsetzen</span>
      </button>
    </nav>
  {/if}
</article>

<style>
  .call { display: flex; flex-direction: column; gap: 0.5rem; }
  header { display: flex; align-items: center; gap: 0.4rem; font-size: 0.85rem; color: var(--muted); }
  header .time { margin-left: auto; font-variant-numeric: tabular-nums; }
  .rec { color: var(--red); }
  .pill {
    display: flex; align-items: center; gap: 0.75rem;
    padding: 0.6rem 0.7rem; border-radius: 3rem 1.6rem 1.6rem 3rem;
    background: #e6e8eb; color: #202326;
    box-shadow: 0 2px 6px #0006 inset;
  }
  .avatar {
    flex: none; width: 3.2rem; height: 3.2rem; border-radius: 50%;
    display: grid; place-items: center; background: #c4c8cd; color: #7a8088;
  }
  .who { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .who strong { font-size: 1.05rem; }
  .who span, .who small { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .who small { border-top: 1px solid #b9bdc2; margin-top: 0.25rem; padding-top: 0.2rem; font-size: 0.75rem; color: #555b62; }
  .sub { color: #555b62; font-size: 0.9rem; }
  .round {
    flex: none; display: grid; place-items: center; border: none; border-radius: 50%;
    width: 3rem; height: 3rem; padding: 0; color: #fff; cursor: pointer;
    box-shadow: 0 2px 4px #0005;
  }
  .round.big { width: 3.8rem; height: 3.8rem; }
  .red { background: radial-gradient(circle at 50% 35%, #ff4b4b, #c4161c); }
  .green { background: radial-gradient(circle at 50% 35%, #8fe04a, #3fa716); }
  .mute { background: none; border: none; color: #5d636a; padding: 0.3rem; border-radius: 50%; cursor: pointer; }
  .mute.on { color: #fff; background: var(--red); }
  .tabs { display: flex; justify-content: space-around; background: #e6e8eb; border-radius: 0 0 16px 16px; margin-top: -0.9rem; padding-top: 0.6rem; }
  .tab {
    flex: 1; display: flex; flex-direction: column; align-items: center; gap: 0.1rem;
    background: none; border: none; color: #202326; padding: 0.35rem 0.5rem; font-size: 0.75rem; cursor: pointer;
  }
  .tab b { font-size: 1.3rem; line-height: 24px; }
  .tab.active { background: var(--accent); }
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
