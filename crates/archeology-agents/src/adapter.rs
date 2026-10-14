//! Adaptadores de agentes, definidos en configuración (RF-042, RF-043).

use anyhow::{Context, Result};
use archeology_core::config::{AdapterConfig, AdapterMode};
use archeology_core::i18n::Lang;
use archeology_core::t;
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
            detail: t!("agents.adapter.export_mode"),
        },
        AdapterMode::Headless if !cfg.trusted => AdapterStatus {
            name: name.to_string(),
            mode: cfg.mode,
            available: false,
            // No se ejecuta ni `--version`: el comando viene del repositorio.
            detail: t!("agents.adapter.untrusted", name = name),
        },
        AdapterMode::Headless => {
            let cmd = cfg.command.clone().unwrap_or_default();
            let found = responds_to_version(cfg);
            AdapterStatus {
                name: name.to_string(),
                mode: cfg.mode,
                available: found,
                detail: if found {
                    t!("agents.adapter.available", command = cmd)
                } else {
                    t!("agents.adapter.not_found", command = cmd)
                },
            }
        }
    }
}

/// Límite para `<cmd> --version`: un CLI que pide algo por terminal o se
/// actualiza no debe bloquear `init`, `doctor` ni `agent list`.
const VERSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// ¿`<cmd> --version` termina bien antes del límite? Sin stdin ni salida.
/// Con el entorno del adaptador (`env`), igual que en la ejecución real.
fn responds_to_version(cfg: &AdapterConfig) -> bool {
    let Some(cmd) = cfg.command.as_deref().filter(|c| !c.is_empty()) else {
        return false;
    };
    let mut command = Command::new(program_for(cfg, cmd));
    command.envs(&cfg.env);
    let child = command
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
/// cuando se pasa como argumento. Es contenido del proyecto: `lang` es el
/// idioma de la documentación (`Project::doc_lang`).
pub fn prompt_text_in(lang: Lang, task_content: &str) -> String {
    format!(
        "{}\n\n{task_content}",
        archeology_core::tl!(lang, "doc.agent_prompt.header")
    )
}

/// [`prompt_text_in`] con el idioma de la interfaz (sin proyecto a mano).
pub fn prompt_text(task_content: &str) -> String {
    prompt_text_in(archeology_core::i18n::ui_lang(), task_content)
}

/// Idioma de la documentación del proyecto en `workdir` (`[project]
/// language` de `archeology-doc.toml`); si no se puede leer, el de la interfaz.
pub fn project_lang(workdir: &Path) -> Lang {
    archeology_core::config::ArcheologyConfig::load(
        &workdir.join(archeology_core::config::CONFIG_FILE),
    )
    .map(|c| c.project.lang())
    .unwrap_or_else(|_| archeology_core::i18n::ui_lang())
}

/// Sustituye los placeholders de los args del adaptador.
/// - `{prompt_file}` → ruta del archivo de la tarea.
/// - `{prompt_file_content}` → texto del prompt (encabezado + tarea).
///
/// El encabezado usa el idioma de la interfaz; con un proyecto, mejor
/// [`expand_args_in`] con `project.doc_lang()`.
pub fn expand_args(args: &[String], prompt_file: &Path, prompt_content: &str) -> Vec<String> {
    expand_args_in(
        archeology_core::i18n::ui_lang(),
        args,
        prompt_file,
        prompt_content,
    )
}

/// [`expand_args`] con el idioma del encabezado del prompt explícito.
pub fn expand_args_in(
    lang: Lang,
    args: &[String],
    prompt_file: &Path,
    prompt_content: &str,
) -> Vec<String> {
    let text = prompt_text_in(lang, prompt_content);
    args.iter()
        .map(|a| {
            a.replace("{prompt_file}", &prompt_file.display().to_string())
                .replace("{prompt_file_content}", &text)
        })
        .collect()
}

/// Programa a lanzar para el comando del adaptador (en Windows, resuelve
/// los shims `.cmd` de npm con `PATH` + `PATHEXT`).
pub fn program_for(cfg: &AdapterConfig, cmd: &str) -> std::path::PathBuf {
    crate::command::resolve_program(cmd, cfg.env.get("PATH").map(String::as_str))
}

/// ¿Está el comando del adaptador en el `PATH`? No lo ejecuta.
pub fn command_found(cfg: &AdapterConfig) -> bool {
    cfg.command.as_deref().is_some_and(|c| {
        crate::command::find_program(c, cfg.env.get("PATH").map(String::as_str)).is_some()
    })
}

/// Cómo recibe el modelo un CLI conocido.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelPassing {
    /// Opción `<flag> <modelo>`, insertada antes del prompt (el argumento
    /// posicional o la opción que lo recibe), para que ningún CLI la tome
    /// como parte del mensaje.
    Flag(&'static str),
    /// Variables de entorno de goose: `GOOSE_MODEL` (y `GOOSE_PROVIDER` si
    /// el modelo viene como `proveedor/modelo`).
    GooseEnv,
    /// El CLI no permite elegir el modelo por línea de comandos.
    Unsupported,
}

/// Formato del nombre del modelo que espera un CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFormat {
    /// Nombre o alias (`opus`, `gpt-5`).
    Name,
    /// Siempre `proveedor/modelo` (`anthropic/claude-sonnet-4-5`).
    ProviderModel,
    /// Nombre, con proveedor opcional (`sonnet`, `anthropic/claude-…`).
    OptionalProvider,
}

/// Cómo se elige el modelo en un CLI conocido.
#[derive(Debug, Clone, Copy)]
pub struct ModelSpec {
    /// Ejecutable (sin extensión, en minúsculas).
    pub program: &'static str,
    pub passing: ModelPassing,
    pub format: ModelFormat,
    /// Valores de ejemplo para los mensajes y la documentación.
    pub examples: &'static [&'static str],
    /// Subcomando del CLI que lista los modelos disponibles (`adoc agent models`).
    pub list_args: Option<&'static [&'static str]>,
}

/// Tabla de CLIs conocidos: cómo reciben el modelo (`adoc agent use X --model Y`).
pub const MODEL_SPECS: &[ModelSpec] = &[
    ModelSpec {
        program: "claude",
        passing: ModelPassing::Flag("--model"),
        format: ModelFormat::Name,
        examples: &["sonnet", "opus", "claude-sonnet-4-5"],
        list_args: None,
    },
    ModelSpec {
        program: "codex",
        passing: ModelPassing::Flag("--model"),
        format: ModelFormat::Name,
        examples: &["gpt-5", "gpt-5-codex", "o3"],
        list_args: None,
    },
    ModelSpec {
        program: "devin",
        passing: ModelPassing::Flag("--model"),
        format: ModelFormat::Name,
        examples: &["opus", "claude-sonnet-4", "codex"],
        list_args: Some(&["models", "list"]),
    },
    ModelSpec {
        program: "gemini",
        passing: ModelPassing::Flag("--model"),
        format: ModelFormat::Name,
        examples: &["gemini-2.5-pro", "gemini-2.5-flash"],
        list_args: None,
    },
    ModelSpec {
        program: "qwen",
        passing: ModelPassing::Flag("--model"),
        format: ModelFormat::Name,
        examples: &["qwen3-coder-plus"],
        list_args: None,
    },
    ModelSpec {
        program: "opencode",
        passing: ModelPassing::Flag("--model"),
        format: ModelFormat::ProviderModel,
        examples: &[
            "anthropic/claude-sonnet-4-5",
            "openai/gpt-5",
            "opencode/grok-code",
        ],
        list_args: Some(&["models"]),
    },
    // Cursor CLI (ejecutable `agent`).
    ModelSpec {
        program: "agent",
        passing: ModelPassing::Flag("--model"),
        format: ModelFormat::Name,
        examples: &["sonnet-4.5", "gpt-5", "auto"],
        list_args: None,
    },
    ModelSpec {
        program: "copilot",
        passing: ModelPassing::Flag("--model"),
        format: ModelFormat::Name,
        examples: &["claude-sonnet-4.5", "gpt-5"],
        list_args: None,
    },
    ModelSpec {
        program: "aider",
        passing: ModelPassing::Flag("--model"),
        format: ModelFormat::OptionalProvider,
        examples: &["sonnet", "gpt-4o", "anthropic/claude-sonnet-4-5"],
        list_args: None,
    },
    ModelSpec {
        program: "goose",
        passing: ModelPassing::GooseEnv,
        format: ModelFormat::OptionalProvider,
        examples: &["anthropic/claude-sonnet-4-5", "openai/gpt-4o"],
        list_args: None,
    },
    ModelSpec {
        program: "amp",
        passing: ModelPassing::Unsupported,
        format: ModelFormat::Name,
        examples: &[],
        list_args: None,
    },
];

/// Especificación de modelo del CLI de un adaptador, si es conocido.
pub fn model_spec(cfg: &AdapterConfig) -> Option<&'static ModelSpec> {
    let program = crate::command::program_stem(cfg.command.as_deref().unwrap_or(""));
    MODEL_SPECS.iter().find(|s| s.program == program)
}

/// Un modelo válido: solo `[A-Za-z0-9._:/@+-]`, sin `-` inicial (no puede
/// pasar por una opción del CLI) y no vacío.
pub fn validate_model(model: &str) -> Result<()> {
    let ok = !model.is_empty()
        && model.len() <= 200
        && !model.starts_with('-')
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:/@+-".contains(c));
    if ok {
        return Ok(());
    }
    if model.trim() != model || model.trim().contains(char::is_whitespace) {
        // Lo habitual: el nombre visible ("Claude Sonnet 4.5") en vez del id.
        anyhow::bail!(t!("agents.model.has_spaces", model = format!("{model:?}")));
    }
    anyhow::bail!(t!(
        "agents.adapter.invalid_model",
        model = format!("{model:?}")
    ));
}

/// Ejemplos de modelo para un CLI: ` (p. ej. a, b)` o vacío.
fn examples_suffix(spec: &ModelSpec) -> String {
    if spec.examples.is_empty() {
        return String::new();
    }
    t!(
        "agents.model.examples_suffix",
        examples = spec.examples.join(", ")
    )
}

/// Cómo listar los modelos de un CLI: ` Lista los disponibles con `x y`.` o vacío.
fn list_suffix(spec: &ModelSpec) -> String {
    spec.list_args
        .map(|a| {
            t!(
                "agents.model.list_suffix",
                command = format!("{} {}", spec.program, a.join(" "))
            )
        })
        .unwrap_or_default()
}

/// Valida el modelo con el formato que espera el CLI (`proveedor/modelo` en
/// opencode) y comprueba que el CLI permita elegirlo.
pub fn validate_model_for(spec: &ModelSpec, model: &str) -> Result<()> {
    validate_model(model)
        .map_err(|e| anyhow::anyhow!("{e}{}{}", examples_suffix(spec), list_suffix(spec)))?;
    match spec.passing {
        ModelPassing::Unsupported => {
            anyhow::bail!(t!("agents.model.unsupported", agent = spec.program))
        }
        ModelPassing::Flag(_) | ModelPassing::GooseEnv => {}
    }
    if spec.format == ModelFormat::ProviderModel {
        let ok = model
            .split_once('/')
            .is_some_and(|(p, m)| !p.is_empty() && !m.is_empty());
        if !ok {
            anyhow::bail!(t!(
                "agents.model.needs_provider",
                agent = spec.program,
                model = format!("{model:?}"),
                examples = spec.examples.join(", "),
                list = list_suffix(spec)
            ));
        }
    }
    Ok(())
}

/// Argumentos (plantillas, antes de expandir los placeholders) y variables
/// de entorno con el modelo aplicado.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelArgs {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// ¿Es `arg` una opción que toma el siguiente argumento como valor?
fn takes_value(arg: &str) -> bool {
    arg.len() > 1 && arg.starts_with('-') && !arg.contains('=')
}

/// Dónde insertar `--model X`: antes del prompt (placeholder o `-` de stdin)
/// o de la opción que lo recibe (`-p {prompt_file_content}`, `--prompt-file
/// {prompt_file}`); sin prompt en los args, al final.
fn model_insert_index(args: &[String]) -> usize {
    let is_prompt = |a: &String| a.contains("{prompt_file") || a == "-";
    match args.iter().position(is_prompt) {
        Some(i) if i > 0 && takes_value(&args[i - 1]) => i - 1,
        Some(i) => i,
        None => args.len(),
    }
}

/// Aplica `model` a las plantillas de args del adaptador (antes de expandir
/// `{prompt_file…}`): sustituye `{model}` o, en CLIs conocidos, usa su forma
/// de recibir el modelo ([`MODEL_SPECS`]). El modelo se valida antes.
///
/// Error si el CLI no permite elegir el modelo o si el adaptador no es de un
/// CLI conocido y sus args no tienen `{model}`: el modelo no se ignora en silencio.
pub fn apply_model(cfg: &AdapterConfig, templates: &[String]) -> Result<ModelArgs> {
    let Some(model) = cfg.model.as_deref().filter(|m| !m.is_empty()) else {
        // Sin modelo, `{model}` no debe llegar literal al comando.
        return Ok(ModelArgs {
            args: templates
                .iter()
                .filter(|a| *a != "{model}")
                .cloned()
                .collect(),
            env: Vec::new(),
        });
    };
    if templates.iter().any(|a| a.contains("{model}")) {
        validate_model(model)?;
        return Ok(ModelArgs {
            args: templates
                .iter()
                .map(|a| a.replace("{model}", model))
                .collect(),
            env: Vec::new(),
        });
    }
    if cfg.mode == AdapterMode::Export {
        anyhow::bail!(t!("agents.model.export_mode"));
    }
    let Some(spec) = model_spec(cfg) else {
        validate_model(model)?;
        anyhow::bail!(t!(
            "agents.model.no_placeholder",
            program = cfg.command.as_deref().unwrap_or("")
        ));
    };
    validate_model_for(spec, model)?;
    let mut args = templates.to_vec();
    let mut env = Vec::new();
    match spec.passing {
        ModelPassing::Flag(flag) => {
            // El usuario ya puso el flag en sus args: se respeta.
            let present = args
                .iter()
                .any(|a| a == flag || a == "-m" || a.starts_with(&format!("{flag}=")));
            if !present {
                let at = model_insert_index(&args);
                args.insert(at, model.to_string());
                args.insert(at, flag.to_string());
            }
        }
        ModelPassing::GooseEnv => match model.split_once('/') {
            Some((provider, name)) if !provider.is_empty() && !name.is_empty() => {
                env.push(("GOOSE_PROVIDER".into(), provider.into()));
                env.push(("GOOSE_MODEL".into(), name.into()));
            }
            _ => env.push(("GOOSE_MODEL".into(), model.into())),
        },
        ModelPassing::Unsupported => unreachable!("validate_model_for lo rechaza"),
    }
    Ok(ModelArgs { args, env })
}

/// Comando efectivo de un adaptador, legible: con el modelo aplicado y el
/// prompt abreviado (`opencode run --model anthropic/x "<prompt>"`).
pub fn command_preview(cfg: &AdapterConfig, interactive: bool) -> Result<Option<String>> {
    let Some(cmd) = cfg.command.as_deref().filter(|c| !c.is_empty()) else {
        return Ok(None);
    };
    let templates = if interactive {
        &cfg.interactive_args
    } else {
        &cfg.args
    };
    let applied = apply_model(cfg, templates)?;
    let quote = |a: &str| {
        if a.is_empty() || a.contains(char::is_whitespace) || a.contains('"') {
            format!("{a:?}")
        } else {
            a.to_string()
        }
    };
    let mut parts: Vec<String> = applied
        .env
        .iter()
        .map(|(k, v)| format!("{k}={}", quote(v)))
        .collect();
    parts.push(quote(cmd));
    for a in &applied.args {
        let shown = a
            .replace("{prompt_file_content}", "<prompt>")
            .replace("{prompt_file}", "<task.md>");
        parts.push(if shown == "<prompt>" || shown == "<task.md>" {
            shown
        } else {
            quote(&shown)
        });
    }
    if cfg.stdin && !interactive {
        parts.push("< <prompt>".into());
    }
    Ok(Some(parts.join(" ")))
}

/// Resultado de una ejecución headless.
#[derive(Debug, Clone, Default)]
pub struct HeadlessOutcome {
    /// Código de salida (None si murió por señal, por timeout o interrupción).
    pub code: Option<i32>,
    /// Señal que terminó al agente (Unix), si no fue por timeout.
    pub signal: Option<i32>,
    /// Se mató al agente por superar el timeout.
    pub timed_out: bool,
    /// Se mató al agente por Ctrl+C/SIGTERM del usuario.
    pub interrupted: bool,
    pub duration_ms: u128,
    /// stdout + stderr (truncados a `MAX_OUTPUT_BYTES` cada uno).
    pub output: String,
}

impl HeadlessOutcome {
    /// Terminó con código 0.
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// Motivo legible de un fallo de ejecución (no de validación).
    pub fn failure_reason(&self) -> String {
        if self.interrupted {
            t!("agents.adapter.reason_interrupted")
        } else if self.timed_out {
            t!("agents.adapter.reason_timeout", ms = self.duration_ms)
        } else if let Some(c) = self.code {
            t!("agents.adapter.reason_exit_code", code = c)
        } else if let Some(sig) = self.signal {
            t!("agents.adapter.reason_signal", signal = sig)
        } else {
            t!("agents.adapter.reason_no_exit_code")
        }
    }
}

/// Timeout máximo de un agente (7 días): evita desbordar `Instant + Duration`.
pub const MAX_TIMEOUT_SECS: u64 = 7 * 24 * 3600;
/// Espera entre SIGTERM y SIGKILL al matar el grupo del agente.
#[cfg(unix)]
const KILL_GRACE: std::time::Duration = std::time::Duration::from_secs(2);
/// Espera máxima a los hilos lectores tras terminar el agente: si un nieto
/// que escapó del grupo conserva los pipes, se devuelve lo leído hasta ahí.
const READER_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

/// Ejecuta un adaptador headless.
///
/// stdout y stderr se leen en hilos aparte mientras el proceso corre: si se
/// leyeran al final, un agente con mucha salida llenaría el pipe y quedaría
/// bloqueado hasta el timeout. Al terminar (o al vencer el timeout, o ante
/// Ctrl+C) se mata todo el grupo del agente y los lectores se esperan con
/// un límite: un nieto que conserve los pipes no cuelga la ejecución.
pub fn run_headless(
    cfg: &AdapterConfig,
    prompt_file: &Path,
    prompt_content: &str,
    workdir: &Path,
    timeout_secs: u64,
) -> Result<HeadlessOutcome> {
    use std::io::Write;

    if !cfg.trusted {
        anyhow::bail!(t!("agents.adapter.not_approved"));
    }
    let command = cfg
        .command
        .clone()
        .with_context(|| t!("agents.adapter.missing_command"))?;
    // El encabezado del prompt va en el idioma de la documentación del proyecto.
    let lang = project_lang(workdir);
    let model = apply_model(cfg, &cfg.args)?;
    let args = expand_args_in(lang, &model.args, prompt_file, prompt_content);

    let mut cmd = Command::new(program_for(cfg, &command));
    cmd.args(&args).current_dir(workdir);
    for (k, v) in &cfg.env {
        cmd.env(k, v);
    }
    // El modelo por entorno (goose) tiene prioridad sobre `env` del adaptador.
    for (k, v) in &model.env {
        cmd.env(k, v);
    }
    cmd.stdin(if cfg.stdin {
        std::process::Stdio::piped()
    } else {
        std::process::Stdio::null()
    });
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    // Grupo de procesos propio: al terminar se mata al agente y a todo lo que
    // lanzó (node, shells…). Si solo se matara al hijo, los nietos mantendrían
    // abiertos los pipes y la lectura no terminaría nunca.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let _guard = crate::interrupt::Guard::new();
    let start = std::time::Instant::now();
    let mut child = cmd
        .spawn()
        .with_context(|| t!("agents.adapter.spawn_failed", command = command))?;
    if let Ok(pid) = i32::try_from(child.id()) {
        crate::interrupt::set_current_pgid(pid);
    }

    if let Some(mut stdin) = child.stdin.take() {
        let text = prompt_text_in(lang, prompt_content);
        // Hilo aparte: escribir un prompt grande no debe bloquear la lectura.
        std::thread::spawn(move || {
            let _ = stdin.write_all(text.as_bytes());
        });
    }
    let out_reader = spawn_reader(child.stdout.take().map(|p| Box::new(p) as BoxedPipe));
    let err_reader = spawn_reader(child.stderr.take().map(|p| Box::new(p) as BoxedPipe));

    // Timeout con polling; evita introducir tokio solo para esto.
    let timeout = std::time::Duration::from_secs(timeout_secs.min(MAX_TIMEOUT_SECS));
    let deadline = start.checked_add(timeout);
    let mut timed_out = false;
    let mut interrupted = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {
                if crate::interrupt::requested() {
                    interrupted = true;
                } else if deadline.is_some_and(|d| std::time::Instant::now() > d) {
                    timed_out = true;
                }
                if interrupted || timed_out {
                    kill_tree(&mut child);
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                // No dejar al agente corriendo sin nadie que lo espere.
                kill_tree(&mut child);
                let _ = child.wait();
                crate::interrupt::set_current_pgid(0);
                return Err(e).with_context(|| t!("agents.adapter.wait_failed"));
            }
        }
    };
    // El agente terminó: lo que quede de su grupo (procesos en segundo plano)
    // también, para que suelten los pipes.
    kill_group(&child);
    crate::interrupt::set_current_pgid(0);
    let elapsed = start.elapsed().as_millis();
    let reader_deadline = std::time::Instant::now() + READER_GRACE;
    let mut combined = String::from_utf8_lossy(&out_reader.collect(reader_deadline)).to_string();
    combined.push_str(&String::from_utf8_lossy(
        &err_reader.collect(reader_deadline),
    ));
    let code = status.and_then(|s| s.code());
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.and_then(|s| s.signal())
    };
    #[cfg(not(unix))]
    let signal = None;
    Ok(HeadlessOutcome {
        code,
        signal,
        timed_out,
        interrupted,
        duration_ms: elapsed,
        output: combined,
    })
}

type BoxedPipe = Box<dyn std::io::Read + Send>;

/// Lector de un pipe en un hilo: guarda lo leído en un búfer compartido para
/// poder devolverlo aunque el hilo siga bloqueado.
struct PipeReader {
    buf: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    done: std::sync::mpsc::Receiver<()>,
}

impl PipeReader {
    /// Lo leído hasta que el pipe se cierre o venza `deadline`.
    fn collect(self, deadline: std::time::Instant) -> Vec<u8> {
        let wait = deadline.saturating_duration_since(std::time::Instant::now());
        let finished = self.done.recv_timeout(wait).is_ok();
        let mut buf = self
            .buf
            .lock()
            .map(|mut b| std::mem::take(&mut *b))
            .unwrap_or_default();
        if !finished {
            buf.extend_from_slice(
                format!("\n{}\n", t!("agents.adapter.output_still_open")).as_bytes(),
            );
        }
        buf
    }
}

/// Se guarda hasta MAX_OUTPUT_BYTES por pipe; el resto se lee y descarta
/// para que el agente no se bloquee escribiendo.
fn spawn_reader(pipe: Option<BoxedPipe>) -> PipeReader {
    let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (tx, done) = std::sync::mpsc::channel();
    let shared = buf.clone();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut truncated = false;
        if let Some(mut p) = pipe {
            let mut chunk = [0u8; 64 * 1024];
            loop {
                match p.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let Ok(mut b) = shared.lock() else { break };
                        let room = MAX_OUTPUT_BYTES.saturating_sub(b.len());
                        b.extend_from_slice(&chunk[..n.min(room)]);
                        truncated |= n > room;
                    }
                }
            }
        }
        if truncated {
            if let Ok(mut b) = shared.lock() {
                b.extend_from_slice(
                    format!("\n{}\n", t!("agents.adapter.output_truncated")).as_bytes(),
                );
            }
        }
        let _ = tx.send(());
    });
    PipeReader { buf, done }
}

/// Salida máxima que se guarda por pipe (stdout y stderr) de un agente.
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

/// Mata al proceso y a todo su árbol: en Unix, su grupo (SIGTERM y, tras una
/// espera, SIGKILL); en Windows, `taskkill /T /F` sobre el árbol del proceso.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    kill_group(child);
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    let _ = child.kill();
}

/// Termina lo que quede del grupo de procesos del agente (Unix): SIGTERM,
/// espera hasta `KILL_GRACE` y SIGKILL. Sin procesos en el grupo, no espera.
/// En Windows, el árbol se mata con `taskkill` en [`kill_tree`].
fn kill_group(child: &std::process::Child) {
    #[cfg(unix)]
    {
        // `process_group(0)`: el id del grupo es el pid del hijo.
        let Ok(pgid) = i32::try_from(child.id()) else {
            return;
        };
        // SAFETY: `kill` no tiene precondiciones de memoria; un pid negativo
        // apunta al grupo creado para este hijo. Con señal 0 solo comprueba
        // si queda algún proceso en el grupo.
        let alive = || unsafe { libc::kill(-pgid, 0) == 0 };
        if !alive() {
            return;
        }
        unsafe {
            libc::kill(-pgid, libc::SIGTERM);
        }
        let until = std::time::Instant::now() + KILL_GRACE;
        while alive() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if alive() {
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
        }
    }
    #[cfg(not(unix))]
    let _ = child;
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
        t!(
            "agents.adapter.unrestricted",
            name = name,
            flags = flags.join(" ")
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
    let bin = crate::command::program_stem(cfg.command.as_deref().unwrap_or(""));
    if let Some((_, flags)) = WRITE_FLAGS.iter().find(|(b, _)| *b == bin) {
        let has = cfg.args.iter().any(|a| {
            flags
                .iter()
                .any(|f| a == f || a.starts_with(&format!("{f}=")))
        });
        if !has {
            let suggested = crate::adapter::builtin_args(name, &bin);
            out.push(t!(
                "agents.adapter.no_write_permission",
                name = name,
                flags = flags.join(" | "),
                suggested = suggested
                    .map(|s| t!("agents.adapter.suggested_args", args = s))
                    .unwrap_or_default()
            ));
        }
    }
    if !cfg.stdin && !cfg.args.iter().any(|a| a.contains("{prompt_file")) {
        out.push(t!("agents.adapter.no_task_input", name = name));
    }
    out
}

/// Args predefinidos para un CLI conocido, como texto TOML.
pub fn builtin_args(name: &str, bin: &str) -> Option<String> {
    let builtin = archeology_core::config::builtin_adapters();
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
        if adapter.stdin {
            t!("agents.adapter.and_stdin")
        } else {
            String::new()
        }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_header_follows_the_given_language() {
        assert!(prompt_text_in(Lang::En, "x").starts_with("You are an agent"));
        assert!(prompt_text_in(Lang::Es, "x").starts_with("Eres un agente"));
        assert!(prompt_text_in(Lang::En, "---\nx").ends_with("\n\n---\nx"));
    }

    #[test]
    fn builtin_adapters_can_write_and_receive_the_task() {
        for (name, cfg) in archeology_core::config::builtin_adapters() {
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

    fn headless(command: &str, model: &str) -> AdapterConfig {
        let mut cfg = AdapterConfig::manual();
        cfg.mode = AdapterMode::Headless;
        cfg.command = Some(command.into());
        cfg.model = Some(model.into());
        cfg
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn model_is_added_for_known_clis() {
        let cfg = headless("codex", "gpt-5");
        let m = apply_model(&cfg, &strings(&["exec", "-s", "workspace-write", "-"])).unwrap();
        assert_eq!(
            m.args,
            strings(&["exec", "-s", "workspace-write", "--model", "gpt-5", "-"])
        );
        // Shim de npm en Windows: `codex.CMD` también es codex.
        let cfg = headless("C:\\npm\\codex.CMD", "gpt-5");
        let m = apply_model(&cfg, &strings(&["exec"])).unwrap();
        assert_eq!(m.args, strings(&["exec", "--model", "gpt-5"]));
        let cfg = headless("mi-agente", "gpt-5");
        let m = apply_model(&cfg, &strings(&["--modelo={model}"])).unwrap();
        assert_eq!(m.args, strings(&["--modelo=gpt-5"]));
    }

    /// Cada CLI predefinido recibe el modelo en el lugar correcto: antes del
    /// prompt (posicional o la opción que lo recibe), nunca detrás.
    #[test]
    fn model_position_per_builtin_adapter() {
        let builtin = archeology_core::config::builtin_adapters();
        let cases: &[(&str, &str, &[&str])] = &[
            (
                "claude",
                "opus",
                &["-p", "--permission-mode", "acceptEdits", "--model", "opus"],
            ),
            (
                "codex",
                "gpt-5",
                &["exec", "-s", "workspace-write", "--model", "gpt-5", "-"],
            ),
            (
                "devin",
                "opus",
                &[
                    "-p",
                    "--permission-mode",
                    "smart",
                    "--respect-workspace-trust",
                    "false",
                    "--model",
                    "opus",
                    "--prompt-file",
                    "{prompt_file}",
                ],
            ),
            (
                "gemini",
                "gemini-2.5-pro",
                &["--approval-mode", "auto_edit", "--model", "gemini-2.5-pro"],
            ),
            (
                "opencode",
                "anthropic/claude-sonnet-4-5",
                &[
                    "run",
                    "--model",
                    "anthropic/claude-sonnet-4-5",
                    "{prompt_file_content}",
                ],
            ),
            (
                "cursor",
                "gpt-5",
                &[
                    "-p",
                    "--force",
                    "--output-format",
                    "text",
                    "--model",
                    "gpt-5",
                    "{prompt_file_content}",
                ],
            ),
            (
                "copilot",
                "gpt-5",
                &[
                    "--model",
                    "gpt-5",
                    "-p",
                    "{prompt_file_content}",
                    "--allow-all-tools",
                ],
            ),
            (
                "aider",
                "sonnet",
                &[
                    "--model",
                    "sonnet",
                    "--message-file",
                    "{prompt_file}",
                    "--yes-always",
                    "--no-auto-commits",
                ],
            ),
        ];
        for (name, model, expected) in cases {
            let mut cfg = builtin[*name].clone();
            cfg.model = Some(model.to_string());
            let m = apply_model(&cfg, &cfg.args).unwrap();
            assert_eq!(m.args, strings(expected), "{name}");
            assert!(m.env.is_empty(), "{name}");
        }
        // Interactivo: también antes del prompt (`-i <prompt>` en gemini).
        let mut cfg = builtin["gemini"].clone();
        cfg.model = Some("gemini-2.5-pro".into());
        let m = apply_model(&cfg, &cfg.interactive_args).unwrap();
        assert_eq!(
            m.args,
            strings(&[
                "--approval-mode",
                "auto_edit",
                "--model",
                "gemini-2.5-pro",
                "-i",
                "{prompt_file_content}"
            ])
        );
        // goose: por entorno, con proveedor opcional.
        let mut cfg = builtin["goose"].clone();
        cfg.model = Some("anthropic/claude-sonnet-4-5".into());
        let m = apply_model(&cfg, &cfg.args).unwrap();
        assert_eq!(m.args, cfg.args);
        assert_eq!(
            m.env,
            vec![
                ("GOOSE_PROVIDER".to_string(), "anthropic".to_string()),
                ("GOOSE_MODEL".to_string(), "claude-sonnet-4-5".to_string())
            ]
        );
    }

    /// opencode exige `proveedor/modelo` y lo explica con ejemplos.
    #[test]
    fn opencode_requires_provider_slash_model() {
        archeology_core::i18n::set_ui_lang(Lang::Es);
        let mut cfg = archeology_core::config::builtin_adapters()["opencode"].clone();
        for bad in ["claude-sonnet-4-5", "anthropic/", "/gpt-5"] {
            cfg.model = Some(bad.into());
            let e = apply_model(&cfg, &cfg.args).unwrap_err().to_string();
            assert!(e.contains("proveedor/modelo"), "{e}");
            assert!(e.contains("anthropic/claude-sonnet-4-5"), "{e}");
            assert!(e.contains("opencode models"), "{e}");
        }
        cfg.model = Some("Claude Sonnet 4.5".into());
        let e = apply_model(&cfg, &cfg.args).unwrap_err().to_string();
        assert!(e.contains("espacios"), "{e}");
        assert!(e.contains("opencode models"), "{e}");
    }

    #[test]
    fn unsupported_and_unknown_clis_reject_the_model() {
        archeology_core::i18n::set_ui_lang(Lang::Es);
        let mut cfg = archeology_core::config::builtin_adapters()["amp"].clone();
        cfg.model = Some("gpt-5".into());
        let e = apply_model(&cfg, &cfg.args).unwrap_err().to_string();
        assert!(e.contains("amp") && e.contains("--no-model"), "{e}");
        // CLI desconocido sin `{model}`: no se ignora en silencio.
        let cfg = headless("mi-agente", "x");
        let e = apply_model(&cfg, &strings(&["run"]))
            .unwrap_err()
            .to_string();
        assert!(e.contains("{model}"), "{e}");
        // Sin modelo, `{model}` suelto desaparece.
        let mut cfg = headless("mi-agente", "x");
        cfg.model = None;
        let m = apply_model(&cfg, &strings(&["--model", "{model}", "run"])).unwrap();
        assert_eq!(m.args, strings(&["--model", "run"]));
    }

    #[test]
    fn preview_shows_the_model_flag() {
        let mut cfg = archeology_core::config::builtin_adapters()["opencode"].clone();
        cfg.model = Some("anthropic/claude-sonnet-4-5".into());
        assert_eq!(
            command_preview(&cfg, false).unwrap().unwrap(),
            "opencode run --model anthropic/claude-sonnet-4-5 <prompt>"
        );
        let mut cfg = archeology_core::config::builtin_adapters()["claude"].clone();
        cfg.model = Some("opus".into());
        assert_eq!(
            command_preview(&cfg, false).unwrap().unwrap(),
            "claude -p --permission-mode acceptEdits --model opus < <prompt>"
        );
    }

    #[test]
    fn model_is_validated_before_substitution() {
        let mut cfg = headless("mi-agente", "");
        for ok in [
            "opus",
            "gpt-5.1",
            "anthropic/claude-sonnet-4@2025",
            "x:y+z_w",
        ] {
            cfg.model = Some(ok.into());
            assert!(
                apply_model(&cfg, &strings(&["--m={model}"])).is_ok(),
                "{ok}"
            );
        }
        for bad in ["--yolo", "a b", "a;rm -rf", "$(id)", "a\nb", "`x`", "a'b"] {
            cfg.model = Some(bad.into());
            assert!(
                apply_model(&cfg, &strings(&["--m={model}"])).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn claude_cmd_shim_gets_write_flag_warning() {
        // Los textos se comprueban en español (idioma fijado para el test).
        archeology_core::i18n::set_ui_lang(Lang::Es);
        let mut cfg = AdapterConfig::manual();
        cfg.mode = AdapterMode::Headless;
        cfg.command = Some("Claude.cmd".into());
        cfg.stdin = true;
        let w = adapter_warnings("claude", &cfg);
        assert!(
            w.iter().any(|w| w.contains("sin permiso de escritura")),
            "{w:?}"
        );
    }

    /// `--version` se prueba con el `env` del adaptador.
    #[cfg(unix)]
    #[test]
    fn version_check_uses_adapter_env() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("adoc-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("agente");
        std::fs::write(&script, "#!/bin/sh\n[ \"$ADOC_TEST_ENV\" = 1 ]\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut cfg = AdapterConfig::manual();
        cfg.mode = AdapterMode::Headless;
        cfg.trusted = true;
        cfg.command = Some(script.display().to_string());
        assert!(!check_adapter("x", &cfg).available);
        cfg.env.insert("ADOC_TEST_ENV".into(), "1".into());
        assert!(check_adapter("x", &cfg).available);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Un nieto que hereda los pipes no deja colgada la ejecución tras el timeout.
    #[cfg(unix)]
    #[test]
    fn timeout_kills_grandchildren() {
        let mut cfg = AdapterConfig::manual();
        cfg.mode = AdapterMode::Headless;
        cfg.command = Some("sh".into());
        cfg.args = vec!["-c".into(), "sleep 30 & sleep 30".into()];
        cfg.trusted = true;
        let start = std::time::Instant::now();
        let out = run_headless(&cfg, Path::new("t.md"), "x", Path::new("."), 1).unwrap();
        assert_eq!(out.code, None);
        assert!(out.timed_out);
        assert!(out.failure_reason().starts_with("timeout"));
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
    }

    /// El agente termina pero deja un nieto con los pipes abiertos: no se
    /// cuelga esperando a los lectores y el nieto muere con el grupo.
    #[cfg(unix)]
    #[test]
    fn exited_agent_with_background_grandchild_does_not_hang() {
        let mut cfg = AdapterConfig::manual();
        cfg.mode = AdapterMode::Headless;
        cfg.command = Some("sh".into());
        cfg.args = vec!["-c".into(), "echo hola; sleep 60 & exit 0".into()];
        cfg.trusted = true;
        let start = std::time::Instant::now();
        let out = run_headless(&cfg, Path::new("t.md"), "x", Path::new("."), 120).unwrap();
        assert_eq!(out.code, Some(0));
        assert!(!out.timed_out);
        assert!(out.output.contains("hola"), "{}", out.output);
        assert!(start.elapsed() < std::time::Duration::from_secs(15));
    }

    /// Muerte por señal (SIGSEGV, OOM…) no se informa como timeout.
    #[cfg(unix)]
    #[test]
    fn signal_death_is_not_a_timeout() {
        archeology_core::i18n::set_ui_lang(Lang::Es);
        let mut cfg = AdapterConfig::manual();
        cfg.mode = AdapterMode::Headless;
        cfg.command = Some("sh".into());
        cfg.args = vec!["-c".into(), "kill -SEGV $$".into()];
        cfg.trusted = true;
        let out = run_headless(&cfg, Path::new("t.md"), "x", Path::new("."), 30).unwrap();
        assert_eq!(out.code, None);
        assert!(!out.timed_out);
        assert_eq!(out.signal, Some(libc::SIGSEGV));
        assert!(
            out.failure_reason().contains("señal"),
            "{}",
            out.failure_reason()
        );
    }

    /// Un timeout enorme no desborda el cálculo del plazo.
    #[cfg(unix)]
    #[test]
    fn huge_timeout_does_not_overflow() {
        let mut cfg = AdapterConfig::manual();
        cfg.mode = AdapterMode::Headless;
        cfg.command = Some("sh".into());
        cfg.args = vec!["-c".into(), "exit 3".into()];
        cfg.trusted = true;
        let out = run_headless(&cfg, Path::new("t.md"), "x", Path::new("."), u64::MAX).unwrap();
        assert_eq!(out.code, Some(3));
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
        let res = run_headless(
            &cfg,
            Path::new("t.md"),
            "---\ntask_id: T-9\n",
            Path::new("."),
            20,
        )
        .unwrap();
        assert_eq!(res.code, Some(0));
        assert!(res.output.contains("task_id: T-9"));
        assert!(res.output.len() > 200_000);
    }
}
