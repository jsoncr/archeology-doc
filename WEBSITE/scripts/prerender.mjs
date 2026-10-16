#!/usr/bin/env node
// Genera la página en español (/es/index.html) a partir de index.html y del
// diccionario `es` de i18n.js, sin dependencias: aplica data-i18n,
// data-i18n-html y data-i18n-attr en el servidor, fija <html lang="es">, ajusta
// las rutas relativas (../) y regenera los datos estructurados (JSON-LD).
//
//   node WEBSITE/scripts/prerender.mjs                 # vista previa: WEBSITE/es/index.html (ignorado por git)
//   node WEBSITE/scripts/prerender.mjs --out dist/site # build: dist/site/index.html, es/index.html y sitemap.xml
//   node WEBSITE/scripts/prerender.mjs --sync-source   # reescribe el JSON-LD de WEBSITE/index.html
//
// Con --out distinto de WEBSITE/ también escribe <out>/index.html (inglés con la
// versión de download/latest.json en el JSON-LD) y <out>/sitemap.xml con
// <lastmod> de hoy (o --lastmod AAAA-MM-DD). WEBSITE/index.html nunca se toca
// salvo con --sync-source.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';

export const SITE_URL = 'https://archeology-doc.com/';
const REPO_URL = 'https://github.com/jsoncr/archeology-doc';
const LICENSE_URL = `${REPO_URL}/blob/main/LICENSE-MIT`;
const MARKETPLACE_URL = 'https://marketplace.visualstudio.com/items?itemName=jcruz.archeology-doc';
const FAQ_COUNT = 7;

export const siteDir = join(dirname(fileURLToPath(import.meta.url)), '..');

// Diccionarios de i18n.js (sin DOM: solo expone globalThis.AdocI18n).
export function loadI18n(site = siteDir) {
  const sandbox = {};
  vm.runInNewContext(readFileSync(join(site, 'i18n.js'), 'utf8'), sandbox, { filename: 'i18n.js' });
  return sandbox.AdocI18n;
}

const escapeText = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const escapeAttr = (s) => escapeText(s).replace(/"/g, '&quot;');
// Texto plano de una cadena con marcado sencillo (<code>, <b>, <a>…) para el JSON-LD.
export const plainText = (html) =>
  html
    .replace(/<[^>]+>/g, '')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, '&')
    .replace(/\s+/g, ' ')
    .trim();

// Aplica un diccionario al HTML. Devuelve el HTML y las claves que faltan.
export function translateHtml(html, dict) {
  const missing = new Set();
  const get = (key) => {
    if (!(key in dict)) {
      missing.add(key);
      return null;
    }
    return dict[key];
  };
  // data-i18n-html: contenido con marcado (cadenas fijas de i18n.js).
  html = html.replace(/<(\w+)\b([^>]*\sdata-i18n-html="([^"]+)"[^>]*)>([\s\S]*?)<\/\1>/g, (m, tag, attrs, key, inner) => {
    const value = get(key);
    return value === null ? m : `<${tag}${attrs}>${value}</${tag}>`;
  });
  // data-i18n: solo texto.
  html = html.replace(/<(\w+)\b([^>]*\sdata-i18n="([^"]+)"[^>]*)>([^<]*)<\/\1>/g, (m, tag, attrs, key) => {
    const value = get(key);
    return value === null ? m : `<${tag}${attrs}>${escapeText(value)}</${tag}>`;
  });
  // data-i18n-attr="attr:clave;attr2:clave2"
  html = html.replace(/<\w+\b[^>]*\sdata-i18n-attr="([^"]+)"[^>]*>/g, (tag, spec) => {
    for (const pair of spec.split(';')) {
      const idx = pair.indexOf(':');
      if (idx < 1) continue;
      const attr = pair.slice(0, idx).trim();
      const value = get(pair.slice(idx + 1).trim());
      if (value === null) continue;
      tag = tag.replace(new RegExp(`(\\s${attr}=")[^"]*(")`), (_, a, b) => `${a}${escapeAttr(value)}${b}`);
    }
    return tag;
  });
  return { html, missing: [...missing] };
}

// Rutas relativas (href, src, srcset) para una página un nivel por debajo de la raíz.
const isRelative = (url) => url && !/^(#|\/|[a-z][a-z0-9+.-]*:)/i.test(url);
const prefixUrl = (url, prefix) => (url === './' ? prefix : isRelative(url) ? prefix + url : url);
export function rebaseRelativeUrls(html, prefix = '../') {
  // Los bloques de datos JSON-LD no se tocan (sus URL ya son absolutas).
  return html.replace(/(<script type="application\/ld\+json"[^>]*>[\s\S]*?<\/script>)|(\s(?:href|src)=")([^"]*)(")|(\ssrcset=")([^"]*)(")/g, (m, script, a1, url, a2, s1, set, s2) => {
    if (script) return script;
    if (a1) return `${a1}${prefixUrl(url, prefix)}${a2}`;
    const rebased = set
      .split(',')
      .map((part) => {
        const [u, ...rest] = part.trim().split(/\s+/);
        return [prefixUrl(u, prefix), ...rest].join(' ');
      })
      .join(', ');
    return `${s1}${rebased}${s2}`;
  });
}

// Versión publicada (download/latest.json), o null si aún no hay.
export function readVersion(site = siteDir) {
  const file = join(site, 'download', 'latest.json');
  if (!existsSync(file)) return null;
  try {
    const version = JSON.parse(readFileSync(file, 'utf8')).version;
    return typeof version === 'string' && /^\d+\.\d+\.\d+/.test(version) ? version : null;
  } catch {
    return null;
  }
}

// Datos estructurados schema.org de la página de un idioma.
export function structuredData(lang, messages, { version } = {}) {
  const dict = messages[lang];
  const pageUrl = dict['meta.url'];
  const software = `${SITE_URL}#software`;
  const graph = [
    {
      '@type': 'WebSite',
      '@id': `${SITE_URL}#website`,
      url: SITE_URL,
      name: 'archeology-doc',
      alternateName: 'adoc',
      inLanguage: ['en', 'es'],
    },
    {
      '@type': 'WebPage',
      '@id': `${pageUrl}#webpage`,
      url: pageUrl,
      name: dict['meta.title'],
      description: dict['meta.description'],
      inLanguage: lang,
      isPartOf: { '@id': `${SITE_URL}#website` },
      about: { '@id': software },
      primaryImageOfPage: { '@type': 'ImageObject', url: dict['meta.ogImage'], width: 1200, height: 630 },
    },
    {
      '@type': 'SoftwareApplication',
      '@id': software,
      name: 'archeology-doc',
      alternateName: 'adoc',
      description: dict['meta.description'],
      applicationCategory: 'DeveloperApplication',
      operatingSystem: 'macOS, Linux, Windows',
      url: SITE_URL,
      downloadUrl: `${SITE_URL}download/`,
      installUrl: `${pageUrl}#instalar`,
      ...(version ? { softwareVersion: version } : {}),
      license: LICENSE_URL,
      isAccessibleForFree: true,
      offers: { '@type': 'Offer', price: '0', priceCurrency: 'USD' },
      screenshot: dict['meta.ogImage'],
      image: dict['meta.ogImage'],
      sameAs: [REPO_URL, MARKETPLACE_URL],
      author: { '@type': 'Organization', name: 'archeology-doc contributors', url: REPO_URL },
    },
    {
      '@type': 'SoftwareSourceCode',
      '@id': `${SITE_URL}#source`,
      name: 'archeology-doc',
      codeRepository: REPO_URL,
      programmingLanguage: 'Rust',
      license: LICENSE_URL,
      targetProduct: { '@id': software },
    },
    {
      '@type': 'FAQPage',
      '@id': `${pageUrl}#faq`,
      inLanguage: lang,
      mainEntity: Array.from({ length: FAQ_COUNT }, (_, i) => ({
        '@type': 'Question',
        name: dict[`faq.q${i + 1}`],
        acceptedAnswer: { '@type': 'Answer', text: plainText(dict[`faq.a${i + 1}`]) },
      })),
    },
  ];
  return { '@context': 'https://schema.org', '@graph': graph };
}

// JSON dentro de <script>: se escapa "<" para que nunca cierre la etiqueta.
const jsonForScript = (data) => JSON.stringify(data, null, 2).replace(/</g, '\\u003c');
const LD_RE = /(<script type="application\/ld\+json" id="structured-data">)[\s\S]*?(<\/script>)/;
export function setStructuredData(html, data) {
  if (!LD_RE.test(html)) throw new Error('index.html no tiene <script type="application/ld+json" id="structured-data">');
  const indented = jsonForScript(data).replace(/\n/g, '\n  ');
  return html.replace(LD_RE, (_, open, close) => `${open}\n  ${indented}\n  ${close}`);
}

// Página completa de un idioma. Lanza un error si falta alguna clave.
export function renderPage(lang, { site = siteDir, version = null, i18n = loadI18n(site) } = {}) {
  let html = readFileSync(join(site, 'index.html'), 'utf8');
  if (lang !== i18n.DEFAULT_LANG) {
    const { html: translated, missing } = translateHtml(html, i18n.MESSAGES[lang]);
    if (missing.length) throw new Error(`claves sin traducir en ${lang}: ${missing.join(', ')}`);
    html = rebaseRelativeUrls(translated, '../');
    // Idioma de la página y raíz del sitio para i18n.js/main.js.
    html = html.replace(/<html lang="[^"]*">/, `<html lang="${lang}" data-site-root="../">`);
    // Selector: la opción actual es este idioma.
    html = html.replace(/<a\b[^>]*\sdata-lang="(\w+)"[^>]*>/g, (tag, l) => {
      const clean = tag.replace(/\saria-current="page"/, '');
      return l === lang ? clean.replace(/>$/, ' aria-current="page">') : clean;
    });
    html = html.replace(
      '<!-- Página en inglés. /es/ la genera scripts/prerender.mjs aplicando el diccionario `es` de i18n.js. -->',
      `<!-- Generado por scripts/prerender.mjs desde index.html y el diccionario \`${lang}\` de i18n.js: no editar. -->`,
    );
  }
  return setStructuredData(html, structuredData(lang, i18n.MESSAGES, { version }));
}

// sitemap.xml con <lastmod> actualizado.
export function renderSitemap(site, lastmod) {
  const xml = readFileSync(join(site, 'sitemap.xml'), 'utf8');
  return xml.replace(/<lastmod>[^<]*<\/lastmod>/g, `<lastmod>${lastmod}</lastmod>`);
}

function main(argv) {
  const opts = { out: null, lastmod: new Date().toISOString().slice(0, 10), syncSource: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--out') opts.out = argv[++i];
    else if (arg === '--lastmod') opts.lastmod = argv[++i];
    else if (arg === '--sync-source') opts.syncSource = true;
    else if (arg === '-h' || arg === '--help') {
      console.log('uso: prerender.mjs [--out DIR] [--lastmod AAAA-MM-DD] [--sync-source]');
      return;
    } else throw new Error(`opción desconocida: ${arg}`);
  }
  if (!/^\d{4}-\d{2}-\d{2}$/.test(opts.lastmod || '')) throw new Error('--lastmod debe ser AAAA-MM-DD');
  const i18n = loadI18n();

  if (opts.syncSource) {
    // Inglés sin versión: el archivo versionado no cambia con cada release.
    const file = join(siteDir, 'index.html');
    writeFileSync(file, setStructuredData(readFileSync(file, 'utf8'), structuredData('en', i18n.MESSAGES)));
    console.log('prerender: JSON-LD actualizado en WEBSITE/index.html');
    return;
  }

  const out = resolve(opts.out || siteDir);
  const isBuild = out !== resolve(siteDir);
  // La versión solo se publica en el build (en la vista previa sería la de una compilación local).
  const version = isBuild ? readVersion() : null;
  const written = [];
  for (const lang of i18n.SUPPORTED) {
    if (lang === i18n.DEFAULT_LANG && !isBuild) continue;
    const dir = lang === i18n.DEFAULT_LANG ? out : join(out, i18n.PATHS[lang]);
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, 'index.html'), renderPage(lang, { version, i18n }));
    written.push(join(dir, 'index.html'));
  }
  if (isBuild) {
    writeFileSync(join(out, 'sitemap.xml'), renderSitemap(siteDir, opts.lastmod));
    written.push(join(out, 'sitemap.xml'));
  }
  console.log(`prerender: ${written.join(', ')}${version ? ` (v${version})` : ''}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (err) {
    console.error(`prerender: ${err.message}`);
    process.exit(1);
  }
}
