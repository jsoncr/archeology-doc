//! `scan` — inventario determinista (§8.2).

use anyhow::Result;
use archeology_core::frontmatter::NoteFrontmatter;
use archeology_core::managed;
use archeology_core::model::Inventory;
use archeology_core::templates;
use archeology_core::{t, tl, Project};
use clap::Args;
use std::fs;

#[derive(Args)]
pub struct ScanArgs {
    /// No analizar el historial git.
    #[arg(long)]
    no_git: bool,
    /// Imprimir el inventario en JSON por stdout.
    #[arg(long)]
    json: bool,
}

pub fn run(project: &Project, args: ScanArgs) -> Result<()> {
    let pb = crate::ui::spinner(t!("cli.scan.scanning"));
    let progress = pb.clone();
    let opts = archeology_scan::ScanOptions {
        on_progress: Some(Box::new(move |e| match e {
            archeology_scan::ScanEvent::Files(n, rel) => progress.set_message(t!(
                "cli.scan.scanning_files",
                count = n,
                path = tail(rel, 50)
            )),
            archeology_scan::ScanEvent::Phase(p) => {
                progress.set_message(t!("cli.scan.scanning_phase", phase = p))
            }
        })),
        git_history: !args.no_git && project.config.scan.git_history,
        max_file_size_kb: project.config.scan.max_file_size_kb,
    };
    let inv = archeology_scan::scan_project(project, &opts)?;

    pb.finish_and_clear();
    // Guardar inventory.json.
    let state_dir = project.state_dir();
    fs::create_dir_all(&state_dir)?;
    archeology_core::fsutil::atomic_write(
        &project.inventory_file(),
        serde_json::to_string_pretty(&inv)?.as_bytes(),
    )?;

    // knowledge/00-overview.md (RF-021).
    write_overview(project, &inv)?;

    // Notas índice por tipo (RF-064).
    archeology_core::index::write_mocs(project)?;

    // AGENTS.md con comandos y módulos, adaptadores y skills (RF-100 a RF-106).
    for f in archeology_core::context::refresh(project, Some(&inv))? {
        eprintln!("{}", t!("cli.scan.updated", path = f));
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&inv)?);
    } else {
        print_summary(&inv);
    }
    Ok(())
}

fn write_overview(project: &Project, inv: &Inventory) -> Result<()> {
    // Región generada: idioma de la documentación del proyecto.
    let lang = project.doc_lang();
    let langs: Vec<String> = inv
        .languages
        .iter()
        .map(|(l, s)| {
            tl!(
                lang,
                "doc.inventory_summary.language_item",
                lang = l,
                files = s.files,
                lines = s.lines
            )
        })
        .collect();
    let modules: Vec<String> = inv
        .modules
        .iter()
        .map(|m| {
            tl!(
                lang,
                "doc.inventory_summary.module_item",
                path = m.path,
                files = m.files
            )
        })
        .collect();
    let body = tl!(
        lang,
        "doc.inventory_summary.body",
        files = inv.total_files,
        size = format!("{:.1}", inv.total_bytes as f64 / 1_048_576.0),
        languages = if langs.is_empty() {
            tl!(lang, "doc.inventory_summary.no_languages")
        } else {
            langs.join(", ")
        },
        entrypoints = inv.entrypoints.len(),
        tests = inv.test_files.len(),
        markers = inv.markers.len(),
        openspec = match &inv.openspec {
            Some(os) => tl!(
                lang,
                "doc.inventory_summary.openspec_detected",
                capabilities = os.capabilities.len(),
                changes = os.changes_active.len()
            ),
            None => tl!(lang, "doc.inventory_summary.openspec_none"),
        },
        modules = if modules.is_empty() {
            tl!(lang, "doc.inventory_summary.flat")
        } else {
            modules.join("\n")
        }
    );
    let path = project.knowledge_dir().join("00-overview.md");
    archeology_core::fsutil::create_dir_in_root(&project.root, &project.knowledge_dir())?;
    let content = if path.is_file() {
        // Solo se regenera la región del inventario: lo que escribió el
        // agente o una persona se preserva (RF-123).
        let existing = fs::read_to_string(&path)?;
        if NoteFrontmatter::parse(&existing)
            .map(|fm| fm.locked)
            .unwrap_or(false)
        {
            eprintln!("{}", t!("cli.scan.locked", path = path.display()));
            return Ok(());
        }
        managed::upsert_region(&existing, "inventory", body.trim_end())?
    } else {
        templates::render(
            lang,
            "overview.md",
            &[
                ("project_name", &project.config.project.name),
                ("date", &chrono::Local::now().format("%Y-%m-%d").to_string()),
                ("summary", body.trim_end()),
            ],
        )?
    };
    // Idempotencia (RNF-07): no reescribir si no cambia.
    // Sin atravesar symlinks del repositorio.
    if archeology_core::fsutil::write_if_changed_in_root(&project.root, &path, &content)? {
        eprintln!("{}", t!("cli.scan.written", path = path.display()));
    }
    Ok(())
}

/// Últimos `n` caracteres de una ruta, para que quepa en una línea.
fn tail(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n {
        s.to_string()
    } else {
        format!("…{}", s.chars().skip(count - n + 1).collect::<String>())
    }
}

fn print_summary(inv: &Inventory) {
    println!("{}", t!("cli.scan.summary_title", path = inv.project_root));
    println!("{}", t!("cli.scan.files", count = inv.total_files));
    println!(
        "{}",
        t!(
            "cli.scan.size",
            size = format!("{:.1}", inv.total_bytes as f64 / 1_048_576.0)
        )
    );
    if !inv.languages.is_empty() {
        let mut langs: Vec<_> = inv.languages.iter().collect();
        langs.sort_by_key(|(_, s)| std::cmp::Reverse(s.files));
        let list = langs
            .iter()
            .map(|(l, s)| format!("{l}({})", s.files))
            .collect::<Vec<_>>()
            .join(" ");
        println!("{}", t!("cli.scan.languages", list = list));
    }
    if !inv.manifests.is_empty() {
        println!("{}", t!("cli.scan.manifests", count = inv.manifests.len()));
    }
    if !inv.entrypoints.is_empty() {
        println!(
            "{}",
            t!("cli.scan.entrypoints", list = inv.entrypoints.join(", "))
        );
    }
    println!("{}", t!("cli.scan.tests", count = inv.test_files.len()));
    if !inv.disabled_tests.is_empty() {
        println!(
            "{}",
            t!("cli.scan.disabled_tests", count = inv.disabled_tests.len())
        );
    }
    println!("{}", t!("cli.scan.markers", count = inv.markers.len()));
    if let Some(g) = &inv.git {
        println!(
            "{}",
            t!(
                "cli.scan.git",
                commits = g.commits_analyzed,
                authors = g.authors.len()
            )
        );
    }
    if let Some(os) = &inv.openspec {
        println!(
            "{}",
            t!(
                "cli.scan.openspec",
                capabilities = os.capabilities.len(),
                changes = os.changes_active.len()
            )
        );
    }
    println!("\n{}", t!("cli.scan.next_step"));
}
