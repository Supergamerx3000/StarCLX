#!/usr/bin/env node
// Prüft, ob alle t("…")-Texte der Oberfläche in en/fr/it übersetzt sind und
// ob die Wörterbücher dieselben Schlüssel haben. Ohne Abhängigkeiten.
//   node scripts/i18n-check.mjs          Prüfen (Exit 1 bei Lücken)
//   node scripts/i18n-check.mjs --keys   Alle gefundenen Schlüssel als JSON ausgeben
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = join(root, "src");
const langs = ["en", "fr", "it"];

function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    if (e.isDirectory()) return e.name === "i18n" ? [] : walk(p);
    return /\.(svelte|ts)$/.test(e.name) ? [p] : [];
  });
}

/** Zeichenkette ab `i` (öffnendes Anführungszeichen) lesen; null bei `${…}`. */
function literal(text, i) {
  const q = text[i];
  let out = "";
  for (let j = i + 1; j < text.length; j++) {
    const c = text[j];
    if (c === "\\") {
      const n = text[++j];
      out += n === "n" ? "\n" : n === "t" ? "\t" : n;
    } else if (c === q) return { value: out, end: j };
    else if (q === "`" && c === "$" && text[j + 1] === "{") return null;
    else out += c;
  }
  return null;
}

const used = new Map(); // Schlüssel → erste Fundstelle
for (const file of walk(src)) {
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(/(?<![\w$.])t\(\s*(["'`])/g)) {
    const start = m.index + m[0].length - 1;
    const lit = literal(text, start);
    if (!lit) continue;
    const line = text.slice(0, m.index).split("\n").length;
    if (!used.has(lit.value)) used.set(lit.value, `${relative(root, file)}:${line}`);
  }
}

if (process.argv.includes("--keys")) {
  console.log(JSON.stringify([...used.keys()], null, 1));
  process.exit(0);
}

/** Wörterbuch einlesen: Zeilen der Form `"Schlüssel": "Wert",` */
function dict(lang) {
  const text = readFileSync(join(src, "lib", "i18n", `${lang}.ts`), "utf8");
  const keys = new Set();
  for (const m of text.matchAll(/^\s*("(?:[^"\\]|\\.)*")\s*:/gm)) keys.add(JSON.parse(m[1]));
  return keys;
}

let failed = false;
const dicts = Object.fromEntries(langs.map((l) => [l, dict(l)]));
for (const l of langs) {
  const missing = [...used.keys()].filter((k) => !dicts[l].has(k));
  for (const k of missing) console.error(`${l}: fehlt ${JSON.stringify(k)} (${used.get(k)})`);
  for (const o of langs) {
    for (const k of dicts[o]) if (!dicts[l].has(k)) console.error(`${l}: fehlt ${JSON.stringify(k)} (nur in ${o}.ts)`);
  }
  if (missing.length || [...dicts.en, ...dicts.fr, ...dicts.it].some((k) => !dicts[l].has(k))) failed = true;
}
if (failed) {
  console.error("Übersetzungen unvollständig.");
  process.exit(1);
}
console.log(`i18n: ${used.size} Texte im Code, ${dicts.en.size} Schlüssel je Sprache – vollständig.`);
