//! Rutas globales de arqueology-doc.
//!
//! ```text
//! ~/.arqueology-doc/                  (o $ARQUEOLOGY_DOC_CONFIG_DIR)
//! ├── registry.toml                   índice de proyectos
//! ├── vitepress/                      VitePress compartido (`adoc site setup`)
//! └── projects/<nombre>-<id8>/        espacio de trabajo de cada proyecto
//!     ├── state.json  inventory.json
//!     ├── tasks/      runs/
//! ```
//!
//! En el proyecto inspeccionado solo quedan `arqueology-doc.toml` y los
//! resultados de documentación (vault, sitio, AGENTS.md…).

use crate::config::ArqueologyConfig;
use crate::error::{Error, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Variable que sobrescribe el directorio de configuración (tests, CI).
pub const CONFIG_DIR_ENV: &str = "ARQUEOLOGY_DOC_CONFIG_DIR";
/// Nombre del directorio de configuración dentro del home.
pub const CONFIG_DIR_NAME: &str = ".arqueology-doc";

/// `canonicalize` sin el prefijo verbatim de Windows (`\\?\C:\...`).
///
/// En rutas verbatim Win32 no convierte `/` en separador, así que
/// `root.join(".github/copilot-instructions.md")` fallaría; además los CLIs
/// de agentes y Node manejan mal ese prefijo como directorio de trabajo.
/// Solo se quita en rutas de unidad (`\\?\C:\`), no en UNC.
pub fn canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    let real = fs::canonicalize(path)?;
    Ok(strip_verbatim(real))
}

fn strip_verbatim(path: PathBuf) -> PathBuf {
    if !cfg!(windows) {
        return path;
    }
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest)
            if rest.len() >= 3
                && rest.as_bytes()[0].is_ascii_alphabetic()
                && &rest[1..3] == r":\" =>
        {
            PathBuf::from(rest)
        }
        _ => path,
    }
}

/// Directorio de configuración global: `~/.arqueology-doc`.
pub fn config_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os(CONFIG_DIR_ENV) {
        let dir = PathBuf::from(dir);
        // Siempre absoluta: las rutas se comparten con otras herramientas.
        return Ok(if dir.is_absolute() {
            dir
        } else {
            std::env::current_dir()?.join(dir)
        });
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| {
            Error::InvalidConfig(format!(
                "no se pudo determinar el directorio home; define {CONFIG_DIR_ENV}"
            ))
        })?;
    Ok(PathBuf::from(home).join(CONFIG_DIR_NAME))
}

/// Directorio que agrupa los espacios de trabajo de los proyectos.
pub fn projects_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("projects"))
}

/// Espacio de trabajo de un proyecto: `projects/<nombre>-<id8>/`.
///
/// Se localiza por el ID (sufijo), así que renombrar el proyecto no pierde
/// su estado: se reutiliza la carpeta existente.
pub fn workspace_dir(config: &ArqueologyConfig) -> Result<PathBuf> {
    let base = projects_dir()?;
    let short: String = config.project.id.chars().take(8).collect();
    let suffix = format!("-{short}");
    if let Ok(entries) = fs::read_dir(&base) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if e.path().is_dir() && (name.ends_with(&suffix) || name == short) {
                return Ok(e.path());
            }
        }
    }
    let slug = slug(&config.project.name);
    let name = if slug.is_empty() {
        short
    } else {
        format!("{slug}{suffix}")
    };
    // Defensa adicional a `validate()`: un solo componente normal.
    let mut parts = Path::new(&name).components();
    match (parts.next(), parts.next()) {
        (Some(std::path::Component::Normal(_)), None) => Ok(base.join(name)),
        _ => Err(Error::InvalidConfig(format!(
            "project.id genera un espacio de trabajo inválido: '{name}'"
        ))),
    }
}

/// Espacio de trabajo existente de un proyecto, buscado solo por su ID.
pub fn find_workspace_by_id(id: &str) -> Option<PathBuf> {
    let short: String = id.chars().take(8).collect();
    let suffix = format!("-{short}");
    fs::read_dir(projects_dir().ok()?)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            p.is_dir() && (name.ends_with(&suffix) || name == short)
        })
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Entradas del estado antiguo que se migran. Cualquier otra cosa en
/// `.arqueology-doc/` (que viene con el repositorio) se queda donde está.
const LEGACY_ENTRIES: &[&str] = &[
    "state.json",
    "state.json.bak",
    "project.lock",
    "inventory.json",
    "tasks",
    "runs",
    "asks",
    "report.html",
    "generated-docs",
];

/// Mueve el estado antiguo `<proyecto>/.arqueology-doc/` al espacio de
/// trabajo global. Devuelve `true` si migró algo.
///
/// `.arqueology-doc/` llega con el repositorio, que puede no ser confiable:
/// no se siguen symlinks y solo se mueven las entradas conocidas.
pub fn migrate_legacy_state(root: &Path, workspace: &Path) -> Result<bool> {
    let legacy = root.join(crate::config::STATE_DIR);
    let is_real_dir = fs::symlink_metadata(&legacy)
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false);
    if !is_real_dir || legacy == workspace {
        return Ok(false);
    }
    fs::create_dir_all(workspace)?;
    for e in fs::read_dir(&legacy)?.flatten() {
        let name = e.file_name();
        if !LEGACY_ENTRIES.iter().any(|n| name == *n) {
            continue;
        }
        if e.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
            continue;
        }
        let dest = workspace.join(&name);
        if dest.exists() {
            // Ya migrado antes: no se pisa el estado global.
            continue;
        }
        move_path(&e.path(), &dest)?;
    }
    // Solo se elimina si quedó vacío (nada que no hayamos movido).
    let _ = fs::remove_dir(&legacy);
    Ok(true)
}

/// `rename`, o copia + borrado si cruza dispositivos.
fn move_path(from: &Path, to: &Path) -> std::io::Result<()> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_recursive(from, to)?;
    if from.is_dir() {
        fs::remove_dir_all(from)
    } else {
        fs::remove_file(from)
    }
}

fn copy_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        // No se copian enlaces: evita ciclos y lecturas fuera del árbol.
        return Ok(());
    }
    if meta.is_dir() {
        fs::create_dir_all(to)?;
        for e in fs::read_dir(from)?.flatten() {
            copy_recursive(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(from, to).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugifies_project_names() {
        assert_eq!(slug("Home Hub"), "home-hub");
        assert_eq!(slug("facturación_legacy"), "facturaci-n-legacy");
        assert_eq!(slug("--x--"), "x");
    }

    #[test]
    fn migration_only_moves_known_entries() {
        let tmp = std::env::temp_dir().join(format!("adoc-mig-{}", uuid::Uuid::new_v4()));
        let root = tmp.join("repo");
        let legacy = root.join(crate::config::STATE_DIR);
        let ws = tmp.join("ws");
        fs::create_dir_all(legacy.join("tasks")).unwrap();
        fs::write(legacy.join("state.json"), "{}").unwrap();
        fs::write(legacy.join(".zshenv"), "malicioso").unwrap();
        assert!(migrate_legacy_state(&root, &ws).unwrap());
        assert!(ws.join("state.json").is_file());
        assert!(ws.join("tasks").is_dir());
        assert!(!ws.join(".zshenv").exists());
        assert!(legacy.join(".zshenv").is_file());
        fs::remove_dir_all(&tmp).ok();
    }
}
