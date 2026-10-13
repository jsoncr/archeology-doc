// Copia el renderizador compartido del grafo (copia canónica en
// crates/archeology-core/assets/) a media/: lo usan el CLI (HTML, reporte,
// sitio) y el dashboard de la extensión. Se ejecuta en `npm run compile`
// (y por tanto en `vscode:prepublish` / `npm run package`).

'use strict';
const fs = require('fs');
const path = require('path');

const src = path.join(__dirname, '..', '..', 'crates', 'archeology-core', 'assets');
const dest = path.join(__dirname, '..', 'media');

for (const file of ['graph.js', 'graph.css']) {
  const from = path.join(src, file);
  if (!fs.existsSync(from)) {
    console.error(`copy-graph: no existe ${from}`);
    process.exit(1);
  }
  fs.copyFileSync(from, path.join(dest, file));
}
