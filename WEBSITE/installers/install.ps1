# Instalador de archeology-doc (CLI `adoc`) para Windows.
#
#   irm https://archeology-doc.com/download/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://archeology-doc.com/download/install.ps1))) -WithVSCode
#
# Descarga el binario precompilado desde la carpeta /download del sitio,
# verifica su SHA-256, lo instala en %LOCALAPPDATA%\Programs\archeology-doc\bin
# y añade esa carpeta al PATH del usuario (sin permisos de administrador).
#
# Variables: ARCHEOLOGY_DOC_DOWNLOAD (URL de /download), ARCHEOLOGY_DOC_INSTALL_DIR,
# ADOC_LANG (en|es). Mensajes en español si la cultura de la interfaz es española
# (o ADOC_LANG=es); en inglés en otro caso.
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

# --- Idioma de los mensajes -------------------------------------------------
# ADOC_LANG tiene prioridad; si no, el idioma de la interfaz (o la cultura) de Windows.
$UiLang = 'en'
if ($env:ADOC_LANG -match '^(en|es)([-_.]|$)') {
    $UiLang = $Matches[1].ToLower()
} else {
    try {
        $uiCulture = [Globalization.CultureInfo]::CurrentUICulture.TwoLetterISOLanguageName
        if ($uiCulture -eq 'es' -or (Get-Culture).TwoLetterISOLanguageName -eq 'es') { $UiLang = 'es' }
    } catch {
        # sin información de cultura: se queda en inglés
    }
}

# Textos por idioma; {0}, {1}… se rellenan con Msg.
$Messages = @{
    en = @{
        title          = 'archeology-doc · installation'
        sumsMissing    = '{0} is not listed in SHA256SUMS'
        shaMismatch    = 'SHA-256 of {0} does not match: corrupted or tampered download'
        noDownloadUrl  = 'this installer has no download URL configured; set ARCHEOLOGY_DOC_DOWNLOAD'
        unsupportedArch = 'unsupported architecture: {0}'
        downloading    = 'downloading {0}…'
        unverified     = 'refusing to install {0} without verification'
        shaOk          = 'SHA-256 verified'
        binBroken      = 'the installed binary does not run: {0}'
        pathAdded      = 'added {0} to the user PATH (open a new terminal)'
        vpOk           = 'shared VitePress installed'
        vpFail         = '`adoc site setup` failed; run it again manually'
        vpNoNpm        = 'no npm: install Node 18+ and run `adoc site setup`'
        extOk          = 'VS Code extension installed'
        extInstallFail = 'could not install the VS Code extension'
        extUnverified  = 'extension not installed: SHA-256 verification failed'
        noCode         = 'the `code` command is not in PATH'
        done           = 'Done. Next steps:'
        next1          = '  adoc doctor          # checks git, Node, VitePress, OpenSpec and agents'
        next2          = '  cd your-project'
        next3          = '  adoc init            # configures the project'
        next4          = '  adoc scan; adoc plan'
        next5          = '  adoc inspect         # the agents document the project'
        next6          = '  adoc preview         # see the result'
    }
    es = @{
        title          = 'archeology-doc · instalación'
        sumsMissing    = '{0} no aparece en SHA256SUMS'
        shaMismatch    = 'SHA-256 de {0} no coincide: descarga dañada o alterada'
        noDownloadUrl  = 'este instalador no tiene URL de descarga configurada; define ARCHEOLOGY_DOC_DOWNLOAD'
        unsupportedArch = 'arquitectura no soportada: {0}'
        downloading    = 'descargando {0}…'
        unverified     = 'no se instala {0} sin verificar'
        shaOk          = 'SHA-256 verificado'
        binBroken      = 'el binario instalado no se ejecuta: {0}'
        pathAdded      = 'añadido {0} al PATH del usuario (abre una terminal nueva)'
        vpOk           = 'VitePress compartido instalado'
        vpFail         = 'falló `adoc site setup`; reinténtalo a mano'
        vpNoNpm        = 'sin npm: instala Node 18+ y ejecuta `adoc site setup`'
        extOk          = 'extensión de VS Code instalada'
        extInstallFail = 'no se pudo instalar la extensión de VS Code'
        extUnverified  = 'extensión no instalada: no pasó la verificación SHA-256'
        noCode         = 'falta el comando `code` en el PATH'
        done           = 'Listo. Siguientes pasos:'
        next1          = '  adoc doctor          # verifica git, Node, VitePress, OpenSpec y agentes'
        next2          = '  cd tu-proyecto'
        next3          = '  adoc init            # configura el proyecto'
        next4          = '  adoc scan; adoc plan'
        next5          = '  adoc inspect         # los agentes documentan el proyecto'
        next6          = '  adoc preview         # ver el resultado'
    }
}

# Msg <clave> [valores…]: texto en el idioma elegido (con respaldo en inglés).
function Msg([string]$Key) {
    $text = $Messages[$UiLang][$Key]
    if (-not $text) { $text = $Messages['en'][$Key] }
    if (-not $text) { $text = $Key }
    if ($args.Count -gt 0) { return [string]::Format($text, [object[]]$args) }
    return $text
}

function Ok($m) { Write-Host "  [ok] $m" -ForegroundColor Green }
function Warn($m) { Write-Host "  [!] $m" -ForegroundColor Yellow }
function Fail($m) { Write-Host "  [x] $m" -ForegroundColor Red; throw $m }

# Compara el SHA-256 de un archivo con su línea en SHA256SUMS; $true si coincide.
function Test-Sha256($sums, $path, $name) {
    $expected = ($sums -split "`n" | Where-Object { $_ -match "\s\*?$([regex]::Escape($name))\s*$" } | ForEach-Object { ($_ -split '\s+')[0] }) | Select-Object -First 1
    if (-not $expected) { Warn (Msg sumsMissing $name); return $false }
    $actual = (Get-FileHash -Algorithm SHA256 -Path $path).Hash.ToLower()
    if ($actual -ne $expected.ToLower()) { Warn (Msg shaMismatch $name); return $false }
    return $true
}

Write-Host (Msg title) -ForegroundColor White

if ($Download -like '__ADOC_*') {
    Fail (Msg noDownloadUrl)
}

$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -ne 'AMD64' -and $arch -ne 'ARM64') {
    Fail (Msg unsupportedArch $arch)
}
# ARM64 ejecuta el binario x64 con emulación.
$target = 'x86_64-pc-windows-gnu'
$archive = "adoc-$target.zip"

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("adoc-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
    Write-Host ("  " + (Msg downloading $archive))
    $zip = Join-Path $tmp $archive
    Invoke-WebRequest -UseBasicParsing -Uri "$Download/$archive" -OutFile $zip
    $sums = (Invoke-WebRequest -UseBasicParsing -Uri "$Download/SHA256SUMS").Content
    if ($sums -is [byte[]]) { $sums = [Text.Encoding]::UTF8.GetString($sums) }
    if (-not (Test-Sha256 $sums $zip $archive)) { Fail (Msg unverified $archive) }
    Ok (Msg shaOk)

    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    foreach ($bin in 'adoc.exe', 'archeology-doc.exe') {
        Copy-Item -Force (Join-Path $tmp "adoc-$target\$bin") (Join-Path $InstallDir $bin)
    }
    $adoc = Join-Path $InstallDir 'adoc.exe'
    # Los comandos nativos no lanzan excepción con $ErrorActionPreference: se
    # revisa $LASTEXITCODE.
    $ver = & $adoc --version
    if ($LASTEXITCODE -ne 0 -or -not $ver) { Fail (Msg binBroken $adoc) }
    Ok $ver

    # PATH del usuario (persistente) y de esta sesión.
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not (($userPath -split ';') -contains $InstallDir)) {
        [Environment]::SetEnvironmentVariable('Path', ($(if ($userPath) { "$userPath;" } else { '' }) + $InstallDir), 'User')
        Ok (Msg pathAdded $InstallDir)
    }
    if (-not (($env:Path -split ';') -contains $InstallDir)) { $env:Path += ";$InstallDir" }

    if ($WithVitepress) {
        if (Get-Command npm -ErrorAction SilentlyContinue) {
            & $adoc site setup
            if ($LASTEXITCODE -eq 0) { Ok (Msg vpOk) }
            else { Warn (Msg vpFail) }
        } else {
            Warn (Msg vpNoNpm)
        }
    }

    if ($WithVSCode) {
        if (Get-Command code -ErrorAction SilentlyContinue) {
            $vsix = Join-Path $tmp 'archeology-doc.vsix'
            Invoke-WebRequest -UseBasicParsing -Uri "$Download/archeology-doc.vsix" -OutFile $vsix
            # La extensión se verifica igual que el binario; si no coincide, no se instala.
            if (Test-Sha256 $sums $vsix 'archeology-doc.vsix') {
                & code --install-extension $vsix --force | Out-Null
                if ($LASTEXITCODE -eq 0) { Ok (Msg extOk) }
                else { Warn (Msg extInstallFail) }
            } else {
                Warn (Msg extUnverified)
            }
        } else {
            Warn (Msg noCode)
        }
    }
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

Write-Host ''
Write-Host (Msg done) -ForegroundColor White
foreach ($line in 'next1', 'next2', 'next3', 'next4', 'next5', 'next6') { Write-Host (Msg $line) }
