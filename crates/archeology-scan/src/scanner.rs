//! Recorrido del repositorio y construcción del inventario (RF-010 a RF-022).

use crate::detect;
use crate::git;
use anyhow::Result;
use archeology_core::frontmatter::looks_like_secret_path;
use archeology_core::model::{GitStats, Inventory, Marker, ModuleInfo, OpenSpecInventory};
use archeology_core::Project;
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub struct ScanOptions {
    pub git_history: bool,
    pub max_file_size_kb: u64,
    /// Callback de progreso para la interfaz (spinner); opcional.
    pub on_progress: Option<ScanCallback>,
}

/// Callback de progreso del escaneo.
pub type ScanCallback = Box<dyn Fn(ScanEvent)>;

/// Progreso del escaneo, para mostrarlo en la terminal.
#[derive(Debug, Clone, Copy)]
pub enum ScanEvent<'a> {
    /// Archivos recorridos hasta ahora y el último visitado.
    Files(usize, &'a str),
    /// Fase en curso (historial git, OpenSpec, módulos), en el idioma de la interfaz.
    Phase(&'a str),
}

/// Cada cuántos archivos se notifica el progreso.
const PROGRESS_EVERY: usize = 50;

const MAX_MARKER_FILE_BYTES: u64 = 256 * 1024;
const MAX_PEEK_BYTES: usize = 64 * 1024;
const MARKER_WORDS: &[&str] = &["TODO", "FIXME", "HACK", "XXX", "DEPRECATED"];
pub use archeology_core::site::GENERATED_DOCS_MARK;

/// Ejecuta el inventario completo del proyecto.
pub fn scan_project(project: &Project, opts: &ScanOptions) -> Result<Inventory> {
    let emit = |e: ScanEvent| {
        if let Some(f) = &opts.on_progress {
            f(e)
        }
    };
    let root = &project.root;
    let cfg = &project.config;

    let mut overrides = OverrideBuilder::new(root);
    for pat in &cfg.paths.exclude {
        // `ignore` trata `!` como whitelist; los excludes van negados.
        overrides.add(&format!("!{pat}"))?;
    }
    for pat in &cfg.scan.secrets_patterns {
        overrides.add(&format!("!{pat}"))?;
        overrides.add(&format!("!**/{pat}"))?;
    }
    // Nunca escaneamos nuestra propia salida ni metadatos de VCS.
    let knowledge = cfg.paths.knowledge.trim_end_matches('/');
    let docs = cfg.paths.docs.trim_end_matches('/');
    let openspec = cfg.openspec.path.trim_end_matches('/');
    // Estado antiguo dentro del proyecto, con ambas grafías.
    for legacy in archeology_core::config::LEGACY_STATE_DIRS {
        overrides.add(&format!("!{legacy}/"))?;
    }
    for internal in [
        ".git/".to_string(),
        format!("/{knowledge}/"),
        format!("/{docs}/.vitepress/"),
        // Copia del vault que genera `site sync`.
        format!("/{docs}/{}/", archeology_core::site::WIKI_DIR),
        // Recursos del grafo que genera `site sync` (graph.js, graph.json).
        format!("/{docs}/{}/", archeology_core::graph_web::SITE_ASSETS_DIR),
        // las specs se parsean por el spec bridge, no como código
        format!("/{openspec}/"),
        "/AGENTS.md".to_string(),
        // Contexto para agentes generado por archeology-doc.
        "/.agents/".to_string(),
        "/.cursor/rules/archeology-doc.mdc".to_string(),
        "/.cursor/rules/arqueology-doc.mdc".to_string(),
        "/llms.txt".to_string(),
        "/archeology-doc.toml".to_string(),
        "/arqueology-doc.toml".to_string(),
    ] {
        overrides.add(&format!("!{internal}"))?;
    }
    // El sitio creado por `init` no es código del proyecto; si `docs/`
    // existía antes de `init`, se sigue escaneando (RF-006).
    if project.state_dir().join(GENERATED_DOCS_MARK).is_file() {
        overrides.add(&format!("!/{docs}/"))?;
    }
    let overrides = overrides.build()?;

    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .require_git(false)
        .overrides(overrides);

    let mut inv = Inventory {
        version: 1,
        generated_at: chrono::Utc::now().to_rfc3339(),
        project_root: root.display().to_string(),
        ..Default::default()
    };

    let mut file_recs: Vec<FileRec> = Vec::new();

    for entry in builder.build().flatten() {
        let path = entry.path();
        // Los symlinks no se siguen: podrían apuntar fuera del repositorio o
        // a un archivo enorme que se salte `max_file_size_kb` (el tamaño de
        // `entry.metadata()` es el del enlace, no el del destino).
        if entry.path_is_symlink() || !path.is_file() {
            continue;
        }
        let rel = project.rel(path);
        if inv.total_files.is_multiple_of(PROGRESS_EVERY) {
            emit(ScanEvent::Files(inv.total_files, &rel));
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let size = meta.len();
        inv.total_files += 1;
        inv.total_bytes += size;

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // Doble verificación de secretos por si el override no aplica.
        if looks_like_secret_path(&rel, &cfg.scan.secrets_patterns) {
            continue;
        }

        if let Some(lang) = detect::language_for(&rel) {
            let stats = inv.languages.entry(lang.to_string()).or_default();
            stats.files += 1;
            stats.bytes += size;
            if size <= cfg.scan.max_file_size_kb * 1024 {
                stats.lines += count_lines(path);
            }
        }

        if let Some((tool, _)) = detect::manifest_for(&file_name) {
            inv.manifests.push(format!("{rel} ({tool})"));
            let peek = peek(path);
            for fw in detect::framework_hints(&rel, peek.as_deref()) {
                if !inv.frameworks.contains(&fw.to_string()) {
                    inv.frameworks.push(fw.to_string());
                }
            }
        }

        if detect::is_entrypoint(&rel) {
            inv.entrypoints.push(rel.clone());
        }
        if detect::is_test_file(&rel) {
            inv.test_files.push(rel.clone());
            if let Some(fw) = detect::test_framework_for(&rel) {
                if !inv.test_frameworks.contains(&fw.to_string()) {
                    inv.test_frameworks.push(fw.to_string());
                }
            }
            if size <= MAX_MARKER_FILE_BYTES {
                if let Ok(text) = fs::read_to_string(path) {
                    if has_disabled_test(&text) {
                        inv.disabled_tests.push(rel.clone());
                    }
                }
            }
        }
        if detect::is_ci_file(&rel) {
            inv.ci_files.push(rel.clone());
        }
        if detect::is_doc_file(&rel) {
            inv.doc_files.push(rel.clone());
        }

        // Marcadores TODO/FIXME (RF-018).
        if size <= MAX_MARKER_FILE_BYTES && detect::language_for(&rel).is_some() {
            if let Ok(text) = fs::read_to_string(path) {
                for (i, line) in text.lines().enumerate() {
                    for word in MARKER_WORDS {
                        if has_marker(line, word) {
                            inv.markers.push(Marker {
                                kind: word.to_string(),
                                path: rel.clone(),
                                line: (i + 1) as u32,
                                text: line.trim().chars().take(160).collect(),
                            });
                            break;
                        }
                    }
                }
            }
        }

        file_recs.push(FileRec {
            path: rel.clone(),
            bytes: size,
            language: detect::language_for(&rel),
            is_test: detect::is_test_file(&rel),
        });
    }

    // Historial git.
    if opts.git_history {
        emit(ScanEvent::Phase(&archeology_core::t!(
            "scan.phase.git_history"
        )));
        inv.git = git::analyze(root);
    } else {
        inv.git = Option::<GitStats>::None;
    }

    // OpenSpec.
    emit(ScanEvent::Phase(&archeology_core::t!(
        "scan.phase.openspec"
    )));
    inv.openspec = detect_openspec(project);
    emit(ScanEvent::Phase(&archeology_core::t!("scan.phase.modules")));

    // Módulos acotados por los límites del plan (RNF-02).
    inv.modules = build_modules(
        &file_recs,
        cfg.plan.max_files_per_task,
        cfg.plan.max_bytes_per_task,
    );
    inv.modules.sort_by_key(|m| std::cmp::Reverse(m.bytes));

    Ok(inv)
}

/// Archivo inventariado, para agrupar en módulos.
struct FileRec {
    path: String,
    bytes: u64,
    language: Option<&'static str>,
    is_test: bool,
}

/// Profundidad máxima al subdividir directorios grandes.
const MAX_SPLIT_DEPTH: usize = 6;

/// Agrupa archivos en módulos que respetan `max_files` y `max_bytes`.
///
/// Cada directorio de primer nivel es un módulo; si excede los límites se
/// divide en sus subdirectorios, y sus archivos directos forman un módulo
/// no recursivo. Los archivos de código en la raíz forman el módulo `root`.
fn build_modules(files: &[FileRec], max_files: usize, max_bytes: u64) -> Vec<ModuleInfo> {
    // Solo archivos de texto reconocidos: imágenes, videos y binarios no
    // aportan contexto al agente ni deben inflar el tamaño de un módulo.
    // Las carpetas ocultas (.devin, .claude, .vscode, .github…) son
    // configuración de herramientas, no módulos del sistema.
    let refs: Vec<&FileRec> = files
        .iter()
        .filter(|f| f.language.is_some())
        .filter(|f| !f.path.split('/').any(|c| c.starts_with('.')))
        .collect();
    let mut out = Vec::new();
    split_dir("", &refs, max_files.max(1), max_bytes.max(1), 0, &mut out);
    dedupe_module_names(&mut out);
    out
}

/// Nombres de módulo únicos y deterministas. `src/api` y `src-api` darían
/// ambos `src-api`, y una carpeta llamada `root` chocaría con el módulo de
/// la raíz. Conservan el nombre, por orden: el módulo de la raíz, los que
/// se llaman igual que su ruta y el resto por ruta; los demás reciben un
/// sufijo `-2`, `-3`… Las comas se sustituyen: el campo `module` de las
/// tareas separa módulos con comas.
fn dedupe_module_names(modules: &mut [ModuleInfo]) {
    for m in modules.iter_mut() {
        if m.name.contains(',') {
            m.name = m.name.replace(',', "_");
        }
    }
    let mut order: Vec<usize> = (0..modules.len()).collect();
    order.sort_by_key(|&i| {
        let m = &modules[i];
        let rank = if m.path == "." {
            0
        } else if m.name == m.path {
            1
        } else {
            2
        };
        (rank, m.path.clone())
    });
    let mut used = std::collections::BTreeSet::new();
    for i in order {
        let base = modules[i].name.clone();
        let mut name = base.clone();
        let mut n = 2;
        while used.contains(&name) {
            name = format!("{base}-{n}");
            n += 1;
        }
        used.insert(name.clone());
        modules[i].name = name;
    }
}

fn split_dir(
    dir: &str,
    files: &[&FileRec],
    max_files: usize,
    max_bytes: u64,
    depth: usize,
    out: &mut Vec<ModuleInfo>,
) {
    let bytes: u64 = files.iter().map(|f| f.bytes).sum();
    let fits = files.len() <= max_files && bytes <= max_bytes;
    if !dir.is_empty() && (fits || depth >= MAX_SPLIT_DEPTH) {
        if files
            .iter()
            .any(|f| f.language.map(is_code_language).unwrap_or(false))
        {
            out.push(module_info(dir, files, true));
        }
        return;
    }

    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let mut direct: Vec<&FileRec> = Vec::new();
    let mut subdirs: BTreeMap<String, Vec<&FileRec>> = BTreeMap::new();
    for f in files {
        let rest = &f.path[prefix.len()..];
        match rest.split_once('/') {
            Some((sub, _)) => subdirs
                .entry(format!("{prefix}{sub}"))
                .or_default()
                .push(*f),
            None => direct.push(*f),
        }
    }

    // Un grupo de archivos es módulo solo si contiene código (no solo docs/config).
    let has_code = direct
        .iter()
        .any(|f| f.language.map(is_code_language).unwrap_or(false));
    if has_code {
        out.push(module_info(dir, &direct, false));
    }
    for (sub, sub_files) in subdirs {
        split_dir(&sub, &sub_files, max_files, max_bytes, depth + 1, out);
    }
}

fn module_info(dir: &str, files: &[&FileRec], recursive: bool) -> ModuleInfo {
    let mut langs: BTreeMap<&str, usize> = BTreeMap::new();
    for f in files {
        if let Some(l) = f.language {
            *langs.entry(l).or_insert(0) += 1;
        }
    }
    let mut langs: Vec<_> = langs.into_iter().collect();
    langs.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    ModuleInfo {
        name: if dir.is_empty() {
            "root".into()
        } else {
            dir.replace('/', "-")
        },
        path: if dir.is_empty() {
            ".".into()
        } else {
            dir.into()
        },
        files: files.len(),
        bytes: files.iter().map(|f| f.bytes).sum(),
        languages: langs.into_iter().map(|(l, _)| l.to_string()).collect(),
        has_tests: files.iter().any(|f| f.is_test),
        recursive,
        code_files: files
            .iter()
            .filter(|f| f.language.map(is_code_language).unwrap_or(false))
            .count(),
    }
}

fn is_code_language(lang: &str) -> bool {
    !matches!(
        lang,
        "Markdown" | "JSON" | "YAML" | "TOML" | "XML" | "HTML" | "CSS"
    )
}

/// Busca el marcador como palabra completa. `deprecated` se acepta en
/// cualquier capitalización; el resto solo en mayúsculas para evitar ruido.
fn has_marker(line: &str, word: &str) -> bool {
    let (hay, needle) = if word == "DEPRECATED" {
        (line.to_ascii_uppercase(), word)
    } else {
        (line.to_string(), word)
    };
    let bytes = hay.as_bytes();
    let mut start = 0;
    while let Some(pos) = hay[start..].find(needle) {
        let i = start + pos;
        let end = i + needle.len();
        let before_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        let after_ok = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        start = end;
    }
    false
}

fn count_lines(path: &Path) -> u64 {
    fs::read(path)
        .map(|b| b.iter().filter(|&&c| c == b'\n').count() as u64 + 1)
        .unwrap_or(0)
}

fn peek(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut f = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; MAX_PEEK_BYTES];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    String::from_utf8(buf).ok()
}

/// Detecta tests desactivados: skip/it.skip/describe.skip/@Ignore/disabled.
fn has_disabled_test(text: &str) -> bool {
    let patterns = [
        ".skip(",
        "xit(",
        "xdescribe(",
        "@pytest.mark.skip",
        "@unittest.skip",
        "@Ignore",
        "#[ignore]",
        "@Disabled",
        "test.skip",
        "describe.skip",
        "it.only(",
    ];
    patterns.iter().any(|p| text.contains(p))
}

/// Detección de OpenSpec durante el scan (RF-150).
fn detect_openspec(project: &Project) -> Option<OpenSpecInventory> {
    let dir = project.config.openspec_dir(&project.root);
    if !dir.is_dir() {
        return None;
    }
    let capabilities = list_dirs(&dir.join("specs"));
    let changes_dir = dir.join("changes");
    let mut active = Vec::new();
    let mut archived = Vec::new();
    if changes_dir.is_dir() {
        for e in fs::read_dir(&changes_dir).ok()?.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if name == "archive" {
                archived = list_dirs(&p);
            } else {
                active.push(name);
            }
        }
    }
    if capabilities.is_empty() && active.is_empty() && archived.is_empty() {
        return None;
    }
    Some(OpenSpecInventory {
        path: project.rel(&dir),
        capabilities,
        changes_active: active,
        changes_archived: archived,
    })
}

fn list_dirs(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(path: &str, bytes: u64) -> FileRec {
        FileRec {
            path: path.into(),
            bytes,
            language: detect::language_for(path),
            is_test: detect::is_test_file(path),
        }
    }

    #[test]
    fn splits_large_modules_and_keeps_root_code() {
        let files = vec![
            rec("main.py", 10),
            rec("README.md", 10),
            rec("src/a.py", 10),
            rec("src/b.py", 10),
            rec("src/api/c.py", 10),
            rec("src/api/d.py", 10),
            rec("lib/x.py", 10),
        ];
        let mods = build_modules(&files, 2, 1_000);
        let names: Vec<_> = mods.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["root", "lib", "src", "src-api"]);
        let src = mods.iter().find(|m| m.name == "src").unwrap();
        assert!(!src.recursive);
        assert_eq!(src.files, 2);
        assert!(mods.iter().find(|m| m.name == "lib").unwrap().recursive);
    }

    #[test]
    fn module_names_are_unique() {
        let files = vec![
            rec("main.py", 10),
            rec("root/x.py", 10),
            rec("src/api/a.py", 10),
            rec("src/b.py", 10),
            rec("src-api/c.py", 10),
            rec("a,b/d.py", 10),
        ];
        let mods = build_modules(&files, 1, 1_000);
        let mut names: Vec<(String, String)> = mods
            .iter()
            .map(|m| (m.path.clone(), m.name.clone()))
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                (".".to_string(), "root".to_string()),
                ("a,b".to_string(), "a_b".to_string()),
                ("root".to_string(), "root-2".to_string()),
                ("src".to_string(), "src".to_string()),
                ("src-api".to_string(), "src-api".to_string()),
                ("src/api".to_string(), "src-api-2".to_string()),
            ]
        );
    }

    #[test]
    fn binaries_and_hidden_dirs_do_not_make_modules() {
        let files = vec![
            rec("src/app.js", 10),
            rec("src/public/videos/intro.mp4", 50_000_000),
            rec("src/public/images/logo.png", 3_000_000),
            rec(".devin/workflows/x.md", 10),
            rec("docs/guia.md", 10),
        ];
        let mods = build_modules(&files, 40, 400_000);
        let names: Vec<_> = mods.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["src"]);
        assert_eq!(mods[0].code_files, 1);
    }

    #[test]
    fn marker_matches_whole_words_only() {
        assert!(has_marker("# TODO: fix", "TODO"));
        assert!(has_marker("pass  # deprecated", "DEPRECATED"));
        assert!(!has_marker("let todos = TODOS;", "TODO"));
        assert!(!has_marker("XXXL size", "XXX"));
    }
}
