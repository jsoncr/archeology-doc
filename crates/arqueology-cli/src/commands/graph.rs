//! `graph` — grafo del conocimiento en JSON (vista de grafo de la extensión).

use anyhow::Result;
use arqueology_core::Project;
use clap::Args;

#[derive(Args)]
pub struct GraphArgs {
    /// Formato de salida.
    #[arg(long, default_value = "json", value_parser = ["json", "text"])]
    format: String,
}

pub fn run(project: &Project, args: GraphArgs) -> Result<()> {
    let graph = arqueology_core::graph::build(project);
    if args.format == "json" {
        println!("{}", serde_json::to_string(&graph)?);
    } else {
        let notes = graph.nodes.iter().filter(|n| n.kind != "code").count();
        println!(
            "{notes} notas, {} archivos de código, {} relaciones",
            graph.nodes.len() - notes,
            graph.edges.len()
        );
    }
    Ok(())
}
