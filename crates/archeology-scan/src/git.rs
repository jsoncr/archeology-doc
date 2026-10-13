//! Historial git vía el ejecutable `git` (RF-016).
//!
//! Se usa el CLI en lugar de una librería para evitar compilar libgit2 y
//! para funcionar en entornos donde el repo tiene configuración poco
//! habitual (worktrees, submodules). Si `git` no existe o no hay repo,
//! se devuelve `None` sin error (RF-022).

use archeology_core::model::GitStats;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

const MAX_COMMITS: usize = 500;

/// `git` en `root` sin comillas ni escapes en rutas no ASCII
/// (`facturación.py` y no `"factura\303\247i\303\263n.py"`) y sin
/// ejecutar fsmonitor ni hooks del repositorio (ver
/// [`archeology_core::gitcmd`]).
fn git(root: &Path) -> Command {
    archeology_core::gitcmd::git_command(root)
}

pub fn analyze(root: &Path) -> Option<GitStats> {
    let inside = git(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .ok()?;
    if !inside.status.success() || String::from_utf8_lossy(&inside.stdout).trim() != "true" {
        return None;
    }

    // Último commit.
    let last = git(root)
        .args(["log", "-1", "--format=%H|%aI|%s"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    // Autores.
    let authors_out = git(root)
        .args(["shortlog", "-sne", "--all"])
        .output()
        .ok()?;
    let authors: Vec<String> = String::from_utf8_lossy(&authors_out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    // Hotspots: archivos más tocados en los últimos N commits. `--relative`
    // y `-- .`: en un monorepo solo cuenta el proyecto y las rutas quedan
    // relativas a su raíz, como en el inventario. Cada commit empieza con
    // NUL + hash (SHA-1 o SHA-256), así que no se confunde con una ruta.
    let log_out = git(root)
        .args([
            "log",
            &format!("-n{MAX_COMMITS}"),
            "--name-only",
            "--relative",
            "--format=%x00%H",
            "--",
            ".",
        ])
        .output()
        .ok()?;
    let mut hotspots: BTreeMap<String, u32> = BTreeMap::new();
    let mut commits = 0u32;
    for line in String::from_utf8_lossy(&log_out.stdout).lines() {
        if line.starts_with('\0') {
            commits += 1;
            continue;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        *hotspots.entry(line.to_string()).or_insert(0) += 1;
    }
    // Mantener solo el top 50.
    let mut ranked: Vec<(String, u32)> = hotspots.into_iter().collect();
    ranked.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    ranked.truncate(50);
    let hotspots = ranked.into_iter().collect();

    Some(GitStats {
        commits_analyzed: commits,
        last_commit: last,
        authors,
        hotspots,
    })
}

/// Archivos del proyecto cambiados desde `reference` hasta el árbol de
/// trabajo actual, incluidos los cambios sin commitear (`sync --since`).
/// Rutas relativas a `root`. `None` si la ref no es válida o no hay repo.
pub fn changed_since(root: &Path, reference: &str) -> Option<Vec<String>> {
    // Una ref que empieza por `-` sería una opción de git (`--output=...`).
    if reference.starts_with('-') {
        return None;
    }
    let out = git(root)
        .args([
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--name-only",
            "--relative",
            "-z",
            reference,
            "--",
            ".",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .map(|l| l.to_string())
            .filter(|l| !l.is_empty())
            .collect(),
    )
}

/// Prefijo del proyecto dentro del repositorio (`""` en la raíz, `sub/` en
/// un monorepo).
pub fn repo_prefix(root: &Path) -> Option<String> {
    let out = git(root)
        .args(["rev-parse", "--show-prefix"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Archivos modificados o nuevos del proyecto según `git status`, relativos
/// a `root` (None si no hay repo). `git status --porcelain` da rutas
/// relativas a la raíz del repositorio: se quita el prefijo del proyecto.
pub fn dirty_files(root: &Path) -> Option<std::collections::BTreeSet<String>> {
    let prefix = repo_prefix(root)?;
    let out = git(root)
        .args([
            "status",
            "--porcelain",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut files = std::collections::BTreeSet::new();
    let mut entries = text.split('\0');
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            continue;
        }
        let (code, path) = entry.split_at(3);
        // Renombrados/copiados: la siguiente entrada es la ruta de origen.
        if code.starts_with('R') || code.starts_with('C') {
            entries.next();
        }
        files.insert(path.strip_prefix(&prefix).unwrap_or(path).to_string());
    }
    Some(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn paths_are_relative_to_a_project_inside_a_monorepo() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let tmp = std::env::temp_dir().join(format!("adoc-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let sub = tmp.join("sub");
        std::fs::create_dir_all(sub.join("src")).unwrap();
        std::fs::create_dir_all(tmp.join("otro")).unwrap();
        run(&tmp, &["init", "-q"]);
        run(&tmp, &["config", "user.email", "t@t"]);
        run(&tmp, &["config", "user.name", "t"]);
        std::fs::write(sub.join("src/facturación.py"), "a\n").unwrap();
        std::fs::write(tmp.join("otro/x.py"), "b\n").unwrap();
        run(&tmp, &["add", "-A"]);
        run(&tmp, &["commit", "-qm", "uno"]);
        std::fs::write(sub.join("src/facturación.py"), "a\nc\n").unwrap();
        std::fs::write(tmp.join("otro/x.py"), "b\nd\n").unwrap();

        let dirty = dirty_files(&sub).unwrap();
        assert_eq!(
            dirty.into_iter().collect::<Vec<_>>(),
            ["src/facturación.py"]
        );
        assert_eq!(changed_since(&sub, "HEAD").unwrap(), ["src/facturación.py"]);
        assert!(changed_since(&sub, "--output=/tmp/x").is_none());
        let stats = analyze(&sub).unwrap();
        assert_eq!(stats.commits_analyzed, 1);
        assert!(stats.hotspots.contains_key("src/facturación.py"));
        assert!(!stats.hotspots.keys().any(|k| k.contains("otro")));
        std::fs::remove_dir_all(&tmp).ok();
    }
}
