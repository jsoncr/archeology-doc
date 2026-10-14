//! Modelos de datos: inventario, tareas y resúmenes.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Inventario determinista producido por `scan` (RF-021).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Inventory {
    pub version: u32,
    pub generated_at: String,
    pub project_root: String,
    pub total_files: usize,
    pub total_bytes: u64,
    pub languages: BTreeMap<String, LanguageStats>,
    pub manifests: Vec<String>,
    pub frameworks: Vec<String>,
    pub entrypoints: Vec<String>,
    pub test_files: Vec<String>,
    pub test_frameworks: Vec<String>,
    /// Tests desactivados o marcados con skip (RF-014).
    pub disabled_tests: Vec<String>,
    pub ci_files: Vec<String>,
    pub doc_files: Vec<String>,
    pub markers: Vec<Marker>,
    /// Hotspots de historial git (ruta → commits), si hay repo.
    pub git: Option<GitStats>,
    /// Detección de OpenSpec (RF-150).
    pub openspec: Option<OpenSpecInventory>,
    /// Módulos inferidos a partir de directorios de primer nivel.
    pub modules: Vec<ModuleInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LanguageStats {
    pub files: usize,
    pub bytes: u64,
    pub lines: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub kind: String,
    pub path: String,
    pub line: u32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitStats {
    pub commits_analyzed: u32,
    pub last_commit: Option<String>,
    pub authors: Vec<String>,
    pub hotspots: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenSpecInventory {
    pub path: String,
    pub capabilities: Vec<String>,
    pub changes_active: Vec<String>,
    pub changes_archived: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    pub name: String,
    pub path: String,
    pub files: usize,
    pub bytes: u64,
    pub languages: Vec<String>,
    pub has_tests: bool,
    /// `false` = solo archivos directos del directorio (módulo dividido).
    #[serde(default = "default_true")]
    pub recursive: bool,
    /// Archivos de código (sin docs ni configuración): mide el trabajo real.
    #[serde(default)]
    pub code_files: usize,
}

fn default_true() -> bool {
    true
}

/// Tipos de tarea del planner (RF-032).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskType {
    Overview,
    Module,
    Feature,
    Contradictions,
    Tests,
    Glossary,
    UserManual,
    Agents,
    SpecCompliance,
    SpecBaseline,
}

impl TaskType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Module => "module",
            Self::Feature => "feature",
            Self::Contradictions => "contradictions",
            Self::Tests => "tests",
            Self::Glossary => "glossary",
            Self::UserManual => "user-manual",
            Self::Agents => "agents",
            Self::SpecCompliance => "spec-compliance",
            Self::SpecBaseline => "spec-baseline",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    #[default]
    Pending,
    Running,
    Done,
    Failed,
    /// Exportada para ejecución manual.
    Manual,
    /// El agente terminó con una pregunta: espera `adoc task answer`.
    Question,
}

/// Definición de tarea (contrato §14.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub task_id: String,
    #[serde(rename = "type")]
    pub kind: TaskType,
    #[serde(default)]
    pub module: Option<String>,
    #[serde(default)]
    pub status: TaskStatus,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub produce: Vec<String>,
    /// Texto libre del objetivo.
    pub objective: String,
}

/// Entrada de ejecución registrada en `state.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub agent: String,
    pub tasks_done: Vec<String>,
    pub tasks_failed: Vec<String>,
    /// Detalle por tarea: duración, código de salida e intentos (RF-044).
    #[serde(default)]
    pub tasks: Vec<TaskRun>,
}

/// Ejecución de una tarea dentro de una corrida.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRun {
    pub task_id: String,
    pub status: String,
    pub duration_ms: u128,
    pub exit_code: Option<i32>,
    pub attempts: u32,
    #[serde(default)]
    pub reason: Option<String>,
    /// Problemas de validación agrupados en frases legibles (si falló).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub problems: Vec<String>,
    /// Qué hacer para corregirlos.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    /// Primera frase del mensaje final del agente.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_message: Option<String>,
}
