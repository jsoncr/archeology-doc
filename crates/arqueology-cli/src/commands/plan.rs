//! `plan` — genera/actualiza tareas (§8.3).

use anyhow::Result;
use arqueology_core::model::Inventory;
use arqueology_core::planner;
use arqueology_core::Project;
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
        anyhow::bail!("no hay inventario; ejecuta `adoc scan` primero");
    }
    let inv: Inventory = serde_json::from_str(&fs::read_to_string(&inv_path)?)?;
    let has_openspec = inv.openspec.is_some();

    let tasks = planner::build_plan(
        &inv,
        &project.config.plan,
        &project.config.paths.knowledge,
        has_openspec,
    );
    let tasks_dir = project.tasks_dir();
    let rules = planner::TaskRules::from_config(&project.config);
    fs::create_dir_all(&tasks_dir)?;

    let profile = planner::SizeProfile::resolve(&inv, &project.config.plan);
    let profile_es = match profile {
        planner::SizeProfile::Small => "pequeño",
        planner::SizeProfile::Medium => "mediano",
        planner::SizeProfile::Large => "grande",
    };
    let groups = tasks
        .iter()
        .filter(|t| t.kind == arqueology_core::model::TaskType::Module)
        .count();
    println!(
        "perfil: {profile_es} ({} archivos de código, {} módulos en {groups} grupos) · \
         ajustable con [plan] size = \"small|medium|large\"",
        planner::code_files(&inv),
        inv.modules.len()
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
            "el plan actual tiene {} tareas y el nuevo tendría {}: no se mezclan.",
            existing.len(),
            tasks.len()
        );
        println!(
            "Para reemplazarlo: `adoc plan --force` (las tareas sin cambios conservan su estado)."
        );
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
        fs::write(&file, md)?;
        written += 1;
    }

    // Con --force y sin filtros, las tareas que ya no están en el plan sobran.
    let mut removed = 0;
    if args.force && !filtered {
        let ids: BTreeSet<&str> = tasks.iter().map(|t| t.task_id.as_str()).collect();
        let mut state = arqueology_core::state::State::load(&project.state_file())?;
        for id in existing.keys().filter(|id| !ids.contains(id.as_str())) {
            fs::remove_file(tasks_dir.join(format!("{id}.md"))).ok();
            state.tasks.remove(id);
            state.questions.remove(id);
            removed += 1;
        }
        state.save(&project.state_file())?;
    }

    println!(
        "plan: {} tareas ({} escritas, {} conservan su estado, {} eliminadas) en {}",
        tasks.len(),
        written,
        kept,
        removed,
        project.rel(&tasks_dir)
    );
    if written > 0 {
        println!("Siguiente paso: `adoc inspect` o `adoc task list`.");
    }
    Ok(())
}
