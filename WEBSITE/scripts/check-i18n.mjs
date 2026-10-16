#!/usr/bin/env node
// Comprueba las traducciones del sitio (sin dependencias):
//   1. en y es tienen exactamente las mismas claves y ningún valor vacío.
//   2. Cada clave usada en index.html (data-i18n, data-i18n-html, data-i18n-attr)
//      y en main.js (t('...')) existe en ambos diccionarios.
//   3. El texto por defecto de index.html coincide con el diccionario en inglés
//      (lo que ven los buscadores y quien navega sin JS).
//   4. No quedan textos en español sin marcar en index.html.
//   5. La página en español que genera prerender.mjs no deja claves sin traducir
//      ni textos en inglés de index.html, y su JSON-LD es válido.
//   6. El JSON-LD de index.html está al día (node WEBSITE/scripts/prerender.mjs --sync-source).
//
//   node WEBSITE/scripts/check-i18n.mjs
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
import { renderPage, structuredData, translateHtml } from './prerender.mjs';

const site = join(dirname(fileURLToPath(import.meta.url)), '..');
const html = readFileSync(join(site, 'index.html'), 'utf8');
const mainJs = readFileSync(join(site, 'main.js'), 'utf8');
const i18nJs = readFileSync(join(site, 'i18n.js'), 'utf8');

// i18n.js sin DOM: solo expone globalThis.AdocI18n con los diccionarios.
const sandbox = {};
vm.runInNewContext(readFileSync(join(site, 'i18n.js'), 'utf8'), sandbox, { filename: 'i18n.js' });
const { MESSAGES, SUPPORTED } = sandbox.AdocI18n;

const errors = [];
const fail = (msg) => errors.push(msg);

// 1. Mismas claves en todos los idiomas.
const [base, ...others] = SUPPORTED;
const baseKeys = new Set(Object.keys(MESSAGES[base]));
for (const lang of others) {
  const keys = new Set(Object.keys(MESSAGES[lang] || {}));
  for (const k of baseKeys) if (!keys.has(k)) fail(`falta '${k}' en ${lang}`);
  for (const k of keys) if (!baseKeys.has(k)) fail(`'${k}' está en ${lang} pero no en ${base}`);
}
for (const lang of SUPPORTED) {
  for (const [k, v] of Object.entries(MESSAGES[lang] || {})) {
    if (typeof v !== 'string' || !v.trim()) fail(`valor vacío: ${lang}.${k}`);
  }
}

// 2. Claves usadas en el HTML y en main.js.
const collapse = (s) => s.replace(/\s+/g, ' ').trim();
const decodeAttr = (s) => s.replace(/&quot;/g, '"').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&');
const used = new Set();
const requireKey = (key, where) => {
  used.add(key);
  for (const lang of SUPPORTED) if (!(key in MESSAGES[lang])) fail(`${where}: clave '${key}' no existe en ${lang}`);
};

// data-i18n: el elemento solo debe contener texto (sin etiquetas hijas).
const textRe = /<(\w+)\b([^>]*?)\sdata-i18n="([^"]+)"([^>]*)>([^<]*)<\/\1>/g;
let count = 0;
for (const m of html.matchAll(textRe)) {
  count++;
  const key = m[3];
  requireKey(key, 'data-i18n');
  const text = collapse(decodeAttr(m[5]));
  if (key in MESSAGES.en && text !== collapse(MESSAGES.en[key])) {
    fail(`index.html difiere de en['${key}']:\n    html: ${text}\n    en:   ${collapse(MESSAGES.en[key])}`);
  }
}
const total = (html.match(/\sdata-i18n="/g) || []).length;
if (count !== total) fail(`hay ${total - count} elementos data-i18n con etiquetas hijas (usa data-i18n-html)`);

// data-i18n-html: sin anidar la misma etiqueta dentro.
const htmlRe = /<(\w+)\b[^>]*\sdata-i18n-html="([^"]+)"[^>]*>([\s\S]*?)<\/\1>/g;
count = 0;
for (const m of html.matchAll(htmlRe)) {
  count++;
  const key = m[2];
  requireKey(key, 'data-i18n-html');
  if (key in MESSAGES.en && collapse(m[3]) !== collapse(MESSAGES.en[key])) {
    fail(`index.html difiere de en['${key}']:\n    html: ${collapse(m[3])}\n    en:   ${collapse(MESSAGES.en[key])}`);
  }
}
if (count !== (html.match(/\sdata-i18n-html="/g) || []).length) fail('no se pudieron leer todos los data-i18n-html');

// data-i18n-attr="attr:clave;attr2:clave2": el valor actual del atributo debe ser el inglés.
for (const m of html.matchAll(/<\w+\b[^>]*\sdata-i18n-attr="([^"]+)"[^>]*>/g)) {
  const tag = m[0];
  for (const pair of m[1].split(';')) {
    const idx = pair.indexOf(':');
    if (idx < 1) {
      fail(`data-i18n-attr mal formado: '${pair}'`);
      continue;
    }
    const attr = pair.slice(0, idx).trim();
    const key = pair.slice(idx + 1).trim();
    requireKey(key, `data-i18n-attr(${attr})`);
    const value = tag.match(new RegExp(`\\s${attr}="([^"]*)"`));
    if (!value) fail(`<${tag.slice(1, 40)}…> no tiene el atributo por defecto '${attr}'`);
    else if (key in MESSAGES.en && decodeAttr(value[1]) !== MESSAGES.en[key]) {
      fail(`atributo ${attr} difiere de en['${key}']: '${value[1]}'`);
    }
  }
}

// Claves de main.js: t('clave') y FALLBACK, que debe coincidir con el inglés.
for (const m of mainJs.matchAll(/\bt\('([^']+)'/g)) requireKey(m[1], 'main.js');
// Textos que pinta i18n.js (aviso de idioma).
for (const m of i18nJs.matchAll(/\bt\('([^']+)'/g)) requireKey(m[1], 'i18n.js');
for (const m of mainJs.matchAll(/^\s*'(js\.[^']+)':\s*'([^']*)',?$/gm)) {
  requireKey(m[1], 'main.js FALLBACK');
  if (MESSAGES.en[m[1]] !== m[2]) fail(`FALLBACK de main.js difiere de en['${m[1]}']`);
}

// Claves definidas que nadie usa (aviso, no error).
const unused = [...baseKeys].filter((k) => !used.has(k));

// 4. Restos en español fuera de las cadenas traducibles (heurística por caracteres).
const visible = html
  .replace(/<!--[\s\S]*?-->/g, '')
  .replace(/<script[\s\S]*?<\/script>/g, '')
  .replace(/title="Español"/g, '');
const spanish = visible.match(/[^\n]*[áéíóúñ¿¡«»][^\n]*/g) || [];
for (const line of spanish) fail(`posible texto en español en index.html: ${line.trim()}`);

// 5. Página en español generada (en memoria).
const ldOf = (page, where) => {
  const m = page.match(/<script type="application\/ld\+json" id="structured-data">([\s\S]*?)<\/script>/);
  if (!m) {
    fail(`${where}: falta el bloque JSON-LD`);
    return null;
  }
  try {
    return JSON.parse(m[1]);
  } catch (err) {
    fail(`${where}: JSON-LD inválido (${err.message})`);
    return null;
  }
};
for (const lang of others) {
  let page = '';
  try {
    page = renderPage(lang);
  } catch (err) {
    fail(`prerender ${lang}: ${err.message}`);
    continue;
  }
  if (!page.includes(`<html lang="${lang}"`)) fail(`prerender ${lang}: <html lang> no es ${lang}`);
  // Una segunda pasada no debe encontrar nada que cambiar ni claves que falten.
  const again = translateHtml(page, MESSAGES[lang]);
  if (again.missing.length) fail(`prerender ${lang}: claves sin traducir: ${again.missing.join(', ')}`);
  // Ningún texto de un elemento traducible queda en inglés (salvo si coincide en ambos idiomas).
  for (const m of page.matchAll(textRe)) {
    const key = m[3];
    if (collapse(decodeAttr(m[5])) !== collapse(MESSAGES[lang][key])) fail(`prerender ${lang}: '${key}' no se tradujo`);
  }
  if (/\s(?:href|src)="(?!#|\/|\.\.\/|[a-z][a-z0-9+.-]*:)[^"]+"/i.test(page.replace(/<script type="application\/ld\+json"[\s\S]*?<\/script>/, ''))) {
    fail(`prerender ${lang}: quedan rutas relativas sin ajustar a ../`);
  }
  const ld = ldOf(page, `prerender ${lang}`);
  if (ld && JSON.stringify(ld) !== JSON.stringify(structuredData(lang, MESSAGES))) fail(`prerender ${lang}: JSON-LD inesperado`);
}

// 6. JSON-LD de index.html al día con los diccionarios.
const sourceLd = ldOf(html, 'index.html');
if (sourceLd && JSON.stringify(sourceLd) !== JSON.stringify(structuredData(base, MESSAGES))) {
  fail('el JSON-LD de index.html está desactualizado: ejecuta node WEBSITE/scripts/prerender.mjs --sync-source');
}

if (unused.length) console.warn(`aviso: claves sin usar: ${unused.join(', ')}`);
if (errors.length) {
  console.error(`check-i18n: ${errors.length} error(es)`);
  for (const e of errors) console.error(`  ✗ ${e}`);
  process.exit(1);
}
console.log(`check-i18n: ok (${baseKeys.size} claves × ${SUPPORTED.join('/')}, ${used.size} usadas)`);
