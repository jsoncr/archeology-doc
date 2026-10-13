//! `resume` — retoma la última inspección pendiente (RF-174, CU-13).

use anyhow::{Context, Result};
use archeology_agents::InspectOptions;
use archeology_core::{t, Project};
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
            let reg = archeology_registry::Registry::load()?;
            let entry = reg
                .resolve(&id)?
                .with_context(|| t!("cli.resume.project_not_registered", name = id))?;
            Project::open(&PathBuf::from(&entry.path))?
        }
        None => pick_pending()?,
    };

    super::touch_registry(&project, "resume");
    let agent = args
        .agent
        .unwrap_or_else(|| project.config.agent.default.clone());
    println!(
        "{}",
        t!(
            "cli.resume.resuming",
            project = project.config.project.name,
            agent = agent
        )
    );
    let headless = project
        .config
        .agent
        .adapters
        .get(&agent)
        .map(|a| a.mode == archeology_core::config::AdapterMode::Headless)
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
    let report = archeology_agents::run_tasks(&project, &opts);
    pb.finish_and_clear();
    let report = report?;
    super::inspect::print_report(&project, &report, &opts.agent, false);
    Ok(())
}

/// Elige entre los proyectos registrados con trabajo pendiente.
fn pick_pending() -> Result<Project> {
    let reg = archeology_registry::Registry::load()?;
    let mut candidates = Vec::new();
    for entry in &reg.projects {
        let Ok(project) = Project::open(&PathBuf::from(&entry.path)) else {
            continue;
        };
        let n = pending_tasks(&project);
        if n > 0 {
            candidates.push((entry.clone(), n));
        }
    }
    match candidates.len() {
        0 => anyhow::bail!(t!("cli.resume.no_pending")),
        1 => {
            let (entry, _) = candidates.into_iter().next().unwrap();
            Ok(Project::open(&PathBuf::from(&entry.path))?)
        }
        _ => {
            println!("{}", t!("cli.resume.pending_projects"));
            for (e, n) in &candidates {
                println!(
                    "{}",
                    t!(
                        "cli.resume.pending_project",
                        name = e.name,
                        count = n,
                        path = e.path
                    )
                );
            }
            let choice = super::ask(&t!("cli.resume.which"), &candidates[0].0.name)?;
            let entry = candidates
                .into_iter()
                .find(|(e, _)| e.name == choice || e.id == choice)
                .map(|(e, _)| e)
                .with_context(|| t!("cli.resume.invalid_choice"))?;
            Ok(Project::open(&PathBuf::from(&entry.path))?)
        }
    }
}

/// Tareas que `resume` retomaría: `pending`, `failed`, `running` (corrida
/// interrumpida) y, si el agente activo es headless, `manual`. Se lee el
/// frontmatter, no se busca texto suelto en el archivo.
fn pending_tasks(project: &Project) -> usize {
    let headless = project
        .config
        .agent
        .adapters
        .get(&project.config.agent.default)
        .is_some_and(|a| a.mode == archeology_core::config::AdapterMode::Headless);
    let Ok(entries) = std::fs::read_dir(project.tasks_dir()) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| e.path().extension().map(|x| x == "md").unwrap_or(false))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|t| archeology_core::planner::parse_task(&t))
        .filter(|m| is_resumable(&m.status, headless))
        .count()
}

fn is_resumable(status: &str, headless: bool) -> bool {
    matches!(status, "pending" | "failed" | "running") || (headless && status == "manual")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resumable_statuses() {
        assert!(is_resumable("running", false));
        assert!(is_resumable("failed", false));
        assert!(!is_resumable("done", true));
        assert!(!is_resumable("question", true));
        assert!(!is_resumable("manual", false));
        assert!(is_resumable("manual", true));
    }
}
