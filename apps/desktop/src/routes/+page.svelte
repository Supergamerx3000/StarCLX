<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import Phone from "$lib/Phone.svelte";

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

{#if phase === "session" && session}
  <header>
    <div>
      <strong>{session.display_name}</strong>
      <span class="sub">STARFACE {session.server_version} · {session.server}</span>
    </div>
    <button onclick={logout}>Abmelden</button>
  </header>
  <main class="app">
    <Phone />
    {#if notice}<p class="notice">{notice}</p>{/if}
  </main>
{:else}
<main>
  {#if phase === "restoring"}
    <p>Verbinde …</p>
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
{/if}

<style>
  :global(:root) {
    font-family: system-ui, sans-serif;
    color: #1f2328;
    background: #f6f8fa;
    --card: #ffffff;
    --line: #d0d7de;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) { color: #e6edf3; background: #0d1117; --card: #161b22; --line: #30363d; }
  }
  :global(body) { margin: 0; }
  :global(input), :global(button) {
    font: inherit; padding: 0.6rem 0.8rem; border-radius: 6px; border: 1px solid #8b949e;
    background: var(--card); color: inherit;
  }
  :global(button) { cursor: pointer; }
  header {
    display: flex; align-items: center; justify-content: space-between; gap: 1rem;
    padding: 0.75rem 1.25rem; border-bottom: 1px solid var(--line); background: var(--card);
  }
  header div { display: flex; flex-direction: column; }
  .sub { font-size: 0.85rem; opacity: 0.7; }
  main { max-width: 28rem; margin: 15vh auto 0; padding: 0 1rem; }
  main.app { max-width: 40rem; margin: 1.5rem auto; }
  form { display: flex; flex-direction: column; gap: 0.75rem; }
  .notice { color: #9a6700; }
</style>
