//! Despacho de comandos y resolución de proyecto.

use anyhow::{bail, Context, Result};
use arqueology_core::Project;
use std::path::PathBuf;

pub mod agent;
pub mod ask;
pub mod check;
pub mod doctor;
pub mod graph;
pub mod init;
pub mod inspect;
pub mod plan;
pub mod projects;
pub mod report;
pub mod resume;
pub mod review;
pub mod scan;
pub mod site;
pub mod spec;
pub mod status;
pub mod sync;
pub mod task;
pub mod toolchain;

/// Resuelve el proyecto objetivo: `--project <nombre|id>` contra el registro,
/// o descubrimiento ascendente desde el directorio actual (RF-173).
pub fn resolve_project(project_arg: &Option<String>) -> Result<Project> {
    if let Some(name_or_id) = project_arg {
        let registry = arqueology_registry::Registry::load()?;
        let entry = registry
            .find(name_or_id)
            .with_context(|| format!("proyecto '{name_or_id}' no está en el registro global"))?;
        return Ok(Project::open(&PathBuf::from(&entry.path))?);
    }
    Ok(Project::discover(&std::env::current_dir()?)?)
}

/// Registra actividad del proyecto en el registro global (tolerante a fallos).
pub fn touch_registry(project: &Project, command: &str) {
    let result = arqueology_registry::Registry::update(|reg| {
        reg.upsert(
            &project.config.project.id,
            &project.config.project.name,
            &project.root,
            command,
        )
    });
    if let Err(e) = result {
        tracing::warn!("no se pudo actualizar el registro global: {e}");
    }
}

pub fn run(command: super::Commands, project_arg: Option<String>) -> Result<()> {
    use super::Commands::*;
    match command {
        Init(args) => init::run(args),
        Scan(args) => {
            let p = resolve_project(&project_arg)?;
            touch_registry(&p, "scan");
            let _lock = p.lock("scan")?;
            scan::run(&p, args)
        }
        Plan(args) => {
            let p = resolve_project(&project_arg)?;
            touch_registry(&p, "plan");
            let _lock = p.lock("plan")?;
            plan::run(&p, args)
        }
        Inspect(args) => {
            let p = resolve_project(&project_arg)?;
            touch_registry(&p, "inspect");
            let _lock = p.lock("inspect")?;
            inspect::run(&p, args)
        }
        Task { cmd } => {
            let p = resolve_project(&project_arg)?;
            touch_registry(&p, "task");
            // Solo los subcomandos que cambian el estado; `task list` y
            // `task show` siguen disponibles durante un `inspect`.
            let _lock = cmd.mutates().then(|| p.lock("task")).transpose()?;
            task::run(&p, cmd)
        }
        Review(args) => {
            let p = resolve_project(&project_arg)?;
            touch_registry(&p, "review");
            review::run(&p, args)
        }
        Check(args) => {
            let p = resolve_project(&project_arg)?;
            touch_registry(&p, "check");
            let code = check::run(&p, args)?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        Sync(args) => {
            let p = resolve_project(&project_arg)?;
            touch_registry(&p, "sync");
            let _lock = p.lock("sync")?;
            sync::run(&p, args)
        }
        Site {
            cmd: site::SiteCmd::Setup { version },
        } => site::setup(&version),
        Site { cmd } => {
            let p = resolve_project(&project_arg)?;
            touch_registry(&p, "site");
            site::run(&p, cmd)
        }
        Agent { cmd } => agent::run(cmd, project_arg),
        Spec { cmd } => {
            let p = resolve_project(&project_arg)?;
            touch_registry(&p, "spec");
            let _lock = p.lock("spec")?;
            spec::run(&p, cmd)
        }
        Projects { cmd } => projects::run(cmd),
        Resume(args) => resume::run(args, project_arg),
        Preview(args) => {
            let p = resolve_project(&project_arg)?;
            touch_registry(&p, "preview");
            site::preview(&p, args.port, args.no_open)
        }
        Report(args) => {
            let p = resolve_project(&project_arg)?;
            touch_registry(&p, "report");
            report::run(&p, args)
        }
        Graph(args) => {
            let p = resolve_project(&project_arg)?;
            graph::run(&p, args)
        }
        Ask(args) => {
            let p = resolve_project(&project_arg)?;
            touch_registry(&p, "ask");
            ask::run(&p, args)
        }
        Status(args) => {
            let p = resolve_project(&project_arg)?;
            status::run(&p, args)
        }
        Doctor(args) => {
            let code = doctor::run(args, project_arg)?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
    }
}

/// Confirma con el usuario salvo `--yes` o entrada no interactiva.
pub fn confirm(prompt: &str, assume_yes: bool) -> Result<bool> {
    if assume_yes {
        return Ok(true);
    }
    if !atty_is_interactive() {
        bail!("operación requiere confirmación; usa --yes en modo no interactivo");
    }
    eprint!("{prompt} [s/N] ");
    use std::io::Write;
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(matches!(
        line.trim().to_lowercase().as_str(),
        "s" | "si" | "sí" | "y" | "yes"
    ))
}

fn atty_is_interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

/// Pregunta de texto simple en modo interactivo.
pub fn ask(prompt: &str, default: &str) -> Result<String> {
    if !atty_is_interactive() {
        return Ok(default.to_string());
    }
    if default.is_empty() {
        eprint!("{prompt}: ");
    } else {
        eprint!("{prompt} [{default}]: ");
    }
    use std::io::Write;
    std::io::stderr().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let v = line.trim();
    Ok(if v.is_empty() {
        default.to_string()
    } else {
        v.to_string()
    })
}
