// Öffnet das Kontaktformular von überall (Adressbuch, Rufliste).

export type ContactField = { block: string; group: string; name: string; key: number; label: string; value: string };

export const contactEdit = $state({
  open: false,
  /** leer = neuer Kontakt */
  id: "",
  /** vorgeschlagenes Adressbuch für neue Kontakte */
  folder: "",
  /** vorbelegte Rufnummer, z. B. aus der Rufliste */
  number: "",
  /** zählt hoch, wenn ein Kontakt gespeichert oder gelöscht wurde */
  changed: 0,
});

export function newContact(opts: { folder?: string; number?: string } = {}) {
  Object.assign(contactEdit, { open: true, id: "", folder: opts.folder ?? "", number: opts.number ?? "" });
}

export function editContact(id: string) {
  Object.assign(contactEdit, { open: true, id, folder: "", number: "" });
}

/** ContactDisplayKey der Rufnummernfelder, bevorzugte zuerst */
export const PHONE_KEYS = [17, 19, 20, 18];
