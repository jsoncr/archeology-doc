#!/bin/sh
# Instalador de archeology-doc (CLI `adoc`) para macOS y Linux.
#
#   curl -fsSL https://archeology-doc.com/download/install.sh | sh
#   curl -fsSL https://archeology-doc.com/download/install.sh | sh -s -- --with-vscode
#   WEBSITE/installers/install.sh      # desde un clon: compila el código local
#
# Por defecto descarga el binario precompilado de la plataforma desde la
# carpeta /download del sitio y verifica su SHA-256. Si no hay binario para la
# plataforma (o con --from-source), compila desde el código con cargo.
#
# Opciones:
#   --with-vitepress   instala VitePress compartido (`adoc site setup`) para el sitio
#   --with-vscode      instala la extensión de VS Code (requiere el comando `code`)
#   --from-source      compila con cargo en lugar de descargar el binario
#   --install-dir <d>  carpeta de los binarios (por defecto ~/.local/bin)
#   --repo <url>       repositorio a clonar al compilar desde el código
#   --ref <tag|commit> versión del código a compilar (recomendado con --from-source)
#
# Variables: ARCHEOLOGY_DOC_DOWNLOAD (URL de /download), ARCHEOLOGY_DOC_INSTALL_DIR,
# ARCHEOLOGY_DOC_REPO (--repo), ARCHEOLOGY_DOC_REF (--ref), ADOC_LANG (en|es).
#
# Idioma de los mensajes: ADOC_LANG si está definido; si no, español cuando el
# locale (LC_ALL > LC_MESSAGES > LANG) empieza por "es", e inglés en otro caso.
set -eu

# --- Idioma de los mensajes -------------------------------------------------
detect_ui_lang() {
  case "${ADOC_LANG:-}" in
    es|es_*|es-*|es.*) echo es; return ;;
    en|en_*|en-*|en.*) echo en; return ;;
  esac
  loc="${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}"
  case "$loc" in
    es|es_*|es-*|es.*) echo es ;;
    *) echo en ;;
  esac
}
UI_LANG="$(detect_ui_lang)"

# Formatos printf por clave (inglés). Sin arrays: un case por idioma (POSIX sh).
msg_en() {
  case "$1" in
    opt_unknown) printf '%s' 'unknown option: %s' ;;
    need_dir) printf '%s' '--install-dir needs a folder' ;;
    need_repo) printf '%s' '--repo needs a url' ;;
    need_ref) printf '%s' '--ref needs a tag or commit' ;;
    title) printf '%s' 'archeology-doc · installation' ;;
    no_fetcher) printf '%s' 'curl or wget is required' ;;
    sums_dl_fail) printf '%s' 'could not download SHA256SUMS' ;;
    sums_missing) printf '%s' '%s is not listed in SHA256SUMS' ;;
    sha_calc_fail) printf '%s' 'could not compute the SHA-256 of %s' ;;
    sha_mismatch) printf '%s' 'SHA-256 of %s does not match: corrupted or tampered download' ;;
    clone_path_fail) printf '%s' 'could not resolve the clone path' ;;
    no_prebuilt) printf '%s' 'no prebuilt binary for %s' ;;
    musl) printf '%s' 'Linux with musl: the prebuilt binaries require glibc' ;;
    no_download_url) printf '%s' 'this installer has no download URL configured' ;;
    downloading) printf '%s' 'downloading %s…' ;;
    download_fail) printf '%s' 'could not download %s' ;;
    unverified) printf '%s' 'refusing to install %s without verification' ;;
    sha_ok) printf '%s' 'SHA-256 verified' ;;
    untar_fail) printf '%s' 'could not extract %s' ;;
    mkdir_fail) printf '%s' 'could not create %s' ;;
    install_fail) printf '%s' 'could not install %s into %s' ;;
    no_rust) printf '%s' "Rust is missing. Install it with: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" ;;
    no_git) printf '%s' 'git is required to clone %s' ;;
    cloning_ref) printf '%s' 'cloning %s @ %s…' ;;
    ref_fail) printf '%s' 'could not fetch %s from %s' ;;
    no_ref) printf '%s' 'no --ref: building the default branch (use --ref <tag> for a fixed version)' ;;
    clone_fail) printf '%s' 'could not clone %s' ;;
    source_at) printf '%s' 'source code in %s' ;;
    compiling) printf '%s' 'building (the first time takes a few minutes)…' ;;
    build_fail) printf '%s' 'build failed (see the error above)' ;;
    will_build) printf '%s' 'building from source instead' ;;
    bin_missing) printf '%s' 'installed binary not found at %s' ;;
    bin_broken) printf '%s' 'the installed binary does not run: %s' ;;
    add_path) printf '%s' "add %s to your PATH: echo 'export PATH=\"%s:\$PATH\"' >> ~/.zshrc  (or ~/.bashrc)" ;;
    vp_ok) printf '%s' 'shared VitePress installed' ;;
    vp_fail) printf '%s' '`adoc site setup` failed; run it again manually' ;;
    vp_no_npm) printf '%s' 'no npm: install Node 18+ and run `adoc site setup`' ;;
    no_code) printf '%s' "the \`code\` command is missing (VS Code → Shell Command: Install 'code' command in PATH)" ;;
    ext_ok) printf '%s' 'VS Code extension installed' ;;
    ext_build_fail) printf '%s' 'could not build or install the VS Code extension' ;;
    ext_install_fail) printf '%s' 'could not install the VS Code extension' ;;
    ext_unverified) printf '%s' 'extension not installed: SHA-256 verification failed' ;;
    ext_fetch_fail) printf '%s' 'could not get the extension' ;;
    done) printf '%s' 'Done. Next steps:' ;;
    *) printf '%s' "$1" ;;
  esac
}

# Formatos printf por clave (español).
msg_es() {
  case "$1" in
    opt_unknown) printf '%s' 'opción desconocida: %s' ;;
    need_dir) printf '%s' '--install-dir necesita una carpeta' ;;
    need_repo) printf '%s' '--repo necesita una url' ;;
    need_ref) printf '%s' '--ref necesita un tag o commit' ;;
    title) printf '%s' 'archeology-doc · instalación' ;;
    no_fetcher) printf '%s' 'falta curl o wget' ;;
    sums_dl_fail) printf '%s' 'no se pudo descargar SHA256SUMS' ;;
    sums_missing) printf '%s' '%s no aparece en SHA256SUMS' ;;
    sha_calc_fail) printf '%s' 'no se pudo calcular el SHA-256 de %s' ;;
    sha_mismatch) printf '%s' 'SHA-256 de %s no coincide: descarga dañada o alterada' ;;
    clone_path_fail) printf '%s' 'no se pudo resolver la ruta del clon' ;;
    no_prebuilt) printf '%s' 'sin binario precompilado para %s' ;;
    musl) printf '%s' 'Linux con musl: los binarios precompilados necesitan glibc' ;;
    no_download_url) printf '%s' 'este instalador no tiene URL de descarga configurada' ;;
    downloading) printf '%s' 'descargando %s…' ;;
    download_fail) printf '%s' 'no se pudo descargar %s' ;;
    unverified) printf '%s' 'no se instala %s sin verificar' ;;
    sha_ok) printf '%s' 'SHA-256 verificado' ;;
    untar_fail) printf '%s' 'no se pudo descomprimir %s' ;;
    mkdir_fail) printf '%s' 'no se pudo crear %s' ;;
    install_fail) printf '%s' 'no se pudo instalar %s en %s' ;;
    no_rust) printf '%s' "falta Rust. Instálalo con: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" ;;
    no_git) printf '%s' 'falta git para clonar %s' ;;
    cloning_ref) printf '%s' 'clonando %s @ %s…' ;;
    ref_fail) printf '%s' 'no se pudo obtener %s de %s' ;;
    no_ref) printf '%s' 'sin --ref: se compila la rama por defecto (usa --ref <tag> para una versión fija)' ;;
    clone_fail) printf '%s' 'no se pudo clonar %s' ;;
    source_at) printf '%s' 'código fuente en %s' ;;
    compiling) printf '%s' 'compilando (la primera vez tarda unos minutos)…' ;;
    build_fail) printf '%s' 'falló la compilación (ver el error arriba)' ;;
    will_build) printf '%s' 'se compilará desde el código' ;;
    bin_missing) printf '%s' 'no se encontró el binario instalado en %s' ;;
    bin_broken) printf '%s' 'el binario instalado no se ejecuta: %s' ;;
    add_path) printf '%s' "añade %s al PATH: echo 'export PATH=\"%s:\$PATH\"' >> ~/.zshrc  (o ~/.bashrc)" ;;
    vp_ok) printf '%s' 'VitePress compartido instalado' ;;
    vp_fail) printf '%s' 'falló `adoc site setup`; reinténtalo a mano' ;;
    vp_no_npm) printf '%s' 'sin npm: instala Node 18+ y ejecuta `adoc site setup`' ;;
    no_code) printf '%s' "falta el comando \`code\` (VS Code → Shell Command: Install 'code' command in PATH)" ;;
    ext_ok) printf '%s' 'extensión de VS Code instalada' ;;
    ext_build_fail) printf '%s' 'no se pudo compilar o instalar la extensión de VS Code' ;;
    ext_install_fail) printf '%s' 'no se pudo instalar la extensión de VS Code' ;;
    ext_unverified) printf '%s' 'extensión no instalada: no pasó la verificación SHA-256' ;;
    ext_fetch_fail) printf '%s' 'no se pudo obtener la extensión' ;;
    done) printf '%s' 'Listo. Siguientes pasos:' ;;
    *) msg_en "$1" ;;
  esac
}

# msg <clave> [args…]: texto traducido sin salto de línea; los args reemplazan los %s.
msg() {
  _key="$1"; shift
  if [ "$UI_LANG" = es ]; then _fmt="$(msg_es "$_key")"; else _fmt="$(msg_en "$_key")"; fi
  # shellcheck disable=SC2059 # el formato es una cadena fija de este archivo
  printf -- "$_fmt" "$@"
}

usage() {
  if [ "$UI_LANG" = es ]; then
    cat <<'EOF'
Instalador de archeology-doc (CLI `adoc`) para macOS y Linux.

  curl -fsSL https://archeology-doc.com/download/install.sh | sh
  curl -fsSL https://archeology-doc.com/download/install.sh | sh -s -- --with-vscode
  WEBSITE/installers/install.sh      # desde un clon: compila el código local

Descarga el binario precompilado de la plataforma y verifica su SHA-256. Si no
hay binario para la plataforma (o con --from-source), compila con cargo.

Opciones:
  --with-vitepress   instala VitePress compartido (`adoc site setup`) para el sitio
  --with-vscode      instala la extensión de VS Code (requiere el comando `code`)
  --from-source      compila con cargo en lugar de descargar el binario
  --install-dir <d>  carpeta de los binarios (por defecto ~/.local/bin)
  --repo <url>       repositorio a clonar al compilar desde el código
  --ref <tag|commit> versión del código a compilar (recomendado con --from-source)
  -h, --help         muestra esta ayuda

Variables: ARCHEOLOGY_DOC_DOWNLOAD (URL de /download), ARCHEOLOGY_DOC_INSTALL_DIR,
ARCHEOLOGY_DOC_REPO (--repo), ARCHEOLOGY_DOC_REF (--ref), ADOC_LANG (en|es).
EOF
  else
    cat <<'EOF'
archeology-doc installer (`adoc` CLI) for macOS and Linux.

  curl -fsSL https://archeology-doc.com/download/install.sh | sh
  curl -fsSL https://archeology-doc.com/download/install.sh | sh -s -- --with-vscode
  WEBSITE/installers/install.sh      # from a clone: builds the local code

Downloads the prebuilt binary for your platform and verifies its SHA-256. If
there is no binary for the platform (or with --from-source), builds with cargo.

Options:
  --with-vitepress   install shared VitePress (`adoc site setup`) for the site
  --with-vscode      install the VS Code extension (requires the `code` command)
  --from-source      build with cargo instead of downloading the binary
  --install-dir <d>  folder for the binaries (default ~/.local/bin)
  --repo <url>       repository to clone when building from source
  --ref <tag|commit> source version to build (recommended with --from-source)
  -h, --help         show this help

Variables: ARCHEOLOGY_DOC_DOWNLOAD (/download URL), ARCHEOLOGY_DOC_INSTALL_DIR,
ARCHEOLOGY_DOC_REPO (--repo), ARCHEOLOGY_DOC_REF (--ref), ADOC_LANG (en|es).
EOF
  fi
}

# `WEBSITE/dist.sh --base-url` reemplaza este valor en la copia publicada en /download.
DEFAULT_DOWNLOAD="__ADOC_DOWNLOAD_BASE__"
DEFAULT_REPO="https://github.com/jsoncr/archeology-doc.git"

DOWNLOAD="${ARCHEOLOGY_DOC_DOWNLOAD:-$DEFAULT_DOWNLOAD}"
INSTALL_DIR="${ARCHEOLOGY_DOC_INSTALL_DIR:-$HOME/.local/bin}"
REPO="${ARCHEOLOGY_DOC_REPO:-$DEFAULT_REPO}"
REF="${ARCHEOLOGY_DOC_REF:-}"
WITH_VITEPRESS=0
WITH_VSCODE=0
FROM_SOURCE=0

while [ $# -gt 0 ]; do
  case "$1" in
    --with-vitepress) WITH_VITEPRESS=1 ;;
    --with-vscode) WITH_VSCODE=1 ;;
    --from-source) FROM_SOURCE=1 ;;
    --install-dir) shift; INSTALL_DIR="${1:?$(msg need_dir)}" ;;
    --repo) shift; REPO="${1:?$(msg need_repo)}" ;;
    --ref) shift; REF="${1:?$(msg need_ref)}" ;;
    -h|--help) usage; exit 0 ;;
    *) printf '%s\n' "$(msg opt_unknown "$1")" >&2; exit 2 ;;
  esac
  shift
done

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
ok() { printf '  \033[32m✓\033[0m %s\n' "$*"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$*"; }
fail() { printf '  \033[31m✗\033[0m %s\n' "$*" >&2; exit 1; }

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

bold "$(msg title)"

# Plataforma → target de Rust de los binarios publicados.
detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"
  # Bajo Rosetta, uname -m dice x86_64 en un Mac Apple Silicon: se prefiere el nativo.
  if [ "$os" = Darwin ] && [ "$(sysctl -n hw.optional.arm64 2>/dev/null)" = 1 ]; then
    arch=arm64
  fi
  case "$arch" in
    x86_64|amd64) arch=x86_64 ;;
    arm64|aarch64) arch=aarch64 ;;
    *) echo ""; return ;;
  esac
  case "$os" in
    Darwin) echo "$arch-apple-darwin" ;;
    Linux) echo "$arch-unknown-linux-gnu" ;;
    *) echo "" ;;
  esac
}

fetch() { # fetch <url> <archivo>
  # Solo HTTPS; http únicamente contra localhost (probar el sitio en local).
  proto='=https'
  case "$1" in http://localhost:*|http://127.0.0.1:*) proto='=http,https' ;; esac
  if command -v curl >/dev/null 2>&1; then
    curl --proto "$proto" --tlsv1.2 -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    case "$proto" in *http,*) wget -q -O "$2" "$1" ;; *) wget -q --https-only -O "$2" "$1" ;; esac
  else
    fail "$(msg no_fetcher)"
  fi
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

# Verifica <archivo> contra la línea <nombre> de SHA256SUMS (se descarga una
# sola vez). Devuelve 1 con un aviso si falta o no coincide; quien llama decide
# si abortar.
verify_sha256() { # verify_sha256 <archivo> <nombre>
  if [ ! -s "$TMP_DIR/SHA256SUMS" ]; then
    fetch "$DOWNLOAD/SHA256SUMS" "$TMP_DIR/SHA256SUMS" \
      || { rm -f "$TMP_DIR/SHA256SUMS"; warn "$(msg sums_dl_fail)"; return 1; }
  fi
  expected="$(awk -v f="$2" '$2 == f || $2 == "*"f {print $1}' "$TMP_DIR/SHA256SUMS")"
  [ -n "$expected" ] || { warn "$(msg sums_missing "$2")"; return 1; }
  actual="$(sha256 "$1")" || { warn "$(msg sha_calc_fail "$2")"; return 1; }
  [ "$actual" = "$expected" ] \
    || { warn "$(msg sha_mismatch "$2")"; return 1; }
  return 0
}

# ¿Linux con musl (Alpine, Void musl…)? Los binarios publicados enlazan glibc.
is_musl() {
  [ "$(uname -s)" = Linux ] || return 1
  ls /lib/ld-musl-* >/dev/null 2>&1 && return 0
  ldd --version 2>&1 | grep -qi musl
}

# ¿Hay URL de descarga configurada (no el marcador sin reemplazar)?
has_download() {
  case "$DOWNLOAD" in __ADOC_*|"") return 1 ;; esac
  return 0
}

# ¿Se ejecuta desde un clon del repositorio (WEBSITE/installers/install.sh)?
# Entonces se compila ese código. Con `curl | sh`, $0 es "sh" (o "-sh",
# "/bin/sh"…) y no aplica: solo cuenta si $0 es un archivo real con esa ruta.
SRC=""
case "$0" in
  */installers/install.sh)
    if [ -f "$0" ]; then
      CLONE_ROOT="$(dirname "$0")/../.."
      if [ -f "$CLONE_ROOT/crates/archeology-cli/Cargo.toml" ]; then
        SRC="$(cd "$CLONE_ROOT" && pwd)" || fail "$(msg clone_path_fail)"
        FROM_SOURCE=1
      fi
    fi ;;
esac

ADOC=""

# Nota: se llama como `! install_binary`, así que `set -e` no aplica dentro;
# cada paso comprueba su error de forma explícita.
install_binary() {
  target="$(detect_target)"
  [ -n "$target" ] || { warn "$(msg no_prebuilt "$(uname -s)/$(uname -m)")"; return 1; }
  if is_musl; then
    warn "$(msg musl)"
    return 1
  fi
  has_download || { warn "$(msg no_download_url)"; return 1; }
  archive="adoc-$target.tar.gz"
  echo "  $(msg downloading "$archive")"
  fetch "$DOWNLOAD/$archive" "$TMP_DIR/$archive" \
    || { warn "$(msg download_fail "$DOWNLOAD/$archive")"; return 1; }
  # Un binario que no verifica nunca se instala ni se sustituye por otro.
  verify_sha256 "$TMP_DIR/$archive" "$archive" || fail "$(msg unverified "$archive")"
  ok "$(msg sha_ok)"
  tar -xzf "$TMP_DIR/$archive" -C "$TMP_DIR" || fail "$(msg untar_fail "$archive")"
  mkdir -p "$INSTALL_DIR" || fail "$(msg mkdir_fail "$INSTALL_DIR")"
  for bin in adoc archeology-doc; do
    install -m 755 "$TMP_DIR/adoc-$target/$bin" "$INSTALL_DIR/$bin" \
      || fail "$(msg install_fail "$bin" "$INSTALL_DIR")"
  done
  ADOC="$INSTALL_DIR/adoc"
  return 0
}

install_from_source() {
  if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
  fi
  command -v cargo >/dev/null 2>&1 \
    || fail "$(msg no_rust)"
  ok "cargo $(cargo --version | awk '{print $2}')"
  if [ -z "$SRC" ]; then
    command -v git >/dev/null 2>&1 || fail "$(msg no_git "$REPO")"
    SRC="$TMP_DIR/archeology-doc"
    if [ -n "$REF" ]; then
      echo "  $(msg cloning_ref "$REPO" "$REF")"
      # `fetch` de una ref concreta funciona igual con tags y con commits.
      git init -q "$SRC" \
        && git -C "$SRC" fetch -q --depth 1 "$REPO" "$REF" \
        && git -C "$SRC" checkout -q FETCH_HEAD \
        || fail "$(msg ref_fail "$REF" "$REPO")"
    else
      warn "$(msg no_ref)"
      git clone -q --depth 1 "$REPO" "$SRC" || fail "$(msg clone_fail "$REPO")"
    fi
  fi
  ok "$(msg source_at "$SRC")"
  echo "  $(msg compiling)"
  # `--locked`: se compila con las versiones de Cargo.lock, sin resolver otras.
  cargo install --path "$SRC/crates/archeology-cli" --force --locked --quiet \
    || fail "$(msg build_fail)"
  ADOC="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin/adoc"
}

if [ "$FROM_SOURCE" = 1 ] || ! install_binary; then
  [ "$FROM_SOURCE" = 1 ] || warn "$(msg will_build)"
  install_from_source
fi
# Dentro de `ok "$(...)"` un fallo no abortaría: se comprueba aparte.
[ -x "$ADOC" ] || fail "$(msg bin_missing "$ADOC")"
ver="$("$ADOC" --version)" || fail "$(msg bin_broken "$ADOC")"
ok "$ver"

BIN_DIR="$(dirname "$ADOC")"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) warn "$(msg add_path "$BIN_DIR" "$BIN_DIR")" ;;
esac

# Opcional: VitePress compartido para `adoc preview` / `adoc site`.
if [ "$WITH_VITEPRESS" = 1 ]; then
  if command -v npm >/dev/null 2>&1; then
    "$ADOC" site setup && ok "$(msg vp_ok)" \
      || warn "$(msg vp_fail)"
  else
    warn "$(msg vp_no_npm)"
  fi
fi

# Opcional: extensión de VS Code (descargada, o compilada desde el clon).
if [ "$WITH_VSCODE" = 1 ]; then
  if ! command -v code >/dev/null 2>&1; then
    warn "$(msg no_code)"
  elif [ -n "$SRC" ] && command -v npm >/dev/null 2>&1; then
    (cd "$SRC/extension" && npm ci --no-fund --no-audit --silent && npm run --silent compile \
      && npm run --silent package -- -o archeology-doc.vsix >/dev/null) \
      && code --install-extension "$SRC/extension/archeology-doc.vsix" --force >/dev/null \
      && ok "$(msg ext_ok)" \
      || warn "$(msg ext_build_fail)"
  elif has_download && fetch "$DOWNLOAD/archeology-doc.vsix" "$TMP_DIR/archeology-doc.vsix" 2>/dev/null; then
    # La extensión se verifica igual que el binario; si no coincide, no se instala.
    if verify_sha256 "$TMP_DIR/archeology-doc.vsix" archeology-doc.vsix; then
      code --install-extension "$TMP_DIR/archeology-doc.vsix" --force >/dev/null \
        && ok "$(msg ext_ok)" \
        || warn "$(msg ext_install_fail)"
    else
      warn "$(msg ext_unverified)"
    fi
  else
    warn "$(msg ext_fetch_fail)"
  fi
fi

echo
bold "$(msg done)"
if [ "$UI_LANG" = es ]; then
  cat <<'EOF'
  adoc doctor          # verifica git, Node, VitePress, OpenSpec y agentes
  cd tu-proyecto
  adoc init            # configura el proyecto
  adoc scan && adoc plan
  adoc inspect         # los agentes documentan el proyecto
  adoc preview         # ver el resultado
EOF
else
  cat <<'EOF'
  adoc doctor          # checks git, Node, VitePress, OpenSpec and agents
  cd your-project
  adoc init            # configures the project
  adoc scan && adoc plan
  adoc inspect         # the agents document the project
  adoc preview         # see the result
EOF
fi
