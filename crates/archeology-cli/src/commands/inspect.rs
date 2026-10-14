//! `inspect` — ejecuta tareas con un agente o las exporta (§8.4).

use anyhow::Result;
use archeology_agents::{InspectOptions, InspectReport};
use archeology_core::{t, Project};
use clap::Args;

use crate::ui;

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
    let (pb, on_event) = progress(project, &agent, headless);
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
    exit_on_limit(&report);
    Ok(())
}

/// Si la corrida se detuvo por un límite del agente, termina el proceso con
/// su código: 75 (EX_TEMPFAIL, cuota o rate limit) o 77 (EX_NOPERM, sin
/// sesión). El bloqueo del proyecto es un `flock`: lo libera el sistema.
pub fn exit_on_limit(report: &InspectReport) {
    if let Some(stop) = &report.limit_stop {
        use std::io::Write;
        std::io::stdout().flush().ok();
        std::io::stderr().flush().ok();
        std::process::exit(stop.exit_code());
    }
}

/// Spinner y callback de progreso para una corrida headless.
///
/// El encabezado y una línea por tarea terminada se imprimen en stdout a
/// medida que avanza la corrida (sin pisar el spinner, que va en stderr).
pub fn progress(
    project: &Project,
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
    let project_name = project.config.project.name.clone();
    let cb = move |e: &archeology_agents::InspectEvent| match e {
        archeology_agents::InspectEvent::RunStarted { total, .. } => {
            if *total > 0 {
                let text = header(&project_name, &agent, *total);
                bar.suspend(|| println!("{text}"));
            }
        }
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
                .map(|m| format!(" {}", short_modules(m)))
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
        archeology_agents::InspectEvent::TaskFinished(outcome) => {
            let text = task_block(outcome);
            bar.suspend(|| print!("{text}"));
        }
    };
    (pb, Some(Box::new(cb)))
}

/// Separador ` · ` atenuado.
fn sep() -> String {
    ui::dim(" · ")
}

/// `homehub · claude · 5 tareas por ejecutar`.
fn header(project: &str, agent: &str, total: usize) -> String {
    let count = if total == 1 {
        t!("cli.inspect.out.tasks_to_run_one")
    } else {
        t!("cli.inspect.out.tasks_to_run", count = total)
    };
    format!(
        "{}{}{}{}{}\n",
        ui::bold(project),
        sep(),
        agent,
        sep(),
        count
    )
}

/// Módulos de una tarea, abreviados: `root, public, src +4`.
fn short_modules(module: &str) -> String {
    let parts: Vec<&str> = module
        .split(',')
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .collect();
    let mut out = parts
        .iter()
        .take(MAX_MODULES)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    if parts.len() > MAX_MODULES {
        out.push_str(&format!(" +{}", parts.len() - MAX_MODULES));
    }
    out
}

/// Módulos que se nombran en la descripción de una tarea.
const MAX_MODULES: usize = 3;
/// Ancho máximo de la descripción (tipo + módulos).
const DESCRIPTION_WIDTH: usize = 40;
/// Ancho de las etiquetas de los detalles de una tarea.
const DETAIL_WIDTH: usize = 9;
/// Ancho de las etiquetas del resumen final.
const LABEL_WIDTH: usize = 11;

/// Bloque de una tarea terminada: línea principal y, si falló, el problema,
/// lo que dijo el agente y cómo corregirlo.
pub fn task_block(o: &archeology_agents::TaskOutcome) -> String {
    let (mark, id) = match o.status.as_str() {
        "done" => (ui::green("✓"), ui::bold(&o.task_id)),
        "failed" => (ui::red("✗"), ui::bold(&o.task_id)),
        "question" => (ui::yellow("?"), ui::bold(&o.task_id)),
        _ => (ui::dim("–"), ui::bold(&o.task_id)),
    };
    let what = match &o.module {
        Some(m) => format!("{} · {}", o.kind, short_modules(m)),
        None => o.kind.clone(),
    };
    let mut timing = crate::ui::duration(o.duration_ms);
    if o.attempts > 1 {
        timing.push_str(&format!(
            " · {}",
            t!("cli.inspect.out.attempts", count = o.attempts)
        ));
    }
    let mut out = format!(
        "{mark} {id}  {}  {}\n",
        ui::truncate(&what, DESCRIPTION_WIDTH),
        ui::dim(&timing)
    );
    let detail = |out: &mut String, label: &str, text: &str| {
        out.push_str(&format!(
            "    {} {text}\n",
            ui::dim(&ui::pad(label, DETAIL_WIDTH))
        ));
    };
    // Evidencias ajustadas al final del archivo: aviso, no fallo.
    for a in &o.adjusted {
        out.push_str(&format!("    {} {}\n", ui::yellow("!"), ui::yellow(a)));
    }
    match o.status.as_str() {
        "question" => {
            if let Some(q) = &o.question {
                detail(&mut out, &t!("cli.inspect.out.question_label"), q);
                out.push_str(&format!(
                    "    {}\n",
                    ui::cyan(&t!("cli.inspect.out.answer_hint", id = o.task_id))
                ));
            }
        }
        "failed" => {
            for (i, p) in o.problems.iter().enumerate() {
                let label = if i == 0 {
                    t!("cli.inspect.out.problem_label")
                } else {
                    String::new()
                };
                detail(&mut out, &label, p);
            }
            if let Some(m) = &o.agent_message {
                detail(
                    &mut out,
                    &t!("cli.inspect.out.agent_label"),
                    &ui::dim(&format!("“{m}”")),
                );
            }
            if o.stopped_early {
                out.push_str(&format!(
                    "    {}\n",
                    ui::dim(&t!("core.task_output.same_failure"))
                ));
            }
            let hint = o
                .hint
                .clone()
                .unwrap_or_else(|| t!("core.task_output.fix_agent", log = ui::tilde(&o.log)));
            out.push_str(&format!("    {}\n", ui::cyan(&format!("→ {hint}"))));
        }
        "pending" => match &o.limit {
            Some(l) => detail(
                &mut out,
                "",
                &ui::yellow(&t!("cli.limit.task_stopped", kind = l.kind.label())),
            ),
            None => detail(&mut out, "", &t!("cli.inspect.out.interrupted")),
        },
        _ => {}
    }
    out
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
    // Modo manual: las tareas quedan exportadas para hacerlas con otro agente.
    if !report.exported.is_empty() {
        println!(
            "{}",
            t!("cli.inspect.run_header", run = report.run_id, agent = agent)
        );
        println!(
            "{}",
            t!("cli.inspect.exported", tasks = report.exported.join(", "))
        );
        print_blocked(project, report);
        return;
    }
    let idle = report.tasks.is_empty() && report.blocked.is_empty();
    if idle {
        if !report.manual_waiting.is_empty() {
            print_manual_waiting(project, report, agent);
        } else {
            println!("{}", t!("cli.inspect.nothing_pending"));
            print_blocked(project, report);
        }
        return;
    }
    print!("{}", summary(project, report));
}

/// Resumen final: conteos, bloqueadas (y su causa), log y siguiente paso.
fn summary(project: &Project, report: &InspectReport) -> String {
    let count = |status: &str| report.tasks.iter().filter(|t| t.status == status).count();
    let plural = |key: &str, n: usize| {
        if n == 1 {
            t!(&format!("cli.inspect.out.count.{key}_one"))
        } else {
            t!(&format!("cli.inspect.out.count.{key}"), count = n)
        }
    };
    let mut parts = Vec::new();
    for (status, key, paint) in [
        ("done", "done", ui::green as fn(&str) -> String),
        ("failed", "failed", ui::red),
        ("question", "question", ui::yellow),
        ("pending", "interrupted", ui::dim),
    ] {
        let n = count(status);
        if n == 0 {
            continue;
        }
        // Pendiente por un límite del agente, no por una interrupción.
        if status == "pending" && report.limit_stop.is_some() {
            parts.push(ui::yellow(&if n == 1 {
                t!("cli.limit.count_stopped_one")
            } else {
                t!("cli.limit.count_stopped", count = n)
            }));
        } else {
            parts.push(paint(&plural(key, n)));
        }
    }
    if !report.blocked.is_empty() {
        parts.push(ui::yellow(&plural("blocked", report.blocked.len())));
    }
    let mut out = String::new();
    let row = |out: &mut String, label: &str, value: &str| {
        out.push_str(&format!(
            "{} {value}\n",
            ui::bold(&ui::pad(label, LABEL_WIDTH))
        ));
    };
    out.push('\n');
    row(
        &mut out,
        &t!("cli.inspect.out.label.summary"),
        &parts.join(&sep()),
    );

    // Bloqueadas: qué esperan y, si la causa es de otra corrida, qué hacer.
    let blocked: Vec<String> = report
        .blocked
        .iter()
        .take(MAX_BLOCKED)
        .map(|(id, deps)| {
            format!(
                "⏸ {}",
                t!("cli.inspect.out.waits_for", id = id, deps = deps.join(", "))
            )
        })
        .collect();
    for (i, b) in blocked.iter().enumerate() {
        let label = if i == 0 {
            t!("cli.inspect.out.label.blocked")
        } else {
            String::new()
        };
        row(&mut out, &label, b);
    }
    if report.blocked.len() > MAX_BLOCKED {
        row(
            &mut out,
            "",
            &ui::dim(&t!(
                "cli.status.more",
                count = report.blocked.len() - MAX_BLOCKED
            )),
        );
    }
    for line in root_cause_lines(project, report) {
        row(&mut out, "", &ui::dim(&line));
    }
    for (id, files) in &report.out_of_scope {
        row(
            &mut out,
            &t!("cli.inspect.out.label.out_of_scope"),
            &ui::yellow(&format!("{id}: {}", files.join(", "))),
        );
    }
    if let Some(dir) = &report.run_dir {
        row(
            &mut out,
            &t!("cli.inspect.out.label.log"),
            &ui::dim(&format!("{}/", ui::tilde(dir))),
        );
    }
    if let Some(stop) = &report.limit_stop {
        out.push_str(&limit_block(project, stop));
    }
    let (cmd, why) = next_step(project, report);
    out.push_str(&format!(
        "{} → {}  {}\n",
        ui::bold(&ui::pad(&t!("cli.inspect.out.label.next"), LABEL_WIDTH)),
        ui::cyan(&ui::bold(&cmd)),
        ui::dim(&why)
    ));
    out
}

/// Bloqueadas que se listan una por una en el resumen.
const MAX_BLOCKED: usize = 5;

/// Un único siguiente paso según lo que pasó en la corrida.
fn next_step(project: &Project, report: &InspectReport) -> (String, String) {
    if let Some(stop) = &report.limit_stop {
        let why = match stop.limit.kind {
            archeology_agents::LimitKind::Auth => {
                t!("cli.limit.next_auth", agent = stop.agent)
            }
            _ => match stop.limit.reset_at {
                Some(r) => t!(
                    "cli.limit.next_quota",
                    agent = stop.agent,
                    when = archeology_agents::limits::relative_reset(r, chrono::Utc::now())
                ),
                None => t!("cli.limit.next_quota_unknown", agent = stop.agent),
            },
        };
        return ("adoc inspect --resume".into(), why);
    }
    // La tarea no cabe en el contexto del agente: hay que dividirla.
    if let Some(f) = report.tasks.iter().find(|t| {
        t.status == "failed"
            && t.limit
                .as_ref()
                .is_some_and(|l| l.kind == archeology_agents::LimitKind::Context)
    }) {
        return (
            format!("adoc task show {}", f.task_id),
            t!("cli.limit.next_context", id = f.task_id),
        );
    }
    if let Some(q) = report.tasks.iter().find(|t| t.status == "question") {
        return (
            format!("adoc task answer {} \"…\"", q.task_id),
            t!("cli.inspect.out.next.answer"),
        );
    }
    if let Some(f) = report.tasks.iter().find(|t| t.status == "failed") {
        return if f.agent_failed {
            (
                "adoc inspect --resume".into(),
                t!("cli.inspect.out.next.agent_failed", id = f.task_id),
            )
        } else {
            (
                format!("adoc task show {}", f.task_id),
                t!("cli.inspect.out.next.show", id = f.task_id),
            )
        };
    }
    if report.tasks.iter().any(|t| t.status == "pending") {
        return (
            "adoc inspect".into(),
            t!("cli.inspect.out.next.interrupted"),
        );
    }
    match super::status::build(project) {
        Ok(st) => super::status_view::next_step(project, &st),
        Err(_) => ("adoc status".into(), String::new()),
    }
}

/// Tareas exportadas en modo manual que esta corrida no tomó.
fn print_manual_waiting(project: &Project, report: &InspectReport, agent: &str) {
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
    for line in root_cause_lines(project, report) {
        println!("  {line}");
    }
}

/// Dependencias de otras corridas que bloquean tareas y qué hacer con cada una.
fn root_cause_lines(project: &Project, report: &InspectReport) -> Vec<String> {
    report
        .root_causes
        .iter()
        .map(|(id, status)| {
            let hint = match status.as_str() {
                "failed" => {
                    let log = last_log(project, id)
                        .map(|p| t!("cli.inspect.log_suffix", path = ui::tilde(&p)))
                        .unwrap_or_default();
                    t!("cli.inspect.hint_failed", log = log)
                }
                "running" => t!("cli.inspect.hint_running"),
                "manual" => t!("cli.inspect.hint_manual", id = id),
                "question" => t!("cli.inspect.hint_question", id = id),
                "pending" => t!("cli.inspect.hint_pending", id = id),
                other => t!("cli.inspect.hint_other", status = other),
            };
            format!("→ {id} {hint}")
        })
        .collect()
}

/// Log más reciente de una tarea en `.archeology-doc/runs/`.
pub fn last_log(project: &Project, id: &str) -> Option<std::path::PathBuf> {
    let mut runs: Vec<_> = std::fs::read_dir(project.runs_dir())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(format!("{id}.log")).is_file())
        .collect();
    runs.sort();
    runs.pop().map(|r| r.join(format!("{id}.log")))
}

/// Bloque que explica por qué se detuvo la corrida: qué agente, su mensaje,
/// cuándo se restablece el límite y qué hacer (esperar, cambiar de agente o
/// iniciar sesión).
fn limit_block(project: &Project, stop: &archeology_agents::LimitStop) -> String {
    use archeology_agents::LimitKind;
    let mut out = String::new();
    let title = match stop.limit.kind {
        LimitKind::Auth => t!("cli.limit.title_auth", agent = stop.agent),
        kind => t!(
            "cli.limit.title_quota",
            agent = stop.agent,
            kind = kind.label()
        ),
    };
    out.push_str(&format!("\n{} {}\n", ui::yellow("⏸"), ui::bold(&title)));
    let row = |out: &mut String, label: &str, text: &str| {
        out.push_str(&format!(
            "    {} {text}\n",
            ui::dim(&ui::pad(label, DETAIL_WIDTH))
        ));
    };
    row(
        &mut out,
        &t!("cli.limit.agent_label"),
        &ui::dim(&format!("“{}”", stop.limit.message)),
    );
    if stop.limit.kind != LimitKind::Auth {
        let reset = match stop.limit.reset_at {
            Some(r) => archeology_agents::limits::describe_reset(r, chrono::Utc::now()),
            None => t!("cli.limit.reset_unknown"),
        };
        row(&mut out, &t!("cli.limit.reset_label"), &reset);
    }
    row(
        &mut out,
        &t!("cli.limit.stopped_label"),
        &t!(
            "cli.limit.stopped",
            task = stop.task_id,
            count = stop.not_started,
            code = stop.exit_code()
        ),
    );
    let next = |out: &mut String, text: String| {
        out.push_str(&format!("    {}\n", ui::cyan(&format!("→ {text}"))));
    };
    if stop.limit.kind == LimitKind::Auth {
        next(
            &mut out,
            t!(
                "cli.limit.next_login",
                hint = archeology_agents::limits::login_hint(&stop.program)
            ),
        );
        next(&mut out, t!("cli.limit.next_after_login"));
    } else {
        next(&mut out, t!("cli.limit.next_after_reset"));
        next(
            &mut out,
            t!(
                "cli.limit.next_switch",
                other = other_agent(project, &stop.agent)
            ),
        );
    }
    out
}

/// Otro agente al que cambiar: el primer headless instalado (solo se busca
/// en el `PATH`, sin ejecutarlo) distinto del actual; si no hay, `codex` o
/// `claude`.
fn other_agent(project: &Project, current: &str) -> String {
    project
        .config
        .agent
        .adapters
        .iter()
        .filter(|(n, a)| {
            n.as_str() != current
                && a.mode == archeology_core::config::AdapterMode::Headless
                && a.trusted
                && a.command
                    != project
                        .config
                        .agent
                        .adapters
                        .get(current)
                        .and_then(|c| c.command.clone())
                && archeology_agents::adapter::command_found(a)
        })
        .map(|(n, _)| n.clone())
        .next()
        .unwrap_or_else(|| {
            if current == "codex" {
                "claude"
            } else {
                "codex"
            }
            .to_string()
        })
}
