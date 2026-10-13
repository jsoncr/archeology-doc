//! Resolución de comandos externos.
//!
//! En Windows, los CLIs instalados con npm son shims `claude.cmd`, `codex.cmd`…
//! `Command::new("claude")` solo prueba `claude.exe`, así que se busca el
//! programa en `PATH` con las extensiones de `PATHEXT`. En Unix el sistema
//! resuelve el `PATH` y el nombre se usa tal cual.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Extensiones por defecto si no hay `PATHEXT`.
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// Ruta del programa a lanzar para `cmd`. `path_override` es el `PATH` que
/// define el adaptador en `env`, si lo hace. En Unix se devuelve `cmd` tal
/// cual (el sistema resuelve el `PATH`).
pub fn resolve_program(cmd: &str, path_override: Option<&str>) -> PathBuf {
    if cfg!(windows) {
        if let Some(found) = find_program(cmd, path_override) {
            return found;
        }
    }
    PathBuf::from(cmd)
}

/// Busca `cmd` en el `PATH` (o en `path_override`) sin ejecutarlo. En
/// Windows prueba las extensiones de `PATHEXT` (`claude` → `claude.cmd`).
pub fn find_program(cmd: &str, path_override: Option<&str>) -> Option<PathBuf> {
    if cmd.is_empty() {
        return None;
    }
    let path_var = path_override
        .map(std::ffi::OsString::from)
        .or_else(|| std::env::var_os("PATH"));
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| DEFAULT_PATHEXT.into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(str::to_string)
            .collect()
    } else {
        vec![String::new()]
    };
    search_path(cmd, path_var.as_deref(), &exts)
}

/// Busca `cmd` probando cada extensión de `exts` (y el nombre tal cual si ya
/// trae extensión). Con separadores de ruta, no se mira el `PATH`.
pub(crate) fn search_path(cmd: &str, path_var: Option<&OsStr>, exts: &[String]) -> Option<PathBuf> {
    let candidates = |base: &Path| -> Vec<PathBuf> {
        let mut out = Vec::new();
        if base.extension().is_some() {
            out.push(base.to_path_buf());
        }
        for e in exts {
            let mut name = base.as_os_str().to_os_string();
            name.push(e);
            out.push(PathBuf::from(name));
        }
        out
    };
    let p = Path::new(cmd);
    if p.components().count() > 1 || p.is_absolute() {
        return candidates(p).into_iter().find(|c| c.is_file());
    }
    let path_var = path_var?;
    std::env::split_paths(path_var)
        .flat_map(|dir| candidates(&dir.join(cmd)))
        .find(|c| c.is_file())
}

/// Nombre del programa sin extensión y en minúsculas (`C:\…\Claude.CMD` → `claude`),
/// para reconocer CLIs conocidos en cualquier plataforma.
pub fn program_stem(cmd: &str) -> String {
    // `\` también separa en Windows; se trata igual en todas las plataformas.
    let last = cmd.rsplit(['/', '\\']).next().unwrap_or(cmd);
    Path::new(last)
        .file_stem()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_npm_shims_with_pathext() {
        let tmp = std::env::temp_dir().join(format!("adoc-pathext-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("claude.cmd"), "").unwrap();
        let exts = vec![".exe".to_string(), ".cmd".to_string()];
        let path = std::env::join_paths([tmp.clone()]).unwrap();
        assert_eq!(
            search_path("claude", Some(&path), &exts),
            Some(tmp.join("claude.cmd"))
        );
        assert_eq!(search_path("codex", Some(&path), &exts), None);
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn stem_ignores_extension_and_case() {
        assert_eq!(program_stem("claude"), "claude");
        assert_eq!(program_stem("C:\\npm\\Claude.CMD"), "claude");
        assert_eq!(program_stem("/usr/local/bin/codex"), "codex");
        assert_eq!(program_stem(""), "");
    }
}
