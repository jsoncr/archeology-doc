//! Configuración `arqueology-doc.toml` (§12 del kickoff).

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CONFIG_FILE: &str = "arqueology-doc.toml";
/// Directorio de estado antiguo dentro del proyecto (se migra a `paths::workspace_dir`).
pub const STATE_DIR: &str = ".arqueology-doc";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArqueologyConfig {
    pub project: ProjectSection,
    #[serde(default)]
    pub paths: PathsSection,
    #[serde(default)]
    pub scan: ScanSection,
    #[serde(default)]
    pub plan: PlanSection,
    #[serde(default)]
    pub links: LinksSection,
    #[serde(default)]
    pub agent: AgentSection,
    #[serde(default)]
    pub site: SiteSection,
    #[serde(default)]
    pub openspec: OpenSpecSection,
    #[serde(default)]
    pub context: ContextSection,
}

/// Contexto para agentes: adaptadores por herramienta y skills (RF-101, RF-106).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSection {
    /// Archivos por herramienta que apuntan a AGENTS.md: claude|cursor|copilot.
    #[serde(default)]
    pub adapters: Vec<String>,
    /// Generar skills en `.agents/skills/`.
    #[serde(default = "default_true")]
    pub skills: bool,
}

impl Default for ContextSection {
    fn default() -> Self {
        Self {
            adapters: Vec::new(),
            skills: true,
        }
    }
}

/// Adaptadores de contexto soportados.
pub const CONTEXT_ADAPTERS: &[&str] = &["claude", "cursor", "copilot"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSection {
    pub id: String,
    pub name: String,
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_language() -> String {
    "es".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsSection {
    #[serde(default = "default_knowledge")]
    pub knowledge: String,
    #[serde(default = "default_docs")]
    pub docs: String,
    #[serde(default = "default_exclude")]
    pub exclude: Vec<String>,
}

impl Default for PathsSection {
    fn default() -> Self {
        Self {
            knowledge: default_knowledge(),
            docs: default_docs(),
            exclude: default_exclude(),
        }
    }
}

fn default_knowledge() -> String {
    "knowledge".to_string()
}
fn default_docs() -> String {
    "docs".to_string()
}
fn default_exclude() -> Vec<String> {
    // `**/` delante: sin `.gitignore` (proyectos legacy) también se excluyen
    // las carpetas anidadas, no solo las de la raíz.
    vec![
        "**/vendor/**".to_string(),
        "**/node_modules/**".to_string(),
        "**/target/**".to_string(),
        "**/dist/**".to_string(),
        "*.min.js".to_string(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSection {
    #[serde(default = "default_true")]
    pub git_history: bool,
    #[serde(default = "default_max_file_size")]
    pub max_file_size_kb: u64,
    #[serde(default = "default_secret_patterns")]
    pub secrets_patterns: Vec<String>,
}

impl Default for ScanSection {
    fn default() -> Self {
        Self {
            git_history: true,
            max_file_size_kb: default_max_file_size(),
            secrets_patterns: default_secret_patterns(),
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_max_file_size() -> u64 {
    512
}
/// Patrones de secretos que siempre se excluyen. La config del repositorio
/// puede añadir patrones, pero no quitar estos (ver `ArqueologyConfig::load`).
pub fn default_secret_patterns() -> Vec<String> {
    [
        ".env*",
        "*.pem",
        "*.key",
        "credentials*",
        "*.p12",
        "*.pfx",
        "*.jks",
        "*.keystore",
        "*id_rsa*",
        "*id_ed25519*",
        "*id_ecdsa*",
        ".npmrc",
        ".pypirc",
        ".netrc",
        ".git-credentials",
        "*.tfvars",
        "*.tfstate",
        "secrets.*",
        "*.secret",
        ".aws/**",
        ".docker/config.json",
        "*.kdbx",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSection {
    #[serde(default = "default_max_files")]
    pub max_files_per_task: usize,
    #[serde(default = "default_max_bytes")]
    pub max_bytes_per_task: u64,
    #[serde(default)]
    pub priority: Vec<String>,
    /// Perfil de tamaño: auto|small|medium|large. Decide cuántas tareas genera
    /// el plan: en proyectos pequeños se agrupa el trabajo en pocas tareas.
    #[serde(default = "default_size")]
    pub size: String,
}

fn default_size() -> String {
    "auto".to_string()
}

impl Default for PlanSection {
    fn default() -> Self {
        Self {
            max_files_per_task: default_max_files(),
            max_bytes_per_task: default_max_bytes(),
            priority: vec![
                "entrypoints".to_string(),
                "hotspots".to_string(),
                "untested".to_string(),
            ],
            size: default_size(),
        }
    }
}

fn default_max_files() -> usize {
    40
}
fn default_max_bytes() -> u64 {
    400_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinksSection {
    #[serde(default = "default_link_style")]
    pub style: String,
}

impl Default for LinksSection {
    fn default() -> Self {
        Self {
            style: default_link_style(),
        }
    }
}

fn default_link_style() -> String {
    "wikilink".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSection {
    #[serde(default = "default_agent")]
    pub default: String,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_retries")]
    pub retries: u32,
    #[serde(default)]
    pub adapters: BTreeMap<String, AdapterConfig>,
}

/// Adaptador headless predefinido: comando, argumentos y si la tarea va por stdin.
fn headless(command: &str, args: &[&str], stdin: bool) -> AdapterConfig {
    AdapterConfig {
        command: Some(command.to_string()),
        args: args.iter().map(|a| a.to_string()).collect(),
        env: Default::default(),
        mode: AdapterMode::Headless,
        timeout_secs: None,
        stdin,
        model: None,
        interactive_args: Vec::new(),
        trusted: true,
    }
}

/// Igual que [`headless`], con argumentos para el modo interactivo.
fn with_interactive(mut cfg: AdapterConfig, args: &[&str]) -> AdapterConfig {
    cfg.interactive_args = args.iter().map(|a| a.to_string()).collect();
    cfg
}

/// Agentes conocidos, con los flags de su modo no interactivo y con permiso
/// para escribir archivos (sin preguntar). Verificados contra la documentación
/// de cada CLI; los flags cambian con frecuencia: `adoc agent test` los prueba.
///
/// La tarea empieza con `---`: se envía por stdin, por archivo
/// (`{prompt_file}`) o como argumento con un encabezado delante
/// (`{prompt_file_content}`), nunca como argumento suelto.
pub fn builtin_adapters() -> BTreeMap<String, AdapterConfig> {
    let mut a = BTreeMap::new();
    // Claude Code: `-p` lee el prompt de stdin.
    a.insert(
        "claude".into(),
        with_interactive(
            headless("claude", &["-p", "--permission-mode", "acceptEdits"], true),
            &["--permission-mode", "acceptEdits", "{prompt_file_content}"],
        ),
    );
    // OpenAI Codex: `exec -` lee stdin; sandbox con escritura en el repo.
    a.insert(
        "codex".into(),
        with_interactive(
            headless("codex", &["exec", "-s", "workspace-write", "-"], true),
            &["{prompt_file_content}"],
        ),
    );
    // Devin CLI (Cognition). `smart`: un modelo aprueba las acciones seguras
    // (lecturas, comandos de exploración). Con `accept-edits` Devin rechaza los
    // comandos de shell en modo no interactivo y abandona la tarea.
    a.insert(
        "devin".into(),
        with_interactive(
            headless(
                "devin",
                &[
                    "-p",
                    "--permission-mode",
                    "smart",
                    "--respect-workspace-trust",
                    "false",
                    "--prompt-file",
                    "{prompt_file}",
                ],
                false,
            ),
            &[
                "--permission-mode",
                "smart",
                "--prompt-file",
                "{prompt_file}",
            ],
        ),
    );
    // Gemini CLI: con stdin redirigido corre sin interfaz.
    a.insert(
        "gemini".into(),
        with_interactive(
            headless("gemini", &["--approval-mode", "auto_edit"], true),
            &[
                "--approval-mode",
                "auto_edit",
                "-i",
                "{prompt_file_content}",
            ],
        ),
    );
    // Qwen Code (derivado de Gemini CLI).
    a.insert(
        "qwen".into(),
        with_interactive(
            headless("qwen", &["--approval-mode", "auto-edit"], true),
            &[
                "--approval-mode",
                "auto-edit",
                "-i",
                "{prompt_file_content}",
            ],
        ),
    );
    // OpenCode: `run` aprueba los permisos automáticamente.
    a.insert(
        "opencode".into(),
        headless("opencode", &["run", "{prompt_file_content}"], false),
    );
    // Cursor CLI (ejecutable `agent`): `--force` aplica los cambios.
    a.insert(
        "cursor".into(),
        with_interactive(
            headless(
                "agent",
                &[
                    "-p",
                    "--force",
                    "--output-format",
                    "text",
                    "{prompt_file_content}",
                ],
                false,
            ),
            &["{prompt_file_content}"],
        ),
    );
    // GitHub Copilot CLI: modo programático.
    a.insert(
        "copilot".into(),
        headless(
            "copilot",
            &["-p", "{prompt_file_content}", "--allow-all-tools"],
            false,
        ),
    );
    // Amp: modo execute.
    a.insert(
        "amp".into(),
        headless(
            "amp",
            &["--dangerously-allow-all", "-x", "{prompt_file_content}"],
            false,
        ),
    );
    // goose: `run -i -` lee las instrucciones de stdin.
    a.insert("goose".into(), headless("goose", &["run", "-i", "-"], true));
    // Aider: sin commits automáticos (arqueology-doc no toca el historial).
    a.insert(
        "aider".into(),
        headless(
            "aider",
            &[
                "--message-file",
                "{prompt_file}",
                "--yes-always",
                "--no-auto-commits",
            ],
            false,
        ),
    );
    // Sin agente: exporta las tareas para ejecutarlas a mano (CU-10).
    a.insert("manual".into(), AdapterConfig::manual());
    a
}

impl Default for AgentSection {
    fn default() -> Self {
        Self {
            default: default_agent(),
            timeout_secs: default_timeout(),
            retries: default_retries(),
            adapters: builtin_adapters(),
        }
    }
}

fn default_agent() -> String {
    "manual".to_string()
}
fn default_timeout() -> u64 {
    900
}
fn default_retries() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdapterConfig {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub mode: AdapterMode,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Enviar la tarea por la entrada estándar del comando.
    #[serde(default)]
    pub stdin: bool,
    /// Modelo del agente. Se sustituye en `{model}` dentro de `args`; en los
    /// CLIs conocidos (claude, codex, gemini, opencode) se añade `--model`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Argumentos para abrir una sesión interactiva con la tarea como primer
    /// mensaje (`adoc task run <id> --interactive`). Mismos placeholders que `args`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interactive_args: Vec<String>,
    /// Se puede ejecutar: es predefinido o el usuario lo aprobó
    /// (`crate::trust`). Nunca se lee del TOML del repositorio.
    #[serde(skip)]
    pub trusted: bool,
}

impl AdapterConfig {
    pub fn manual() -> Self {
        Self {
            command: None,
            args: vec![],
            env: Default::default(),
            mode: AdapterMode::Export,
            timeout_secs: None,
            stdin: false,
            model: None,
            interactive_args: Vec::new(),
            trusted: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AdapterMode {
    #[default]
    Headless,
    Export,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteSection {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub include_knowledge: bool,
    #[serde(default = "default_true")]
    pub include_specs: bool,
}

impl Default for SiteSection {
    fn default() -> Self {
        Self {
            enabled: true,
            include_knowledge: true,
            include_specs: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenSpecSection {
    /// auto|true|false — auto = activado si existe el directorio openspec/.
    #[serde(default = "default_openspec_enabled")]
    pub enabled: String,
    #[serde(default = "default_openspec_path")]
    pub path: String,
    #[serde(default = "default_true")]
    pub validate_with_cli: bool,
    /// change|off — specs base como carpeta de cambio.
    #[serde(default = "default_baseline")]
    pub baseline: String,
    #[serde(default = "default_stale_days")]
    pub stale_change_days: u64,
}

impl Default for OpenSpecSection {
    fn default() -> Self {
        Self {
            enabled: default_openspec_enabled(),
            path: default_openspec_path(),
            validate_with_cli: true,
            baseline: default_baseline(),
            stale_change_days: default_stale_days(),
        }
    }
}

fn default_openspec_enabled() -> String {
    "auto".to_string()
}
fn default_openspec_path() -> String {
    "openspec".to_string()
}
fn default_baseline() -> String {
    "change".to_string()
}
fn default_stale_days() -> u64 {
    30
}

impl ArqueologyConfig {
    pub fn new(name: impl Into<String>, language: impl Into<String>) -> Self {
        Self {
            project: ProjectSection {
                id: uuid::Uuid::new_v4().to_string(),
                name: name.into(),
                language: language.into(),
            },
            ..Default::default()
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let mut cfg: Self = toml::from_str(&text)?;
        // Adaptadores predefinidos que falten (configs mínimas o antiguas).
        for (name, adapter) in builtin_adapters() {
            cfg.agent.adapters.entry(name).or_insert(adapter);
        }
        // El repositorio puede añadir patrones de secretos, no quitarlos.
        for p in default_secret_patterns() {
            if !cfg.scan.secrets_patterns.contains(&p) {
                cfg.scan.secrets_patterns.push(p);
            }
        }
        cfg.validate()?;
        let root = path.parent().unwrap_or(Path::new("."));
        cfg.resolve_trust(root);
        Ok(cfg)
    }

    /// Marca qué adaptadores se pueden ejecutar en el proyecto `root`.
    pub fn resolve_trust(&mut self, root: &Path) {
        for (name, adapter) in self.agent.adapters.iter_mut() {
            adapter.trusted = crate::trust::is_trusted(root, name, adapter);
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        // El id forma parte de una ruta en `~/.arqueology-doc/projects/`:
        // solo caracteres seguros, nunca `/`, `\` ni `..`.
        let id = &self.project.id;
        if id.trim().is_empty() {
            return Err(Error::InvalidConfig(
                "project.id no puede estar vacío".into(),
            ));
        }
        if id.len() > 64
            || id.starts_with('-')
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(Error::InvalidConfig(format!(
                "project.id inválido ('{id}'): usa un UUID (letras, números y '-')"
            )));
        }
        for (key, value) in [
            ("paths.knowledge", &self.paths.knowledge),
            ("paths.docs", &self.paths.docs),
            ("openspec.path", &self.openspec.path),
        ] {
            check_inner_path(key, value)?;
        }
        for (name, adapter) in &self.agent.adapters {
            if let Some(m) = &adapter.model {
                if m.starts_with('-') || m.chars().any(char::is_whitespace) {
                    return Err(Error::InvalidConfig(format!(
                        "agent.adapters.{name}.model inválido ('{m}')"
                    )));
                }
            }
        }
        match self.links.style.as_str() {
            "wikilink" | "relative" => {}
            other => {
                return Err(Error::InvalidConfig(format!(
                    "links.style debe ser wikilink|relative, no '{other}'"
                )))
            }
        }
        for a in &self.context.adapters {
            if !CONTEXT_ADAPTERS.contains(&a.as_str()) {
                return Err(Error::InvalidConfig(format!(
                    "context.adapters: '{a}' no soportado (válidos: {})",
                    CONTEXT_ADAPTERS.join(", ")
                )));
            }
        }
        match self.plan.size.as_str() {
            "auto" | "small" | "medium" | "large" => {}
            other => {
                return Err(Error::InvalidConfig(format!(
                    "plan.size debe ser auto|small|medium|large, no '{other}'"
                )))
            }
        }
        match self.openspec.enabled.as_str() {
            "auto" | "true" | "false" => {}
            other => {
                return Err(Error::InvalidConfig(format!(
                    "openspec.enabled debe ser auto|true|false, no '{other}'"
                )))
            }
        }
        Ok(())
    }

    /// `true` si la integración con OpenSpec está activa para este proyecto.
    pub fn openspec_enabled(&self, root: &Path) -> bool {
        match self.openspec.enabled.as_str() {
            "true" => true,
            "false" => false,
            _ => {
                root.join(&self.openspec.path).join("specs").is_dir()
                    || root.join(&self.openspec.path).join("changes").is_dir()
            }
        }
    }

    pub fn openspec_dir(&self, root: &Path) -> PathBuf {
        root.join(&self.openspec.path)
    }
}

/// Una ruta de salida debe quedar dentro del proyecto: relativa, sin `..` y
/// distinta de la raíz (las carpetas de salida se regeneran o se borran).
fn check_inner_path(key: &str, value: &str) -> Result<()> {
    use std::path::Component;
    let path = Path::new(value);
    let mut normal = 0;
    for c in path.components() {
        match c {
            Component::Normal(_) => normal += 1,
            Component::CurDir => {}
            _ => {
                return Err(Error::InvalidConfig(format!(
                    "{key} = '{value}' debe ser una ruta relativa dentro del proyecto (sin '..')"
                )))
            }
        }
    }
    // `C:foo` en Windows no da componentes Prefix en todas las formas.
    if normal == 0 || value.contains(':') {
        return Err(Error::InvalidConfig(format!(
            "{key} = '{value}' debe ser una carpeta dentro del proyecto"
        )));
    }
    Ok(())
}

impl Default for ArqueologyConfig {
    fn default() -> Self {
        Self {
            project: ProjectSection {
                id: uuid::Uuid::new_v4().to_string(),
                name: String::new(),
                language: default_language(),
            },
            paths: PathsSection::default(),
            scan: ScanSection::default(),
            plan: PlanSection::default(),
            links: LinksSection::default(),
            agent: AgentSection::default(),
            site: SiteSection::default(),
            openspec: OpenSpecSection::default(),
            context: ContextSection::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_roundtrips_toml() {
        let cfg = ArqueologyConfig::new("demo", "es");
        let text = toml::to_string_pretty(&cfg).unwrap();
        let parsed: ArqueologyConfig = toml::from_str(&text).unwrap();
        assert_eq!(parsed.project.name, "demo");
        parsed.validate().unwrap();
    }

    fn cfg_with(f: impl FnOnce(&mut ArqueologyConfig)) -> ArqueologyConfig {
        let mut cfg = ArqueologyConfig::new("demo", "es");
        f(&mut cfg);
        cfg
    }

    #[test]
    fn rejects_ids_that_escape_the_workspace() {
        for id in ["../..", "..", "a/b", "a\\b", "-x", "x y", ""] {
            let cfg = cfg_with(|c| c.project.id = id.into());
            assert!(cfg.validate().is_err(), "id aceptado: {id:?}");
        }
        let cfg = cfg_with(|c| c.project.id = "3f2a9c1e-0000-4000-8000-000000000000".into());
        cfg.validate().unwrap();
    }

    #[test]
    fn rejects_output_paths_outside_the_project() {
        for p in ["/etc", "../x", "docs/../../x", ".", "", "C:/x"] {
            let cfg = cfg_with(|c| c.paths.docs = p.into());
            assert!(cfg.validate().is_err(), "docs aceptado: {p:?}");
            let cfg = cfg_with(|c| c.paths.knowledge = p.into());
            assert!(cfg.validate().is_err(), "knowledge aceptado: {p:?}");
            let cfg = cfg_with(|c| c.openspec.path = p.into());
            assert!(cfg.validate().is_err(), "openspec aceptado: {p:?}");
        }
        let cfg = cfg_with(|c| c.paths.docs = "site/docs".into());
        cfg.validate().unwrap();
    }

    #[test]
    fn rejects_models_that_look_like_flags() {
        let cfg =
            cfg_with(|c| c.agent.adapters.get_mut("claude").unwrap().model = Some("--yolo".into()));
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn load_restores_builtins_and_secret_patterns() {
        let dir = std::env::temp_dir().join(format!("adoc-cfg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CONFIG_FILE);
        std::fs::write(
            &path,
            "[project]\nid = \"abc12345\"\nname = \"x\"\n[agent]\ndefault = \"claude\"\n\
             [scan]\nsecrets_patterns = []\n",
        )
        .unwrap();
        let cfg = ArqueologyConfig::load(&path).unwrap();
        assert!(cfg.agent.adapters["claude"].trusted);
        assert!(cfg.scan.secrets_patterns.contains(&".env*".to_string()));
        std::fs::remove_dir_all(&dir).ok();
    }
}
