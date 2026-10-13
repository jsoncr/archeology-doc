# Instalador de archeology-doc (CLI `adoc`) para Windows.
#
#   irm https://<sitio>/download/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://<sitio>/download/install.ps1))) -WithVSCode
#
# Descarga el binario precompilado desde la carpeta /download del sitio,
# verifica su SHA-256, lo instala en %LOCALAPPDATA%\Programs\archeology-doc\bin
# y añade esa carpeta al PATH del usuario (sin permisos de administrador).
#
# Variables: ARCHEOLOGY_DOC_DOWNLOAD (URL de /download), ARCHEOLOGY_DOC_INSTALL_DIR.
param(
    [switch]$WithVSCode,
    [switch]$WithVitepress,
    [string]$InstallDir = $(if ($env:ARCHEOLOGY_DOC_INSTALL_DIR) { $env:ARCHEOLOGY_DOC_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\archeology-doc\bin' })
)

$ErrorActionPreference = 'Stop'
# PowerShell 5.1 no activa TLS 1.2 por defecto.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

# `WEBSITE/dist.sh --base-url` reemplaza este valor en la copia publicada en /download.
$DefaultDownload = '__ADOC_DOWNLOAD_BASE__'
$Download = if ($env:ARCHEOLOGY_DOC_DOWNLOAD) { $env:ARCHEOLOGY_DOC_DOWNLOAD } else { $DefaultDownload }

function Ok($m) { Write-Host "  [ok] $m" -ForegroundColor Green }
function Warn($m) { Write-Host "  [!] $m" -ForegroundColor Yellow }
function Fail($m) { Write-Host "  [x] $m" -ForegroundColor Red; throw $m }

Write-Host 'archeology-doc · instalación' -ForegroundColor White

if ($Download -like '__ADOC_*') {
    Fail 'este instalador no tiene URL de descarga configurada; define ARCHEOLOGY_DOC_DOWNLOAD'
}

$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -ne 'AMD64' -and $arch -ne 'ARM64') {
    Fail "arquitectura no soportada: $arch"
}
# ARM64 ejecuta el binario x64 con emulación.
$target = 'x86_64-pc-windows-gnu'
$archive = "adoc-$target.zip"

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("adoc-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Host "  descargando ${archive}…"
    $zip = Join-Path $tmp $archive
    Invoke-WebRequest -UseBasicParsing -Uri "$Download/$archive" -OutFile $zip
    $sums = (Invoke-WebRequest -UseBasicParsing -Uri "$Download/SHA256SUMS").Content
    if ($sums -is [byte[]]) { $sums = [Text.Encoding]::UTF8.GetString($sums) }
    $expected = ($sums -split "`n" | Where-Object { $_ -match "\s\*?$([regex]::Escape($archive))\s*$" } | ForEach-Object { ($_ -split '\s+')[0] }) | Select-Object -First 1
    if (-not $expected) { Fail "$archive no aparece en SHA256SUMS" }
    $actual = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLower()
    if ($actual -ne $expected.ToLower()) { Fail "SHA-256 de $archive no coincide: descarga dañada o alterada" }
    Ok 'SHA-256 verificado'

    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    foreach ($bin in 'adoc.exe', 'archeology-doc.exe') {
        Copy-Item -Force (Join-Path $tmp "adoc-$target\$bin") (Join-Path $InstallDir $bin)
    }
    $adoc = Join-Path $InstallDir 'adoc.exe'
    Ok (& $adoc --version)

    # PATH del usuario (persistente) y de esta sesión.
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not (($userPath -split ';') -contains $InstallDir)) {
        [Environment]::SetEnvironmentVariable('Path', ($(if ($userPath) { "$userPath;" } else { '' }) + $InstallDir), 'User')
        Ok "añadido $InstallDir al PATH del usuario (abre una terminal nueva)"
    }
    if (-not (($env:Path -split ';') -contains $InstallDir)) { $env:Path += ";$InstallDir" }

    if ($WithVitepress) {
        if (Get-Command npm -ErrorAction SilentlyContinue) {
            & $adoc site setup
            Ok 'VitePress compartido instalado'
        } else {
            Warn 'sin npm: instala Node 18+ y ejecuta `adoc site setup`'
        }
    }

    if ($WithVSCode) {
        if (Get-Command code -ErrorAction SilentlyContinue) {
            $vsix = Join-Path $tmp 'archeology-doc.vsix'
            Invoke-WebRequest -UseBasicParsing -Uri "$Download/archeology-doc.vsix" -OutFile $vsix
            & code --install-extension $vsix --force | Out-Null
            Ok 'extensión de VS Code instalada'
        } else {
            Warn 'falta el comando `code` en el PATH'
        }
    }
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

Write-Host ''
Write-Host 'Listo. Siguientes pasos:' -ForegroundColor White
Write-Host '  adoc doctor          # verifica git, Node, VitePress, OpenSpec y agentes'
Write-Host '  cd tu-proyecto'
Write-Host '  adoc init            # configura el proyecto'
Write-Host '  adoc scan; adoc plan'
Write-Host '  adoc inspect         # los agentes documentan el proyecto'
Write-Host '  adoc preview         # ver el resultado'
