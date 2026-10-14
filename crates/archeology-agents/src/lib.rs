//! Orquestador y adaptadores de agentes (§8.4, §14.2).
//!
//! archeology-doc no interpreta la respuesta textual del agente:
//! valida los archivos producidos. El éxito es `exit code 0` + validación
//! de salida superada. La única lectura de la salida es la de los límites
//! del agente (cuota, sesión, contexto; ver [`limits`]).

pub mod adapter;
pub mod command;
pub mod interrupt;
pub mod limits;
pub mod orchestrator;

pub use adapter::{adapter_warnings, check_adapter, AdapterStatus, HeadlessOutcome};
pub use interrupt::Interrupted;
pub use limits::{AgentLimit, LimitKind};
pub use orchestrator::{
    run_tasks, InspectCallback, InspectEvent, InspectOptions, InspectReport, LimitStop, TaskOutcome,
};
