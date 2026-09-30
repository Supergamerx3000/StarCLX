<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

  type Phase = "setup" | "ringing" | "ringback" | "connected" | "held" | "other";
  type Call = {
    id: string;
    phase: Phase;
    incoming: boolean;
    remote_name: string;
    remote_number: string;
    connected_since: number | null;
  };
  type Status = {
    state: "off" | "starting" | "ready" | "error";
    detail: string;
    calls: Call[];
    muted: boolean;
  };

  let status = $state<Status>({ state: "off", detail: "", calls: [], muted: false });
  let number = $state("");
  let notice = $state("");
  let busy = $state(false);
  let keypadFor = $state<string | null>(null);
  let now = $state(Date.now());

  onMount(() => {
    const offs = [
      listen<Status>("phone", (e) => (status = e.payload)),
      listen<string>("phone-error", (e) => (notice = e.payload)),
    ];
    invoke<Status>("phone_status").then((s) => (status = s));
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => {
      clearInterval(timer);
      offs.forEach((p) => p.then((off) => off()));
    };
  });

  async function run(cmd: string, args: Record<string, unknown> = {}) {
    notice = "";
    try {
      await invoke(cmd, args);
      return true;
    } catch (e) {
      notice = String(e);
      return false;
    }
  }

  async function dial(event: Event) {
    event.preventDefault();
    if (!number.trim() || busy) return;
    busy = true;
    if (await run("phone_dial", { number })) number = "";
    busy = false;
  }

  function label(c: Call) {
    return c.remote_name || c.remote_number || "Unbekannt";
  }

  function phaseText(c: Call) {
    switch (c.phase) {
      case "setup": return "Verbinde …";
      case "ringing": return c.incoming ? "Ruft an" : "Klingelt";
      case "ringback": return "Klingelt beim Gegenüber";
      case "held": return "Gehalten";
      case "connected": return duration(c);
      default: return "";
    }
  }

  function duration(c: Call) {
    if (!c.connected_since) return "Verbunden";
    const s = Math.max(0, Math.floor((now - c.connected_since) / 1000));
    const mm = String(Math.floor(s / 60)).padStart(2, "0");
    const ss = String(s % 60).padStart(2, "0");
    return `${mm}:${ss}`;
  }

  // Klingelton über die Web-Audio-API, solange ein eingehender Anruf klingelt.
  const ringing = $derived(status.calls.some((c) => c.incoming && c.phase === "ringing"));
  $effect(() => {
    if (!ringing) return;
    const ctx = new AudioContext();
    const burst = () => {
      for (const [start, freq] of [[0, 880], [0.25, 660], [0.5, 880], [0.75, 660]] as const) {
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        osc.frequency.value = freq;
        gain.gain.setValueAtTime(0.15, ctx.currentTime + start);
        gain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + start + 0.22);
        osc.connect(gain).connect(ctx.destination);
        osc.start(ctx.currentTime + start);
        osc.stop(ctx.currentTime + start + 0.23);
      }
    };
    burst();
    const timer = setInterval(burst, 3000);
    return () => {
      clearInterval(timer);
      ctx.close();
    };
  });

  const keys = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "*", "0", "#"];
  const stateText: Record<Status["state"], string> = {
    off: "Softphone aus",
    starting: "Softphone startet …",
    ready: "Softphone bereit",
    error: "Softphone nicht angemeldet",
  };
</script>

<section class="phone">
  <p class="status {status.state}" title={status.detail}>
    <span class="dot"></span>{stateText[status.state]}
    {#if status.state === "error" && status.detail}<span class="detail">: {status.detail}</span>{/if}
  </p>

  <form class="dial" onsubmit={dial}>
    <input
      bind:value={number}
      placeholder="Nummer oder Durchwahl"
      inputmode="tel"
      autocomplete="off"
      aria-label="Nummer"
    />
    <button class="primary" type="submit" disabled={status.state !== "ready" || busy || !number.trim()}>
      Anrufen
    </button>
  </form>

  {#if notice}<p class="notice">{notice}</p>{/if}

  <ul class="calls">
    {#each status.calls as c (c.id)}
      <li class="call {c.phase}" class:incoming={c.incoming && c.phase === "ringing"}>
        <div class="who">
          <strong>{label(c)}</strong>
          {#if c.remote_name && c.remote_number}<span class="num">{c.remote_number}</span>{/if}
          <span class="phase">{c.incoming ? "↙" : "↗"} {phaseText(c)}</span>
        </div>
        <div class="actions">
          {#if c.incoming && c.phase === "ringing"}
            <button class="accept" onclick={() => run("phone_answer", { callId: c.id })}>Annehmen</button>
          {/if}
          {#if c.phase === "connected"}
            <button onclick={() => run("phone_hold", { callId: c.id, hold: true })}>Halten</button>
            <button class:active={status.muted} onclick={() => run("phone_mute", { muted: !status.muted })}>
              {status.muted ? "Stumm aus" : "Stumm"}
            </button>
            <button class:active={keypadFor === c.id} onclick={() => (keypadFor = keypadFor === c.id ? null : c.id)}>
              Tasten
            </button>
          {:else if c.phase === "held"}
            <button onclick={() => run("phone_hold", { callId: c.id, hold: false })}>Fortsetzen</button>
          {/if}
          <button class="hangup" onclick={() => run("phone_hangup", { callId: c.id })}>
            {c.incoming && c.phase === "ringing" ? "Ablehnen" : "Auflegen"}
          </button>
        </div>
        {#if keypadFor === c.id && c.phase === "connected"}
          <div class="keypad">
            {#each keys as k}
              <button onclick={() => run("phone_dtmf", { callId: c.id, digits: k })}>{k}</button>
            {/each}
          </div>
        {/if}
      </li>
    {:else}
      <li class="empty">Keine Gespräche</li>
    {/each}
  </ul>
</section>

<style>
  .phone { display: flex; flex-direction: column; gap: 1rem; }
  .status { display: flex; align-items: center; gap: 0.5rem; margin: 0; font-size: 0.9rem; opacity: 0.85; }
  .dot { width: 0.6rem; height: 0.6rem; border-radius: 50%; background: #8b949e; }
  .ready .dot { background: #1a7f37; }
  .starting .dot { background: #9a6700; }
  .error .dot { background: #cf222e; }
  .detail { opacity: 0.8; }
  .dial { display: flex; gap: 0.5rem; }
  .dial input { flex: 1; font-size: 1.1rem; }
  .calls { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 0.75rem; }
  .call {
    display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 0.75rem;
    padding: 0.9rem 1rem; border-radius: 10px; border: 1px solid var(--line);
    background: var(--card);
  }
  .call.incoming { border-color: #1a7f37; box-shadow: 0 0 0 2px #1a7f3740; }
  .who { display: flex; flex-direction: column; gap: 0.15rem; min-width: 0; }
  .who strong { font-size: 1.1rem; overflow: hidden; text-overflow: ellipsis; }
  .num, .phase { font-size: 0.9rem; opacity: 0.75; font-variant-numeric: tabular-nums; }
  .actions { display: flex; flex-wrap: wrap; gap: 0.5rem; }
  .keypad { flex-basis: 100%; display: grid; grid-template-columns: repeat(3, 4rem); gap: 0.4rem; }
  .keypad button { font-size: 1.1rem; }
  .empty { opacity: 0.6; padding: 0.5rem 0; }
  .notice { color: #9a6700; margin: 0; }
  button.primary, button.accept { background: #1a7f37; border-color: #1a7f37; color: #fff; }
  button.hangup { background: #cf222e; border-color: #cf222e; color: #fff; }
  button.active { outline: 2px solid #0969da; }
  button:disabled { opacity: 0.5; cursor: default; }
</style>
