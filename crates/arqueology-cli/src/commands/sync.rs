//! `sync` — detecta cambios en fuentes, marca notas stale y reabre las
//! tareas afectadas (§8.11).

use anyhow::Result;
use arqueology_core::index::{self, MarkResult};
use arqueology_core::planner;
use arqueology_core::state::State;
use arqueology_core::Project;
use clap::Args;
use std::collections::BTreeMap;
use std::fs;

#[derive(Args)]
pub struct SyncArgs {
    /// Comparar contra una ref git en vez de los hashes guardados.
    #[arg(long)]
    since: Option<String>,
    /// Mostrar qué cambiaría sin modificar notas, tareas ni estado.
    #[arg(long)]
    dry_run: bool,
}

pub fn run(project: &Project, args: SyncArgs) -> Result<()> {
    let pb = crate::ui::spinner("Comparando fuentes citadas por las notas…");
    let mut state = State::load(&project.state_file())?;
    // El índice se reconstruye desde las notas (fuente de verdad).
    state.source_to_notes = index::source_index(project);

    // Hashes actuales de todas las fuentes citadas.
    let mut current: BTreeMap<String, String> = BTreeMap::new();
    for rel in state.source_to_notes.keys() {
        if let Some(h) = project.source_hash(rel) {
            current.insert(rel.clone(), h);
        }
    }

    let changed: Vec<String> = if let Some(reference) = &args.since {
        match arqueology_scan::git::changed_since(&project.root, reference) {
            Some(files) => files,
            None => anyhow::bail!("no se pudo hacer git diff contra {reference}"),
        }
    } else {
        let mut changed = Vec::new();
        for rel in state.source_to_notes.keys() {
            // Sin hash previo = primera vez que se cita: es la línea base.
            let Some(old) = state.file_hashes.get(rel) else {
                continue;
            };
            match current.get(rel) {
                Some(h) if h == old => {}
                _ => changed.push(rel.clone()),
            }
        }
        changed
    };

    pb.finish_and_clear();
    let affected = state.affected_notes(&changed);
    if changed.is_empty() {
        println!("sync: sin cambios detectados");
    } else {
        println!("sync: {} fuentes cambiadas", changed.len());
        for c in &changed {
            let gone = if current.contains_key(c) {
                ""
            } else {
                " (eliminado)"
            };
            println!("  - {c}{gone}");
        }
    }

    // Marcar notas stale respetando `locked` (RF-121, RF-123).
    let mut marked = Vec::new();
    for note in &affected {
        let path = project.root.join(note);
        let result = if args.dry_run {
            MarkResult::Marked
        } else {
            index::mark_stale(&path)?
        };
        match result {
            MarkResult::Marked => {
                println!("  stale: {note}");
                marked.push(note.clone());
            }
            MarkResult::Locked => println!("  bloqueada (locked), no se toca: {note}"),
            MarkResult::AlreadyStale | MarkResult::NoFrontmatter => {}
        }
    }

    // Reabrir las tareas que producen notas afectadas (RF-122).
    let reopened = reopen_tasks(project, &mut state, &marked, args.dry_run)?;
    if !reopened.is_empty() {
        println!(
            "tareas reabiertas: {} — ejecuta `adoc inspect`",
            reopened.join(", ")
        );
    }

    if args.dry_run {
        println!("(dry-run: notas, tareas y estado sin modificar)");
        return Ok(());
    }

    // Nueva línea base: un cambio se notifica una sola vez.
    for (rel, h) in &current {
        state.file_hashes.insert(rel.clone(), h.clone());
    }
    for c in &changed {
        if !current.contains_key(c) {
            state.file_hashes.remove(c);
        }
    }
    state.stale_notes = index::stale_notes(project);
    state.save(&project.state_file())?;
    index::write_mocs(project)?;
    Ok(())
}

/// Vuelve a `pending` las tareas `done` cuyo `# Produce` cubre alguna nota.
fn reopen_tasks(
    project: &Project,
    state: &mut State,
    notes: &[String],
    dry_run: bool,
) -> Result<Vec<String>> {
    let mut reopened = Vec::new();
    if notes.is_empty() || !project.tasks_dir().is_dir() {
        return Ok(reopened);
    }
    for e in fs::read_dir(project.tasks_dir())?.flatten() {
        let path = e.path();
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Some(meta) = planner::parse_task(&text) else {
            continue;
        };
        if meta.status != "done" {
            continue;
        }
        let covers = meta.produce.iter().any(|p| {
            notes
                .iter()
                .any(|n| n == p || (p.ends_with('/') && n.starts_with(p.as_str())))
        });
        if covers {
            if !dry_run {
                fs::write(&path, planner::set_status_in_markdown(&text, "pending"))?;
                state.tasks.insert(meta.task_id.clone(), "pending".into());
            }
            reopened.push(meta.task_id);
        }
    }
    reopened.sort();
    Ok(reopened)
}
