//! Rutas globales de archeology-doc.
//!
//! ```text
//! ~/.archeology-doc/                  (o $ARCHEOLOGY_DOC_CONFIG_DIR)
//! ├── registry.toml                   índice de proyectos
//! ├── vitepress/                      VitePress compartido (`adoc site setup`)
//! └── projects/<nombre>-<id>/          espacio de trabajo de cada proyecto
//!                                     (los antiguos usan `<nombre>-<id8>`)
//!     ├── state.json  inventory.json  (state.json registra id y raíz)
//!     ├── tasks/      runs/
//! ```
//!
//! En el proyecto inspeccionado solo quedan `archeology-doc.toml` y los
//! resultados de documentación (vault, sitio, AGENTS.md…).

use crate::config::ArcheologyConfig;
use crate::error::{Error, Result};
use crate::t;
use std::fs;
use std::path::{Path, PathBuf};

/// Variable que sobrescribe el directorio de configuración (tests, CI).
pub const CONFIG_DIR_ENV: &str = "ARCHEOLOGY_DOC_CONFIG_DIR";
/// Nombre del directorio de configuración dentro del home.
pub const CONFIG_DIR_NAME: &str = ".archeology-doc";
/// Variable y directorio con el nombre anterior del proyecto (arqueology).
pub const LEGACY_CONFIG_DIR_ENV: &str = "ARQUEOLOGY_DOC_CONFIG_DIR";
pub const LEGACY_CONFIG_DIR_NAME: &str = ".arqueology-doc";

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

/// Directorio de configuración global: `~/.archeology-doc`.
pub fn config_dir() -> Result<PathBuf> {
    let env = std::env::var_os(CONFIG_DIR_ENV).or_else(|| std::env::var_os(LEGACY_CONFIG_DIR_ENV));
    if let Some(dir) = env {
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
        .ok_or_else(|| Error::InvalidConfig(t!("core.paths.no_home", var = CONFIG_DIR_ENV)))?;
    let home = PathBuf::from(home);
    let dir = home.join(CONFIG_DIR_NAME);
    migrate_renamed_config_dir(&home.join(LEGACY_CONFIG_DIR_NAME), &dir);
    Ok(dir)
}

/// `~/.arqueology-doc` (nombre anterior) → `~/.archeology-doc`, una sola vez:
/// registro, espacios de trabajo, aprobaciones de adaptadores y VitePress.
fn migrate_renamed_config_dir(legacy: &Path, dir: &Path) {
    let is_real_dir = fs::symlink_metadata(legacy)
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false);
    if !is_real_dir || dir.exists() {
        return;
    }
    match fs::rename(legacy, dir) {
        Ok(()) => eprintln!(
            "{}",
            t!(
                "core.paths.config_dir_renamed",
                from = legacy.display(),
                to = dir.display()
            )
        ),
        Err(e) => eprintln!(
            "{}",
            t!(
                "core.paths.config_dir_rename_failed",
                from = legacy.display(),
                to = dir.display(),
                error = e
            )
        ),
    }
}

/// Directorio que agrupa los espacios de trabajo de los proyectos.
pub fn projects_dir() -> Result<PathBuf> {
    Ok(config_dir()?.join("projects"))
}

/// Espacio de trabajo de un proyecto.
///
/// Los nuevos se crean como `projects/<nombre>-<id>/` (ID completo). Se
/// localiza por el ID, así que renombrar el proyecto no pierde su estado:
/// se reutiliza la carpeta existente, también las antiguas
/// `<nombre>-<id8>/` cuyo `state.json` corresponde a este ID (ver
/// [`find_workspace_by_id`]). La elección es determinista.
pub fn workspace_dir(config: &ArcheologyConfig) -> Result<PathBuf> {
    let base = projects_dir()?;
    let id = &config.project.id;
    if let Some(found) = find_workspace_in(&base, id, true) {
        return Ok(found);
    }
    let slug = slug(&config.project.name);
    let name = if slug.is_empty() {
        id.clone()
    } else {
        format!("{slug}-{id}")
    };
    // Defensa adicional a `validate()`: un solo componente normal.
    let mut parts = Path::new(&name).components();
    match (parts.next(), parts.next()) {
        (Some(std::path::Component::Normal(_)), None) => Ok(base.join(name)),
        _ => Err(Error::InvalidConfig(t!(
            "core.paths.workspace_invalid",
            name = name
        ))),
    }
}

/// Espacio de trabajo existente de un proyecto, buscado solo por su ID.
///
/// Coincidencia exacta: carpeta con el ID completo en el nombre, o carpeta
/// antigua `<nombre>-<id8>` cuyo `state.json` registra este ID. Una carpeta
/// antigua sin ID registrado solo cuenta si es la única candidata (se usa
/// para borrar espacios huérfanos: ante la duda, ninguno).
pub fn find_workspace_by_id(id: &str) -> Option<PathBuf> {
    find_workspace_in(&projects_dir().ok()?, id, false)
}

/// ID completo registrado en el `state.json` de un espacio de trabajo.
fn recorded_id(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("state.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("project_id")
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// Busca el espacio de trabajo de `id` en `base`. `pick_legacy`: con varias
/// carpetas antiguas sin ID registrado, toma la primera por nombre (en vez
/// de ninguna).
fn find_workspace_in(base: &Path, id: &str, pick_legacy: bool) -> Option<PathBuf> {
    if id.is_empty() {
        return None;
    }
    let short: String = id.chars().take(8).collect();
    let full_suffix = format!("-{id}");
    let short_suffix = format!("-{short}");
    // Orden por nombre: `read_dir` no garantiza ningún orden.
    let mut dirs: Vec<(String, PathBuf)> = fs::read_dir(base)
        .ok()?
        .flatten()
        // Solo directorios reales: no se siguen symlinks.
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
        .collect();
    dirs.sort();

    let mut exact = Vec::new();
    let mut legacy = Vec::new();
    for (name, path) in dirs {
        let by_full_name = name == id || name.ends_with(&full_suffix);
        let by_short_name = name == short || name.ends_with(&short_suffix);
        if !by_full_name && !by_short_name {
            continue;
        }
        match recorded_id(&path) {
            Some(rec) if rec == id => exact.push(path),
            // Pertenece a otro proyecto con el mismo prefijo.
            Some(_) => {}
            None if by_full_name => exact.push(path),
            None => legacy.push(path),
        }
    }
    if let Some(first) = exact.into_iter().next() {
        return Some(first);
    }
    match legacy.len() {
        0 => None,
        1 => legacy.pop(),
        _ if pick_legacy => {
            eprintln!(
                "{}",
                t!(
                    "core.paths.ambiguous_workspace",
                    prefix = short,
                    path = legacy[0].display()
                )
            );
            Some(legacy.remove(0))
        }
        _ => None,
    }
}

/// Registra en `state.json` del espacio de trabajo el ID completo y la raíz
/// del proyecto. Si la raíz registrada es otra (otro clon o worktree con el
/// mismo `project.id`), avisa: ambos comparten estado, tareas y ejecuciones.
/// Solo escribe si algo cambió y si el espacio de trabajo ya existe.
pub fn record_workspace_owner(workspace: &Path, id: &str, root: &Path) -> Result<()> {
    if !fs::symlink_metadata(workspace).is_ok_and(|m| m.is_dir()) {
        return Ok(());
    }
    let state_file = workspace.join("state.json");
    let mut state = crate::state::State::load(&state_file)?;
    let root_text = root.display().to_string();
    let mut changed = false;
    if state.project_id.as_deref() != Some(id) {
        state.project_id = Some(id.to_string());
        changed = true;
    }
    match state.project_root.as_deref() {
        Some(prev) if prev == root_text => {}
        Some(prev) => {
            eprintln!(
                "{}",
                t!(
                    "core.paths.workspace_shared",
                    workspace = workspace.display(),
                    previous = prev,
                    current = root_text
                )
            );
            state.project_root = Some(root_text);
            changed = true;
        }
        None => {
            state.project_root = Some(root_text);
            changed = true;
        }
    }
    if changed {
        state.save(&state_file)?;
    }
    Ok(())
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

/// Entradas del estado antiguo que se migran. Cualquier otra cosa en los
/// directorios antiguos (que vienen con el repositorio) se queda donde está.
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

/// Mueve el estado antiguo del proyecto (`<proyecto>/.arqueology-doc/` o
/// `<proyecto>/.archeology-doc/`, ver [`crate::config::LEGACY_STATE_DIRS`])
/// al espacio de trabajo global. Devuelve `true` solo si movió algo.
///
/// Esos directorios llegan con el repositorio, que puede no ser confiable:
/// no se siguen symlinks y solo se mueven las entradas conocidas.
pub fn migrate_legacy_state(root: &Path, workspace: &Path) -> Result<bool> {
    let mut moved = false;
    for dir in crate::config::LEGACY_STATE_DIRS {
        moved |= migrate_legacy_dir(&root.join(dir), workspace)?;
    }
    Ok(moved)
}

fn migrate_legacy_dir(legacy: &Path, workspace: &Path) -> Result<bool> {
    let is_real_dir = fs::symlink_metadata(legacy)
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false);
    if !is_real_dir || legacy == workspace {
        return Ok(false);
    }
    let mut moved = false;
    for e in fs::read_dir(legacy)?.flatten() {
        let name = e.file_name();
        if !LEGACY_ENTRIES.iter().any(|n| name == *n) {
            continue;
        }
        if e.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
            continue;
        }
        let dest = workspace.join(&name);
        if fs::symlink_metadata(&dest).is_ok() {
            // Ya migrado antes: no se pisa el estado global.
            continue;
        }
        fs::create_dir_all(workspace)?;
        move_path(&e.path(), &dest)?;
        moved = true;
    }
    // Solo se elimina si quedó vacío (nada que no hayamos movido).
    let _ = fs::remove_dir(legacy);
    Ok(moved)
}

/// `rename`, o copia + borrado si cruza dispositivos. La copia va a un
/// nombre temporal y se renombra al final: una copia interrumpida nunca
/// queda con el nombre definitivo (que se tomaría por ya migrada).
fn move_path(from: &Path, to: &Path) -> std::io::Result<()> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    let name = to
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let tmp = to.with_file_name(format!(".{name}.partial-{}", std::process::id()));
    // Restos de un intento anterior con el mismo PID.
    if let Ok(meta) = fs::symlink_metadata(&tmp) {
        if meta.is_dir() {
            fs::remove_dir_all(&tmp)?;
        } else {
            fs::remove_file(&tmp)?;
        }
    }
    if let Err(e) = copy_recursive(from, &tmp).and_then(|_| fs::rename(&tmp, to)) {
        let _ = fs::remove_dir_all(&tmp).or_else(|_| fs::remove_file(&tmp));
        return Err(e);
    }
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
    fn renamed_config_dir_is_moved_once() {
        let tmp = std::env::temp_dir().join(format!("adoc-ren-{}", uuid::Uuid::new_v4()));
        let legacy = tmp.join(LEGACY_CONFIG_DIR_NAME);
        let dir = tmp.join(CONFIG_DIR_NAME);
        fs::create_dir_all(legacy.join("projects")).unwrap();
        fs::write(legacy.join("registry.toml"), "x").unwrap();
        migrate_renamed_config_dir(&legacy, &dir);
        assert!(dir.join("registry.toml").is_file() && !legacy.exists());
        // Si ya existe el nuevo, el anterior no se toca.
        fs::create_dir_all(&legacy).unwrap();
        migrate_renamed_config_dir(&legacy, &dir);
        assert!(legacy.exists());
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn migrates_both_legacy_spellings_and_reports_only_real_moves() {
        let tmp = std::env::temp_dir().join(format!("adoc-mig2-{}", uuid::Uuid::new_v4()));
        let root = tmp.join("repo");
        let ws = tmp.join("ws");
        fs::create_dir_all(root.join(".archeology-doc/runs")).unwrap();
        fs::create_dir_all(root.join(".arqueology-doc")).unwrap();
        fs::write(root.join(".arqueology-doc/state.json"), "{}").unwrap();
        assert!(migrate_legacy_state(&root, &ws).unwrap());
        assert!(ws.join("state.json").is_file() && ws.join("runs").is_dir());
        // Nada más que mover: `false`, aunque exista el directorio.
        fs::create_dir_all(root.join(".archeology-doc")).unwrap();
        fs::write(root.join(".archeology-doc/otro.txt"), "x").unwrap();
        assert!(!migrate_legacy_state(&root, &ws).unwrap());
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn workspace_lookup_is_exact_and_deterministic() {
        let base = std::env::temp_dir().join(format!("adoc-ws-{}", uuid::Uuid::new_v4()));
        let id = "3f2a9c1e-0000-4000-8000-000000000001";
        let other = "3f2a9c1e-0000-4000-8000-000000000002";
        // Carpeta antigua de otro proyecto con el mismo prefijo.
        fs::create_dir_all(base.join("aaa-3f2a9c1e")).unwrap();
        fs::write(
            base.join("aaa-3f2a9c1e/state.json"),
            format!(r#"{{"version":1,"project_id":"{other}"}}"#),
        )
        .unwrap();
        assert_eq!(find_workspace_in(&base, id, true), None);
        // Carpeta antigua de este proyecto (registrado en su estado).
        fs::create_dir_all(base.join("zzz-3f2a9c1e")).unwrap();
        fs::write(
            base.join("zzz-3f2a9c1e/state.json"),
            format!(r#"{{"version":1,"project_id":"{id}"}}"#),
        )
        .unwrap();
        assert_eq!(
            find_workspace_in(&base, id, true),
            Some(base.join("zzz-3f2a9c1e"))
        );
        // Con el ID completo en el nombre también se encuentra.
        assert_eq!(
            find_workspace_in(&base, other, false),
            Some(base.join("aaa-3f2a9c1e"))
        );
        fs::create_dir_all(base.join(format!("bbb-{other}"))).unwrap();
        assert_eq!(
            find_workspace_in(&base, other, false),
            Some(base.join("aaa-3f2a9c1e"))
        );
        // Un ID corto no captura carpetas ajenas que terminan igual.
        fs::create_dir_all(base.join("x-ab")).unwrap();
        assert_eq!(find_workspace_in(&base, "b", false), None);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn records_owner_and_root() {
        let ws = std::env::temp_dir().join(format!("adoc-own-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&ws).unwrap();
        record_workspace_owner(&ws, "id-1", Path::new("/a")).unwrap();
        let st = crate::state::State::load(&ws.join("state.json")).unwrap();
        assert_eq!(st.project_id.as_deref(), Some("id-1"));
        assert_eq!(st.project_root.as_deref(), Some("/a"));
        record_workspace_owner(&ws, "id-1", Path::new("/b")).unwrap();
        let st = crate::state::State::load(&ws.join("state.json")).unwrap();
        assert_eq!(st.project_root.as_deref(), Some("/b"));
        fs::remove_dir_all(&ws).ok();
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
