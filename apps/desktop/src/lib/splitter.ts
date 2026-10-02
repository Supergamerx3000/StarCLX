// Verschiebbare Trennlinie zwischen zwei Spalten. Die Breite der linken
// Spalte landet in der CSS-Variablen --split des Elternelements und wird
// lokal gemerkt. Zeigerereignisse, weil HTML5-Drag&Drop im Linux-Fenster
// nicht funktioniert.

const MIN = 180;

function load(key: string): number | null {
  try {
    const v = Number(localStorage.getItem(`split:${key}`));
    return v > 0 ? v : null;
  } catch {
    return null;
  }
}

function save(key: string, px: number) {
  try {
    localStorage.setItem(`split:${key}`, String(Math.round(px)));
  } catch {
    // ohne Speicher gilt die Breite nur bis zum Neustart
  }
}

export function splitter(node: HTMLElement, key: string) {
  const parent = node.parentElement!;
  const set = (px: number) => parent.style.setProperty("--split", `${px}px`);
  const saved = load(key);
  if (saved) set(saved);

  let dragging = false;
  let px = 0;

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    node.setPointerCapture(e.pointerId);
    node.classList.add("dragging");
  }
  function move(e: PointerEvent) {
    if (!dragging) return;
    const r = parent.getBoundingClientRect();
    px = Math.min(Math.max(MIN, e.clientX - r.left), r.width - MIN);
    set(px);
  }
  function up() {
    if (!dragging) return;
    dragging = false;
    node.classList.remove("dragging");
    if (px) save(key, px);
  }

  node.addEventListener("pointerdown", down);
  node.addEventListener("pointermove", move);
  node.addEventListener("pointerup", up);
  node.addEventListener("pointercancel", up);
  return {
    destroy() {
      node.removeEventListener("pointerdown", down);
      node.removeEventListener("pointermove", move);
      node.removeEventListener("pointerup", up);
      node.removeEventListener("pointercancel", up);
    },
  };
}
