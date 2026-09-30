<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

  type SessionInfo = { server: string; server_version: string; display_name: string };

  let server = $state("");
  let phase = $state<"restoring" | "login" | "waiting" | "session">("restoring");
  let notice = $state("");
  let session = $state<SessionInfo | null>(null);

  onMount(() => {
    const offs = [
      listen<SessionInfo>("session", (e) => { session = e.payload; notice = ""; phase = "session"; }),
      listen<string>("login-error", (e) => { notice = e.payload; phase = "login"; }),
      listen<string>("logged-out", (e) => { session = null; notice = e.payload; phase = "login"; }),
    ];
    restore();
    return () => offs.forEach((p) => p.then((off) => off()));
  });

  async function restore() {
    server = (await invoke<string | null>("last_server")) ?? "";
    try {
      const info = await invoke<SessionInfo | null>("restore_session");
      if (info) { session = info; phase = "session"; return; }
    } catch (e) {
      notice = `Automatische Anmeldung fehlgeschlagen: ${e}`;
    }
    phase = "login";
  }

  async function login(event: Event) {
    event.preventDefault();
    notice = "";
    phase = "waiting";
    try {
      await invoke("start_login", { server });
    } catch (e) {
      notice = String(e);
      phase = "login";
    }
  }

  async function logout() {
    try {
      await invoke("logout");
      notice = "Abgemeldet";
    } catch (e) {
      notice = `Abmelden unvollständig: ${e}`;
    }
    session = null;
    phase = "login";
  }
</script>

<main>
  {#if phase === "restoring"}
    <p>Verbinde …</p>
  {:else if phase === "session" && session}
    <h1>Angemeldet als {session.display_name}</h1>
    <p>STARFACE {session.server_version} · {session.server}</p>
    <button onclick={logout}>Abmelden</button>
  {:else}
    <h1>Anmelden</h1>
    <form onsubmit={login}>
      <input placeholder="https://anlage.example.com" bind:value={server} required />
      <button type="submit" disabled={phase === "waiting"}>
        {phase === "waiting" ? "Warte auf Browser …" : "Im Browser anmelden"}
      </button>
    </form>
  {/if}
  {#if notice}<p class="notice">{notice}</p>{/if}
</main>

<style>
  :root {
    font-family: system-ui, sans-serif;
    color: #1f2328;
    background: #f6f8fa;
  }
  @media (prefers-color-scheme: dark) {
    :root { color: #e6edf3; background: #0d1117; }
  }
  main { max-width: 28rem; margin: 15vh auto 0; padding: 0 1rem; }
  form { display: flex; flex-direction: column; gap: 0.75rem; }
  input, button { font: inherit; padding: 0.6rem 0.8rem; border-radius: 6px; border: 1px solid #8b949e; }
  button { cursor: pointer; }
  .notice { color: #9a6700; }
</style>
