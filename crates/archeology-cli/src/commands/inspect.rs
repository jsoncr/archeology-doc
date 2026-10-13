//! `inspect` — ejecuta tareas con un agente o las exporta (§8.4).

use anyhow::Result;
use archeology_agents::{InspectOptions, InspectReport};
use archeology_core::{t, Project};
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
            .map(|a| a.mode == archeology_core::config::AdapterMode::Headless)
            .unwrap_or(false);
    if headless {
        if let Some(w) = project
            .config
            .agent
            .adapters
            .get(&agent)
            .and_then(|a| archeology_agents::adapter::unrestricted_warning(&agent, a))
        {
            eprintln!("{}", t!("core.common.warning", message = w));
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
    let report = archeology_agents::run_tasks(project, &opts);
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
    Option<archeology_agents::InspectCallback>,
) {
    if !enabled {
        return (indicatif::ProgressBar::hidden(), None);
    }
    let pb = crate::ui::spinner(t!("cli.inspect.preparing", agent = agent));
    let bar = pb.clone();
    let agent = agent.to_string();
    let cb = move |e: &archeology_agents::InspectEvent| match e {
        archeology_agents::InspectEvent::TaskStarted {
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
                t!(
                    "cli.inspect.attempt",
                    attempt = attempt,
                    attempts = attempts
                )
            } else {
                String::new()
            };
            bar.reset_elapsed();
            bar.set_message(t!(
                "cli.inspect.working",
                index = index,
                total = total,
                task = task_id,
                kind = kind,
                module = module,
                agent = agent,
                retry = retry
            ));
        }
        archeology_agents::InspectEvent::TaskFinished {
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
            "{}",
            t!(
                "cli.inspect.dry_run",
                agent = agent,
                count = report.exported.len()
            )
        );
        for t in &report.exported {
            println!("  - {t}");
        }
        print_blocked(project, report);
        return;
    }
    println!(
        "{}",
        t!("cli.inspect.run_header", run = report.run_id, agent = agent)
    );
    if !report.executed.is_empty() {
        println!(
            "{}",
            t!("cli.inspect.completed", tasks = report.executed.join(", "))
        );
    }
    if !report.exported.is_empty() {
        println!(
            "{}",
            t!("cli.inspect.exported", tasks = report.exported.join(", "))
        );
    }
    for (id, reason) in &report.failed {
        println!("{}", t!("cli.inspect.failed", id = id, reason = reason));
    }
    for (id, q) in &report.questions {
        println!("{}", t!("cli.inspect.question", id = id, question = q));
        println!("{}", t!("cli.inspect.answer_hint", id = id));
    }
    for (id, files) in &report.out_of_scope {
        println!(
            "{}",
            t!(
                "cli.inspect.out_of_scope",
                id = id,
                files = files.join(", ")
            )
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
            .map(|a| a.mode == archeology_core::config::AdapterMode::Headless)
            .unwrap_or(false);
        println!(
            "{}",
            t!(
                "cli.inspect.manual_waiting",
                count = report.manual_waiting.len()
            )
        );
        println!("{}", t!("cli.inspect.manual_hint_done"));
        if headless {
            println!("{}", t!("cli.inspect.manual_hint_resume", agent = agent));
        } else {
            println!("{}", t!("cli.inspect.manual_hint_switch"));
        }
    } else if idle {
        println!("{}", t!("cli.inspect.nothing_pending"));
    }
}

fn print_blocked(project: &Project, report: &InspectReport) {
    // Con muchas tareas bloqueadas, un resumen en vez de una línea por tarea.
    if report.blocked.len() > 5 {
        println!(
            "{}",
            t!("cli.inspect.blocked_summary", count = report.blocked.len())
        );
    } else {
        for (id, deps) in &report.blocked {
            println!(
                "{}",
                t!("cli.inspect.blocked", id = id, deps = deps.join(", "))
            );
        }
    }
    for (id, status) in &report.root_causes {
        let hint = match status.as_str() {
            "failed" => {
                let log = last_log(project, id)
                    .map(|p| t!("cli.inspect.log_suffix", path = project.rel(&p)))
                    .unwrap_or_default();
                t!("cli.inspect.hint_failed", log = log)
            }
            "running" => t!("cli.inspect.hint_running"),
            "manual" => t!("cli.inspect.hint_manual", id = id),
            "question" => t!("cli.inspect.hint_question", id = id),
            "pending" => t!("cli.inspect.hint_pending", id = id),
            other => t!("cli.inspect.hint_other", status = other),
        };
        println!("  → {id} {hint}");
    }
}

/// Log más reciente de una tarea en `.archeology-doc/runs/`.
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
