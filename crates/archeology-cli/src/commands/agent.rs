//! `agent` — lista, prueba y selecciona adaptadores de agentes (§13).

use anyhow::{Context, Result};
use archeology_agents::check_adapter;
use archeology_core::config::{AdapterMode, ArcheologyConfig};
use archeology_core::{t, Project};
use clap::Subcommand;
use std::fs;

#[derive(Subcommand)]
pub enum AgentCmd {
    /// Lista los adaptadores configurados; `*` marca el agente activo.
    List,
    /// Prueba que el comando del adaptador responde.
    Test {
        /// Nombre del adaptador (por defecto: todos).
        #[arg(long)]
        agent: Option<String>,
    },
    /// Cambia el agente (y opcionalmente el modelo) que usa `inspect` en este proyecto.
    ///
    /// Ejemplos: `adoc agent use claude --model opus`, `adoc agent use devin`, `adoc agent use manual`.
    Use {
        /// Adaptador predefinido (claude, codex, devin, gemini, qwen, opencode, cursor, copilot,
        /// amp, goose, aider, manual) u otro definido en la config.
        name: String,
        /// Modelo del agente (p. ej. opus, sonnet, gpt-5).
        #[arg(long, conflicts_with = "no_model")]
        model: Option<String>,
        /// Quita el modelo fijado y usa el predeterminado del agente.
        #[arg(long)]
        no_model: bool,
        /// Reemplaza la definición del adaptador por la predefinida actual
        /// (p. ej. tras actualizar adoc).
        #[arg(long)]
        reset: bool,
    },
    /// Aprueba un adaptador definido en archeology-doc.toml para este proyecto.
    ///
    /// La config viaja con el repositorio: un adaptador que no es predefinido
    /// decide qué programa se ejecuta. Solo se ejecuta tras revisarlo y
    /// aprobarlo; si cambia en el repositorio, hay que aprobarlo de nuevo.
    Trust {
        name: String,
        /// No pedir confirmación (scripts, CI).
        #[arg(long)]
        yes: bool,
    },
    /// Retira la aprobación de un adaptador en este proyecto.
    Untrust { name: String },
}

pub fn run(cmd: AgentCmd, project_arg: Option<String>) -> Result<()> {
    match cmd {
        AgentCmd::Use {
            name,
            model,
            no_model,
            reset,
        } => {
            let project = super::resolve_project(&project_arg)?;
            let _lock = project.lock("agent use")?;
            use_agent(&project, &name, model.as_deref(), no_model, reset)
        }
        AgentCmd::Trust { name, yes } => {
            let project = super::resolve_project(&project_arg)?;
            trust(&project, &name, yes)
        }
        AgentCmd::Untrust { name } => {
            let project = super::resolve_project(&project_arg)?;
            if archeology_core::trust::revoke(&project.root, &name)? {
                println!("{}", t!("cli.agent.trust_revoked", name = name));
            } else {
                println!("{}", t!("cli.agent.not_trusted", name = name));
            }
            Ok(())
        }
        cmd => {
            // Los adaptadores pueden vivir en la config del proyecto; si no hay
            // proyecto, se usan los predefinidos. Una config inválida se informa:
            // no se esconde detrás de los predefinidos.
            let config = match super::resolve_project(&project_arg) {
                Ok(p) => p.config,
                Err(e)
                    if project_arg.is_none()
                        && matches!(
                            e.downcast_ref::<archeology_core::Error>(),
                            Some(archeology_core::Error::ProjectNotFound)
                        ) =>
                {
                    ArcheologyConfig::default()
                }
                Err(e) => return Err(e),
            };
            match cmd {
                AgentCmd::List => list(&config),
                AgentCmd::Test { agent } => test(&config, agent),
                AgentCmd::Use { .. } | AgentCmd::Trust { .. } | AgentCmd::Untrust { .. } => {
                    unreachable!()
                }
            }
            Ok(())
        }
    }
}

fn list(config: &ArcheologyConfig) {
    let builtin = archeology_core::config::builtin_adapters();
    // Configurados en el proyecto + predefinidos que aún no están en la config.
    let mut rows: Vec<(String, archeology_core::config::AdapterConfig, bool)> = config
        .agent
        .adapters
        .iter()
        .map(|(n, a)| (n.clone(), a.clone(), true))
        .collect();
    for (n, a) in &builtin {
        if !config.agent.adapters.contains_key(n) {
            rows.push((n.clone(), a.clone(), false));
        }
    }
    let statuses: Vec<_> = crate::ui::with_spinner(t!("cli.agent.checking_agents"), || {
        rows.iter().map(|(n, a, _)| check_adapter(n, a)).collect()
    });
    println!(
        "  {:<10} {:<8} {:<12} {:<10} {}",
        t!("cli.agent.col_agent"),
        t!("cli.agent.col_mode"),
        t!("cli.agent.col_available"),
        t!("cli.agent.col_model"),
        t!("cli.agent.col_detail")
    );
    let mut outdated = Vec::new();
    for ((name, adapter, configured), st) in rows.iter().zip(statuses) {
        let mode = match st.mode {
            AdapterMode::Headless => "headless",
            AdapterMode::Export => "export",
        };
        let mark = if *name == config.agent.default {
            "*"
        } else {
            " "
        };
        // Distinta del predefinido actual (versión anterior de adoc o editada a mano).
        let stale = *configured
            && builtin.get(name).is_some_and(|b| {
                b.mode != adapter.mode || b.command != adapter.command || b.args != adapter.args
            });
        let detail = if !adapter.trusted && adapter.mode == AdapterMode::Headless {
            st.detail.clone()
        } else if !configured {
            t!("cli.agent.builtin_detail", detail = st.detail, name = name)
        } else if stale {
            outdated.push(name.clone());
            t!("cli.agent.differs_detail", detail = st.detail)
        } else {
            st.detail.clone()
        };
        println!(
            "{mark} {:<10} {:<8} {:<12} {:<10} {}",
            name,
            mode,
            if st.available {
                t!("cli.commands.yes")
            } else {
                t!("cli.commands.no")
            },
            adapter.model.as_deref().unwrap_or("—"),
            detail
        );
    }
    println!("{}", t!("cli.agent.active", name = config.agent.default));
    for name in outdated {
        println!("{}", t!("cli.agent.outdated", name = name));
    }
}

fn test(config: &ArcheologyConfig, agent: Option<String>) {
    let names: Vec<String> = match agent {
        Some(a) => vec![a],
        None => config.agent.adapters.keys().cloned().collect(),
    };
    for name in names {
        match config.agent.adapters.get(&name) {
            Some(adapter) => {
                let st = check_adapter(&name, adapter);
                println!("{name}: {}", st.detail);
                for w in archeology_agents::adapter_warnings(&name, adapter) {
                    println!("  {}", t!("core.common.warning", message = w));
                }
            }
            None => println!("{}", t!("cli.agent.not_in_config", name = name)),
        }
    }
}

/// Muestra la definición del adaptador y la aprueba tras confirmar.
fn trust(project: &Project, name: &str, yes: bool) -> Result<()> {
    let adapter = project
        .config
        .agent
        .adapters
        .get(name)
        .with_context(|| t!("cli.commands.adapter_undefined", agent = name))?;
    if adapter.mode == AdapterMode::Export || archeology_core::trust::is_builtin(name, adapter) {
        println!("{}", t!("cli.agent.no_trust_needed", name = name));
        return Ok(());
    }
    // Todo se muestra escapado ({:?}): un carácter de control en el TOML no
    // puede ocultar ni falsear lo que se aprueba en la terminal.
    println!(
        "{}",
        t!(
            "cli.agent.trust_header",
            name = format!("{name:?}"),
            root = format!("{:?}", project.root.display().to_string())
        )
    );
    match adapter.command.as_deref() {
        Some(c) => println!("  command = {c:?}"),
        None => println!("  command = {}", t!("cli.agent.command_none")),
    }
    println!("  args    = {:?}", adapter.args);
    if !adapter.interactive_args.is_empty() {
        println!("  interactive_args = {:?}", adapter.interactive_args);
    }
    for (k, v) in &adapter.env {
        println!("  env     {k:?} = {v:?}");
    }
    println!("  stdin   = {}", adapter.stdin);
    match adapter.model.as_deref() {
        Some(m) => println!("  model   = {m:?}"),
        None => println!("  model   = {}", t!("cli.agent.model_default")),
    }
    println!("{}", t!("cli.agent.trust_warning"));
    if !super::confirm(&t!("cli.agent.trust_confirm"), yes)? {
        println!("{}", t!("cli.agent.not_approved"));
        return Ok(());
    }
    archeology_core::trust::trust(&project.root, name, adapter)?;
    println!("{}", t!("cli.agent.approved", name = name));
    Ok(())
}

/// Ajusta el estado de las tareas al cambiar de agente.
///
/// Con un agente headless, las tareas `manual` (exportadas para un agente sin
/// CLI y no completadas) vuelven a `pending`. Con un agente de exportación no
/// hace falta cambiar nada: `inspect` exporta las `pending`. Las `done` nunca
/// se tocan. Devuelve cuántas tareas se reabrieron.
pub fn reconcile_tasks(project: &Project, mode: AdapterMode) -> Result<usize> {
    if mode != AdapterMode::Headless || !project.tasks_dir().is_dir() {
        return Ok(0);
    }
    let mut state = archeology_core::state::State::load(&project.state_file())?;
    let mut reopened = 0;
    for e in fs::read_dir(project.tasks_dir())?.flatten() {
        let path = e.path();
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Some(meta) = archeology_core::planner::parse_task(&text) else {
            continue;
        };
        if meta.status == "manual" {
            archeology_core::fsutil::atomic_write(
                &path,
                archeology_core::planner::set_status_in_markdown(&text, "pending").as_bytes(),
            )?;
            state.tasks.insert(meta.task_id, "pending".into());
            reopened += 1;
        }
    }
    if reopened > 0 {
        state.save(&project.state_file())?;
    }
    Ok(reopened)
}

/// ¿Hay tareas en modo manual en el proyecto?
fn has_manual_tasks(project: &Project) -> bool {
    fs::read_dir(project.tasks_dir())
        .map(|entries| {
            entries.flatten().any(|e| {
                fs::read_to_string(e.path())
                    .ok()
                    .and_then(|t| archeology_core::planner::parse_task(&t))
                    .is_some_and(|m| m.status == "manual")
            })
        })
        .unwrap_or(false)
}

/// Fija `agent.default` (y `model`) en `archeology-doc.toml` conservando
/// el formato y los comentarios del archivo.
fn use_agent(
    project: &Project,
    name: &str,
    model: Option<&str>,
    no_model: bool,
    reset: bool,
) -> Result<()> {
    if let Some(m) = model {
        archeology_agents::adapter::validate_model(m)?;
    }
    let path = project.config_path();
    let text = fs::read_to_string(&path)?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| t!("cli.agent.invalid_config_file"))?;

    // Un adaptador predefinido que falte en la config (proyectos antiguos) se añade.
    let builtin = ArcheologyConfig::default().agent.adapters;
    // Se mira el archivo, no `project.config`: `load()` completa los predefinidos.
    let defined = doc
        .get("agent")
        .and_then(|a| a.get("adapters"))
        .and_then(|t| t.get(name))
        .is_some();
    if reset && !builtin.contains_key(name) {
        anyhow::bail!(t!("cli.agent.not_builtin", name = name));
    }
    if !defined || reset {
        let Some(adapter) = builtin.get(name) else {
            let mut known: Vec<&String> = project.config.agent.adapters.keys().collect();
            known.extend(
                builtin
                    .keys()
                    .filter(|k| !project.config.agent.adapters.contains_key(*k)),
            );
            anyhow::bail!(t!(
                "cli.agent.unknown_agent",
                name = name,
                available = known
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        };
        let table: toml_edit::DocumentMut = toml::to_string(adapter)?.parse()?;
        let adapters = doc["agent"]["adapters"]
            .or_insert(toml_edit::table())
            .as_table_mut()
            .with_context(|| t!("cli.agent.adapters_not_table"))?;
        adapters.set_implicit(true);
        adapters.insert(name, toml_edit::Item::Table(table.as_table().clone()));
        if defined {
            println!("{}", t!("cli.agent.adapter_reset", name = name));
        } else {
            println!("{}", t!("cli.agent.adapter_added", name = name));
        }
    }

    doc["agent"]["default"] = toml_edit::value(name);
    let adapter = &mut doc["agent"]["adapters"][name];
    if let Some(m) = model {
        adapter["model"] = toml_edit::value(m);
    } else if no_model {
        if let Some(t) = adapter.as_table_like_mut() {
            t.remove("model");
        }
    }

    // Validar antes de escribir: nunca dejar una config rota.
    let updated = doc.to_string();
    let parsed: ArcheologyConfig = toml::from_str(&updated)?;
    parsed.validate()?;
    // Sin atravesar symlinks del repositorio, de forma atómica.
    archeology_core::fsutil::write_in_root(&project.root, &path, updated.as_bytes())?;
    // Recarga completa: predefinidos, patrones de secretos y confianza.
    let parsed = ArcheologyConfig::load(&path)?;

    let adapter = &parsed.agent.adapters[name];
    // ¿Puede ejecutar tareas? Solo entonces se reabren las manuales: con un
    // agente sin aprobar o no instalado, quedarían `pending` sin poder correr.
    let status = (adapter.mode == AdapterMode::Headless).then(|| {
        crate::ui::with_spinner(t!("cli.agent.checking_agent", name = name), || {
            check_adapter(name, adapter)
        })
    });
    // Basta con que el comando exista (algunos CLIs no aceptan `--version`).
    let runnable = adapter.mode == AdapterMode::Headless
        && adapter.trusted
        && (status.as_ref().is_some_and(|st| st.available)
            || archeology_agents::adapter::command_found(adapter));
    // Las tareas reflejan el agente actual, no el que se usó al exportarlas.
    if runnable {
        let reopened = reconcile_tasks(project, adapter.mode)?;
        if reopened > 0 {
            println!(
                "{}",
                t!("cli.agent.tasks_reopened", count = reopened, name = name)
            );
        }
    } else if adapter.mode == AdapterMode::Headless && has_manual_tasks(project) {
        println!("{}", t!("cli.agent.manual_not_reopened", name = name));
    }
    let model = adapter
        .model
        .as_deref()
        .map(|m| t!("cli.agent.model_suffix", model = m))
        .unwrap_or_default();
    println!(
        "{}",
        t!("cli.agent.active_agent", name = name, model = model)
    );
    match adapter.mode {
        AdapterMode::Export => println!("{}", t!("cli.agent.export_mode", name = name)),
        AdapterMode::Headless => {
            if let Some(st) = status.filter(|st| !st.available) {
                println!("{}", t!("core.common.warning", message = st.detail));
            }
            for w in archeology_agents::adapter_warnings(name, adapter) {
                println!("{}", t!("core.common.warning", message = w));
            }
            if let Some(w) = archeology_agents::adapter::unrestricted_warning(name, adapter) {
                println!("{}", t!("core.common.warning", message = w));
            }
        }
    }
    Ok(())
}
