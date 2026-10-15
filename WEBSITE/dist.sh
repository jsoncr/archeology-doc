#!/bin/sh
# Compila los binarios de archeology-doc para cada plataforma y arma la
# carpeta de descargas del sitio: WEBSITE/download/.
#
#   WEBSITE/dist.sh                                  # todo lo posible en esta máquina
#   WEBSITE/dist.sh --base-url https://ejemplo.dev/download
#   WEBSITE/dist.sh --targets "x86_64-apple-darwin aarch64-apple-darwin"
#   WEBSITE/dist.sh --skip-docker --skip-vsix
#   WEBSITE/dist.sh --native --targets "..."   # CI: cargo directo, linkers ya instalados
#   WEBSITE/dist.sh --no-build --base-url ...  # CI: solo vsix, instaladores y sumas
#
# - macOS (x86_64 y aarch64): cargo nativo (necesita un Mac).
# - Linux (x86_64 y aarch64) y Windows (x86_64): con cargo-zigbuild si está
#   disponible (zig como linker cruzado; Linux contra glibc 2.28, compatible
#   con distros desde 2018). Instalación: `pip install ziglang cargo-zigbuild`.
#   Si no, dentro de un contenedor Docker con los linkers cruzados.
# - Extensión de VS Code: `npm run package` en extension/.
#
# Resultado en WEBSITE/download/:
#   adoc-<target>.tar.gz | .zip   binarios `adoc` y `archeology-doc` + licencias
#   archeology-doc.vsix           extensión de VS Code
#   install.sh, install.ps1       instaladores con --base-url ya aplicado
#   SHA256SUMS, latest.json       sumas y manifiesto que lee la página
#
# --base-url es la URL pública de la carpeta /download; los instaladores la
# usan para descargar. Sin ella, los instaladores piden ARCHEOLOGY_DOC_DOWNLOAD.
set -eu

SITE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SITE/.." && pwd)"
OUT="$SITE/download"
BASE_URL=""
SKIP_DOCKER=0
SKIP_VSIX=0
NATIVE=0
NO_BUILD=0
RUST_IMAGE="rust:1.90-bookworm"
ALL_TARGETS="x86_64-apple-darwin aarch64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-pc-windows-gnu"
TARGETS=""

while [ $# -gt 0 ]; do
  case "$1" in
    --base-url) shift; BASE_URL="${1%/}" ;;
    --targets) shift; TARGETS="$1" ;;
    --out) shift; OUT="$1" ;;
    --skip-docker) SKIP_DOCKER=1 ;;
    --skip-vsix) SKIP_VSIX=1 ;;
    --native) NATIVE=1 ;;
    --no-build) NO_BUILD=1 ;;
    -h|--help) sed -n '2,27p' "$0"; exit 0 ;;
    *) echo "opción desconocida: $1" >&2; exit 2 ;;
  esac
  shift
done

say() { printf '\033[1m==> %s\033[0m\n' "$*"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$*"; }

# ¿cargo-zigbuild con zig (binario o paquete `ziglang` de Python)?
has_zigbuild() {
  command -v cargo-zigbuild >/dev/null 2>&1 || return 1
  command -v zig >/dev/null 2>&1 || python3 -c 'import ziglang' >/dev/null 2>&1
}

# 0. Traducciones del sitio (en/es): claves completas en ambos idiomas.
if command -v node >/dev/null 2>&1; then
  say "comprobando traducciones del sitio"
  node "$SITE/scripts/check-i18n.mjs"
else
  warn "sin node: no se comprueban las traducciones (WEBSITE/scripts/check-i18n.mjs)"
fi

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
HOST_OS="$(uname -s)"

if [ -z "$TARGETS" ]; then
  TARGETS="$ALL_TARGETS"
fi
mac_targets=""
docker_targets=""
for t in $TARGETS; do
  [ "$NO_BUILD" = 0 ] || break
  if [ "$NATIVE" = 1 ]; then
    case "$t" in
      *-apple-darwin) mac_targets="$mac_targets $t" ;;
      # Con zig disponible, Linux/Windows siguen por cargo-zigbuild (glibc 2.28).
      *) if has_zigbuild; then docker_targets="$docker_targets $t"; else mac_targets="$mac_targets $t"; fi ;;
    esac
    continue
  fi
  case "$t" in
    *-apple-darwin)
      if [ "$HOST_OS" = Darwin ]; then mac_targets="$mac_targets $t"; else warn "$t solo se compila en macOS"; fi ;;
    *) docker_targets="$docker_targets $t" ;;
  esac
done

# Ruta absoluta para --out: package() y el paso de sumas hacen `cd` antes de
# escribir en $OUT, así que una ruta relativa apuntaría a otro lado.
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT INT TERM

# Empaqueta los binarios de un target en $OUT.
package() { # package <target> <dir con los binarios>
  target="$1"; bindir="$2"; ext=""
  case "$target" in *windows*) ext=".exe" ;; esac
  pkg="$STAGE/adoc-$target"
  rm -rf "$pkg"; mkdir -p "$pkg"
  for bin in adoc archeology-doc; do
    cp "$bindir/$bin$ext" "$pkg/"
  done
  cp "$ROOT/README.md" "$ROOT/LICENSE-MIT" "$pkg/"
  case "$target" in
    *windows*)
      rm -f "$OUT/adoc-$target.zip"
      (cd "$STAGE" && zip -qr "$OUT/adoc-$target.zip" "adoc-$target") ;;
    *)
      # Sin metadatos de macOS (._*) dentro del tar.
      (cd "$STAGE" && COPYFILE_DISABLE=1 tar -czf "$OUT/adoc-$target.tar.gz" "adoc-$target") ;;
  esac
  echo "  ✓ $target"
}

# 1. macOS (o todos con --native): cargo directo.
for t in $mac_targets; do
  say "compilando $t"
  rustup target add "$t" >/dev/null 2>&1 || true
  cargo build --release --locked --manifest-path "$ROOT/Cargo.toml" -p archeology-cli --target "$t"
  package "$t" "$ROOT/target/$t/release"
done

# 2. Linux y Windows: contenedor con los linkers cruzados.
if [ -n "$docker_targets" ] && has_zigbuild; then
  for t in $docker_targets; do
    say "compilando $t (zig)"
    rustup target add "$t" >/dev/null 2>&1 || true
    # Sufijo de glibc solo en Linux: el binario corre en glibc >= 2.28.
    zt="$t"
    case "$t" in *-linux-gnu) zt="$t.2.28" ;; esac
    cargo zigbuild --release --locked --manifest-path "$ROOT/Cargo.toml" -p archeology-cli --target "$zt"
    package "$t" "$ROOT/target/$t/release"
  done
  docker_targets=""
fi

if [ -n "$docker_targets" ]; then
  if [ "$SKIP_DOCKER" = 1 ]; then
    warn "omitidos (--skip-docker):$docker_targets"
  elif ! docker info >/dev/null 2>&1; then
    warn "Docker no está disponible; omitidos:$docker_targets"
  else
    # Otra compilación en marcha comparte target/docker: se bloquearían entre sí.
    running="$(docker ps -q --filter label=archeology-doc.build=1)"
    if [ -n "$running" ]; then
      warn "ya hay una compilación de Docker en marcha ($running)."
      warn "detenla con: docker stop $running"
      exit 1
    fi
    # Imagen con los linkers cruzados ya instalados (scripts/Dockerfile.cross):
    # apt solo se ejecuta al crearla, no en cada compilación. La etiqueta lleva
    # un hash del Dockerfile y de RUST_IMAGE: si cambian, se reconstruye sola.
    DOCKERFILE="$ROOT/scripts/Dockerfile.cross"
    [ -f "$DOCKERFILE" ] || { echo "falta $DOCKERFILE" >&2; exit 1; }
    if command -v sha256sum >/dev/null 2>&1; then sum="sha256sum"; else sum="shasum -a 256"; fi
    cross_hash="$( { cat "$DOCKERFILE"; echo "$RUST_IMAGE"; } | $sum | cut -c1-12)"
    CROSS_IMAGE="adoc-cross:${RUST_IMAGE#rust:}-$cross_hash"
    if ! docker image inspect "$CROSS_IMAGE" >/dev/null 2>&1; then
      say "preparando la imagen $CROSS_IMAGE (solo la primera vez; descarga ~150 MB)"
      docker build --progress=plain --build-arg RUST_IMAGE="$RUST_IMAGE" \
        -f "$DOCKERFILE" -t "$CROSS_IMAGE" "$ROOT/scripts"
    fi
    say "compilando en Docker:$docker_targets"
    # Registro de cargo en un volumen: las siguientes ejecuciones no descargan todo.
    # Linkers y CC por target: definidos en el Dockerfile.
    docker run --rm --label archeology-doc.build=1 \
      -v "$ROOT":/work -w /work \
      -v adoc-cargo-registry:/usr/local/cargo/registry \
      -e CARGO_TARGET_DIR=/work/target/docker \
      -e TARGETS="$docker_targets" \
      "$CROSS_IMAGE" sh -euc '
        for t in $TARGETS; do
          echo "--> $t"
          cargo build --release --locked -p archeology-cli --target "$t"
        done
        # Los archivos quedan con el usuario del host, no root.
        chown -R "$(stat -c %u:%g /work)" /work/target/docker
      '
    for t in $docker_targets; do
      package "$t" "$ROOT/target/docker/$t/release"
    done
  fi
fi

# 3. Extensión de VS Code.
if [ "$SKIP_VSIX" = 0 ]; then
  if command -v npm >/dev/null 2>&1; then
    say "empaquetando la extensión de VS Code"
    (cd "$ROOT/extension" && npm ci --no-fund --no-audit --silent && npm run --silent compile \
      && npm run --silent package -- -o "$OUT/archeology-doc.vsix" >/dev/null)
    echo "  ✓ archeology-doc.vsix"
  else
    warn "sin npm: no se empaqueta la extensión"
  fi
fi

# 4. Instaladores con la URL de descarga aplicada.
say "instaladores"
for f in install.sh install.ps1; do
  if [ -n "$BASE_URL" ]; then
    sed "s|__ADOC_DOWNLOAD_BASE__|$BASE_URL|" "$SITE/installers/$f" > "$OUT/$f"
  else
    cp "$SITE/installers/$f" "$OUT/$f"
  fi
done
chmod 755 "$OUT/install.sh"
[ -n "$BASE_URL" ] || warn "sin --base-url: los instaladores necesitarán ARCHEOLOGY_DOC_DOWNLOAD"

# 5. Sumas SHA-256 y manifiesto para la página.
say "SHA256SUMS y latest.json"
cd "$OUT"
files="$(ls adoc-*.tar.gz adoc-*.zip archeology-doc.vsix install.sh install.ps1 2>/dev/null || true)"
: > SHA256SUMS
for f in $files; do
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$f"; else shasum -a 256 "$f"; fi >> SHA256SUMS
done
{
  printf '{\n  "version": "%s",\n  "generated_at": "%s",\n  "files": [' "$VERSION" "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  sep=""
  for f in $files; do
    sum="$(awk -v f="$f" '$2 == f {print $1}' SHA256SUMS)"
    size="$(wc -c < "$f" | tr -d ' ')"
    printf '%s\n    {"file": "%s", "sha256": "%s", "size": %s}' "$sep" "$f" "$sum" "$size"
    sep=","
  done
  printf '\n  ]\n}\n'
} > latest.json

echo
ls -lh "$OUT" | awk 'NR > 1 {print "  " $5 "\t" $9}'
