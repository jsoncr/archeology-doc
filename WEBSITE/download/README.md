# Descargas

Carpeta generada; no se versiona (salvo este archivo).

- **Local:** `WEBSITE/dist.sh --base-url https://<sitio>/download` compila macOS de forma nativa y
  Linux/Windows en Docker, y deja aquí los binarios, la extensión, los instaladores,
  `SHA256SUMS` y `latest.json` (que lee la página).
- **CI:** el workflow `Release` (tag `v*`) hace lo mismo en GitHub Actions, crea el release y
  publica el sitio en GitHub Pages. Define la variable del repositorio `SITE_URL`.
