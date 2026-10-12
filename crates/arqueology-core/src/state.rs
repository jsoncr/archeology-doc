//! Estado incremental en `.arqueology-doc/state.json` (RF-120, RF-123).

use crate::error::Result;
use crate::model::RunRecord;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub version: u32,
    /// Hash blake3 por archivo fuente (ruta relativa POSIX → hash).
    #[serde(default)]
    pub file_hashes: BTreeMap<String, String>,
    /// Estado de tareas (task_id → status).
    #[serde(default)]
    pub tasks: BTreeMap<String, String>,
    /// Mapeo archivo fuente → notas que lo citan como evidencia.
    #[serde(default)]
    pub source_to_notes: BTreeMap<String, Vec<String>>,
    /// Notas marcadas como desactualizadas (RF-121).
    #[serde(default)]
    pub stale_notes: Vec<String>,
    /// Historial de ejecuciones.
    #[serde(default)]
    pub runs: Vec<RunRecord>,
    /// Preguntas pendientes del agente por tarea (estado `question`).
    #[serde(default)]
    pub questions: BTreeMap<String, String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            version: 1,
            file_hashes: BTreeMap::new(),
            tasks: BTreeMap::new(),
            source_to_notes: BTreeMap::new(),
            stale_notes: Vec::new(),
            runs: Vec::new(),
            questions: BTreeMap::new(),
        }
    }
}

impl State {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.is_file() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path)?;
        let mut state: Self = match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(e) => {
                // Copia anterior a la última escritura (ver `save`).
                let bak = path.with_extension("json.bak");
                let recovered = fs::read_to_string(&bak)
                    .ok()
                    .and_then(|t| serde_json::from_str(&t).ok())
                    .ok_or(e)?;
                eprintln!(
                    "aviso: {} estaba dañado; se recuperó la copia {}",
                    path.display(),
                    bak.display()
                );
                recovered
            }
        };
        if state.version == 0 {
            state.version = 1;
        }
        Ok(state)
    }

    /// Escritura atómica: temporal con nombre único + fsync + rename. Antes
    /// se guarda la versión anterior en `state.json.bak` para recuperarla.
    pub fn save(&self, path: &Path) -> Result<()> {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let tmp = path.with_extension(format!("json.{}-{nanos}.tmp", std::process::id()));
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
            f.sync_all()?;
        }
        if path.is_file() {
            let _ = fs::copy(path, path.with_extension("json.bak"));
        }
        if let Err(e) = fs::rename(&tmp, path) {
            let _ = fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    }

    /// Notas afectadas por una lista de fuentes cambiadas.
    pub fn affected_notes(&self, changed: &[String]) -> Vec<String> {
        let mut notes = Vec::new();
        for src in changed {
            if let Some(list) = self.source_to_notes.get(src) {
                for n in list {
                    if !notes.contains(n) {
                        notes.push(n.clone());
                    }
                }
            }
        }
        notes
    }
}

/// Hash blake3 de un archivo, en hex.
///
/// Por streaming: no carga el archivo entero en memoria. Solo archivos
/// regulares (no `/dev/zero`, FIFOs ni directorios).
pub fn file_hash(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "no es un archivo regular",
        ));
    }
    let mut hasher = blake3::Hasher::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().to_hex().to_string())
}
