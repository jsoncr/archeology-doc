//! Smoke tests de punta a punta sobre proyectos temporales.

use assert_cmd::Command;
use std::fs;
use std::path::Path;

/// Proyecto temporal con su propio directorio de configuración global.
struct Sandbox {
    _tmp: tempfile::TempDir,
    root: std::path::PathBuf,
    config: std::path::PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        let config = tmp.path().join("config");
        fs::create_dir_all(&root).unwrap();
        Self {
            _tmp: tmp,
            root,
            config,
        }
    }

    fn cmd(&self) -> Command {
        let mut cmd = adoc_cmd("archeology-doc", &self.config);
        cmd.current_dir(&self.root);
        cmd
    }

    /// Como [`Self::cmd`], sin idioma fijado: sin `ADOC_LANG` ni variables
    /// de locale salvo `LANG=C` (resolución determinista).
    fn cmd_without_lang(&self) -> Command {
        let mut cmd = self.cmd();
        cmd.env_remove("ADOC_LANG")
            .env_remove("LC_ALL")
            .env_remove("LC_MESSAGES")
            .env("LANG", "C");
        cmd
    }

    /// Espacio de trabajo global del proyecto (`<config>/projects/<nombre>-<id8>`).
    fn work(&self) -> std::path::PathBuf {
        let mut dirs: Vec<_> = fs::read_dir(self.config.join("projects"))
            .expect("sin espacio de trabajo: ¿se ejecutó init?")
            .flatten()
            .map(|e| e.path())
            .collect();
        assert_eq!(dirs.len(), 1, "{dirs:?}");
        dirs.pop().unwrap()
    }

    fn run(&self, args: &[&str]) -> String {
        let out = self.cmd().args(args).assert().success();
        String::from_utf8_lossy(&out.get_output().stdout).to_string()
    }
}

/// Binario `bin` con el registro global aislado (`config`) y la interfaz en
/// español (`ADOC_LANG=es`): las aserciones de estos tests usan los textos
/// en español. Los tests de idioma lo sobrescriben.
fn adoc_cmd(bin: &str, config: &Path) -> Command {
    let mut cmd = Command::cargo_bin(bin).unwrap();
    // Registro global aislado: nunca tocar el del usuario.
    cmd.env("ARCHEOLOGY_DOC_CONFIG_DIR", config)
        .env("ADOC_LANG", "es");
    cmd
}

fn fixture(dir: &Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/main.py"),
        "def main():\n    pass  # TODO: implementar\n",
    )
    .unwrap();
    fs::write(dir.join("README.md"), "# demo\n").unwrap();
}

#[test]
fn init_scan_plan_check_flow() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    let root = &sb.root;

    sb.run(&["init", "--yes"]);
    assert!(root.join("archeology-doc.toml").is_file());
    assert!(root.join("AGENTS.md").is_file());
    assert!(root.join("knowledge/_templates/feature.md").is_file());
    assert!(sb.config.join("registry.toml").is_file());

    sb.run(&["scan"]);
    assert!(sb.work().join("inventory.json").is_file());
    // El proyecto solo contiene resultados: el estado vive fuera.
    assert!(!root.join(".archeology-doc").exists());
    assert!(root.join("knowledge/00-overview.md").is_file());

    sb.run(&["plan"]);
    assert!(sb.work().join("tasks/T-001.md").is_file());

    assert!(sb.run(&["task", "list"]).contains("T-001"));
    assert!(sb.run(&["inspect", "--dry-run"]).contains("T-001"));
    sb.run(&["check"]);
}

#[test]
fn scan_excludes_own_output_and_sees_ci_and_root_tests() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    fs::create_dir_all(sb.root.join(".github/workflows")).unwrap();
    fs::write(sb.root.join(".github/workflows/ci.yml"), "on: push\n").unwrap();
    fs::create_dir_all(sb.root.join("tests")).unwrap();
    fs::write(sb.root.join("tests/check_main.py"), "def t(): pass\n").unwrap();
    fs::write(sb.root.join(".env"), "SECRET=1\n").unwrap();

    sb.run(&["init", "--yes"]);
    let inv: serde_json::Value = serde_json::from_str(&sb.run(&["scan", "--json"])).unwrap();

    let list = |k: &str| -> Vec<String> {
        inv[k]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(list("ci_files"), vec![".github/workflows/ci.yml"]);
    assert!(list("test_files").contains(&"tests/check_main.py".to_string()));
    // La salida de archeology-doc y los secretos no forman parte del inventario.
    let docs = list("doc_files");
    assert!(!docs
        .iter()
        .any(|d| d.starts_with("docs/") || d == "AGENTS.md"));
    let modules: Vec<&str> = inv["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert!(!modules.contains(&"docs"));
    assert!(!modules.contains(&"knowledge"));
    assert!(!inv.to_string().contains(".env"));
}

/// Agente falso: escribe una nota válida por cada entrada de `# Produce`.
#[cfg(unix)]
const FAKE_AGENT: &str = r#"
f="$1"
awk '/^# Produce/{p=1;next} /^# /{p=0} p && /^- /{l=substr($0,3); sub(/ \(.*\)$/, "", l); gsub(/`/, "", l); print l}' "$f" |
while read -r out; do
  case "$out" in
    */) mkdir -p "$out"; n="${out}note-$(basename "$f" .md).md" ;;
    *) mkdir -p "$(dirname "$out")"; n="$out" ;;
  esac
  case "$n" in
    knowledge/*) printf -- '---\nid: %s\ntype: module\ntitle: t\nsources:\n  - README.md:1\n---\n' "$(echo "$n" | tr '/.' '--')" > "$n" ;;
    *) echo "x" >> "$n" ;;
  esac
done
"#;

#[cfg(unix)]
fn add_adapter(sb: &Sandbox, name: &str, script: &str) {
    let root = &sb.root;
    let mut cfg = fs::read_to_string(root.join("archeology-doc.toml")).unwrap();
    let script = script
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    cfg.push_str(&format!(
        "\n[agent.adapters.{name}]\ncommand = \"sh\"\nargs = [\"-c\", \"{script}\", \"sh\", \"{{prompt_file}}\"]\n"
    ));
    fs::write(root.join("archeology-doc.toml"), cfg).unwrap();
    // Un adaptador propio no se ejecuta sin aprobación.
    sb.run(&["agent", "trust", name, "--yes"]);
}

#[cfg(unix)]
#[test]
fn inspect_runs_all_dependency_layers_in_one_run() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes"]);
    add_adapter(&sb, "fake", FAKE_AGENT);
    sb.run(&["scan"]);
    sb.run(&["plan"]);

    let out = sb.run(&["inspect", "--agent", "fake"]);
    assert!(!out.contains("falló"), "{out}");
    let list = sb.run(&["task", "list"]);
    assert!(!list.contains("pending"), "{list}");
    assert!(sb
        .root
        .join("knowledge/features")
        .read_dir()
        .unwrap()
        .next()
        .is_some());
    sb.run(&["check"]);
    // Log legible por corrida (RNF-09), en el espacio de trabajo.
    let run = fs::read_dir(sb.work().join("runs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let log = fs::read_to_string(run.path().join("run.log")).unwrap();
    assert!(log.contains("T-001 done"), "{log}");
    // Tiempos para el dashboard: última ejecución por tarea y totales.
    let st: serde_json::Value = serde_json::from_str(&sb.run(&["status"])).unwrap();
    assert_eq!(st["tasks"][0]["last_run"]["status"], "done");
    assert_eq!(st["tasks"][0]["last_run"]["attempts"], 1);
    assert_eq!(st["timing"]["runs"], 1);
    assert!(st["timing"]["avg_done_ms"].is_number(), "{}", st["timing"]);
    // Datos del proyecto (inventario) para el dashboard.
    let inv = &st["inventory"];
    assert!(inv["total_files"].as_u64().unwrap() >= 2, "{inv}");
    assert!(inv["total_lines"].as_u64().unwrap() > 0, "{inv}");
    assert!(inv["languages"][0]["name"].is_string(), "{inv}");
}

#[cfg(unix)]
#[test]
fn inspect_fails_task_without_output_and_blocks_dependents() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes"]);
    add_adapter(&sb, "lazy", "true");
    sb.run(&["scan"]);
    sb.run(&["plan"]);

    let out = sb.run(&["inspect", "--agent", "lazy"]);
    assert!(out.contains("falló T-001"), "{out}");
    assert!(
        out.contains("bloqueada T-002") || out.contains("tareas bloqueadas"),
        "{out}"
    );
    assert!(!sb.run(&["task", "list"]).contains(" done "));
}

#[test]
fn task_done_requires_valid_output() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes"]);
    sb.run(&["scan"]);
    sb.run(&["plan"]);
    sb.run(&["inspect", "--agent", "manual"]);

    // T-002 produce knowledge/modules/src.md, que aún no existe.
    sb.cmd().args(["task", "done", "T-002"]).assert().failure();
    fs::write(
        sb.root.join("knowledge/modules/src.md"),
        "---\nid: mod-src\ntype: module\ntitle: src\nsources:\n  - src/main.py:1\n---\n",
    )
    .unwrap();
    sb.run(&["task", "done", "T-002"]);
}

#[test]
fn commands_fail_without_project() {
    let sb = Sandbox::new();
    sb.cmd().arg("scan").assert().failure();
}

#[test]
fn sync_marks_notes_stale_and_reopens_tasks() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes"]);
    sb.run(&["scan"]);
    sb.run(&["plan"]);
    sb.run(&["inspect", "--agent", "manual"]);

    let note = sb.root.join("knowledge/modules/src.md");
    fs::write(
        &note,
        "---\nid: mod-src\ntype: module\ntitle: src\nstatus: draft\nsources:\n  - src/main.py:1-2\n---\n",
    )
    .unwrap();
    let locked = sb.root.join("knowledge/decisions/keep.md");
    let locked_text =
        "---\nid: dec-keep\ntype: decision\nstatus: reviewed\nlocked: true\nsources:\n  - src/main.py:1\n---\n";
    fs::write(&locked, locked_text).unwrap();
    sb.run(&["task", "done", "T-002"]);
    assert!(sb.run(&["sync"]).contains("sin cambios"));

    fs::write(sb.root.join("src/main.py"), "def main():\n    return 1\n").unwrap();
    let out = sb.run(&["sync"]);
    assert!(out.contains("stale: knowledge/modules/src.md"), "{out}");
    assert!(out.contains("T-002"), "{out}");
    assert!(fs::read_to_string(&note).unwrap().contains("status: stale"));
    assert_eq!(fs::read_to_string(&locked).unwrap(), locked_text);
    assert!(sb
        .run(&["task", "list", "--status", "pending"])
        .contains("T-002"));

    // Un cambio se notifica una sola vez; `check --strict` falla con notas stale.
    assert!(sb.run(&["sync"]).contains("sin cambios"));
    sb.run(&["check"]);
    sb.cmd().args(["check", "--strict"]).assert().failure();
}

#[test]
fn scan_is_idempotent_and_preserves_overview_edits() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes"]);
    sb.run(&["scan"]);
    let overview = sb.root.join("knowledge/00-overview.md");
    let mut text = fs::read_to_string(&overview).unwrap();
    text.push_str("\n## Arquitectura\n\nTexto del agente.\n");
    fs::write(&overview, &text).unwrap();

    sb.run(&["scan"]);
    assert_eq!(fs::read_to_string(&overview).unwrap(), text);
}

#[test]
fn check_rejects_out_of_range_evidence() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes"]);
    fs::write(
        sb.root.join("knowledge/features/f.md"),
        "---\nid: feat-f\ntype: feature\ntitle: f\nsources:\n  - src/main.py:40-50\n---\n",
    )
    .unwrap();
    let out = sb.cmd().arg("check").assert().failure();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("fuera de rango"), "{err}");
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        let dest = to.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &dest);
        } else {
            fs::copy(&p, &dest).unwrap();
        }
    }
}

fn fixture_dir(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

/// Usa la validación estructural propia (sin depender del CLI de OpenSpec).
fn disable_openspec_cli(root: &Path) {
    let cfg = fs::read_to_string(root.join("archeology-doc.toml")).unwrap();
    let cfg = cfg.replace("validate_with_cli = true", "validate_with_cli = false");
    fs::write(root.join("archeology-doc.toml"), cfg).unwrap();
}

#[cfg(unix)]
#[test]
fn spec_status_reports_compliance_and_stale_changes() {
    let sb = Sandbox::new();
    copy_dir(&fixture_dir("with-openspec"), &sb.root);
    // Cambio sin actividad desde 2020 (sembrado como abandonado).
    std::process::Command::new("sh")
        .args([
            "-c",
            "find openspec/changes/add-export -exec touch -t 202001010000 {} +",
        ])
        .current_dir(&sb.root)
        .status()
        .unwrap();
    sb.run(&["init", "--yes"]);

    let out = sb.run(&["spec", "status"]);
    assert!(out.contains("unevaluated=3"), "{out}");
    assert!(out.contains("posiblemente abandonado"), "{out}");

    fs::write(
        sb.root.join("knowledge/specs/billing.md"),
        "---\nid: spec-billing\ntype: spec-report\ntitle: billing\ncapability: billing\n\
         requirements:\n  - id: billing/add-numbers\n    status: implemented\n    evidence: [src/main.py:4]\n\
         \x20 - id: billing/round-totals\n    status: contradicted\n    evidence: [src/main.py:8-10]\n\
         \x20 - id: billing/apply-discount\n    status: not-found\n---\n",
    )
    .unwrap();
    let out = sb.run(&["spec", "status"]);
    assert!(out.contains("contradicted=1"), "{out}");
    assert!(out.contains("implemented=1"), "{out}");
    assert!(out.contains("not-found=1"), "{out}");

    let review: serde_json::Value =
        serde_json::from_str(&sb.run(&["review", "--format", "json"])).unwrap();
    assert_eq!(review["openspec"]["by_status"]["contradicted"], 1);
    assert_eq!(review["openspec"]["stale_changes"][0], "add-export");
}

#[test]
fn spec_baseline_prepares_change_and_task() {
    let sb = Sandbox::new();
    copy_dir(&fixture_dir("legacy-py"), &sb.root);
    sb.run(&["init", "--yes"]);
    disable_openspec_cli(&sb.root);
    sb.run(&["scan"]);
    sb.run(&["plan"]);

    let out = sb.run(&["spec", "baseline"]);
    assert!(out.contains("T-BASE-"), "{out}");
    let changes = sb.root.join("openspec/changes");
    let change = fs::read_dir(&changes).unwrap().next().unwrap().unwrap();
    let name = change.file_name().to_string_lossy().to_string();
    assert!(name.starts_with("baseline-"));
    assert!(!sb.root.join("openspec/specs").exists());

    // Sin deltas todavía: la validación falla.
    sb.cmd()
        .args(["spec", "validate", &name])
        .assert()
        .failure();

    fs::create_dir_all(change.path().join("specs/app")).unwrap();
    fs::write(
        change.path().join("specs/app/spec.md"),
        "## ADDED Requirements\n\n### Requirement: Cap discount\n\n\
         The system SHALL cap discounts at 20 percent.\n\n\
         > Inferido por archeology-doc · confianza: medium · evidencia: app/billing.py:7\n\n\
         #### Scenario: Above cap\n\n- **WHEN** 30 percent is requested\n- **THEN** 20 percent is applied\n",
    )
    .unwrap();
    sb.run(&["spec", "validate", &name]);
}

#[test]
fn projects_list_shows_progress() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--name", "demo"]);
    sb.run(&["scan"]);
    sb.run(&["plan"]);
    let rows: serde_json::Value =
        serde_json::from_str(&sb.run(&["projects", "list", "--format", "json"])).unwrap();
    assert_eq!(rows[0]["name"], "demo");
    assert_eq!(rows[0]["progress"], 0);
    assert_eq!(rows[0]["open_findings"], 0);
}

#[test]
fn site_sync_builds_sidebar_wiki_and_llms_index() {
    let sb = Sandbox::new();
    copy_dir(&fixture_dir("with-openspec"), &sb.root);
    sb.run(&["init", "--yes"]);
    assert!(sb.root.join("docs/.vitepress/config.mts").is_file());
    fs::write(
        sb.root.join("knowledge/features/billing.md"),
        "---\nid: feat-billing\ntype: feature\ntitle: Facturación\nsources: [src/main.py:4]\n---\n\
         Ver [[facts|hechos]] y [[nada]]. Evidencia [main](../../src/main.py).\n",
    )
    .unwrap();
    sb.run(&["site", "sync"]);

    let sidebar =
        fs::read_to_string(sb.root.join("docs/.vitepress/sidebar.generated.json")).unwrap();
    assert!(sidebar.contains("/wiki/features/billing"), "{sidebar}");
    assert!(sidebar.contains("/specs/billing"), "{sidebar}");
    let page = fs::read_to_string(sb.root.join("docs/wiki/features/billing.md")).unwrap();
    assert!(page.contains("[hechos](/wiki/memory/facts)"), "{page}");
    assert!(page.contains("main (`../../src/main.py`)"), "{page}");
    let llms = fs::read_to_string(sb.root.join("llms.txt")).unwrap();
    assert!(
        llms.contains("[Facturación](knowledge/features/billing.md)"),
        "{llms}"
    );
    // La copia del vault no se inventaría como código.
    let inv = sb.run(&["scan", "--json"]);
    assert!(!inv.contains("docs/wiki"));
}

/// Build real: `adoc site setup` instala VitePress en el directorio de
/// configuración del sandbox y `check --site` compila (requiere Node y red:
/// `cargo test -- --ignored`).
#[test]
#[ignore]
fn check_site_builds_with_shared_vitepress() {
    let sb = Sandbox::new();
    copy_dir(&fixture_dir("with-openspec"), &sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    sb.run(&["site", "setup"]);
    sb.run(&["check", "--site", "--yes"]);
    assert!(sb.root.join("docs/.vitepress/dist/index.html").is_file());
}

/// VitePress falso en la instalación compartida: verifica la resolución,
/// el enlace `docs/node_modules` y el `.gitignore` sin red.
#[cfg(unix)]
#[test]
fn site_build_uses_shared_vitepress_install() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("node no disponible; se omite");
        return;
    }
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let pkg = sb.config.join("vitepress/node_modules/vitepress");
    fs::create_dir_all(pkg.join("bin")).unwrap();
    fs::write(
        pkg.join("package.json"),
        r#"{"name":"vitepress","version":"9.9.9","bin":{"vitepress":"bin/vitepress.js"}}"#,
    )
    .unwrap();
    fs::write(
        pkg.join("bin/vitepress.js"),
        "require('fs').writeFileSync('built.txt', process.argv.slice(2).join(' '))\n",
    )
    .unwrap();

    // Ejecuta JavaScript del repositorio: sin terminal ni --yes, error con instrucciones.
    let out = sb.cmd().args(["site", "build"]).assert().failure();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("--yes"), "{err}");
    assert!(!sb.root.join("built.txt").exists());
    sb.run(&["site", "build", "--yes"]);
    fs::remove_file(sb.root.join("built.txt")).unwrap();
    // La aprobación se recuerda para el proyecto.
    sb.run(&["site", "build"]);
    let built = fs::read_to_string(sb.root.join("built.txt")).unwrap();
    assert!(built.starts_with("build "), "{built}");
    let link = fs::read_link(sb.root.join("docs/node_modules")).unwrap();
    assert_eq!(link, sb.config.join("vitepress/node_modules"));
    assert!(fs::read_to_string(sb.root.join("docs/.gitignore"))
        .unwrap()
        .contains("node_modules/"));

    let doctor: serde_json::Value =
        serde_json::from_str(&sb.run(&["doctor", "--format", "json"])).unwrap();
    let vp = doctor["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "vitepress")
        .unwrap();
    assert_eq!(vp["level"], "ok");
    assert!(vp["detail"]
        .as_str()
        .unwrap()
        .contains("9.9.9 (compartida)"));
}

#[test]
fn doctor_reports_invalid_config_and_adoc_alias_works() {
    let sb = Sandbox::new();
    fs::write(
        sb.root.join("archeology-doc.toml"),
        "[project]\nid = \"\"\nname = \"x\"\n",
    )
    .unwrap();
    let mut cmd = adoc_cmd("adoc", &sb.config);
    cmd.current_dir(&sb.root)
        .args(["doctor", "--format", "json"]);
    let out = cmd.assert().failure();
    let json: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(json["ok"], false);
    assert!(json.to_string().contains("archeology-doc.toml inválido"));
}

#[test]
fn scan_writes_agent_context_adapters_and_skills() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    fs::write(
        sb.root.join("package.json"),
        r#"{"scripts":{"test":"node --test","lint":"eslint ."}}"#,
    )
    .unwrap();
    fs::write(sb.root.join("AGENTS.md"), "# Mío\n\nTexto del equipo.\n").unwrap();
    sb.run(&[
        "init",
        "--yes",
        "--agent",
        "manual",
        "--adapters",
        "claude,cursor,copilot",
    ]);
    sb.run(&["scan"]);

    let agents = fs::read_to_string(sb.root.join("AGENTS.md")).unwrap();
    assert!(
        agents.starts_with("# Mío\n\nTexto del equipo.\n"),
        "{agents}"
    );
    assert!(agents.contains("- test: `npm test`"), "{agents}");
    assert!(agents.contains("archeology-doc:begin context"));
    assert!(agents.contains("`.env*`"));
    assert!(fs::read_to_string(sb.root.join("CLAUDE.md"))
        .unwrap()
        .contains("@AGENTS.md"));
    assert!(
        fs::read_to_string(sb.root.join(".cursor/rules/archeology-doc.mdc"))
            .unwrap()
            .starts_with("---\ndescription:")
    );
    assert!(sb.root.join(".github/copilot-instructions.md").is_file());
    let skill = fs::read_to_string(sb.root.join(".agents/skills/run-tests/SKILL.md")).unwrap();
    assert!(
        skill.contains("name: run-tests") && skill.contains("npm test"),
        "{skill}"
    );
    assert!(sb
        .root
        .join(".agents/skills/project-memory/SKILL.md")
        .is_file());
    assert!(sb.root.join("knowledge/_templates/session.md").is_file());
    assert!(
        fs::read_to_string(sb.root.join("knowledge/memory/facts.md"))
            .unwrap()
            .contains("memory: stable")
    );

    // Idempotente: un segundo scan no cambia nada.
    sb.run(&["scan"]);
    assert_eq!(
        fs::read_to_string(sb.root.join("AGENTS.md")).unwrap(),
        agents
    );
    sb.run(&["check"]);
}

#[test]
fn mocs_and_duplicate_findings() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes"]);
    let finding = |id: &str| {
        format!(
            "---\nid: {id}\ntype: finding\ntitle: {id}\nkind: risk\nseverity: high\nstate: open\n\
             sources:\n  - src/main.py:2\n---\n"
        )
    };
    fs::write(sb.root.join("knowledge/findings/a.md"), finding("find-a")).unwrap();
    fs::write(sb.root.join("knowledge/findings/b.md"), finding("find-b")).unwrap();
    sb.run(&["scan"]);

    let moc = fs::read_to_string(sb.root.join("knowledge/findings/index.md")).unwrap();
    assert!(
        moc.contains("- [[findings/a|find-a]] — high · open"),
        "{moc}"
    );
    let out = sb.cmd().arg("check").assert().success();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("posible hallazgo duplicado"), "{err}");

    // Mismo id en dos notas: error.
    fs::write(
        sb.root.join("knowledge/findings/c.md"),
        finding("find-a").replace("risk", "config"),
    )
    .unwrap();
    sb.cmd().arg("check").assert().failure();
}

/// Flujo del MVP sobre el fixture mediano: inventario, plan acotado con
/// pistas de contradicciones y ejecución completa con un agente falso.
#[cfg(unix)]
#[test]
fn medium_fixture_end_to_end() {
    let sb = Sandbox::new();
    copy_dir(&fixture_dir("medium-ts"), &sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let cfg = fs::read_to_string(sb.root.join("archeology-doc.toml")).unwrap();
    fs::write(
        sb.root.join("archeology-doc.toml"),
        cfg.replace("max_files_per_task = 40", "max_files_per_task = 5"),
    )
    .unwrap();

    let inv: serde_json::Value = serde_json::from_str(&sb.run(&["scan", "--json"])).unwrap();
    let text = inv.to_string();
    for expected in [
        "src/server.ts",
        ".github/workflows/ci.yml",
        "src/__tests__/orders.test.ts",
        "Express",
        "Vitest",
        "src-services",
    ] {
        assert!(text.contains(expected), "falta {expected} en el inventario");
    }
    let markers: Vec<String> = inv["markers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| format!("{}:{}", m["path"].as_str().unwrap(), m["line"]))
        .collect();
    assert!(
        markers.contains(&"src/services/tax.ts:1".to_string()),
        "{markers:?}"
    );
    assert!(
        markers.contains(&"src/utils/dates.ts:1".to_string()),
        "{markers:?}"
    );
    // Ningún módulo excede el límite configurado.
    for m in inv["modules"].as_array().unwrap() {
        assert!(m["files"].as_u64().unwrap() <= 5, "{m}");
    }

    sb.run(&["plan"]);
    let tasks = sb.run(&["task", "list"]);
    let contradictions = fs::read_dir(sb.work().join("tasks"))
        .unwrap()
        .flatten()
        .map(|e| fs::read_to_string(e.path()).unwrap())
        .find(|t| t.contains("type: contradictions"))
        .unwrap_or_else(|| panic!("sin tarea de contradicciones: {tasks}"));
    assert!(
        contradictions.contains("- docs/architecture.md"),
        "{contradictions}"
    );
    assert!(
        contradictions.contains("src/services/tax.ts:1"),
        "{contradictions}"
    );
    assert!(fs::read_to_string(sb.root.join("AGENTS.md"))
        .unwrap()
        .contains("npm test"));

    add_adapter(&sb, "fake", FAKE_AGENT);
    let out = sb.run(&["inspect", "--agent", "fake"]);
    assert!(
        !out.contains("falló") && !out.contains("bloqueada"),
        "{out}"
    );
    sb.run(&["check"]);
    let review = sb.run(&["review"]);
    assert!(review.contains("Progreso:  100%"), "{review}");
}

/// Una tarea que falló en una corrida anterior no se reintenta sin
/// `--resume`; el informe debe decir la causa y cómo seguir.
#[cfg(unix)]
#[test]
fn inspect_explains_previous_failure_and_resume_retries_it() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    add_adapter(&sb, "lazy", "true");
    add_adapter(&sb, "fake", FAKE_AGENT);
    sb.run(&["scan"]);
    sb.run(&["plan"]);
    sb.run(&["inspect", "--agent", "lazy"]);

    let out = sb.run(&["inspect", "--agent", "fake"]);
    assert!(
        out.contains("→ T-001 falló en una corrida anterior"),
        "{out}"
    );
    assert!(out.contains("adoc inspect --resume"), "{out}");
    assert!(out.contains("T-001.log"), "{out}");

    let out = sb.run(&["inspect", "--agent", "fake", "--resume"]);
    assert!(out.contains("completadas: T-001"), "{out}");
    assert!(!out.contains("bloqueada"), "{out}");
}

/// Un adaptador de Claude sin modo de permisos no puede escribir: `inspect`
/// debe detenerse antes de lanzar el agente.
#[test]
fn inspect_rejects_claude_adapter_without_permissions() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let mut cfg = fs::read_to_string(sb.root.join("archeology-doc.toml")).unwrap();
    cfg.push_str(
        "\n[agent.adapters.viejo]\ncommand = \"claude\"\nargs = [\"-p\", \"{prompt_file_content}\"]\n",
    );
    fs::write(sb.root.join("archeology-doc.toml"), cfg).unwrap();
    sb.run(&["agent", "trust", "viejo", "--yes"]);
    sb.run(&["scan"]);
    sb.run(&["plan"]);
    let out = sb
        .cmd()
        .args(["inspect", "--agent", "viejo"])
        .assert()
        .failure();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("--permission-mode"), "{err}");
    // No se tocó ninguna tarea.
    assert!(!sb.run(&["task", "list"]).contains("failed"));
}

/// Un proyecto con el estado antiguo `.arqueology-doc/` (nombre anterior) se migra al
/// espacio de trabajo global en el primer comando.
#[test]
fn legacy_state_dir_is_migrated_to_workspace() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    sb.run(&["scan"]);
    sb.run(&["plan"]);
    // Simula una versión anterior: el estado vivía dentro del proyecto.
    let work = sb.work();
    fs::rename(&work, sb.root.join(".arqueology-doc")).unwrap();

    let list = sb.run(&["task", "list"]);
    assert!(list.contains("T-001"), "{list}");
    assert!(!sb.root.join(".arqueology-doc").exists());
    assert!(sb.work().join("tasks/T-001.md").is_file());
}

#[test]
fn agent_use_switches_agent_and_model() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let toml_path = sb.root.join("archeology-doc.toml");
    // Un proyecto antiguo sin el adaptador devin predefinido.
    let cfg = fs::read_to_string(&toml_path).unwrap();
    // Quita [agent.adapters.devin] y sus subtablas ([agent.adapters.devin.env]).
    let mut kept = vec!["# comentario del usuario"];
    let mut skipping = false;
    for line in cfg.lines() {
        if line.starts_with('[') {
            skipping = line.starts_with("[agent.adapters.devin");
        }
        if !skipping {
            kept.push(line);
        }
    }
    fs::write(&toml_path, kept.join("\n") + "\n").unwrap();

    let out = sb.run(&["agent", "use", "devin"]);
    assert!(out.contains("añadido el adaptador predefinido"), "{out}");
    let cfg = fs::read_to_string(&toml_path).unwrap();
    assert!(cfg.contains("\"smart\""), "{cfg}");
    let out = sb.run(&["agent", "use", "manual"]);
    assert!(out.contains("modo exportación"), "{out}");
    sb.run(&["agent", "use", "claude", "--model", "opus"]);
    let cfg = fs::read_to_string(&toml_path).unwrap();
    assert!(
        cfg.starts_with("# comentario del usuario"),
        "se perdió el formato:\n{cfg}"
    );
    assert!(cfg.contains("default = \"claude\""), "{cfg}");
    assert!(cfg.contains("model = \"opus\""), "{cfg}");
    let list = sb.run(&["agent", "list"]);
    assert!(list.contains("* claude"), "{list}");
    assert!(list.contains("opus"), "{list}");

    sb.run(&["agent", "use", "claude", "--no-model"]);
    assert!(!fs::read_to_string(&toml_path).unwrap().contains("model = "));
    sb.cmd()
        .args(["agent", "use", "inexistente"])
        .assert()
        .failure();
}

/// Tareas exportadas en modo manual: el informe lo explica y, al
/// volver a un agente headless, `--resume` las ejecuta.
#[cfg(unix)]
#[test]
fn manual_tasks_are_explained_and_resumed_by_headless_agent() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    add_adapter(&sb, "fake", FAKE_AGENT);
    sb.run(&["scan"]);
    sb.run(&["plan"]);
    sb.run(&["agent", "use", "manual"]);
    sb.run(&["inspect"]);

    let out = sb.run(&["inspect", "--resume"]);
    assert!(out.contains("tareas en modo manual"), "{out}");
    assert!(out.contains("adoc agent use claude"), "{out}");

    let out = sb.run(&["inspect", "--agent", "fake", "--resume"]);
    assert!(out.contains("completadas: T-001"), "{out}");
    assert!(!sb.run(&["task", "list"]).contains("manual"));
}

/// Cambiar a un agente headless reabre las tareas exportadas en modo manual:
/// basta `adoc inspect`, sin `--resume`.
#[cfg(unix)]
#[test]
fn agent_use_headless_reopens_manual_tasks() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    add_adapter(&sb, "fake", FAKE_AGENT);
    sb.run(&["scan"]);
    sb.run(&["plan"]);
    sb.run(&["agent", "use", "manual"]);
    sb.run(&["inspect"]);
    sb.run(&["task", "done", "T-001", "--force"]);

    let out = sb.run(&["agent", "use", "fake"]);
    assert!(out.contains("vuelven a `pending` para fake"), "{out}");
    let list = sb.run(&["task", "list"]);
    assert!(!list.contains("manual"), "{list}");
    assert!(
        list.contains("T-001    done"),
        "las done no se tocan: {list}"
    );

    let out = sb.run(&["inspect"]);
    assert!(out.contains("completadas: T-002"), "{out}");
}

/// `agent use <agente> --reset` reemplaza una definición antigua por la
/// predefinida (p. ej. devin en modo exportación → CLI de Devin).
#[test]
fn agent_use_reset_restores_builtin_adapter() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let toml_path = sb.root.join("archeology-doc.toml");
    let mut cfg = String::new();
    let mut skipping = false;
    for line in fs::read_to_string(&toml_path).unwrap().lines() {
        if line.starts_with('[') {
            skipping = line.starts_with("[agent.adapters.devin");
        }
        if !skipping {
            cfg.push_str(line);
            cfg.push('\n');
        }
    }
    cfg.push_str("\n[agent.adapters.devin]\nargs = []\nmode = \"export\"\n");
    fs::write(&toml_path, cfg).unwrap();

    let out = sb.run(&["agent", "use", "devin", "--reset"]);
    assert!(out.contains("restablecido"), "{out}");
    let cfg = fs::read_to_string(&toml_path).unwrap();
    let devin = &cfg[cfg.find("[agent.adapters.devin]").unwrap()..];
    assert!(devin.contains("command = \"devin\""), "{devin}");
    assert!(devin.contains("mode = \"headless\""), "{devin}");
    sb.cmd()
        .args(["agent", "use", "inventado", "--reset"])
        .assert()
        .failure();
}

/// Agente que termina con una pregunta sin producir nada: la tarea queda en
/// `question`, no se reintenta, y `task answer` la reabre con la respuesta.
#[cfg(unix)]
#[test]
fn agent_question_is_reported_and_answered() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    add_adapter(
        &sb,
        "dudoso",
        "echo 'Revisé el código.'; echo '**¿El tope correcto es 10 % o 20 %?**'",
    );
    add_adapter(&sb, "fake", FAKE_AGENT);
    sb.run(&["scan"]);
    sb.run(&["plan"]);

    let out = sb.run(&["inspect", "--agent", "dudoso"]);
    assert!(
        out.contains("? T-001 pregunta: ¿El tope correcto es 10 % o 20 %?"),
        "{out}"
    );
    assert!(out.contains("adoc task answer T-001"), "{out}");
    assert!(sb
        .run(&["task", "list", "--status", "question"])
        .contains("T-001"));
    // Sin respuesta no se reintenta; el informe explica qué falta.
    let out = sb.run(&["inspect", "--agent", "dudoso", "--resume"]);
    assert!(out.contains("→ T-001 espera tu respuesta"), "{out}");

    sb.run(&["task", "answer", "T-001", "Es 10 %, el código está mal"]);
    let task = fs::read_to_string(sb.work().join("tasks/T-001.md")).unwrap();
    assert!(task.contains("# Respuestas"), "{task}");
    assert!(
        task.contains("Pregunta: ¿El tope correcto es 10 % o 20 %?"),
        "{task}"
    );
    assert!(
        task.contains("Respuesta: Es 10 %, el código está mal"),
        "{task}"
    );
    assert!(task.contains("status: pending"), "{task}");

    let out = sb.run(&["task", "run", "T-001", "--agent", "fake"]);
    assert!(out.contains("completadas: T-001"), "{out}");
}

/// `task run --interactive` abre el agente con la tarea y valida al salir.
#[cfg(unix)]
#[test]
fn task_run_interactive_validates_output() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let mut cfg = fs::read_to_string(sb.root.join("archeology-doc.toml")).unwrap();
    let script = FAKE_AGENT
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    cfg.push_str(&format!(
        "\n[agent.adapters.vivo]\ncommand = \"sh\"\nargs = [\"-c\", \"true\", \"sh\", \"{{prompt_file}}\"]\n\
         interactive_args = [\"-c\", \"{script}\", \"sh\", \"{{prompt_file}}\"]\n"
    ));
    fs::write(sb.root.join("archeology-doc.toml"), cfg).unwrap();
    sb.run(&["agent", "trust", "vivo", "--yes"]);
    sb.run(&["scan"]);
    sb.run(&["plan"]);

    let out = sb.run(&["task", "run", "T-001", "--interactive", "--agent", "vivo"]);
    assert!(out.contains("✓ T-001 completada"), "{out}");
    assert!(sb.run(&["task", "list"]).contains("T-001    done"));

    // Agente sin modo interactivo: instrucciones para hacerlo a mano.
    let out = sb.run(&["task", "run", "T-002", "--interactive", "--agent", "manual"]);
    assert!(out.contains("no tiene modo interactivo"), "{out}");
}

#[test]
fn tasks_tell_the_agent_not_to_ask() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    sb.run(&["scan"]);
    sb.run(&["plan"]);
    let task = fs::read_to_string(sb.work().join("tasks/T-001.md")).unwrap();
    assert!(task.contains("No hagas preguntas"), "{task}");
    assert!(task.contains("open-questions.md"), "{task}");
}

/// Un plan nuevo distinto no se mezcla con el anterior; `--force` lo
/// reemplaza, borra las tareas sobrantes y conserva el estado de las iguales.
#[test]
fn plan_force_replaces_previous_plan() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    sb.run(&["scan"]);
    let cfg_path = sb.root.join("archeology-doc.toml");
    let cfg = fs::read_to_string(&cfg_path).unwrap();
    fs::write(
        &cfg_path,
        cfg.replace("size = \"auto\"", "size = \"large\""),
    )
    .unwrap();
    let out = sb.run(&["plan"]);
    assert!(out.contains("perfil: grande"), "{out}");
    let large = fs::read_dir(sb.work().join("tasks")).unwrap().count();
    sb.run(&["task", "done", "T-001", "--force"]);

    let cfg = fs::read_to_string(&cfg_path).unwrap();
    fs::write(
        &cfg_path,
        cfg.replace("size = \"large\"", "size = \"small\""),
    )
    .unwrap();
    let out = sb.run(&["plan"]);
    assert!(out.contains("no se mezclan"), "{out}");

    let out = sb.run(&["plan", "--force"]);
    let small = fs::read_dir(sb.work().join("tasks")).unwrap().count();
    assert!(small < large, "{small} >= {large}");
    assert!(out.contains("eliminadas"), "{out}");
    // T-001 (overview) cambió de salidas en el perfil pequeño (incluye glosario).
    assert!(sb.run(&["task", "list"]).contains("T-001"));
}

/// `adoc status` (JSON para la extensión de VS Code) y `task list --format json`.
#[test]
fn status_json_describes_project_for_integrations() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    sb.run(&["scan"]);
    sb.run(&["plan"]);
    fs::write(
        sb.root.join("knowledge/findings/x.md"),
        "---\nid: find-x\ntype: finding\ntitle: X\nkind: risk\nseverity: high\nstate: open\n\
         sources:\n  - src/main.py:1\n---\n",
    )
    .unwrap();

    let st: serde_json::Value = serde_json::from_str(&sb.run(&["status"])).unwrap();
    assert_eq!(st["agent"]["active"], "manual");
    assert_eq!(st["profile"], "small");
    assert!(st["tasks"][0]["file"].as_str().unwrap().starts_with('/'));
    assert_eq!(st["findings"][0]["sources"][0], "src/main.py:1");
    assert!(st["agent"]["available"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a == "devin"));

    let tasks: serde_json::Value =
        serde_json::from_str(&sb.run(&["task", "list", "--format", "json", "--status", "pending"]))
            .unwrap();
    assert!(tasks
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["status"] == "pending"));
}

/// Reporte HTML y dashboard del sitio: estado, hallazgos, preguntas
/// pendientes (agente y equipo) y FAQ; el texto se escapa.
#[cfg(unix)]
#[test]
fn report_and_site_dashboard_show_findings_and_questions() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    add_adapter(&sb, "dudoso", "echo '¿Cuál es el tope?'");
    sb.run(&["scan"]);
    sb.run(&["plan"]);
    sb.run(&["inspect", "--agent", "dudoso"]);
    fs::write(
        sb.root.join("knowledge/findings/x.md"),
        "---\nid: find-x\ntype: finding\ntitle: \"Tope <script>alert(1)</script>\"\nkind: risk\n\
         severity: high\nstate: open\nsources:\n  - src/main.py:1\n---\n",
    )
    .unwrap();
    let mut open_q =
        fs::read_to_string(sb.root.join("knowledge/memory/open-questions.md")).unwrap();
    open_q.push_str("- 2026-10-09 · ¿Quién aprueba los reembolsos? · fuente: src/main.py:1\n");
    fs::write(sb.root.join("knowledge/memory/open-questions.md"), open_q).unwrap();
    fs::create_dir_all(sb.root.join("knowledge/faq")).unwrap();
    fs::write(
        sb.root.join("knowledge/faq/como-arranca.md"),
        "---\nid: faq-como-arranca\ntype: faq\ntitle: \"¿Cómo arranca?\"\nsources: [src/main.py:1]\n---\n\n# ¿Cómo arranca?\n\nCon main().\n",
    )
    .unwrap();

    let out = sb.root.join("r.html");
    sb.run(&["report", "--out", out.to_str().unwrap(), "--no-open"]);
    let html = fs::read_to_string(&out).unwrap();
    assert!(html.contains("&lt;script&gt;"), "sin escapar");
    assert!(!html.contains("<script>alert"), "XSS");
    assert!(html.contains("¿Cuál es el tope?"), "pregunta del agente");
    assert!(
        html.contains("¿Quién aprueba los reembolsos?"),
        "pregunta del equipo"
    );
    assert!(html.contains("Con main()."), "FAQ");

    fs::write(
        sb.root.join("docs/index.md"),
        "---\nlayout: home\n---\n\nTexto del equipo.\n",
    )
    .unwrap();
    sb.run(&["site", "sync"]);
    let index = fs::read_to_string(sb.root.join("docs/index.md")).unwrap();
    assert!(
        index.contains("Texto del equipo.") && index.contains("begin dashboard"),
        "{index}"
    );
    let preguntas = fs::read_to_string(sb.root.join("docs/preguntas.md")).unwrap();
    assert!(preguntas.contains("adoc task answer T-001"), "{preguntas}");
    let sidebar =
        fs::read_to_string(sb.root.join("docs/.vitepress/sidebar.generated.json")).unwrap();
    assert!(sidebar.contains("/hallazgos"), "{sidebar}");
}

/// `ask` con un agente que responde por pantalla: la respuesta queda en la FAQ.
#[cfg(unix)]
#[test]
fn ask_saves_answer_in_faq() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    add_adapter(&sb, "sabio", "echo 'Arranca en src/main.py:1 con main().'");
    let out = sb.run(&["ask", "¿Cómo arranca la app?", "--agent", "sabio"]);
    assert!(out.contains("Arranca en src/main.py:1"), "{out}");
    let faq = fs::read_dir(sb.root.join("knowledge/faq")).unwrap().count();
    assert!(faq >= 1);
    let st: serde_json::Value = serde_json::from_str(&sb.run(&["status"])).unwrap();
    assert_eq!(
        st["questions"]["faq"][0]["question"],
        "¿Cómo arranca la app?"
    );
}

/// La pregunta puede ir sin comillas y con flags en cualquier posición.
#[cfg(unix)]
#[test]
fn ask_accepts_unquoted_words() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    add_adapter(&sb, "sabio", "echo 'Sí, por día completo.'");
    let out = sb.run(&[
        "ask",
        "las",
        "reservas",
        "son",
        "por",
        "dia",
        "completo?",
        "--agent",
        "sabio",
    ]);
    assert!(out.contains("Sí, por día completo."), "{out}");
    let st: serde_json::Value = serde_json::from_str(&sb.run(&["status"])).unwrap();
    assert_eq!(
        st["questions"]["faq"][0]["question"],
        "las reservas son por dia completo?"
    );
    // Sin pregunta y sin terminal: error claro, no una pregunta vacía al agente.
    sb.cmd()
        .args(["ask", "--agent", "sabio"])
        .assert()
        .failure();
}

/// `adoc graph`: notas, wikilinks y evidencias para la vista de grafo.
#[test]
fn graph_json_has_notes_links_and_code() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    fs::write(
        sb.root.join("knowledge/modules/src.md"),
        "---\nid: mod-src\ntype: module\ntitle: src\nsources: [src/main.py:1]\n---\nVer [[00-overview]].\n",
    )
    .unwrap();
    sb.run(&["scan"]);
    let g: serde_json::Value = serde_json::from_str(&sb.run(&["graph"])).unwrap();
    let edges = g["edges"].as_array().unwrap();
    assert!(edges
        .iter()
        .any(|e| e["source"] == "modules/src" && e["target"] == "00-overview"));
    assert!(edges
        .iter()
        .any(|e| e["target"] == "code:src/main.py" && e["kind"] == "evidence"));
    assert!(g["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["kind"] == "code" && n["line"] == 1));
}

/// Vault con un título hostil, un enlace y una nota huérfana (tests del grafo).
fn graph_fixture(sb: &Sandbox) {
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    fs::write(
        sb.root.join("knowledge/modules/src.md"),
        "---\nid: mod-src\ntype: module\ntitle: \"Src </script><script>alert(1)</script>\"\n\
         sources: [src/main.py:1]\n---\nVer [[00-overview]].\n",
    )
    .unwrap();
    fs::write(
        sb.root.join("knowledge/modules/lonely.md"),
        "---\nid: mod-lonely\ntype: module\ntitle: Solitaria\n---\nSin enlaces.\n",
    )
    .unwrap();
}

/// `adoc graph --format html --out`: HTML autocontenido con los datos
/// escapados, CSP estricta y sin recursos externos.
#[test]
fn graph_html_is_self_contained_and_escaped() {
    let sb = Sandbox::new();
    graph_fixture(&sb);
    let out = sb.root.join("g.html");
    let stdout = sb.run(&["graph", "--format", "html", "--out", out.to_str().unwrap()]);
    assert!(stdout.contains("g.html"), "{stdout}");
    let html = fs::read_to_string(&out).unwrap();
    assert!(html.contains("Content-Security-Policy"), "sin CSP");
    assert!(html.contains("script-src 'nonce-"), "CSP sin nonce");
    assert!(html.contains("AdocGraph"), "sin renderizador");
    assert!(html.contains("modules/src"), "sin datos");
    assert!(html.contains("code:src/main.py"), "sin nodos de código");
    // El título hostil no cierra el bloque de datos.
    assert!(!html.contains("<script>alert"), "XSS");
    assert!(html.contains("\\u003c/script\\u003e"), "título sin escapar");
    // Sin recursos externos.
    assert!(
        !html.contains("http://") && !html.contains("https://"),
        "URL externa"
    );
    assert!(!html.contains(" src="), "script externo");
    // Textos en el idioma de la interfaz (es en estos tests).
    assert!(html.contains("Grafo del conocimiento"), "idioma");
    // Por defecto, en el espacio de trabajo; `--open` con otro formato es un error.
    sb.run(&["graph", "--format", "html"]);
    assert!(sb.work().join("graph.html").is_file());
    sb.cmd()
        .args(["graph", "--format", "json", "--open"])
        .assert()
        .failure();
}

/// `review` muestra la sección del grafo (texto en en/es, markdown) y la
/// añade al JSON como clave nueva.
#[test]
fn review_shows_graph_section() {
    let sb = Sandbox::new();
    graph_fixture(&sb);
    let es = sb.run(&["review"]);
    assert!(es.contains("== Grafo =="), "{es}");
    assert!(es.contains("Notas huérfanas ("), "{es}");
    assert!(es.contains("modules/lonely"), "{es}");
    assert!(es.contains("Más conectadas: Src"), "{es}");
    assert!(es.contains("adoc graph --open"), "{es}");
    let out = sb
        .cmd()
        .env("ADOC_LANG", "en")
        .args(["review"])
        .assert()
        .success();
    let en = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(en.contains("== Graph =="), "{en}");
    assert!(
        en.contains("Orphan notes (") && en.contains("modules/lonely"),
        "{en}"
    );
    assert!(en.contains("Most connected:"), "{en}");
    let md = sb.run(&["review", "--format", "markdown"]);
    assert!(md.contains("## Grafo"), "{md}");
    let json: serde_json::Value =
        serde_json::from_str(&sb.run(&["review", "--format", "json"])).unwrap();
    assert!(json["graph"]["orphans"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o["id"] == "modules/lonely"));
    assert_eq!(json["graph"]["top_connected"][0]["id"], "modules/src");
    assert!(json["graph"]["edges_by_kind"]["evidence"].as_u64().unwrap() >= 1);
    // Las claves existentes no cambian.
    assert!(json["progress"].is_number() && json["tasks"].is_object());
}

/// `site sync` publica la página del grafo, sus recursos y el enlace en el
/// sidebar; `report` incrusta el grafo (HTML) o su resumen (markdown).
#[test]
fn site_and_report_include_graph() {
    let sb = Sandbox::new();
    graph_fixture(&sb);
    sb.run(&["site", "sync"]);
    let docs = sb.root.join("docs");
    let page = fs::read_to_string(docs.join("grafo.md")).unwrap();
    assert!(page.contains("id=\"adoc-graph\""), "{page}");
    assert!(page.contains("adoc-graph/"), "{page}");
    assert!(page.contains("Notas huérfanas"), "{page}");
    assert!(!page.contains("<script>alert"), "{page}");
    let assets = docs.join("public/adoc-graph");
    assert!(assets.join(".generated-by-archeology-doc").is_file());
    let js = fs::read_to_string(assets.join("graph.js")).unwrap();
    assert!(js.contains("class Layout"));
    assert!(assets.join("graph.css").is_file());
    let data: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(assets.join("graph.json")).unwrap()).unwrap();
    let src = data["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "modules/src")
        .unwrap();
    assert_eq!(src["link"], "/wiki/modules/src");
    assert!(data["strings"]["search"].is_string());
    // El sitio puede publicarse: sin rutas absolutas locales.
    let raw = fs::read_to_string(assets.join("graph.json")).unwrap();
    assert!(!raw.contains(sb.root.to_str().unwrap()), "ruta local");
    let sidebar = fs::read_to_string(docs.join(".vitepress/sidebar.generated.json")).unwrap();
    assert!(sidebar.contains("/grafo"), "{sidebar}");
    let index = fs::read_to_string(docs.join("index.md")).unwrap();
    assert!(index.contains("(/grafo)"), "{index}");
    // Idempotente; una carpeta de recursos del usuario no se pisa.
    sb.run(&["site", "sync"]);
    fs::remove_file(assets.join(".generated-by-archeology-doc")).unwrap();
    sb.cmd().args(["site", "sync"]).assert().failure();
    fs::remove_dir_all(&assets).unwrap();
    // Los recursos del grafo no se inventarían como código.
    sb.run(&["site", "sync"]);
    let inv = sb.run(&["scan", "--json"]);
    assert!(!inv.contains("adoc-graph"), "{inv}");

    let out = sb.root.join("r.html");
    sb.run(&["report", "--out", out.to_str().unwrap(), "--no-open"]);
    let html = fs::read_to_string(&out).unwrap();
    assert!(html.contains("id=\"grafo\""), "sin sección");
    assert!(html.contains("AdocGraph"), "sin renderizador");
    assert!(html.contains("Content-Security-Policy"), "sin CSP");
    assert!(!html.contains("<script>alert"), "XSS");
    assert!(html.contains("Solitaria"), "huérfana");
    let md = sb.root.join("r.md");
    sb.run(&[
        "report",
        "--format",
        "markdown",
        "--out",
        md.to_str().unwrap(),
        "--no-open",
    ]);
    let md = fs::read_to_string(&md).unwrap();
    assert!(md.contains("## Grafo del conocimiento"), "{md}");
    assert!(md.contains("Notas más conectadas"), "{md}");
}

/// Un `archeology-doc.toml` hostil no ejecuta nada sin aprobación: ni con
/// `doctor`/`agent list` (que antes lanzaban `<cmd> --version`) ni con `inspect`.
#[cfg(unix)]
#[test]
fn repo_defined_adapter_never_runs_without_trust() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let mut cfg = fs::read_to_string(sb.root.join("archeology-doc.toml")).unwrap();
    cfg = cfg.replace("default = \"manual\"", "default = \"evil\"");
    cfg.push_str(
        "\n[agent.adapters.evil]\ncommand = \"sh\"\nargs = [\"-c\", \"touch PWNED\", \"sh\", \"{prompt_file}\"]\n",
    );
    fs::write(sb.root.join("archeology-doc.toml"), cfg).unwrap();

    let _ = sb.cmd().arg("doctor").output().unwrap();
    let out = sb.run(&["agent", "list"]);
    assert!(out.contains("sin aprobar"), "{out}");
    sb.run(&["scan"]);
    sb.run(&["plan"]);
    let out = sb.cmd().arg("inspect").assert().failure();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("adoc agent trust evil"), "{err}");
    assert!(!sb.root.join("PWNED").exists(), "se ejecutó el adaptador");

    // Aprobado, se ejecuta; si luego cambia en el repositorio, deja de estarlo.
    sb.run(&["agent", "trust", "evil", "--yes"]);
    assert!(!sb.run(&["agent", "list"]).contains("sin aprobar"));
    let cfg = fs::read_to_string(sb.root.join("archeology-doc.toml"))
        .unwrap()
        .replace("touch PWNED", "touch PWNED2");
    fs::write(sb.root.join("archeology-doc.toml"), cfg).unwrap();
    assert!(sb.run(&["agent", "list"]).contains("sin aprobar"));
}

/// Un `project.id` con `..` no puede sacar el espacio de trabajo de
/// `projects/` ni migrar archivos del repositorio fuera de él.
#[test]
fn hostile_project_id_is_rejected() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let cfg = fs::read_to_string(sb.root.join("archeology-doc.toml")).unwrap();
    let id_line = cfg.lines().find(|l| l.starts_with("id = ")).unwrap();
    let cfg = cfg.replace(id_line, "id = \"../..\"");
    fs::write(sb.root.join("archeology-doc.toml"), cfg).unwrap();
    fs::create_dir_all(sb.root.join(".archeology-doc")).unwrap();
    fs::write(sb.root.join(".archeology-doc/.zshenv"), "malicioso").unwrap();

    let out = sb.cmd().arg("status").assert().failure();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("project.id inválido"), "{err}");
    assert!(sb.root.join(".archeology-doc/.zshenv").is_file());
    assert!(!sb.config.join(".zshenv").exists());
}

/// Rutas de salida fuera del proyecto se rechazan antes de escribir nada.
#[test]
fn output_paths_outside_project_are_rejected() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let cfg = fs::read_to_string(sb.root.join("archeology-doc.toml"))
        .unwrap()
        .replace("docs = \"docs\"", "docs = \"../fuera\"");
    fs::write(sb.root.join("archeology-doc.toml"), cfg).unwrap();
    let out = sb.cmd().args(["site", "sync"]).assert().failure();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("paths.docs"), "{err}");
    assert!(!sb.root.parent().unwrap().join("fuera").exists());
}

/// `docs/wiki` escrito a mano (docs/ existía antes de `init`) no se borra.
#[test]
fn site_sync_keeps_user_owned_wiki() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    fs::create_dir_all(sb.root.join("docs/wiki")).unwrap();
    fs::write(sb.root.join("docs/wiki/manual.md"), "# Mío\n").unwrap();
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let out = sb.cmd().args(["site", "sync"]).assert().failure();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("no lo generó archeology-doc"), "{err}");
    assert!(sb.root.join("docs/wiki/manual.md").is_file());
}

/// Un proyecto inicializado con el nombre anterior (arqueology-doc) se migra:
/// archivo de configuración y marcadores de AGENTS.md, sin duplicar regiones.
#[test]
fn project_with_previous_name_is_migrated() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    fs::rename(
        sb.root.join("archeology-doc.toml"),
        sb.root.join("arqueology-doc.toml"),
    )
    .unwrap();
    let agents = fs::read_to_string(sb.root.join("AGENTS.md"))
        .unwrap()
        .replace("archeology-doc:", "arqueology-doc:");
    fs::write(sb.root.join("AGENTS.md"), agents).unwrap();

    sb.run(&["scan"]);
    assert!(sb.root.join("archeology-doc.toml").is_file());
    assert!(!sb.root.join("arqueology-doc.toml").exists());
    let agents = fs::read_to_string(sb.root.join("AGENTS.md")).unwrap();
    assert!(!agents.contains("arqueology-doc:"), "{agents}");
    assert_eq!(agents.matches("archeology-doc:begin context").count(), 1);
}

/// Quita `[agent.adapters.<name>]` (y sus subtablas) del TOML del proyecto.
fn strip_adapter(sb: &Sandbox, name: &str) -> String {
    let header = format!("[agent.adapters.{name}");
    let mut cfg = String::new();
    let mut skipping = false;
    for line in fs::read_to_string(sb.root.join("archeology-doc.toml"))
        .unwrap()
        .lines()
    {
        if line.starts_with('[') {
            skipping = line == format!("{header}]") || line.starts_with(&format!("{header}."));
        }
        if !skipping {
            cfg.push_str(line);
            cfg.push('\n');
        }
    }
    cfg
}

/// `[agent.adapters.claude]` con la definición peligrosa de `amp` no pasa por
/// predefinido: requiere aprobación como cualquier adaptador del repositorio.
#[test]
fn spoofed_builtin_name_requires_trust() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let mut cfg = strip_adapter(&sb, "claude");
    cfg.push_str(
        "\n[agent.adapters.claude]\ncommand = \"amp\"\n\
         args = [\"--dangerously-allow-all\", \"-x\", \"{prompt_file_content}\"]\n",
    );
    fs::write(sb.root.join("archeology-doc.toml"), cfg).unwrap();
    let list = sb.run(&["agent", "list"]);
    let row = list.lines().find(|l| l.contains(" claude ")).unwrap();
    assert!(row.contains("sin aprobar"), "{list}");
    sb.run(&["scan"]);
    sb.run(&["plan"]);
    let out = sb
        .cmd()
        .args(["inspect", "--agent", "claude"])
        .assert()
        .failure();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("adoc agent trust claude"), "{err}");
    // `agent trust` muestra la definición y pide confirmación (no "predefinido").
    let out = sb
        .cmd()
        .args(["agent", "trust", "claude"])
        .assert()
        .failure();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(!stdout.contains("no necesita aprobación"), "{stdout}");
    assert!(stdout.contains("--dangerously-allow-all"), "{stdout}");
}

/// `agent trust` escapa los caracteres de control (no pueden ocultar lo que
/// se aprueba) y muestra el modelo; cambiar un modelo usado en `{model}`
/// exige volver a aprobar.
#[test]
fn agent_trust_escapes_control_chars_and_shows_model() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let mut cfg = fs::read_to_string(sb.root.join("archeology-doc.toml")).unwrap();
    cfg.push_str(
        "\n[agent.adapters.raro]\ncommand = \"sh\\u001b[2K\"\n\
         args = [\"-c\", \"true\", \"--modelo={model}\", \"{prompt_file}\"]\nmodel = \"opus\"\n\
         [agent.adapters.raro.env]\nCLAVE = \"v\\r\\u001b[1Aoculto\"\n",
    );
    fs::write(sb.root.join("archeology-doc.toml"), &cfg).unwrap();
    let out = sb.run(&["agent", "trust", "raro", "--yes"]);
    assert!(!out.contains('\u{1b}'), "{out:?}");
    assert!(!out.contains('\r'), "{out:?}");
    assert!(out.contains("\\u{1b}"), "{out}");
    assert!(out.contains("model   = \"opus\""), "{out}");
    assert!(!sb.run(&["agent", "list"]).contains("sin aprobar"));
    // El modelo entra en `{model}`: cambiarlo en el repositorio exige aprobar de nuevo.
    fs::write(
        sb.root.join("archeology-doc.toml"),
        cfg.replace("model = \"opus\"", "model = \"otro\""),
    )
    .unwrap();
    assert!(sb.run(&["agent", "list"]).contains("sin aprobar"));
}

/// `task run` con un agente sin aprobar no toca la tarea: una `done` sigue `done`.
/// `task answer` solo responde tareas en `question`.
#[test]
fn task_run_with_untrusted_agent_keeps_done_task() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let mut cfg = fs::read_to_string(sb.root.join("archeology-doc.toml")).unwrap();
    cfg.push_str(
        "\n[agent.adapters.ajeno]\ncommand = \"sh\"\nargs = [\"-c\", \"touch PWNED\", \"sh\", \"{prompt_file}\"]\n",
    );
    fs::write(sb.root.join("archeology-doc.toml"), cfg).unwrap();
    sb.run(&["scan"]);
    sb.run(&["plan"]);
    sb.run(&["task", "done", "T-001", "--force"]);

    let out = sb
        .cmd()
        .args(["task", "run", "T-001", "--agent", "ajeno"])
        .assert()
        .failure();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("adoc agent trust ajeno"), "{err}");
    assert!(sb.run(&["task", "list"]).contains("T-001    done"));
    assert!(!sb.root.join("PWNED").exists());
    // Agente inexistente: tampoco se toca.
    sb.cmd()
        .args(["task", "run", "T-001", "--agent", "inventado"])
        .assert()
        .failure();
    assert!(sb.run(&["task", "list"]).contains("T-001    done"));

    // `task answer` sobre una tarea que no pregunta nada: error, sigue done.
    let out = sb
        .cmd()
        .args(["task", "answer", "T-001", "hola"])
        .assert()
        .failure();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("question"), "{err}");
    assert!(sb.run(&["task", "list"]).contains("T-001    done"));
}

/// `agent use` con un agente headless sin aprobar no reabre las tareas manuales.
#[test]
fn agent_use_untrusted_headless_keeps_manual_tasks() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let mut cfg = fs::read_to_string(sb.root.join("archeology-doc.toml")).unwrap();
    cfg.push_str(
        "\n[agent.adapters.nuevo]\ncommand = \"sh\"\nargs = [\"-c\", \"true\", \"sh\", \"{prompt_file}\"]\n",
    );
    fs::write(sb.root.join("archeology-doc.toml"), cfg).unwrap();
    sb.run(&["scan"]);
    sb.run(&["plan"]);
    sb.run(&["inspect"]);
    let out = sb.run(&["agent", "use", "nuevo"]);
    assert!(out.contains("no se reabren"), "{out}");
    assert!(sb.run(&["task", "list"]).contains("manual"));
}

/// `agent list`/`agent test` no esconden una config inválida tras los predefinidos.
#[test]
fn agent_list_reports_invalid_config() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let mut cfg = fs::read_to_string(sb.root.join("archeology-doc.toml")).unwrap();
    cfg.push_str("\n[agent.adapters.mal]\ncommand = \"x\"\nmodel = \"--yolo\"\n");
    fs::write(sb.root.join("archeology-doc.toml"), cfg).unwrap();
    let out = sb.cmd().args(["agent", "list"]).assert().failure();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("model"), "{err}");
    sb.cmd().args(["agent", "test"]).assert().failure();
    // Sin proyecto, se listan los predefinidos.
    let tmp = tempfile::tempdir().unwrap();
    adoc_cmd("archeology-doc", &sb.config)
        .current_dir(tmp.path())
        .args(["agent", "list"])
        .assert()
        .success();
}

/// `ask` con un agente que falla no deja una nota a medias en la FAQ.
#[cfg(unix)]
#[test]
fn ask_discards_note_when_agent_fails() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    add_adapter(
        &sb,
        "roto",
        "f=$(grep -m1 '^- knowledge/faq/' \"$1\" | cut -c3-); mkdir -p \"$(dirname \"$f\")\"; \
         printf -- '---\\nid: x\\n---\\nmitad' > \"$f\"; exit 1",
    );
    let out = sb
        .cmd()
        .args(["ask", "¿Cómo arranca?", "--agent", "roto"])
        .assert()
        .failure();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("exit code 1"), "{err}");
    let faq: Vec<_> = fs::read_dir(sb.root.join("knowledge/faq"))
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(
        faq.iter()
            .all(|f| !f.to_string_lossy().ends_with(".md")
                || f.to_string_lossy().starts_with("index")),
        "{faq:?}"
    );
    let partial = fs::read_dir(sb.work().join("asks"))
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().ends_with(".partial.md"));
    assert!(partial, "{err}");
}

/// Ctrl+C durante `inspect`: se mata al agente (aunque esté en otro grupo de
/// procesos), la tarea vuelve a `pending`, queda el log y `adoc` sale con 130.
#[cfg(unix)]
#[test]
fn ctrl_c_kills_agent_and_leaves_task_resumable() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    add_adapter(&sb, "lento", "echo $$ > agent.pid; exec sleep 60");
    sb.run(&["scan"]);
    sb.run(&["plan"]);

    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("archeology-doc"))
        .env("ARCHEOLOGY_DOC_CONFIG_DIR", &sb.config)
        .env("ADOC_LANG", "es")
        .current_dir(&sb.root)
        .args(["inspect", "--agent", "lento"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid_file = sb.root.join("agent.pid");
    let start = std::time::Instant::now();
    let agent_pid = loop {
        if let Some(pid) = fs::read_to_string(&pid_file)
            .ok()
            .filter(|t| t.ends_with('\n'))
        {
            break pid.trim().to_string();
        }
        assert!(start.elapsed() < std::time::Duration::from_secs(30));
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let status = std::process::Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let code = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(30),
            "adoc no terminó"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert_eq!(code, Some(130));
    let alive = std::process::Command::new("kill")
        .args(["-0", &agent_pid])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(!alive, "el agente quedó huérfano");
    let list = sb.run(&["task", "list"]);
    assert!(list.contains("T-001    pending"), "{list}");
    assert!(!list.contains("running"), "{list}");
    let run = fs::read_dir(sb.work().join("runs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let log = fs::read_to_string(run.path().join("run.log")).unwrap();
    assert!(log.contains("interrumpida"), "{log}");
}

/// Un almacén de aprobaciones dañado no se reemplaza en silencio: aprobar
/// falla y el archivo queda intacto (no se pierden las demás aprobaciones).
#[test]
fn corrupt_trust_store_is_not_wiped() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let mut cfg = fs::read_to_string(sb.root.join("archeology-doc.toml")).unwrap();
    cfg.push_str(
        "\n[agent.adapters.propio]\ncommand = \"sh\"\nargs = [\"-c\", \"true\", \"sh\", \"{prompt_file}\"]\n",
    );
    fs::write(sb.root.join("archeology-doc.toml"), cfg).unwrap();
    let store = sb.config.join("trusted-adapters.toml");
    fs::write(&store, "[[adapter]\nroto").unwrap();
    let out = sb
        .cmd()
        .args(["agent", "trust", "propio", "--yes"])
        .assert()
        .failure();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("dañado"), "{err}");
    assert_eq!(fs::read_to_string(&store).unwrap(), "[[adapter]\nroto");
    // Falla cerrado: nada cuenta como aprobado.
    assert!(sb.run(&["agent", "list"]).contains("sin aprobar"));
    // `agent untrust` en un almacén inexistente no falla.
    fs::remove_file(&store).unwrap();
    sb.run(&["agent", "untrust", "propio"]);
}

// --- Idioma (i18n) -----------------------------------------------------------

fn json_out(cmd: &mut Command) -> serde_json::Value {
    let out = cmd.assert().success();
    serde_json::from_slice(&out.get_output().stdout).unwrap()
}

#[test]
fn ui_language_defaults_to_english() {
    let sb = Sandbox::new();
    let v = json_out(
        sb.cmd_without_lang()
            .args(["config", "get", "language", "--json"]),
    );
    assert_eq!(v["value"], "en", "{v}");
    assert_eq!(v["source"], "default", "{v}");
    // Mensajes de error en inglés.
    let out = sb.cmd_without_lang().args(["scan"]).assert().failure();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("no archeology-doc project found"), "{err}");
    // El locale del sistema cuenta si no hay nada más.
    let v = json_out(
        sb.cmd_without_lang()
            .env("LANG", "es_CR.UTF-8")
            .args(["config", "get", "language", "--json"]),
    );
    assert_eq!(
        (v["value"].as_str(), v["source"].as_str()),
        (Some("es"), Some("locale"))
    );
}

#[test]
fn lang_flag_beats_env_and_invalid_values_are_skipped() {
    let sb = Sandbox::new();
    let v = json_out(
        sb.cmd()
            .env("ADOC_LANG", "en")
            .args(["--lang", "es", "config", "get", "language", "--json"]),
    );
    assert_eq!(
        (v["value"].as_str(), v["source"].as_str()),
        (Some("es"), Some("flag"))
    );
    let v = json_out(
        sb.cmd()
            .env("ADOC_LANG", "en")
            .args(["config", "get", "language", "--json"]),
    );
    assert_eq!(
        (v["value"].as_str(), v["source"].as_str()),
        (Some("en"), Some("env"))
    );
    // Valor inválido: aviso y siguiente fuente.
    let out = sb
        .cmd()
        .args(["config", "get", "language", "--lang=fr"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(
        stdout.starts_with("es (origen: la variable de entorno ADOC_LANG)"),
        "{stdout}"
    );
    assert!(stderr.contains("'fr'"), "{stderr}");
}

#[test]
fn config_set_get_unset_language() {
    let sb = Sandbox::new();
    fs::create_dir_all(&sb.config).unwrap();
    let cfg = sb.config.join("config.toml");
    fs::write(&cfg, "default_agent = \"claude\"\n").unwrap();

    let out = sb.run(&["config", "set", "language", "en"]);
    assert!(out.contains("language = en"), "{out}");
    let text = fs::read_to_string(&cfg).unwrap();
    assert!(text.contains("language = \"en\""), "{text}");
    assert!(text.contains("default_agent = \"claude\""), "{text}");
    let v = json_out(
        sb.cmd_without_lang()
            .env("LANG", "es_ES.UTF-8")
            .args(["config", "get", "language", "--json"]),
    );
    assert_eq!(
        (v["value"].as_str(), v["source"].as_str()),
        (Some("en"), Some("config"))
    );
    // Texto legible en el idioma resuelto (inglés, por la configuración).
    let out = sb
        .cmd_without_lang()
        .args(["config", "get", "language"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    assert!(
        stdout.starts_with("en (source: the user configuration"),
        "{stdout}"
    );

    sb.cmd()
        .args(["config", "set", "language", "fr"])
        .assert()
        .failure();
    sb.run(&["config", "unset", "language"]);
    let text = fs::read_to_string(&cfg).unwrap();
    assert!(!text.contains("language"), "{text}");
    assert!(text.contains("default_agent"), "{text}");
    let v = json_out(
        sb.cmd_without_lang()
            .args(["config", "get", "language", "--json"]),
    );
    assert_eq!(v["source"], "default", "{v}");
}

#[test]
fn init_writes_project_language() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    // Sin --lang: el idioma de la interfaz resuelto (ADOC_LANG=es).
    sb.run(&["init", "--yes"]);
    let toml = fs::read_to_string(sb.root.join("archeology-doc.toml")).unwrap();
    assert!(toml.contains("language = \"es\""), "{toml}");
    let agents = fs::read_to_string(sb.root.join("AGENTS.md")).unwrap();
    assert!(
        agents.contains("## Contexto para agentes (archeology-doc)"),
        "{agents}"
    );

    // --lang inválido en init: error (no solo aviso).
    let other = Sandbox::new();
    other
        .cmd()
        .args(["init", "--yes", "--lang", "fr"])
        .assert()
        .failure();
    assert!(!other.root.join("archeology-doc.toml").exists());
}

#[test]
fn init_with_lang_en_generates_english_content() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--lang", "en"]);
    let root = &sb.root;
    let toml = fs::read_to_string(root.join("archeology-doc.toml")).unwrap();
    assert!(toml.contains("language = \"en\""), "{toml}");
    let agents = fs::read_to_string(root.join("AGENTS.md")).unwrap();
    assert!(
        agents.contains("## Agent context (archeology-doc)"),
        "{agents}"
    );
    assert!(agents.contains("Write the notes in English."), "{agents}");
    let feature = fs::read_to_string(root.join("knowledge/_templates/feature.md")).unwrap();
    assert!(feature.contains("## Main flow"), "{feature}");
    let facts = fs::read_to_string(root.join("knowledge/memory/facts.md")).unwrap();
    assert!(facts.contains("# Confirmed facts"), "{facts}");
    let llms = fs::read_to_string(root.join("llms.txt")).unwrap();
    assert!(llms.contains("Index for LLMs"), "{llms}");
    let vp = fs::read_to_string(root.join("docs/.vitepress/config.mts")).unwrap();
    assert!(vp.contains("lang: 'en-US'"), "{vp}");

    // La interfaz sigue en español (ADOC_LANG=es); el contenido, en inglés.
    let out = sb.cmd().args(["scan"]).assert().success();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(!err.contains("Initializing"), "{err}");
    let agents = fs::read_to_string(root.join("AGENTS.md")).unwrap();
    assert!(
        agents.contains("## Project (archeology-doc inventory)"),
        "{agents}"
    );
    sb.run(&["plan"]);
    let task = fs::read_to_string(sb.work().join("tasks/T-001.md")).unwrap();
    assert!(task.contains("# Objective\n"), "{task}");
    assert!(task.contains("Write the notes in English."), "{task}");
}

#[test]
fn config_without_language_generates_spanish_content() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--lang", "en"]);
    // Configuración anterior a la internacionalización: sin el campo.
    let path = sb.root.join("archeology-doc.toml");
    let toml = fs::read_to_string(&path).unwrap();
    let without: String = toml
        .lines()
        .filter(|l| !l.trim_start().starts_with("language"))
        .map(|l| format!("{l}\n"))
        .collect();
    fs::write(&path, without).unwrap();
    fs::remove_file(sb.root.join("AGENTS.md")).unwrap();

    // Interfaz en inglés, contenido en español.
    sb.cmd()
        .env("ADOC_LANG", "en")
        .args(["scan"])
        .assert()
        .success();
    let agents = fs::read_to_string(sb.root.join("AGENTS.md")).unwrap();
    assert!(
        agents.contains("## Contexto para agentes (archeology-doc)"),
        "{agents}"
    );
    assert!(
        agents.contains("## Proyecto (inventario de archeology-doc)"),
        "{agents}"
    );
    sb.cmd()
        .env("ADOC_LANG", "en")
        .args(["plan"])
        .assert()
        .success();
    let task = fs::read_to_string(sb.work().join("tasks/T-001.md")).unwrap();
    assert!(task.contains("# Objetivo\n"), "{task}");
}

// --- i18n fase 2: comandos de trabajo ---

/// Ejecuta `args` con la interfaz en inglés y devuelve (stdout, stderr).
fn run_en(sb: &Sandbox, args: &[&str], ok: bool) -> (String, String) {
    let mut cmd = sb.cmd();
    cmd.env("ADOC_LANG", "en").args(args);
    let out = if ok {
        cmd.assert().success()
    } else {
        cmd.assert().failure()
    };
    let o = out.get_output();
    (
        String::from_utf8_lossy(&o.stdout).to_string(),
        String::from_utf8_lossy(&o.stderr).to_string(),
    )
}

#[test]
fn work_commands_speak_english() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes"]);

    // Sin inventario: plan falla con el mensaje en inglés.
    let (_, err) = run_en(&sb, &["plan"], false);
    assert!(
        err.contains("no inventory found; run `adoc scan` first"),
        "{err}"
    );

    sb.run(&["scan"]);
    let (out, _) = run_en(&sb, &["plan"], true);
    assert!(out.contains("profile: "), "{out}");
    assert!(
        out.contains("Next step: `adoc inspect` or `adoc task list`."),
        "{out}"
    );

    let (out, _) = run_en(&sb, &["task", "list"], true);
    assert!(out.contains("task") && out.contains("status"), "{out}");
    let (_, err) = run_en(&sb, &["task", "show", "T-999"], false);
    assert!(err.contains("task T-999 does not exist"), "{err}");
    let (_, err) = run_en(&sb, &["task", "show", "bad id"], false);
    assert!(err.contains("invalid task id"), "{err}");

    let (out, _) = run_en(&sb, &["inspect", "--dry-run"], true);
    assert!(out.contains("tasks would run:"), "{out}");

    let (out, _) = run_en(&sb, &["check"], true);
    assert!(out.contains("check: 0 errors"), "{out}");

    let (out, _) = run_en(&sb, &["sync", "--dry-run"], true);
    assert!(out.contains("sync: no changes detected"), "{out}");

    // Cambio de agente: mensajes de `agent use` en inglés.
    let (out, _) = run_en(&sb, &["agent", "use", "manual"], true);
    assert!(out.contains("active agent: manual"), "{out}");
    assert!(out.contains("export mode"), "{out}");
}

// --- i18n fase 2: sitio y reportes ---

/// Ejecuta con la interfaz en inglés y devuelve stdout (debe terminar bien).
fn site_run_en(sb: &Sandbox, args: &[&str]) -> String {
    let out = sb
        .cmd()
        .env("ADOC_LANG", "en")
        .args(args)
        .assert()
        .success();
    String::from_utf8_lossy(&out.get_output().stdout).to_string()
}

#[test]
fn site_and_report_commands_speak_english() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--lang", "en"]);

    // scan: resumen en inglés y región del inventario en el idioma del proyecto.
    let out = site_run_en(&sb, &["scan"]);
    assert!(out.contains("Next step: `adoc plan`"), "{out}");
    let overview = fs::read_to_string(sb.root.join("knowledge/00-overview.md")).unwrap();
    assert!(overview.contains("## Summary"), "{overview}");
    assert!(overview.contains("- Entry points:"), "{overview}");

    sb.run(&["plan"]);
    let out = site_run_en(&sb, &["review"]);
    assert!(out.contains("Progress:  0%"), "{out}");
    let out = site_run_en(&sb, &["status", "--format", "text"]);
    assert!(out.contains(" tasks · "), "{out}");

    // doctor: el JSON conserva claves y nombres; el detalle se traduce.
    let out = sb
        .cmd()
        .env("ADOC_LANG", "en")
        .args(["doctor", "--format", "json"])
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let checks = json["checks"].as_array().unwrap();
    let adoc = checks.iter().find(|c| c["name"] == "adoc").unwrap();
    assert!(
        adoc["detail"].as_str().unwrap().starts_with("version "),
        "{adoc}"
    );
    assert!(checks.iter().any(|c| c["name"] == "proyecto"), "{json}");

    // Reporte HTML: contenido en el idioma del proyecto (en), aunque la
    // interfaz esté en español.
    let html_path = sb.root.join("report.html");
    let out = sb.run(&[
        "report",
        "--html",
        "--no-open",
        "--out",
        html_path.to_str().unwrap(),
    ]);
    assert!(out.contains("reporte: "), "{out}");
    let html = fs::read_to_string(&html_path).unwrap();
    assert!(html.contains("<html lang=\"en\">"), "{html}");
    assert!(html.contains("Open findings"), "{html}");
    assert!(html.contains("Generated by archeology-doc"), "{html}");

    // Páginas del dashboard del sitio, también en inglés.
    let out = sb
        .cmd()
        .env("ADOC_LANG", "en")
        .args(["site", "sync"])
        .assert()
        .success();
    let err = String::from_utf8_lossy(&out.get_output().stderr).to_string();
    assert!(err.contains("pages in the sidebar"), "{err}");
    let estado = fs::read_to_string(sb.root.join("docs/estado.md")).unwrap();
    assert!(estado.contains("- **Plan progress:**"), "{estado}");
    let preguntas = fs::read_to_string(sb.root.join("docs/preguntas.md")).unwrap();
    assert!(
        preguntas.contains("## Frequently asked questions (FAQ)"),
        "{preguntas}"
    );
}

// --- i18n fase 2: ayuda y crates ---

/// `--help` sigue el idioma de la interfaz (ADOC_LANG o --lang), también en
/// subcomandos y en el subcomando `help` de clap.
#[test]
fn help_follows_the_interface_language() {
    let sb = Sandbox::new();
    let help = |lang: &str, args: &[&str]| {
        let out = sb
            .cmd()
            .env("ADOC_LANG", lang)
            .args(args)
            .assert()
            .success();
        String::from_utf8_lossy(&out.get_output().stdout).to_string()
    };
    let en = help("en", &["--help"]);
    assert!(en.contains("Usage:") && en.contains("Commands:"), "{en}");
    assert!(en.contains("Options:") && en.contains("Print help"), "{en}");
    assert!(
        en.contains("Deterministic inventory of the repository"),
        "{en}"
    );
    assert!(!en.contains("Inventario determinista"), "{en}");

    let es = help("es", &["--help"]);
    assert!(es.contains("Uso:") && es.contains("Comandos:"), "{es}");
    assert!(
        es.contains("Opciones:") && es.contains("Muestra la ayuda"),
        "{es}"
    );
    assert!(
        es.contains("Inventario determinista del repositorio"),
        "{es}"
    );
    assert!(!es.contains("Usage:") && !es.contains("Print help"), "{es}");

    // --lang manda sobre ADOC_LANG; subcomandos anidados y `help <cmd>`.
    let out = help("es", &["--lang", "en", "task", "answer", "--help"]);
    assert!(
        out.contains("Arguments:") && out.contains("Task ID"),
        "{out}"
    );
    let out = help("es", &["help", "spec", "baseline"]);
    assert!(out.contains("Prepara specs base inferidas"), "{out}");
    assert!(out.contains("baseline-<fecha>"), "{out}");
    let out = help("en", &["agent", "--help"]);
    assert!(out.contains("Change the agent"), "{out}");
}

/// Mensajes de los crates (registro, OpenSpec) en el idioma de la interfaz.
#[test]
fn crate_messages_follow_the_interface_language() {
    let sb = Sandbox::new();
    fixture(&sb.root);
    sb.run(&["init", "--yes", "--agent", "manual"]);
    let change = sb.root.join("openspec/changes/add-x");
    fs::create_dir_all(&change).unwrap();
    let out = sb
        .cmd()
        .env("ADOC_LANG", "en")
        .env("PATH", "")
        .args(["spec", "validate", "add-x"])
        .assert();
    let o = out.get_output();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(text.contains("proposal.md is missing"), "{text}");
    assert!(!text.contains("falta proposal.md"), "{text}");
}
