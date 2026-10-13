// Comprueba la internacionalización (se ejecuta al final de `npm run compile`):
// - los diccionarios en/es de src/strings.ts tienen las mismas claves y los
//   mismos marcadores `{nombre}` en cada texto;
// - package.nls.json y package.nls.es.json tienen las mismas claves y cubren
//   todos los `%clave%` de package.json;
// - cada `t('clave')` de media/*.js existe en el diccionario.
// Necesita out/strings.js (tsc): `node scripts/check-i18n.js` tras compilar.

'use strict';
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const errors = [];

/** Diferencias entre dos conjuntos de claves, con nombres para el mensaje. */
function compareKeys(a, b, nameA, nameB) {
  const setA = new Set(Object.keys(a));
  const setB = new Set(Object.keys(b));
  for (const k of setA) if (!setB.has(k)) errors.push(`${nameB}: falta la clave "${k}" (está en ${nameA})`);
  for (const k of setB) if (!setA.has(k)) errors.push(`${nameA}: falta la clave "${k}" (está en ${nameB})`);
}

const placeholders = (text) => [...String(text).matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(',');

// ── Diccionarios de tiempo de ejecución ──
const { dictionaries } = require(path.join(root, 'out', 'strings.js'));
const { en, es } = dictionaries;
compareKeys(en, es, 'en (src/strings.ts)', 'es (src/strings.ts)');
for (const k of Object.keys(en)) {
  if (k in es && placeholders(en[k]) !== placeholders(es[k])) {
    errors.push(`src/strings.ts: "${k}" tiene marcadores distintos en en ({${placeholders(en[k])}}) y es ({${placeholders(es[k])}})`);
  }
}

// ── package.nls*.json ──
const readJson = (f) => JSON.parse(fs.readFileSync(path.join(root, f), 'utf8'));
const nlsEn = readJson('package.nls.json');
const nlsEs = readJson('package.nls.es.json');
compareKeys(nlsEn, nlsEs, 'package.nls.json', 'package.nls.es.json');
const pkgText = fs.readFileSync(path.join(root, 'package.json'), 'utf8');
JSON.parse(pkgText);
for (const m of pkgText.matchAll(/"%([\w.-]+)%"/g)) {
  if (!(m[1] in nlsEn)) errors.push(`package.json: "%${m[1]}%" no está en package.nls.json`);
}

// ── Claves usadas en los webviews ──
for (const file of fs.readdirSync(path.join(root, 'media')).filter((f) => f.endsWith('.js'))) {
  const text = fs.readFileSync(path.join(root, 'media', file), 'utf8');
  for (const m of text.matchAll(/\bt\('([\w.-]+)'/g)) {
    if (!(m[1] in en)) errors.push(`media/${file}: la clave "${m[1]}" no existe en src/strings.ts`);
  }
}

if (errors.length) {
  console.error(`check-i18n: ${errors.length} problema(s)\n  ${errors.join('\n  ')}`);
  process.exit(1);
}
console.log(
  `check-i18n: OK (${Object.keys(en).length} textos en/es, ${Object.keys(nlsEn).length} claves en package.nls*.json)`,
);
