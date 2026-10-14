//! Orquestador: ejecuta tareas via adaptadores (headless) o las exporta
//! (manual). Registra salida, duración y agente por tarea (RF-044).
//!
//! Éxito de una tarea = código de salida 0 **y** validación de los
//! artefactos declarados en `# Produce` (§14.2).

use crate::adapter;
use crate::limits::{self, AgentLimit, LimitKind};
use anyhow::Result;
use archeology_core::config::AdapterMode;
use archeology_core::model::{RunRecord, TaskRun};
use archeology_core::planner::{self, TaskMeta};
use archeology_core::state::State;
use archeology_core::t;
use archeology_core::task_context;
use archeology_core::validate::{self, OutputProblem};
use archeology_core::Project;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub struct InspectOptions {
    pub agent: String,
    /// Solo estas tareas (task_id). Vacío = todas las pendientes.
    pub only_tasks: Vec<String>,
    /// Solo este módulo.
    pub module: Option<String>,
    pub resume: bool,
    pub dry_run: bool,
    /// Modelo para esta corrida (sobrescribe `model` del adaptador).
    pub model: Option<String>,
    /// Callback de progreso para la interfaz (spinner); opcional.
    pub on_event: Option<InspectCallback>,
}

/// Callback de progreso de `inspect`.
pub type InspectCallback = Box<dyn Fn(&InspectEvent)>;

/// Progreso de una corrida de `inspect`, para mostrarlo en la terminal.
#[derive(Debug, Clone)]
pub enum InspectEvent {
    /// Empieza la corrida: `total` tareas por ejecutar.
    RunStarted { run_id: String, total: usize },
    /// Empieza un intento de una tarea.
    TaskStarted {
        task_id: String,
        kind: String,
        module: Option<String>,
        attempt: u32,
        attempts: u32,
        /// Posición de la tarea en la corrida (1..=total).
        index: usize,
        total: usize,
    },
    /// Terminó una tarea (tras todos sus intentos).
    TaskFinished(Box<TaskOutcome>),
}

/// Resultado de una tarea en una corrida, listo para mostrarlo.
#[derive(Debug, Clone, Default)]
pub struct TaskOutcome {
    pub task_id: String,
    pub kind: String,
    pub module: Option<String>,
    /// `done`, `failed`, `question` o `pending` (interrumpida).
    pub status: String,
    pub duration_ms: u128,
    pub attempts: u32,
    /// Problemas en frases legibles, agrupados por tipo (vacío si terminó bien).
    pub problems: Vec<String>,
    /// Qué hacer ante problemas de validación (None si el fallo es del agente).
    pub hint: Option<String>,
    /// Primera frase del mensaje final del agente.
    pub agent_message: Option<String>,
    /// Se dejó de reintentar porque el fallo se repitió igual.
    pub stopped_early: bool,
    /// El agente terminó mal (código de salida, timeout…), no la validación.
    pub agent_failed: bool,
    /// Pregunta con la que terminó el agente (estado `question`).
    pub question: Option<String>,
    /// Log de la tarea en esta corrida.
    pub log: PathBuf,
    /// Evidencias ajustadas al final del archivo citado (avisos, no fallos).
    pub adjusted: Vec<String>,
    /// Límite del agente que cortó la tarea (cuota, sesión, contexto).
    pub limit: Option<AgentLimit>,
}

/// La corrida se detuvo porque el agente alcanzó un límite (cuota, rate
/// limit) o no tiene sesión: la tarea volvió a `pending` y no se lanzaron más.
#[derive(Debug, Clone)]
pub struct LimitStop {
    pub agent: String,
    /// Ejecutable del agente (para la ayuda de inicio de sesión).
    pub program: String,
    pub task_id: String,
    pub limit: AgentLimit,
    /// Tareas seleccionadas que no llegaron a empezar.
    pub not_started: usize,
}

impl LimitStop {
    /// Código de salida del proceso: 75 (EX_TEMPFAIL) ante un límite de uso
    /// o de peticiones; 77 (EX_NOPERM) si el agente no tiene sesión.
    pub fn exit_code(&self) -> i32 {
        match self.limit.kind {
            LimitKind::Auth => EXIT_AUTH,
            _ => EXIT_LIMIT,
        }
    }
}

/// Código de salida cuando la corrida se detiene por un límite del agente.
pub const EXIT_LIMIT: i32 = 75;
/// Código de salida cuando el agente no tiene sesión iniciada.
pub const EXIT_AUTH: i32 = 77;

#[derive(Debug, Default)]
pub struct InspectReport {
    pub run_id: String,
    pub executed: Vec<String>,
    pub exported: Vec<String>,
    pub failed: Vec<(String, String)>,
    /// Tareas seleccionadas que no se ejecutaron por dependencias no cumplidas.
    pub blocked: Vec<(String, Vec<String>)>,
    /// Archivos modificados por el agente fuera de las rutas permitidas (§19).
    pub out_of_scope: Vec<(String, Vec<String>)>,
    /// Evidencias ajustadas automáticamente por tarea (avisos).
    pub adjusted: Vec<(String, Vec<String>)>,
    /// Causa raíz de los bloqueos: dependencias fuera de esta corrida
    /// (fallidas antes, manuales, interrumpidas o filtradas) y su estado.
    pub root_causes: Vec<(String, String)>,
    /// Tareas en modo manual que esta corrida no tomó (esperan `task done`).
    pub manual_waiting: Vec<String>,
    /// Tareas que terminaron con una pregunta del agente (estado `question`).
    pub questions: Vec<(String, String)>,
    /// Resultado por tarea ejecutada, en orden.
    pub tasks: Vec<TaskOutcome>,
    /// Carpeta de logs de la corrida (None en simulación o modo manual).
    pub run_dir: Option<PathBuf>,
    /// La corrida se detuvo por un límite del agente.
    pub limit_stop: Option<LimitStop>,
}

pub struct TaskFile {
    pub meta: TaskMeta,
    pub path: PathBuf,
    pub content: String,
}

/// Carga y ordena las tareas de `.archeology-doc/tasks/`.
pub fn load_tasks(project: &Project) -> Result<Vec<TaskFile>> {
    let dir = project.tasks_dir();
    let mut tasks = Vec::new();
    if !dir.is_dir() {
        return Ok(tasks);
    }
    for entry in fs::read_dir(&dir)?.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "md").unwrap_or(false) {
            let content = fs::read_to_string(&path)?;
            if let Some(meta) = planner::parse_task(&content) {
                tasks.push(TaskFile {
                    meta,
                    path,
                    content,
                });
            }
        }
    }
    tasks.sort_by(|a, b| a.meta.task_id.cmp(&b.meta.task_id));
    Ok(tasks)
}

/// Escribe el nuevo status en el frontmatter del archivo de tarea.
pub fn set_task_status(path: &Path, status: &str) -> Result<()> {
    let text = fs::read_to_string(path)?;
    archeology_core::fsutil::atomic_write(
        path,
        planner::set_status_in_markdown(&text, status).as_bytes(),
    )?;
    Ok(())
}

/// Ejecuta las tareas pendientes según el adaptador (§14.2).
///
/// Las tareas se ejecutan en orden topológico dentro de la misma corrida:
/// una tarea corre en cuanto todas sus dependencias están `done` (RF-035).
/// Si una dependencia falla, sus dependientes quedan bloqueadas.
pub fn run_tasks(project: &Project, opts: &InspectOptions) -> Result<InspectReport> {
    let cfg = project.config.clone();
    let adapter = cfg
        .agent
        .adapters
        .get(&opts.agent)
        .ok_or_else(|| {
            anyhow::anyhow!(t!(
                "agents.orchestrator.adapter_undefined",
                agent = opts.agent
            ))
        })?
        .clone();
    let mut adapter = adapter;
    if opts.model.is_some() {
        adapter.model = opts.model.clone();
    }
    // El modelo llega a los argumentos del agente: se valida (con el formato
    // que espera ese CLI) antes de tocar tareas.
    if adapter.mode == AdapterMode::Headless {
        adapter::apply_model(&adapter, &adapter.args)?;
    } else if let Some(m) = adapter.model.as_deref().filter(|m| !m.is_empty()) {
        adapter::validate_model(m)?;
    }
    if adapter.mode == archeology_core::config::AdapterMode::Headless
        && !adapter.trusted
        && !opts.dry_run
    {
        anyhow::bail!(archeology_core::trust::untrusted_message(&opts.agent));
    }

    // Verificación previa: no lanzar un agente que no podrá cumplir la tarea.
    if !opts.dry_run && adapter.mode == AdapterMode::Headless {
        let problems = crate::adapter::adapter_warnings(&opts.agent, &adapter);
        if !problems.is_empty() {
            anyhow::bail!(t!(
                "agents.orchestrator.invalid_config",
                problems = problems.join("\n  - ")
            ));
        }
    }

    let all = load_tasks(project)?;
    let mut state = State::load(&project.state_file())?;
    let mut status: BTreeMap<String, String> = all
        .iter()
        .map(|t| (t.meta.task_id.clone(), t.meta.status.clone()))
        .collect();

    let selected: Vec<&TaskFile> = all
        .iter()
        .filter(|t| {
            let st = t.meta.status.as_str();
            // Con un agente headless, `--resume` también ejecuta las tareas que
            // quedaron exportadas en modo manual (p. ej. tras usar `manual`).
            let resumable = st == "failed"
                || st == "running"
                || (st == "manual" && adapter.mode == AdapterMode::Headless);
            let pending = st == "pending" || (opts.resume && resumable);
            let task_ok = opts.only_tasks.is_empty() || opts.only_tasks.contains(&t.meta.task_id);
            let mod_ok = match &opts.module {
                Some(m) => planner::covers_module(t.meta.module.as_deref(), m),
                None => true,
            };
            pending && task_ok && mod_ok
        })
        .collect();

    let selected_ids: BTreeSet<&str> = selected.iter().map(|t| t.meta.task_id.as_str()).collect();
    let manual_waiting: Vec<String> = all
        .iter()
        .filter(|t| t.meta.status == "manual" && !selected_ids.contains(t.meta.task_id.as_str()))
        .map(|t| t.meta.task_id.clone())
        .collect();
    let run_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let started_at = chrono::Utc::now().to_rfc3339();
    let mut report = InspectReport {
        manual_waiting,
        run_id: run_id.clone(),
        ..Default::default()
    };

    // Simulación / exportación: se asume que las seleccionadas se completarán.
    if opts.dry_run || adapter.mode == AdapterMode::Export {
        let order = topological(&selected, &status);
        for t in &selected {
            if !order.contains(&t.meta.task_id) {
                report
                    .blocked
                    .push((t.meta.task_id.clone(), unmet_deps(&t.meta, &status)));
            }
        }
        report.root_causes = root_causes(&report, &status);
        if opts.dry_run {
            report.exported = order;
            return Ok(report);
        }
        // Modo manual (RF-041): las tareas ya están en el espacio de trabajo.
        for id in order {
            let t = selected.iter().find(|t| t.meta.task_id == id).unwrap();
            set_task_status(&t.path, "manual")?;
            state.tasks.insert(id.clone(), "manual".into());
            report.exported.push(id);
        }
        state.runs.push(RunRecord {
            run_id,
            started_at,
            finished_at: Some(chrono::Utc::now().to_rfc3339()),
            agent: opts.agent.clone(),
            tasks_done: vec![],
            tasks_failed: vec![],
            tasks: vec![],
        });
        state.save(&project.state_file())?;
        return Ok(report);
    }

    let run_dir = project.runs_dir().join(format!(
        "{}-{run_id}",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    ));
    fs::create_dir_all(&run_dir)?;
    report.run_dir = Some(run_dir.clone());
    if let Some(f) = &opts.on_event {
        f(&InspectEvent::RunStarted {
            run_id: run_id.clone(),
            total: selected.len(),
        });
    }
    let ui_lang = archeology_core::i18n::ui_lang();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let timeout = adapter
        .timeout_secs
        .unwrap_or(cfg.agent.timeout_secs)
        .min(adapter::MAX_TIMEOUT_SECS);
    let attempts = cfg.agent.retries.min(MAX_RETRIES) + 1;
    // Ctrl+C/SIGTERM durante la corrida: se mata al agente y la tarea queda reanudable.
    let _interrupt = crate::interrupt::Guard::new();

    let mut remaining: Vec<&TaskFile> = selected.clone();
    let mut task_runs: Vec<TaskRun> = Vec::new();
    loop {
        let ready_idx = remaining.iter().position(|t| {
            t.meta
                .depends_on
                .iter()
                .all(|d| status.get(d).map(|s| s == "done").unwrap_or(false))
        });
        let Some(idx) = ready_idx else { break };
        if crate::interrupt::requested() {
            break;
        }
        let t = remaining.remove(idx);
        let id = t.meta.task_id.clone();

        // Marca `running` para que una interrupción sea reanudable (RNF-08).
        set_task_status(&t.path, "running")?;
        state.tasks.insert(id.clone(), "running".into());
        state.save(&project.state_file())?;

        tracing::info!(task = %id, "{}", t!("agents.orchestrator.running_task"));
        let index = selected.len() - remaining.len();
        let before = git_dirty_files(&project.root);
        let task_start = SystemTime::now();
        let mut outcome: std::result::Result<(), String> =
            Err(t!("agents.orchestrator.no_attempts"));
        let mut total_ms: u128 = 0;
        let mut exit_code: Option<i32> = None;
        let mut used_attempts = 0;
        let mut question: Option<String> = None;
        let mut interrupted = false;
        let mut log = String::new();
        // La tarea dice qué salidas están desactualizadas (y por qué) y cuáles
        // ya están vigentes: re-verificar no exige reescribir lo que sigue bien.
        let mut content = task_context::refresh_task_file(project, &state, &t.path, &today)
            .unwrap_or_else(|_| t.content.clone());
        let task_lang = planner::task_lang(&content);
        // Retroalimentación que ya recibió el agente (intento o corrida anterior).
        let mut last_feedback = task_context::previous_feedback(&content);
        let mut problems: Vec<OutputProblem> = Vec::new();
        let mut stale_in_dirs: Vec<String> = Vec::new();
        // Se acumulan entre intentos: el ajuste ya quedó escrito en la nota.
        let mut adjusted: Vec<String> = Vec::new();
        let mut agent_message: Option<String> = None;
        let mut agent_failed = false;
        let mut stopped_early = false;
        let mut limit: Option<AgentLimit> = None;
        for attempt in 1..=attempts {
            if let Some(f) = &opts.on_event {
                f(&InspectEvent::TaskStarted {
                    task_id: id.clone(),
                    kind: t.meta.kind.clone(),
                    module: t.meta.module.clone(),
                    attempt,
                    attempts,
                    index,
                    total: selected.len(),
                });
            }
            let result = adapter::run_headless(&adapter, &t.path, &content, &project.root, timeout);
            log.push_str(&t!(
                "agents.orchestrator.attempt_header",
                attempt = attempt,
                attempts = attempts
            ));
            log.push('\n');
            used_attempts = attempt;
            problems.clear();
            stale_in_dirs.clear();
            agent_failed = false;
            if let Ok(run) = &result {
                total_ms += run.duration_ms;
                exit_code = run.code;
                interrupted = run.interrupted;
                agent_message = task_context::agent_summary(&run.output);
            }
            outcome = match result {
                Ok(run) if run.success() && !run.interrupted => {
                    let (ms, output) = (run.duration_ms, run.output);
                    log.push_str(&output);
                    // Desde el inicio de la tarea, no del intento: un archivo
                    // escrito en un intento anterior también cuenta.
                    let check = validate::check_task_output(
                        project,
                        &t.meta.produce,
                        &t.meta.optional,
                        Some(task_start),
                    );
                    problems = check.problems.clone();
                    stale_in_dirs = check.stale_in_dirs.clone();
                    adjusted.extend(check.repairs.iter().map(|r| r.message()));
                    if !log.ends_with('\n') {
                        log.push('\n');
                    }
                    for i in &check.issues {
                        log.push_str(&format!(
                            "{} {}: {}\n",
                            t!("agents.orchestrator.validation_prefix"),
                            project.rel(Path::new(&i.path)),
                            i.message
                        ));
                    }
                    tracing::info!(task = %id, ms, "{}", t!("agents.orchestrator.agent_finished"));
                    if problems.is_empty() {
                        Ok(())
                    } else if let Some(l) = limits::classify(&output, true) {
                        // Terminó con 0 pero sin escribir: el agente avisó de un límite.
                        let reason = l.reason();
                        limit = Some(l);
                        Err(reason)
                    } else if let Some(q) = extract_question(&output) {
                        // No reintentar: la misma tarea volvería a preguntar.
                        question = Some(q.clone());
                        Err(t!("agents.orchestrator.agent_question", question = q))
                    } else {
                        let hint = agent_hint(&output)
                            .map(|h| t!("agents.orchestrator.agent_hint", hint = h))
                            .unwrap_or_default();
                        Err(t!(
                            "agents.orchestrator.invalid_output",
                            errors = task_context::summarize(ui_lang, &problems, &stale_in_dirs)
                                .join("; "),
                            hint = hint
                        ))
                    }
                }
                Ok(run) => {
                    agent_failed = true;
                    log.push_str(&run.output);
                    // Cuota, sesión o contexto: reintentar no sirve.
                    match (!run.timed_out && !run.interrupted)
                        .then(|| limits::classify(&run.output, false))
                        .flatten()
                    {
                        Some(l) => {
                            let reason = l.reason();
                            limit = Some(l);
                            Err(reason)
                        }
                        None => Err(run.failure_reason()),
                    }
                }
                Err(e) => {
                    agent_failed = true;
                    Err(e.to_string())
                }
            };
            if let Some(l) = &limit {
                if !log.ends_with('\n') {
                    log.push('\n');
                }
                log.push_str(&format!("\n{}\n", l.log_line()));
                tracing::info!(task = %id, kind = l.kind.code(), "{}", l.message);
            }
            if outcome.is_ok() || question.is_some() || interrupted || limit.is_some() {
                break;
            }
            if let Err(reason) = &outcome {
                log.push_str(&format!(
                    "\n{}\n",
                    t!("agents.orchestrator.failure_line", reason = reason)
                ));
            }
            // Fallo de validación: el siguiente intento recibe los problemas y
            // cómo corregirlos. Si se repite igual, no se gastan más intentos.
            if !problems.is_empty() {
                let items = task_context::feedback_items(task_lang, &problems);
                if items == last_feedback {
                    stopped_early = true;
                    log.push_str(&format!("{}\n", t!("core.task_output.same_failure")));
                    break;
                }
                let updated = task_context::apply_feedback_section(&content, Some(&items));
                if archeology_core::fsutil::atomic_write(&t.path, updated.as_bytes()).is_ok() {
                    content = updated;
                }
                last_feedback = items;
            }
        }
        fs::write(run_dir.join(format!("{id}.log")), &log).ok();

        // Verificación de alcance: cambios del agente fuera de las rutas permitidas.
        if let (Some(before), Some(after)) = (before, git_dirty_files(&project.root)) {
            let outside: Vec<String> = after
                .difference(&before)
                .filter(|f| !is_allowed_output(project, f))
                .cloned()
                .collect();
            if !outside.is_empty() {
                tracing::warn!(task = %id, "{}", t!("agents.orchestrator.out_of_scope_warning"));
                report.out_of_scope.push((id.clone(), outside));
            }
        }

        let reason = outcome.as_ref().err().cloned();
        let stop = limit.as_ref().filter(|l| l.kind.stops_run()).cloned();
        let new_status = match outcome {
            // Interrumpida por el usuario: vuelve a `pending` (se retoma con
            // `adoc inspect`); no cuenta como fallo del agente.
            Err(_) if interrupted => {
                log_interrupted(&run_dir, &id, &log);
                "pending"
            }
            Ok(()) => {
                report.executed.push(id.clone());
                // Línea base de las fuentes citadas por las notas nuevas (RF-120);
                // las notas re-verificadas dejan de figurar como desactualizadas.
                archeology_core::index::refresh_hashes(project, &mut state, task_start);
                // Sin retroalimentación pendiente y con el contexto al día.
                if let Ok(text) = fs::read_to_string(&t.path) {
                    let cleared = task_context::apply_feedback_section(&text, None);
                    if cleared != text {
                        archeology_core::fsutil::atomic_write(&t.path, cleared.as_bytes()).ok();
                    }
                }
                task_context::refresh_task_file(project, &state, &t.path, &today).ok();
                "done"
            }
            // Cuota, rate limit o sin sesión: la tarea no falló, vuelve a
            // `pending` y la corrida se detiene (las siguientes fallarían igual).
            Err(_) if stop.is_some() => "pending",
            Err(_) if question.is_some() => {
                let q = question.clone().unwrap_or_default();
                state.questions.insert(id.clone(), q.clone());
                report.questions.push((id.clone(), q));
                "question"
            }
            Err(reason) => {
                // Una tarea fallida no aborta la ejecución completa (RNF-08).
                report.failed.push((id.clone(), reason));
                "failed"
            }
        };
        // Problemas en frases legibles: los de validación agrupados por
        // tipo; si falló el agente, su motivo tal cual.
        let failed = new_status == "failed";
        let lines = if !failed {
            Vec::new()
        } else if !problems.is_empty() {
            task_context::summarize(ui_lang, &problems, &stale_in_dirs)
        } else {
            reason.clone().into_iter().collect()
        };
        let hint = if failed && !problems.is_empty() {
            Some(task_context::fix_hint(ui_lang, &id, &problems))
        } else if failed && limit.as_ref().is_some_and(|l| l.kind == LimitKind::Context) {
            // La tarea no cabe en el contexto: dividirla.
            Some(limits::context_hint(
                cfg.plan.max_files_per_task,
                cfg.plan.max_bytes_per_task,
            ))
        } else {
            None
        };
        let task_outcome = TaskOutcome {
            task_id: id.clone(),
            kind: t.meta.kind.clone(),
            module: t.meta.module.clone(),
            status: new_status.to_string(),
            duration_ms: total_ms,
            attempts: used_attempts,
            problems: lines.clone(),
            hint: hint.clone(),
            agent_message: agent_message.clone().filter(|_| failed),
            stopped_early: stopped_early && failed,
            agent_failed: agent_failed && failed,
            question: question.clone(),
            log: run_dir.join(format!("{id}.log")),
            adjusted: adjusted.clone(),
            limit: limit.clone(),
        };
        if !adjusted.is_empty() {
            report.adjusted.push((id.clone(), adjusted.clone()));
        }
        if let Some(f) = &opts.on_event {
            f(&InspectEvent::TaskFinished(Box::new(task_outcome.clone())));
        }
        report.tasks.push(task_outcome);
        task_runs.push(TaskRun {
            task_id: id.clone(),
            status: new_status.into(),
            duration_ms: total_ms,
            exit_code,
            attempts: used_attempts,
            reason: if failed && !lines.is_empty() {
                Some(lines.join("; "))
            } else {
                reason
            },
            problems: lines,
            hint,
            agent_message: agent_message.filter(|_| failed),
        });
        set_task_status(&t.path, new_status)?;
        state.tasks.insert(id.clone(), new_status.into());
        status.insert(id.clone(), new_status.into());
        state.save(&project.state_file())?;
        if let Some(l) = stop {
            report.limit_stop = Some(LimitStop {
                agent: opts.agent.clone(),
                program: crate::command::program_stem(adapter.command.as_deref().unwrap_or("")),
                task_id: id,
                limit: l,
                not_started: remaining.len(),
            });
            break;
        }
        if interrupted {
            break;
        }
    }
    let was_interrupted = crate::interrupt::requested();
    let stopped = was_interrupted || report.limit_stop.is_some();

    // Tras una interrupción o un límite del agente, las que faltan no están
    // bloqueadas: siguen pendientes.
    for t in remaining.into_iter().filter(|_| !stopped) {
        report
            .blocked
            .push((t.meta.task_id.clone(), unmet_deps(&t.meta, &status)));
    }
    report.root_causes = root_causes(&report, &status);

    state.runs.push(RunRecord {
        run_id,
        started_at: started_at.clone(),
        finished_at: Some(chrono::Utc::now().to_rfc3339()),
        agent: opts.agent.clone(),
        tasks_done: report.executed.clone(),
        tasks_failed: report.failed.iter().map(|(id, _)| id.clone()).collect(),
        tasks: task_runs.clone(),
    });
    state.save(&project.state_file())?;

    // Notas índice por tipo con lo que produjo la corrida (RF-064).
    if !report.executed.is_empty() {
        archeology_core::index::write_mocs(project)?;
    }

    // Log legible de la corrida (RNF-09).
    let mut summary = t!(
        "agents.run_log.header",
        run_id = report.run_id,
        agent = opts.agent,
        start = started_at,
        end = chrono::Utc::now().to_rfc3339()
    );
    summary.push_str("\n\n");
    for r in &task_runs {
        let exit = r
            .exit_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "-".into());
        summary.push_str(&t!(
            "agents.run_log.row",
            task = r.task_id,
            status = format!("{:<7}", r.status),
            ms = format!("{:>8}", r.duration_ms),
            exit = format!("{exit:<4}"),
            attempts = r.attempts,
            reason = r
                .reason
                .as_deref()
                .map(|x| format!("  — {x}"))
                .unwrap_or_default()
        ));
        summary.push('\n');
    }
    for (id, deps) in &report.blocked {
        summary.push_str(&t!(
            "agents.run_log.blocked",
            task = id,
            deps = deps.join(", ")
        ));
        summary.push('\n');
    }
    for (id, files) in &report.out_of_scope {
        summary.push_str(&t!(
            "agents.run_log.out_of_scope",
            task = id,
            files = files.join(", ")
        ));
        summary.push('\n');
    }
    for (id, notes) in &report.adjusted {
        for n in notes {
            summary.push_str(&format!("{id} {n}\n"));
        }
    }
    if let Some(stop) = &report.limit_stop {
        summary.push_str(&format!(
            "\n{}\n",
            t!(
                "agents.limit.run_log",
                kind = stop.limit.kind.code(),
                agent = stop.agent,
                task = stop.task_id,
                message = stop.limit.message,
                reset = stop
                    .limit
                    .reset_at
                    .map(|r| t!("agents.limit.reset_suffix", when = r.to_rfc3339()))
                    .unwrap_or_default(),
                count = stop.not_started,
                code = stop.exit_code()
            )
        ));
    }
    if was_interrupted {
        summary.push_str(&format!("\n{}\n", t!("agents.run_log.interrupted")));
    }
    fs::write(run_dir.join("run.log"), summary).ok();

    if was_interrupted {
        return Err(crate::interrupt::Interrupted.into());
    }
    Ok(report)
}

/// Reintentos máximos por tarea (protege de configs con valores absurdos).
const MAX_RETRIES: u32 = 10;

/// Nota en el log de la tarea interrumpida.
fn log_interrupted(run_dir: &Path, id: &str, log: &str) {
    let text = format!("{log}\n{}\n", t!("agents.orchestrator.interrupted_note"));
    fs::write(run_dir.join(format!("{id}.log")), text).ok();
}

/// Pregunta con la que terminó el agente, si la hay: una de las últimas
/// líneas con texto que termina en `?` (ignorando formato Markdown).
pub fn extract_question(output: &str) -> Option<String> {
    // Prefijo de las líneas de validación del log, en cualquier idioma.
    let validation: Vec<String> = archeology_core::i18n::Lang::ALL
        .iter()
        .map(|l| archeology_core::tl!(*l, "agents.orchestrator.validation_prefix"))
        .collect();
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && !validation.iter().any(|v| l.starts_with(v.as_str()))
                && !l.starts_with("===")
        })
        .collect();
    lines.iter().rev().take(6).find_map(|l| {
        let clean =
            l.trim_matches(|c: char| c == '*' || c == '_' || c == '`' || c == '>' || c == ' ');
        if clean.ends_with('?') && clean.chars().count() > 8 {
            Some(clean.chars().take(400).collect())
        } else {
            None
        }
    })
}

/// Primera línea de la salida del agente que explica por qué no escribió:
/// permisos rechazados, herramientas bloqueadas, sesión sin autenticar…
fn agent_hint(output: &str) -> Option<String> {
    const MARKERS: &[&str] = &[
        "rejected",
        "requires confirmation",
        "permission",
        "not allowed",
        "denied",
        "unauthorized",
        "not logged in",
        "login",
        "api key",
        "rate limit",
        "quota",
    ];
    output
        .lines()
        .map(str::trim)
        .find(|l| {
            let lower = l.to_lowercase();
            MARKERS.iter().any(|m| lower.contains(m))
        })
        .map(|l| l.chars().take(240).collect())
}

/// Orden de ejecución suponiendo que cada tarea seleccionada termina bien.
fn topological(selected: &[&TaskFile], status: &BTreeMap<String, String>) -> Vec<String> {
    let mut done: BTreeSet<String> = status
        .iter()
        .filter(|(_, s)| s.as_str() == "done")
        .map(|(id, _)| id.clone())
        .collect();
    let mut order = Vec::new();
    let mut remaining: Vec<&&TaskFile> = selected.iter().collect();
    while let Some(idx) = remaining
        .iter()
        .position(|t| t.meta.depends_on.iter().all(|d| done.contains(d)))
    {
        let t = remaining.remove(idx);
        done.insert(t.meta.task_id.clone());
        order.push(t.meta.task_id.clone());
    }
    order
}

/// Dependencias que bloquean y no están a su vez bloqueadas ni fallaron en
/// esta corrida: lo que el usuario tiene que resolver.
fn root_causes(report: &InspectReport, status: &BTreeMap<String, String>) -> Vec<(String, String)> {
    let blocked: BTreeSet<&str> = report.blocked.iter().map(|(id, _)| id.as_str()).collect();
    let failed_now: BTreeSet<&str> = report.failed.iter().map(|(id, _)| id.as_str()).collect();
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for (_, deps) in &report.blocked {
        for d in deps {
            if !blocked.contains(d.as_str()) && !failed_now.contains(d.as_str()) {
                let st = status
                    .get(d)
                    .cloned()
                    .unwrap_or_else(|| t!("agents.orchestrator.missing_dependency"));
                out.insert(d.clone(), st);
            }
        }
    }
    out.into_iter().collect()
}

fn unmet_deps(meta: &TaskMeta, status: &BTreeMap<String, String>) -> Vec<String> {
    meta.depends_on
        .iter()
        .filter(|d| status.get(*d).map(|s| s != "done").unwrap_or(true))
        .cloned()
        .collect()
}

/// Archivos modificados o nuevos del proyecto (None si no hay repo).
fn git_dirty_files(root: &Path) -> Option<BTreeSet<String>> {
    archeology_scan::git::dirty_files(root)
}

/// Rutas donde un agente puede escribir (§10, RF-161).
fn is_allowed_output(project: &Project, rel: &str) -> bool {
    let cfg = &project.config;
    let prefixes = [
        format!("{}/", cfg.paths.knowledge.trim_end_matches('/')),
        format!("{}/", cfg.paths.docs.trim_end_matches('/')),
        ".archeology-doc/".to_string(),
        ".agents/".to_string(),
        format!("{}/changes/", cfg.openspec.path.trim_end_matches('/')),
    ];
    let files = [
        "AGENTS.md",
        "CLAUDE.md",
        "llms.txt",
        ".github/copilot-instructions.md",
    ];
    prefixes.iter().any(|p| rel.starts_with(p.as_str()))
        || files.contains(&rel)
        || rel.starts_with(".cursor/rules")
}

/// Vista previa del comando que ejecutaría un adaptador (`--dry-run`).
pub fn preview_command(
    adapter: &archeology_core::config::AdapterConfig,
    prompt_file: &std::path::Path,
    prompt_content: &str,
) -> Option<String> {
    let cmd = adapter.command.clone()?;
    // Con el modelo aplicado: la vista previa muestra el flag real.
    let templates = adapter::apply_model(adapter, &adapter.args)
        .map(|m| m.args)
        .unwrap_or_else(|_| adapter.args.clone());
    let args = adapter::expand_args(&templates, prompt_file, prompt_content);
    let preview_args: Vec<String> = args
        .iter()
        .map(|a| {
            if a.len() > 80 {
                format!(
                    "\"{}\"",
                    t!("agents.orchestrator.prompt_bytes", bytes = a.len())
                )
            } else {
                format!("\"{a}\"")
            }
        })
        .collect();
    Some(format!("{} {}", cmd, preview_args.join(" ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tf(id: &str, deps: &[&str]) -> TaskFile {
        TaskFile {
            meta: TaskMeta {
                task_id: id.into(),
                kind: "module".into(),
                module: None,
                status: "pending".into(),
                depends_on: deps.iter().map(|d| d.to_string()).collect(),
                produce: vec![],
                optional: vec![],
            },
            path: PathBuf::new(),
            content: String::new(),
        }
    }

    #[test]
    fn extracts_final_question() {
        let out = "Revisé el código.\n\n**¿El descuento máximo correcto es 10 % o 20 %?**\n";
        assert_eq!(
            extract_question(out).as_deref(),
            Some("¿El descuento máximo correcto es 10 % o 20 %?")
        );
        assert_eq!(extract_question("Listo.\nNo hay dudas."), None);
    }

    #[test]
    fn extracts_agent_permission_hint() {
        let out =
            "Leyendo archivos...\nwarning: rejected a tool call that requires confirmation.\nfin";
        assert_eq!(
            agent_hint(out).as_deref(),
            Some("warning: rejected a tool call that requires confirmation.")
        );
        assert_eq!(agent_hint("todo bien"), None);
    }

    #[test]
    fn topological_runs_all_layers_in_one_pass() {
        let a = tf("T-001", &[]);
        let b = tf("T-002", &["T-001"]);
        let c = tf("T-003", &["T-002"]);
        let d = tf("T-004", &["T-999"]);
        let status = BTreeMap::new();
        let order = topological(&[&c, &a, &b, &d], &status);
        assert_eq!(order, vec!["T-001", "T-002", "T-003"]);
    }
}
