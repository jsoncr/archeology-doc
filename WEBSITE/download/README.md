# Downloads

Generated folder; not versioned (except this file).

- **Local:** `WEBSITE/dist.sh --base-url https://archeology-doc.com/download` builds macOS natively and
  Linux/Windows in Docker, and places here the binaries, the extension, the installers,
  `SHA256SUMS` and `latest.json` (read by the page).
- **CI:** the `Release` workflow (tag `v*`) does the same on GitHub Actions, creates the release
  and publishes the site on Cloudflare Pages (https://archeology-doc.com).

## Descargas (español)

Carpeta generada; no se versiona (salvo este archivo).

- **Local:** `WEBSITE/dist.sh --base-url https://archeology-doc.com/download` compila macOS de forma nativa y
  Linux/Windows en Docker, y deja aquí los binarios, la extensión, los instaladores,
  `SHA256SUMS` y `latest.json` (que lee la página).
- **CI:** el workflow `Release` (tag `v*`) hace lo mismo en GitHub Actions, crea el release y
  publica el sitio en Cloudflare Pages (https://archeology-doc.com).
