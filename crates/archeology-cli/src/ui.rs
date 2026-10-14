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

// ---------------------------------------------------------------------------
// Estilo de la salida de texto (stdout)
// ---------------------------------------------------------------------------

/// ¿Colores y negrita en stdout? Solo en una terminal, sin `NO_COLOR`
/// (cualquier valor no vacío) ni `TERM=dumb`.
pub fn color_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::io::stdout().is_terminal()
            && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
            && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true)
    })
}

fn paint(code: &str, text: &str) -> String {
    if color_enabled() && !text.is_empty() {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

pub fn bold(text: &str) -> String {
    paint("1", text)
}
pub fn dim(text: &str) -> String {
    paint("2", text)
}
pub fn green(text: &str) -> String {
    paint("32", text)
}
pub fn red(text: &str) -> String {
    paint("31", text)
}
pub fn yellow(text: &str) -> String {
    paint("33", text)
}
pub fn cyan(text: &str) -> String {
    paint("36", text)
}

/// Rellena con espacios hasta `width` caracteres (se aplica antes de pintar:
/// los códigos ANSI no cuentan como ancho).
pub fn pad(text: &str, width: usize) -> String {
    let n = text.chars().count();
    if n >= width {
        text.to_string()
    } else {
        format!("{text}{}", " ".repeat(width - n))
    }
}

/// Recorta a `max` caracteres con `…`.
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Tamaño legible: `850 B`, `4.4 KB`, `12.0 MB`.
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64 / 1024.0;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    format!("{v:.1} {}", UNITS[unit])
}

/// Número con separador de miles: `12,345`.
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Ruta con `~` en lugar del directorio personal.
pub fn tilde(path: &std::path::Path) -> String {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    if let Some(home) = home.filter(|h| !h.is_empty()) {
        if let Ok(rest) = path.strip_prefix(std::path::Path::new(&home)) {
            return if rest.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", rest.display())
            };
        }
    }
    path.display().to_string()
}

/// Antigüedad compacta: `40s`, `5m`, `3h`, `2d`, `4mo`, `1y`.
pub fn age(seconds: i64) -> String {
    let s = seconds.max(0);
    match s {
        0..=59 => format!("{s}s"),
        60..=3_599 => format!("{}m", s / 60),
        3_600..=86_399 => format!("{}h", s / 3_600),
        86_400..=2_591_999 => format!("{}d", s / 86_400),
        2_592_000..=31_535_999 => format!("{}mo", s / 2_592_000),
        _ => format!("{}y", s / 31_536_000),
    }
}

/// Segundos desde una fecha RFC 3339 (None si no se puede leer).
pub fn seconds_since_rfc3339(date: &str) -> Option<i64> {
    let t = chrono::DateTime::parse_from_rfc3339(date.trim()).ok()?;
    Some((chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds())
}

/// Segundos desde la última modificación de un archivo.
pub fn seconds_since_modified(path: &std::path::Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let elapsed = std::time::SystemTime::now().duration_since(modified).ok()?;
    Some(elapsed.as_secs() as i64)
}

/// Barra de progreso `████░░░░` de `width` celdas.
pub fn bar(done: usize, total: usize, width: usize) -> String {
    let filled = (done * width).checked_div(total).unwrap_or(0).min(width);
    let full = "█".repeat(filled);
    let empty = "░".repeat(width - filled);
    format!("{}{}", green(&full), dim(&empty))
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

    #[test]
    fn formats_sizes_ages_and_padding() {
        assert_eq!(bytes(850), "850 B");
        assert_eq!(bytes(4555), "4.4 KB");
        assert_eq!(bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(thousands(12), "12");
        assert_eq!(age(30), "30s");
        assert_eq!(age(7_200), "2h");
        assert_eq!(age(3 * 86_400), "3d");
        assert_eq!(pad("ñu", 4), "ñu  ");
        assert_eq!(truncate("abcdef", 4), "abc…");
    }
}
