//! Descubrimiento del proyecto inspeccionado y rutas estándar.

use crate::config::{ArcheologyConfig, CONFIG_FILE};
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

const LOCK_FILE: &str = "project.lock";

/// Lock exclusivo del proyecto; se libera al soltarlo (ver [`Project::lock`]).
pub struct ProjectLock {
    _file: std::fs::File,
}

/// Un proyecto inicializado con archeology-doc.
pub struct Project {
    /// Raíz del repositorio inspeccionado.
    pub root: PathBuf,
    pub config: ArcheologyConfig,
    /// Espacio de trabajo global (`~/.archeology-doc/projects/<nombre>-<id>/`):
    /// estado, inventario, tareas y ejecuciones. Fuera del repositorio.
    pub workspace: PathBuf,
}

impl Project {
    /// Busca `archeology-doc.toml` subiendo directorios desde `start`.
    pub fn discover(start: &Path) -> Result<Self> {
        let start = crate::paths::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
        let mut dir: Option<&Path> = Some(start.as_path());
        while let Some(d) = dir {
            migrate_legacy_config_file(d)?;
            let candidate = d.join(CONFIG_FILE);
            if candidate.is_file() {
                return Self::from_config(d, ArcheologyConfig::load(&candidate)?);
            }
            dir = d.parent();
        }
        Err(Error::ProjectNotFound)
    }

    /// Abre un proyecto en una ruta concreta (sin búsqueda ascendente).
    pub fn open(root: &Path) -> Result<Self> {
        migrate_legacy_config_file(root)?;
        let cfg_path = root.join(CONFIG_FILE);
        if !cfg_path.is_file() {
            return Err(Error::ProjectNotFound);
        }
        Self::from_config(root, ArcheologyConfig::load(&cfg_path)?)
    }

    /// Construye el proyecto y migra el estado antiguo (`.arqueology-doc/`,
    /// `.archeology-doc/`).
    fn from_config(root: &Path, config: ArcheologyConfig) -> Result<Self> {
        check_outputs_inside(root, &config)?;
        let workspace = crate::paths::workspace_dir(&config)?;
        if crate::paths::migrate_legacy_state(root, &workspace)? {
            eprintln!(
                "{}",
                crate::t!(
                    "core.project.legacy_state_moved",
                    from = root.display(),
                    to = workspace.display()
                )
            );
        }
        // Tolerante a fallos: un estado ilegible no impide abrir el proyecto.
        if let Err(e) = crate::paths::record_workspace_owner(&workspace, &config.project.id, root) {
            eprintln!(
                "{}",
                crate::t!("core.project.record_owner_failed", error = e)
            );
        }
        Ok(Self {
            root: root.to_path_buf(),
            config,
            workspace,
        })
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join(CONFIG_FILE)
    }

    /// Directorio de estado: el espacio de trabajo global del proyecto.
    pub fn state_dir(&self) -> PathBuf {
        self.workspace.clone()
    }

    pub fn state_file(&self) -> PathBuf {
        self.state_dir().join("state.json")
    }

    pub fn inventory_file(&self) -> PathBuf {
        self.state_dir().join("inventory.json")
    }

    pub fn tasks_dir(&self) -> PathBuf {
        self.state_dir().join("tasks")
    }

    pub fn runs_dir(&self) -> PathBuf {
        self.state_dir().join("runs")
    }

    pub fn knowledge_dir(&self) -> PathBuf {
        self.root.join(&self.config.paths.knowledge)
    }

    pub fn docs_dir(&self) -> PathBuf {
        self.root.join(&self.config.paths.docs)
    }

    pub fn memory_dir(&self) -> PathBuf {
        self.knowledge_dir().join("memory")
    }

    /// Idioma de la documentación generada (`[project] language`). El
    /// contenido generado (plantillas, AGENTS.md, MOCs, llms.txt, sitio,
    /// prompts de tareas) usa este idioma, no el de la interfaz.
    pub fn doc_lang(&self) -> crate::i18n::Lang {
        self.config.project.lang()
    }

    /// Toma el lock exclusivo del proyecto para un comando que modifica el
    /// estado. Si otro proceso de adoc lo tiene (p. ej. un `inspect` en otra
    /// terminal), falla en vez de pisar sus cambios. El sistema operativo lo
    /// libera al terminar el proceso, aunque termine de forma abrupta.
    pub fn lock(&self, command: &str) -> Result<ProjectLock> {
        use std::io::Write;
        std::fs::create_dir_all(&self.workspace)?;
        let path = self.workspace.join(LOCK_FILE);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        match file.try_lock() {
            Ok(()) => {
                file.set_len(0)?;
                write!(file, "pid {} · adoc {command}", std::process::id())?;
                Ok(ProjectLock { _file: file })
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                let holder = std::fs::read_to_string(&path).unwrap_or_default();
                Err(Error::Validation(crate::t!(
                    "core.project.locked",
                    holder = if holder.trim().is_empty() {
                        "adoc"
                    } else {
                        holder.trim()
                    }
                )))
            }
            Err(std::fs::TryLockError::Error(e)) => Err(e.into()),
        }
    }

    /// Hash de una fuente citada en una nota (`sources`). Las rutas vienen de
    /// texto escrito por agentes: se ignoran las que salen del proyecto.
    pub fn source_hash(&self, rel: &str) -> Option<String> {
        let path = self.root.join(rel);
        if !self.contains(&path) {
            return None;
        }
        crate::state::file_hash(&path).ok()
    }

    /// Comprueba que `path` (ya resuelto) queda dentro del proyecto.
    pub fn contains(&self, path: &Path) -> bool {
        is_inside(&self.root, path)
    }

    /// Ruta relativa a la raíz del proyecto (POSIX-style).
    pub fn rel(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
    }
}

/// `arqueology-doc.toml` (nombre anterior del proyecto) → `archeology-doc.toml`.
fn migrate_legacy_config_file(dir: &Path) -> Result<()> {
    let legacy = dir.join(crate::config::LEGACY_CONFIG_FILE);
    let current = dir.join(CONFIG_FILE);
    if legacy.is_file() && !current.exists() {
        std::fs::rename(&legacy, &current)?;
        eprintln!(
            "{}",
            crate::t!(
                "core.project.config_renamed",
                from = crate::config::LEGACY_CONFIG_FILE,
                to = CONFIG_FILE
            )
        );
    }
    Ok(())
}

/// `true` si `path` queda dentro de `root` tras resolver symlinks. Si `path`
/// aún no existe, se resuelve su ancestro existente más cercano.
pub fn is_inside(root: &Path, path: &Path) -> bool {
    let Ok(root) = crate::paths::canonicalize(root) else {
        return false;
    };
    let mut probe = path.to_path_buf();
    let mut rest = Vec::new();
    loop {
        if let Ok(real) = crate::paths::canonicalize(&probe) {
            let full = rest.iter().rev().fold(real, |acc, c| acc.join(c));
            return full.starts_with(&root);
        }
        match (probe.file_name(), probe.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name.to_os_string());
                probe = parent.to_path_buf();
            }
            _ => return false,
        }
    }
}

/// Las carpetas de salida no pueden salir del proyecto, tampoco mediante un
/// symlink versionado en el repositorio (`validate()` ya descarta `..`).
fn check_outputs_inside(root: &Path, config: &ArcheologyConfig) -> Result<()> {
    for (key, rel) in [
        ("paths.knowledge", &config.paths.knowledge),
        ("paths.docs", &config.paths.docs),
        ("openspec.path", &config.openspec.path),
    ] {
        if !is_inside(root, &root.join(rel)) {
            return Err(Error::InvalidConfig(crate::t!(
                "core.project.output_outside",
                key = key,
                value = rel
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn detects_symlinked_outputs_outside_root() {
        let tmp = std::env::temp_dir().join(format!("adoc-inside-{}", uuid::Uuid::new_v4()));
        let root = tmp.join("repo");
        let outside = tmp.join("fuera");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("docs")).unwrap();
        assert!(!is_inside(&root, &root.join("docs/wiki/x.md")));
        assert!(is_inside(&root, &root.join("knowledge/nuevo/x.md")));
        std::fs::remove_dir_all(&tmp).ok();
    }
}
