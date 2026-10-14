//! `adoc upgrade`: actualiza `adoc` y `archeology-doc` a la última versión
//! publicada en la carpeta /download del sitio (la misma que usan los
//! instaladores y que arma `WEBSITE/dist.sh`).
//!
//! Flujo: `latest.json` → comparar versiones → descargar
//! `adoc-<target>.tar.gz|.zip` → verificar su SHA-256 contra `latest.json` y
//! contra `SHA256SUMS` (deben coincidir los tres) → extraer en memoria →
//! escribir los binarios nuevos junto a los actuales y renombrarlos encima
//! (atómico en la misma carpeta) → `adoc --version` para confirmar.
//!
//! Las instalaciones de cargo y Homebrew no se sustituyen: se indica el
//! comando de su gestor. `adoc doctor` usa [`passive_check`] (resultado en
//! caché 24 h) para avisar de versiones nuevas sin bloquear.

use super::install_location::{InstallLocation, BIN_NAMES};
use anyhow::{bail, Context, Result};
use archeology_core::t;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

#[derive(clap::Args)]
pub struct UpgradeArgs {
    /// Only report whether a newer version exists (exit code 0 either way).
    #[arg(long)]
    check: bool,
    /// Output format of --check.
    #[arg(long, default_value = "text", value_parser = ["text", "json"])]
    format: String,
    /// Do not ask for confirmation (required in non-interactive mode).
    #[arg(long, short = 'y')]
    yes: bool,
    /// Reinstall even if this is already the latest version.
    #[arg(long)]
    force: bool,
    /// Also update the VS Code extension (requires the `code` command).
    #[arg(long)]
    with_vscode: bool,
}

/// Versión del binario en ejecución.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Target de Rust con el que se compiló (lo fija `build.rs`).
const BUILD_TARGET: &str = env!("ADOC_BUILD_TARGET");
/// Carpeta /download oficial.
const DEFAULT_DOWNLOAD: &str = "https://archeology-doc.com/download";
/// Misma variable que usan los instaladores.
const DOWNLOAD_ENV: &str = "ARCHEOLOGY_DOC_DOWNLOAD";
/// Clave de `~/.archeology-doc/config.toml` con la URL de descarga.
const DOWNLOAD_CONFIG_KEY: &str = "download_url";
/// Desactiva la comprobación pasiva de `adoc doctor`.
const NO_CHECK_ENV: &str = "ADOC_NO_UPDATE_CHECK";
/// Caché de la comprobación pasiva (en el directorio de configuración).
const CHECK_CACHE_FILE: &str = "update-check.json";
/// Vigencia de la caché de la comprobación pasiva.
const CHECK_TTL_SECONDS: i64 = 24 * 60 * 60;
/// Repositorio para compilar desde el código.
const REPO_URL: &str = "https://github.com/jsoncr/archeology-doc";
/// Extensión de VS Code publicada junto a los binarios.
const VSIX_FILE: &str = "archeology-doc.vsix";

/// Límite de tamaño de una descarga (archivo de binarios, extensión).
const MAX_DOWNLOAD_BYTES: u64 = 200 * 1024 * 1024;
/// Límite de `latest.json` y `SHA256SUMS`.
const MAX_INDEX_BYTES: u64 = 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Tiempo total de una descarga grande (incluye el cuerpo).
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
/// Tiempo total de `latest.json` / `SHA256SUMS`.
const INDEX_TIMEOUT: Duration = Duration::from_secs(30);
/// Tiempo total de la comprobación pasiva (`adoc doctor`).
const PASSIVE_TIMEOUT: Duration = Duration::from_secs(3);

/// `latest.json` (lo genera `WEBSITE/dist.sh`).
#[derive(Debug, Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    files: Vec<ManifestFile>,
}

#[derive(Debug, Deserialize)]
struct ManifestFile {
    file: String,
    sha256: String,
    #[serde(default)]
    size: Option<u64>,
}

impl Manifest {
    fn file(&self, name: &str) -> Option<&ManifestFile> {
        self.files.iter().find(|f| f.file == name)
    }
}

/// Resultado de `--check --format json`.
#[derive(Serialize)]
struct CheckReport {
    current: String,
    latest: String,
    update_available: bool,
    /// Archivo para esta plataforma (None si no hay binarios publicados para ella).
    url: Option<String>,
}

pub fn run(args: UpgradeArgs) -> Result<()> {
    if args.format == "json" && !args.check {
        bail!(t!("cli.upgrade.json_needs_check"));
    }
    let base = download_base()?;
    let index = http_agent(INDEX_TIMEOUT);
    let manifest =
        crate::ui::with_spinner(t!("cli.upgrade.checking"), || fetch_manifest(&index, &base))?;
    // La caché de `doctor` aprovecha esta consulta.
    write_check_cache(&base, Some(&manifest.version));
    let newer = is_newer(&manifest.version, CURRENT_VERSION).with_context(|| {
        t!(
            "cli.upgrade.invalid_version",
            version = manifest.version.as_str()
        )
    })?;
    let artifact = running_artifact();

    if args.check {
        return print_check(&base, &manifest, newer, &artifact, &args.format);
    }
    if !newer && !args.force {
        println!(
            "{}",
            t!(
                "cli.upgrade.up_to_date",
                current = CURRENT_VERSION,
                latest = manifest.version.as_str()
            )
        );
        return Ok(());
    }
    let artifact = artifact.map_err(|e| anyhow::anyhow!(e.message()))?;

    // ¿Quién gestiona esta instalación?
    let install = InstallLocation::detect()?;
    if install.homebrew {
        println!(
            "{}",
            t!(
                "cli.upgrade.homebrew",
                path = install.running_real.display()
            )
        );
        return Ok(());
    }
    if install.managed_by_cargo() {
        println!(
            "{}",
            t!(
                "cli.upgrade.cargo_managed",
                path = install.running_real.display(),
                repo = REPO_URL
            )
        );
        return Ok(());
    }
    if !install.real_is_ours() {
        bail!(t!(
            "cli.upgrade.not_ours",
            path = install.running_real.display(),
            installer = installer_command(&base)
        ));
    }
    let dir = install
        .running_real
        .parent()
        .with_context(|| t!("cli.uninstall.no_parent"))?
        .to_path_buf();
    let entry = manifest.file(&artifact.file).with_context(|| {
        t!(
            "cli.upgrade.not_in_release",
            version = manifest.version.as_str(),
            file = artifact.file.as_str()
        )
    })?;

    // Plan y confirmación.
    if newer {
        println!(
            "{}",
            t!(
                "cli.upgrade.plan_upgrade",
                current = CURRENT_VERSION,
                latest = manifest.version.as_str()
            )
        );
    } else {
        println!(
            "{}",
            t!(
                "cli.upgrade.plan_reinstall",
                latest = manifest.version.as_str()
            )
        );
    }
    println!("{}", t!("cli.upgrade.plan_dir", dir = dir.display()));
    if install.invoked != install.running_real {
        println!(
            "{}",
            t!(
                "cli.upgrade.plan_link",
                link = install.invoked.display(),
                target = install.running_real.display()
            )
        );
    }
    if !args.yes {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            bail!(t!("cli.upgrade.needs_yes"));
        }
        if !super::confirm(&t!("cli.upgrade.confirm"), false)? {
            println!("{}", t!("cli.upgrade.cancelled"));
            return Ok(());
        }
    }
    // Antes de descargar: ¿se puede escribir en la carpeta?
    check_writable(&dir, &base)?;

    let download = http_agent(DOWNLOAD_TIMEOUT);
    let sums = fetch_sums(&index, &base)?;
    let archive = crate::ui::with_spinner(
        t!("cli.upgrade.downloading", file = artifact.file.as_str()),
        || {
            fetch(
                &download,
                &format!("{base}/{}", artifact.file),
                MAX_DOWNLOAD_BYTES,
            )
        },
    )?;
    verify(&archive, entry, &sums)?;
    println!(
        "{}",
        t!("cli.upgrade.sha_ok", file = artifact.file.as_str())
    );
    let binaries = extract_binaries(&archive, &artifact)?;

    let staged = Staged::write(&dir, &binaries)?;
    // El binario nuevo debe arrancar en esta máquina antes de sustituir nada.
    let main = staged.path_of(BIN_NAMES[0]);
    if let Err(e) = run_version(&main) {
        bail!(t!("cli.upgrade.staged_broken", error = e));
    }
    staged.commit()?;

    let new_bin = dir.join(exe_name(BIN_NAMES[0]));
    match run_version(&new_bin) {
        Ok(v) => println!("{}", t!("cli.upgrade.done", version = v)),
        Err(e) => bail!(t!(
            "cli.upgrade.verify_failed",
            error = e,
            installer = installer_command(&base)
        )),
    }
    if args.with_vscode {
        install_vscode(&download, &base, &manifest, &sums);
    }
    Ok(())
}

fn print_check(
    base: &str,
    manifest: &Manifest,
    newer: bool,
    artifact: &std::result::Result<Artifact, Unsupported>,
    format: &str,
) -> Result<()> {
    let url = artifact.as_ref().ok().map(|a| format!("{base}/{}", a.file));
    if format == "json" {
        let report = CheckReport {
            current: CURRENT_VERSION.into(),
            latest: manifest.version.clone(),
            update_available: newer,
            url,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    if newer {
        println!(
            "{}",
            t!(
                "cli.upgrade.update_available",
                current = CURRENT_VERSION,
                latest = manifest.version.as_str()
            )
        );
        match artifact {
            Ok(_) => println!("{}", t!("cli.upgrade.update_hint")),
            Err(e) => println!("{}", e.message()),
        }
    } else {
        println!(
            "{}",
            t!(
                "cli.upgrade.up_to_date",
                current = CURRENT_VERSION,
                latest = manifest.version.as_str()
            )
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// URL de descarga
// ---------------------------------------------------------------------------

/// URL de la carpeta /download: `ARCHEOLOGY_DOC_DOWNLOAD` > `download_url`
/// en `~/.archeology-doc/config.toml` > la oficial. Sin `/` final.
fn download_base() -> Result<String> {
    let from_env = std::env::var(DOWNLOAD_ENV)
        .ok()
        .filter(|v| is_configured(v));
    let url = match from_env {
        Some(v) => v,
        None => configured_download_url().unwrap_or_else(|| DEFAULT_DOWNLOAD.to_string()),
    };
    let url = url.trim().trim_end_matches('/').to_string();
    validate_url(&url)?;
    Ok(url)
}

/// Valor útil (no vacío ni el marcador sin reemplazar de los instaladores).
fn is_configured(value: &str) -> bool {
    let v = value.trim();
    !v.is_empty() && !v.starts_with("__ADOC_")
}

/// `download_url` de las preferencias del usuario, si está.
fn configured_download_url() -> Option<String> {
    let path = archeology_core::i18n::user_config_path().ok()?;
    let text = fs::read_to_string(path).ok()?;
    let table: toml::Table = text.parse().ok()?;
    table
        .get(DOWNLOAD_CONFIG_KEY)
        .and_then(|v| v.as_str())
        .filter(|v| is_configured(v))
        .map(String::from)
}

/// Solo https; http únicamente contra `localhost`/`127.0.0.1` (pruebas locales).
fn validate_url(url: &str) -> Result<()> {
    if !is_allowed_url(url) {
        bail!(t!("cli.upgrade.insecure_url", url = url));
    }
    Ok(())
}

fn is_allowed_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("https://") {
        return !rest.is_empty() && !rest.starts_with('/');
    }
    let Some(rest) = lower.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    // Sin credenciales (`user@host`) ni puertos raros en el nombre.
    if authority.contains('@') {
        return false;
    }
    let host = match authority.rsplit_once(':') {
        Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) => h,
        Some(_) => return false,
        None => authority,
    };
    host == "127.0.0.1" || host == "localhost"
}

/// Comando del instalador oficial para esta plataforma.
fn installer_command(base: &str) -> String {
    if cfg!(windows) {
        format!("irm {base}/install.ps1 | iex")
    } else {
        format!("curl -fsSL {base}/install.sh | sh")
    }
}

// ---------------------------------------------------------------------------
// Red
// ---------------------------------------------------------------------------

fn http_agent(total: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout(total)
        .redirects(5)
        .user_agent(&format!("adoc/{CURRENT_VERSION}"))
        .build()
}

/// GET de `url` con un límite de tamaño. La URL final (tras redirecciones)
/// también debe ser https (o loopback).
fn fetch(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Vec<u8>> {
    validate_url(url)?;
    let resp = match agent.get(url).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(code, _)) => {
            bail!(t!("cli.upgrade.http_status", url = url, status = code))
        }
        Err(e) => bail!(t!("cli.upgrade.network", url = url, error = e)),
    };
    validate_url(resp.get_url())?;
    let too_large = || {
        t!(
            "cli.upgrade.too_large",
            url = url,
            limit = crate::ui::bytes(limit)
        )
    };
    if let Some(len) = resp
        .header("Content-Length")
        .and_then(|v| v.trim().parse::<u64>().ok())
    {
        if len > limit {
            bail!(too_large());
        }
    }
    let mut body = Vec::new();
    resp.into_reader()
        .take(limit + 1)
        .read_to_end(&mut body)
        .with_context(|| t!("cli.upgrade.network", url = url, error = "read"))?;
    if body.len() as u64 > limit {
        bail!(too_large());
    }
    Ok(body)
}

fn fetch_manifest(agent: &ureq::Agent, base: &str) -> Result<Manifest> {
    let url = format!("{base}/latest.json");
    let body = fetch(agent, &url, MAX_INDEX_BYTES)?;
    serde_json::from_slice(&body)
        .with_context(|| t!("cli.upgrade.invalid_manifest", url = url.as_str()))
}

/// `SHA256SUMS`: archivo → suma (formato de `sha256sum`/`shasum`).
fn fetch_sums(agent: &ureq::Agent, base: &str) -> Result<HashMap<String, String>> {
    let body = fetch(agent, &format!("{base}/SHA256SUMS"), MAX_INDEX_BYTES)?;
    Ok(parse_sums(&String::from_utf8_lossy(&body)))
}

fn parse_sums(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let sum = parts.next()?;
            let file = parts.next()?.trim_start_matches('*');
            Some((file.to_string(), sum.to_ascii_lowercase()))
        })
        .collect()
}

/// La suma de `latest.json`, la de `SHA256SUMS` y la real deben coincidir.
fn verify(data: &[u8], entry: &ManifestFile, sums: &HashMap<String, String>) -> Result<()> {
    let file = entry.file.as_str();
    let listed = sums
        .get(file)
        .with_context(|| t!("cli.upgrade.sums_missing", file = file))?;
    let manifest_sum = entry.sha256.trim().to_ascii_lowercase();
    if *listed != manifest_sum {
        bail!(t!("cli.upgrade.sums_disagree", file = file));
    }
    if entry.size.is_some_and(|s| s != data.len() as u64) {
        bail!(t!("cli.upgrade.sha_mismatch", file = file));
    }
    if sha256_hex(data) != manifest_sum {
        bail!(t!("cli.upgrade.sha_mismatch", file = file));
    }
    Ok(())
}

fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

// ---------------------------------------------------------------------------
// Plataforma → archivo publicado
// ---------------------------------------------------------------------------

/// Archivo publicado para una plataforma (nombres de `WEBSITE/dist.sh`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Artifact {
    /// Target de Rust de los binarios del archivo.
    target: String,
    /// `adoc-<target>.tar.gz` o `.zip`.
    file: String,
    zip: bool,
}

impl Artifact {
    /// Carpeta raíz dentro del archivo: `adoc-<target>/`.
    fn dir(&self) -> String {
        format!("adoc-{}", self.target)
    }
}

/// Plataformas sin binarios publicados.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Unsupported {
    Musl(String),
    Unknown(String),
}

impl Unsupported {
    fn message(&self) -> String {
        match self {
            Unsupported::Musl(target) => {
                t!(
                    "cli.upgrade.unsupported_musl",
                    target = target,
                    repo = REPO_URL
                )
            }
            Unsupported::Unknown(target) => t!(
                "cli.upgrade.unsupported_target",
                target = target,
                repo = REPO_URL
            ),
        }
    }
}

/// Target → archivo, exactamente como los nombra `WEBSITE/dist.sh`.
fn artifact_for(target: &str) -> std::result::Result<Artifact, Unsupported> {
    let tar = |t: &str| Artifact {
        target: t.to_string(),
        file: format!("adoc-{t}.tar.gz"),
        zip: false,
    };
    match target {
        "x86_64-apple-darwin"
        | "aarch64-apple-darwin"
        | "x86_64-unknown-linux-gnu"
        | "aarch64-unknown-linux-gnu" => Ok(tar(target)),
        // Solo se publica el de GNU; uno compilado con MSVC usa el mismo.
        t if t.starts_with("x86_64-pc-windows-") => {
            let t = "x86_64-pc-windows-gnu";
            Ok(Artifact {
                target: t.into(),
                file: format!("adoc-{t}.zip"),
                zip: true,
            })
        }
        t if t.contains("musl") => Err(Unsupported::Musl(t.into())),
        t => Err(Unsupported::Unknown(if t.is_empty() {
            format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH)
        } else {
            t.into()
        })),
    }
}

/// Archivo para el binario en ejecución. Un binario x86_64 de macOS bajo
/// Rosetta en un Mac Apple Silicon pasa al nativo (como `install.sh`).
fn running_artifact() -> std::result::Result<Artifact, Unsupported> {
    let target = if BUILD_TARGET == "x86_64-apple-darwin" && apple_silicon() {
        "aarch64-apple-darwin"
    } else {
        BUILD_TARGET
    };
    artifact_for(target)
}

/// ¿Mac con Apple Silicon? `hw.optional.arm64` vale 1 incluso bajo Rosetta.
fn apple_silicon() -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    std::process::Command::new("sysctl")
        .args(["-n", "hw.optional.arm64"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
}

fn exe_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

// ---------------------------------------------------------------------------
// Archivos .tar.gz / .zip
// ---------------------------------------------------------------------------

/// Ruta de una entrada del archivo, normalizada a `/`. Error si es absoluta
/// o sale de la carpeta (`..`, unidad de Windows): zip-slip.
fn safe_entry_path(name: &str) -> Option<String> {
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/') || normalized.contains(':') {
        return None;
    }
    let path = Path::new(&normalized);
    let ok = path
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    ok.then(|| {
        path.components()
            .filter_map(|c| match c {
                Component::Normal(p) => Some(p.to_string_lossy().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/")
    })
}

/// Extrae en memoria `adoc` y `archeology-doc` de `adoc-<target>/`. Se
/// revisan todas las entradas: una ruta peligrosa rechaza el archivo entero.
fn extract_binaries(archive: &[u8], artifact: &Artifact) -> Result<Vec<(String, Vec<u8>)>> {
    let wanted: Vec<(String, String)> = BIN_NAMES
        .iter()
        .map(|n| (exe_name(n), format!("{}/{}", artifact.dir(), exe_name(n))))
        .collect();
    let mut found: HashMap<String, Vec<u8>> = HashMap::new();
    let file = artifact.file.as_str();
    let invalid =
        |e: &dyn std::fmt::Display| t!("cli.upgrade.archive_invalid", file = file, error = e);
    let mut visit = |name: &str, is_file: bool, reader: &mut dyn Read| -> Result<()> {
        let Some(path) = safe_entry_path(name) else {
            bail!(t!("cli.upgrade.unsafe_entry", file = file, entry = name));
        };
        if let Some((bin, _)) = wanted.iter().find(|(_, p)| *p == path) {
            if is_file {
                let mut data = Vec::new();
                reader
                    .take(MAX_DOWNLOAD_BYTES + 1)
                    .read_to_end(&mut data)
                    .map_err(|e| anyhow::anyhow!(invalid(&e)))?;
                if data.len() as u64 > MAX_DOWNLOAD_BYTES {
                    bail!(invalid(&"too large"));
                }
                found.insert(bin.clone(), data);
            }
        }
        Ok(())
    };
    if artifact.zip {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))
            .map_err(|e| anyhow::anyhow!(invalid(&e)))?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| anyhow::anyhow!(invalid(&e)))?;
            let name = entry.name().to_string();
            let is_file = entry.is_file();
            visit(&name, is_file, &mut entry)?;
        }
    } else {
        let gz = flate2::read::GzDecoder::new(archive);
        let mut tar = tar::Archive::new(gz);
        for entry in tar.entries().map_err(|e| anyhow::anyhow!(invalid(&e)))? {
            let mut entry = entry.map_err(|e| anyhow::anyhow!(invalid(&e)))?;
            let name = String::from_utf8_lossy(&entry.path_bytes()).to_string();
            let is_file = entry.header().entry_type().is_file();
            visit(&name, is_file, &mut entry)?;
        }
    }
    wanted
        .iter()
        .map(|(bin, path)| {
            found
                .remove(bin)
                .map(|data| (bin.clone(), data))
                .with_context(|| {
                    t!(
                        "cli.upgrade.binary_missing",
                        file = file,
                        entry = path.as_str()
                    )
                })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Sustitución de los binarios
// ---------------------------------------------------------------------------

/// Error claro si no se puede escribir en `dir` (carpeta del sistema, etc.).
fn check_writable(dir: &Path, base: &str) -> Result<()> {
    let probe = dir.join(format!(".adoc-upgrade-probe-{}", std::process::id()));
    match fs::write(&probe, b"") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            Ok(())
        }
        Err(e) => bail!(t!(
            "cli.upgrade.not_writable",
            dir = dir.display(),
            error = e,
            installer = installer_command(base)
        )),
    }
}

/// Binarios nuevos escritos junto a los actuales (`.adoc.new-<pid>`). Si no
/// se confirman con [`Staged::commit`], se borran al descartarse.
struct Staged {
    dir: PathBuf,
    files: Vec<(String, PathBuf)>,
    committed: bool,
}

impl Staged {
    fn write(dir: &Path, binaries: &[(String, Vec<u8>)]) -> Result<Self> {
        let mut staged = Staged {
            dir: dir.to_path_buf(),
            files: Vec::new(),
            committed: false,
        };
        for (name, data) in binaries {
            let tmp = dir.join(format!(".{name}.new-{}", std::process::id()));
            write_executable(&tmp, data)
                .with_context(|| t!("cli.upgrade.write_failed", path = tmp.display()))?;
            staged.files.push((name.clone(), tmp));
        }
        Ok(staged)
    }

    fn path_of(&self, bin: &str) -> PathBuf {
        let name = exe_name(bin);
        self.files
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, p)| p.clone())
            .unwrap_or_default()
    }

    /// Renombra cada binario nuevo encima del actual.
    fn commit(mut self) -> Result<()> {
        for (name, tmp) in &self.files {
            let dest = self.dir.join(name);
            replace_file(tmp, &dest)
                .with_context(|| t!("cli.upgrade.replace_failed", path = dest.display()))?;
        }
        self.committed = true;
        Ok(())
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.committed {
            for (_, tmp) in &self.files {
                let _ = fs::remove_file(tmp);
            }
        }
    }
}

fn write_executable(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let _ = fs::remove_file(path);
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o755);
    }
    let mut f = opts.open(path)?;
    std::io::Write::write_all(&mut f, data)?;
    f.sync_all()
}

/// Sustituye `dest` por `tmp` (misma carpeta: el renombrado es atómico). En
/// Windows un `.exe` en uso no se puede sobrescribir, pero sí renombrar: el
/// actual pasa a `<nombre>.old` (se borra al arrancar la próxima vez).
fn replace_file(tmp: &Path, dest: &Path) -> std::io::Result<()> {
    if cfg!(windows) && dest.exists() {
        let old = old_path(dest);
        let _ = fs::remove_file(&old);
        fs::rename(dest, &old)?;
        if let Err(e) = fs::rename(tmp, dest) {
            // Se deja el binario anterior en su sitio.
            let _ = fs::rename(&old, dest);
            return Err(e);
        }
        return Ok(());
    }
    fs::rename(tmp, dest)
}

/// `adoc.exe` → `adoc.exe.old`.
fn old_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    path.with_file_name(name)
}

/// Borra los `*.old` que dejó una actualización anterior en Windows (al
/// arrancar el CLI; silencioso: si siguen en uso, se intentará la próxima vez).
pub fn remove_stale_old_binaries() {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    else {
        return;
    };
    for name in BIN_NAMES {
        let old = old_path(&dir.join(exe_name(name)));
        if old.is_file() {
            let _ = fs::remove_file(old);
        }
    }
}

/// `<bin> --version` → primera línea de la salida.
fn run_version(bin: &Path) -> std::result::Result<String, String> {
    let out = std::process::Command::new(bin)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("{}", out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string())
}

// ---------------------------------------------------------------------------
// Extensión de VS Code
// ---------------------------------------------------------------------------

/// Descarga `archeology-doc.vsix` (verificada igual que los binarios) y la
/// instala con `code`. Los fallos son avisos: los binarios ya se actualizaron.
fn install_vscode(
    agent: &ureq::Agent,
    base: &str,
    manifest: &Manifest,
    sums: &HashMap<String, String>,
) {
    let Some(code) = archeology_agents::command::find_program("code", None) else {
        eprintln!("{}", t!("cli.upgrade.vscode_no_code"));
        return;
    };
    let result = (|| -> Result<()> {
        let entry = manifest.file(VSIX_FILE).with_context(|| {
            t!(
                "cli.upgrade.not_in_release",
                version = manifest.version.as_str(),
                file = VSIX_FILE
            )
        })?;
        let data =
            crate::ui::with_spinner(t!("cli.upgrade.downloading", file = VSIX_FILE), || {
                fetch(agent, &format!("{base}/{VSIX_FILE}"), MAX_DOWNLOAD_BYTES)
            })?;
        verify(&data, entry, sums)?;
        let dir = std::env::temp_dir().join(format!("adoc-upgrade-{}", std::process::id()));
        fs::create_dir_all(&dir)?;
        let file = dir.join(VSIX_FILE);
        fs::write(&file, &data)?;
        let status = std::process::Command::new(&code)
            .arg("--install-extension")
            .arg(&file)
            .arg("--force")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .status();
        let _ = fs::remove_dir_all(&dir);
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => bail!("code --install-extension: {s}"),
            Err(e) => bail!("code --install-extension: {e}"),
        }
    })();
    match result {
        Ok(()) => println!("{}", t!("cli.upgrade.vscode_ok")),
        Err(e) => eprintln!(
            "{}",
            t!("cli.upgrade.vscode_failed", error = format!("{e:#}"))
        ),
    }
}

// ---------------------------------------------------------------------------
// Versiones (semver)
// ---------------------------------------------------------------------------

/// Versión `x.y.z[-pre][+build]` (se acepta una `v` inicial).
#[derive(Debug, PartialEq, Eq)]
struct Version {
    core: [u64; 3],
    pre: Vec<String>,
}

fn parse_version(text: &str) -> Option<Version> {
    let text = text.trim();
    let text = text.strip_prefix('v').unwrap_or(text);
    // Los metadatos de compilación (`+…`) no cuentan para el orden.
    let text = text.split('+').next()?;
    let (core, pre) = match text.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (text, None),
    };
    let mut nums = core.split('.');
    let mut out = [0u64; 3];
    for slot in &mut out {
        let part = nums.next()?;
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        *slot = part.parse().ok()?;
    }
    if nums.next().is_some() {
        return None;
    }
    let pre = match pre {
        Some(p) if p.is_empty() || p.split('.').any(str::is_empty) => return None,
        Some(p) => p.split('.').map(String::from).collect(),
        None => Vec::new(),
    };
    Some(Version { core: out, pre })
}

/// Orden de semver: núcleo numérico; una pre-versión es menor que la versión
/// final; identificadores numéricos < alfanuméricos.
fn compare_versions(a: &Version, b: &Version) -> Ordering {
    a.core
        .cmp(&b.core)
        .then_with(|| match (a.pre.is_empty(), b.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (x, y) in a.pre.iter().zip(&b.pre) {
                    let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
                        (Ok(x), Ok(y)) => x.cmp(&y),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => x.cmp(y),
                    };
                    if ord != Ordering::Equal {
                        return ord;
                    }
                }
                a.pre.len().cmp(&b.pre.len())
            }
        })
}

/// ¿`latest` es posterior a `current`? None si alguna no es una versión válida.
fn is_newer(latest: &str, current: &str) -> Option<bool> {
    let l = parse_version(latest)?;
    let c = parse_version(current)?;
    Some(compare_versions(&l, &c) == Ordering::Greater)
}

// ---------------------------------------------------------------------------
// Comprobación pasiva (`adoc doctor`)
// ---------------------------------------------------------------------------

/// Caché de la última comprobación.
#[derive(Serialize, Deserialize)]
struct CheckCache {
    checked_at: String,
    base_url: String,
    /// None si no se pudo consultar (sin red): no se reintenta hasta que caduque.
    latest: Option<String>,
}

fn check_cache_path() -> Option<PathBuf> {
    archeology_core::paths::config_dir()
        .ok()
        .map(|d| d.join(CHECK_CACHE_FILE))
}

fn write_check_cache(base: &str, latest: Option<&str>) {
    let Some(path) = check_cache_path() else {
        return;
    };
    let cache = CheckCache {
        checked_at: chrono::Utc::now().to_rfc3339(),
        base_url: base.to_string(),
        latest: latest.map(String::from),
    };
    if let Ok(text) = serde_json::to_string_pretty(&cache) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = archeology_core::fsutil::atomic_write(&path, text.as_bytes());
    }
}

/// Resultado de la comprobación pasiva.
pub struct PassiveCheck {
    pub latest: String,
    pub update_available: bool,
}

/// Última versión publicada para `adoc doctor`: de la caché (24 h) o con una
/// consulta corta (3 s). None si está desactivada (`ADOC_NO_UPDATE_CHECK=1`),
/// sin red o con cualquier error: nunca hace fallar a doctor.
pub fn passive_check() -> Option<PassiveCheck> {
    if std::env::var(NO_CHECK_ENV).is_ok_and(|v| !v.is_empty() && v != "0") {
        return None;
    }
    let base = download_base().ok()?;
    let cached = check_cache_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<CheckCache>(&t).ok())
        .filter(|c| c.base_url == base)
        .filter(|c| {
            crate::ui::seconds_since_rfc3339(&c.checked_at)
                .is_some_and(|s| (0..CHECK_TTL_SECONDS).contains(&s))
        });
    let latest = match cached {
        Some(c) => c.latest,
        None => {
            let latest = fetch_manifest(&http_agent(PASSIVE_TIMEOUT), &base)
                .ok()
                .map(|m| m.version);
            write_check_cache(&base, latest.as_deref());
            latest
        }
    }?;
    let update_available = is_newer(&latest, CURRENT_VERSION)?;
    Some(PassiveCheck {
        latest,
        update_available,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison() {
        assert_eq!(is_newer("0.2.0", "0.1.0"), Some(true));
        assert_eq!(is_newer("0.1.10", "0.1.9"), Some(true));
        assert_eq!(is_newer("1.0.0", "0.99.99"), Some(true));
        assert_eq!(is_newer("0.1.0", "0.1.0"), Some(false));
        assert_eq!(is_newer("0.1.0", "0.2.0"), Some(false));
        assert_eq!(is_newer("v0.2.0", "0.1.0"), Some(true));
        // Pre-versiones: menores que la final; numéricos < alfanuméricos.
        assert_eq!(is_newer("0.2.0", "0.2.0-rc.1"), Some(true));
        assert_eq!(is_newer("0.2.0-rc.1", "0.2.0"), Some(false));
        assert_eq!(is_newer("0.2.0-rc.2", "0.2.0-rc.1"), Some(true));
        assert_eq!(is_newer("0.2.0-rc.10", "0.2.0-rc.9"), Some(true));
        assert_eq!(is_newer("0.2.0-beta", "0.2.0-alpha"), Some(true));
        assert_eq!(is_newer("0.2.0-alpha.1", "0.2.0-alpha"), Some(true));
        assert_eq!(is_newer("0.2.0-alpha", "0.2.0-1"), Some(true));
        // Los metadatos de compilación no cuentan.
        assert_eq!(is_newer("0.2.0+abc", "0.2.0"), Some(false));
        // Inválidas.
        assert_eq!(is_newer("0.2", "0.1.0"), None);
        assert_eq!(is_newer("latest", "0.1.0"), None);
        assert_eq!(is_newer("0.2.0.1", "0.1.0"), None);
        assert_eq!(is_newer("0.2.0-", "0.1.0"), None);
    }

    #[test]
    fn targets_map_to_dist_artifacts() {
        let a = artifact_for("aarch64-apple-darwin").unwrap();
        assert_eq!(a.file, "adoc-aarch64-apple-darwin.tar.gz");
        assert_eq!(a.dir(), "adoc-aarch64-apple-darwin");
        assert!(!a.zip);
        for t in [
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
        ] {
            assert_eq!(artifact_for(t).unwrap().file, format!("adoc-{t}.tar.gz"));
        }
        for t in ["x86_64-pc-windows-gnu", "x86_64-pc-windows-msvc"] {
            let a = artifact_for(t).unwrap();
            assert_eq!(a.file, "adoc-x86_64-pc-windows-gnu.zip");
            assert_eq!(a.dir(), "adoc-x86_64-pc-windows-gnu");
            assert!(a.zip);
        }
        assert!(matches!(
            artifact_for("x86_64-unknown-linux-musl"),
            Err(Unsupported::Musl(_))
        ));
        assert!(matches!(
            artifact_for("riscv64gc-unknown-linux-gnu"),
            Err(Unsupported::Unknown(_))
        ));
        // El binario en ejecución se compiló para algún target conocido.
        assert!(!BUILD_TARGET.is_empty());
    }

    #[test]
    fn only_https_or_loopback_http() {
        assert!(is_allowed_url("https://archeology-doc.com/download"));
        assert!(is_allowed_url("http://127.0.0.1:8080/download"));
        assert!(is_allowed_url("http://localhost:3000"));
        assert!(is_allowed_url("http://localhost/download"));
        assert!(!is_allowed_url("http://example.com/download"));
        assert!(!is_allowed_url("http://localhost.example.com/download"));
        assert!(!is_allowed_url("http://127.0.0.1@example.com/"));
        assert!(!is_allowed_url("ftp://example.com"));
        assert!(!is_allowed_url("https://"));
    }

    #[test]
    fn rejects_unsafe_archive_paths() {
        assert_eq!(
            safe_entry_path("adoc-x/adoc").as_deref(),
            Some("adoc-x/adoc")
        );
        assert_eq!(
            safe_entry_path("./adoc-x/adoc").as_deref(),
            Some("adoc-x/adoc")
        );
        assert_eq!(
            safe_entry_path("adoc-x\\adoc.exe").as_deref(),
            Some("adoc-x/adoc.exe")
        );
        assert!(safe_entry_path("../adoc").is_none());
        assert!(safe_entry_path("adoc-x/../../adoc").is_none());
        assert!(safe_entry_path("/usr/local/bin/adoc").is_none());
        assert!(safe_entry_path("\\Windows\\adoc.exe").is_none());
        assert!(safe_entry_path("C:\\adoc.exe").is_none());
    }

    #[test]
    fn zip_archives_are_read_and_zip_slip_is_rejected() {
        use std::io::Write;
        let artifact = artifact_for("x86_64-pc-windows-gnu").unwrap();
        let build = |entries: &[(&str, &[u8])]| {
            let mut buf = std::io::Cursor::new(Vec::new());
            {
                let mut w = zip::ZipWriter::new(&mut buf);
                let opts = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored);
                for (name, data) in entries {
                    w.start_file(*name, opts).unwrap();
                    w.write_all(data).unwrap();
                }
                w.finish().unwrap();
            }
            buf.into_inner()
        };
        let exe = std::env::consts::EXE_SUFFIX;
        let a = format!("adoc-x86_64-pc-windows-gnu/adoc{exe}");
        let b = format!("adoc-x86_64-pc-windows-gnu/archeology-doc{exe}");
        let ok = build(&[
            (&a, b"A"),
            (&b, b"B"),
            ("adoc-x86_64-pc-windows-gnu/README.md", b"r"),
        ]);
        let bins = extract_binaries(&ok, &artifact).unwrap();
        assert_eq!(bins[0], (format!("adoc{exe}"), b"A".to_vec()));
        assert_eq!(bins[1], (format!("archeology-doc{exe}"), b"B".to_vec()));

        let evil = build(&[(&a, b"A"), (&b, b"B"), ("../evil.exe", b"x")]);
        let err = extract_binaries(&evil, &artifact).unwrap_err().to_string();
        assert!(err.contains("evil.exe"), "{err}");

        let missing = build(&[(&a, b"A")]);
        assert!(extract_binaries(&missing, &artifact).is_err());
    }

    #[test]
    fn checksums_must_agree() {
        let data = b"hola";
        let sum = sha256_hex(data);
        let entry = ManifestFile {
            file: "adoc-x.tar.gz".into(),
            sha256: sum.to_uppercase(),
            size: Some(4),
        };
        let sums = parse_sums(&format!("{sum}  adoc-x.tar.gz\n{sum} *otro.zip\n"));
        assert_eq!(sums.get("otro.zip"), Some(&sum));
        verify(data, &entry, &sums).unwrap();
        // Contenido distinto.
        assert!(verify(b"adios", &entry, &sums).is_err());
        // SHA256SUMS no coincide con latest.json.
        let other = parse_sums(&format!("{}  adoc-x.tar.gz\n", "0".repeat(64)));
        assert!(verify(data, &entry, &other).is_err());
        // No aparece en SHA256SUMS.
        assert!(verify(data, &entry, &HashMap::new()).is_err());
    }

    #[test]
    fn windows_old_binary_name() {
        assert_eq!(
            old_path(Path::new("/x/adoc.exe")),
            PathBuf::from("/x/adoc.exe.old")
        );
    }
}
