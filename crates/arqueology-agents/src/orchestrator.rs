//! Orquestador: ejecuta tareas via adaptadores (headless) o las exporta
//! (manual). Registra salida, duración y agente por tarea (RF-044).
//!
//! Éxito de una tarea = código de salida 0 **y** validación de los
//! artefactos declarados en `# Produce` (§14.2).

use crate::adapter::{self, expand_args};
use anyhow::Result;
use arqueology_core::config::AdapterMode;
use arqueology_core::frontmatter::IssueSeverity;
use arqueology_core::model::{RunRecord, TaskRun};
use arqueology_core::planner::{self, TaskMeta};
use arqueology_core::state::State;
use arqueology_core::validate;
use arqueology_core::Project;
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
    TaskFinished {
        task_id: String,
        ok: bool,
        duration_ms: u128,
        detail: Option<String>,
    },
}

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
    /// Causa raíz de los bloqueos: dependencias fuera de esta corrida
    /// (fallidas antes, manuales, interrumpidas o filtradas) y su estado.
    pub root_causes: Vec<(String, String)>,
    /// Tareas en modo manual que esta corrida no tomó (esperan `task done`).
    pub manual_waiting: Vec<String>,
    /// Tareas que terminaron con una pregunta del agente (estado `question`).
    pub questions: Vec<(String, String)>,
}

pub struct TaskFile {
    pub meta: TaskMeta,
    pub path: PathBuf,
    pub content: String,
}

/// Carga y ordena las tareas de `.arqueology-doc/tasks/`.
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
    fs::write(path, planner::set_status_in_markdown(&text, status))?;
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
            anyhow::anyhow!(
                "adaptador '{}' no definido en arqueology-doc.toml",
                opts.agent
            )
        })?
        .clone();
    let mut adapter = adapter;
    if opts.model.is_some() {
        adapter.model = opts.model.clone();
    }
    if adapter.mode == arqueology_core::config::AdapterMode::Headless
        && !adapter.trusted
        && !opts.dry_run
    {
        anyhow::bail!(arqueology_core::trust::untrusted_message(&opts.agent));
    }

    // Verificación previa: no lanzar un agente que no podrá cumplir la tarea.
    if !opts.dry_run && adapter.mode == AdapterMode::Headless {
        let problems = crate::adapter::adapter_warnings(&opts.agent, &adapter);
        if !problems.is_empty() {
            anyhow::bail!(
                "configuración del agente inválida:\n  - {}\n(revisa arqueology-doc.toml o ejecuta `adoc doctor`)",
                problems.join("\n  - ")
            );
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
    let timeout = adapter.timeout_secs.unwrap_or(cfg.agent.timeout_secs);
    let attempts = cfg.agent.retries + 1;

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
        let t = remaining.remove(idx);
        let id = t.meta.task_id.clone();

        // Marca `running` para que una interrupción sea reanudable (RNF-08).
        set_task_status(&t.path, "running")?;
        state.tasks.insert(id.clone(), "running".into());
        state.save(&project.state_file())?;

        tracing::info!(task = %id, "ejecutando tarea");
        let index = selected.len() - remaining.len();
        let before = git_dirty_files(&project.root);
        let task_start = SystemTime::now();
        let mut outcome: std::result::Result<(), String> = Err("sin intentos".into());
        let mut total_ms: u128 = 0;
        let mut exit_code: Option<i32> = None;
        let mut used_attempts = 0;
        let mut question: Option<String> = None;
        let mut log = String::new();
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
            let since = SystemTime::now();
            let result =
                adapter::run_headless(&adapter, &t.path, &t.content, &project.root, timeout);
            log.push_str(&format!("=== intento {attempt}/{attempts} ===\n"));
            used_attempts = attempt;
            if let Ok((code, ms, _)) = &result {
                total_ms += ms;
                exit_code = *code;
            }
            outcome = match result {
                Ok((Some(0), ms, output)) => {
                    log.push_str(&output);
                    let issues = validate::validate_task_output(
                        project,
                        &t.meta.produce,
                        &t.meta.optional,
                        Some(since),
                    );
                    let errors: Vec<String> = issues
                        .iter()
                        .filter(|i| i.severity == IssueSeverity::Error)
                        .map(|i| format!("{}: {}", i.path, i.message))
                        .collect();
                    for i in &issues {
                        log.push_str(&format!("[validación] {}: {}\n", i.path, i.message));
                    }
                    tracing::info!(task = %id, ms, "agente terminó");
                    if errors.is_empty() {
                        Ok(())
                    } else if let Some(q) = extract_question(&output) {
                        // No reintentar: la misma tarea volvería a preguntar.
                        question = Some(q.clone());
                        Err(format!("pregunta del agente: {q}"))
                    } else {
                        let hint = agent_hint(&output)
                            .map(|h| format!(" — el agente avisó: {h}"))
                            .unwrap_or_default();
                        Err(format!("salida inválida: {}{hint}", errors.join("; ")))
                    }
                }
                Ok((code, ms, output)) => {
                    log.push_str(&output);
                    Err(match code {
                        Some(c) => format!("exit code {c}"),
                        None => format!("timeout tras {ms}ms"),
                    })
                }
                Err(e) => Err(e.to_string()),
            };
            if outcome.is_ok() || question.is_some() {
                break;
            }
            if let Err(reason) = &outcome {
                log.push_str(&format!("\n[fallo] {reason}\n"));
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
                tracing::warn!(task = %id, "el agente modificó archivos fuera de su alcance");
                report.out_of_scope.push((id.clone(), outside));
            }
        }

        let reason = outcome.as_ref().err().cloned();
        if let Some(f) = &opts.on_event {
            f(&InspectEvent::TaskFinished {
                task_id: id.clone(),
                ok: outcome.is_ok(),
                duration_ms: total_ms,
                detail: reason.clone(),
            });
        }
        let new_status = match outcome {
            Ok(()) => {
                report.executed.push(id.clone());
                // Línea base de las fuentes citadas por las notas nuevas (RF-120).
                arqueology_core::index::refresh_hashes(project, &mut state, task_start);
                "done"
            }
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
        task_runs.push(TaskRun {
            task_id: id.clone(),
            status: new_status.into(),
            duration_ms: total_ms,
            exit_code,
            attempts: used_attempts,
            reason,
        });
        set_task_status(&t.path, new_status)?;
        state.tasks.insert(id.clone(), new_status.into());
        status.insert(id, new_status.into());
        state.save(&project.state_file())?;
    }

    for t in remaining {
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
        arqueology_core::index::write_mocs(project)?;
    }

    // Log legible de la corrida (RNF-09).
    let mut summary = format!(
        "run {} · agente {} · inicio {started_at} · fin {}\n\n",
        report.run_id,
        opts.agent,
        chrono::Utc::now().to_rfc3339()
    );
    for r in &task_runs {
        summary.push_str(&format!(
            "{} {:<7} {:>8} ms  exit={:<4} intentos={}{}\n",
            r.task_id,
            r.status,
            r.duration_ms,
            r.exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "-".into()),
            r.attempts,
            r.reason
                .as_deref()
                .map(|x| format!("  — {x}"))
                .unwrap_or_default()
        ));
    }
    for (id, deps) in &report.blocked {
        summary.push_str(&format!("{id} bloqueada (espera {})\n", deps.join(", ")));
    }
    for (id, files) in &report.out_of_scope {
        summary.push_str(&format!("{id} fuera de alcance: {}\n", files.join(", ")));
    }
    fs::write(run_dir.join("run.log"), summary).ok();

    Ok(report)
}

/// Pregunta con la que terminó el agente, si la hay: una de las últimas
/// líneas con texto que termina en `?` (ignorando formato Markdown).
pub fn extract_question(output: &str) -> Option<String> {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("[validación]") && !l.starts_with("==="))
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
                    .unwrap_or_else(|| "inexistente".into());
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
    arqueology_scan::git::dirty_files(root)
}

/// Rutas donde un agente puede escribir (§10, RF-161).
fn is_allowed_output(project: &Project, rel: &str) -> bool {
    let cfg = &project.config;
    let prefixes = [
        format!("{}/", cfg.paths.knowledge.trim_end_matches('/')),
        format!("{}/", cfg.paths.docs.trim_end_matches('/')),
        ".arqueology-doc/".to_string(),
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
    adapter: &arqueology_core::config::AdapterConfig,
    prompt_file: &std::path::Path,
    prompt_content: &str,
) -> Option<String> {
    let cmd = adapter.command.clone()?;
    let args = expand_args(&adapter.args, prompt_file, prompt_content);
    let preview_args: Vec<String> = args
        .iter()
        .map(|a| {
            if a.len() > 80 {
                format!("\"<{} bytes de prompt>\"", a.len())
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
