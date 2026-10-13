//! Interrupción de una corrida (Ctrl+C, SIGTERM).
//!
//! El agente corre en su propio grupo de procesos (`process_group(0)`), así
//! que el Ctrl+C de la terminal ya no le llega: sin manejo, `adoc` moriría y el
//! agente seguiría corriendo huérfano. Mientras hay una corrida activa:
//! - la primera señal solo marca la interrupción; el bucle de espera mata al
//!   grupo del agente, deja la tarea en un estado reanudable, escribe el log y
//!   termina con error;
//! - una segunda señal mata al grupo del agente y sale de inmediato.
//!
//! Sin corrida activa, la señal tiene su efecto por defecto.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

static ACTIVE: AtomicUsize = AtomicUsize::new(0);
static INTERRUPTED: AtomicBool = AtomicBool::new(false);
/// Grupo de procesos del agente en curso (0 = ninguno).
static CURRENT_PGID: AtomicI32 = AtomicI32::new(0);

/// Error que devuelve una corrida interrumpida por el usuario.
#[derive(Debug, Clone, Copy)]
pub struct Interrupted;

impl std::fmt::Display for Interrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&archeology_core::t!("agents.interrupt.interrupted"))
    }
}

impl std::error::Error for Interrupted {}

/// `true` si llegó una señal de interrupción durante la corrida activa.
pub fn requested() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

/// Mantiene activo el manejo de señales mientras vive.
pub struct Guard(());

impl Guard {
    pub fn new() -> Self {
        install();
        if ACTIVE.fetch_add(1, Ordering::SeqCst) == 0 {
            INTERRUPTED.store(false, Ordering::SeqCst);
        }
        Guard(())
    }
}

impl Default for Guard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        ACTIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Registra el grupo del agente en curso (para la segunda señal).
pub(crate) fn set_current_pgid(pgid: i32) {
    CURRENT_PGID.store(pgid, Ordering::SeqCst);
}

#[cfg(unix)]
fn install() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        for sig in [libc::SIGINT, libc::SIGTERM] {
            // SAFETY: `sigaction` con un manejador que solo usa operaciones
            // async-signal-safe (atómicos, `kill`, `signal`, `raise`).
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as usize;
                libc::sigemptyset(&mut action.sa_mask);
                action.sa_flags = libc::SA_RESTART;
                libc::sigaction(sig, &action, std::ptr::null_mut());
            }
        }
    });
}

#[cfg(unix)]
extern "C" fn on_signal(sig: libc::c_int) {
    if ACTIVE.load(Ordering::SeqCst) > 0 && !INTERRUPTED.swap(true, Ordering::SeqCst) {
        // Primera señal: el bucle de espera se encarga del resto.
        return;
    }
    let pgid = CURRENT_PGID.load(Ordering::SeqCst);
    // SAFETY: solo funciones async-signal-safe.
    unsafe {
        if pgid > 0 {
            libc::kill(-pgid, libc::SIGKILL);
        }
        // Efecto por defecto de la señal (terminar el proceso).
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

/// En Windows el Ctrl+C de la consola llega a todos sus procesos, agente
/// incluido; no hace falta un manejador para no dejarlo huérfano.
#[cfg(not(unix))]
fn install() {}
