//! `scan` — inventario determinista (§8.2).

use anyhow::Result;
use arqueology_core::frontmatter::NoteFrontmatter;
use arqueology_core::managed;
use arqueology_core::model::Inventory;
use arqueology_core::templates;
use arqueology_core::Project;
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
    let pb = crate::ui::spinner("Escaneando el repositorio…");
    let progress = pb.clone();
    let opts = arqueology_scan::ScanOptions {
        on_progress: Some(Box::new(move |e| match e {
            arqueology_scan::ScanEvent::Files(n, rel) => {
                progress.set_message(format!("Escaneando… {n} archivos · {}", tail(rel, 50)))
            }
            arqueology_scan::ScanEvent::Phase(p) => {
                progress.set_message(format!("Escaneando: {p}"))
            }
        })),
        git_history: !args.no_git && project.config.scan.git_history,
        max_file_size_kb: project.config.scan.max_file_size_kb,
    };
    let inv = arqueology_scan::scan_project(project, &opts)?;

    pb.finish_and_clear();
    // Guardar inventory.json.
    let state_dir = project.state_dir();
    fs::create_dir_all(&state_dir)?;
    fs::write(
        project.inventory_file(),
        serde_json::to_string_pretty(&inv)?,
    )?;

    // knowledge/00-overview.md (RF-021).
    write_overview(project, &inv)?;

    // Notas índice por tipo (RF-064).
    arqueology_core::index::write_mocs(project)?;

    // AGENTS.md con comandos y módulos, adaptadores y skills (RF-100 a RF-106).
    for f in arqueology_core::context::refresh(project, Some(&inv))? {
        eprintln!("actualizado {f}");
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&inv)?);
    } else {
        print_summary(&inv);
    }
    Ok(())
}

fn write_overview(project: &Project, inv: &Inventory) -> Result<()> {
    let langs: Vec<String> = inv
        .languages
        .iter()
        .map(|(l, s)| format!("{l} ({} archivos, {} líneas)", s.files, s.lines))
        .collect();
    let modules: Vec<String> = inv
        .modules
        .iter()
        .map(|m| format!("- `{}` — {} archivos", m.path, m.files))
        .collect();
    let body = format!(
        "## Resumen\n\n\
         - Archivos: {} ({:.1} MB)\n\
         - Lenguajes: {}\n\
         - Puntos de entrada: {}\n\
         - Archivos de test: {}\n\
         - Marcadores TODO/FIXME: {}\n\
         - OpenSpec: {}\n\n\
         ## Módulos\n\n{}\n",
        inv.total_files,
        inv.total_bytes as f64 / 1_048_576.0,
        if langs.is_empty() {
            "ninguno detectado".into()
        } else {
            langs.join(", ")
        },
        inv.entrypoints.len(),
        inv.test_files.len(),
        inv.markers.len(),
        match &inv.openspec {
            Some(os) => format!(
                "detectado ({} capabilities, {} cambios activos)",
                os.capabilities.len(),
                os.changes_active.len()
            ),
            None => "no detectado".into(),
        },
        if modules.is_empty() {
            "(estructura plana)".into()
        } else {
            modules.join("\n")
        },
    );
    let path = project.knowledge_dir().join("00-overview.md");
    fs::create_dir_all(project.knowledge_dir())?;
    let content = if path.is_file() {
        // Solo se regenera la región del inventario: lo que escribió el
        // agente o una persona se preserva (RF-123).
        let existing = fs::read_to_string(&path)?;
        if NoteFrontmatter::parse(&existing)
            .map(|fm| fm.locked)
            .unwrap_or(false)
        {
            eprintln!(
                "{} está bloqueada (locked); no se actualiza",
                path.display()
            );
            return Ok(());
        }
        managed::upsert_region(&existing, "inventory", body.trim_end())?
    } else {
        templates::render(
            "overview.md",
            &[
                ("project_name", &project.config.project.name),
                ("date", &chrono::Local::now().format("%Y-%m-%d").to_string()),
                ("summary", body.trim_end()),
            ],
        )?
    };
    // Idempotencia (RNF-07): no reescribir si no cambia.
    if fs::read_to_string(&path).ok().as_deref() != Some(content.as_str()) {
        fs::write(&path, content)?;
        eprintln!("escrito {}", path.display());
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
    println!("Inventario ({})", inv.project_root);
    println!("  archivos:     {}", inv.total_files);
    println!(
        "  tamaño:       {:.1} MB",
        inv.total_bytes as f64 / 1_048_576.0
    );
    if !inv.languages.is_empty() {
        let mut langs: Vec<_> = inv.languages.iter().collect();
        langs.sort_by_key(|(_, s)| std::cmp::Reverse(s.files));
        println!(
            "  lenguajes:    {}",
            langs
                .iter()
                .map(|(l, s)| format!("{l}({})", s.files))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    if !inv.manifests.is_empty() {
        println!("  manifiestos:  {}", inv.manifests.len());
    }
    if !inv.entrypoints.is_empty() {
        println!("  entrypoints:  {}", inv.entrypoints.join(", "));
    }
    println!("  tests:        {} archivos", inv.test_files.len());
    if !inv.disabled_tests.is_empty() {
        println!("  tests desactivados: {}", inv.disabled_tests.len());
    }
    println!("  marcadores:   {}", inv.markers.len());
    if let Some(g) = &inv.git {
        println!(
            "  git:          {} commits analizados, {} autores",
            g.commits_analyzed,
            g.authors.len()
        );
    }
    if let Some(os) = &inv.openspec {
        println!(
            "  openspec:     {} capabilities, {} cambios activos",
            os.capabilities.len(),
            os.changes_active.len()
        );
    }
    println!("\nSiguiente paso: `adoc plan` para generar las tareas.");
}
