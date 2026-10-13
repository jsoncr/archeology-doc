//! Herramientas externas: Node, VitePress y OpenSpec.
//!
//! VitePress se busca en este orden:
//! 1. instalado en el proyecto (`node_modules/vitepress`),
//! 2. instalación compartida en el directorio de configuración (`adoc site setup`),
//! 3. instalación global de npm (`npm i -g vitepress`).
//!
//! Para 2 y 3 se enlaza `docs/node_modules` a las dependencias de esa
//! instalación, de modo que Vite resuelva `vue` y el tema sin tocar el
//! `package.json` del proyecto.
//!
//! Seguridad: `preview`, `site dev|build` y `check --site` ejecutan JavaScript
//! del proyecto (`docs/.vitepress/config.mts` y, si existe, su
//! `node_modules/vitepress`). Equivale a ejecutar código del repositorio: la
//! primera vez en cada proyecto se pide confirmación (o `--yes`) y la
//! aprobación se guarda junto a la de los adaptadores (`crate::trust`).

use anyhow::{Context, Result};
use archeology_core::{t, Project};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Versión de VitePress que instala `adoc site setup`.
pub const VITEPRESS_VERSION: &str = "1.6.4";
/// Versión mínima de Node que requiere VitePress.
pub const MIN_NODE_MAJOR: u32 = 18;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VitepressSource {
    Project,
    Shared,
    Global,
}

impl VitepressSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Shared => "shared",
            Self::Global => "global",
        }
    }

    /// Etiqueta en el idioma de la interfaz (`doctor`).
    pub fn label(&self) -> String {
        match self {
            Self::Project => t!("cli.toolchain.source_project"),
            Self::Shared => t!("cli.toolchain.source_shared"),
            Self::Global => t!("cli.toolchain.source_global"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Vitepress {
    pub source: VitepressSource,
    /// Directorio del paquete `vitepress`.
    pub package_dir: PathBuf,
    /// `node_modules` a enlazar como `docs/node_modules` (None si es del proyecto).
    pub deps_dir: Option<PathBuf>,
    pub version: Option<String>,
}

impl Vitepress {
    /// Script ejecutable según el campo `bin` de su package.json.
    pub fn bin(&self) -> PathBuf {
        let rel = read_package_json(&self.package_dir)
            .and_then(|v| match v.get("bin") {
                Some(serde_json::Value::String(s)) => Some(s.clone()),
                Some(serde_json::Value::Object(m)) => m
                    .get("vitepress")
                    .and_then(|b| b.as_str())
                    .map(String::from),
                _ => None,
            })
            // `bin` viene de un package.json que puede estar en el repositorio:
            // solo rutas dentro del paquete (sin `..` ni rutas absolutas).
            .filter(|rel| {
                Path::new(rel).components().all(|c| {
                    matches!(
                        c,
                        std::path::Component::Normal(_) | std::path::Component::CurDir
                    )
                })
            })
            .unwrap_or_else(|| "bin/vitepress.js".into());
        self.package_dir.join(rel)
    }
}

/// Directorio de la instalación compartida.
pub fn shared_dir() -> Result<PathBuf> {
    Ok(archeology_registry::config_dir()?.join("vitepress"))
}

/// Localiza VitePress para un proyecto (o solo compartido/global si no hay).
pub fn find_vitepress(project: Option<&Project>) -> Option<Vitepress> {
    if let Some(p) = project {
        let docs_modules = p.docs_dir().join("node_modules");
        // El enlace `docs/node_modules` lo crea archeology-doc: no es del proyecto.
        let docs_is_ours = fs::symlink_metadata(&docs_modules)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
        let mut bases = vec![p.root.join("node_modules")];
        if !docs_is_ours {
            bases.push(docs_modules);
        }
        for base in bases {
            let pkg = base.join("vitepress");
            if pkg.join("package.json").is_file() {
                return Some(make(VitepressSource::Project, pkg, None));
            }
        }
    }
    if let Ok(shared) = shared_dir() {
        let modules = shared.join("node_modules");
        let pkg = modules.join("vitepress");
        if pkg.join("package.json").is_file() {
            return Some(make(VitepressSource::Shared, pkg, Some(modules)));
        }
    }
    let root = npm_global_root()?;
    let pkg = root.join("vitepress");
    if !pkg.join("package.json").is_file() {
        return None;
    }
    // npm global anida las dependencias dentro del paquete.
    let nested = pkg.join("node_modules");
    let deps = if nested.join("vue").is_dir() {
        nested
    } else {
        root
    };
    Some(make(VitepressSource::Global, pkg, Some(deps)))
}

fn make(source: VitepressSource, package_dir: PathBuf, deps_dir: Option<PathBuf>) -> Vitepress {
    let version = read_package_json(&package_dir)
        .and_then(|v| v.get("version").and_then(|x| x.as_str()).map(String::from));
    Vitepress {
        source,
        package_dir,
        deps_dir,
        version,
    }
}

fn read_package_json(dir: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&fs::read_to_string(dir.join("package.json")).ok()?).ok()
}

fn npm_global_root() -> Option<PathBuf> {
    let out = Command::new(npm_cmd()).args(["root", "-g"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// `npm` es `npm.cmd` en Windows.
fn npm_cmd() -> &'static str {
    if cfg!(windows) {
        "npm.cmd"
    } else {
        "npm"
    }
}

/// Primera línea de `<cmd> --version`, si el comando existe.
///
/// Con límite de tiempo: un CLI colgado en el PATH no debe bloquear `doctor`.
pub fn tool_version(cmd: &str) -> Option<String> {
    use std::io::Read;
    let mut child = Command::new(cmd)
        .arg("--version")
        .env("OPENSPEC_TELEMETRY", "0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() < std::time::Duration::from_secs(10) => {
                std::thread::sleep(std::time::Duration::from_millis(25))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    let mut text = String::new();
    child.stdout.take()?.read_to_string(&mut text).ok()?;
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(String::from)
}

pub fn node_version() -> Option<String> {
    tool_version("node")
}

pub fn npm_version() -> Option<String> {
    tool_version(npm_cmd())
}

/// Versión mayor de Node (`v22.3.0` → 22).
pub fn major(version: &str) -> Option<u32> {
    version
        .trim_start_matches('v')
        .split('.')
        .next()?
        .parse()
        .ok()
}

/// Instala VitePress en el directorio compartido (`adoc site setup`).
pub fn setup(version: &str) -> Result<Vitepress> {
    let node = node_version().with_context(|| t!("cli.toolchain.node_missing"))?;
    if major(&node).map(|m| m < MIN_NODE_MAJOR).unwrap_or(true) {
        anyhow::bail!(t!(
            "cli.toolchain.node_old",
            version = node,
            min = MIN_NODE_MAJOR
        ));
    }
    let dir = shared_dir()?;
    fs::create_dir_all(&dir)?;
    if !dir.join("package.json").is_file() {
        archeology_core::fsutil::atomic_write(
            &dir.join("package.json"),
            b"{\n  \"name\": \"archeology-doc-vitepress\",\n  \"private\": true,\n  \"type\": \"module\"\n}\n",
        )?;
    }
    let pb = crate::ui::spinner(t!(
        "cli.toolchain.installing",
        version = version,
        path = dir.display()
    ));
    let output = Command::new(npm_cmd())
        .args([
            "install",
            "--no-fund",
            "--no-audit",
            "--save-exact",
            &format!("vitepress@{version}"),
        ])
        .current_dir(&dir)
        .output();
    pb.finish_and_clear();
    let output = output.with_context(|| t!("cli.toolchain.npm_run_failed"))?;
    if !output.status.success() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        anyhow::bail!(t!(
            "cli.toolchain.npm_install_failed",
            status = output.status
        ));
    }
    let modules = dir.join("node_modules");
    Ok(make(
        VitepressSource::Shared,
        modules.join("vitepress"),
        Some(modules),
    ))
}

/// Abre una ruta o URL con la aplicación del sistema (navegador).
/// Si no se puede, el usuario ya tiene la ruta impresa en la terminal.
pub fn open_in_browser(target: &str) {
    let ok = if cfg!(target_os = "macos") {
        Command::new("open")
            .arg(target)
            .status()
            .is_ok_and(|s| s.success())
    } else if cfg!(windows) {
        // Sin `cmd /C start`: cmd interpretaría `&`, `|`… de la ruta. `explorer`
        // recibe la ruta como un único argumento. Su código de salida no es
        // fiable (devuelve 1 aunque abra), así que basta con que arranque.
        Command::new("explorer").arg(target).spawn().is_ok()
    } else {
        Command::new("xdg-open")
            .arg(target)
            .status()
            .is_ok_and(|s| s.success())
    };
    if !ok {
        eprintln!("{}", t!("cli.toolchain.open_manually", target = target));
    }
}

/// Ejecuta `vitepress <sub> docs` con la instalación disponible.
/// `yes` aprueba (y recuerda) la ejecución del sitio de este proyecto.
pub fn run_vitepress(project: &Project, sub: &str, yes: bool) -> Result<()> {
    run_vitepress_with(project, sub, &[], yes)
}

/// Igual que [`run_vitepress`], con argumentos extra (p. ej. `--open`, `--port`).
pub fn run_vitepress_with(project: &Project, sub: &str, extra: &[String], yes: bool) -> Result<()> {
    let vp =
        find_vitepress(Some(project)).with_context(|| t!("cli.toolchain.vitepress_unavailable"))?;
    ensure_site_trusted(project, &vp, yes)?;
    if let Some(deps) = &vp.deps_dir {
        link_docs_modules(project, deps)?;
    }
    tracing::info!(source = vp.source.as_str(), "usando vitepress");
    let docs = project.docs_dir();
    let status = Command::new("node")
        .arg(vp.bin())
        .args([sub, &docs.display().to_string()])
        .args(extra)
        .current_dir(&project.root)
        .status()
        .with_context(|| t!("cli.toolchain.node_run_failed"))?;
    if !status.success() {
        anyhow::bail!(t!(
            "cli.toolchain.vitepress_failed",
            sub = sub,
            status = status
        ));
    }
    Ok(())
}

/// VitePress ejecuta la configuración del repositorio (JavaScript): la primera
/// vez en cada proyecto se pide confirmación. Sin terminal y sin `--yes`, error
/// con instrucciones. La aprobación se recuerda por proyecto.
pub fn ensure_site_trusted(project: &Project, vp: &Vitepress, yes: bool) -> Result<()> {
    use std::io::IsTerminal;
    if archeology_core::trust::is_site_trusted(&project.root) {
        return Ok(());
    }
    if !yes {
        let config = project.docs_dir().join(".vitepress");
        if !std::io::stdin().is_terminal() {
            anyhow::bail!(t!(
                "cli.toolchain.site_not_approved",
                path = project.root.display(),
                config = config.display(),
                extra = if vp.source == VitepressSource::Project {
                    t!("cli.toolchain.site_not_approved_project_modules")
                } else {
                    String::new()
                }
            ));
        }
        eprintln!(
            "{}",
            t!(
                "cli.toolchain.site_runs_js",
                config = config.display(),
                extra = if vp.source == VitepressSource::Project {
                    format!("\n  · {}", vp.package_dir.display())
                } else {
                    String::new()
                }
            )
        );
        if !super::confirm(&t!("cli.toolchain.confirm_site"), false)? {
            anyhow::bail!(t!("cli.toolchain.site_cancelled"));
        }
    }
    archeology_core::trust::trust_site(&project.root)?;
    Ok(())
}

/// Enlaza `docs/node_modules` a las dependencias de VitePress y lo excluye de git.
fn link_docs_modules(project: &Project, deps: &Path) -> Result<()> {
    let link = project.docs_dir().join("node_modules");
    if let Ok(meta) = fs::symlink_metadata(&link) {
        if !meta.file_type().is_symlink() {
            anyhow::bail!(t!(
                "cli.toolchain.docs_modules_foreign",
                path = link.display()
            ));
        }
        if fs::read_link(&link).ok().as_deref() == Some(deps) {
            return ensure_gitignored(project);
        }
        remove_link(&link)?;
    }
    symlink_dir(deps, &link).with_context(|| {
        t!(
            "cli.toolchain.link_failed",
            link = link.display(),
            target = deps.display()
        )
    })?;
    ensure_gitignored(project)
}

fn ensure_gitignored(project: &Project) -> Result<()> {
    let ignore = project.docs_dir().join(".gitignore");
    let text = fs::read_to_string(&ignore).unwrap_or_default();
    if !text.lines().any(|l| l.trim() == "node_modules/") {
        let mut out = text;
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("node_modules/\n");
        archeology_core::fsutil::write_in_root(&project.root, &ignore, out.as_bytes())?;
    }
    Ok(())
}

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(unix)]
fn remove_link(link: &Path) -> std::io::Result<()> {
    fs::remove_file(link)
}

#[cfg(windows)]
fn remove_link(link: &Path) -> std::io::Result<()> {
    fs::remove_dir(link)
}

/// ¿El `config` de VitePress importa el paquete en tiempo de ejecución?
/// Con una instalación global eso falla; la plantilla actual no lo hace.
pub fn config_imports_vitepress(project: &Project) -> bool {
    let vp = project.docs_dir().join(".vitepress");
    ["config.mts", "config.ts"].iter().any(|f| {
        fs::read_to_string(vp.join(f))
            .map(|t| {
                t.lines().any(|l| {
                    let l = l.trim();
                    l.starts_with("import ")
                        && !l.starts_with("import type")
                        && l.contains("'vitepress'")
                })
            })
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_node_major() {
        assert_eq!(major("v22.3.0"), Some(22));
        assert_eq!(major("18.0.1"), Some(18));
        assert_eq!(major("x"), None);
    }
}
