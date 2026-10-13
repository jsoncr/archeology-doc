//! Registro global de proyectos (§8.15, RF-170 a RF-179).
//!
//! El registro es solo un índice (`~/.archeology-doc/registry.toml`, o
//! `$ARCHEOLOGY_DOC_CONFIG_DIR/registry.toml`);
//! la fuente de verdad es el estado de cada proyecto y puede reconstruirse
//! completo con `projects discover`.

use anyhow::{Context, Result};
use archeology_core::t;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Registry {
    #[serde(default)]
    pub projects: Vec<RegistryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub last_activity: String,
    pub last_command: String,
}

/// Directorio de configuración global: `~/.archeology-doc` (RF-170).
/// `ARCHEOLOGY_DOC_CONFIG_DIR` lo sobrescribe (tests, entornos aislados).
///
/// Si existe una ubicación anterior por plataforma (p. ej.
/// `~/Library/Application Support/arqueology-doc` o `.../archeology-doc`),
/// se migra una vez.
pub fn config_dir() -> Result<PathBuf> {
    let dir = archeology_core::paths::config_dir()?;
    if std::env::var_os(archeology_core::paths::CONFIG_DIR_ENV).is_none() {
        migrate_legacy_config_dir(&dir);
    }
    Ok(dir)
}

/// Ubicaciones usadas por versiones anteriores (convención de cada
/// plataforma), con ambas grafías del nombre del proyecto.
fn legacy_config_dirs() -> Vec<PathBuf> {
    ["arqueology-doc", "archeology-doc"]
        .iter()
        .filter_map(|name| directories::ProjectDirs::from("", "", name))
        .map(|d| d.config_dir().to_path_buf())
        .collect()
}

fn migrate_legacy_config_dir(dir: &Path) {
    for legacy in legacy_config_dirs() {
        migrate_one_legacy_dir(&legacy, dir);
    }
}

fn migrate_one_legacy_dir(legacy: &Path, dir: &Path) {
    // Sin seguir symlinks: solo se mueve un directorio real.
    let is_real_dir = fs::symlink_metadata(legacy).is_ok_and(|m| m.is_dir());
    if legacy == dir || !is_real_dir || dir.join("registry.toml").exists() {
        return;
    }
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let Ok(entries) = fs::read_dir(legacy) else {
        return;
    };
    for e in entries.flatten() {
        let dest = dir.join(e.file_name());
        if !dest.exists() {
            let _ = fs::rename(e.path(), &dest);
        }
    }
    if fs::remove_dir(legacy).is_ok() {
        eprintln!(
            "{}",
            t!(
                "registry.config_moved",
                from = legacy.display(),
                to = dir.display()
            )
        );
    }
}

pub fn registry_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("registry.toml"))
}

/// Preferencias del usuario (`config.toml`, p. ej. `language`): las
/// gestiona `archeology_core::i18n` (`adoc config`).
pub fn global_config_path() -> Result<PathBuf> {
    config_dir()?;
    Ok(archeology_core::i18n::user_config_path()?)
}

impl Registry {
    pub fn load() -> Result<Self> {
        Self::load_from(&registry_path()?)
    }

    fn load_from(path: &Path) -> Result<Self> {
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path)?;
        let reg: Self =
            toml::from_str(&text).with_context(|| t!("registry.corrupt", path = path.display()))?;
        Ok(reg)
    }

    /// Lee, modifica y guarda el registro bajo un único bloqueo, con
    /// escritura atómica (RF-179): dos procesos no pierden actualizaciones.
    pub fn update<T>(f: impl FnOnce(&mut Registry) -> T) -> Result<T> {
        Self::update_at(&registry_path()?, f)
    }

    pub fn update_at<T>(path: &Path, f: impl FnOnce(&mut Registry) -> T) -> Result<T> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _guard = FileLock::acquire(&path.with_extension("lock"), LOCK_TIMEOUT)?;
        let mut reg = Self::load_from(path)?;
        let out = f(&mut reg);
        write_durable(path, toml::to_string_pretty(&reg)?.as_bytes())?;
        Ok(out)
    }

    /// Registra o actualiza un proyecto. Re-vincula por ID si la ruta cambió (RF-175).
    pub fn upsert(&mut self, id: &str, name: &str, path: &Path, command: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        let canonical = archeology_core::paths::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
            .to_string();
        if let Some(entry) = self.projects.iter_mut().find(|p| p.id == id) {
            entry.name = name.to_string();
            entry.path = canonical;
            entry.last_activity = now;
            entry.last_command = command.to_string();
        } else {
            self.projects.push(RegistryEntry {
                id: id.to_string(),
                name: name.to_string(),
                path: canonical,
                last_activity: now,
                last_command: command.to_string(),
            });
        }
    }

    /// Busca por nombre o ID (o prefijo único de ID). `None` si no existe,
    /// si la consulta está vacía o si es ambigua (ver [`Self::resolve`], que
    /// distingue la ambigüedad y lista las candidatas).
    pub fn find(&self, name_or_id: &str) -> Option<&RegistryEntry> {
        self.resolve(name_or_id).ok().flatten()
    }

    /// Como [`Self::find`], pero una consulta ambigua (varios proyectos con
    /// el mismo nombre o un prefijo de ID compartido) es un error que lista
    /// las candidatas. Orden: ID exacto, nombre exacto, prefijo de ID.
    pub fn resolve(&self, name_or_id: &str) -> Result<Option<&RegistryEntry>> {
        let q = name_or_id.trim();
        if q.is_empty() {
            return Ok(None);
        }
        if let Some(p) = self.projects.iter().find(|p| p.id == q) {
            return Ok(Some(p));
        }
        let by_name: Vec<&RegistryEntry> = self.projects.iter().filter(|p| p.name == q).collect();
        match by_name.len() {
            1 => return Ok(Some(by_name[0])),
            0 => {}
            _ => return Err(ambiguous(q, &by_name)),
        }
        let by_prefix: Vec<&RegistryEntry> = self
            .projects
            .iter()
            .filter(|p| p.id.starts_with(q))
            .collect();
        match by_prefix.len() {
            0 => Ok(None),
            1 => Ok(Some(by_prefix[0])),
            _ => Err(ambiguous(q, &by_prefix)),
        }
    }

    /// Elimina entradas cuyo directorio ya no existe. Devuelve las eliminadas.
    pub fn prune(&mut self) -> Vec<RegistryEntry> {
        let (gone, kept): (Vec<_>, Vec<_>) = self
            .projects
            .drain(..)
            .partition(|p| !Path::new(&p.path).is_dir());
        self.projects = kept;
        gone
    }
}

fn ambiguous(q: &str, candidates: &[&RegistryEntry]) -> anyhow::Error {
    let list = candidates
        .iter()
        .map(|p| {
            t!(
                "registry.ambiguous_item",
                name = p.name,
                id = p.id,
                path = p.path
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    anyhow::anyhow!(t!("registry.ambiguous", query = q, list = list))
}

/// Escritura atómica y duradera: temporal en el mismo directorio + fsync +
/// rename + fsync del directorio (unix). Un corte de luz no deja el
/// registro vacío ni a medias.
fn write_durable(path: &Path, content: &[u8]) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension(format!("toml.{}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(content)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        if let Ok(dir) = fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

/// Descubre proyectos con `archeology-doc.toml` bajo `dir` (RF-177).
pub fn discover(dir: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    discover_inner(dir, 0, max_depth, &mut found);
    found
}

fn discover_inner(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    if depth > max_depth {
        return;
    }
    if dir.join("archeology-doc.toml").is_file() || dir.join("arqueology-doc.toml").is_file() {
        out.push(dir.to_path_buf());
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        discover_inner(&p, depth + 1, max_depth, out);
    }
}

/// Espera máxima por el bloqueo del registro.
const LOCK_TIMEOUT: Duration = Duration::from_secs(10);
/// Bloqueo exclusivo del sistema operativo (`flock` / `LockFileEx`) sobre un
/// archivo `.lock`. Si el proceso muere, el sistema lo libera solo: no hay
/// bloqueos huérfanos que limpiar ni carreras al limpiarlos (RNF-15). El
/// archivo no se borra al soltarlo: borrarlo permitiría que dos procesos
/// bloquearan archivos distintos con el mismo nombre.
pub struct FileLock {
    _file: fs::File,
}

impl FileLock {
    pub fn acquire(path: &Path, timeout: Duration) -> Result<Self> {
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        let start = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { _file: file }),
                Err(fs::TryLockError::WouldBlock) => {
                    if start.elapsed() > timeout {
                        anyhow::bail!(t!("registry.locked", path = path.display()));
                    }
                    std::thread::sleep(Duration::from_millis(15));
                }
                Err(fs::TryLockError::Error(e)) => return Err(e.into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_updates_are_not_lost() {
        let dir = std::env::temp_dir().join(format!("arq-reg-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("registry.toml");
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    Registry::update_at(&path, |r| {
                        r.upsert(
                            &format!("id-{i}"),
                            &format!("p{i}"),
                            Path::new("/tmp"),
                            "test",
                        )
                    })
                    .unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(Registry::load_from(&path).unwrap().projects.len(), 8);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn leftover_lock_file_does_not_block_and_held_lock_does() {
        let dir = std::env::temp_dir().join(format!("arq-lock-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let lock = dir.join("registry.lock");
        // Un archivo que quedó de un proceso muerto no bloquea.
        fs::write(&lock, "").unwrap();
        let held = FileLock::acquire(&lock, Duration::from_millis(100)).unwrap();
        // Mientras otro lo tiene, se agota el tiempo de espera.
        assert!(FileLock::acquire(&lock, Duration::from_millis(50)).is_err());
        drop(held);
        assert!(FileLock::acquire(&lock, Duration::from_millis(100)).is_ok());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn find_by_name_id_or_unique_prefix() {
        let mut r = Registry::default();
        r.upsert("abc-1", "uno", Path::new("/a"), "init");
        r.upsert("abd-2", "dos", Path::new("/b"), "init");
        assert_eq!(r.find("dos").unwrap().id, "abd-2");
        assert_eq!(r.find("abc").unwrap().name, "uno");
        assert!(r.find("ab").is_none());
        assert!(r.find("").is_none() && r.find("  ").is_none());
        assert!(r.resolve("ab").unwrap_err().to_string().contains("abc-1"));
    }

    #[test]
    fn duplicate_names_are_ambiguous() {
        let mut r = Registry::default();
        r.upsert("id-1", "app", Path::new("/a"), "init");
        r.upsert("id-2", "app", Path::new("/b"), "init");
        assert!(r.find("app").is_none());
        let err = r.resolve("app").unwrap_err().to_string();
        assert!(err.contains("id-1") && err.contains("id-2"), "{err}");
        assert_eq!(r.find("id-2").unwrap().path, "/b");
    }

    #[test]
    fn migrates_legacy_platform_dir_without_symlinks() {
        let tmp = std::env::temp_dir().join(format!("arq-leg-{}", uuid::Uuid::new_v4()));
        let legacy = tmp.join("old");
        let dir = tmp.join("new");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("registry.toml"), "").unwrap();
        migrate_one_legacy_dir(&legacy, &dir);
        assert!(dir.join("registry.toml").is_file() && !legacy.exists());
        fs::remove_dir_all(&tmp).ok();
    }
}
