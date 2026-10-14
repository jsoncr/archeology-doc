//! `projects delete <nombre|id>` — elimina un proyecto de archeology-doc.
//!
//! Por defecto: entrada del registro, espacio de trabajo
//! (`~/.archeology-doc/projects/<ws>`: estado, tareas, ejecuciones) y
//! aprobaciones (adaptadores y sitio) del proyecto. Con `-d` también los
//! archivos que adoc generó dentro del proyecto, y solo esos:
//!
//! - `archeology-doc.toml`, `knowledge/` (puede tener ediciones: se avisa);
//! - archivos con regiones gestionadas (`AGENTS.md`, `CLAUDE.md`, `llms.txt`,
//!   reglas de Cursor/Copilot, skills de `.agents/`): se borran si solo
//!   contienen lo generado; si no, se quitan las regiones y se conserva el
//!   texto del usuario;
//! - en `docs/`, las carpetas con la marca de generado (`wiki/`, `specs/`,
//!   `public/adoc-graph/`) y el sidebar generado. `docs/` completo solo si
//!   lo creó `adoc init` y no tiene páginas del usuario.
//!
//! Nunca se siguen symlinks ni se sale de la raíz del proyecto.

use crate::ui;
use anyhow::{Context, Result};
use archeology_core::fsutil::check_no_symlinks;
use archeology_core::i18n::Lang;
use archeology_core::site::{GENERATED_DOCS_MARK, GENERATED_MARK, SIDEBAR_FILE};
use archeology_core::{managed, t, tl, Project};
use archeology_registry::{Registry, RegistryEntry};
use std::fs;
use std::path::{Path, PathBuf};

/// Marca de carpeta generada con el nombre anterior del proyecto.
const LEGACY_GENERATED_MARK: &str = ".generated-by-arqueology-doc";

/// Qué se hace con una ruta del proyecto.
#[derive(Debug)]
enum Action {
    RemoveDir,
    RemoveFile,
    /// Se quitan las regiones gestionadas; queda este texto del usuario.
    Strip(String),
}

#[derive(Debug)]
struct Item {
    path: PathBuf,
    action: Action,
    bytes: u64,
    files: usize,
    /// Aviso junto al elemento (p. ej. `knowledge/` puede tener ediciones).
    warning: Option<String>,
}

/// Plan de borrado dentro del proyecto (`-d`).
#[derive(Debug, Default)]
struct FilePlan {
    items: Vec<Item>,
    /// Lo que se conserva a propósito, con el motivo.
    kept: Vec<String>,
}

pub fn run(query: &str, yes: bool, delete_files: bool) -> Result<()> {
    let registry = Registry::load()?;
    let entry = registry
        .resolve(query)?
        .cloned()
        .with_context(|| t!("projects.delete.not_found", query = query))?;
    let root = PathBuf::from(&entry.path);
    let root_exists = fs::symlink_metadata(&root).is_ok_and(|m| m.is_dir());
    let workspace = archeology_core::paths::find_workspace_by_id(&entry.id)
        .filter(|w| fs::symlink_metadata(w).is_ok_and(|m| m.is_dir()));
    let approvals = archeology_core::trust::project_entries(&root);

    // Plan de archivos del proyecto (antes de borrar el espacio de trabajo:
    // ahí está la marca de `docs/` creado por `init`).
    let mut notes = Vec::new();
    let plan = if !delete_files {
        None
    } else if !root_exists {
        notes.push(t!("projects.delete.files_skipped_missing"));
        None
    } else {
        match Project::open(&root) {
            Ok(p) if p.config.project.id != entry.id => {
                notes.push(t!(
                    "projects.delete.files_skipped_other_id",
                    id = p.config.project.id
                ));
                None
            }
            Ok(p) => Some(file_plan(&p, workspace.as_deref())?),
            Err(e) => {
                notes.push(t!("projects.delete.files_skipped_unreadable", error = e));
                None
            }
        }
    };

    print_screen(
        &entry,
        root_exists,
        workspace.as_deref(),
        approvals,
        plan.as_ref(),
        delete_files,
        &notes,
    )?;
    if !super::confirm(&t!("projects.delete.confirm"), yes)? {
        println!("{}", t!("projects.delete.cancelled"));
        return Ok(());
    }

    // Primero los archivos del proyecto; el registro, al final.
    if let Some(plan) = &plan {
        for item in &plan.items {
            // La etiqueta se calcula antes: después la ruta ya no existe.
            let label = rel(&root, &item.path);
            apply(&root, item)?;
            println!("{} {label}", ui::green("✓"));
        }
        prune_empty_dirs(&root);
    }
    if let Some(w) = &workspace {
        fs::remove_dir_all(w)?;
        println!("{} {}", ui::green("✓"), ui::tilde(w));
    }
    let removed_approvals = archeology_core::trust::forget_project(&root)?;
    if removed_approvals > 0 {
        println!(
            "{} {}",
            ui::green("✓"),
            t!("projects.delete.approvals", count = removed_approvals)
        );
    }
    Registry::update(|reg| reg.remove(&entry.id))?;
    println!(
        "{} {}",
        ui::green("✓"),
        t!("projects.delete.registry_entry")
    );
    println!(
        "{}",
        t!("projects.delete.done", name = entry.name, id = entry.id)
    );
    Ok(())
}

/// Pantalla de confirmación: qué se borra (con tamaños), qué se modifica y
/// qué se conserva.
fn print_screen(
    entry: &RegistryEntry,
    root_exists: bool,
    workspace: Option<&Path>,
    approvals: usize,
    plan: Option<&FilePlan>,
    delete_files: bool,
    notes: &[String],
) -> Result<()> {
    let root = Path::new(&entry.path);
    println!(
        "{} {} {}",
        ui::bold(&t!("projects.delete.title")),
        ui::bold(&entry.name),
        ui::dim(&format!("({})", entry.id))
    );
    let missing = if root_exists {
        String::new()
    } else {
        format!(" {}", ui::yellow(&t!("projects.delete.path_missing")))
    };
    println!("  {}{missing}", ui::tilde(root));
    println!();
    println!("{}", t!("projects.delete.will_remove"));
    let registry = archeology_registry::registry_path()?;
    line(
        "✗",
        &t!("projects.delete.registry_entry"),
        "",
        &ui::tilde(&registry),
    );
    match workspace {
        Some(w) => {
            let (bytes, files) = size_of(w);
            line(
                "✗",
                &t!("projects.delete.workspace"),
                &size_label(bytes, files),
                &ui::tilde(w),
            );
        }
        None => println!("  {}", ui::dim(&t!("projects.delete.no_workspace"))),
    }
    if approvals > 0 {
        line(
            "✗",
            &t!("projects.delete.approvals", count = approvals),
            "",
            "",
        );
    }
    if let Some(plan) = plan {
        println!();
        println!("{}", t!("projects.delete.project_files"));
        if plan.items.is_empty() {
            println!("  {}", ui::dim(&t!("projects.delete.nothing_generated")));
        }
        for item in &plan.items {
            let (mark, what) = match &item.action {
                Action::Strip(_) => ("~", t!("projects.delete.strip_regions")),
                _ => ("✗", String::new()),
            };
            let name = rel(root, &item.path);
            let size = match item.action {
                Action::RemoveDir => size_label(item.bytes, item.files),
                _ => ui::bytes(item.bytes),
            };
            line(mark, &name, &size, &what);
            if let Some(w) = &item.warning {
                println!("      {} {}", ui::yellow("!"), ui::yellow(w));
            }
        }
        if !plan.kept.is_empty() {
            println!();
            println!("{}", t!("projects.delete.kept"));
            for k in &plan.kept {
                println!("  {} {k}", ui::dim("·"));
            }
        }
    }
    for n in notes {
        println!("  {} {n}", ui::yellow("!"));
    }
    if !delete_files {
        println!();
        println!("{}", ui::dim(&t!("projects.delete.files_kept_hint")));
    }
    println!();
    Ok(())
}

/// Línea `  ✗ nombre   tamaño   detalle` con columnas alineadas.
fn line(mark: &str, name: &str, size: &str, detail: &str) {
    let painted = match mark {
        "✗" => ui::red(mark),
        "~" => ui::yellow(mark),
        _ => mark.to_string(),
    };
    let text = format!(
        "  {painted} {} {} {}",
        ui::pad(name, 40),
        ui::pad(size, 20),
        ui::dim(detail)
    );
    println!("{}", text.trim_end());
}

fn size_label(bytes: u64, files: usize) -> String {
    format!(
        "{} · {}",
        ui::bytes(bytes),
        t!("projects.delete.files_count", count = files)
    )
}

/// Ruta relativa a la raíz para mostrar (`/` final en carpetas).
fn rel(root: &Path, path: &Path) -> String {
    let r = path
        .strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string());
    if fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
        format!("{r}/")
    } else {
        r
    }
}

/// Tamaño y número de archivos sin seguir symlinks (un enlace cuenta como
/// archivo, con su propio tamaño).
fn size_of(path: &Path) -> (u64, usize) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return (0, 0);
    };
    if !meta.is_dir() {
        return (meta.len(), 1);
    }
    let mut total = (0, 0);
    if let Ok(entries) = fs::read_dir(path) {
        for e in entries.flatten() {
            let (b, f) = size_of(&e.path());
            total.0 += b;
            total.1 += f;
        }
    }
    total
}

/// `path` es una carpeta real (no un symlink) dentro de la raíz, sin
/// symlinks intermedios, y no es la raíz misma.
fn safe_dir(root: &Path, path: &Path) -> bool {
    path != root
        && check_no_symlinks(root, path).is_ok()
        && fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

/// Como [`safe_dir`], para un archivo regular.
fn safe_file(root: &Path, path: &Path) -> bool {
    check_no_symlinks(root, path).is_ok() && fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
}

fn dir_item(path: PathBuf, warning: Option<String>) -> Item {
    let (bytes, files) = size_of(&path);
    Item {
        path,
        action: Action::RemoveDir,
        bytes,
        files,
        warning,
    }
}

fn file_item(path: PathBuf) -> Item {
    let (bytes, files) = size_of(&path);
    Item {
        path,
        action: Action::RemoveFile,
        bytes,
        files,
        warning: None,
    }
}

/// Lo que adoc generó dentro del proyecto.
fn file_plan(project: &Project, workspace: Option<&Path>) -> Result<FilePlan> {
    let root = &project.root;
    let mut plan = FilePlan::default();
    let name = &project.config.project.name;

    let cfg = project.config_path();
    if safe_file(root, &cfg) {
        plan.items.push(file_item(cfg));
    }
    let knowledge = project.knowledge_dir();
    if safe_dir(root, &knowledge) {
        plan.items.push(dir_item(
            knowledge,
            Some(t!("projects.delete.knowledge_warning")),
        ));
    }

    // Archivos con regiones gestionadas y el esqueleto que adoc les pone
    // al crearlos (en cualquiera de los idiomas de la documentación).
    let skeletons =
        |f: &dyn Fn(Lang) -> String| -> Vec<String> { Lang::ALL.iter().map(|l| f(*l)).collect() };
    let mut managed_files: Vec<(&str, Vec<String>)> = vec![
        (
            "AGENTS.md",
            skeletons(&|l| format!("# {name}\n\n{}", tl!(l, "doc.agents.default_intro"))),
        ),
        ("CLAUDE.md", Vec::new()),
        (
            "llms.txt",
            skeletons(&|l| format!("# {name}\n\n> {}", tl!(l, "doc.llms.intro"))),
        ),
        (".github/copilot-instructions.md", Vec::new()),
    ];
    let cursor = skeletons(&|l| {
        format!(
            "---\ndescription: {}\nalwaysApply: true\n---",
            tl!(l, "doc.adapters.cursor_description")
        )
    });
    managed_files.push((".cursor/rules/archeology-doc.mdc", cursor.clone()));
    managed_files.push((".cursor/rules/arqueology-doc.mdc", cursor));
    for (skill, key) in [
        ("project-memory", "doc.skills.memory_description"),
        ("run-tests", "doc.skills.tests_description"),
    ] {
        let rel = match skill {
            "project-memory" => ".agents/skills/project-memory/SKILL.md",
            _ => ".agents/skills/run-tests/SKILL.md",
        };
        managed_files.push((
            rel,
            skeletons(&|l| {
                format!(
                    "---\nname: {skill}\ndescription: {}\n---\n\n# {skill}",
                    tl!(l, key)
                )
            }),
        ));
    }
    for (rel_path, skel) in &managed_files {
        managed_item(root, &root.join(rel_path), skel, &mut plan);
    }

    docs_plan(project, workspace, &mut plan);
    Ok(plan)
}

/// Archivo con regiones gestionadas: se borra si sin ellas queda vacío o
/// solo el esqueleto de adoc; si no, se quitan las regiones. Sin regiones
/// (no lo generó adoc) o con marcadores rotos, no se toca.
fn managed_item(root: &Path, path: &Path, skeletons: &[String], plan: &mut FilePlan) {
    if !safe_file(root, path) {
        return;
    }
    let Ok(Some(text)) = archeology_core::fsutil::read_to_string_nofollow(path) else {
        return;
    };
    if !text.lines().any(|l| managed::is_marker(l, "begin ")) {
        return;
    }
    let shown = rel(root, path);
    let Some(rest) = managed::strip_regions(&text) else {
        plan.kept
            .push(t!("projects.delete.kept_unbalanced", path = shown));
        return;
    };
    let rest_trim = rest.trim();
    if rest_trim.is_empty() || skeletons.iter().any(|s| s.trim() == rest_trim) {
        plan.items.push(file_item(path.to_path_buf()));
    } else {
        plan.items.push(Item {
            path: path.to_path_buf(),
            bytes: text.len() as u64,
            files: 1,
            action: Action::Strip(rest),
            warning: None,
        });
    }
}

/// `docs/`: carpetas marcadas como generadas y sidebar; completo solo si lo
/// creó `init` y no hay páginas del usuario.
fn docs_plan(project: &Project, workspace: Option<&Path>, plan: &mut FilePlan) {
    let root = &project.root;
    let docs = project.docs_dir();
    if !safe_dir(root, &docs) {
        return;
    }
    let marked = |dir: &Path| {
        safe_dir(root, dir)
            && [GENERATED_MARK, LEGACY_GENERATED_MARK].iter().any(|m| {
                fs::symlink_metadata(dir.join(m)).is_ok_and(|meta| meta.file_type().is_file())
            })
    };
    let generated_dirs: Vec<PathBuf> = [
        archeology_core::site::WIKI_DIR,
        archeology_core::site::SPECS_DIR,
        archeology_core::graph_web::SITE_ASSETS_DIR,
    ]
    .iter()
    .map(|d| docs.join(d))
    .filter(|d| marked(d))
    .collect();
    let sidebar = docs.join(".vitepress").join(SIDEBAR_FILE);

    let from_init = workspace.is_some_and(|w| w.join(GENERATED_DOCS_MARK).is_file());
    if from_init {
        let mut user_files = Vec::new();
        let skip: Vec<PathBuf> = generated_dirs
            .iter()
            .cloned()
            .chain([
                docs.join(".vitepress").join("cache"),
                docs.join(".vitepress").join("dist"),
            ])
            .collect();
        collect_user_files(&docs, &docs, &skip, &mut user_files);
        if user_files.is_empty() {
            plan.items
                .push(dir_item(docs, Some(t!("projects.delete.docs_from_init"))));
            return;
        }
        plan.kept.push(t!(
            "projects.delete.docs_user_pages",
            path = rel(root, &docs),
            count = user_files.len()
        ));
        // Páginas del dashboard (`adoc site sync`) en un `docs/` de `init`.
        for page in ["estado.md", "hallazgos.md", "preguntas.md"]
            .iter()
            .map(|p| p.to_string())
            .chain([format!("{}.md", archeology_core::graph_web::SITE_PAGE)])
        {
            let path = docs.join(page);
            if safe_file(root, &path) {
                plan.items.push(file_item(path));
            }
        }
    } else {
        plan.kept
            .push(t!("projects.delete.docs_not_ours", path = rel(root, &docs)));
    }
    for d in generated_dirs {
        plan.items.push(dir_item(d, None));
    }
    if safe_file(root, &sidebar) {
        plan.items.push(file_item(sidebar));
    }
    // Portada: solo la región `dashboard`; el resto es del usuario.
    managed_item(root, &docs.join("index.md"), &[], plan);
}

/// Archivos de `docs/` que no generó adoc (rutas relativas). Los symlinks
/// cuentan como del usuario y no se siguen.
fn collect_user_files(docs: &Path, dir: &Path, skip: &[PathBuf], out: &mut Vec<String>) {
    const GENERATED: &[&str] = &[
        "index.md",
        "estado.md",
        "hallazgos.md",
        "preguntas.md",
        "grafo.md",
        ".gitignore",
        ".vitepress/config.mts",
    ];
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if skip.contains(&path) {
            continue;
        }
        let rel = path
            .strip_prefix(docs)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        match e.file_type() {
            Ok(t) if t.is_dir() => collect_user_files(docs, &path, skip, out),
            _ if GENERATED.contains(&rel.as_str()) || rel.ends_with(SIDEBAR_FILE) => {}
            _ => out.push(rel),
        }
    }
}

/// Aplica un elemento del plan, comprobando de nuevo que sigue dentro de la
/// raíz y sin symlinks (pudo cambiar mientras se confirmaba).
fn apply(root: &Path, item: &Item) -> Result<()> {
    let path = &item.path;
    match &item.action {
        Action::RemoveDir => {
            anyhow::ensure!(
                safe_dir(root, path),
                t!("projects.delete.unsafe", path = path.display())
            );
            fs::remove_dir_all(path)?;
        }
        Action::RemoveFile => {
            anyhow::ensure!(
                safe_file(root, path),
                t!("projects.delete.unsafe", path = path.display())
            );
            fs::remove_file(path)?;
        }
        Action::Strip(rest) => {
            archeology_core::fsutil::write_in_root(root, path, rest.as_bytes())?;
        }
    }
    Ok(())
}

/// Carpetas que quedan vacías tras borrar skills y adaptadores.
fn prune_empty_dirs(root: &Path) {
    for d in [
        ".agents/skills/project-memory",
        ".agents/skills/run-tests",
        ".agents/skills",
        ".agents",
        ".cursor/rules",
        ".cursor",
        ".github",
    ] {
        let path = root.join(d);
        if safe_dir(root, &path) {
            // `remove_dir` solo borra carpetas vacías.
            let _ = fs::remove_dir(&path);
        }
    }
}
