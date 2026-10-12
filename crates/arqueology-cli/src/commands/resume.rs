//! `resume` — retoma la última inspección pendiente (RF-174, CU-13).

use anyhow::{Context, Result};
use arqueology_agents::InspectOptions;
use arqueology_core::Project;
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct ResumeArgs {
    /// Nombre o ID del proyecto a retomar.
    project: Option<String>,
    /// Adaptador a usar.
    #[arg(long)]
    agent: Option<String>,
    /// Modelo del agente solo para esta corrida.
    #[arg(long)]
    model: Option<String>,
}

pub fn run(args: ResumeArgs, project_flag: Option<String>) -> Result<()> {
    let name_or_id = args.project.clone().or(project_flag);
    let project = match name_or_id {
        Some(id) => {
            let reg = arqueology_registry::Registry::load()?;
            let entry = reg
                .find(&id)
                .with_context(|| format!("proyecto '{id}' no registrado"))?;
            Project::open(&PathBuf::from(&entry.path))?
        }
        None => pick_pending()?,
    };

    super::touch_registry(&project, "resume");
    let agent = args
        .agent
        .unwrap_or_else(|| project.config.agent.default.clone());
    println!(
        "retomando {} (agente: {agent})",
        project.config.project.name
    );
    let headless = project
        .config
        .agent
        .adapters
        .get(&agent)
        .map(|a| a.mode == arqueology_core::config::AdapterMode::Headless)
        .unwrap_or(false);
    let (pb, on_event) = super::inspect::progress(&agent, headless);
    let opts = InspectOptions {
        model: args.model.clone(),
        on_event,
        agent,
        only_tasks: vec![],
        module: None,
        resume: true,
        dry_run: false,
    };
    let _lock = project.lock("resume")?;
    let report = arqueology_agents::run_tasks(&project, &opts);
    pb.finish_and_clear();
    let report = report?;
    super::inspect::print_report(&project, &report, &opts.agent, false);
    Ok(())
}

/// Elige entre los proyectos registrados con trabajo pendiente.
fn pick_pending() -> Result<Project> {
    let reg = arqueology_registry::Registry::load()?;
    let mut candidates = Vec::new();
    for entry in &reg.projects {
        let Ok(project) = Project::open(&PathBuf::from(&entry.path)) else {
            continue;
        };
        let tasks_dir = project.tasks_dir();
        if tasks_dir.is_dir() {
            let pending = std::fs::read_dir(&tasks_dir)?
                .flatten()
                .filter(|e| e.path().extension().map(|x| x == "md").unwrap_or(false))
                .filter(|e| {
                    std::fs::read_to_string(e.path())
                        .map(|t| t.contains("status: pending") || t.contains("status: failed"))
                        .unwrap_or(false)
                })
                .count();
            if pending > 0 {
                candidates.push((entry.clone(), pending));
            }
        }
    }
    match candidates.len() {
        0 => anyhow::bail!("no hay proyectos con trabajo pendiente"),
        1 => {
            let (entry, _) = candidates.into_iter().next().unwrap();
            Ok(Project::open(&PathBuf::from(&entry.path))?)
        }
        _ => {
            println!("proyectos con trabajo pendiente:");
            for (e, n) in &candidates {
                println!("  {} ({n} tareas) — {}", e.name, e.path);
            }
            let choice = super::ask("¿Cuál retomar?", &candidates[0].0.name)?;
            let entry = candidates
                .into_iter()
                .find(|(e, _)| e.name == choice || e.id == choice)
                .map(|(e, _)| e)
                .context("elección inválida")?;
            Ok(Project::open(&PathBuf::from(&entry.path))?)
        }
    }
}
