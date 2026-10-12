# Sitio de arqueology-doc

Página estática (HTML + CSS + JS, sin build ni dependencias): qué es el proyecto, cómo funciona, instalación
con bash, comandos, agentes soportados y la extensión de VS Code. Tema claro/oscuro automático (botón ◐). El logo tiene dos versiones: `assets/logo.png` (fondos claros) y `assets/logo-dark.png` (texto claro, para fondos oscuros); `assets/icon.png` es el icono de la app y `favicon.png` el favicon.

## Ver en local

```bash
open WEBSITE/index.html                 # macOS
# o con servidor: python3 -m http.server -d WEBSITE 8080  →  http://localhost:8080
```

## Descargas e instaladores

La sección «Instalar» detecta el sistema operativo y muestra el comando correspondiente:

- macOS / Linux: `curl -fsSL https://<sitio>/download/install.sh | sh`
- Windows: `irm https://<sitio>/download/install.ps1 | iex`

Los comandos usan la URL desde la que se abre la página. Los instaladores descargan el binario de
`/download`, verifican su SHA-256 contra `SHA256SUMS` y lo instalan sin pedir Rust. La tabla de
descargas se llena con `download/latest.json`.

`/download` se genera; no se versiona:

```bash
WEBSITE/dist.sh --base-url https://<sitio>/download   # macOS nativo + Linux/Windows en Docker
python3 -m http.server -d WEBSITE 8080                # probar en http://localhost:8080
```

## Publicar

Con GitHub: *Settings → Pages → Source: GitHub Actions*. Opcional: la variable del repositorio
`SITE_URL` para un dominio propio (*Settings → Secrets and variables → Actions → Variables*). Cada tag `v*` ejecuta
`.github/workflows/release.yml`: compila las cinco plataformas, crea el release y publica el sitio
con `/download` incluido.

En otro hosting estático (Netlify, Cloudflare Pages, S3) sube `WEBSITE/` después de ejecutar
`WEBSITE/dist.sh`.

## Repositorio

<https://github.com/jsoncr/arqueology-doc>. Con GitHub Pages el sitio queda en
<https://jsoncr.github.io/arqueology-doc> y los instaladores descargan de
`https://jsoncr.github.io/arqueology-doc/download`. Con un dominio propio, define `SITE_URL`.
