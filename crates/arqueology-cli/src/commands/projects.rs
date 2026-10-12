//! `projects` — gestión del registro global (§8.15).

use anyhow::{Context, Result};
use arqueology_registry::{discover, Registry};
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
    /// Descubre proyectos con arqueology-doc.toml en un árbol.
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

fn row(entry: &arqueology_registry::RegistryEntry) -> ProjectRow {
    let project = arqueology_core::Project::open(&PathBuf::from(&entry.path)).ok();
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
        arqueology_core::state::State::load(&p.state_file())
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
        println!("no hay proyectos registrados; `adoc init` los registra automáticamente");
        return Ok(());
    }
    println!(
        "{:<20} {:>5} {:>9} {:>6} {:<8} {:<10} {:<16} ruta",
        "nombre", "plan", "hallazgos", "specs", "agente", "último cmd", "actividad"
    );
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
            if r.available { "" } else { "  (no encontrado)" }
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
    let dir = arqueology_core::paths::canonicalize(Path::new(path)).context("ruta inválida")?;
    let cfg = arqueology_core::config::ArqueologyConfig::load(
        &dir.join(arqueology_core::config::CONFIG_FILE),
    )
    .context("no hay arqueology-doc.toml en esa ruta")?;
    Registry::update(|reg| reg.upsert(&cfg.project.id, &cfg.project.name, &dir, "add"))?;
    println!("registrado {} ({})", cfg.project.name, dir.display());
    Ok(())
}

fn discover_cmd(dir: &str, depth: usize) -> Result<()> {
    let root = arqueology_core::paths::canonicalize(Path::new(dir)).context("ruta inválida")?;
    let found = discover(&root, depth);
    if found.is_empty() {
        println!(
            "no se encontraron proyectos con arqueology-doc.toml bajo {}",
            root.display()
        );
        return Ok(());
    }
    let mut configs = Vec::new();
    for p in &found {
        let cfg = arqueology_core::config::ArqueologyConfig::load(
            &p.join(arqueology_core::config::CONFIG_FILE),
        )?;
        println!("+ {} ({})", cfg.project.name, p.display());
        configs.push((p, cfg));
    }
    Registry::update(|reg| {
        for (p, cfg) in &configs {
            reg.upsert(&cfg.project.id, &cfg.project.name, p, "discover");
        }
    })?;
    println!("{} proyecto(s) registrados", found.len());
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
        .filter_map(|p| arqueology_core::paths::find_workspace_by_id(&p.id))
        .collect();
    if gone.is_empty() {
        println!("nada que podar");
        return Ok(());
    }
    println!("entradas huérfanas:");
    for g in &gone {
        println!("  - {g}");
    }
    for w in &workspaces {
        println!("  - espacio de trabajo {}", w.display());
    }
    if !super::confirm("¿Eliminarlas del registro junto con su estado?", yes)? {
        println!("cancelado");
        return Ok(());
    }
    // Se recalcula bajo bloqueo por si el registro cambió mientras se confirmaba.
    let removed = Registry::update(|reg| reg.prune().len())?;
    for w in &workspaces {
        std::fs::remove_dir_all(w)?;
    }
    println!(
        "{removed} entrada(s) eliminadas, {} espacio(s) de trabajo borrados",
        workspaces.len()
    );
    Ok(())
}
