//! `projects` — gestión del registro global (§8.15).

use anyhow::{Context, Result};
use archeology_core::t;
use archeology_registry::{discover, Registry};
use clap::Subcommand;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum ProjectsCmd {
    /// Lista los proyectos registrados.
    List {
        /// Formato de salida.
        #[arg(long, default_value = "text", value_parser = ["text", "json"])]
        format: String,
    },
    /// Registra un proyecto existente.
    Add {
        /// Ruta del proyecto.
        path: String,
    },
    /// Descubre proyectos con archeology-doc.toml en un árbol.
    Discover {
        /// Directorio raíz de la búsqueda.
        dir: String,
        /// Profundidad máxima.
        #[arg(long, default_value_t = 4)]
        depth: usize,
    },
    /// Elimina entradas cuyo directorio ya no existe.
    Prune {
        /// Sin pedir confirmación.
        #[arg(long)]
        yes: bool,
    },
}

pub fn run(cmd: ProjectsCmd) -> Result<()> {
    match cmd {
        ProjectsCmd::List { format } => list(&format),
        ProjectsCmd::Add { path } => add(&path),
        ProjectsCmd::Discover { dir, depth } => discover_cmd(&dir, depth),
        ProjectsCmd::Prune { yes } => prune(yes),
    }
}

/// Fila de `projects list`: el registro es solo un índice; el resto se lee
/// del estado de cada proyecto (RF-172, RF-178).
#[derive(serde::Serialize)]
struct ProjectRow {
    id: String,
    name: String,
    path: String,
    last_activity: String,
    last_command: String,
    available: bool,
    progress: Option<usize>,
    open_findings: Option<usize>,
    compliance: Option<String>,
    agent: Option<String>,
}

fn row(entry: &archeology_registry::RegistryEntry) -> ProjectRow {
    let project = archeology_core::Project::open(&PathBuf::from(&entry.path)).ok();
    let report = project
        .as_ref()
        .and_then(|p| super::review::build_report(p, None).ok());
    let compliance = report.as_ref().and_then(|r| {
        let os = r.openspec.as_ref()?;
        let total = os["requirements"].as_u64()?;
        let implemented = os["by_status"]["implemented"].as_u64().unwrap_or(0);
        Some(format!("{implemented}/{total}"))
    });
    let agent = project.as_ref().map(|p| {
        archeology_core::state::State::load(&p.state_file())
            .ok()
            .and_then(|s| s.runs.last().map(|r| r.agent.clone()))
            .unwrap_or_else(|| p.config.agent.default.clone())
    });
    ProjectRow {
        id: entry.id.clone(),
        name: entry.name.clone(),
        path: entry.path.clone(),
        last_activity: entry.last_activity.clone(),
        last_command: entry.last_command.clone(),
        available: project.is_some(),
        progress: report.as_ref().map(|r| r.progress),
        open_findings: report.as_ref().map(|r| r.open_findings()),
        compliance,
        agent,
    }
}

fn list(format: &str) -> Result<()> {
    let reg = Registry::load()?;
    let rows: Vec<ProjectRow> = reg.projects.iter().map(row).collect();
    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("{}", t!("cli.projects.none"));
        return Ok(());
    }
    println!(
        "{:<20} {:>5} {:>9} {:>6} {:<8} {:<10} {:<16} {}",
        t!("cli.projects.col_name"),
        t!("cli.projects.col_plan"),
        t!("cli.projects.col_findings"),
        t!("cli.projects.col_specs"),
        t!("cli.projects.col_agent"),
        t!("cli.projects.col_last_command"),
        t!("cli.projects.col_activity"),
        t!("cli.projects.col_path")
    );
    let not_found = t!("cli.projects.not_found");
    for r in &rows {
        let dash = || "—".to_string();
        println!(
            "{:<20} {:>5} {:>9} {:>6} {:<8} {:<10} {:<16} {}{}",
            truncate(&r.name, 20),
            r.progress.map(|p| format!("{p}%")).unwrap_or_else(dash),
            r.open_findings.map(|n| n.to_string()).unwrap_or_else(dash),
            r.compliance.clone().unwrap_or_else(dash),
            truncate(r.agent.as_deref().unwrap_or("—"), 8),
            r.last_command,
            truncate(&r.last_activity.replace('T', " "), 16),
            r.path,
            if r.available { "" } else { not_found.as_str() }
        );
    }
    Ok(())
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    } else {
        s.to_string()
    }
}

fn add(path: &str) -> Result<()> {
    let dir = archeology_core::paths::canonicalize(Path::new(path))
        .with_context(|| t!("cli.projects.invalid_path"))?;
    let cfg = archeology_core::config::ArcheologyConfig::load(
        &dir.join(archeology_core::config::CONFIG_FILE),
    )
    .with_context(|| t!("cli.projects.no_config"))?;
    Registry::update(|reg| reg.upsert(&cfg.project.id, &cfg.project.name, &dir, "add"))?;
    println!(
        "{}",
        t!(
            "cli.projects.registered",
            name = cfg.project.name,
            path = dir.display()
        )
    );
    Ok(())
}

fn discover_cmd(dir: &str, depth: usize) -> Result<()> {
    let root = archeology_core::paths::canonicalize(Path::new(dir))
        .with_context(|| t!("cli.projects.invalid_path"))?;
    let found = discover(&root, depth);
    if found.is_empty() {
        println!("{}", t!("cli.projects.none_found", path = root.display()));
        return Ok(());
    }
    let mut configs = Vec::new();
    for p in &found {
        let cfg = archeology_core::config::ArcheologyConfig::load(
            &p.join(archeology_core::config::CONFIG_FILE),
        )?;
        println!("+ {} ({})", cfg.project.name, p.display());
        configs.push((p, cfg));
    }
    Registry::update(|reg| {
        for (p, cfg) in &configs {
            reg.upsert(&cfg.project.id, &cfg.project.name, p, "discover");
        }
    })?;
    println!("{}", t!("cli.projects.discovered", count = found.len()));
    Ok(())
}

fn prune(yes: bool) -> Result<()> {
    let reg = Registry::load()?;
    let orphans: Vec<_> = reg
        .projects
        .iter()
        .filter(|p| !std::path::Path::new(&p.path).is_dir())
        .collect();
    let gone: Vec<String> = orphans
        .iter()
        .map(|p| format!("{} ({})", p.name, p.path))
        .collect();
    // Su espacio de trabajo (tareas, ejecuciones) también queda huérfano.
    let workspaces: Vec<PathBuf> = orphans
        .iter()
        .filter_map(|p| archeology_core::paths::find_workspace_by_id(&p.id))
        .collect();
    if gone.is_empty() {
        println!("{}", t!("cli.projects.nothing_to_prune"));
        return Ok(());
    }
    println!("{}", t!("cli.projects.orphans"));
    for g in &gone {
        println!("  - {g}");
    }
    for w in &workspaces {
        println!("{}", t!("cli.projects.workspace", path = w.display()));
    }
    if !super::confirm(&t!("cli.projects.confirm_prune"), yes)? {
        println!("{}", t!("cli.projects.cancelled"));
        return Ok(());
    }
    // Se recalcula bajo bloqueo por si el registro cambió mientras se confirmaba.
    let removed = Registry::update(|reg| reg.prune().len())?;
    for w in &workspaces {
        std::fs::remove_dir_all(w)?;
    }
    println!(
        "{}",
        t!(
            "cli.projects.pruned",
            removed = removed,
            workspaces = workspaces.len()
        )
    );
    Ok(())
}
