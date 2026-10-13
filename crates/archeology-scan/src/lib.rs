//! Scanner: inventario determinista del repositorio (§8.2).
//!
//! Todo lo que puede calcularse sin IA se calcula aquí: lenguajes,
//! manifiestos, puntos de entrada, tests, CI, marcadores, historial git
//! y detección de OpenSpec. La interpretación queda para los agentes.

pub mod detect;
pub mod git;
pub mod scanner;

pub use scanner::{scan_project, ScanEvent, ScanOptions};
