//! Orquestador y adaptadores de agentes (§8.4, §14.2).
//!
//! arqueology-doc no interpreta la respuesta textual del agente:
//! valida los archivos producidos. El éxito es `exit code 0` + validación
//! de salida superada.

pub mod adapter;
pub mod orchestrator;

pub use adapter::{adapter_warnings, check_adapter, AdapterStatus};
pub use orchestrator::{run_tasks, InspectCallback, InspectEvent, InspectOptions, InspectReport};
