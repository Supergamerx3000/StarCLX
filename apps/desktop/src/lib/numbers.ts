// Rufnummern in Texten erkennen und vergleichen.

/** Zerlegt einen Text in Teile; Rufnummern (ab 5 Ziffern) sind markiert. */
export function numberParts(text: string): { text: string; number?: string }[] {
  const parts: { text: string; number?: string }[] = [];
  const re = /(?:\+|\b)\d[\d /().-]{3,}\d\b/g;
  let last = 0;
  for (const m of text.matchAll(re)) {
    const digits = m[0].replace(/\D/g, "");
    // Datum (01.10.2026), Uhrzeit und kurze Zahlen sind keine Rufnummern
    if (digits.length < 5 || /^\d{1,2}\.\d{1,2}\.\d{2,4}$/.test(m[0])) continue;
    if (m.index > last) parts.push({ text: text.slice(last, m.index) });
    parts.push({ text: m[0], number: m[0].replace(/[^\d+]/g, "") });
    last = m.index + m[0].length;
  }
  if (last < text.length) parts.push({ text: text.slice(last) });
  return parts;
}

/** Form, in der die Kontaktsuche der Anlage eine Nummer findet: 0041… → +41… */
export function searchable(number: string): string {
  const n = number.replace(/[^\d+]/g, "");
  return n.startsWith("00") ? `+${n.slice(2)}` : n;
}
