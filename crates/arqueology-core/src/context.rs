//! Contexto para agentes: `AGENTS.md`, adaptadores por herramienta y skills
//! (RF-100 a RF-106).
//!
//! Todo se escribe en regiones gestionadas: el resto de cada archivo es del
//! usuario (o de OpenSpec) y nunca se toca (RF-162).

use crate::managed;
use crate::model::Inventory;
use crate::project::Project;
use crate::templates;
use anyhow::Result;
use std::fs;
use std::path::Path;

/// Comando del proyecto detectado de forma determinista.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectCommand {
    /// build|test|lint|format|run
    pub kind: &'static str,
    pub command: String,
}

/// Detecta comandos de build, test y lint a partir de los manifiestos de la raíz.
pub fn detect_commands(root: &Path, inv: &Inventory) -> Vec<ProjectCommand> {
    let mut out = Vec::new();
    let mut push = |kind: &'static str, command: &str| {
        if !out
            .iter()
            .any(|c: &ProjectCommand| c.kind == kind && c.command == command)
        {
            out.push(ProjectCommand {
                kind,
                command: command.to_string(),
            });
        }
    };

    if root.join("Cargo.toml").is_file() {
        push("build", "cargo build --workspace");
        push("test", "cargo test --workspace");
        push(
            "lint",
            "cargo clippy --workspace --all-targets -- -D warnings",
        );
        push("format", "cargo fmt --all");
    }
    if let Ok(text) = fs::read_to_string(root.join("package.json")) {
        let pm = if root.join("pnpm-lock.yaml").is_file() {
            "pnpm"
        } else if root.join("yarn.lock").is_file() {
            "yarn"
        } else {
            "npm"
        };
        let scripts = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("scripts").cloned())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        for (script, kind) in [
            ("build", "build"),
            ("test", "test"),
            ("lint", "lint"),
            ("format", "format"),
            ("dev", "run"),
            ("start", "run"),
        ] {
            if scripts.contains_key(script) {
                let cmd = if script == "test" || script == "start" {
                    format!("{pm} {script}")
                } else {
                    format!("{pm} run {script}")
                };
                push(kind, &cmd);
            }
        }
    }
    if root.join("go.mod").is_file() {
        push("build", "go build ./...");
        push("test", "go test ./...");
        push("lint", "go vet ./...");
    }
    if root.join("pom.xml").is_file() {
        push("build", "mvn -q package");
        push("test", "mvn test");
    }
    if root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file() {
        let gradle = if root.join("gradlew").is_file() {
            "./gradlew"
        } else {
            "gradle"
        };
        push("build", &format!("{gradle} build"));
        push("test", &format!("{gradle} test"));
    }
    let python = [
        "pyproject.toml",
        "requirements.txt",
        "setup.py",
        "setup.cfg",
    ]
    .iter()
    .any(|f| root.join(f).is_file())
        || inv.languages.contains_key("Python");
    if python && inv.test_files.iter().any(|t| t.ends_with(".py")) {
        if inv.frameworks.iter().any(|f| f == "pytest")
            || inv.test_frameworks.iter().any(|f| f == "pytest")
        {
            push("test", "pytest");
        } else {
            push("test", "python -m unittest discover");
        }
    }
    if let Ok(make) = fs::read_to_string(root.join("Makefile")) {
        for target in ["build", "test", "lint"] {
            if make.lines().any(|l| l.starts_with(&format!("{target}:"))) {
                push(
                    match target {
                        "build" => "build",
                        "test" => "test",
                        _ => "lint",
                    },
                    &format!("make {target}"),
                );
            }
        }
    }
    out
}

/// Región `project` de AGENTS.md, derivada del inventario (RF-100).
pub fn project_region(project: &Project, inv: &Inventory) -> String {
    let mut s = String::from("## Proyecto (inventario de arqueology-doc)\n\n");

    let commands = detect_commands(&project.root, inv);
    s.push_str("### Comandos\n\n");
    if commands.is_empty() {
        s.push_str(
            "- (no se detectaron comandos de build/test; documéntalos aquí fuera de la región)\n",
        );
    }
    for c in &commands {
        s.push_str(&format!("- {}: `{}`\n", c.kind, c.command));
    }

    if !inv.entrypoints.is_empty() {
        s.push_str("\n### Puntos de entrada\n\n");
        for e in inv.entrypoints.iter().take(15) {
            s.push_str(&format!("- `{e}`\n"));
        }
    }

    if !inv.modules.is_empty() {
        s.push_str("\n### Módulos\n\n");
        let know = project.config.paths.knowledge.trim_end_matches('/');
        for m in inv.modules.iter().take(25) {
            s.push_str(&format!(
                "- `{}` ({} archivos) — `{know}/modules/{}.md`\n",
                m.path, m.files, m.name
            ));
        }
    }

    s.push_str("\n### Zonas sensibles\n\n");
    s.push_str(&format!(
        "- Nunca leas ni cites archivos de secretos: {}.\n",
        project
            .config
            .scan
            .secrets_patterns
            .iter()
            .map(|p| format!("`{p}`"))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    if inv.openspec.is_some() {
        s.push_str("- `openspec/specs/` es normativo y de solo lectura; propone cambios en `openspec/changes/`.\n");
    }
    if !inv.ci_files.is_empty() {
        s.push_str(&format!(
            "- CI definido en: {}.\n",
            inv.ci_files
                .iter()
                .map(|c| format!("`{c}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    s.trim_end().to_string()
}

/// Escribe o actualiza AGENTS.md: región `context` (protocolo) y, si hay
/// inventario, región `project` (comandos, módulos, zonas sensibles).
pub fn write_agents_md(project: &Project, inv: Option<&Inventory>) -> Result<bool> {
    let path = project.root.join("AGENTS.md");
    let existing = managed::read_or_default(&path, || {
        format!(
            "# {}\n\nContexto para agentes de IA.\n",
            project.config.project.name
        )
    })?;
    let context = templates::render(
        "agents.md",
        &[
            ("project_name", &project.config.project.name),
            ("knowledge_dir", &project.config.paths.knowledge),
        ],
    )?;
    let mut updated = managed::upsert_region(&existing, "context", context.trim_end())?;
    if let Some(inv) = inv {
        updated = managed::upsert_region(&updated, "project", &project_region(project, inv))?;
    }
    write_if_changed(&path, &updated)
}

/// Archivos por herramienta que referencian AGENTS.md sin duplicarlo (RF-101).
pub fn write_adapters(project: &Project) -> Result<Vec<String>> {
    let mut written = Vec::new();
    let pointer = "Las instrucciones de este proyecto viven en `AGENTS.md` (contexto, \
                   comandos, memoria y reglas). Léelo completo antes de trabajar; no \
                   dupliques aquí su contenido.";
    for adapter in &project.config.context.adapters {
        let (rel, header, body) = match adapter.as_str() {
            "claude" => (
                "CLAUDE.md",
                String::new(),
                format!("@AGENTS.md\n\n{pointer}"),
            ),
            "cursor" => (
                ".cursor/rules/arqueology-doc.mdc",
                "---\ndescription: Contexto del proyecto (arqueology-doc)\nalwaysApply: true\n---\n\n"
                    .to_string(),
                pointer.to_string(),
            ),
            "copilot" => (
                ".github/copilot-instructions.md",
                String::new(),
                pointer.to_string(),
            ),
            _ => continue,
        };
        let path = project.root.join(rel);
        let existing = managed::read_or_default(&path, || header)?;
        let updated = managed::upsert_region(&existing, "pointer", &body)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if write_if_changed(&path, &updated)? {
            written.push(rel.to_string());
        }
    }
    Ok(written)
}

/// Skills reutilizables en `.agents/skills/<nombre>/SKILL.md` (RF-106).
pub fn write_skills(project: &Project, inv: &Inventory) -> Result<Vec<String>> {
    let mut written = Vec::new();
    if !project.config.context.skills {
        return Ok(written);
    }
    let know = project.config.paths.knowledge.trim_end_matches('/');
    let mut skills: Vec<(&str, &str, String)> = vec![(
        "project-memory",
        "Cómo leer y actualizar la memoria del proyecto (hechos, convenciones, decisiones y sesiones).",
        format!(
            "1. Al empezar, lee `{know}/memory/facts.md`, `conventions.md`, `decisions.md` y \
             `open-questions.md`.\n\
             2. Al terminar, crea `{know}/memory/sessions/<AAAA-MM-DD>-<tema>.md` con la \
             plantilla `{know}/_templates/session.md`.\n\
             3. Promueve a la memoria estable solo lo verificado, con el formato \
             `- AAAA-MM-DD · afirmación · fuente: ruta:línea`.\n\
             4. Si algo contradice la memoria, regístralo en `open-questions.md` en vez de \
             sobrescribir la entrada."
        ),
    )];
    let commands = detect_commands(&project.root, inv);
    let tests: Vec<&ProjectCommand> = commands
        .iter()
        .filter(|c| c.kind == "test" || c.kind == "lint" || c.kind == "build")
        .collect();
    if !tests.is_empty() {
        let mut body = String::from("Comandos detectados en los manifiestos del proyecto:\n\n");
        for c in tests {
            body.push_str(&format!("- {}: `{}`\n", c.kind, c.command));
        }
        body.push_str(
            "\nEjecuta primero el build, luego los tests. Si un test falla, cita el archivo \
             y la línea del fallo en lugar de resumirlo.",
        );
        skills.push((
            "run-tests",
            "Cómo compilar, ejecutar los tests y el linter de este proyecto.",
            body,
        ));
    }

    for (name, description, body) in skills {
        let rel = format!(".agents/skills/{name}/SKILL.md");
        let path = project.root.join(&rel);
        let existing = managed::read_or_default(&path, || {
            format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n")
        })?;
        let updated = managed::upsert_region(&existing, "skill", &body)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if write_if_changed(&path, &updated)? {
            written.push(rel);
        }
    }
    Ok(written)
}

/// Regenera todo el contexto para agentes. Devuelve los archivos modificados.
pub fn refresh(project: &Project, inv: Option<&Inventory>) -> Result<Vec<String>> {
    let mut changed = Vec::new();
    if write_agents_md(project, inv)? {
        changed.push("AGENTS.md".to_string());
    }
    changed.extend(write_adapters(project)?);
    if let Some(inv) = inv {
        changed.extend(write_skills(project, inv)?);
    }
    Ok(changed)
}

fn write_if_changed(path: &Path, content: &str) -> Result<bool> {
    if fs::read_to_string(path).ok().as_deref() == Some(content) {
        return Ok(false);
    }
    fs::write(path, content)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_npm_scripts_and_cargo() {
        let tmp = std::env::temp_dir().join(format!("arq-ctx-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(
            tmp.join("package.json"),
            r#"{"scripts":{"test":"vitest","lint":"eslint ."}}"#,
        )
        .unwrap();
        fs::write(tmp.join("Cargo.toml"), "[package]\n").unwrap();
        let cmds = detect_commands(&tmp, &Inventory::default());
        let list: Vec<String> = cmds.iter().map(|c| c.command.clone()).collect();
        assert!(list.contains(&"npm test".to_string()));
        assert!(list.contains(&"npm run lint".to_string()));
        assert!(list.contains(&"cargo test --workspace".to_string()));
        fs::remove_dir_all(&tmp).ok();
    }
}
