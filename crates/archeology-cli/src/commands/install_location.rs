//! Dónde está instalado el `adoc` en uso y quién lo gestiona (instalador,
//! `cargo install`, Homebrew). Lo comparten `adoc uninstall` y `adoc upgrade`.

use anyhow::{Context, Result};
use archeology_core::t;
use std::fs;
use std::path::{Path, PathBuf};

/// Binarios que instala archeology-doc (el mismo programa con dos nombres).
pub const BIN_NAMES: &[&str] = &["adoc", "archeology-doc"];

/// Instalación del ejecutable en uso.
pub struct InstallLocation {
    /// Ejecutable real (con los enlaces resueltos).
    pub running_real: PathBuf,
    /// Ruta con la que se invocó (p. ej. el enlace `~/.local/bin/adoc`).
    pub invoked: PathBuf,
    /// Carpeta de `invoked` (puede contener enlaces).
    pub bin_dir: PathBuf,
    /// `bin_dir` con los enlaces resueltos.
    pub bin_dir_real: PathBuf,
    /// El ejecutable real vive en un Cellar de Homebrew.
    pub homebrew: bool,
    /// `bin_dir` es la carpeta de `cargo install` (cargo lleva su propio registro).
    pub via_cargo: bool,
}

impl InstallLocation {
    /// Detecta la instalación del proceso actual.
    pub fn detect() -> Result<Self> {
        let running = std::env::current_exe().with_context(|| t!("cli.uninstall.no_executable"))?;
        let running_real =
            archeology_core::paths::canonicalize(&running).unwrap_or(running.clone());
        // La ruta que invocó el usuario, no su destino: borrar o sustituir el
        // destino de un enlace podría tocar archivos de otro gestor (Homebrew).
        let invoked = invoked_path(&running_real).unwrap_or(running);
        let bin_dir = invoked
            .parent()
            .with_context(|| t!("cli.uninstall.no_parent"))?
            .to_path_buf();
        let bin_dir_real =
            archeology_core::paths::canonicalize(&bin_dir).unwrap_or(bin_dir.clone());
        let homebrew = is_homebrew(&running_real);
        let via_cargo = !homebrew && installed_with_cargo(&bin_dir_real);
        Ok(Self {
            running_real,
            invoked,
            bin_dir,
            bin_dir_real,
            homebrew,
            via_cargo,
        })
    }

    /// ¿Lo gestiona cargo, por la carpeta invocada o por la del ejecutable
    /// real (un enlace hacia `~/.cargo/bin`)?
    pub fn managed_by_cargo(&self) -> bool {
        !self.homebrew
            && (self.via_cargo || self.running_real.parent().is_some_and(installed_with_cargo))
    }

    /// ¿El ejecutable real es uno de los nuestros (`adoc` o `archeology-doc`)?
    /// Un enlace hacia un programa con otro nombre no se toca.
    pub fn real_is_ours(&self) -> bool {
        self.running_real
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .is_some_and(|s| BIN_NAMES.contains(&s.as_str()))
    }
}

/// Ruta con la que se invocó el programa (`argv[0]`, resuelto en el `PATH`
/// si no trae carpeta), sin seguir enlaces. Solo si apunta a este mismo
/// ejecutable; si no (argv[0] arbitrario), None.
pub fn invoked_path(running_real: &Path) -> Option<PathBuf> {
    let argv0 = PathBuf::from(std::env::args_os().next()?);
    let path = if argv0.components().count() > 1 {
        if argv0.is_absolute() {
            argv0
        } else {
            std::env::current_dir().ok()?.join(argv0)
        }
    } else {
        archeology_agents::command::find_program(&argv0.to_string_lossy(), None)?
    };
    let real = archeology_core::paths::canonicalize(&path).ok()?;
    (real == running_real).then_some(path)
}

pub fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

/// ¿El ejecutable real vive en un Cellar de Homebrew?
pub fn is_homebrew(real: &Path) -> bool {
    real.components().any(|c| {
        let c = c.as_os_str().to_string_lossy();
        c == "Cellar" || c.eq_ignore_ascii_case("homebrew") || c == ".linuxbrew"
    })
}

/// ¿Instalado con `cargo install`? Entonces cargo lleva su propio registro.
pub fn installed_with_cargo(bin_dir: &Path) -> bool {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|h| PathBuf::from(h).join(".cargo"))
        });
    let Some(cargo_home) = cargo_home else {
        return false;
    };
    let cargo_bin = archeology_core::paths::canonicalize(&cargo_home.join("bin"))
        .unwrap_or_else(|_| cargo_home.join("bin"));
    bin_dir == cargo_bin && cargo_home.join(".crates.toml").is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_homebrew_installs() {
        assert!(is_homebrew(Path::new(
            "/opt/homebrew/Cellar/archeology-doc/0.1.0/bin/adoc"
        )));
        assert!(is_homebrew(Path::new(
            "/usr/local/Cellar/archeology-doc/0.1.0/bin/adoc"
        )));
        assert!(!is_homebrew(Path::new("/home/ana/.local/bin/adoc")));
    }
}
