//! `task` — gestión de tareas en modo manual (RF-041, RF-050).

use anyhow::{Context, Result};
use arqueology_core::frontmatter::IssueSeverity;
use arqueology_core::planner;
use arqueology_core::validate;
use arqueology_core::Project;
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
            anyhow::bail!("id de tarea inválido: '{id}'");
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
        println!("no hay tareas; ejecuta `adoc plan` primero");
        return Ok(());
    }
    println!("{:<8} {:<10} archivo", "tarea", "estado");
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
    let text = fs::read_to_string(&path).with_context(|| format!("la tarea {id} no existe"))?;
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
        "{n} tareas exportadas a {} — ejecútalas con tu agente y marca con `adoc task done <id>`",
        dir.display()
    );
    Ok(())
}

fn done(project: &Project, id: &str, force: bool) -> Result<()> {
    let path = project.tasks_dir().join(format!("{id}.md"));
    if !path.is_file() {
        anyhow::bail!("la tarea {id} no existe");
    }
    let text = fs::read_to_string(&path)?;
    let meta = planner::parse_task(&text).context("frontmatter de tarea inválido")?;

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
        anyhow::bail!(
            "la salida de {id} no supera la validación ({} errores); corrígela o usa --force",
            errors.len()
        );
    }

    fs::write(&path, planner::set_status_in_markdown(&text, "done"))?;
    let mut state = arqueology_core::state::State::load(&project.state_file())?;
    state.tasks.insert(id.to_string(), "done".into());
    state.questions.remove(id);
    // Línea base de las fuentes citadas por la salida validada (RF-120).
    arqueology_core::index::refresh_hashes_for(project, &mut state, &meta.produce);
    state.save(&project.state_file())?;
    arqueology_core::index::write_mocs(project)?;
    println!("tarea {id} marcada como done");
    Ok(())
}

fn task_path(project: &Project, id: &str) -> Result<std::path::PathBuf> {
    let path = project.tasks_dir().join(format!("{id}.md"));
    if !path.is_file() {
        anyhow::bail!("la tarea {id} no existe");
    }
    Ok(path)
}

/// Guarda la respuesta en la tarea (`# Respuestas`) y la deja en `pending`.
fn answer(project: &Project, id: &str, text: &str) -> Result<()> {
    let path = task_path(project, id)?;
    let mut state = arqueology_core::state::State::load(&project.state_file())?;
    let question = state.questions.remove(id);
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let content = fs::read_to_string(&path)?;
    let updated = planner::append_answer(&content, question.as_deref(), text, &date);
    fs::write(&path, planner::set_status_in_markdown(&updated, "pending"))?;
    state.tasks.insert(id.to_string(), "pending".into());
    state.save(&project.state_file())?;
    println!("respuesta guardada en {id}; reintenta con `adoc inspect` o `adoc task run {id}`");
    Ok(())
}

/// Ejecuta una sola tarea con el agente (sin importar su estado previo).
fn run_one(project: &Project, id: &str, agent: &str, model: Option<String>) -> Result<()> {
    let path = task_path(project, id)?;
    let text = fs::read_to_string(&path)?;
    fs::write(&path, planner::set_status_in_markdown(&text, "pending"))?;
    let headless = project
        .config
        .agent
        .adapters
        .get(agent)
        .map(|a| a.mode == arqueology_core::config::AdapterMode::Headless)
        .unwrap_or(false);
    let (pb, on_event) = super::inspect::progress(agent, headless);
    let opts = arqueology_agents::InspectOptions {
        agent: agent.to_string(),
        only_tasks: vec![id.to_string()],
        module: None,
        resume: false,
        dry_run: false,
        model,
        on_event,
    };
    let report = arqueology_agents::run_tasks(project, &opts);
    pb.finish_and_clear();
    super::inspect::print_report(project, &report?, agent, false);
    Ok(())
}

/// Abre la sesión interactiva del agente con la tarea como primer mensaje;
/// al salir valida la salida y marca la tarea.
fn run_interactive(project: &Project, id: &str, agent: &str, model: Option<String>) -> Result<()> {
    let path = task_path(project, id)?;
    let text = fs::read_to_string(&path)?;
    let meta = planner::parse_task(&text).context("frontmatter de tarea inválido")?;
    let mut adapter = project
        .config
        .agent
        .adapters
        .get(agent)
        .cloned()
        .with_context(|| format!("adaptador '{agent}' no definido"))?;
    if model.is_some() {
        adapter.model = model;
    }
    if adapter.mode == arqueology_core::config::AdapterMode::Headless && !adapter.trusted {
        anyhow::bail!(arqueology_core::trust::untrusted_message(agent));
    }
    let builtin = arqueology_core::config::builtin_adapters();
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
        println!("'{agent}' no tiene modo interactivo configurado.");
        println!(
            "Abre tu agente en {} y dale la tarea:",
            project.root.display()
        );
        println!("  {}", path.display());
        println!("Al terminar: adoc task done {id}");
        return Ok(());
    };

    let args = arqueology_agents::adapter::with_model(
        &adapter,
        arqueology_agents::adapter::expand_args(&adapter.interactive_args, &path, &text),
    );
    let mut state = arqueology_core::state::State::load(&project.state_file())?;
    if let Some(q) = state.questions.get(id) {
        println!("Pregunta pendiente del agente: {q}");
    }
    println!("Abriendo {agent} con la tarea {id}. Cuando termines, sal del agente para validar la salida.\n");
    let start = std::time::SystemTime::now();
    let status = std::process::Command::new(&command)
        .args(&args)
        .current_dir(&project.root)
        .status()
        .with_context(|| format!("no se pudo lanzar '{command}'"))?;
    if !status.success() {
        eprintln!("aviso: {agent} terminó con {status}");
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
            "\n✗ {id}: la salida no supera la validación ({} errores). Reintenta con \
             `adoc task run {id} --interactive` o ciérrala con `adoc task done {id} --force`.",
            errors.len()
        );
        return Ok(());
    }
    fs::write(&path, planner::set_status_in_markdown(&text, "done"))?;
    state.tasks.insert(id.to_string(), "done".into());
    state.questions.remove(id);
    arqueology_core::index::refresh_hashes(project, &mut state, start);
    state.save(&project.state_file())?;
    arqueology_core::index::write_mocs(project)?;
    println!("\n✓ {id} completada");
    Ok(())
}
