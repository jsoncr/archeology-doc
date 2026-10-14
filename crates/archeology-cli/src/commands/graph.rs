//! `graph` — grafo del conocimiento. Por defecto genera la página HTML
//! interactiva autocontenida en el espacio de trabajo y la abre en el
//! navegador; `--json` imprime el JSON (vista de grafo de la extensión).
//!
//! `--format json|text|html` y `--open` siguen aceptándose (ocultos) por
//! compatibilidad: versiones anteriores de la extensión llaman
//! `graph --format json`.

use anyhow::Result;
use archeology_core::graph_web;
use archeology_core::i18n::ui_lang;
use archeology_core::t;
use archeology_core::Project;
use clap::Args;
use std::path::PathBuf;

/// Variable de entorno que evita abrir el navegador (tests, entornos sin GUI).
pub const NO_OPEN_ENV: &str = "ADOC_NO_OPEN";

#[derive(Args)]
pub struct GraphArgs {
    /// Imprime el grafo en JSON (con `--out`, lo escribe en ese archivo).
    #[arg(long, conflicts_with_all = ["format", "open", "no_open"])]
    json: bool,
    /// Escribe el HTML (o el JSON con `--json`) en esta ruta sin abrir el navegador.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Genera el HTML sin abrir el navegador.
    #[arg(long, conflicts_with = "open")]
    no_open: bool,
    /// Compatibilidad: formato de salida antiguo (`json`, `text`, `html`).
    #[arg(long, hide = true, value_parser = ["json", "text", "html"])]
    format: Option<String>,
    /// Compatibilidad: abrir el HTML (ahora es el comportamiento por defecto).
    #[arg(long, hide = true)]
    open: bool,
}

/// Qué hace `graph` según los flags.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Json,
    Text,
    Html { open: bool },
}

fn mode(args: &GraphArgs) -> Result<Mode> {
    let no_open_env = std::env::var_os(NO_OPEN_ENV).is_some_and(|v| !v.is_empty() && v != "0");
    if args.json {
        return Ok(Mode::Json);
    }
    // Flags antiguos: `--format` decide; `--open` exige HTML.
    let html_requested = args.open || args.out.is_some();
    match args.format.as_deref() {
        Some(f) if html_requested && f != "html" => anyhow::bail!(t!("graph.cli.needs_html")),
        Some("json") => Ok(Mode::Json),
        Some("text") => Ok(Mode::Text),
        // `--format html` solo abría con `--open`.
        Some(_) => Ok(Mode::Html {
            open: args.open && !no_open_env,
        }),
        // Nuevo comportamiento: HTML y navegador, salvo `--out`/`--no-open`.
        None => Ok(Mode::Html {
            open: (args.open || (args.out.is_none() && !args.no_open)) && !no_open_env,
        }),
    }
}

pub fn run(project: &Project, args: GraphArgs) -> Result<()> {
    let mode = mode(&args)?;
    let graph = archeology_core::graph::build(project);
    match mode {
        Mode::Json => {
            let json = serde_json::to_string(&graph)?;
            match &args.out {
                Some(out) => {
                    write_file(out, json.as_bytes())?;
                    println!("{}", t!("graph.cli.written", path = out.display()));
                }
                None => println!("{json}"),
            }
        }
        Mode::Html { open } => {
            let out = write_html(project, &graph, args.out)?;
            println!("{}", t!("graph.cli.written", path = out.display()));
            if open {
                super::toolchain::open_in_browser(&out.display().to_string());
            }
        }
        Mode::Text => {
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

/// Escritura atómica creando el directorio padre si hace falta.
fn write_file(out: &std::path::Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    archeology_core::fsutil::atomic_write(out, content)?;
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
    let html = graph_web::standalone_html(project, graph, ui_lang());
    write_file(&out, html.as_bytes())?;
    Ok(out)
}
