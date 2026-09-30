<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

  type SessionInfo = { server_version: string; display_name: string };

  let server = $state("");
  let busy = $state(false);
  let error = $state("");
  let session = $state<SessionInfo | null>(null);

  onMount(() => {
    const offs = [
      listen<SessionInfo>("session", (e) => { session = e.payload; busy = false; }),
      listen<string>("login-error", (e) => { error = e.payload; busy = false; }),
    ];
    return () => offs.forEach((p) => p.then((off) => off()));
  });

  async function login(event: Event) {
    event.preventDefault();
    error = "";
    busy = true;
    try {
      await invoke("start_login", { server });
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }
</script>

<main>
  {#if session}
    <h1>Angemeldet als {session.display_name}</h1>
    <p>STARFACE {session.server_version}</p>
  {:else}
    <h1>Anmelden</h1>
    <form onsubmit={login}>
      <input placeholder="https://anlage.example.com" bind:value={server} required />
      <button type="submit" disabled={busy}>{busy ? "Warte auf Browser …" : "Im Browser anmelden"}</button>
    </form>
    {#if error}<p class="error">{error}</p>{/if}
  {/if}
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
  .error { color: #cf222e; }
</style>
