<script lang="ts">
  // Eine Funktionstaste mit Zustandsfarbe; im Arbeitsbereich und im Editor.
  // Besetztlampenfeld wie in der STARFACE-App: Bild auf einer Fläche in der
  // Telefonfarbe, Ruhe oben links, Chat-Status oben rechts, Umleitung unten
  // links, darunter der Statustext.
  import { account, avatarOf, blfUser, callDrag, chatText, keyState, keyTitle, typeInfo, type FunctionKey } from "./fkeys.svelte";
  import ChatBubble from "../../ChatBubble.svelte";
  import Icon from "../../Icon.svelte";
  import { t } from "../../i18n.svelte";

  let { key, onclick, disabled = false, editor = false }: { key: FunctionKey; onclick?: () => void; disabled?: boolean; editor?: boolean } = $props();

  // Leere Taste: im Betrieb nur die Fläche, ohne Text und nicht anklickbar
  const blank = $derived(key.functionKeyType === "SEPARATOR" && !editor);
  const blf = $derived(key.functionKeyType === "BUSYLAMPFIELD" && !blank);

  const info = $derived(typeInfo(key.functionKeyType));
  const state = $derived(keyState(key));
  const user = $derived(blf ? blfUser(key) : undefined);
  const presence = $derived(user?.state);
  // Gruppen haben kein Bild
  const avatar = $derived(user && !user.group ? avatarOf(user.id) : null);
  // Wie in der STARFACE-App aus dem angezeigten Namen, ohne „[Nummer]“
  const initials = $derived(
    keyTitle(key)
      .replace(/\[.*\]/, "")
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0]?.toUpperCase())
      .join(""),
  );
  const status = $derived(chatText(presence));
  // Schaltbare Tasten wie die Schnellaktionen der STARFACE-App: Schalter mit Symbol
  const toggle = $derived(
    blank
      ? undefined
      : ({ DONOTDISTURB: "dndOutline", MODULEACTIVATION: "module", SIGNALNUMBER: "eye", GROUPLOGIN: "groups", FORWARD: "redirect", FORWARDNUMBER: "redirect", FORWARDTOTARGET: "redirect" } as const)[
          key.functionKeyType as "DONOTDISTURB"
        ],
  );
  const sub = $derived.by(() => {
    switch (key.functionKeyType) {
      case "BUSYLAMPFIELD": return status || account(key)?.number || "";
      case "QUICKDIAL": return key.directCallTargetnumber ?? "";
      case "PARKANDORBIT": return key.poNumber ? t("Platz {n}", { n: key.poNumber }) : "";
      case "SIGNALNUMBER": return t("Rufnummer signalisieren");
      case "FORWARD": return t("Alle Rufnummern umleiten");
      case "FORWARDNUMBER":
      case "FORWARDTOTARGET": return t("Rufnummern umleiten");
      default: return key.name ? info.label : "";
    }
  });
  const stateText: Record<string, string> = $derived({ partial: t("teilaktiv"), on: t("aktiv"), busy: t("im Gespräch"), ringing: t("klingelt"), free: t("frei"), off: t("nicht erreichbar"), dnd: t("Bitte nicht stören"), parked: t("Gespräch geparkt") });
  // Gruppen-Taste: an = angemeldet
  const groupText: Record<string, string> = $derived({ on: t("angemeldet"), "": t("abgemeldet") });
  // Modul-Taste: an = aktiv, sonst aus
  const moduleText: Record<string, string> = $derived({ on: t("aktiv"), "": t("aus"), none: t("Modul nicht gefunden oder nicht freigegeben") });
  const tooltip = $derived(
    blank
      ? undefined
      : [
          info.label,
          (key.functionKeyType === "GROUPLOGIN" ? groupText : key.functionKeyType === "MODULEACTIVATION" ? moduleText : stateText)[state],
          blf && presence?.redirect ? t("Umleitung aktiv") : "",
          blf ? status : "",
        ]
          .filter(Boolean)
          .join(" · "),
  );
  // Vollständiger Name im Tooltip, falls er auf der Taste gekürzt ist
  const title = $derived(tooltip && keyTitle(key) !== info.label ? `${keyTitle(key)}
${tooltip}` : tooltip);
</script>

<button
  class="tile {state}"
  class:sep={key.functionKeyType === "SEPARATOR" && editor}
  class:blank
  class:unusable={!info.usable && !blank}
  class:target={callDrag.call && callDrag.over === key.id}
  data-fkey={key.id}
  title={title}
  disabled={disabled || blank}
  {onclick}
>
  {#if blf}
    <span class="blf">
      {#if state === "off"}<span class="dash">–</span>{/if}
      <span class="pic">
        {#if avatar}<img src={avatar} alt="" />{:else}{initials}{/if}
      </span>
      {#if presence?.dnd}
        <span class="badge dnd" title={t("Bitte nicht stören")}>
          <svg viewBox="0 0 12 12"><circle cx="6" cy="6" r="5.5" /><rect x="3" y="5.1" width="6" height="1.8" rx="0.5" /></svg>
        </span>
      {/if}
      {#if presence?.chat}
        <span class="badge chat"><ChatBubble state={presence.chat} /></span>
      {/if}
      {#if presence?.redirect}
        <span class="badge redirect" title={t("Umleitung aktiv")}>
          <svg viewBox="0 0 12 12"><circle cx="6" cy="6" r="5.5" /><path class="mark" d="M3.4 3V7.4h5M6.8 5.6l1.9 1.8-1.9 1.8" /></svg>
        </span>
      {/if}
    </span>
  {:else if key.functionKeyType === "QUICKDIAL"}
    <!-- Direktwahl wie ein Kontakt in der STARFACE-App -->
    <span class="contact"><Icon name="person" size={20} /></span>
  {:else if toggle}
    <span class="switch" class:on={state === "on"} class:partial={state === "partial"} class:none={state === "none"}>
      {#if state === "partial"}<span class="half">–</span>{/if}
      <span class="knob"><Icon name={toggle} size={15} /></span>
    </span>
  {:else}
    <span class="lamp"></span>
  {/if}
  {#if !blank}<span class="txt"><strong>{keyTitle(key)}</strong>{#if sub}<small>{sub}</small>{/if}</span>{/if}
  {#if state === "partial"}<span class="partial-pill"><Icon name="info" size={15} /> {t("Teilaktiv")}</span>{/if}
</button>

<style>
  /* Alle Tasten so hoch wie ein Besetztlampenfeld (Bild 2,5rem + Innenabstand) */
  .tile { width: 100%; min-height: calc(2.5rem + 0.8rem + 2px); box-sizing: border-box; display: flex; align-items: center; gap: 0.6rem; text-align: left; padding: 0.4rem 0.6rem; background: var(--panel-2); border: 1px solid var(--line); border-radius: 6px; }
  .tile:hover:not(:disabled) { border-color: var(--accent); }
  .lamp { width: 0.7rem; height: 0.7rem; border-radius: 50%; flex: none; background: var(--line); }
  .free .lamp { background: var(--green); }
  .busy .lamp, .on .lamp { background: var(--red); }
  .on .lamp { background: var(--accent); }
  .ringing .lamp { background: var(--red); animation: blink 0.6s steps(2) infinite; }
  .parked .lamp { background: var(--accent); animation: blink 0.6s steps(2) infinite; }
  .off .lamp { background: transparent; border: 1px solid var(--muted); }

  /* Besetztlampenfeld: Fläche in der Telefonfarbe, rechts darauf das Bild */
  .blf {
    position: relative; flex: none; width: 3.3rem; height: 2.5rem;
    border-radius: 1.25rem 1.25rem 1.25rem 0.25rem; background: #69737d;
  }
  /* Grau (kein Telefon, Ruhe): spitze Ecke unten rechts, wie in der STARFACE-App */
  .off .blf, .dnd .blf { border-radius: 1.25rem 1.25rem 0.25rem 1.25rem; }
  .free .blf { background: var(--green); }
  .busy .blf, .ringing .blf { background: var(--red); }
  .ringing .blf { animation: blink 0.6s steps(2) infinite; }
  .pic {
    position: absolute; right: 0.15rem; top: 0.15rem; width: 2.2rem; height: 2.2rem; border-radius: 50%;
    overflow: hidden; display: grid; place-items: center; background: #c9ccd0; color: #555b62;
    font-size: 0.8rem; font-weight: 600; letter-spacing: 0.02em;
  }
  .pic img { width: 100%; height: 100%; object-fit: cover; }
  .dash { position: absolute; left: 0.3rem; top: 50%; transform: translateY(-55%); color: #fff; font-weight: 700; }
  /* Symbole immer über .badge ansprechen: „dnd“ ist auch eine Zustandsklasse der Taste */
  .badge { position: absolute; display: grid; }
  .badge svg { width: 100%; height: 100%; overflow: visible; }
  .badge .mark { fill: none; stroke: #fff; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
  .badge.dnd { left: 0.05rem; top: -0.15rem; width: 0.95rem; height: 0.95rem; }
  .badge.dnd circle { fill: var(--red); stroke: #fff; stroke-width: 1; }
  .badge.dnd rect { fill: #fff; }
  .badge.chat { right: -0.35rem; top: -0.3rem; width: 1rem; height: 0.95rem; }
  .badge.redirect { left: 0.05rem; bottom: -0.15rem; width: 0.95rem; height: 0.95rem; }
  .badge.redirect circle { fill: #111; stroke: #fff; stroke-width: 1; }
  .badge.redirect .mark { stroke-width: 1.2; }

  .switch { position: relative; flex: none; width: 2.6rem; height: 1.45rem; border-radius: 999px; background: var(--line); transition: background 0.15s; }
  .switch.on { background: var(--accent); }
  .switch.partial { background: color-mix(in srgb, var(--accent) 45%, transparent); }
  .switch.none { opacity: 0.45; }
  .half { position: absolute; left: 0.45rem; top: 50%; transform: translateY(-55%); color: #fff; font-weight: 700; }
  .knob {
    position: absolute; left: 0.15rem; top: 0.15rem; width: 1.15rem; height: 1.15rem; border-radius: 50%;
    background: #fff; display: grid; place-items: center; transition: left 0.15s; box-shadow: 0 1px 2px #0004; color: #8a9096;
  }
  .switch.on .knob, .switch.partial .knob { left: 1.3rem; color: #3b4045; }
  /* Symbol im Knopf wie in der STARFACE-App: im dunklen Design dunkel, im hellen hellgrau */
  .knob { color: #2f3439; }
  :global(:root[data-theme="light"]) .knob { color: #9aa0a6; }
  :global(:root[data-theme="light"]) .switch.on .knob, :global(:root[data-theme="light"]) .switch.partial .knob { color: #6b7177; }
  .partial-pill { margin-left: auto; flex: none; display: inline-flex; align-items: center; gap: 0.2rem; font-size: 0.75rem; font-weight: 600; padding: 0.15rem 0.5rem; border-radius: 999px; background: var(--line); color: var(--text); white-space: nowrap; }
  .partial-pill :global(svg) { color: var(--muted); }
  .contact { flex: none; width: 2.2rem; height: 2.2rem; border-radius: 50%; display: grid; place-items: center; background: #c9ccd0; color: #555b62; }
  .txt { min-width: 0; display: flex; flex-direction: column; }
  /* Lange Namen (z. B. „Umleitung [+49 … → mailbox 11]“) auf zwei Zeilen, erst dann „…“ */
  .txt strong { font-weight: 600; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow-wrap: anywhere; line-height: 1.2; }
  .txt small { color: var(--muted); font-size: 0.78rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .unusable { opacity: 0.55; }
  .tile.target { border-color: var(--accent); outline: 2px solid var(--accent); }
  .sep { background: none; border-style: dashed; }
  .sep .lamp { visibility: hidden; }
  .blank { cursor: default; }
  .blank .lamp { visibility: hidden; }
  @keyframes blink { to { opacity: 0.2; } }
</style>
