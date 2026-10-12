//! Confianza en los adaptadores de agentes.
//!
//! `arqueology-doc.toml` viaja con el repositorio inspeccionado, que puede
//! venir de terceros. Un adaptador decide qué programa se ejecuta, con qué
//! argumentos y con qué variables de entorno, así que solo se ejecuta si:
//! - es idéntico a un adaptador predefinido, o
//! - el usuario lo aprobó para ese proyecto con `adoc agent trust <nombre>`.
//!
//! La aprobación se guarda en `~/.arqueology-doc/trusted-adapters.toml` como
//! huella (hash) de la definición: si alguien cambia el adaptador en el
//! repositorio, deja de ser confiable hasta que se vuelva a aprobar.

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

fn load_store() -> TrustStore {
    store_path()
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

/// Clave del proyecto: ruta canónica de su raíz.
fn root_key(root: &Path) -> String {
    crate::paths::canonicalize(root)
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .to_string()
}

/// Huella de lo que decide qué se ejecuta. El modelo y el timeout no cuentan:
/// cambiarlos no debe exigir una nueva aprobación (el modelo se valida aparte).
pub fn fingerprint(cfg: &AdapterConfig) -> String {
    let material = serde_json::json!({
        "mode": cfg.mode,
        "command": cfg.command,
        "args": cfg.args,
        "env": cfg.env,
        "stdin": cfg.stdin,
        "interactive_args": cfg.interactive_args,
    });
    blake3::hash(material.to_string().as_bytes())
        .to_hex()
        .to_string()
}

/// `true` si la definición coincide con algún adaptador predefinido. Las
/// configs antiguas sin `interactive_args` también cuentan: `task run
/// --interactive` completa esos argumentos desde el predefinido.
pub fn is_builtin(cfg: &AdapterConfig) -> bool {
    let fp = fingerprint(cfg);
    builtin_adapters().values().any(|b| {
        if fingerprint(b) == fp {
            return true;
        }
        let mut bare = b.clone();
        bare.interactive_args.clear();
        cfg.interactive_args.is_empty() && fingerprint(&bare) == fp
    })
}

/// `true` si el adaptador se puede ejecutar en el proyecto `root`.
pub fn is_trusted(root: &Path, name: &str, cfg: &AdapterConfig) -> bool {
    if cfg.mode == AdapterMode::Export || is_builtin(cfg) {
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
    let mut store = load_store();
    let key = root_key(root);
    // Una sola aprobación por proyecto y adaptador: la nueva reemplaza a la vieja.
    store.adapter.retain(|e| !(e.root == key && e.name == name));
    store.adapter.push(TrustEntry {
        root: key,
        name: name.to_string(),
        hash: fingerprint(cfg),
    });
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("toml.{}.tmp", std::process::id()));
    fs::write(&tmp, toml::to_string_pretty(&store)?)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// Retira la aprobación de `name` en `root`. Devuelve `true` si existía.
pub fn revoke(root: &Path, name: &str) -> Result<bool> {
    let path = store_path()?;
    let mut store = load_store();
    let key = root_key(root);
    let before = store.adapter.len();
    store.adapter.retain(|e| !(e.root == key && e.name == name));
    if store.adapter.len() == before {
        return Ok(false);
    }
    let tmp = path.with_extension(format!("toml.{}.tmp", std::process::id()));
    fs::write(&tmp, toml::to_string_pretty(&store)?)?;
    fs::rename(&tmp, &path)?;
    Ok(true)
}

/// Mensaje para un adaptador no confiable.
pub fn untrusted_message(name: &str) -> String {
    format!(
        "el adaptador '{name}' está definido en arqueology-doc.toml y no es uno predefinido: \
         no se ejecuta sin tu aprobación. Revísalo y apruébalo con `adoc agent trust {name}`, \
         o vuelve al predefinido con `adoc agent use {name} --reset`"
    )
}
