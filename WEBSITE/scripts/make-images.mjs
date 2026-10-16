#!/usr/bin/env node
// Genera las imágenes optimizadas del sitio a partir de los originales de assets/.
// Herramienta puntual (no forma parte del build): solo hace falta volver a
// ejecutarla si cambian logo.png, logo-dark.png o icon.png, o los textos de la
// tarjeta social.
//
//   PLAYWRIGHT_CORE=/ruta/a/playwright-core node WEBSITE/scripts/make-images.mjs
//
// Necesita macOS (`sips` para redimensionar PNG) y Google Chrome instalado
// (Playwright, canal 'chrome'): el canvas de Chrome codifica WebP y la tarjeta
// social (og-image*.png, 1200×630) se renderiza como HTML y se captura.
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync, statSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const site = join(dirname(fileURLToPath(import.meta.url)), '..');
const assets = join(site, 'assets');
const DEFAULT_PW =
  '/Users/mac/.nvm/versions/node/v22.23.1/lib/node_modules/@playwright/cli/node_modules/playwright-core';
const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_CORE || DEFAULT_PW);

// Redimensiona con sips (ancho fijo; el alto mantiene la proporción).
const resize = (src, out, width, height) => {
  const args = height ? ['-z', String(height), String(width)] : ['--resampleWidth', String(width)];
  execFileSync('sips', [...args, join(assets, src), '--out', join(assets, out)], { stdio: 'ignore' });
};

// PNG redimensionados: logo del hero (respaldo sin WebP, 1×), icono de la cabecera, favicons y PWA.
const pngJobs = [
  ['logo.png', 'logo-440.png', 440],
  ['logo-dark.png', 'logo-dark-440.png', 440],
  ['icon.png', 'icon-64.png', 64, 64],
  ['icon.png', 'favicon-32.png', 32, 32],
  ['icon.png', 'favicon-16.png', 16, 16],
  ['icon.png', 'apple-touch-icon.png', 180, 180],
  ['icon.png', 'icon-192.png', 192, 192],
];
for (const [src, out, w, h] of pngJobs) resize(src, out, w, h);

const dataUrl = (file) => `data:image/png;base64,${readFileSync(join(assets, file)).toString('base64')}`;

const browser = await chromium.launch({ channel: 'chrome' });
try {
  const page = await browser.newPage();
  await page.setContent('<!doctype html><title>webp</title>');

  // WebP con el canvas de Chrome (conserva la transparencia).
  // width (opcional) reescala manteniendo la proporción.
  const toWebp = async (src, out, quality, width) => {
    const b64 = await page.evaluate(
      async ({ url, q, w }) => {
        const img = new Image();
        img.src = url;
        await img.decode();
        const canvas = document.createElement('canvas');
        canvas.width = w || img.naturalWidth;
        canvas.height = Math.round((canvas.width * img.naturalHeight) / img.naturalWidth);
        const ctx = canvas.getContext('2d');
        ctx.imageSmoothingQuality = 'high';
        ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
        return canvas.toDataURL('image/webp', q).split(',')[1];
      },
      { url: dataUrl(src), q: quality, w: width },
    );
    writeFileSync(join(assets, out), Buffer.from(b64, 'base64'));
  };
  for (const [src, out, width] of [
    ['logo.png', 'logo-880.webp', 880],
    ['logo-440.png', 'logo-440.webp'],
    ['logo-dark.png', 'logo-dark-880.webp', 880],
    ['logo-dark-440.png', 'logo-dark-440.webp'],
  ]) await toWebp(src, out, 0.86, width);
  await toWebp('icon-64.png', 'icon-64.webp', 0.9);

  // Tarjeta social 1200×630: logo para fondo oscuro + lema, sin recursos externos.
  const cards = {
    'og-image.png': {
      lang: 'en',
      title: 'Document any codebase with your AI agent',
      sub: 'Knowledge base, docs and AGENTS.md for new or legacy projects — every claim cites path:line.',
      tags: ['Open source · MIT', 'Claude Code · Codex · Gemini · Copilot…', 'macOS · Linux · Windows'],
    },
    'og-image-es.png': {
      lang: 'es',
      title: 'Documenta cualquier código con tu agente de IA',
      sub: 'Base de conocimiento, documentación y AGENTS.md para proyectos nuevos o legacy — cada afirmación cita ruta:línea.',
      tags: ['Código abierto · MIT', 'Claude Code · Codex · Gemini · Copilot…', 'macOS · Linux · Windows'],
    },
  };
  const logo = dataUrl('logo-dark.png');
  await page.setViewportSize({ width: 1200, height: 630 });
  for (const [out, c] of Object.entries(cards)) {
    await page.setContent(`<!doctype html><html lang="${c.lang}"><head><meta charset="utf-8"><style>
      * { box-sizing: border-box; margin: 0; }
      body { width: 1200px; height: 630px; background: #0a1730; color: #e6ecf5;
        font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
        display: grid; grid-template-columns: 470px 1fr; align-items: center; gap: 36px; padding: 0 64px 0 48px;
        border-bottom: 10px solid #f5a524; }
      img { width: 470px; height: auto; }
      .site { color: #22c7e8; font-weight: 700; font-size: 26px; letter-spacing: .02em; margin-bottom: 18px; }
      h1 { font-size: 50px; line-height: 1.1; text-wrap: balance; letter-spacing: -.025em; margin-bottom: 22px; }
      p { font-size: 25px; line-height: 1.4; color: #b8c4d8; margin-bottom: 28px; }
      ul { list-style: none; padding: 0; display: flex; flex-wrap: wrap; gap: 10px; }
      li { font-size: 19px; font-weight: 600; padding: 7px 14px; border-radius: 999px; border: 2px solid #1f3a66; color: #e6ecf5; }
    </style></head><body>
      <img src="${logo}" alt="">
      <div><div class="site">archeology-doc.com</div><h1>${c.title}</h1><p>${c.sub}</p>
      <ul>${c.tags.map((t) => `<li>${t}</li>`).join('')}</ul></div>
    </body></html>`);
    await page.waitForLoadState('load');
    await page.screenshot({ path: join(assets, out), type: 'png' });
  }
} finally {
  await browser.close();
}

for (const f of ['logo-880.webp', 'logo-dark-880.webp', 'logo-440.webp', 'icon-64.webp', 'og-image.png', 'og-image-es.png', 'apple-touch-icon.png']) {
  console.log(`${f}\t${Math.round(statSync(join(assets, f)).size / 1024)} KB`);
}
