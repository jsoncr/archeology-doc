//! Confianza en los adaptadores de agentes.
//!
//! `archeology-doc.toml` viaja con el repositorio inspeccionado, que puede
//! venir de terceros. Un adaptador decide qué programa se ejecuta, con qué
//! argumentos y con qué variables de entorno, así que solo se ejecuta si:
//! - es idéntico a un adaptador predefinido, o
//! - el usuario lo aprobó para ese proyecto con `adoc agent trust <nombre>`.
//!
//! La aprobación se guarda en `~/.archeology-doc/trusted-adapters.toml` como
//! huella (hash) de la definición: si alguien cambia el adaptador en el
//! repositorio, deja de ser confiable hasta que se vuelva a aprobar.
//!
//! El mismo archivo guarda los proyectos cuyo sitio VitePress (que ejecuta
//! JavaScript del repositorio) el usuario aprobó (`adoc preview`, `adoc site`).

use crate::config::{builtin_adapters, AdapterConfig, AdapterMode};
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const TRUST_FILE: &str = "trusted-adapters.toml";

#[derive(Debug, Default, Serialize, Deserialize)]
struct TrustStore {
    #[serde(default)]
    adapter: Vec<TrustEntry>,
    /// Proyectos (raíz canónica) con el sitio VitePress aprobado.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    site: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TrustEntry {
    root: String,
    name: String,
    hash: String,
}

fn store_path() -> Result<PathBuf> {
    Ok(crate::paths::config_dir()?.join(TRUST_FILE))
}

/// Lee el almacén. Si no existe, vacío; si está dañado (TOML inválido o
/// ilegible), error: una aprobación nueva no debe borrar las existentes.
fn read_store() -> Result<TrustStore> {
    let path = store_path()?;
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(TrustStore::default()),
        Err(e) => return Err(e.into()),
    };
    toml::from_str(&text).map_err(|e| {
        crate::error::Error::InvalidConfig(crate::t!(
            "core.trust.store_corrupt",
            path = path.display(),
            error = e
        ))
    })
}

/// Almacén para consultas: si está dañado, nada cuenta como aprobado
/// (falla cerrado) y se avisa.
fn load_store() -> TrustStore {
    read_store().unwrap_or_else(|e| {
        // Una vez por proceso: se consulta por cada adaptador.
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| eprintln!("{}", crate::t!("core.common.warning", message = e)));
        TrustStore::default()
    })
}

/// Clave del proyecto: ruta canónica de su raíz.
fn root_key(root: &Path) -> String {
    crate::paths::canonicalize(root)
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .to_string()
}

/// `true` si algún argumento o variable de entorno usa `{model}`: entonces el
/// modelo forma parte de lo que se ejecuta.
fn uses_model_placeholder(cfg: &AdapterConfig) -> bool {
    cfg.args
        .iter()
        .chain(&cfg.interactive_args)
        .chain(cfg.env.values())
        .any(|a| a.contains("{model}"))
}

/// Huella de lo que decide qué se ejecuta. El timeout no cuenta. El modelo
/// solo cuenta si se sustituye en `{model}` (ahí decide parte de los
/// argumentos); si no, cambiarlo no exige una nueva aprobación (se valida aparte).
pub fn fingerprint(cfg: &AdapterConfig) -> String {
    let mut material = serde_json::json!({
        "mode": cfg.mode,
        "command": cfg.command,
        "args": cfg.args,
        "env": cfg.env,
        "stdin": cfg.stdin,
        "interactive_args": cfg.interactive_args,
    });
    if uses_model_placeholder(cfg) {
        material["model"] = serde_json::json!(cfg.model);
    }
    blake3::hash(material.to_string().as_bytes())
        .to_hex()
        .to_string()
}

/// `true` si `name` es un adaptador predefinido y la definición coincide con
/// ESE predefinido. Un nombre ajeno nunca cuenta como predefinido aunque copie
/// una definición predefinida (p. ej. `[agent.adapters.claude]` con la de `amp`).
/// Las configs antiguas sin `interactive_args` también cuentan: `task run
/// --interactive` completa esos argumentos desde el mismo predefinido.
pub fn is_builtin(name: &str, cfg: &AdapterConfig) -> bool {
    let Some(builtin) = builtin_adapters().remove(name) else {
        return false;
    };
    let fp = fingerprint(cfg);
    if fingerprint(&builtin) == fp {
        return true;
    }
    let mut bare = builtin;
    bare.interactive_args.clear();
    cfg.interactive_args.is_empty() && fingerprint(&bare) == fp
}

/// `true` si el adaptador se puede ejecutar en el proyecto `root`.
pub fn is_trusted(root: &Path, name: &str, cfg: &AdapterConfig) -> bool {
    if cfg.mode == AdapterMode::Export || is_builtin(name, cfg) {
        return true;
    }
    let entry = TrustEntry {
        root: root_key(root),
        name: name.to_string(),
        hash: fingerprint(cfg),
    };
    load_store().adapter.contains(&entry)
}

/// Aprueba el adaptador `name` tal como está definido ahora en `root`.
pub fn trust(root: &Path, name: &str, cfg: &AdapterConfig) -> Result<()> {
    let path = store_path()?;
    let mut store = read_store()?;
    let key = root_key(root);
    // Una sola aprobación por proyecto y adaptador: la nueva reemplaza a la vieja.
    store.adapter.retain(|e| !(e.root == key && e.name == name));
    store.adapter.push(TrustEntry {
        root: key,
        name: name.to_string(),
        hash: fingerprint(cfg),
    });
    save_store(&path, &store)
}

/// Escritura atómica del almacén (archivo temporal + rename).
fn save_store(path: &Path, store: &TrustStore) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    crate::fsutil::atomic_write(path, toml::to_string_pretty(store)?.as_bytes())
}

/// Retira la aprobación de `name` en `root`. Devuelve `true` si existía.
pub fn revoke(root: &Path, name: &str) -> Result<bool> {
    let path = store_path()?;
    let mut store = read_store()?;
    let key = root_key(root);
    let before = store.adapter.len();
    store.adapter.retain(|e| !(e.root == key && e.name == name));
    if store.adapter.len() == before {
        return Ok(false);
    }
    save_store(&path, &store)?;
    Ok(true)
}

/// `true` si el usuario aprobó ejecutar el sitio VitePress de `root`.
pub fn is_site_trusted(root: &Path) -> bool {
    load_store().site.contains(&root_key(root))
}

/// Aprueba ejecutar el sitio VitePress (JavaScript del repositorio) de `root`.
pub fn trust_site(root: &Path) -> Result<()> {
    let path = store_path()?;
    let mut store = read_store()?;
    let key = root_key(root);
    if !store.site.contains(&key) {
        store.site.push(key);
        save_store(&path, &store)?;
    }
    Ok(())
}

/// Mensaje para un adaptador no confiable.
pub fn untrusted_message(name: &str) -> String {
    crate::t!("core.trust.untrusted", name = name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_name_must_match_its_own_definition() {
        let builtin = builtin_adapters();
        assert!(is_builtin("claude", &builtin["claude"]));
        // `claude` con la definición peligrosa de `amp`: requiere aprobación.
        assert!(!is_builtin("claude", &builtin["amp"]));
        // Un nombre que no es predefinido nunca cuenta como predefinido.
        assert!(!is_builtin("mi-claude", &builtin["claude"]));
        // Configs antiguas sin interactive_args del mismo predefinido: sí.
        let mut legacy = builtin["claude"].clone();
        legacy.interactive_args.clear();
        assert!(is_builtin("claude", &legacy));
        assert!(!is_builtin("codex", &legacy));
    }

    #[test]
    fn save_store_creates_missing_dir() {
        let dir = std::env::temp_dir().join(format!("adoc-trust-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // Guardar crea la carpeta si falta y escribe de forma atómica.
        let nested = dir.join("nueva").join(TRUST_FILE);
        save_store(&nested, &TrustStore::default()).unwrap();
        assert!(nested.is_file());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn model_counts_in_fingerprint_only_with_placeholder() {
        let mut cfg = builtin_adapters()["claude"].clone();
        let base = fingerprint(&cfg);
        cfg.model = Some("opus".into());
        assert_eq!(fingerprint(&cfg), base);
        cfg.args.push("--model={model}".into());
        let with_placeholder = fingerprint(&cfg);
        cfg.model = Some("otro".into());
        assert_ne!(fingerprint(&cfg), with_placeholder);
    }
}
