//! `graph` — grafo del conocimiento: JSON (vista de grafo de la extensión),
//! resumen en texto o página HTML interactiva autocontenida (`--format html`,
//! `--open`).

use anyhow::Result;
use archeology_core::graph_web;
use archeology_core::i18n::ui_lang;
use archeology_core::t;
use archeology_core::Project;
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct GraphArgs {
    /// Formato de salida (por defecto `json`; `html` con `--open` o `--out`).
    #[arg(long, value_parser = ["json", "text", "html"])]
    format: Option<String>,
    /// Genera el grafo HTML y lo abre en el navegador.
    #[arg(long)]
    open: bool,
    /// Ruta del HTML (por defecto, `graph.html` en el espacio de trabajo).
    #[arg(long)]
    out: Option<PathBuf>,
}

pub fn run(project: &Project, args: GraphArgs) -> Result<()> {
    let html_requested = args.open || args.out.is_some();
    let format = match args.format.as_deref() {
        Some(f) if html_requested && f != "html" => anyhow::bail!(t!("graph.cli.needs_html")),
        Some(f) => f,
        None if html_requested => "html",
        None => "json",
    };
    let graph = archeology_core::graph::build(project);
    match format {
        "json" => println!("{}", serde_json::to_string(&graph)?),
        "html" => {
            let out = write_html(project, &graph, args.out)?;
            println!("{}", t!("graph.cli.written", path = out.display()));
            if args.open {
                super::toolchain::open_in_browser(&out.display().to_string());
            }
        }
        _ => {
            let notes = graph.nodes.iter().filter(|n| n.kind != "code").count();
            println!(
                "{}",
                t!(
                    "cli.graph.summary",
                    notes = notes,
                    code = graph.nodes.len() - notes,
                    edges = graph.edges.len()
                )
            );
        }
    }
    Ok(())
}

/// Escribe la página HTML del grafo (idioma de la interfaz) y devuelve su ruta.
/// Sin `out`, en el espacio de trabajo del proyecto: nunca dentro del repositorio.
fn write_html(
    project: &Project,
    graph: &archeology_core::graph::Graph,
    out: Option<PathBuf>,
) -> Result<PathBuf> {
    let out = out.unwrap_or_else(|| project.state_dir().join("graph.html"));
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let html = graph_web::standalone_html(project, graph, ui_lang());
    archeology_core::fsutil::atomic_write(&out, html.as_bytes())?;
    Ok(out)
}
