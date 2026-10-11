<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import Icon from "../../Icon.svelte";
  import { connection } from "../../connection.svelte";
  import { t } from "../../i18n.svelte";
  import { canDial, phone, type Call } from "../call/phone.svelte";
  import { myAgent, queues, type Agent, type Queue, type QueueCall } from "./queue.svelte";

  let notice = $state("");
  let busy = $state<string | null>(null);
  let now = $state(Date.now());

  const ready = $derived(canDial());
  const anyCalls = $derived(queues.queues.some((q) => q.calls.length));

  // Wartezeiten zählen nur, solange jemand in einer Queue ist
  $effect(() => {
    if (!anyCalls) return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  async function act(key: string, cmd: string, args: Record<string, unknown>) {
    notice = "";
    busy = key;
    try {
      await invoke(cmd, args);
    } catch (e) {
      notice = String(e);
    } finally {
      busy = null;
    }
  }

  const mmss = (s: number) => `${Math.floor(s / 60)}:${String(Math.max(0, s) % 60).padStart(2, "0")}`;
  const since = (ms: number) => mmss(Math.max(0, Math.floor((now - ms) / 1000)));

  /** „Nachname, Vorname“ → „Vorname Nachname“ */
  const shortName = (n: string) => {
    const [last, first] = n.split(",").map((x) => x.trim());
    return first ? `${first} ${last}` : n;
  };

  const agentName = (q: Queue, id: string) => {
    const a = q.agents.find((x) => x.user_id === id);
    return a ? shortName(a.name) : "";
  };

  const digits = (s: string) => s.replace(/\D/g, "");

  /** Klingelt der Queue-Anruf bei mir: der passende Anruf im Call Manager */
  function ringingHere(c: QueueCall): Call | undefined {
    if (c.state !== "ringing" || !c.agents.includes(queues.me)) return;
    const ringing = phone.status.calls.filter((x) => x.incoming && x.phase === "ringing");
    const n = digits(c.caller_number).slice(-8);
    return (n && ringing.find((x) => digits(x.remote_number).endsWith(n))) || (ringing.length === 1 ? ringing[0] : undefined);
  }

  /** Klingelt es bei mir, annehmen wie im Call Manager, sonst auf mein Telefon holen */
  function take(q: Queue, c: QueueCall) {
    const local = ringingHere(c);
    if (local) act(c.id, "phone_answer", { callId: local.id });
    else act(c.id, "queue_grab", { queue: q.id, call: c.id });
  }

  function callState(q: Queue, c: QueueCall) {
    const who = c.agents.map((id) => agentName(q, id)).filter(Boolean).join(", ");
    if (c.state === "connected") return who ? t("verbunden mit {name}", { name: who }) : t("verbunden");
    if (c.state === "ringing") return who ? t("klingelt bei {name}", { name: who }) : t("klingelt");
    return t("wartet");
  }

  /** Zustand eines Agenten aus Queue-Daten: abgemeldet, Nachbearbeitung, Gespräch, klingelt, frei */
  function agentState(q: Queue, a: Agent): { cls: string; text: string } {
    if (!a.logged_in) return { cls: "off", text: t("abgemeldet") };
    if (a.paused) return { cls: "wrapup", text: t("Nachbearbeitung") };
    const call = q.calls.find((c) => c.agents.includes(a.user_id));
    if (call?.state === "connected") return { cls: "busy", text: t("im Gespräch") };
    if (call?.state === "ringing") return { cls: "ringing", text: t("klingelt") };
    return { cls: "free", text: t("frei") };
  }
</script>

<div class="queues">
  {#if notice && connection.online}<p class="error">{notice}</p>{/if}
  {#each queues.queues as q (q.id)}
    {@const me = myAgent(q)}
    <section class="queue">
      <header>
        <strong class="name">{q.name}</strong>
        {#if me}
          {@const s = agentState(q, me)}
          <span class="state {s.cls}"><span class="lamp"></span>{s.text}</span>
          <button
            class="login"
            class:on={me.logged_in}
            disabled={busy === `login-${q.id}`}
            onclick={() => act(`login-${q.id}`, "queue_login", { id: q.id, on: !me.logged_in })}
          >{me.logged_in ? t("Abmelden") : t("Anmelden")}</button>
        {/if}
      </header>

      <div class="stats">
        <span title={t("Anrufer in der Warteschlange")}><b>{q.stats.callers}</b> {t("wartend")}</span>
        <span title={t("Angemeldete Agenten ohne Gespräch und Nachbearbeitung")}><b>{q.stats.free_agents}</b> {t("frei")}</span>
        <span title={t("Mittlere Wartezeit")}>Ø <b>{mmss(q.stats.avg_wait_secs)}</b></span>
        <span title={t("Anrufe insgesamt")}><b>{q.stats.total}</b> {t("Anrufe")}</span>
        <span class:warn={q.stats.missed > 0} title={t("Verpasste Anrufe")}><b>{q.stats.missed}</b> {t("verpasst")}</span>
      </div>

      <div class="calls">
        {#each q.calls as c (c.id)}
          <div class="call {c.state}">
            <span class="pos">{c.state === "connected" ? "" : c.position || "·"}</span>
            <div class="who">
              <strong>
                {c.caller_name || c.caller_number || t("Unbekannt")}
                {#if c.priority > 0}<span class="prio" title={t("Priorisiert")}>★</span>{/if}
              </strong>
              <small>{[c.caller_name && c.caller_number, callState(q, c)].filter(Boolean).join(" · ")}</small>
              {#if Object.keys(c.bot_output).length}
                <dl class="bot">
                  {#each Object.entries(c.bot_output) as [k, v]}<dt>{k}</dt><dd>{v}</dd>{/each}
                </dl>
              {/if}
            </div>
            <span class="time" title={c.state === "connected" ? t("Gesprächsdauer") : t("Wartezeit")}>
              {since(c.state === "connected" && c.connected ? c.connected : c.incoming)}
            </span>
            {#if c.state !== "connected"}
              {@const here = ringingHere(c)}
              <button
                class="grab"
                title={here ? t("Annehmen") : t("Anruf auf mein Telefon holen")}
                disabled={(!here && !ready) || busy === c.id}
                onclick={() => take(q, c)}
              ><Icon name="call" size={18} /></button>
            {/if}
          </div>
        {:else}
          <p class="muted">{t("Niemand in der Warteschlange.")}</p>
        {/each}
      </div>

      <div class="agents">
        {#each q.agents as a (a.user_id)}
          {@const s = agentState(q, a)}
          <span class="agent {s.cls}" title={s.text}><span class="lamp"></span>{shortName(a.name)}</span>
        {/each}
      </div>
    </section>
  {:else}
    <p class="muted">{t("Du bist in keiner Warteschlange als Agent eingetragen.")}</p>
  {/each}
</div>

<style>
  .queues { display: flex; flex-direction: column; gap: 0.6rem; height: 100%; min-height: 0; overflow: auto; }
  .queue { background: var(--panel); border-radius: 4px; padding: 0.6rem 0.8rem; display: flex; flex-direction: column; gap: 0.5rem; }
  header { display: flex; align-items: center; gap: 0.6rem; flex-wrap: wrap; }
  .name { flex: 1; min-width: 0; }
  .login { padding: 0.25rem 0.8rem; border-radius: 999px; font-size: 0.9rem; }
  .login.on { background: var(--panel-2); }
  .login:not(.on) { background: var(--accent); color: var(--on-accent); border-color: var(--accent); }
  .stats { display: flex; gap: 0.9rem; flex-wrap: wrap; color: var(--muted); font-size: 0.9rem; }
  .stats b { color: var(--text); font-variant-numeric: tabular-nums; }
  .stats .warn b { color: var(--accent-text); }
  .calls { display: flex; flex-direction: column; }
  .call { display: flex; align-items: center; gap: 0.7rem; padding: 0.4rem 0.2rem; border-top: 1px solid var(--line); }
  .call.ringing .pos { animation: blink 0.6s steps(2) infinite; }
  .pos { width: 1.4rem; text-align: center; font-weight: 600; color: var(--accent-text); font-variant-numeric: tabular-nums; }
  .who { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .who small { color: var(--muted); }
  .prio { color: var(--accent-text); margin-left: 0.3rem; }
  .bot { display: grid; grid-template-columns: auto 1fr; gap: 0 0.5rem; margin: 0.2rem 0 0; font-size: 0.8rem; }
  .bot dt { color: var(--muted); }
  .bot dd { margin: 0; }
  .time { font-variant-numeric: tabular-nums; color: var(--muted); }
  .call.waiting .time, .call.ringing .time { color: var(--text); }
  .grab { width: 2.1rem; height: 2.1rem; padding: 0; border-radius: 50%; display: grid; place-items: center; background: var(--green); border: none; color: #fff; }
  .grab:disabled { opacity: 0.4; }
  .agents { display: flex; flex-wrap: wrap; gap: 0.3rem 0.8rem; font-size: 0.85rem; }
  .agent, .state { display: inline-flex; align-items: center; gap: 0.35rem; }
  .state { font-size: 0.85rem; color: var(--muted); }
  .lamp { width: 0.6rem; height: 0.6rem; border-radius: 50%; background: var(--line); flex: none; }
  .free .lamp { background: var(--green); }
  .busy .lamp { background: var(--red); }
  .ringing .lamp { background: var(--red); animation: blink 0.6s steps(2) infinite; }
  .wrapup .lamp { background: var(--accent); }
  .off .lamp { background: transparent; border: 1px solid var(--muted); }
  .off { color: var(--muted); }
  .muted { color: var(--muted); margin: 0.3rem 0; }
  .error { color: var(--accent-text); margin: 0; }
  @keyframes blink { to { opacity: 0.2; } }
</style>
