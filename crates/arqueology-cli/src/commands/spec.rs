//! `spec` — integración con OpenSpec (§8.14).

use anyhow::{Context, Result};
use arqueology_core::model::{Inventory, Task, TaskStatus, TaskType};
use arqueology_core::planner;
use arqueology_core::Project;
use arqueology_openspec::{ComplianceSummary, SpecProject};
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
    arqueology_openspec::load(&project.root, &rel)
}

/// Cumplimiento cruzando specs con los `spec-report` del vault.
pub fn compliance(project: &Project, spec: &SpecProject) -> ComplianceSummary {
    let reports = arqueology_openspec::load_reports(&project.knowledge_dir().join("specs"));
    arqueology_openspec::compliance(spec, &reports, project.config.openspec.stale_change_days)
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
                anyhow::bail!("no existe el cambio {}", dir.display());
            }
            if !validate_change(project, &change, &dir) {
                anyhow::bail!("el cambio {change} no es válido");
            }
            println!("cambio {change} válido");
            Ok(())
        }
    }
}

fn status(project: &Project, capability: Option<String>, format: &str) -> Result<()> {
    let Some(mut spec) = load_spec(project)? else {
        println!("no se detectó openspec/ en este proyecto");
        println!("sugerencia: `adoc spec baseline` genera specs base inferidas");
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
        println!("capability: {cap} ({} requisitos)", reqs.len());
        for r in reqs {
            let evidence = if r.evidence.is_empty() {
                String::new()
            } else {
                format!(" — {}", r.evidence.join(", "))
            };
            println!("  [{:<12}] {} {}{evidence}", r.status, r.id, r.title);
        }
    }
    println!("\ntotal: {}", fmt_counts(&summary));
    if !spec.active_changes.is_empty() {
        println!("\ncambios activos:");
        for ch in &spec.active_changes {
            let stale = if ch.is_stale(project.config.openspec.stale_change_days) {
                format!(
                    " — sin actividad hace {} días (posiblemente abandonado)",
                    ch.days_since_update.unwrap_or_default()
                )
            } else {
                String::new()
            };
            println!(
                "  - {} ({}/{} tareas){stale}",
                ch.name, ch.tasks_done, ch.tasks_total
            );
        }
    }
    if !summary.unknown_ids.is_empty() {
        println!(
            "\naviso: reportes con requisitos que no existen en las specs: {}",
            summary.unknown_ids.join(", ")
        );
    }
    if summary.totals.contains_key("unevaluated") {
        println!(
            "\nLos requisitos `unevaluated` se evalúan con la tarea spec-compliance \
             (`adoc inspect`)."
        );
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
        anyhow::bail!("openspec.baseline = \"off\" en arqueology-doc.toml");
    }
    let inv: Inventory = serde_json::from_str(
        &fs::read_to_string(project.inventory_file())
            .context("no hay inventario; ejecuta `adoc scan` primero")?,
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
        anyhow::bail!("no hay módulos sin spec para generar baseline");
    }

    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let change = format!("baseline-{date}");
    let change_dir = openspec_dir.join("changes").join(&change);
    let change_rel = project.rel(&change_dir);
    let names: Vec<&str> = modules.iter().map(|m| m.name.as_str()).collect();

    if dry_run {
        println!(
            "dry-run: se generaría {change_rel}/ para: {}",
            names.join(", ")
        );
        println!("  proposal.md, tasks.md y una tarea spec-baseline para el agente");
        return Ok(());
    }
    if change_dir.exists() {
        anyhow::bail!("{change_rel} ya existe; revísalo o elimínalo antes de regenerar");
    }

    // RF-161: nunca escribir en openspec/specs/; solo en changes/.
    fs::create_dir_all(change_dir.join("specs"))?;
    let mut proposal = String::from(
        "# Baseline inferido\n\n## Why\n\n\
         El proyecto no tiene especificaciones para estas áreas. arqueology-doc propone \
         requisitos base inferidos del código por ingeniería inversa. Son **inferidos**: \
         describen lo que el código hace, no una intención de negocio verificada, y deben \
         revisarse antes de archivar el cambio.\n\n## What Changes\n\n",
    );
    for n in &names {
        proposal.push_str(&format!("- Agrega requisitos inferidos para `{n}`.\n"));
    }
    fs::write(change_dir.join("proposal.md"), proposal)?;
    fs::write(
        change_dir.join("tasks.md"),
        "## 1. Revisión humana\n\n\
         - [ ] 1.1 Revisar cada requisito inferido contra la intención de negocio\n\
         - [ ] 1.2 Completar escenarios faltantes\n\
         - [ ] 1.3 Validar con `openspec validate <cambio> --strict`\n\
         - [ ] 1.4 Archivar el cambio con `openspec archive`\n",
    )?;

    let task = baseline_task(project, &inv, &modules, &change_rel, &date)?;
    let task_path = project.tasks_dir().join(format!("{}.md", task.task_id));
    fs::create_dir_all(project.tasks_dir())?;
    fs::write(
        &task_path,
        planner::task_to_markdown(&task, &planner::TaskRules::from_config(&project.config)),
    )?;

    println!("cambio preparado en {change_rel}/ ({})", names.join(", "));
    println!(
        "tarea {} creada: ejecútala con `adoc inspect --task {}` y valida con \
         `adoc spec validate {change}`",
        task.task_id, task.task_id
    );
    if !openspec_dir.join("specs").is_dir() {
        println!("sugerencia: ejecuta `openspec init` para adoptar OpenSpec en el proyecto");
    }
    Ok(())
}

fn baseline_task(
    project: &Project,
    _inv: &Inventory,
    modules: &[&arqueology_core::model::ModuleInfo],
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
        objective: format!(
            "Inferir specs base en formato OpenSpec para: {}. Por cada capability escribe \
             {change_rel}/specs/<capability>/spec.md con `## ADDED Requirements`; cada \
             requisito es `### Requirement: <título>` con una frase SHALL/MUST, una línea \
             `> Inferido por arqueology-doc · confianza: high|medium|low · evidencia: \
             ruta:línea` y al menos un `#### Scenario:` con viñetas **WHEN**/**THEN**. \
             Describe solo comportamiento verificable en el código; no afirmes intención de \
             negocio (RF-158). No toques openspec/specs/.",
            names.join(", ")
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
        eprintln!("aviso: CLI `openspec` no disponible; validación estructural básica");
    }
    let issues = arqueology_openspec::validate_change_structure(dir);
    for i in &issues {
        eprintln!("error: {i}");
    }
    issues.is_empty()
}
