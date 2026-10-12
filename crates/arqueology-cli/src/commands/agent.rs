//! `agent` — lista, prueba y selecciona adaptadores de agentes (§13).

use anyhow::{Context, Result};
use arqueology_agents::check_adapter;
use arqueology_core::config::{AdapterMode, ArqueologyConfig};
use arqueology_core::Project;
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
    /// Aprueba un adaptador definido en arqueology-doc.toml para este proyecto.
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
            if arqueology_core::trust::revoke(&project.root, &name)? {
                println!("aprobación de '{name}' retirada");
            } else {
                println!("'{name}' no tenía aprobación en este proyecto");
            }
            Ok(())
        }
        cmd => {
            // Los adaptadores pueden vivir en la config del proyecto; si no hay
            // proyecto, se usan los predefinidos.
            let config = super::resolve_project(&project_arg)
                .map(|p| p.config)
                .unwrap_or_default();
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

fn list(config: &ArqueologyConfig) {
    let builtin = arqueology_core::config::builtin_adapters();
    // Configurados en el proyecto + predefinidos que aún no están en la config.
    let mut rows: Vec<(String, arqueology_core::config::AdapterConfig, bool)> = config
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
    let statuses: Vec<_> = crate::ui::with_spinner("Comprobando agentes…", || {
        rows.iter().map(|(n, a, _)| check_adapter(n, a)).collect()
    });
    println!(
        "  {:<10} {:<8} {:<12} {:<10} detalle",
        "agente", "modo", "disponible", "modelo"
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
            format!(
                "{} · predefinido (lo añade `adoc agent use {name}`)",
                st.detail
            )
        } else if stale {
            outdated.push(name.clone());
            format!("{} · distinta de la predefinida", st.detail)
        } else {
            st.detail.clone()
        };
        println!(
            "{mark} {:<10} {:<8} {:<12} {:<10} {}",
            name,
            mode,
            if st.available { "sí" } else { "no" },
            adapter.model.as_deref().unwrap_or("—"),
            detail
        );
    }
    println!(
        "\nactivo: {} · cambiar con `adoc agent use <agente> [--model <modelo>]`",
        config.agent.default
    );
    for name in outdated {
        println!(
            "{name} difiere de la definición predefinida; para usarla: `adoc agent use {name} --reset`"
        );
    }
}

fn test(config: &ArqueologyConfig, agent: Option<String>) {
    let names: Vec<String> = match agent {
        Some(a) => vec![a],
        None => config.agent.adapters.keys().cloned().collect(),
    };
    for name in names {
        match config.agent.adapters.get(&name) {
            Some(adapter) => {
                let st = check_adapter(&name, adapter);
                println!("{name}: {}", st.detail);
                for w in arqueology_agents::adapter_warnings(&name, adapter) {
                    println!("  aviso: {w}");
                }
            }
            None => println!("{name}: no definido en la configuración"),
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
        .with_context(|| format!("adaptador '{name}' no definido"))?;
    if adapter.mode == AdapterMode::Export || arqueology_core::trust::is_builtin(adapter) {
        println!("'{name}' es predefinido o de exportación: no necesita aprobación");
        return Ok(());
    }
    println!(
        "Adaptador [agent.adapters.{name}] de {}:",
        project.root.display()
    );
    println!(
        "  command = {}",
        adapter.command.as_deref().unwrap_or("(ninguno)")
    );
    println!("  args    = {:?}", adapter.args);
    if !adapter.interactive_args.is_empty() {
        println!("  interactive_args = {:?}", adapter.interactive_args);
    }
    for (k, v) in &adapter.env {
        println!("  env     {k}={v}");
    }
    println!("  stdin   = {}", adapter.stdin);
    println!("\nEste comando se ejecutará en tu máquina con tus permisos y tus credenciales.");
    if !super::confirm("¿Aprobar este adaptador?", yes)? {
        println!("no se aprobó");
        return Ok(());
    }
    arqueology_core::trust::trust(&project.root, name, adapter)?;
    println!("'{name}' aprobado para este proyecto");
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
    let mut state = arqueology_core::state::State::load(&project.state_file())?;
    let mut reopened = 0;
    for e in fs::read_dir(project.tasks_dir())?.flatten() {
        let path = e.path();
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Some(meta) = arqueology_core::planner::parse_task(&text) else {
            continue;
        };
        if meta.status == "manual" {
            fs::write(
                &path,
                arqueology_core::planner::set_status_in_markdown(&text, "pending"),
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

/// Fija `agent.default` (y `model`) en `arqueology-doc.toml` conservando
/// el formato y los comentarios del archivo.
fn use_agent(
    project: &Project,
    name: &str,
    model: Option<&str>,
    no_model: bool,
    reset: bool,
) -> Result<()> {
    let path = project.config_path();
    let text = fs::read_to_string(&path)?;
    let mut doc: toml_edit::DocumentMut = text.parse().context("arqueology-doc.toml inválido")?;

    // Un adaptador predefinido que falte en la config (proyectos antiguos) se añade.
    let builtin = ArqueologyConfig::default().agent.adapters;
    // Se mira el archivo, no `project.config`: `load()` completa los predefinidos.
    let defined = doc
        .get("agent")
        .and_then(|a| a.get("adapters"))
        .and_then(|t| t.get(name))
        .is_some();
    if reset && !builtin.contains_key(name) {
        anyhow::bail!("'{name}' no es un agente predefinido; no hay definición a la que volver");
    }
    if !defined || reset {
        let Some(adapter) = builtin.get(name) else {
            let mut known: Vec<&String> = project.config.agent.adapters.keys().collect();
            known.extend(
                builtin
                    .keys()
                    .filter(|k| !project.config.agent.adapters.contains_key(*k)),
            );
            anyhow::bail!(
                "agente '{name}' desconocido; disponibles: {} (o define [agent.adapters.{name}])",
                known
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        };
        let table: toml_edit::DocumentMut = toml::to_string(adapter)?.parse()?;
        let adapters = doc["agent"]["adapters"]
            .or_insert(toml_edit::table())
            .as_table_mut()
            .context("[agent.adapters] no es una tabla")?;
        adapters.set_implicit(true);
        adapters.insert(name, toml_edit::Item::Table(table.as_table().clone()));
        if defined {
            println!("restablecido el adaptador predefinido [agent.adapters.{name}]");
        } else {
            println!("añadido el adaptador predefinido [agent.adapters.{name}]");
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
    let parsed: ArqueologyConfig = toml::from_str(&updated)?;
    parsed.validate()?;
    fs::write(&path, updated)?;
    // Recarga completa: predefinidos, patrones de secretos y confianza.
    let parsed = ArqueologyConfig::load(&path)?;

    let adapter = &parsed.agent.adapters[name];
    // Las tareas reflejan el agente actual, no el que se usó al exportarlas.
    let reopened = reconcile_tasks(project, adapter.mode)?;
    if reopened > 0 {
        println!(
            "{reopened} tareas exportadas en modo manual vuelven a `pending` para {name}: \
             ejecútalas con `adoc inspect`"
        );
    }
    let model = adapter
        .model
        .as_deref()
        .map(|m| format!(" · modelo {m}"))
        .unwrap_or_default();
    println!("agente activo: {name}{model}");
    match adapter.mode {
        AdapterMode::Export => println!(
            "modo exportación: `adoc inspect` marcará las tareas como manuales; ejecútalas en {name} \
             y ciérralas con `adoc task done <id>`"
        ),
        AdapterMode::Headless => {
            let st = crate::ui::with_spinner(format!("Comprobando {name}…"), || {
                check_adapter(name, adapter)
            });
            if !st.available {
                println!("aviso: {}", st.detail);
            }
            for w in arqueology_agents::adapter_warnings(name, adapter) {
                println!("aviso: {w}");
            }
            if let Some(w) = arqueology_agents::adapter::unrestricted_warning(name, adapter) {
                println!("aviso: {w}");
            }
        }
    }
    Ok(())
}
