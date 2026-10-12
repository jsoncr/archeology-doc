//! `inspect` — ejecuta tareas con un agente o las exporta (§8.4).

use anyhow::Result;
use arqueology_agents::{InspectOptions, InspectReport};
use arqueology_core::Project;
use clap::Args;

#[derive(Args)]
pub struct InspectArgs {
    /// Adaptador a usar (por defecto: agent.default de la config).
    #[arg(long)]
    agent: Option<String>,
    /// Modelo del agente solo para esta corrida (p. ej. opus, sonnet).
    #[arg(long)]
    model: Option<String>,
    /// Ejecutar solo estas tareas.
    #[arg(long, value_delimiter = ',')]
    task: Vec<String>,
    /// Solo tareas de este módulo.
    #[arg(long)]
    module: Option<String>,
    /// Reanudar incluyendo tareas fallidas.
    #[arg(long)]
    resume: bool,
    /// Mostrar qué se ejecutaría sin ejecutarlo.
    #[arg(long)]
    dry_run: bool,
}

pub fn run(project: &Project, args: InspectArgs) -> Result<()> {
    let agent = args
        .agent
        .clone()
        .unwrap_or_else(|| project.config.agent.default.clone());
    let headless = !args.dry_run
        && project
            .config
            .agent
            .adapters
            .get(&agent)
            .map(|a| a.mode == arqueology_core::config::AdapterMode::Headless)
            .unwrap_or(false);
    if headless {
        if let Some(w) = project
            .config
            .agent
            .adapters
            .get(&agent)
            .and_then(|a| arqueology_agents::adapter::unrestricted_warning(&agent, a))
        {
            eprintln!("aviso: {w}");
        }
    }
    let (pb, on_event) = progress(&agent, headless);
    let opts = InspectOptions {
        model: args.model.clone(),
        on_event,
        agent: agent.clone(),
        only_tasks: args.task.clone(),
        module: args.module.clone(),
        resume: args.resume,
        dry_run: args.dry_run,
    };
    let report = arqueology_agents::run_tasks(project, &opts);
    pb.finish_and_clear();
    let report = report?;
    print_report(project, &report, &agent, args.dry_run);
    Ok(())
}

/// Spinner y callback de progreso para una corrida headless.
pub fn progress(
    agent: &str,
    enabled: bool,
) -> (
    indicatif::ProgressBar,
    Option<arqueology_agents::InspectCallback>,
) {
    if !enabled {
        return (indicatif::ProgressBar::hidden(), None);
    }
    let pb = crate::ui::spinner(format!("Preparando tareas para {agent}…"));
    let bar = pb.clone();
    let agent = agent.to_string();
    let cb = move |e: &arqueology_agents::InspectEvent| match e {
        arqueology_agents::InspectEvent::TaskStarted {
            task_id,
            kind,
            module,
            attempt,
            attempts,
            index,
            total,
        } => {
            let module = module
                .as_deref()
                .map(|m| format!(" {m}"))
                .unwrap_or_default();
            let retry = if *attempt > 1 {
                format!(" (intento {attempt}/{attempts})")
            } else {
                String::new()
            };
            bar.reset_elapsed();
            bar.set_message(format!(
                "[{index}/{total}] {task_id} · {kind}{module} · {agent} trabajando{retry}…"
            ));
        }
        arqueology_agents::InspectEvent::TaskFinished {
            task_id,
            ok,
            duration_ms,
            detail,
        } => {
            let mark = if *ok { "✓" } else { "✗" };
            let detail = detail
                .as_deref()
                .map(|d| format!(" — {d}"))
                .unwrap_or_default();
            bar.println(format!(
                "{mark} {task_id} ({}){detail}",
                crate::ui::duration(*duration_ms)
            ));
        }
    };
    (pb, Some(Box::new(cb)))
}

pub fn print_report(project: &Project, report: &InspectReport, agent: &str, dry_run: bool) {
    if dry_run {
        println!(
            "dry-run (agente: {agent}): se ejecutarían {} tareas:",
            report.exported.len()
        );
        for t in &report.exported {
            println!("  - {t}");
        }
        print_blocked(project, report);
        return;
    }
    println!("run {} (agente: {agent})", report.run_id);
    if !report.executed.is_empty() {
        println!("  completadas: {}", report.executed.join(", "));
    }
    if !report.exported.is_empty() {
        println!(
            "  exportadas (modo manual): {} — complétalas con tu agente y usa `task done <id>`",
            report.exported.join(", ")
        );
    }
    for (id, reason) in &report.failed {
        println!("  falló {id}: {reason}");
    }
    for (id, q) in &report.questions {
        println!("  ? {id} pregunta: {q}");
        println!("    responde con: adoc task answer {id} \"<respuesta>\"  (o hazla en vivo: adoc task run {id} --interactive)");
    }
    for (id, files) in &report.out_of_scope {
        println!(
            "  aviso: {id} modificó archivos fuera de su alcance: {}",
            files.join(", ")
        );
    }
    print_blocked(project, report);
    let idle = report.executed.is_empty()
        && report.exported.is_empty()
        && report.failed.is_empty()
        && report.questions.is_empty()
        && report.blocked.is_empty();
    if !report.manual_waiting.is_empty() && idle {
        let headless = project
            .config
            .agent
            .adapters
            .get(agent)
            .map(|a| a.mode == arqueology_core::config::AdapterMode::Headless)
            .unwrap_or(false);
        println!(
            "  {} tareas en modo manual (exportadas para hacerlas a mano):",
            report.manual_waiting.len()
        );
        println!("    · hazlas en ese agente y ciérralas con `adoc task done <id>` (ver `adoc task show <id>`)");
        if headless {
            println!("    · o ejecútalas con {agent}: `adoc inspect --resume`");
        } else {
            println!("    · o cambia a un agente con CLI: `adoc agent use claude` (las reabre) y luego `adoc inspect`");
        }
    } else if idle {
        println!("  no hay tareas pendientes");
    }
}

fn print_blocked(project: &Project, report: &InspectReport) {
    // Con muchas tareas bloqueadas, un resumen en vez de una línea por tarea.
    if report.blocked.len() > 5 {
        println!(
            "  {} tareas bloqueadas por dependencias sin completar",
            report.blocked.len()
        );
    } else {
        for (id, deps) in &report.blocked {
            println!("  bloqueada {id}: espera a {}", deps.join(", "));
        }
    }
    for (id, status) in &report.root_causes {
        let hint = match status.as_str() {
            "failed" => {
                let log = last_log(project, id)
                    .map(|p| format!(" (log: {})", project.rel(&p)))
                    .unwrap_or_default();
                format!(
                    "falló en una corrida anterior{log} — reintenta con `adoc inspect --resume`"
                )
            }
            "running" => "quedó interrumpida — reintenta con `adoc inspect --resume`".into(),
            "manual" => format!("está en modo manual — complétala y usa `adoc task done {id}`"),
            "question" => format!("espera tu respuesta — `adoc task answer {id} \"…\"`"),
            "pending" => format!(
                "está pendiente pero fuera del filtro — ejecútala con `adoc inspect --task {id}`"
            ),
            other => format!("estado `{other}`"),
        };
        println!("  → {id} {hint}");
    }
}

/// Log más reciente de una tarea en `.arqueology-doc/runs/`.
fn last_log(project: &Project, id: &str) -> Option<std::path::PathBuf> {
    let mut runs: Vec<_> = std::fs::read_dir(project.runs_dir())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(format!("{id}.log")).is_file())
        .collect();
    runs.sort();
    runs.pop().map(|r| r.join(format!("{id}.log")))
}
