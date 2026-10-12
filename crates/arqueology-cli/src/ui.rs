//! Indicadores de progreso en la terminal (spinner + texto de la acción).
//!
//! Se dibujan en stderr y solo en una terminal interactiva: en CI, tests o
//! con la salida redirigida quedan ocultos y no ensucian la salida.

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressFinish, ProgressStyle};
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Desactivado con `-v` (los logs de tracing se mezclarían con el spinner)
/// o con `ADOC_NO_PROGRESS`.
static ENABLED: AtomicBool = AtomicBool::new(true);

pub fn init(verbose: u8) {
    let enabled = verbose == 0
        && std::env::var_os("ADOC_NO_PROGRESS").is_none()
        && std::io::stderr().is_terminal();
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Spinner con mensaje y tiempo transcurrido.
pub fn spinner(msg: impl Into<String>) -> ProgressBar {
    if !ENABLED.load(Ordering::Relaxed) {
        return ProgressBar::hidden();
    }
    // Si el comando falla y el spinner se descarta, se borra igualmente.
    let pb = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr())
        .with_finish(ProgressFinish::AndClear);
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg} {elapsed:.dim}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner())
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✓"]),
    );
    pb.set_message(msg.into());
    pb.enable_steady_tick(Duration::from_millis(90));
    pb
}

/// Ejecuta `f` mostrando un spinner con `msg`; lo limpia al terminar.
pub fn with_spinner<T>(msg: impl Into<String>, f: impl FnOnce() -> T) -> T {
    let pb = spinner(msg);
    let out = f();
    pb.finish_and_clear();
    out
}

/// Duración legible: `850ms`, `12.3s`, `2m05s`.
pub fn duration(ms: u128) -> String {
    match ms {
        0..=999 => format!("{ms}ms"),
        1_000..=59_999 => format!("{:.1}s", ms as f64 / 1000.0),
        _ => {
            let s = ms / 1000;
            format!("{}m{:02}s", s / 60, s % 60)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_durations() {
        assert_eq!(duration(850), "850ms");
        assert_eq!(duration(12_300), "12.3s");
        assert_eq!(duration(125_000), "2m05s");
    }
}
