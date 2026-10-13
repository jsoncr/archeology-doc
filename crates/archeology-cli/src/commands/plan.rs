//! `plan` — genera/actualiza tareas (§8.3).

use anyhow::Result;
use archeology_core::model::Inventory;
use archeology_core::planner;
use archeology_core::{t, Project};
use clap::Args;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

#[derive(Args)]
pub struct PlanArgs {
    /// Solo este módulo.
    #[arg(long)]
    module: Option<String>,
    /// Solo este tipo de tarea.
    #[arg(long, rename_all = "kebab-case")]
    r#type: Option<String>,
    /// Solo tareas cuyo alcance incluya esta ruta.
    #[arg(long)]
    path: Option<String>,
    /// Regenerar tareas existentes.
    #[arg(long)]
    force: bool,
}

pub fn run(project: &Project, args: PlanArgs) -> Result<()> {
    let inv_path = project.inventory_file();
    if !inv_path.is_file() {
        anyhow::bail!(t!("cli.plan.no_inventory"));
    }
    let inv: Inventory = serde_json::from_str(&fs::read_to_string(&inv_path)?)?;
    let has_openspec = inv.openspec.is_some();

    // Requisitos de OpenSpec: las tareas de verificación los citan por ID.
    let requirement_ids = archeology_openspec::load(
        &project.root,
        std::path::Path::new(&project.config.openspec.path),
    )?
    .map(|p| p.requirement_ids())
    .unwrap_or_default();
    let tasks = planner::build_plan_with(
        &inv,
        &project.config.plan,
        &planner::PlanPaths::from_config(&project.config),
        has_openspec,
        &requirement_ids,
    );
    let tasks_dir = project.tasks_dir();
    let rules = planner::TaskRules::from_config(&project.config);
    fs::create_dir_all(&tasks_dir)?;

    let profile = planner::SizeProfile::resolve(&inv, &project.config.plan);
    let profile_label = match profile {
        planner::SizeProfile::Small => t!("cli.plan.profile_small"),
        planner::SizeProfile::Medium => t!("cli.plan.profile_medium"),
        planner::SizeProfile::Large => t!("cli.plan.profile_large"),
    };
    let groups = tasks
        .iter()
        .filter(|t| t.kind == archeology_core::model::TaskType::Module)
        .count();
    println!(
        "{}",
        t!(
            "cli.plan.profile",
            profile = profile_label,
            files = planner::code_files(&inv),
            modules = inv.modules.len(),
            groups = groups
        )
    );

    // Tareas existentes: un plan nuevo no se mezcla con uno anterior distinto.
    let existing: BTreeMap<String, planner::TaskMeta> = fs::read_dir(&tasks_dir)?
        .flatten()
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .filter_map(|t| planner::parse_task(&t))
        .map(|m| (m.task_id.clone(), m))
        .collect();
    let filtered = args.module.is_some() || args.r#type.is_some() || args.path.is_some();
    let same_plan =
        existing.len() == tasks.len() && tasks.iter().all(|t| existing.contains_key(&t.task_id));
    if !existing.is_empty() && !same_plan && !args.force && !filtered {
        println!(
            "{}",
            t!(
                "cli.plan.not_merged",
                existing = existing.len(),
                new = tasks.len()
            )
        );
        println!("{}", t!("cli.plan.replace_hint"));
        return Ok(());
    }

    let mut written = 0;
    let mut kept = 0;
    for task in &tasks {
        if let Some(m) = &args.module {
            if !planner::covers_module(task.module.as_deref(), m) {
                continue;
            }
        }
        if let Some(t) = &args.r#type {
            if task.kind.as_str() != t {
                continue;
            }
        }
        if let Some(p) = &args.path {
            if !task.read.iter().any(|r| r.contains(p.as_str()))
                && !planner::covers_module(task.module.as_deref(), p)
            {
                continue;
            }
        }
        let file = tasks_dir.join(format!("{}.md", task.task_id));
        if file.exists() && !args.force {
            continue; // RF-005: no sobrescribir sin --force
        }
        let mut md = planner::task_to_markdown(task, &rules);
        // Misma tarea (tipo, módulos y salidas): conserva su estado.
        if let Some(old) = existing.get(&task.task_id) {
            let new = planner::parse_task(&md);
            if new.as_ref().is_some_and(|n| {
                n.kind == old.kind && n.module == old.module && n.produce == old.produce
            }) {
                md = planner::set_status_in_markdown(&md, &old.status);
                kept += 1;
            }
        }
        archeology_core::fsutil::atomic_write(&file, md.as_bytes())?;
        written += 1;
    }

    // Con --force y sin filtros, las tareas que ya no están en el plan sobran.
    let mut removed = 0;
    if args.force && !filtered {
        let ids: BTreeSet<&str> = tasks.iter().map(|t| t.task_id.as_str()).collect();
        let mut state = archeology_core::state::State::load(&project.state_file())?;
        for id in existing.keys().filter(|id| !ids.contains(id.as_str())) {
            fs::remove_file(tasks_dir.join(format!("{id}.md"))).ok();
            state.tasks.remove(id);
            state.questions.remove(id);
            removed += 1;
        }
        state.save(&project.state_file())?;
    }

    println!(
        "{}",
        t!(
            "cli.plan.summary",
            total = tasks.len(),
            written = written,
            kept = kept,
            removed = removed,
            path = project.rel(&tasks_dir)
        )
    );
    if written > 0 {
        println!("{}", t!("cli.plan.next_step"));
    }
    Ok(())
}
