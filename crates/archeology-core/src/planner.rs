//! Planner: divide el proyecto en tareas acotadas (RF-030 a RF-035).

use crate::config::PlanSection;
use crate::i18n::Lang;
use crate::model::{Inventory, Task, TaskStatus, TaskType};
use crate::tl;

/// Perfil de tamaño del proyecto: decide la granularidad del plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeProfile {
    /// Pocas tareas grandes: cada grupo de módulos se documenta completo
    /// (módulos + funcionalidades + tests) en una sola sesión del agente.
    Small,
    /// Dos tareas por grupo: módulos + funcionalidades, y tests.
    Medium,
    /// Tres tareas por grupo: módulos, funcionalidades y tests por separado.
    Large,
}

/// Umbrales de `plan.size = "auto"` en archivos de código.
pub const SMALL_MAX_CODE_FILES: usize = 150;
pub const MEDIUM_MAX_CODE_FILES: usize = 1500;

impl SizeProfile {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    /// Perfil configurado o, con `auto`, según los archivos de código.
    pub fn resolve(inv: &Inventory, plan_cfg: &PlanSection) -> Self {
        match plan_cfg.size.as_str() {
            "small" => Self::Small,
            "medium" => Self::Medium,
            "large" => Self::Large,
            _ => {
                let code = code_files(inv);
                if code <= SMALL_MAX_CODE_FILES {
                    Self::Small
                } else if code <= MEDIUM_MAX_CODE_FILES {
                    Self::Medium
                } else {
                    Self::Large
                }
            }
        }
    }
}

/// ¿La tarea (campo `module`, p. ej. `"api,db"`) cubre el módulo `m`?
///
/// Los nombres de módulo del scanner no contienen comas; por compatibilidad
/// con inventarios antiguos también cuenta la coincidencia del campo entero.
pub fn covers_module(module_field: Option<&str>, m: &str) -> bool {
    let m = m.trim();
    if m.is_empty() {
        return false;
    }
    module_field
        .map(|f| f.trim() == m || f.split(',').any(|x| x.trim() == m))
        .unwrap_or(false)
}

/// Rutas del proyecto que usan las tareas, derivadas de la configuración.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanPaths {
    /// `paths.knowledge` (vault).
    pub knowledge: String,
    /// `paths.docs` (sitio).
    pub docs: String,
    /// `openspec.path`.
    pub openspec: String,
    /// Idioma de la documentación: los objetivos de las tareas se escriben en él.
    pub lang: Lang,
}

impl PlanPaths {
    pub fn from_config(cfg: &crate::config::ArcheologyConfig) -> Self {
        Self {
            knowledge: cfg.paths.knowledge.trim_end_matches('/').to_string(),
            docs: cfg.paths.docs.trim_end_matches('/').to_string(),
            openspec: cfg.openspec.path.trim_end_matches('/').to_string(),
            lang: cfg.project.lang(),
        }
    }
}

impl Default for PlanPaths {
    fn default() -> Self {
        Self::from_config(&crate::config::ArcheologyConfig::default())
    }
}

/// Anotación (en español) de las salidas de `# Produce` que el agente puede
/// no crear. Ver [`optional_note`] para el texto en otro idioma.
pub const OPTIONAL_NOTE: &str = "(opcional)";
/// La misma anotación en todos los idiomas: los archivos de tarea pueden
/// estar escritos en cualquiera de ellos.
const OPTIONAL_NOTES: &[&str] = &["(opcional)", "(optional)"];
/// Anotaciones conocidas tras una ruta en las secciones de la tarea.
const KNOWN_NOTES: &[&str] = &["(opcional)", "(optional)", "(si existe)", "(if it exists)"];

/// Anotación de salida opcional en el idioma `lang`.
pub fn optional_note(lang: Lang) -> String {
    tl!(lang, "doc.task.optional")
}

/// Encabezados de sección del archivo de tarea en todos los idiomas, por
/// clave (`doc.task.section.<clave>`). `Produce` coincide en ambos.
fn section_names(key: &str) -> Vec<String> {
    Lang::ALL
        .iter()
        .map(|l| tl!(*l, &format!("doc.task.section.{key}")))
        .collect()
}

/// Idioma en el que está escrito un archivo de tarea (por sus encabezados).
pub fn task_lang(text: &str) -> Lang {
    for lang in Lang::ALL {
        let objective = format!("# {}", tl!(lang, "doc.task.section.objective"));
        if text.lines().any(|l| l.trim_end() == objective) {
            return lang;
        }
    }
    crate::config::LEGACY_DEFAULT_LANGUAGE
}

/// Separa `ruta (opcional)` en la ruta y su anotación conocida. Solo se
/// reconocen anotaciones conocidas al final: `New folder (2).md` es una ruta.
fn split_note(item: &str) -> (&str, Option<&'static str>) {
    let item = item.trim();
    for note in KNOWN_NOTES {
        if let Some(path) = item.strip_suffix(note) {
            if path.ends_with(' ') {
                return (path.trim_end(), Some(note));
            }
        }
    }
    (item, None)
}

/// Archivos de código del proyecto (sin docs, datos ni configuración).
pub fn code_files(inv: &Inventory) -> usize {
    let from_modules: usize = inv.modules.iter().map(|m| m.code_files).sum();
    if from_modules > 0 {
        return from_modules;
    }
    // Inventarios antiguos sin `code_files`: estimación por lenguaje.
    inv.languages
        .iter()
        .filter(|(l, _)| !NON_CODE_LANGUAGES.contains(&l.as_str()))
        .map(|(_, s)| s.files)
        .sum()
}

const NON_CODE_LANGUAGES: &[&str] = &["Markdown", "JSON", "YAML", "TOML", "XML", "HTML", "CSS"];

/// Agrupa módulos (ya priorizados) en lotes que respetan los límites de
/// contexto (RNF-02): muchos módulos pequeños comparten una tarea.
pub fn group_modules<'a>(
    modules: &[&'a crate::model::ModuleInfo],
    max_files: usize,
    max_bytes: u64,
) -> Vec<Vec<&'a crate::model::ModuleInfo>> {
    let mut groups: Vec<Vec<&crate::model::ModuleInfo>> = Vec::new();
    let (mut files, mut bytes) = (0usize, 0u64);
    for m in modules {
        let fits = files + m.files <= max_files && bytes + m.bytes <= max_bytes;
        match groups.last_mut() {
            Some(g) if fits => g.push(m),
            _ => {
                groups.push(vec![m]);
                files = 0;
                bytes = 0;
            }
        }
        files += m.files;
        bytes += m.bytes;
    }
    groups
}

/// Genera el plan de tareas a partir del inventario.
///
/// Orden: overview → grupos de módulos (priorizados según `plan.priority`,
/// RF-033) → contradicciones → glosario → manual de usuario → contexto de
/// agentes → cumplimiento de specs. La granularidad depende del perfil de
/// tamaño: en proyectos pequeños hay pocas tareas, cada una más completa.
pub fn build_plan(
    inv: &Inventory,
    plan_cfg: &PlanSection,
    knowledge: &str,
    has_openspec: bool,
) -> Vec<Task> {
    let paths = PlanPaths {
        knowledge: knowledge.trim_end_matches('/').to_string(),
        ..PlanPaths::default()
    };
    build_plan_with(inv, plan_cfg, &paths, has_openspec, &[])
}

/// Máximo de IDs de requisitos que se listan en la tarea de cumplimiento.
const MAX_REQUIREMENT_IDS: usize = 400;

/// Como [`build_plan`], con las rutas de la configuración (`paths.docs`,
/// `openspec.path`) y los IDs exactos de los requisitos de OpenSpec
/// (`archeology_openspec::SpecProject::requirement_ids`), que se incluyen en
/// la tarea de cumplimiento para que los reportes usen exactamente esos IDs.
pub fn build_plan_with(
    inv: &Inventory,
    plan_cfg: &PlanSection,
    paths: &PlanPaths,
    has_openspec: bool,
    requirement_ids: &[String],
) -> Vec<Task> {
    let know = paths.knowledge.trim_end_matches('/');
    let docs = paths.docs.trim_end_matches('/');
    let spec_dir = paths.openspec.trim_end_matches('/');
    let lang = paths.lang;
    let optional = optional_note(lang);
    let glossary_objective = tl!(lang, "doc.plan.glossary_objective");
    let profile = SizeProfile::resolve(inv, plan_cfg);
    let mut tasks = Vec::new();
    let mut n = 0usize;
    let next_id = |n: &mut usize| {
        *n += 1;
        format!("T-{:03}", *n)
    };
    let task = |id: String, kind: TaskType, module: Option<String>, deps: Vec<String>| Task {
        task_id: id,
        kind,
        module,
        status: TaskStatus::Pending,
        depends_on: deps,
        read: vec![],
        produce: vec![],
        objective: String::new(),
    };

    // Documentación previa del proyecto, acotada (RF-006, RNF-02).
    let existing_docs: Vec<String> = inv.doc_files.iter().take(MAX_DOC_FILES).cloned().collect();

    // Overview (en proyectos pequeños incluye el glosario).
    let overview_id = next_id(&mut n);
    let mut overview = task(overview_id.clone(), TaskType::Overview, None, vec![]);
    overview.read = existing_docs.clone();
    overview.read.push(format!("{know}/00-overview.md"));
    overview.produce = vec![format!("{know}/00-overview.md")];
    overview.objective = tl!(lang, "doc.plan.overview_objective");
    if profile == SizeProfile::Small {
        overview
            .produce
            .push(format!("{know}/glossary/ {optional}"));
        overview.objective.push(' ');
        overview.objective.push_str(&glossary_objective);
    }
    tasks.push(overview);

    // Grupos de módulos.
    // Se agrupan módulos vecinos (orden por ruta: `src/views/*` juntos) y
    // luego los grupos se ordenan por la prioridad de su mejor módulo.
    let prioritized = prioritized_modules(inv, &plan_cfg.priority);
    let rank = |m: &crate::model::ModuleInfo| {
        prioritized
            .iter()
            .position(|p| p.name == m.name)
            .unwrap_or(usize::MAX)
    };
    let mut by_path = prioritized.clone();
    by_path.sort_by(|a, b| a.path.cmp(&b.path));
    let mut groups = group_modules(
        &by_path,
        plan_cfg.max_files_per_task,
        plan_cfg.max_bytes_per_task,
    );
    groups.sort_by_key(|g| g.iter().map(|m| rank(m)).min().unwrap_or(usize::MAX));
    // Tareas de las que dependen contradicciones, manual y specs.
    let mut module_done_ids = Vec::new();
    for group in &groups {
        let names: Vec<String> = group.iter().map(|m| m.name.clone()).collect();
        let label = names.join(", ");
        let module_field = Some(names.join(","));
        let read: Vec<String> = group.iter().map(|m| module_glob(m)).collect();
        let module_notes: Vec<String> = names
            .iter()
            .map(|m| format!("{know}/modules/{m}.md"))
            .collect();
        let doc_modules = tl!(
            lang,
            "doc.plan.modules_objective",
            target = if names.len() == 1 {
                tl!(lang, "doc.plan.module_single", label = label)
            } else {
                tl!(lang, "doc.plan.module_multiple", label = label)
            }
        );
        let doc_features = tl!(lang, "doc.plan.features_objective");
        let doc_tests = tl!(lang, "doc.plan.tests_objective");
        let features_dir = format!("{know}/features/ {optional}");
        let tests_dir = format!("{know}/tests/ {optional}");

        match profile {
            SizeProfile::Small => {
                let id = next_id(&mut n);
                let mut t = task(
                    id.clone(),
                    TaskType::Module,
                    module_field,
                    vec![overview_id.clone()],
                );
                t.read = read;
                t.produce = module_notes;
                t.produce.push(features_dir);
                t.produce.push(tests_dir);
                t.objective = format!("{doc_modules} {doc_features} {doc_tests}");
                tasks.push(t);
                module_done_ids.push(id);
            }
            SizeProfile::Medium => {
                let id = next_id(&mut n);
                let mut t = task(
                    id.clone(),
                    TaskType::Module,
                    module_field.clone(),
                    vec![overview_id.clone()],
                );
                t.read = read;
                t.produce = module_notes.clone();
                t.produce.push(features_dir);
                t.objective = format!("{doc_modules} {doc_features}");
                tasks.push(t);
                let tid = next_id(&mut n);
                let mut tt = task(tid, TaskType::Tests, module_field, vec![id.clone()]);
                tt.read = module_notes;
                tt.read.push(format!("{know}/features/"));
                tt.produce = vec![tests_dir];
                tt.objective = doc_tests.clone();
                tasks.push(tt);
                module_done_ids.push(id);
            }
            SizeProfile::Large => {
                let id = next_id(&mut n);
                let mut t = task(
                    id.clone(),
                    TaskType::Module,
                    module_field.clone(),
                    vec![overview_id.clone()],
                );
                t.read = read;
                t.produce = module_notes.clone();
                t.objective = doc_modules;
                tasks.push(t);
                let fid = next_id(&mut n);
                let mut f = task(
                    fid.clone(),
                    TaskType::Feature,
                    module_field.clone(),
                    vec![id.clone()],
                );
                f.read = module_notes;
                f.produce = vec![features_dir];
                f.objective = doc_features.clone();
                tasks.push(f);
                let mut tt = task(next_id(&mut n), TaskType::Tests, module_field, vec![fid]);
                tt.read = vec![format!("{know}/features/")];
                tt.produce = vec![tests_dir];
                tt.objective = doc_tests.clone();
                tasks.push(tt);
                module_done_ids.push(id);
            }
        }
    }

    let mut contradictions = task(
        next_id(&mut n),
        TaskType::Contradictions,
        None,
        module_done_ids.clone(),
    );
    contradictions.read = existing_docs;
    contradictions.read.push(format!("{know}/modules/"));
    contradictions.read.push(format!("{know}/features/"));
    contradictions.produce = vec![format!("{know}/findings/")];
    contradictions.objective = format!(
        "{}{}",
        tl!(lang, "doc.plan.contradictions_objective"),
        marker_hints(inv, lang)
    );
    tasks.push(contradictions);

    if profile != SizeProfile::Small {
        let mut glossary = task(
            next_id(&mut n),
            TaskType::Glossary,
            None,
            vec![overview_id.clone()],
        );
        glossary.produce = vec![format!("{know}/glossary/")];
        glossary.objective = glossary_objective.clone();
        tasks.push(glossary);
    }

    let agents_produce = vec![
        "AGENTS.md".to_string(),
        format!("{know}/memory/facts.md"),
        format!("{know}/memory/conventions.md"),
    ];
    let agents_objective = tl!(lang, "doc.plan.agents_objective");
    let mut manual = task(
        next_id(&mut n),
        TaskType::UserManual,
        None,
        module_done_ids.clone(),
    );
    manual.read = vec![format!("{know}/features/")];
    manual.produce = vec![format!("{docs}/user-manual/")];
    manual.objective = tl!(lang, "doc.plan.manual_objective");
    if profile == SizeProfile::Small {
        // Proyecto pequeño: manual y contexto de agentes en una sola sesión.
        manual.produce.extend(agents_produce);
        manual.objective.push(' ');
        manual.objective.push_str(&agents_objective);
        tasks.push(manual);
    } else {
        tasks.push(manual);
        let mut agents = task(next_id(&mut n), TaskType::Agents, None, vec![overview_id]);
        agents.produce = agents_produce;
        agents.objective = agents_objective;
        tasks.push(agents);
    }

    if has_openspec {
        let mut spec = task(
            next_id(&mut n),
            TaskType::SpecCompliance,
            None,
            module_done_ids,
        );
        spec.read = vec![
            format!("{spec_dir}/specs/**/spec.md"),
            format!("{spec_dir}/changes/**"),
        ];
        spec.produce = vec![format!("{know}/specs/")];
        spec.objective = tl!(lang, "doc.plan.spec_objective", know = know);
        // IDs exactos (vienen de títulos y carpetas del repositorio: sin
        // caracteres de control que rompan el archivo de tarea).
        let ids: Vec<&str> = requirement_ids
            .iter()
            .map(|i| i.as_str())
            .filter(|i| !i.is_empty() && !i.chars().any(char::is_control))
            .collect();
        if !ids.is_empty() {
            spec.objective.push(' ');
            spec.objective
                .push_str(&tl!(lang, "doc.plan.spec_ids_intro"));
            for id in ids.iter().take(MAX_REQUIREMENT_IDS) {
                spec.objective.push_str(&format!(" `{id}`,"));
            }
            spec.objective.pop();
            if ids.len() > MAX_REQUIREMENT_IDS {
                spec.objective.push(' ');
                spec.objective.push_str(&tl!(
                    lang,
                    "doc.plan.spec_ids_more",
                    count = ids.len() - MAX_REQUIREMENT_IDS
                ));
            } else {
                spec.objective.push('.');
            }
        }
        tasks.push(spec);
    }

    tasks
}

/// Reglas comunes de las tareas, derivadas de la configuración.
#[derive(Debug, Clone)]
pub struct TaskRules {
    pub knowledge: String,
    /// wikilink|relative (RF-063).
    pub link_style: String,
    /// Patrones de secretos que el agente no debe leer (RNF-05).
    pub secrets: Vec<String>,
    /// `openspec.path`: sus specs son de solo lectura para el agente.
    pub openspec: String,
    /// Idioma de la documentación: el del archivo de tarea y de las notas.
    pub lang: Lang,
}

impl TaskRules {
    pub fn from_config(cfg: &crate::config::ArcheologyConfig) -> Self {
        Self {
            knowledge: cfg.paths.knowledge.trim_end_matches('/').to_string(),
            link_style: cfg.links.style.clone(),
            secrets: cfg.scan.secrets_patterns.clone(),
            openspec: cfg.openspec.path.trim_end_matches('/').to_string(),
            lang: cfg.project.lang(),
        }
    }
}

impl Default for TaskRules {
    fn default() -> Self {
        Self::from_config(&crate::config::ArcheologyConfig::default())
    }
}

/// Serializa una tarea al formato de archivo `.archeology-doc/tasks/<id>.md` (§14.1).
pub fn task_to_markdown(task: &Task, rules: &TaskRules) -> String {
    let status = match task.status {
        TaskStatus::Pending => "pending",
        TaskStatus::Running => "running",
        TaskStatus::Done => "done",
        TaskStatus::Failed => "failed",
        TaskStatus::Manual => "manual",
        TaskStatus::Question => "question",
    };
    let deps = task
        .depends_on
        .iter()
        .map(|d| yaml_scalar(d))
        .collect::<Vec<_>>()
        .join(", ");
    // Valores serializados como YAML: un módulo `a: b`, `#x` o `[x]` no
    // rompe ni altera el frontmatter.
    let module = task
        .module
        .as_deref()
        .map(yaml_scalar)
        .unwrap_or_else(|| "null".into());
    let mut out = format!(
        "---\ntask_id: {}\ntype: {}\nmodule: {}\nstatus: {}\ndepends_on: [{}]\n---\n",
        yaml_scalar(&task.task_id),
        yaml_scalar(task.kind.as_str()),
        module,
        yaml_scalar(status),
        deps
    );
    let lang = rules.lang;
    let section = |key: &str| tl!(lang, &format!("doc.task.section.{key}"));
    out.push_str(&format!(
        "# {}\n{}\n\n",
        section("objective"),
        flatten_lines(&task.objective)
    ));
    out.push_str(&format!("# {}\n", section("read")));
    for r in &task.read {
        out.push_str(&format!("- {}\n", flatten_lines(r)));
    }
    if task.read.is_empty() {
        out.push_str(&format!("- {}\n", tl!(lang, "doc.task.read_default")));
    }
    out.push_str(&format!("\n# {}\n", section("produce")));
    for p in &task.produce {
        out.push_str(&format!("- {}\n", produce_item(p)));
    }
    let links = if rules.link_style == "relative" {
        tl!(lang, "doc.task.links_relative")
    } else {
        tl!(lang, "doc.task.links_wikilink")
    };
    let secrets = rules
        .secrets
        .iter()
        .map(|p| format!("`{p}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let know = &rules.knowledge;
    let spec = &rules.openspec;
    out.push_str(&format!(
        "\n# {}\n{}\n",
        section("rules"),
        tl!(
            lang,
            "doc.task.rules",
            know = know,
            links = links,
            spec = spec,
            secrets = secrets,
            answers = section("answers")
        )
        .trim_end()
    ));
    out
}

/// Datos de la tarea de `adoc ask` (pregunta libre sobre el proyecto).
#[derive(Debug, Clone)]
pub struct AskPrompt<'a> {
    /// Slug de la pregunta (id de la tarea y nombre de la nota).
    pub slug: &'a str,
    pub question: &'a str,
    /// `paths.knowledge` sin `/` final.
    pub knowledge: &'a str,
    /// Ruta relativa de la nota FAQ que debe escribir el agente.
    pub note: &'a str,
    /// Título para el frontmatter (sin comillas dobles).
    pub title: &'a str,
    /// Fecha `AAAA-MM-DD`.
    pub today: &'a str,
    pub secrets: &'a [String],
}

/// Archivo de tarea de `adoc ask` en el idioma de la documentación `lang`.
pub fn ask_prompt(lang: Lang, p: &AskPrompt) -> String {
    let section = |key: &str| tl!(lang, &format!("doc.task.section.{key}"));
    let secrets = p
        .secrets
        .iter()
        .map(|s| format!("`{s}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "---\ntask_id: ASK-{slug}\ntype: faq\nmodule: null\nstatus: pending\ndepends_on: []\n---\n\
         # {objective_h}\n{objective}\n\n\
         # {read_h}\n{read}\n\n\
         # {produce_h}\n- {note}\n\n\
         # {rules_h}\n{rules}\n",
        slug = p.slug,
        objective_h = section("objective"),
        objective = tl!(lang, "doc.ask.objective", question = p.question),
        read_h = section("read"),
        read = tl!(lang, "doc.ask.read", know = p.knowledge).trim_end(),
        produce_h = section("produce"),
        note = p.note,
        rules_h = section("rules"),
        rules = tl!(
            lang,
            "doc.ask.rules",
            note = p.note,
            slug = p.slug,
            title = p.title,
            today = p.today,
            know = p.knowledge,
            secrets = secrets
        )
        .trim_end(),
    )
}

/// Escalar YAML seguro: sin comillas si se relee igual como cadena, entre
/// comillas (serde_yaml) en otro caso.
fn yaml_scalar(value: &str) -> String {
    let plain_ok = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
        && serde_yaml::from_str::<serde_yaml::Value>(value)
            .ok()
            .and_then(|v| v.as_str().map(|s| s == value))
            .unwrap_or(false);
    if plain_ok {
        return value.to_string();
    }
    serde_yaml::to_string(value)
        .map(|s| s.trim_end().to_string())
        .unwrap_or_else(|_| format!("{value:?}"))
}

/// Una sola línea: los saltos de línea se sustituyen por espacios para que
/// un texto no pueda abrir una sección (`# Produce`) en el archivo de tarea.
fn flatten_lines(text: &str) -> String {
    text.split(['\n', '\r'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Elemento de `# Produce`: la ruta entre comillas invertidas y, detrás,
/// la anotación (`(opcional)`). Así una ruta con paréntesis (`New folder
/// (2).md`) no se confunde con una anotación.
fn produce_item(item: &str) -> String {
    let flat = flatten_lines(item);
    let (path, note) = split_note(&flat);
    let path = path.to_string();
    let quoted = if path.contains('`') {
        path
    } else {
        format!("`{path}`")
    };
    match note {
        Some(n) => format!("{quoted} {n}"),
        None => quoted,
    }
}

/// Ruta de un elemento de sección: entre comillas invertidas (formato
/// actual) o el texto sin la anotación conocida final (formato anterior).
fn item_path(item: &str) -> (String, bool) {
    let item = item.trim();
    if let Some(rest) = item.strip_prefix('`') {
        if let Some(end) = rest.find('`') {
            let path = rest[..end].trim().to_string();
            let note = rest[end + 1..].trim();
            return (path, OPTIONAL_NOTES.iter().any(|n| note.contains(n)));
        }
    }
    let (path, note) = split_note(item);
    (
        path.to_string(),
        note.is_some_and(|n| OPTIONAL_NOTES.contains(&n)),
    )
}

/// Máximo de documentos previos que lee una tarea.
const MAX_DOC_FILES: usize = 30;
/// Máximo de marcadores sugeridos a la tarea de contradicciones.
const MAX_MARKER_HINTS: usize = 20;

/// Marcadores FIXME/HACK/XXX/DEPRECATED como pistas de contradicciones.
fn marker_hints(inv: &Inventory, lang: Lang) -> String {
    let hints: Vec<String> = inv
        .markers
        .iter()
        .filter(|m| m.kind != "TODO")
        .take(MAX_MARKER_HINTS)
        .map(|m| format!("{}:{}", m.path, m.line))
        .collect();
    if hints.is_empty() {
        String::new()
    } else {
        format!(
            " {}",
            tl!(lang, "doc.plan.marker_hints", markers = hints.join(", "))
        )
    }
}

/// ¿El archivo `rel` pertenece al módulo?
fn in_module(module: &crate::model::ModuleInfo, rel: &str) -> bool {
    if module.path == "." {
        return !rel.contains('/');
    }
    match rel.strip_prefix(&format!("{}/", module.path)) {
        Some(rest) => module.recursive || !rest.contains('/'),
        None => false,
    }
}

/// Ordena los módulos según los criterios de `plan.priority` (RF-033):
/// `entrypoints` (contiene puntos de entrada), `hotspots` (commits recientes)
/// y `untested` (sin tests). Empates: tamaño descendente.
pub fn prioritized_modules<'a>(
    inv: &'a Inventory,
    priority: &[String],
) -> Vec<&'a crate::model::ModuleInfo> {
    let score = |m: &crate::model::ModuleInfo| -> Vec<u64> {
        priority
            .iter()
            .map(|p| match p.as_str() {
                "entrypoints" => inv.entrypoints.iter().filter(|e| in_module(m, e)).count() as u64,
                "hotspots" => inv
                    .git
                    .as_ref()
                    .map(|g| {
                        g.hotspots
                            .iter()
                            .filter(|(f, _)| in_module(m, f))
                            .map(|(_, n)| *n as u64)
                            .sum()
                    })
                    .unwrap_or(0),
                "untested" => u64::from(!m.has_tests),
                _ => 0,
            })
            .collect()
    };
    let mut mods: Vec<_> = inv.modules.iter().collect();
    mods.sort_by(|a, b| {
        score(b)
            .cmp(&score(a))
            .then(b.bytes.cmp(&a.bytes))
            .then(a.name.cmp(&b.name))
    });
    mods
}

/// Glob de lectura de un módulo: recursivo o solo archivos directos.
fn module_glob(module: &crate::model::ModuleInfo) -> String {
    match (module.path.as_str(), module.recursive) {
        (".", _) => "./*".into(),
        (p, true) => format!("{p}/**"),
        (p, false) => format!("{p}/*"),
    }
}

/// Metadatos de un archivo de tarea: frontmatter + sección `# Produce`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskMeta {
    pub task_id: String,
    pub kind: String,
    pub module: Option<String>,
    pub status: String,
    pub depends_on: Vec<String>,
    pub produce: Vec<String>,
    /// Salidas de `produce` marcadas `(opcional)`: se validan si existen,
    /// pero la tarea no falla si el agente no las crea.
    pub optional: Vec<String>,
}

/// Un id de tarea forma nombres de archivo (`<id>.md`, `<id>.log`): solo
/// letras, números, `-` y `_`, sin empezar por `-`.
pub fn is_valid_task_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Parsea un archivo de tarea completo (§14.1).
pub fn parse_task(text: &str) -> Option<TaskMeta> {
    let (map, body) = crate::frontmatter::extract_frontmatter(text)?;
    let get = |k: &str| {
        map.get(serde_yaml::Value::String(k.into()))
            .and_then(scalar_string)
    };
    let depends_on = map
        .get(serde_yaml::Value::String("depends_on".into()))
        .and_then(|v| v.as_sequence())
        .map(|seq| seq.iter().filter_map(scalar_string).collect())
        .unwrap_or_default();
    Some(TaskMeta {
        task_id: get("task_id").filter(|id| is_valid_task_id(id))?,
        kind: get("type").unwrap_or_default(),
        module: get("module").filter(|m| !m.is_empty() && m != "null"),
        status: get("status").unwrap_or_else(|| "pending".into()),
        depends_on,
        produce: section_items(body, "produce"),
        optional: section_raw_items(body, "produce")
            .into_iter()
            .map(|i| item_path(&i))
            .filter(|(p, optional)| *optional && !p.is_empty())
            .map(|(p, _)| p)
            .collect(),
    })
}

/// Escalar YAML como texto (`module: 2024` es un número en YAML).
fn scalar_string(v: &serde_yaml::Value) -> Option<String> {
    match v {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Rutas `- x` de una sección del cuerpo de la tarea (clave `key` de
/// `doc.task.section`, en cualquier idioma), sin anotaciones como
/// "(si existe)" u "(opcional)".
fn section_items(body: &str, key: &str) -> Vec<String> {
    section_raw_items(body, key)
        .into_iter()
        .map(|i| item_path(&i).0)
        .filter(|i| !i.is_empty())
        .collect()
}

/// Elementos `- x` de una sección, tal cual (con anotaciones).
fn section_raw_items(body: &str, key: &str) -> Vec<String> {
    let names = section_names(key);
    let mut out = Vec::new();
    let mut inside = false;
    for line in body.lines() {
        let t = line.trim();
        // Solo encabezados sin sangría: las respuestas añadidas por
        // `append_answer` van sangradas y no abren secciones.
        if let Some(h) = line.trim_end().strip_prefix("# ") {
            inside = names.iter().any(|n| h.trim().eq_ignore_ascii_case(n));
            continue;
        }
        if inside {
            if let Some(item) = t.strip_prefix("- ") {
                if !item.trim().is_empty() {
                    out.push(item.trim().to_string());
                }
            }
        }
    }
    out
}

/// Añade una respuesta a la sección `# Respuestas` (`# Answers`) de la
/// tarea (la crea si no existe), con la pregunta que la originó. Se usa el
/// idioma en el que está escrita la tarea ([`task_lang`]).
///
/// La pregunta (del agente) y la respuesta pueden tener varias líneas: las
/// siguientes se sangran, así ninguna puede abrir una sección (`# Produce`)
/// que cambie las salidas de la tarea.
pub fn append_answer(text: &str, question: Option<&str>, answer: &str, date: &str) -> String {
    let lang = task_lang(text);
    let date = flatten_lines(date);
    let answer = indent_continuation(answer);
    let mut entry = String::new();
    if let Some(q) = question {
        let q = indent_continuation(q);
        entry.push_str(&format!(
            "- {date} · {}: {q}\n  {}: {answer}\n",
            tl!(lang, "doc.task.question_label"),
            tl!(lang, "doc.task.answer_label")
        ));
    } else {
        entry.push_str(&format!("- {date} · {answer}\n"));
    }
    let mut out = text.trim_end().to_string();
    out.push('\n');
    let headings: Vec<String> = section_names("answers")
        .into_iter()
        .map(|h| format!("# {h}"))
        .collect();
    if !text
        .lines()
        .any(|l| headings.iter().any(|h| l.trim_end() == h))
    {
        out.push_str(&format!("\n# {}\n", tl!(lang, "doc.task.section.answers")));
    }
    out.push_str(&entry);
    out
}

/// Preguntas y respuestas de la sección de respuestas de una tarea (escrita
/// por [`append_answer`] en cualquier idioma): pares (pregunta, respuesta).
/// Solo la primera línea de cada una.
pub fn parse_answers(text: &str) -> Vec<(String, String)> {
    let headings: Vec<String> = section_names("answers")
        .into_iter()
        .map(|h| format!("# {h}"))
        .collect();
    let labels = |key: &str| -> Vec<String> {
        Lang::ALL
            .iter()
            .map(|l| format!("{}: ", tl!(*l, key)))
            .collect()
    };
    let questions = labels("doc.task.question_label");
    let answers = labels("doc.task.answer_label");
    let mut out = Vec::new();
    let mut lines = text
        .lines()
        .skip_while(|l| !headings.iter().any(|h| l.trim() == h))
        .skip(1);
    while let Some(line) = lines.next() {
        let Some(q) = questions
            .iter()
            .find_map(|label| line.split_once(label.as_str()).map(|(_, q)| q))
        else {
            continue;
        };
        let Some(next) = lines.next() else {
            break;
        };
        if let Some(a) = answers
            .iter()
            .find_map(|label| next.trim().strip_prefix(label.as_str()))
        {
            out.push((q.trim().to_string(), a.trim().to_string()));
        }
    }
    out
}

/// Sangra las líneas siguientes a la primera (4 espacios) y quita las vacías.
fn indent_continuation(text: &str) -> String {
    let lines: Vec<&str> = text
        .split(['\n', '\r'])
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect();
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            out.push_str("\n    ");
            out.push_str(l.trim_start());
        } else {
            out.push_str(l.trim());
        }
    }
    out
}

/// Reemplaza el `status` del frontmatter conservando el resto del archivo.
pub fn set_status_in_markdown(text: &str, status: &str) -> String {
    // Solo dentro del frontmatter: una línea `status:` del cuerpo no cuenta.
    crate::frontmatter::set_field(text, "status", status)
}

/// Parsea el frontmatter de un archivo de tarea.
pub fn task_from_markdown(text: &str) -> Option<(String, String, String)> {
    let (map, _) = crate::frontmatter::extract_frontmatter(text)?;
    let get = |k: &str| {
        map.get(serde_yaml::Value::String(k.into()))
            .and_then(|v| v.as_str().map(String::from))
    };
    Some((
        get("task_id")?,
        get("type").unwrap_or_default(),
        get("status").unwrap_or_else(|| "pending".into()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ModuleInfo;

    fn module(name: &str, bytes: u64, has_tests: bool) -> ModuleInfo {
        ModuleInfo {
            name: name.into(),
            path: name.into(),
            files: 1,
            bytes,
            languages: vec![],
            has_tests,
            recursive: true,
            code_files: 1,
        }
    }

    #[test]
    fn modules_are_prioritized_by_entrypoints_then_untested() {
        let inv = Inventory {
            modules: vec![
                module("big", 1000, true),
                module("api", 10, true),
                module("util", 10, false),
            ],
            entrypoints: vec!["api/main.py".into()],
            ..Default::default()
        };
        let prio = vec!["entrypoints".to_string(), "untested".to_string()];
        let names: Vec<_> = prioritized_modules(&inv, &prio)
            .iter()
            .map(|m| m.name.as_str())
            .collect();
        assert_eq!(names, vec!["api", "util", "big"]);
    }

    #[test]
    fn small_projects_get_few_grouped_tasks() {
        let modules: Vec<ModuleInfo> = (0..20)
            .map(|i| module(&format!("m{i:02}"), 1_000, i % 2 == 0))
            .collect();
        let inv = Inventory {
            modules,
            ..Default::default()
        };
        let cfg = PlanSection::default();
        assert_eq!(SizeProfile::resolve(&inv, &cfg), SizeProfile::Small);
        let tasks = build_plan(&inv, &cfg, "knowledge", false);
        // overview + 1 grupo (20 archivos ≤ 40) + contradicciones + manual/agentes.
        assert_eq!(
            tasks.len(),
            4,
            "{:?}",
            tasks.iter().map(|t| &t.task_id).collect::<Vec<_>>()
        );
        let group = &tasks[1];
        assert_eq!(group.module.as_deref().unwrap().split(',').count(), 20);
        assert!(group.produce.iter().any(|p| p.ends_with("(opcional)")));

        let large = PlanSection {
            size: "large".into(),
            ..Default::default()
        };
        // Grande: 3 tareas por grupo + glosario y agentes por separado.
        assert_eq!(
            build_plan(&inv, &large, "knowledge", false).len(),
            1 + 3 + 1 + 1 + 1 + 1
        );
    }

    #[test]
    fn optional_outputs_are_parsed() {
        let md = "---\ntask_id: T-1\nstatus: pending\n---\n# Produce\n- a.md\n- knowledge/features/ (opcional)\n";
        let meta = parse_task(md).unwrap();
        assert_eq!(meta.produce, vec!["a.md", "knowledge/features/"]);
        assert_eq!(meta.optional, vec!["knowledge/features/"]);
    }

    #[test]
    fn hostile_values_do_not_break_the_task_file() {
        let task = Task {
            task_id: "T-009".into(),
            kind: TaskType::Module,
            module: Some("a: b # c,[x]".into()),
            status: TaskStatus::Pending,
            depends_on: vec!["T-001".into()],
            read: vec!["src/**".into()],
            produce: vec![
                "knowledge/modules/New folder (2).md".into(),
                "knowledge/features/ (opcional)".into(),
            ],
            objective: "x\n# Produce\n- /etc/passwd".into(),
        };
        let md = task_to_markdown(&task, &TaskRules::default());
        let meta = parse_task(&md).unwrap();
        assert_eq!(meta.module.as_deref(), Some("a: b # c,[x]"));
        assert_eq!(
            meta.produce,
            vec!["knowledge/modules/New folder (2).md", "knowledge/features/"]
        );
        assert_eq!(meta.optional, vec!["knowledge/features/"]);
        // Formato anterior sin comillas invertidas.
        let old = "---\ntask_id: T-1\n---\n# Produce\n- docs/New folder (2).md\n- x/ (si existe)\n";
        assert_eq!(
            parse_task(old).unwrap().produce,
            vec!["docs/New folder (2).md", "x/"]
        );
    }

    #[test]
    fn answers_cannot_inject_sections() {
        let md = "---\ntask_id: T-1\nstatus: question\n---\n# Produce\n- `a.md`\n";
        let out = append_answer(
            md,
            Some("¿Seguro?\n# Produce\n- /etc/passwd"),
            "sí\n# Produce\n- ~/.ssh/id_rsa",
            "2026-01-01",
        );
        assert_eq!(parse_task(&out).unwrap().produce, vec!["a.md"]);
        assert!(out.contains("# Respuestas"));
    }

    #[test]
    fn plan_uses_config_paths_and_requirement_ids() {
        let inv = Inventory {
            modules: vec![module("api", 10, true)],
            ..Default::default()
        };
        let paths = PlanPaths {
            knowledge: "kb".into(),
            docs: "site".into(),
            openspec: "specs-dir".into(),
            lang: Lang::Es,
        };
        let ids = vec!["auth/login".to_string(), "auth/login-2".to_string()];
        let tasks = build_plan_with(&inv, &PlanSection::default(), &paths, true, &ids);
        let manual = tasks
            .iter()
            .find(|t| t.kind == TaskType::UserManual)
            .unwrap();
        assert_eq!(manual.produce[0], "site/user-manual/");
        let spec = tasks
            .iter()
            .find(|t| t.kind == TaskType::SpecCompliance)
            .unwrap();
        assert!(spec.read[0].starts_with("specs-dir/specs/"));
        assert!(spec.objective.contains("`auth/login-2`"));
        // El glosario no choca con la nota índice (MOC) `glossary/index.md`.
        assert!(!tasks
            .iter()
            .flat_map(|t| t.produce.iter())
            .any(|p| p.contains("glossary/index.md")));
    }

    #[test]
    fn task_files_follow_the_project_language() {
        let task = Task {
            task_id: "T-002".into(),
            kind: TaskType::Module,
            module: Some("src".into()),
            status: TaskStatus::Pending,
            depends_on: vec![],
            read: vec![],
            produce: vec!["knowledge/features/ (optional)".into()],
            objective: "x".into(),
        };
        let rules = TaskRules {
            lang: Lang::En,
            ..TaskRules::default()
        };
        let md = task_to_markdown(&task, &rules);
        assert!(md.contains("# Objective\n"), "{md}");
        assert!(md.contains("# Rules\n"), "{md}");
        assert!(md.contains("Write the notes in English"), "{md}");
        assert_eq!(task_lang(&md), Lang::En);
        let meta = parse_task(&md).unwrap();
        assert_eq!(meta.optional, vec!["knowledge/features/"]);
        let answered = append_answer(&md, Some("Why?"), "Because.", "2026-01-01");
        assert!(answered.contains("# Answers\n"), "{answered}");
        assert!(answered.contains("Question: Why?\n  Answer: Because."));
        assert_eq!(
            parse_answers(&answered),
            vec![("Why?".to_string(), "Because.".to_string())]
        );

        let es = task_to_markdown(&task, &TaskRules::default());
        assert!(es.contains("# Objetivo\n"), "{es}");
        assert!(es.contains("Escribe las notas en español"), "{es}");
        let answered = append_answer(&es, Some("¿Por qué?"), "Porque sí.", "2026-01-01");
        assert!(answered.contains("Pregunta: ¿Por qué?\n  Respuesta: Porque sí."));
        assert_eq!(parse_answers(&answered).len(), 1);

        let inv = Inventory {
            modules: vec![module("api", 10, true)],
            ..Default::default()
        };
        let paths = PlanPaths {
            lang: Lang::En,
            ..PlanPaths::default()
        };
        let tasks = build_plan_with(&inv, &PlanSection::default(), &paths, false, &[]);
        assert!(tasks[0].objective.starts_with("Write the system overview"));
        assert!(tasks[1].produce.iter().any(|p| p.ends_with("(optional)")));
    }

    #[test]
    fn covers_module_is_exact() {
        assert!(covers_module(Some("api,db"), "db"));
        assert!(!covers_module(Some("api,db"), ""));
        assert!(!covers_module(Some("api"), "ap"));
    }

    #[test]
    fn parses_task_markdown_roundtrip() {
        let task = Task {
            task_id: "T-002".into(),
            kind: TaskType::Module,
            module: Some("src".into()),
            status: TaskStatus::Pending,
            depends_on: vec!["T-001".into()],
            read: vec!["src/**".into()],
            produce: vec!["knowledge/modules/src.md".into()],
            objective: "x".into(),
        };
        let md = task_to_markdown(&task, &TaskRules::default());
        let meta = parse_task(&md).unwrap();
        assert_eq!(meta.module.as_deref(), Some("src"));
        assert_eq!(meta.depends_on, vec!["T-001"]);
        assert_eq!(meta.produce, vec!["knowledge/modules/src.md"]);
        let done = set_status_in_markdown(&md, "done");
        assert_eq!(parse_task(&done).unwrap().status, "done");
    }
}
