//! `spec` — integración con OpenSpec (§8.14).

use anyhow::{Context, Result};
use archeology_core::model::{Inventory, Task, TaskStatus, TaskType};
use archeology_core::planner;
use archeology_core::{t, tl, Project};
use archeology_openspec::{ComplianceSummary, SpecProject};
use clap::Subcommand;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum SpecCmd {
    /// Estado de cumplimiento de requisitos (RF-150, RF-163).
    Status {
        /// Solo esta capability.
        #[arg(long)]
        capability: Option<String>,
        /// Formato de salida.
        #[arg(long, default_value = "text", value_parser = ["text", "json"])]
        format: String,
    },
    /// Prepara specs base inferidas como cambio de OpenSpec (RF-157).
    ///
    /// Crea `openspec/changes/baseline-<fecha>/` y una tarea para que el
    /// agente escriba los deltas de requisitos inferidos.
    Baseline {
        /// Solo esta capability (módulo del inventario).
        #[arg(long)]
        capability: Option<String>,
        /// Mostrar qué se generaría sin escribir.
        #[arg(long)]
        dry_run: bool,
    },
    /// Valida un cambio de OpenSpec (CLI oficial o validación estructural, RF-159).
    Validate {
        /// Nombre del cambio (carpeta en openspec/changes/).
        change: String,
    },
}

/// Carga las specs del proyecto (None si no hay OpenSpec).
pub fn load_spec(project: &Project) -> Result<Option<SpecProject>> {
    let rel = PathBuf::from(&project.config.openspec.path);
    archeology_openspec::load(&project.root, &rel)
}

/// Cumplimiento cruzando specs con los `spec-report` del vault.
pub fn compliance(project: &Project, spec: &SpecProject) -> ComplianceSummary {
    let reports = archeology_openspec::load_reports(&project.knowledge_dir().join("specs"));
    archeology_openspec::compliance(spec, &reports, project.config.openspec.stale_change_days)
}

pub fn run(project: &Project, cmd: SpecCmd) -> Result<()> {
    match cmd {
        SpecCmd::Status { capability, format } => status(project, capability, &format),
        SpecCmd::Baseline {
            capability,
            dry_run,
        } => baseline(project, capability.as_deref(), dry_run),
        SpecCmd::Validate { change } => {
            let dir = project
                .config
                .openspec_dir(&project.root)
                .join("changes")
                .join(&change);
            if !dir.is_dir() {
                anyhow::bail!(t!("cli.spec.change_not_found", path = dir.display()));
            }
            if !validate_change(project, &change, &dir) {
                anyhow::bail!(t!("cli.spec.change_invalid", change = change));
            }
            println!("{}", t!("cli.spec.change_valid", change = change));
            Ok(())
        }
    }
}

fn status(project: &Project, capability: Option<String>, format: &str) -> Result<()> {
    let Some(mut spec) = load_spec(project)? else {
        println!("{}", t!("cli.spec.no_openspec"));
        println!("{}", t!("cli.spec.baseline_hint"));
        return Ok(());
    };
    if let Some(c) = &capability {
        spec.capabilities.retain(|cap| &cap.name == c);
    }
    let summary = compliance(project, &spec);
    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }
    for (cap, reqs) in &summary.requirements {
        println!(
            "{}",
            t!("cli.spec.capability", name = cap, count = reqs.len())
        );
        for r in reqs {
            let evidence = if r.evidence.is_empty() {
                String::new()
            } else {
                format!(" — {}", r.evidence.join(", "))
            };
            println!("  [{:<12}] {} {}{evidence}", r.status, r.id, r.title);
        }
    }
    println!("\n{}", t!("cli.spec.total", counts = fmt_counts(&summary)));
    if !spec.active_changes.is_empty() {
        println!("\n{}", t!("cli.spec.active_changes"));
        for ch in &spec.active_changes {
            let stale = if ch.is_stale(project.config.openspec.stale_change_days) {
                t!(
                    "cli.spec.stale_change",
                    days = ch.days_since_update.unwrap_or_default()
                )
            } else {
                String::new()
            };
            println!(
                "{}",
                t!(
                    "cli.spec.change_tasks",
                    name = ch.name,
                    done = ch.tasks_done,
                    total = ch.tasks_total,
                    stale = stale
                )
            );
        }
    }
    if !summary.unknown_ids.is_empty() {
        println!(
            "\n{}",
            t!("cli.spec.unknown_ids", ids = summary.unknown_ids.join(", "))
        );
    }
    if summary.totals.contains_key("unevaluated") {
        println!("\n{}", t!("cli.spec.unevaluated"));
    }
    Ok(())
}

/// `implemented=1  contradicted=1 ...`
pub fn fmt_counts(summary: &ComplianceSummary) -> String {
    summary
        .totals
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("  ")
}

fn baseline(project: &Project, capability: Option<&str>, dry_run: bool) -> Result<()> {
    if project.config.openspec.baseline == "off" {
        anyhow::bail!(t!("cli.spec.baseline_off"));
    }
    let inv: Inventory = serde_json::from_str(
        &fs::read_to_string(project.inventory_file())
            .with_context(|| t!("cli.spec.no_inventory"))?,
    )?;
    let openspec_dir = project.config.openspec_dir(&project.root);
    let specified: Vec<String> = load_spec(project)?
        .map(|s| s.capabilities.into_iter().map(|c| c.name).collect())
        .unwrap_or_default();

    // Granularidad: una capability por módulo del inventario (§20 pendiente).
    let modules: Vec<_> = inv
        .modules
        .iter()
        .filter(|m| capability.map(|c| c == m.name).unwrap_or(true))
        .filter(|m| !specified.contains(&m.name))
        .collect();
    if modules.is_empty() {
        anyhow::bail!(t!("cli.spec.no_modules"));
    }

    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let change = format!("baseline-{date}");
    let change_dir = openspec_dir.join("changes").join(&change);
    let change_rel = project.rel(&change_dir);
    let names: Vec<&str> = modules.iter().map(|m| m.name.as_str()).collect();

    if dry_run {
        println!(
            "{}",
            t!(
                "cli.spec.dry_run",
                path = change_rel,
                names = names.join(", ")
            )
        );
        println!("{}", t!("cli.spec.dry_run_files"));
        return Ok(());
    }
    if change_dir.exists() {
        anyhow::bail!(t!("cli.spec.change_exists", path = change_rel));
    }

    // RF-161: nunca escribir en openspec/specs/; solo en changes/.
    archeology_core::fsutil::create_dir_in_root(&project.root, &change_dir.join("specs"))?;
    // Contenido generado: idioma de la documentación del proyecto.
    // `## Why` y `## What Changes` son encabezados de OpenSpec (no se traducen).
    let lang = project.doc_lang();
    let mut proposal = format!(
        "# {}\n\n## Why\n\n{}\n\n## What Changes\n\n",
        tl!(lang, "doc.spec_baseline.title"),
        tl!(lang, "doc.spec_baseline.why")
    );
    for n in &names {
        proposal.push_str(&tl!(lang, "doc.spec_baseline.what_item", name = n));
        proposal.push('\n');
    }
    archeology_core::fsutil::write_in_root(
        &project.root,
        &change_dir.join("proposal.md"),
        proposal.as_bytes(),
    )?;
    archeology_core::fsutil::write_in_root(
        &project.root,
        &change_dir.join("tasks.md"),
        tl!(lang, "doc.spec_baseline.tasks").as_bytes(),
    )?;

    let task = baseline_task(project, &inv, &modules, &change_rel, &date)?;
    let task_path = project.tasks_dir().join(format!("{}.md", task.task_id));
    fs::create_dir_all(project.tasks_dir())?;
    archeology_core::fsutil::atomic_write(
        &task_path,
        planner::task_to_markdown(&task, &planner::TaskRules::from_config(&project.config))
            .as_bytes(),
    )?;

    println!(
        "{}",
        t!(
            "cli.spec.change_ready",
            path = change_rel,
            names = names.join(", ")
        )
    );
    println!(
        "{}",
        t!("cli.spec.task_created", id = task.task_id, change = change)
    );
    if !openspec_dir.join("specs").is_dir() {
        println!("{}", t!("cli.spec.openspec_init_hint"));
    }
    Ok(())
}

fn baseline_task(
    project: &Project,
    _inv: &Inventory,
    modules: &[&archeology_core::model::ModuleInfo],
    change_rel: &str,
    date: &str,
) -> Result<Task> {
    // Depende de las tareas de módulo existentes: reutiliza su documentación.
    let mut depends_on = Vec::new();
    let mut read = Vec::new();
    if project.tasks_dir().is_dir() {
        for e in fs::read_dir(project.tasks_dir())?.flatten() {
            if let Some(meta) = fs::read_to_string(e.path())
                .ok()
                .and_then(|t| planner::parse_task(&t))
            {
                let wanted = modules
                    .iter()
                    .any(|x| planner::covers_module(meta.module.as_deref(), &x.name));
                if meta.kind == "module" && wanted {
                    depends_on.push(meta.task_id);
                }
            }
        }
    }
    depends_on.sort();
    let know = project.config.paths.knowledge.trim_end_matches('/');
    for m in modules {
        read.push(if m.path == "." {
            "./*".to_string()
        } else if m.recursive {
            format!("{}/**", m.path)
        } else {
            format!("{}/*", m.path)
        });
        read.push(format!("{know}/modules/{}.md", m.name));
    }
    read.push(format!("{know}/features/"));
    let names: Vec<&str> = modules.iter().map(|m| m.name.as_str()).collect();
    Ok(Task {
        task_id: format!("T-BASE-{}", date.replace('-', "")),
        kind: TaskType::SpecBaseline,
        module: None,
        status: TaskStatus::Pending,
        depends_on,
        read,
        produce: vec![format!("{change_rel}/specs/")],
        objective: tl!(
            project.doc_lang(),
            "doc.spec_baseline.objective",
            names = names.join(", "),
            change = change_rel
        ),
    })
}

/// Valida un cambio con `openspec validate --strict` o, si el CLI no está
/// disponible o está desactivado, con la validación estructural propia.
fn validate_change(project: &Project, change: &str, dir: &Path) -> bool {
    if project.config.openspec.validate_with_cli {
        let out = std::process::Command::new("openspec")
            .args(["validate", change, "--strict", "--no-interactive"])
            .env("OPENSPEC_TELEMETRY", "0")
            .current_dir(&project.root)
            .output();
        if let Ok(out) = out {
            print!("{}", String::from_utf8_lossy(&out.stdout));
            eprint!("{}", String::from_utf8_lossy(&out.stderr));
            return out.status.success();
        }
        eprintln!("{}", t!("cli.spec.cli_unavailable"));
    }
    let issues = archeology_openspec::validate_change_structure(dir);
    for i in &issues {
        eprintln!("error: {i}");
    }
    issues.is_empty()
}
