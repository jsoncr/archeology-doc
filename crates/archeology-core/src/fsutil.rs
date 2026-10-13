//! Escritura segura de archivos generados.
//!
//! El repositorio inspeccionado puede no ser confiable: un symlink versionado
//! (`AGENTS.md -> ~/.zshrc`, `docs/.vitepress -> /etc`) haría que una
//! escritura normal modificara archivos fuera del proyecto. Todas las salidas
//! generadas dentro del proyecto pasan por [`write_in_root`], que:
//!
//! - exige que el destino quede dentro de la raíz ([`crate::project::is_inside`]);
//! - rechaza el destino y cualquier ancestro existente entre la raíz y él
//!   si es un symlink (sin seguirlo, vía `symlink_metadata`);
//! - escribe de forma atómica: temporal en el mismo directorio + fsync + rename.

use crate::error::{Error, Result};
use crate::t;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// Comprueba que `path` está dentro de `root` y que ni él ni ningún
/// directorio existente entre `root` y él es un symlink.
pub fn check_no_symlinks(root: &Path, path: &Path) -> Result<()> {
    let rel = path.strip_prefix(root).map_err(|_| outside(root, path))?;
    let mut probe = root.to_path_buf();
    for c in rel.components() {
        match c {
            Component::Normal(name) => probe.push(name),
            Component::CurDir => continue,
            // `..`, raíz o prefijo: nunca en una salida generada.
            _ => return Err(outside(root, path)),
        }
        match fs::symlink_metadata(&probe) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(Error::Validation(t!(
                    "core.fsutil.symlink_in_path",
                    path = probe.display()
                )))
            }
            Ok(_) => {}
            // El resto de la ruta aún no existe: no puede ser un symlink.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(e.into()),
        }
    }
    if !crate::project::is_inside(root, path) {
        return Err(outside(root, path));
    }
    Ok(())
}

fn outside(root: &Path, path: &Path) -> Error {
    Error::Validation(t!(
        "core.fsutil.outside_project",
        path = path.display(),
        root = root.display()
    ))
}

/// Crea `dir` (y sus padres) dentro de `root` sin atravesar symlinks.
pub fn create_dir_in_root(root: &Path, dir: &Path) -> Result<()> {
    check_no_symlinks(root, dir)?;
    let rel = dir.strip_prefix(root).map_err(|_| outside(root, dir))?;
    let mut probe = root.to_path_buf();
    for c in rel.components() {
        if let Component::Normal(name) = c {
            probe.push(name);
        } else {
            continue;
        }
        match fs::symlink_metadata(&probe) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(Error::Validation(t!(
                    "core.fsutil.symlink_not_modified",
                    path = probe.display()
                )))
            }
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(Error::Validation(t!(
                    "core.fsutil.not_a_directory",
                    path = probe.display()
                )))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => match fs::create_dir(&probe) {
                Ok(()) => {}
                // Otro proceso lo creó a la vez: se vuelve a comprobar abajo.
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if fs::symlink_metadata(&probe)?.file_type().is_symlink() {
                        return Err(Error::Validation(t!(
                            "core.fsutil.symlink_not_modified",
                            path = probe.display()
                        )));
                    }
                }
                Err(e) => return Err(e.into()),
            },
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Lee un archivo sin seguir symlinks. `Ok(None)` si no existe; error si
/// es un symlink o no es UTF-8 válido.
pub fn read_to_string_nofollow(path: &Path) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(Error::Validation(t!(
            "core.fsutil.symlink_not_read",
            path = path.display()
        ))),
        Ok(meta) if !meta.is_file() => Err(Error::Validation(t!(
            "core.fsutil.not_a_file",
            path = path.display()
        ))),
        Ok(_) => match fs::read_to_string(path) {
            Ok(t) => Ok(Some(t)),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => Err(Error::Validation(t!(
                "core.fsutil.not_utf8",
                path = path.display()
            ))),
            Err(e) => Err(e.into()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Escritura atómica de `path` (sin comprobación de raíz): temporal con
/// nombre único en el mismo directorio + fsync + rename. Si el destino ya
/// existe, conserva sus permisos. Un destino que es un symlink se rechaza.
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let existing = match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(Error::Validation(t!(
                "core.fsutil.symlink_not_overwritten",
                path = path.display()
            )))
        }
        Ok(meta) => Some(meta),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "archivo".into());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let tmp = parent.join(format!(".{name}.{}-{nanos}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        // `create_new`: nunca reutiliza (ni sigue) un archivo existente.
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        f.write_all(content)?;
        f.sync_all()?;
        drop(f);
        if let Some(meta) = &existing {
            let _ = fs::set_permissions(&tmp, meta.permissions());
        }
        fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    // Persistir también la entrada del directorio (mejor esfuerzo).
    #[cfg(unix)]
    if let Ok(dir) = fs::File::open(&parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Escribe `path` dentro de `root` de forma segura (ver el módulo): crea los
/// directorios padres sin atravesar symlinks y escribe de forma atómica.
pub fn write_in_root(root: &Path, path: &Path, content: &[u8]) -> Result<()> {
    check_no_symlinks(root, path)?;
    if let Some(parent) = path.parent() {
        create_dir_in_root(root, parent)?;
    }
    // Comprobación final tras crear los padres (carreras con otro proceso).
    check_no_symlinks(root, path)?;
    atomic_write(path, content)
}

/// Como [`write_in_root`], solo si el contenido cambió. La lectura previa
/// tampoco sigue symlinks. Devuelve `true` si escribió.
pub fn write_if_changed_in_root(root: &Path, path: &Path, content: &str) -> Result<bool> {
    check_no_symlinks(root, path)?;
    let current = match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => fs::read(path).ok(),
        _ => None,
    };
    if current.as_deref() == Some(content.as_bytes()) {
        return Ok(false);
    }
    write_in_root(root, path, content.as_bytes())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("adoc-fs-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        crate::paths::canonicalize(&dir).unwrap()
    }

    #[test]
    fn writes_atomically_and_creates_parents() {
        let root = tmpdir("ok");
        let path = root.join("a/b/c.md");
        assert!(write_if_changed_in_root(&root, &path, "hola").unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "hola");
        assert!(!write_if_changed_in_root(&root, &path, "hola").unwrap());
        // No quedan temporales.
        let leftovers: Vec<_> = fs::read_dir(root.join("a/b"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        assert!(write_in_root(&root, &root.join("../fuera.md"), b"x").is_err());
        fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinked_targets_and_ancestors() {
        let root = tmpdir("root");
        let outside = tmpdir("out");
        fs::write(outside.join("victima"), "original").unwrap();
        // Destino symlink.
        std::os::unix::fs::symlink(outside.join("victima"), root.join("AGENTS.md")).unwrap();
        assert!(write_if_changed_in_root(&root, &root.join("AGENTS.md"), "x").is_err());
        assert!(read_to_string_nofollow(&root.join("AGENTS.md")).is_err());
        // Ancestro symlink.
        std::os::unix::fs::symlink(&outside, root.join(".github")).unwrap();
        let target = root.join(".github/copilot-instructions.md");
        assert!(write_in_root(&root, &target, b"x").is_err());
        assert!(create_dir_in_root(&root, &root.join(".github/sub")).is_err());
        assert_eq!(
            fs::read_to_string(outside.join("victima")).unwrap(),
            "original"
        );
        assert!(!outside.join("copilot-instructions.md").exists());
        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&outside).ok();
    }
}
