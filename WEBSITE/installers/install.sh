#!/bin/sh
# Instalador de arqueology-doc (CLI `adoc`) para macOS y Linux.
#
#   curl -fsSL https://<sitio>/download/install.sh | sh
#   curl -fsSL https://<sitio>/download/install.sh | sh -s -- --with-vscode
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
# Variables: ARQUEOLOGY_DOC_DOWNLOAD (URL de /download), ARQUEOLOGY_DOC_INSTALL_DIR,
# ARQUEOLOGY_DOC_REPO (--repo), ARQUEOLOGY_DOC_REF (--ref).
set -eu

# `WEBSITE/dist.sh --base-url` reemplaza este valor en la copia publicada en /download.
DEFAULT_DOWNLOAD="__ADOC_DOWNLOAD_BASE__"
DEFAULT_REPO="https://github.com/jsoncr/arqueology-doc.git"

DOWNLOAD="${ARQUEOLOGY_DOC_DOWNLOAD:-$DEFAULT_DOWNLOAD}"
INSTALL_DIR="${ARQUEOLOGY_DOC_INSTALL_DIR:-$HOME/.local/bin}"
REPO="${ARQUEOLOGY_DOC_REPO:-$DEFAULT_REPO}"
REF="${ARQUEOLOGY_DOC_REF:-}"
WITH_VITEPRESS=0
WITH_VSCODE=0
FROM_SOURCE=0

while [ $# -gt 0 ]; do
  case "$1" in
    --with-vitepress) WITH_VITEPRESS=1 ;;
    --with-vscode) WITH_VSCODE=1 ;;
    --from-source) FROM_SOURCE=1 ;;
    --install-dir) shift; INSTALL_DIR="${1:?--install-dir necesita una carpeta}" ;;
    --repo) shift; REPO="${1:?--repo necesita una url}" ;;
    --ref) shift; REF="${1:?--ref necesita un tag o commit}" ;;
    -h|--help) sed -n '2,23p' "$0" 2>/dev/null || true; exit 0 ;;
    *) echo "opción desconocida: $1" >&2; exit 2 ;;
  esac
  shift
done

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
ok() { printf '  \033[32m✓\033[0m %s\n' "$*"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$*"; }
fail() { printf '  \033[31m✗\033[0m %s\n' "$*" >&2; exit 1; }

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

bold "arqueology-doc · instalación"

# Plataforma → target de Rust de los binarios publicados.
detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"
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
    fail "falta curl o wget"
  fi
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

# ¿Se ejecuta desde un clon del repositorio (WEBSITE/installers/install.sh)?
# Entonces se compila ese código. Con `curl | sh`, $0 es "sh" y no aplica.
SRC=""
CLONE_ROOT="$(dirname "$0")/../.."
if [ -f "$CLONE_ROOT/crates/arqueology-cli/Cargo.toml" ]; then
  SRC="$(cd "$CLONE_ROOT" && pwd)"
  FROM_SOURCE=1
fi

ADOC=""

install_binary() {
  target="$(detect_target)"
  [ -n "$target" ] || { warn "sin binario precompilado para $(uname -s)/$(uname -m)"; return 1; }
  case "$DOWNLOAD" in
    __ADOC_*|"") warn "este instalador no tiene URL de descarga configurada"; return 1 ;;
  esac
  archive="adoc-$target.tar.gz"
  echo "  descargando ${archive}…"
  fetch "$DOWNLOAD/$archive" "$TMP_DIR/$archive" \
    || { warn "no se pudo descargar $DOWNLOAD/$archive"; return 1; }
  fetch "$DOWNLOAD/SHA256SUMS" "$TMP_DIR/SHA256SUMS" || fail "no se pudo descargar SHA256SUMS"
  expected="$(awk -v f="$archive" '$2 == f || $2 == "*"f {print $1}' "$TMP_DIR/SHA256SUMS")"
  [ -n "$expected" ] || fail "$archive no aparece en SHA256SUMS"
  [ "$(sha256 "$TMP_DIR/$archive")" = "$expected" ] || fail "SHA-256 de $archive no coincide: descarga dañada o alterada"
  ok "SHA-256 verificado"
  tar -xzf "$TMP_DIR/$archive" -C "$TMP_DIR"
  mkdir -p "$INSTALL_DIR"
  for bin in adoc arqueology-doc; do
    install -m 755 "$TMP_DIR/adoc-$target/$bin" "$INSTALL_DIR/$bin"
  done
  ADOC="$INSTALL_DIR/adoc"
  return 0
}

install_from_source() {
  if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
  fi
  command -v cargo >/dev/null 2>&1 \
    || fail "falta Rust. Instálalo con: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  ok "cargo $(cargo --version | awk '{print $2}')"
  if [ -z "$SRC" ]; then
    command -v git >/dev/null 2>&1 || fail "falta git para clonar $REPO"
    SRC="$TMP_DIR/arqueology-doc"
    if [ -n "$REF" ]; then
      echo "  clonando $REPO @ ${REF}…"
      # `fetch` de una ref concreta funciona igual con tags y con commits.
      git init -q "$SRC" \
        && git -C "$SRC" fetch -q --depth 1 "$REPO" "$REF" \
        && git -C "$SRC" checkout -q FETCH_HEAD \
        || fail "no se pudo obtener $REF de $REPO"
    else
      warn "sin --ref: se compila la rama por defecto (usa --ref <tag> para una versión fija)"
      git clone -q --depth 1 "$REPO" "$SRC" || fail "no se pudo clonar $REPO"
    fi
  fi
  ok "código fuente en $SRC"
  echo "  compilando (la primera vez tarda unos minutos)…"
  # `--locked`: se compila con las versiones de Cargo.lock, sin resolver otras.
  cargo install --path "$SRC/crates/arqueology-cli" --force --locked --quiet \
    || fail "falló la compilación (ver el error arriba)"
  ADOC="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin/adoc"
}

if [ "$FROM_SOURCE" = 1 ] || ! install_binary; then
  [ "$FROM_SOURCE" = 1 ] || warn "se compilará desde el código"
  install_from_source
fi
ok "$("$ADOC" --version)"

BIN_DIR="$(dirname "$ADOC")"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) warn "añade $BIN_DIR al PATH: echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.zshrc  (o ~/.bashrc)" ;;
esac

# Opcional: VitePress compartido para `adoc preview` / `adoc site`.
if [ "$WITH_VITEPRESS" = 1 ]; then
  if command -v npm >/dev/null 2>&1; then
    "$ADOC" site setup && ok "VitePress compartido instalado"
  else
    warn "sin npm: instala Node 18+ y ejecuta \`adoc site setup\`"
  fi
fi

# Opcional: extensión de VS Code (descargada, o compilada desde el clon).
if [ "$WITH_VSCODE" = 1 ]; then
  if ! command -v code >/dev/null 2>&1; then
    warn "falta el comando \`code\` (VS Code → Shell Command: Install 'code' command in PATH)"
  elif [ -n "$SRC" ] && command -v npm >/dev/null 2>&1; then
    (cd "$SRC/extension" && npm ci --no-fund --no-audit --silent && npm run --silent compile \
      && npm run --silent package -- -o arqueology-doc.vsix >/dev/null) \
      && code --install-extension "$SRC/extension/arqueology-doc.vsix" --force >/dev/null \
      && ok "extensión de VS Code instalada"
  elif fetch "$DOWNLOAD/arqueology-doc.vsix" "$TMP_DIR/arqueology-doc.vsix" 2>/dev/null; then
    code --install-extension "$TMP_DIR/arqueology-doc.vsix" --force >/dev/null \
      && ok "extensión de VS Code instalada"
  else
    warn "no se pudo obtener la extensión"
  fi
fi

echo
bold "Listo. Siguientes pasos:"
cat <<'EOF'
  adoc doctor          # verifica git, Node, VitePress, OpenSpec y agentes
  cd tu-proyecto
  adoc init            # configura el proyecto
  adoc scan && adoc plan
  adoc inspect         # los agentes documentan el proyecto
  adoc preview         # ver el resultado
EOF
