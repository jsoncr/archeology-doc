//! Adaptadores de agentes, definidos en configuración (RF-042, RF-043).

use anyhow::{Context, Result};
use arqueology_core::config::{AdapterConfig, AdapterMode};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct AdapterStatus {
    pub name: String,
    pub mode: AdapterMode,
    pub available: bool,
    pub detail: String,
}

/// Comprueba si el comando del adaptador existe (`agent test`, RF-003).
pub fn check_adapter(name: &str, cfg: &AdapterConfig) -> AdapterStatus {
    match cfg.mode {
        AdapterMode::Export => AdapterStatus {
            name: name.to_string(),
            mode: cfg.mode,
            available: true,
            detail: "modo exportación (sin comando)".into(),
        },
        AdapterMode::Headless if !cfg.trusted => AdapterStatus {
            name: name.to_string(),
            mode: cfg.mode,
            available: false,
            // No se ejecuta ni `--version`: el comando viene del repositorio.
            detail: format!(
                "definido en el repositorio, sin aprobar (revísalo y ejecuta `adoc agent trust {name}`)"
            ),
        },
        AdapterMode::Headless => {
            let cmd = cfg.command.clone().unwrap_or_default();
            let found = responds_to_version(&cmd);
            AdapterStatus {
                name: name.to_string(),
                mode: cfg.mode,
                available: found,
                detail: if found {
                    format!("'{cmd}' disponible")
                } else {
                    format!("'{cmd}' no encontrado en PATH")
                },
            }
        }
    }
}

/// Límite para `<cmd> --version`: un CLI que pide algo por terminal o se
/// actualiza no debe bloquear `init`, `doctor` ni `agent list`.
const VERSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// ¿`<cmd> --version` termina bien antes del límite? Sin stdin ni salida.
fn responds_to_version(cmd: &str) -> bool {
    let child = Command::new(cmd)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return false;
    };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if start.elapsed() < VERSION_TIMEOUT => {
                std::thread::sleep(std::time::Duration::from_millis(25))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Detecta adaptadores headless instalados en el sistema (RF-003).
pub fn detect_installed(
    adapters: &std::collections::BTreeMap<String, AdapterConfig>,
) -> Vec<String> {
    adapters
        .iter()
        .filter(|(n, c)| check_adapter(n, c).available)
        .map(|(n, _)| n.clone())
        .collect()
}

/// Texto que recibe el agente: un encabezado y la tarea. El encabezado evita
/// que la tarea (que empieza con `---`) se interprete como una opción del CLI
/// cuando se pasa como argumento.
pub fn prompt_text(task_content: &str) -> String {
    format!(
        "Eres un agente que ejecuta una tarea de arqueology-doc en este repositorio. \
         Sigue la tarea al pie de la letra y escribe los archivos indicados en `# Produce`.\n\n\
         {task_content}"
    )
}

/// Sustituye los placeholders de los args del adaptador.
/// - `{prompt_file}` → ruta del archivo de la tarea.
/// - `{prompt_file_content}` → texto del prompt (encabezado + tarea).
pub fn expand_args(args: &[String], prompt_file: &Path, prompt_content: &str) -> Vec<String> {
    let text = prompt_text(prompt_content);
    args.iter()
        .map(|a| {
            a.replace("{prompt_file}", &prompt_file.display().to_string())
                .replace("{prompt_file_content}", &text)
        })
        .collect()
}

/// CLIs que aceptan `--model <nombre>`.
const MODEL_FLAG_CLIS: &[&str] = &[
    "claude", "codex", "devin", "gemini", "qwen", "opencode", "agent", "copilot", "aider",
];

/// Aplica `model`: sustituye `{model}` en los args o, en CLIs conocidos,
/// añade `--model <nombre>`.
pub fn with_model(cfg: &AdapterConfig, args: Vec<String>) -> Vec<String> {
    let Some(model) = cfg.model.as_deref().filter(|m| !m.is_empty()) else {
        // Sin modelo, `{model}` no debe llegar literal al comando.
        return args.into_iter().filter(|a| a != "{model}").collect();
    };
    if args.iter().any(|a| a.contains("{model}")) {
        return args.iter().map(|a| a.replace("{model}", model)).collect();
    }
    let cmd = cfg.command.as_deref().unwrap_or("");
    let name = Path::new(cmd)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut args = args;
    if MODEL_FLAG_CLIS.contains(&name.as_str()) && !args.iter().any(|a| a == "--model" || a == "-m")
    {
        // Antes de un `-` final (prompt por stdin en codex).
        let at = if args.last().map(|a| a == "-").unwrap_or(false) {
            args.len() - 1
        } else {
            args.len()
        };
        args.insert(at, model.to_string());
        args.insert(at, "--model".to_string());
    }
    args
}

/// Ejecuta un adaptador headless. Devuelve (código, duración_ms, salida combinada).
///
/// stdout y stderr se leen en hilos aparte mientras el proceso corre: si se
/// leyeran al final, un agente con mucha salida llenaría el pipe y quedaría
/// bloqueado hasta el timeout.
pub fn run_headless(
    cfg: &AdapterConfig,
    prompt_file: &Path,
    prompt_content: &str,
    workdir: &Path,
    timeout_secs: u64,
) -> Result<(Option<i32>, u128, String)> {
    use std::io::{Read, Write};

    if !cfg.trusted {
        anyhow::bail!(
            "adaptador sin aprobar: está definido en arqueology-doc.toml y no es uno predefinido \
             (revísalo y apruébalo con `adoc agent trust <nombre>`)"
        );
    }
    let command = cfg
        .command
        .clone()
        .context("adaptador headless sin `command`")?;
    let args = with_model(cfg, expand_args(&cfg.args, prompt_file, prompt_content));

    let mut cmd = Command::new(&command);
    cmd.args(&args).current_dir(workdir);
    for (k, v) in &cfg.env {
        cmd.env(k, v);
    }
    cmd.stdin(if cfg.stdin {
        std::process::Stdio::piped()
    } else {
        std::process::Stdio::null()
    });
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    // Grupo de procesos propio: al vencer el timeout se mata al agente y a
    // todo lo que lanzó (node, shells…). Si solo se matara al hijo, los nietos
    // mantendrían abiertos los pipes y la lectura no terminaría nunca.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let start = std::time::Instant::now();
    let mut child = cmd
        .spawn()
        .with_context(|| format!("no se pudo lanzar '{command}'"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let text = prompt_text(prompt_content);
        // Hilo aparte: escribir un prompt grande no debe bloquear la lectura.
        std::thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
    }
    // Se guarda hasta MAX_OUTPUT_BYTES por pipe; el resto se lee y descarta
    // para que el agente no se bloquee escribiendo.
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut truncated = false;
            if let Some(mut p) = pipe {
                let mut chunk = [0u8; 64 * 1024];
                loop {
                    match p.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let room = MAX_OUTPUT_BYTES.saturating_sub(buf.len());
                            buf.extend_from_slice(&chunk[..n.min(room)]);
                            truncated |= n > room;
                        }
                    }
                }
            }
            if truncated {
                buf.extend_from_slice(b"\n[salida truncada por arqueology-doc]\n");
            }
            buf
        })
    };
    let out_handle = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err_handle = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );

    // Timeout con polling; evita introducir tokio solo para esto.
    let deadline = start + std::time::Duration::from_secs(timeout_secs);
    let status = loop {
        match child.try_wait()? {
            Some(s) => break Some(s),
            None => {
                if std::time::Instant::now() > deadline {
                    kill_tree(&mut child);
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        }
    };
    let elapsed = start.elapsed().as_millis();
    let mut combined = String::from_utf8_lossy(&out_handle.join().unwrap_or_default()).to_string();
    combined.push_str(&String::from_utf8_lossy(
        &err_handle.join().unwrap_or_default(),
    ));
    let code = status.and_then(|s| s.code());
    Ok((code, elapsed, combined))
}

/// Salida máxima que se guarda por pipe (stdout y stderr) de un agente.
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

/// Mata al proceso y a su grupo (Unix). En Windows solo al proceso directo.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // `process_group(0)`: el id del grupo es el pid del hijo.
        if let Ok(pid) = i32::try_from(child.id()) {
            // SAFETY: `kill` no tiene precondiciones de memoria; un pid negativo
            // apunta al grupo creado para este hijo.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
}

/// Flags que dan permiso de escritura sin preguntar, por CLI conocido.
/// Sin ellos el agente se queda esperando aprobación y no escribe las notas.
const WRITE_FLAGS: &[(&str, &[&str])] = &[
    (
        "claude",
        &[
            "--permission-mode",
            "--allowedTools",
            "--allowed-tools",
            "--dangerously-skip-permissions",
        ],
    ),
    ("devin", &["--permission-mode"]),
    ("gemini", &["--approval-mode", "--yolo", "-y"]),
    ("qwen", &["--approval-mode", "--yolo", "-y"]),
    ("agent", &["--force", "-f", "--yolo"]),
    (
        "copilot",
        &["--allow-all-tools", "--allow-tool", "--allow-all"],
    ),
    ("amp", &["--dangerously-allow-all"]),
    ("aider", &["--yes-always"]),
    (
        "codex",
        &[
            "-s",
            "--sandbox",
            "--full-auto",
            "--dangerously-bypass-approvals-and-sandbox",
        ],
    ),
];

/// Flags que quitan al agente toda confirmación (shell incluido) o desactivan
/// protecciones del CLI. Con contenido no confiable en el repositorio, un
/// prompt injection en el código puede hacer que el agente ejecute comandos.
const UNRESTRICTED_FLAGS: &[&str] = &[
    "--dangerously-skip-permissions",
    "--dangerously-bypass-approvals-and-sandbox",
    "--dangerously-allow-all",
    "--allow-all-tools",
    "--allow-all",
    "--yes-always",
    "--yolo",
    "--force",
    "--respect-workspace-trust",
];

/// Flags sin restricciones presentes en el adaptador (para avisar).
pub fn unrestricted_flags(cfg: &AdapterConfig) -> Vec<String> {
    let mut found: Vec<String> = cfg
        .args
        .iter()
        .chain(&cfg.interactive_args)
        .filter(|a| {
            UNRESTRICTED_FLAGS
                .iter()
                .any(|f| *a == f || a.starts_with(&format!("{f}=")))
        })
        .cloned()
        .collect();
    found.dedup();
    found
}

/// Aviso para adaptadores que corren sin restricciones.
pub fn unrestricted_warning(name: &str, cfg: &AdapterConfig) -> Option<String> {
    let flags = unrestricted_flags(cfg);
    (!flags.is_empty()).then(|| {
        format!(
            "'{name}' corre sin confirmaciones ({}): puede ejecutar comandos de shell. Úsalo \
             solo con repositorios de confianza; texto malicioso en el código podría darle órdenes",
            flags.join(" ")
        )
    })
}

/// Problemas conocidos de configuración de un adaptador (para `doctor` y
/// la verificación previa de `inspect`).
pub fn adapter_warnings(name: &str, cfg: &AdapterConfig) -> Vec<String> {
    let mut out = Vec::new();
    if cfg.mode != AdapterMode::Headless {
        return out;
    }
    let cmd = cfg.command.as_deref().unwrap_or("");
    let bin = Path::new(cmd)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if let Some((_, flags)) = WRITE_FLAGS.iter().find(|(b, _)| *b == bin) {
        let has = cfg.args.iter().any(|a| {
            flags
                .iter()
                .any(|f| a == f || a.starts_with(&format!("{f}=")))
        });
        if !has {
            let suggested = crate::adapter::builtin_args(name, &bin);
            out.push(format!(
                "[agent.adapters.{name}] sin permiso de escritura ({}): el agente no podrá \
                 escribir las notas{}",
                flags.join(" | "),
                suggested
                    .map(|s| format!(" (sugerido: args = {s})"))
                    .unwrap_or_default()
            ));
        }
    }
    if !cfg.stdin && !cfg.args.iter().any(|a| a.contains("{prompt_file")) {
        out.push(format!(
            "[agent.adapters.{name}] no recibe la tarea: usa stdin = true o {{prompt_file}} en args"
        ));
    }
    out
}

/// Args predefinidos para un CLI conocido, como texto TOML.
pub fn builtin_args(name: &str, bin: &str) -> Option<String> {
    let builtin = arqueology_core::config::builtin_adapters();
    let adapter = builtin
        .get(name)
        .or_else(|| builtin.values().find(|a| a.command.as_deref() == Some(bin)))?;
    Some(format!(
        "[{}]{}",
        adapter
            .args
            .iter()
            .map(|a| format!("\"{a}\""))
            .collect::<Vec<_>>()
            .join(", "),
        if adapter.stdin { " y stdin = true" } else { "" }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_adapters_can_write_and_receive_the_task() {
        for (name, cfg) in arqueology_core::config::builtin_adapters() {
            assert!(
                adapter_warnings(&name, &cfg).is_empty(),
                "{name}: {:?}",
                adapter_warnings(&name, &cfg)
            );
            // Nunca la tarea como argumento suelto (empieza con `---`).
            let args = expand_args(&cfg.args, Path::new("t.md"), "---\nx\n");
            assert!(
                args.iter().all(|a| !a.starts_with("---")),
                "{name}: {args:?}"
            );
        }
    }

    #[test]
    fn model_is_added_for_known_clis() {
        let mut cfg = AdapterConfig::manual();
        cfg.mode = AdapterMode::Headless;
        cfg.command = Some("codex".into());
        cfg.model = Some("gpt-5".into());
        let args = with_model(&cfg, vec!["exec".into(), "-".into()]);
        assert_eq!(args, vec!["exec", "--model", "gpt-5", "-"]);
        cfg.command = Some("mi-agente".into());
        let args = with_model(&cfg, vec!["--modelo={model}".into()]);
        assert_eq!(args, vec!["--modelo=gpt-5"]);
    }

    #[test]
    fn prompt_never_starts_with_dash() {
        let args = expand_args(
            &["-p".into(), "{prompt_file_content}".into()],
            Path::new("t.md"),
            "---\ntask_id: T-001\n---\n",
        );
        assert!(!args[1].starts_with('-'));
        assert!(args[1].contains("task_id: T-001"));
    }

    #[test]
    fn untrusted_adapters_are_never_spawned() {
        let mut cfg = AdapterConfig::manual();
        cfg.mode = AdapterMode::Headless;
        cfg.command = Some("sh".into());
        cfg.args = vec!["-c".into(), "exit 0".into()];
        cfg.trusted = false;
        assert!(!check_adapter("x", &cfg).available);
        let err = run_headless(&cfg, Path::new("t.md"), "x", Path::new("."), 5).unwrap_err();
        assert!(err.to_string().contains("sin aprobar"));
    }

    /// Un nieto que hereda los pipes no deja colgada la ejecución tras el timeout.
    #[cfg(unix)]
    #[test]
    fn timeout_kills_grandchildren() {
        let mut cfg = AdapterConfig::manual();
        cfg.mode = AdapterMode::Headless;
        cfg.command = Some("sh".into());
        cfg.args = vec!["-c".into(), "sleep 30 & sleep 30".into()];
        let start = std::time::Instant::now();
        let (code, _, _) = run_headless(&cfg, Path::new("t.md"), "x", Path::new("."), 1).unwrap();
        assert_eq!(code, None);
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
    }

    #[cfg(unix)]
    #[test]
    fn sends_prompt_by_stdin_and_reads_large_output() {
        let cfg = AdapterConfig {
            command: Some("sh".into()),
            // Repite la entrada y genera >64 KiB de salida para probar el pipe.
            args: vec![
                "-c".into(),
                "cat; head -c 200000 /dev/zero | tr '\\0' x".into(),
            ],
            env: Default::default(),
            mode: AdapterMode::Headless,
            timeout_secs: None,
            stdin: true,
            model: None,
            interactive_args: vec![],
            trusted: true,
        };
        let (code, _, out) = run_headless(
            &cfg,
            Path::new("t.md"),
            "---\ntask_id: T-9\n",
            Path::new("."),
            20,
        )
        .unwrap();
        assert_eq!(code, Some(0));
        assert!(out.contains("task_id: T-9"));
        assert!(out.len() > 200_000);
    }
}
