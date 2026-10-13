//! Embebe los catálogos de traducción `locales/<idioma>/*.toml`.
//!
//! Genera `$OUT_DIR/locales.rs` con un `include_str!` por archivo, así basta
//! con añadir un `.toml` nuevo en `locales/en/` y `locales/es/` (p. ej. uno
//! por crate) para que se incluya sin tocar código.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Idiomas soportados (mismo orden que `i18n::Lang::ALL`).
const LANGS: &[&str] = &["en", "es"];

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("locales");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut out = String::new();
    for lang in LANGS {
        let dir = root.join(lang);
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut files = toml_files(&dir);
        files.sort();
        writeln!(
            out,
            "pub static {}: &[(&str, &str)] = &[",
            lang.to_uppercase()
        )
        .unwrap();
        for f in files {
            println!("cargo:rerun-if-changed={}", f.display());
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            writeln!(
                out,
                "    ({name:?}, include_str!({:?})),",
                f.display().to_string()
            )
            .unwrap();
        }
        writeln!(out, "];").unwrap();
    }
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("locales.rs");
    std::fs::write(dest, out).unwrap();
}

fn toml_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "toml"))
        .collect()
}
