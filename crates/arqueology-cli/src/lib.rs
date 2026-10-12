//! arqueology-doc: CLI de inspección de proyectos y generación de documentación.
//!
//! Se distribuye con dos binarios equivalentes: `adoc` (corto) y `arqueology-doc`.

mod commands;
mod ui;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "adoc",
    version,
    about = "Inspecciona proyectos de software y genera documentación, conocimiento y contexto para agentes de IA.",
    long_about = None
)]
pub struct Cli {
    /// Opera sobre un proyecto registrado por nombre o ID (RF-173).
    #[arg(long, global = true)]
    project: Option<String>,

    /// Verbosidad (-v info, -vv debug).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Inicializa arqueology-doc en el proyecto actual.
    Init(commands::init::InitArgs),
    /// Inventario determinista del repositorio.
    Scan(commands::scan::ScanArgs),
    /// Genera o actualiza las tareas de inspección.
    Plan(commands::plan::PlanArgs),
    /// Ejecuta tareas con un agente (o las exporta para modo manual).
    Inspect(commands::inspect::InspectArgs),
    /// Gestión de tareas (modo manual).
    Task {
        #[command(subcommand)]
        cmd: commands::task::TaskCmd,
    },
    /// Resumen de estado, hallazgos y progreso.
    Review(commands::review::ReviewArgs),
    /// Validaciones para CI (frontmatter, enlaces, evidencias).
    Check(commands::check::CheckArgs),
    /// Detecta cambios y marca notas desactualizadas.
    Sync(commands::sync::SyncArgs),
    /// Sitio de documentación VitePress.
    Site {
        #[command(subcommand)]
        cmd: commands::site::SiteCmd,
    },
    /// Lista y prueba adaptadores de agentes.
    Agent {
        #[command(subcommand)]
        cmd: commands::agent::AgentCmd,
    },
    /// Integración con OpenSpec.
    Spec {
        #[command(subcommand)]
        cmd: commands::spec::SpecCmd,
    },
    /// Gestión del registro global de proyectos.
    Projects {
        #[command(subcommand)]
        cmd: commands::projects::ProjectsCmd,
    },
    /// Retoma la última inspección pendiente.
    Resume(commands::resume::ResumeArgs),
    /// Muestra el resultado: sitio con dashboard en el navegador (o reporte HTML sin VitePress).
    Preview(PreviewArgs),
    /// Genera un reporte HTML único (o Markdown) con el estado, hallazgos y preguntas.
    Report(commands::report::ReportArgs),
    /// Pregunta al agente sobre el proyecto; la respuesta queda en la FAQ (knowledge/faq/).
    Ask(commands::ask::AskArgs),
    /// Grafo del conocimiento en JSON: notas, relaciones y evidencias.
    Graph(commands::graph::GraphArgs),
    /// Estado completo del proyecto en JSON (tareas, hallazgos, notas, agente).
    Status(commands::status::StatusArgs),
    /// Verifica requisitos: git, Node, VitePress, OpenSpec, agentes y configuración.
    Doctor(commands::doctor::DoctorArgs),
}

#[derive(clap::Args)]
pub struct PreviewArgs {
    /// Puerto del servidor de VitePress.
    #[arg(long)]
    pub port: Option<u16>,
    /// No abrir el navegador.
    #[arg(long)]
    pub no_open: bool,
}

/// Punto de entrada compartido por los binarios `adoc` y `arqueology-doc`.
pub fn main_entry() -> Result<()> {
    let cli = Cli::parse();
    ui::init(cli.verbose);
    let filter = match cli.verbose {
        0 => "warn",
        1 => "info",
        _ => "debug",
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| filter.into()),
        )
        .with_writer(std::io::stderr)
        .init();

    commands::run(cli.command, cli.project)
}
