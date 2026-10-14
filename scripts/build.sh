#!/usr/bin/env bash
# Punto de entrada único para publicar una versión en local: actualiza la
# versión (Cargo.toml, Cargo.lock y la extensión), compila los instaladores con
# WEBSITE/dist.sh, arma la carpeta de subida dist/site/ (igual que el paso
# `_site` de .github/workflows/release.yml) y, opcionalmente, la despliega en
# Cloudflare Pages.
#
# Ejecuta `scripts/build.sh --help` para ver opciones y ejemplos.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SITE_SRC="$ROOT/WEBSITE"
DOWNLOAD_DIR="$SITE_SRC/download"
SITE_OUT="$ROOT/dist/site"
DEFAULT_BASE_URL="https://archeology-doc.com/download"
WRANGLER="wrangler@4.141.0"
ALL_TARGETS="x86_64-apple-darwin aarch64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-pc-windows-gnu"
# Límite de Cloudflare Pages por archivo.
MAX_FILE_MIB=25

usage() {
  cat <<'EOF'
Uso: scripts/build.sh (--version X.Y.Z | --bump patch|minor|major | --no-bump) [opciones]

Actualiza la versión, compila binarios, extensión e instaladores (WEBSITE/dist.sh)
y prepara dist/site/, la carpeta lista para subir a Cloudflare Pages.

Versión (elige una):
  --version X.Y.Z        fija la versión (semver; se acepta el prefijo v)
  --bump patch|minor|major
                         incrementa la versión actual de [workspace.package]
  --no-bump              compila la versión actual sin cambiarla

Compilación (se pasan a WEBSITE/dist.sh):
  --base-url URL         URL pública de /download para los instaladores
                         (por defecto https://archeology-doc.com/download)
  --targets "T1 T2"      targets a compilar (por defecto, todos):
                           x86_64-apple-darwin aarch64-apple-darwin
                           x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
                           x86_64-pc-windows-gnu
  --native               cargo directo (o cargo-zigbuild para Linux/Windows), sin Docker
  --skip-docker          omite los targets que necesitarían Docker
  --skip-vsix            no empaqueta la extensión de VS Code
  --clean                vacía WEBSITE/download antes de compilar (evita mezclar
                         binarios de versiones anteriores)

Publicación:
  --deploy               despliega dist/site en Cloudflare Pages con wrangler
                         (proyecto $CLOUDFLARE_PAGES_PROJECT o archeology-doc);
                         requiere `npx wrangler login` o CLOUDFLARE_API_TOKEN +
                         CLOUDFLARE_ACCOUNT_ID
  --yes, -y              no pide confirmación antes de desplegar
  --tag                  muestra (no ejecuta) los comandos git para etiquetar y publicar

Otros:
  --dry-run              muestra qué cambiaría y qué se ejecutaría, sin tocar nada
  -h, --help             esta ayuda

Ejemplos:
  scripts/build.sh --dry-run --bump patch
  scripts/build.sh --bump minor --tag
  scripts/build.sh --version 0.2.0 --deploy
  scripts/build.sh --no-bump --targets "$(rustc -vV | sed -n 's/host: //p')" --native --skip-vsix
  scripts/build.sh --no-bump --clean --deploy --yes

Herramientas: cargo y rustup; node/npm para la extensión y la comprobación de
traducciones; para Linux/Windows, cargo-zigbuild + zig (pip install ziglang
cargo-zigbuild) o Docker en marcha.
EOF
}

# --- Salida -----------------------------------------------------------------
if [ -t 1 ]; then
  BOLD=$'\033[1m'; DIM=$'\033[2m'; RED=$'\033[31m'; GREEN=$'\033[32m'; YELLOW=$'\033[33m'; RESET=$'\033[0m'
else
  BOLD=""; DIM=""; RED=""; GREEN=""; YELLOW=""; RESET=""
fi
say() { printf '%s==> %s%s\n' "$BOLD" "$*" "$RESET"; }
ok() { printf '  %s✓%s %s\n' "$GREEN" "$RESET" "$*"; }
warn() { printf '  %s!%s %s\n' "$YELLOW" "$RESET" "$*" >&2; }
die() { printf '%serror:%s %s\n' "$RED" "$RESET" "$*" >&2; exit 1; }
# En --dry-run solo se describe la acción; si no, se ejecuta.
run() {
  if [ "$DRY_RUN" = 1 ]; then
    # Un solo argumento es una descripción; varios, un comando (se muestra citado).
    if [ $# -eq 1 ]; then
      printf '  %s[dry-run]%s %s\n' "$DIM" "$RESET" "$1"
    else
      printf '  %s[dry-run]%s' "$DIM" "$RESET"; printf ' %q' "$@"; printf '\n'
    fi
  else
    "$@"
  fi
}

# --- Argumentos ---------------------------------------------------------------
VERSION_MODE=""   # set | bump | none
NEW_VERSION_ARG=""
BUMP_KIND=""
BASE_URL="$DEFAULT_BASE_URL"
TARGETS=""
NATIVE=0
SKIP_DOCKER=0
SKIP_VSIX=0
CLEAN=0
DEPLOY=0
ASSUME_YES=0
PRINT_TAG=0
DRY_RUN=0

set_mode() {
  [ -z "$VERSION_MODE" ] || die "usa solo una de --version, --bump o --no-bump"
  VERSION_MODE="$1"
}
need_value() { [ $# -ge 2 ] && [ -n "$2" ] || die "falta el valor de $1"; }

while [ $# -gt 0 ]; do
  case "$1" in
    --version) need_value "$@"; set_mode set; NEW_VERSION_ARG="$2"; shift ;;
    --version=*) set_mode set; NEW_VERSION_ARG="${1#*=}" ;;
    --bump) need_value "$@"; set_mode bump; BUMP_KIND="$2"; shift ;;
    --bump=*) set_mode bump; BUMP_KIND="${1#*=}" ;;
    --no-bump) set_mode none ;;
    --base-url) need_value "$@"; BASE_URL="${2%/}"; shift ;;
    --base-url=*) BASE_URL="${1#*=}"; BASE_URL="${BASE_URL%/}" ;;
    --targets) need_value "$@"; TARGETS="$2"; shift ;;
    --targets=*) TARGETS="${1#*=}" ;;
    --native) NATIVE=1 ;;
    --skip-docker) SKIP_DOCKER=1 ;;
    --skip-vsix) SKIP_VSIX=1 ;;
    --clean) CLEAN=1 ;;
    --deploy) DEPLOY=1 ;;
    -y|--yes) ASSUME_YES=1 ;;
    --tag) PRINT_TAG=1 ;;
    --dry-run) DRY_RUN=1 ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'opción desconocida: %s\n\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

[ -n "$VERSION_MODE" ] || die "indica la versión: --version X.Y.Z, --bump patch|minor|major o --no-bump (ver --help)"
[ -n "$TARGETS" ] || TARGETS="$ALL_TARGETS"
for t in $TARGETS; do
  case " $ALL_TARGETS " in
    *" $t "*) ;;
    *) die "target desconocido: $t (válidos: $ALL_TARGETS)" ;;
  esac
done
case "$BASE_URL" in
  http://*|https://*) ;;
  *) die "--base-url debe empezar por http:// o https:// (recibido: $BASE_URL)" ;;
esac

# --- Versión ------------------------------------------------------------------
SEMVER_RE='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$'

# Primera línea `version = "..."` dentro de [workspace.package] (como el workflow).
read_cargo_version() {
  sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1
}
read_ext_version() {
  node -p "require('$ROOT/extension/package.json').version" 2>/dev/null \
    || sed -n 's/^  "version": "\(.*\)",$/\1/p' "$ROOT/extension/package.json" | head -1
}

# Compara solo X.Y.Z: imprime -1, 0 o 1.
compare_core() {
  local a b i
  IFS=. read -r -a a <<<"${1%%[-+]*}"
  IFS=. read -r -a b <<<"${2%%[-+]*}"
  for i in 0 1 2; do
    if [ "${a[$i]}" -lt "${b[$i]}" ]; then echo -1; return; fi
    if [ "${a[$i]}" -gt "${b[$i]}" ]; then echo 1; return; fi
  done
  echo 0
}

OLD_VERSION="$(read_cargo_version)"
[ -n "$OLD_VERSION" ] || die "no se encontró la versión en [workspace.package] de Cargo.toml"
[[ "$OLD_VERSION" =~ $SEMVER_RE ]] || die "la versión actual de Cargo.toml no es semver: $OLD_VERSION"
OLD_EXT_VERSION="$(read_ext_version)"

case "$VERSION_MODE" in
  none) NEW_VERSION="$OLD_VERSION" ;;
  set)
    NEW_VERSION="${NEW_VERSION_ARG#v}"
    [[ "$NEW_VERSION" =~ $SEMVER_RE ]] || die "versión no válida: '$NEW_VERSION_ARG' (se espera semver, p. ej. 0.2.0 o 1.0.0-rc.1)"
    ;;
  bump)
    IFS=. read -r major minor patch <<<"${OLD_VERSION%%[-+]*}"
    case "$BUMP_KIND" in
      major) NEW_VERSION="$((major + 1)).0.0" ;;
      minor) NEW_VERSION="$major.$((minor + 1)).0" ;;
      # Con prerelease (1.2.3-rc.1), patch publica la versión final 1.2.3.
      patch) if [ "$OLD_VERSION" != "${OLD_VERSION%%[-+]*}" ]; then NEW_VERSION="${OLD_VERSION%%[-+]*}"; else NEW_VERSION="$major.$minor.$((patch + 1))"; fi ;;
      *) die "--bump espera patch, minor o major (recibido: '$BUMP_KIND')" ;;
    esac
    ;;
esac

if [ "$VERSION_MODE" != none ]; then
  case "$(compare_core "$NEW_VERSION" "$OLD_VERSION")" in
    -1) warn "la versión nueva ($NEW_VERSION) es menor que la actual ($OLD_VERSION)" ;;
    0) [ "$NEW_VERSION" != "$OLD_VERSION" ] || warn "la versión no cambia ($OLD_VERSION)" ;;
  esac
fi

[ "$DRY_RUN" = 1 ] && say "modo --dry-run: no se modifica ningún archivo"

# --- Comprobaciones previas -----------------------------------------------------
has() { command -v "$1" >/dev/null 2>&1; }
# cargo-zigbuild con zig (binario o paquete `ziglang` de Python), como en dist.sh.
has_zigbuild() {
  has cargo-zigbuild || return 1
  has zig || python3 -c 'import ziglang' >/dev/null 2>&1
}

say "comprobando herramientas"
missing=0
if has cargo; then ok "cargo $(cargo --version | awk '{print $2}')"; else warn "falta cargo: instala Rust desde https://rustup.rs"; missing=1; fi
if has rustup; then ok "rustup"; else warn "sin rustup: no se podrán añadir targets (rustup target add)"; fi
HOST_TARGET=""
has rustc && HOST_TARGET="$(rustc -vV | sed -n 's/^host: //p')"
if has node; then ok "node $(node --version)"; else warn "sin node: no se comprueban las traducciones del sitio"; fi
if [ "$SKIP_VSIX" = 0 ] || [ "$VERSION_MODE" != none ] || [ "$DEPLOY" = 1 ]; then
  if has npm; then ok "npm $(npm --version)"; else
    warn "falta npm (Node 18+): hace falta para la versión de la extensión, el .vsix y wrangler; usa --skip-vsix si no los necesitas"
    missing=1
  fi
fi
if has sha256sum || has shasum; then ok "sha256sum/shasum"; else warn "falta sha256sum o shasum"; missing=1; fi
case " $TARGETS " in
  *windows*) if has zip; then ok "zip"; else warn "falta zip (empaqueta el binario de Windows)"; missing=1; fi ;;
esac

# Qué targets necesitan herramientas cruzadas (Linux/Windows) en esta máquina.
HOST_OS="$(uname -s)"
cross_targets=""
for t in $TARGETS; do
  case "$t" in
    *-apple-darwin)
      [ "$HOST_OS" = Darwin ] || warn "$t solo se compila en macOS: dist.sh lo omitirá" ;;
    *) [ "$t" = "$HOST_TARGET" ] && [ "$NATIVE" = 1 ] || cross_targets="$cross_targets $t" ;;
  esac
done
cross_targets="${cross_targets# }"
if [ -n "$cross_targets" ]; then
  if has_zigbuild; then
    ok "cargo-zigbuild + zig para: $cross_targets"
  elif [ "$NATIVE" = 0 ] && [ "$SKIP_DOCKER" = 0 ] && has docker && docker info >/dev/null 2>&1; then
    ok "Docker en marcha para: $cross_targets"
  elif [ "$SKIP_DOCKER" = 1 ] && [ "$NATIVE" = 0 ]; then
    warn "--skip-docker sin cargo-zigbuild: se omitirán $cross_targets"
  else
    # Aviso destacado: sin Docker (ni zig) no hay binarios de Linux/Windows.
    printf '\n\033[1;33m┌─ Aviso: Docker no está disponible ──────────────────────────────\033[0m\n' >&2
    if [ "$NATIVE" = 1 ]; then
      printf '\033[1;33m│\033[0m con --native, Linux/Windows necesitan cargo-zigbuild + zig\n' >&2
    elif has docker; then
      printf '\033[1;33m│\033[0m Docker está instalado pero no responde: abre Docker Desktop\n' >&2
      printf '\033[1;33m│\033[0m (o `colima start`) y espera a que `docker info` funcione.\n' >&2
    else
      printf '\033[1;33m│\033[0m Docker no está instalado: https://docs.docker.com/get-docker/\n' >&2
    fi
    printf '\033[1;33m│\033[0m Sin Docker ni zig no se pueden compilar: %s\n' "$cross_targets" >&2
    printf '\033[1;33m└─────────────────────────────────────────────────────────────────\033[0m\n\n' >&2
    warn "opciones:"
    warn "  · iniciar Docker y volver a ejecutar este comando"
    warn "  · instalar zig:      pip install ziglang cargo-zigbuild"
    [ -n "$HOST_TARGET" ] && warn "  · solo esta máquina: --targets \"$HOST_TARGET\" --native"
    warn "  · omitirlos:         --skip-docker"
    missing=1
  fi
fi

if [ "$DEPLOY" = 1 ]; then
  if [ -n "${CLOUDFLARE_API_TOKEN:-}" ] && [ -n "${CLOUDFLARE_ACCOUNT_ID:-}" ]; then
    ok "credenciales de Cloudflare en el entorno"
  else
    warn "sin CLOUDFLARE_API_TOKEN/CLOUDFLARE_ACCOUNT_ID: wrangler usará la sesión de \`npx $WRANGLER login\`"
  fi
fi

if [ "$missing" = 1 ]; then
  if [ "$DRY_RUN" = 1 ]; then
    warn "faltan herramientas: una ejecución real se detendría aquí"
  else
    die "faltan herramientas (ver arriba); no se ha modificado nada"
  fi
fi

# --- 1. Versión ---------------------------------------------------------------
say "versión"
printf '  Cargo.toml  [workspace.package]  %s → %s\n' "$OLD_VERSION" "$NEW_VERSION"
if [ "$VERSION_MODE" != none ]; then
  printf '  extension/package.json           %s → %s\n' "${OLD_EXT_VERSION:-?}" "$NEW_VERSION"
fi

update_versions() {
  local tmp
  tmp="$(mktemp)"
  # Solo la primera línea `version = ` dentro de [workspace.package].
  awk -v new="$NEW_VERSION" '
    /^\[/ { in_pkg = ($0 == "[workspace.package]") }
    in_pkg && !done && /^version = "/ { print "version = \"" new "\""; done = 1; next }
    { print }
  ' "$ROOT/Cargo.toml" >"$tmp"
  cat "$tmp" >"$ROOT/Cargo.toml"
  rm -f "$tmp"
  [ "$(read_cargo_version)" = "$NEW_VERSION" ] || die "no se pudo escribir la versión en Cargo.toml"
  ok "Cargo.toml"
  # Cargo.lock: solo los crates del workspace (sin actualizar dependencias).
  (cd "$ROOT" && { cargo update --workspace --offline --quiet 2>/dev/null || cargo update --workspace --quiet; })
  ok "Cargo.lock"
  (cd "$ROOT/extension" && npm version "$NEW_VERSION" --no-git-tag-version --allow-same-version >/dev/null)
  ok "extension/package.json y package-lock.json"
}

if [ "$VERSION_MODE" = none ]; then
  ok "sin cambios (--no-bump)"
elif [ "$NEW_VERSION" = "$OLD_VERSION" ] && [ "$OLD_EXT_VERSION" = "$NEW_VERSION" ]; then
  ok "sin cambios: ya es $NEW_VERSION"
elif [ "$DRY_RUN" = 1 ]; then
  run "actualizar Cargo.toml, Cargo.lock (cargo update --workspace) y extension/ (npm version $NEW_VERSION --no-git-tag-version)"
else
  update_versions
fi

# --- 2. Compilación -------------------------------------------------------------
DIST_ARGS=(--base-url "$BASE_URL" --targets "$TARGETS")
[ "$NATIVE" = 1 ] && DIST_ARGS+=(--native)
[ "$SKIP_DOCKER" = 1 ] && DIST_ARGS+=(--skip-docker)
[ "$SKIP_VSIX" = 1 ] && DIST_ARGS+=(--skip-vsix)

say "compilando con WEBSITE/dist.sh"
if [ "$CLEAN" = 1 ]; then
  run find "$DOWNLOAD_DIR" -mindepth 1 ! -name README.md -delete
fi
BUILD_MARK="$(mktemp)"
trap 'rm -f "$BUILD_MARK"' EXIT
run "$SITE_SRC/dist.sh" "${DIST_ARGS[@]}"

# Archivos de compilaciones anteriores que siguen en WEBSITE/download (otros targets).
if [ "$DRY_RUN" = 0 ]; then
  stale="$(find "$DOWNLOAD_DIR" -maxdepth 1 -type f \( -name 'adoc-*' -o -name '*.vsix' \) ! -newer "$BUILD_MARK" -exec basename {} \; | sort)"
  if [ -n "$stale" ]; then
    warn "archivos de una compilación anterior (no se recompilaron ahora; usa --clean para descartarlos):"
    printf '%s\n' "$stale" | sed 's/^/      /' >&2
  fi
fi

# --- 3. Carpeta de subida (como el paso `_site` del workflow de release) ---------
say "preparando dist/site"
prepare_site() {
  rm -rf "$SITE_OUT"
  mkdir -p "$SITE_OUT"
  cp -R "$SITE_SRC/." "$SITE_OUT/"
  rm -rf "$SITE_OUT/dist.sh" "$SITE_OUT/README.md" "$SITE_OUT/installers" "$SITE_OUT/scripts" "$SITE_OUT/download/README.md"
  # Metadatos de macOS: no deben publicarse.
  find "$SITE_OUT" -name .DS_Store -delete
  [ -f "$SITE_OUT/i18n.js" ] || die "falta i18n.js en el sitio"
  local leftover big
  leftover="$(grep -rlI '__ADOC_DOWNLOAD_BASE__' "$SITE_OUT" || true)"
  [ -z "$leftover" ] || die "quedan marcadores __ADOC_DOWNLOAD_BASE__ sin reemplazar: $leftover"
  big="$(find "$SITE_OUT" -type f -size +"$MAX_FILE_MIB"M)"
  [ -z "$big" ] || die "archivos de más de $MAX_FILE_MIB MiB (límite de Cloudflare Pages): $big"
  ok "sin marcadores ni archivos > $MAX_FILE_MIB MiB"
  [ -f "$SITE_OUT/download/SHA256SUMS" ] || die "falta download/SHA256SUMS"
  local check_out
  if has sha256sum; then
    check_out="$(cd "$SITE_OUT/download" && sha256sum -c SHA256SUMS 2>&1)" || { printf '%s\n' "$check_out" >&2; die "SHA256SUMS no coincide en dist/site/download"; }
  else
    check_out="$(cd "$SITE_OUT/download" && shasum -a 256 -c SHA256SUMS 2>&1)" || { printf '%s\n' "$check_out" >&2; die "SHA256SUMS no coincide en dist/site/download"; }
  fi
  ok "SHA256SUMS verificado ($(wc -l <"$SITE_OUT/download/SHA256SUMS" | tr -d ' ') archivos)"
  local manifest_version
  manifest_version="$(sed -n 's/^  "version": "\(.*\)",$/\1/p' "$SITE_OUT/download/latest.json" | head -1)"
  [ "$manifest_version" = "$NEW_VERSION" ] || warn "latest.json dice $manifest_version y Cargo.toml $NEW_VERSION"
}
if [ "$DRY_RUN" = 1 ]; then
  run "copiar WEBSITE/ a dist/site/ sin dist.sh, README.md, installers/, scripts/ ni download/README.md"
  run "comprobar marcadores __ADOC_DOWNLOAD_BASE__, archivos > $MAX_FILE_MIB MiB y SHA256SUMS"
else
  prepare_site
fi

# --- 4. Despliegue opcional -------------------------------------------------------
PROJECT="${CLOUDFLARE_PAGES_PROJECT:-archeology-doc}"
DEPLOY_CMD=(npx --yes "$WRANGLER" pages deploy "$SITE_OUT" --project-name "$PROJECT" --branch main --commit-dirty=true)
DEPLOYED=0
if [ "$DEPLOY" = 1 ]; then
  say "desplegando en Cloudflare Pages ($PROJECT)"
  if [ "$DRY_RUN" = 1 ]; then
    run "${DEPLOY_CMD[@]}"
  else
    if [ "$ASSUME_YES" = 0 ]; then
      [ -t 0 ] || die "sin terminal para confirmar el despliegue: usa --yes"
      printf '  ¿Publicar v%s en el proyecto %s (rama main)? [s/N] ' "$NEW_VERSION" "$PROJECT"
      read -r answer
      case "$answer" in
        s|S|si|sí|SI|y|Y|yes) ;;
        *) die "despliegue cancelado; la carpeta sigue en dist/site" ;;
      esac
    fi
    (cd "$ROOT" && "${DEPLOY_CMD[@]}")
    DEPLOYED=1
  fi
fi

# --- Resumen ---------------------------------------------------------------------
echo
say "resumen"
printf '  versión:  %s%s\n' "$NEW_VERSION" "$([ "$NEW_VERSION" != "$OLD_VERSION" ] && printf ' (antes %s)' "$OLD_VERSION")"
printf '  base-url: %s\n' "$BASE_URL"
if [ "$DRY_RUN" = 0 ] && [ -d "$SITE_OUT/download" ]; then
  echo "  artefactos (dist/site/download):"
  (cd "$SITE_OUT/download" && ls -lh | awk 'NR > 1 {printf "    %7s  %s\n", $5, $9}')
  printf '  carpeta de subida: %s\n' "$SITE_OUT"
fi
echo
echo "Siguientes pasos:"
if [ "$DRY_RUN" = 1 ]; then
  echo "  · ejecuta el mismo comando sin --dry-run"
fi
if [ "$DEPLOYED" = 0 ]; then
  echo "  · revisar en local:  python3 -m http.server -d dist/site 8080"
  echo "  · desplegar:         npx --yes $WRANGLER pages deploy dist/site --project-name $PROJECT --branch main"
  echo "                       (o vuelve a ejecutar este script con --deploy)"
fi
if [ "$PRINT_TAG" = 1 ] || [ "$NEW_VERSION" != "$OLD_VERSION" ]; then
  echo "  · etiquetar y publicar (el tag dispara .github/workflows/release.yml):"
  echo "      git commit -am \"release v$NEW_VERSION\" && git tag v$NEW_VERSION && git push origin main v$NEW_VERSION"
fi
