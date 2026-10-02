/** Hängt ein Element an <body>, damit Dialoge aus einer Kachel des
 *  Arbeitsbereichs nicht hinter anderen Kacheln liegen (jede Kachel hat
 *  einen eigenen z-index). */
export function portal(node: HTMLElement) {
  document.body.appendChild(node);
  return { destroy: () => node.remove() };
}
