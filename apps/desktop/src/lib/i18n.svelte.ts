// Übersetzungen: der deutsche Text ist zugleich der Schlüssel, fehlt eine
// Übersetzung, bleibt er stehen. Die Sprache kommt aus den Einstellungen.
import en from "./i18n/en";
import fr from "./i18n/fr";
import it from "./i18n/it";

export type Language = "de" | "en" | "fr" | "it";

const dicts: Record<string, Record<string, string>> = { en, fr, it };
const locales: Record<Language, string> = { de: "de-CH", en: "en-GB", fr: "fr-CH", it: "it-CH" };

const i18n = $state({ lang: "de" as Language });

/** Sprache setzen; Unbekanntes fällt auf Deutsch zurück. */
export function setLanguage(lang: string | undefined | null) {
  const l = (lang && lang in locales ? lang : "de") as Language;
  i18n.lang = l;
  if (typeof document !== "undefined") document.documentElement.lang = l;
}

/** Übersetzt einen deutschen Text; `{name}` wird durch `vars.name` ersetzt. */
export function t(de: string, vars?: Record<string, string | number>): string {
  const text = dicts[i18n.lang]?.[de] ?? de;
  return vars ? text.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m)) : text;
}

/** Gebietsschema für Datum und Zahlen */
export function locale(): string {
  return locales[i18n.lang];
}
