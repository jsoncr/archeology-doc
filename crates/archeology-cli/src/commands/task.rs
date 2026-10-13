//! `task` — gestión de tareas en modo manual (RF-041, RF-050).

use anyhow::{Context, Result};
use archeology_core::frontmatter::IssueSeverity;
use archeology_core::planner;
use archeology_core::validate;
use archeology_core::{t, Project};
use clap::Subcommand;
use std::fs;

#[derive(Subcommand)]
pub enum TaskCmd {
    /// Lista las tareas.
    List {
        /// Filtrar por estado (pending|manual|question|done|failed).
        #[arg(long)]
        status: Option<String>,
        /// Formato de salida.
        #[arg(long, default_value = "text", value_parser = ["text", "json"])]
        format: String,
    },
    /// Muestra una tarea completa.
    Show {
        /// ID de la tarea (T-001, ...).
        id: String,
    },
    /// Exporta tareas pendientes como prompts listos para usar.
    Export {
        /// Directorio de salida (por defecto, las tareas del espacio de trabajo).
        #[arg(long)]
        out: Option<String>,
    },
    /// Responde la pregunta de un agente y deja la tarea lista para reintentar.
    Answer {
        /// ID de la tarea.
        id: String,
        /// Respuesta (o una instrucción adicional para el agente).
        text: String,
    },
    /// Ejecuta una sola tarea; con `--interactive`, abre la sesión del agente.
    Run {
        /// ID de la tarea.
        id: String,
        /// Sesión interactiva: conversas con el agente y, al salir, se valida la salida.
        #[arg(long, short = 'i')]
        interactive: bool,
        /// Agente a usar (por defecto, el activo).
        #[arg(long)]
        agent: Option<String>,
        /// Modelo del agente solo para esta ejecución.
        #[arg(long)]
        model: Option<String>,
    },
    /// Marca una tarea manual como completada tras validar su salida.
    Done {
        /// ID de la tarea.
        id: String,
        /// Marcar como done aunque la validación de salida falle.
        #[arg(long)]
        force: bool,
    },
}

impl TaskCmd {
    /// `true` si el subcomando modifica el estado del proyecto.
    pub fn mutates(&self) -> bool {
        matches!(
            self,
            TaskCmd::Done { .. } | TaskCmd::Answer { .. } | TaskCmd::Run { .. }
        )
    }
}

pub fn run(project: &Project, cmd: TaskCmd) -> Result<()> {
    if let TaskCmd::Show { id }
    | TaskCmd::Done { id, .. }
    | TaskCmd::Answer { id, .. }
    | TaskCmd::Run { id, .. } = &cmd
    {
        if !planner::is_valid_task_id(id) {
            anyhow::bail!(t!("cli.task.invalid_id", id = id));
        }
    }
    match cmd {
        TaskCmd::List { status, format } if format == "json" => {
            let mut tasks = super::status::build(project)?.tasks;
            if let Some(s) = &status {
                tasks.retain(|t| &t.status == s);
            }
            println!("{}", serde_json::to_string_pretty(&tasks)?);
            Ok(())
        }
        TaskCmd::List { status, .. } => list(project, status.as_deref()),
        TaskCmd::Show { id } => show(project, &id),
        TaskCmd::Export { out } => export(project, out.as_deref()),
        TaskCmd::Done { id, force } => done(project, &id, force),
        TaskCmd::Answer { id, text } => answer(project, &id, &text),
        TaskCmd::Run {
            id,
            interactive,
            agent,
            model,
        } => {
            let agent = agent.unwrap_or_else(|| project.config.agent.default.clone());
            if interactive {
                run_interactive(project, &id, &agent, model)
            } else {
                run_one(project, &id, &agent, model)
            }
        }
    }
}

fn task_files(project: &Project) -> Result<Vec<(String, String, std::path::PathBuf)>> {
    let dir = project.tasks_dir();
    let mut out = Vec::new();
    if dir.is_dir() {
        for e in fs::read_dir(&dir)?.flatten() {
            let p = e.path();
            if p.extension().map(|x| x == "md").unwrap_or(false) {
                let text = fs::read_to_string(&p)?;
                if let Some((id, _ty, status)) = planner::task_from_markdown(&text) {
                    out.push((id, status, p));
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

fn list(project: &Project, status: Option<&str>) -> Result<()> {
    let tasks = task_files(project)?;
    if tasks.is_empty() {
        println!("{}", t!("cli.task.no_tasks"));
        return Ok(());
    }
    println!(
        "{:<8} {:<10} {}",
        t!("cli.task.col_task"),
        t!("cli.task.col_status"),
        t!("cli.task.col_file")
    );
    for (id, st, path) in tasks {
        if let Some(s) = status {
            if st != s {
                continue;
            }
        }
        println!("{id:<8} {st:<10} {}", project.rel(&path));
    }
    Ok(())
}

fn show(project: &Project, id: &str) -> Result<()> {
    let path = project.tasks_dir().join(format!("{id}.md"));
    let text =
        fs::read_to_string(&path).with_context(|| t!("core.error.task_not_found", id = id))?;
    println!("{text}");
    Ok(())
}

fn export(project: &Project, out: Option<&str>) -> Result<()> {
    let dir = out
        .map(|o| project.root.join(o))
        .unwrap_or_else(|| project.tasks_dir());
    fs::create_dir_all(&dir)?;
    let tasks = task_files(project)?;
    let mut n = 0;
    for (id, status, path) in tasks {
        if status == "pending" || status == "failed" || status == "manual" {
            let dest = dir.join(format!("{id}.md"));
            if path != dest {
                fs::copy(&path, &dest)?;
            }
            n += 1;
        }
    }
    println!(
        "{}",
        t!("cli.task.exported", count = n, path = dir.display())
    );
    Ok(())
}

fn done(project: &Project, id: &str, force: bool) -> Result<()> {
    let path = project.tasks_dir().join(format!("{id}.md"));
    if !path.is_file() {
        anyhow::bail!(t!("core.error.task_not_found", id = id));
    }
    let text = fs::read_to_string(&path)?;
    let meta = planner::parse_task(&text).with_context(|| t!("cli.task.invalid_frontmatter"))?;

    // Validar la salida antes de cerrar la tarea (RF-050, §14.2).
    let issues = validate::validate_task_output(project, &meta.produce, &meta.optional, None);
    let errors: Vec<_> = issues
        .iter()
        .filter(|i| i.severity == IssueSeverity::Error)
        .collect();
    for i in &issues {
        eprintln!("{}: {}", i.path, i.message);
    }
    if !errors.is_empty() && !force {
        anyhow::bail!(t!(
            "cli.task.validation_failed",
            id = id,
            count = errors.len()
        ));
    }

    archeology_core::fsutil::atomic_write(
        &path,
        planner::set_status_in_markdown(&text, "done").as_bytes(),
    )?;
    let mut state = archeology_core::state::State::load(&project.state_file())?;
    state.tasks.insert(id.to_string(), "done".into());
    state.questions.remove(id);
    // Línea base de las fuentes citadas por la salida validada (RF-120).
    archeology_core::index::refresh_hashes_for(project, &mut state, &meta.produce);
    state.save(&project.state_file())?;
    archeology_core::index::write_mocs(project)?;
    println!("{}", t!("cli.task.marked_done", id = id));
    Ok(())
}

fn task_path(project: &Project, id: &str) -> Result<std::path::PathBuf> {
    let path = project.tasks_dir().join(format!("{id}.md"));
    if !path.is_file() {
        anyhow::bail!(t!("core.error.task_not_found", id = id));
    }
    Ok(path)
}

/// Guarda la respuesta en la tarea (`# Respuestas`) y la deja en `pending`.
/// Solo para tareas en estado `question`: nunca reabre una `done`.
fn answer(project: &Project, id: &str, text: &str) -> Result<()> {
    let path = task_path(project, id)?;
    let content = fs::read_to_string(&path)?;
    let meta = planner::parse_task(&content).with_context(|| t!("cli.task.invalid_frontmatter"))?;
    if meta.status != "question" {
        anyhow::bail!(t!("cli.task.not_question", id = id, status = meta.status));
    }
    let mut state = archeology_core::state::State::load(&project.state_file())?;
    let question = state.questions.remove(id);
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let updated = planner::append_answer(&content, question.as_deref(), text, &date);
    archeology_core::fsutil::atomic_write(
        &path,
        planner::set_status_in_markdown(&updated, "pending").as_bytes(),
    )?;
    state.tasks.insert(id.to_string(), "pending".into());
    state.save(&project.state_file())?;
    println!("{}", t!("cli.task.answer_saved", id = id));
    Ok(())
}

/// Comprueba que el agente puede ejecutar tareas antes de tocar ninguna:
/// definido, aprobado, con un modelo válido y sin problemas de configuración.
fn check_runnable(project: &Project, agent: &str, model: Option<&str>) -> Result<()> {
    let mut adapter = project
        .config
        .agent
        .adapters
        .get(agent)
        .cloned()
        .with_context(|| t!("cli.task.adapter_undefined_in_config", agent = agent))?;
    if let Some(m) = model {
        adapter.model = Some(m.to_string());
    }
    if adapter.mode != archeology_core::config::AdapterMode::Headless {
        return Ok(());
    }
    if !adapter.trusted {
        anyhow::bail!(archeology_core::trust::untrusted_message(agent));
    }
    if let Some(m) = adapter.model.as_deref().filter(|m| !m.is_empty()) {
        archeology_agents::adapter::validate_model(m)?;
    }
    let problems = archeology_agents::adapter_warnings(agent, &adapter);
    if !problems.is_empty() {
        anyhow::bail!(t!(
            "cli.task.invalid_agent_config",
            problems = problems.join("\n  - ")
        ));
    }
    Ok(())
}

/// Ejecuta una sola tarea con el agente (sin importar su estado previo).
///
/// Se valida el agente antes de tocar la tarea, y si la ejecución no llega a
/// correrla (error, interrupción, dependencias sin cumplir) se restaura el
/// archivo y el estado anteriores: una tarea `done` no se pierde por un
/// agente mal configurado.
fn run_one(project: &Project, id: &str, agent: &str, model: Option<String>) -> Result<()> {
    let path = task_path(project, id)?;
    check_runnable(project, agent, model.as_deref())?;
    let text = fs::read_to_string(&path)?;
    let previous_state = archeology_core::state::State::load(&project.state_file())?
        .tasks
        .get(id)
        .cloned();
    archeology_core::fsutil::atomic_write(
        &path,
        planner::set_status_in_markdown(&text, "pending").as_bytes(),
    )?;
    let headless = project
        .config
        .agent
        .adapters
        .get(agent)
        .map(|a| a.mode == archeology_core::config::AdapterMode::Headless)
        .unwrap_or(false);
    let (pb, on_event) = super::inspect::progress(agent, headless);
    let opts = archeology_agents::InspectOptions {
        agent: agent.to_string(),
        only_tasks: vec![id.to_string()],
        module: None,
        resume: false,
        dry_run: false,
        model,
        on_event,
    };
    let report = archeology_agents::run_tasks(project, &opts);
    pb.finish_and_clear();
    // Si sigue en `pending`, la corrida no llegó a cerrarla (error,
    // interrupción o dependencias sin cumplir): vuelve a como estaba.
    let still_pending = fs::read_to_string(&path)
        .ok()
        .and_then(|t| planner::parse_task(&t))
        .is_none_or(|m| m.status == "pending");
    if still_pending {
        restore_task(project, id, &path, &text, previous_state)?;
    }
    super::inspect::print_report(project, &report?, agent, false);
    Ok(())
}

/// Devuelve la tarea al contenido y estado que tenía antes de `task run`.
fn restore_task(
    project: &Project,
    id: &str,
    path: &std::path::Path,
    text: &str,
    previous: Option<String>,
) -> Result<()> {
    archeology_core::fsutil::atomic_write(path, text.as_bytes())?;
    let mut state = archeology_core::state::State::load(&project.state_file())?;
    match previous {
        Some(st) => state.tasks.insert(id.to_string(), st),
        None => state.tasks.remove(id),
    };
    state.save(&project.state_file())?;
    Ok(())
}

/// Abre la sesión interactiva del agente con la tarea como primer mensaje;
/// al salir valida la salida y marca la tarea.
fn run_interactive(project: &Project, id: &str, agent: &str, model: Option<String>) -> Result<()> {
    let path = task_path(project, id)?;
    let text = fs::read_to_string(&path)?;
    let meta = planner::parse_task(&text).with_context(|| t!("cli.task.invalid_frontmatter"))?;
    let mut adapter = project
        .config
        .agent
        .adapters
        .get(agent)
        .cloned()
        .with_context(|| t!("cli.commands.adapter_undefined", agent = agent))?;
    if model.is_some() {
        adapter.model = model;
    }
    if adapter.mode == archeology_core::config::AdapterMode::Headless && !adapter.trusted {
        anyhow::bail!(archeology_core::trust::untrusted_message(agent));
    }
    let builtin = archeology_core::config::builtin_adapters();
    // Proyectos antiguos: sin interactive_args propios, se usan los predefinidos.
    if adapter.interactive_args.is_empty() {
        if let Some(b) = builtin.get(agent).filter(|b| b.command == adapter.command) {
            adapter.interactive_args = b.interactive_args.clone();
        }
    }
    let Some(command) = adapter
        .command
        .clone()
        .filter(|_| !adapter.interactive_args.is_empty())
    else {
        println!("{}", t!("cli.task.no_interactive", agent = agent));
        println!(
            "{}",
            t!("cli.task.open_agent_hint", path = project.root.display())
        );
        println!("  {}", path.display());
        println!("{}", t!("cli.task.when_done_hint", id = id));
        return Ok(());
    };

    let args = archeology_agents::adapter::with_model(
        &adapter,
        // El encabezado del prompt va en el idioma del proyecto, no en el de la interfaz.
        archeology_agents::adapter::expand_args_in(
            project.doc_lang(),
            &adapter.interactive_args,
            &path,
            &text,
        ),
    )?;
    let mut state = archeology_core::state::State::load(&project.state_file())?;
    if let Some(q) = state.questions.get(id) {
        println!("{}", t!("cli.task.pending_question", question = q));
    }
    println!("{}", t!("cli.task.opening", agent = agent, id = id));
    let start = std::time::SystemTime::now();
    let status =
        std::process::Command::new(archeology_agents::adapter::program_for(&adapter, &command))
            .envs(&adapter.env)
            .args(&args)
            .current_dir(&project.root)
            .status()
            .with_context(|| t!("cli.task.launch_failed", command = command))?;
    if !status.success() {
        eprintln!(
            "{}",
            t!("cli.task.agent_exit_status", agent = agent, status = status)
        );
    }

    let issues =
        validate::validate_task_output(project, &meta.produce, &meta.optional, Some(start));
    let errors: Vec<_> = issues
        .iter()
        .filter(|i| i.severity == IssueSeverity::Error)
        .collect();
    for i in &issues {
        eprintln!("{}: {}", i.path, i.message);
    }
    if !errors.is_empty() {
        println!(
            "{}",
            t!(
                "cli.task.interactive_validation_failed",
                id = id,
                count = errors.len()
            )
        );
        return Ok(());
    }
    archeology_core::fsutil::atomic_write(
        &path,
        planner::set_status_in_markdown(&text, "done").as_bytes(),
    )?;
    state.tasks.insert(id.to_string(), "done".into());
    state.questions.remove(id);
    archeology_core::index::refresh_hashes(project, &mut state, start);
    state.save(&project.state_file())?;
    archeology_core::index::write_mocs(project)?;
    println!("{}", t!("cli.task.completed", id = id));
    Ok(())
}
