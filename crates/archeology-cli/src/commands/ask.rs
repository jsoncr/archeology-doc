//! `ask` — pregunta al agente sobre el proyecto; la respuesta queda en el
//! vault como nota `type: faq` (`knowledge/faq/`) y aparece en el preview.

use anyhow::{Context, Result};
use archeology_core::config::AdapterMode;
use archeology_core::{t, tl, Project};
use clap::Args;
use std::fs;

/// Pregunta ya normalizada.
struct AskQuestion {
    question: String,
    agent: Option<String>,
    model: Option<String>,
}

#[derive(Args)]
pub struct AskArgs {
    /// La pregunta; puede ir sin comillas. Sin argumentos, se pide por la terminal
    /// (útil en zsh, donde `?` es un comodín).
    #[arg(num_args = 0..)]
    question: Vec<String>,
    /// Agente a usar (por defecto, el activo).
    #[arg(long)]
    agent: Option<String>,
    /// Modelo del agente solo para esta pregunta.
    #[arg(long)]
    model: Option<String>,
}

pub fn run(project: &Project, mut args: AskArgs) -> Result<()> {
    let question = if args.question.is_empty() {
        super::ask(&t!("cli.ask.prompt"), "")?
    } else {
        args.question.join(" ")
    };
    let question = question.trim().to_string();
    if question.is_empty() {
        anyhow::bail!(t!("cli.ask.empty"));
    }
    let args = AskQuestion {
        question,
        agent: args.agent.take(),
        model: args.model.take(),
    };
    let agent = args
        .agent
        .unwrap_or_else(|| project.config.agent.default.clone());
    let mut adapter = project
        .config
        .agent
        .adapters
        .get(&agent)
        .cloned()
        .with_context(|| t!("cli.commands.adapter_undefined", agent = agent))?;
    if args.model.is_some() {
        adapter.model = args.model;
    }
    if adapter.mode == archeology_core::config::AdapterMode::Headless && !adapter.trusted {
        anyhow::bail!(archeology_core::trust::untrusted_message(&agent));
    }

    let slug = archeology_openspec::slugify(&args.question)
        .chars()
        .take(60)
        .collect::<String>();
    let slug = if slug.is_empty() {
        // Nombre de archivo en el idioma de la documentación del proyecto.
        tl!(project.doc_lang(), "doc.faq.fallback_slug")
    } else {
        slug
    };
    let know = project
        .config
        .paths
        .knowledge
        .trim_end_matches('/')
        .to_string();
    let rel = format!("{know}/faq/{slug}.md");
    let note = project.root.join(&rel);
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let title = args.question.replace('"', "'");

    // Prompt en el idioma de la documentación del proyecto (no el de la interfaz).
    let prompt = archeology_core::planner::ask_prompt(
        project.doc_lang(),
        &archeology_core::planner::AskPrompt {
            slug: &slug,
            question: &args.question,
            knowledge: &know,
            note: &rel,
            title: &title,
            today: &today,
            secrets: &project.config.scan.secrets_patterns,
        },
    );

    if adapter.mode != AdapterMode::Headless {
        println!(
            "{}",
            t!("cli.ask.not_headless", agent = agent, prompt = prompt)
        );
        return Ok(());
    }
    if let Some(w) = archeology_agents::adapter_warnings(&agent, &adapter).first() {
        anyhow::bail!(t!("cli.ask.invalid_agent_config", warning = w));
    }

    // La tarea se guarda en el espacio de trabajo (`{prompt_file}` de algunos agentes).
    let asks = project.state_dir().join("asks");
    fs::create_dir_all(&asks)?;
    let prompt_file = asks.join(format!("{slug}.md"));
    archeology_core::fsutil::atomic_write(&prompt_file, prompt.as_bytes())?;

    let start = std::time::SystemTime::now();
    let pb = crate::ui::spinner(t!("cli.ask.answering", agent = agent));
    let result = archeology_agents::adapter::run_headless(
        &adapter,
        &prompt_file,
        &prompt,
        &project.root,
        adapter
            .timeout_secs
            .unwrap_or(project.config.agent.timeout_secs),
    );
    pb.finish_and_clear();
    let run = result?;
    let log = asks.join(format!("{slug}.log"));
    fs::write(&log, &run.output).ok();

    let written = fs::symlink_metadata(&note)
        .and_then(|m| m.modified())
        .map(|m| m >= start)
        .unwrap_or(false);
    if !run.success() {
        // Error, timeout o interrupción: lo escrito puede estar a medias. No
        // entra en la FAQ: se aparta al espacio de trabajo para revisarlo.
        let mut detail = String::new();
        if written {
            let partial = asks.join(format!("{slug}.partial.md"));
            if fs::rename(&note, &partial).is_ok() {
                detail = t!("cli.ask.partial_kept", path = partial.display(), rel = rel);
            }
        }
        if run.interrupted {
            eprintln!(
                "{}",
                t!("cli.ask.interrupted", agent = agent, detail = detail)
            );
            return Err(archeology_agents::Interrupted.into());
        }
        anyhow::bail!(t!(
            "cli.ask.no_response",
            agent = agent,
            reason = run.failure_reason(),
            log = log.display(),
            detail = detail
        ));
    }
    if !written {
        // El agente respondió por pantalla sin escribir la nota: se guarda igual.
        let answer = run.output.trim();
        if answer.is_empty() {
            anyhow::bail!(t!(
                "cli.ask.empty_response",
                agent = agent,
                log = log.display()
            ));
        }
        archeology_core::fsutil::write_in_root(
            &project.root,
            &note,
            format!(
                "---\nid: faq-{slug}\ntype: faq\ntitle: \"{title}\"\nstatus: draft\nconfidence: low\n\
                 sources: []\ngenerated_by: {agent}\nupdated: {today}\n---\n\n# {title}\n\n{answer}\n"
            )
            .as_bytes(),
        )?;
    }

    // Sin seguir symlinks: la nota podría ser un enlace plantado en el repositorio.
    let text = archeology_core::fsutil::read_to_string_nofollow(&note)?
        .with_context(|| t!("cli.ask.note_missing", path = rel))?;
    let body = archeology_core::frontmatter::extract_frontmatter(&text)
        .map(|(_, b)| b.to_string())
        .unwrap_or_else(|| text.clone());
    println!("{}\n", body.trim());
    for issue in archeology_core::frontmatter::validate_note(&note, &text, &project.root) {
        eprintln!("{}", t!("core.common.warning", message = issue.message));
    }
    archeology_core::index::write_mocs(project)?;
    println!("{}", t!("cli.ask.saved", path = rel));
    Ok(())
}
