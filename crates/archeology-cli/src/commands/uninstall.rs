//! `adoc uninstall`: quita los binarios y, con `--purge`, los datos globales.
//!
//! Nunca toca los proyectos: `archeology-doc.toml`, `knowledge/`, `docs/` y
//! las regiones de AGENTS.md quedan en cada repositorio.

use super::install_location::{is_symlink, InstallLocation, BIN_NAMES};
use anyhow::{bail, Context, Result};
use archeology_core::t;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(clap::Args)]
pub struct UninstallArgs {
    /// Borra también los datos globales: registro de proyectos, espacios de
    /// trabajo (estado, tareas, ejecuciones), aprobaciones de adaptadores y
    /// VitePress compartido (`~/.archeology-doc/`).
    #[arg(long)]
    purge: bool,
    /// No pedir confirmación.
    #[arg(long, short = 'y')]
    yes: bool,
}

pub fn run(args: UninstallArgs) -> Result<()> {
    let InstallLocation {
        running_real,
        bin_dir,
        bin_dir_real,
        homebrew,
        via_cargo,
        ..
    } = InstallLocation::detect()?;
    let bins: Vec<PathBuf> = if homebrew {
        Vec::new()
    } else {
        BIN_NAMES
            .iter()
            .map(|n| bin_dir.join(format!("{n}{}", std::env::consts::EXE_SUFFIX)))
            // `symlink_metadata`: un enlace (incluso roto) también se quita.
            .filter(|p| {
                fs::symlink_metadata(p)
                    .map(|m| m.is_file() || m.file_type().is_symlink())
                    .unwrap_or(false)
            })
            .collect()
    };
    // Destinos de enlaces fuera de la carpeta: no se sabe si son nuestros.
    let foreign_targets: Vec<PathBuf> = bins
        .iter()
        .filter(|b| is_symlink(b))
        .filter_map(|b| archeology_core::paths::canonicalize(b).ok())
        .filter(|t| t.parent() != Some(bin_dir_real.as_path()))
        .collect();
    if homebrew {
        println!(
            "{}",
            t!("cli.uninstall.homebrew", path = running_real.display())
        );
    }
    let purge_dirs = if args.purge {
        purge_targets()?
    } else {
        Vec::new()
    };

    println!("{}", t!("cli.uninstall.will_remove"));
    if via_cargo {
        println!(
            "{}",
            t!("cli.uninstall.cargo_binaries", path = bin_dir.display())
        );
    } else {
        for b in &bins {
            if is_symlink(b) {
                println!("{}", t!("cli.uninstall.link_item", path = b.display()));
            } else {
                println!("  · {}", b.display());
            }
        }
        for t in &foreign_targets {
            println!(
                "{}",
                t!(
                    "cli.uninstall.foreign_kept",
                    target = t.display(),
                    dir = bin_dir.display()
                )
            );
        }
    }
    for d in &purge_dirs {
        println!("{}", t!("cli.uninstall.purge_item", path = d.display()));
    }
    if !args.purge {
        if let Ok(dir) = archeology_core::paths::config_dir() {
            if dir.is_dir() {
                println!("{}", t!("cli.uninstall.config_kept", path = dir.display()));
            }
        }
    }
    print_projects_hint(args.purge);

    if !super::confirm(&t!("cli.uninstall.confirm"), args.yes)? {
        println!("{}", t!("cli.uninstall.cancelled"));
        return Ok(());
    }

    // Primero los datos: después de borrar los binarios ya no hay vuelta atrás.
    for d in &purge_dirs {
        fs::remove_dir_all(d)
            .with_context(|| t!("cli.uninstall.delete_failed", path = d.display()))?;
        println!("{}", t!("cli.uninstall.deleted", path = d.display()));
    }

    if via_cargo {
        let status = std::process::Command::new("cargo")
            .args(["uninstall", "archeology-cli"])
            .status()
            .with_context(|| t!("cli.uninstall.cargo_run_failed"))?;
        if !status.success() {
            bail!(t!("cli.uninstall.cargo_failed"));
        }
    } else {
        for b in &bins {
            remove_binary(b, &running_real)?;
            println!("{}", t!("cli.uninstall.removed", path = b.display()));
        }
    }

    println!("\n{}", t!("cli.uninstall.done"));
    if cfg!(windows) && !via_cargo && !homebrew {
        println!(
            "{}",
            t!("cli.uninstall.windows_path", path = bin_dir.display())
        );
    }
    println!("{}", t!("cli.uninstall.vscode"));
    Ok(())
}

/// Carpetas que borra `--purge`: la configuración global (y la del nombre
/// anterior del proyecto si quedó). Solo si parecen nuestras: nunca el home,
/// la raíz ni una carpeta sin rastro de archeology-doc (p. ej. una
/// `ARCHEOLOGY_DOC_CONFIG_DIR` mal puesta).
fn purge_targets() -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    // (carpeta, es la del nombre anterior)
    let mut candidates = vec![(archeology_core::paths::config_dir()?, false)];
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        candidates.push((
            PathBuf::from(home).join(archeology_core::paths::LEGACY_CONFIG_DIR_NAME),
            true,
        ));
    }
    for (dir, legacy) in candidates {
        let Ok(meta) = fs::symlink_metadata(&dir) else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        if !looks_like_config_dir(&dir) {
            if legacy {
                // La carpeta antigua con contenido ajeno no frena la desinstalación.
                eprintln!(
                    "{}",
                    t!("cli.uninstall.legacy_unknown", path = dir.display())
                );
                continue;
            }
            bail!(t!("cli.uninstall.not_config_dir", path = dir.display()));
        }
        if !out.contains(&dir) {
            out.push(dir);
        }
    }
    Ok(out)
}

fn looks_like_config_dir(dir: &Path) -> bool {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    if dir.parent().is_none() || Some(dir.to_path_buf()) == home {
        return false;
    }
    let known = [
        "registry.toml",
        "projects",
        "trusted-adapters.toml",
        "vitepress",
    ];
    let has_known = known.iter().any(|k| dir.join(k).exists());
    // Solo entradas conocidas (o ninguna): no se borra una carpeta con otras cosas.
    let only_known = fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().all(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                known.contains(&name.as_str())
                    || name == "registry.lock"
                    || name.starts_with("registry.toml.")
                    || name.starts_with("trusted-adapters.toml.")
                    || name == ".DS_Store"
            })
        })
        .unwrap_or(false);
    has_known && only_known
}

/// Los proyectos conservan sus archivos: se indica cuáles son.
fn print_projects_hint(purge: bool) {
    let Ok(reg) = archeology_registry::Registry::load() else {
        return;
    };
    if reg.projects.is_empty() {
        return;
    }
    println!(
        "{}",
        t!(
            "cli.uninstall.projects_keep",
            extra = if purge {
                t!("cli.uninstall.projects_keep_purge")
            } else {
                String::new()
            }
        )
    );
    for p in &reg.projects {
        println!("  · {} — {}", p.name, p.path);
    }
}

/// Borra un binario. En Windows el ejecutable en uso no se puede borrar: se
/// renombra y se programa su borrado al terminar este proceso.
fn remove_binary(path: &Path, running: &Path) -> Result<()> {
    if cfg!(windows) && path == running {
        let old = path.with_extension(format!("old-{}", std::process::id()));
        fs::rename(path, &old)
            .with_context(|| t!("cli.uninstall.move_failed", path = path.display()))?;
        // La ruta va por variable de entorno: no se interpola en la línea de cmd.
        std::process::Command::new("cmd")
            .args([
                "/C",
                "ping 127.0.0.1 -n 3 >nul & del /f /q \"%ADOC_OLD_EXE%\"",
            ])
            .env("ADOC_OLD_EXE", &old)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| t!("cli.uninstall.schedule_failed"))?;
        return Ok(());
    }
    fs::remove_file(path).with_context(|| t!("cli.uninstall.delete_failed", path = path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_to_purge_foreign_dirs() {
        let tmp = std::env::temp_dir().join(format!("adoc-purge-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("projects")).unwrap();
        fs::write(tmp.join("registry.toml"), "").unwrap();
        assert!(looks_like_config_dir(&tmp));
        // Con algo ajeno dentro (p. ej. el home por error), no.
        fs::write(tmp.join("tesis.docx"), "").unwrap();
        assert!(!looks_like_config_dir(&tmp));
        // Vacía o sin rastro de archeology-doc, tampoco.
        let empty = tmp.join("vacia");
        fs::create_dir_all(&empty).unwrap();
        assert!(!looks_like_config_dir(&empty));
        assert!(!looks_like_config_dir(Path::new("/")));
        fs::remove_dir_all(&tmp).ok();
    }
}
