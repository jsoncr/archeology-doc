//! Validaciones del comando `check` (RF-130, RF-131).

use crate::error::Result;
use crate::frontmatter::{validate_note, IssueKind, IssueSeverity, NoteIssue};
use crate::project::Project;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Default)]
pub struct CheckReport {
    pub errors: Vec<NoteIssue>,
    pub warnings: Vec<NoteIssue>,
    /// Notas validadas (frontmatter).
    pub notes_checked: usize,
    /// Evidencias `sources:` revisadas.
    pub sources_checked: usize,
    /// Wikilinks encontrados en el vault.
    pub wikilinks_checked: usize,
    /// Evidencias fuera de rango que `check --fix` puede ajustar.
    pub repairable: Vec<EvidenceRepair>,
}

impl CheckReport {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn push(&mut self, issue: NoteIssue) {
        match issue.severity {
            IssueSeverity::Error => self.errors.push(issue),
            IssueSeverity::Warning => self.warnings.push(issue),
        }
    }
}

/// Ejecuta todas las validaciones sobre el proyecto.
pub fn run_checks(project: &Project, strict: bool) -> Result<CheckReport> {
    let mut report = CheckReport::default();
    let knowledge = project.knowledge_dir();

    if knowledge.is_dir() {
        validate_notes_dir(&knowledge, &project.root, &mut report)?;
    }

    check_wikilinks(&knowledge, &project.root, &mut report);
    check_duplicates(project, &mut report);

    // Notas marcadas `status: stale` por `sync` (RF-121).
    for note in &crate::index::stale_notes(project) {
        report.push(NoteIssue {
            path: note.clone(),
            severity: if strict {
                IssueSeverity::Error
            } else {
                IssueSeverity::Warning
            },
            message: crate::t!("core.validate.stale_note"),
            kind: IssueKind::Stale,
            line: None,
        });
    }

    Ok(report)
}

fn validate_notes_dir(dir: &Path, root: &Path, report: &mut CheckReport) -> Result<()> {
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(dir)?.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        // Tipo de la entrada sin seguir symlinks: un enlace a un directorio
        // padre sería un ciclo y uno externo leería fuera del vault.
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if path.file_name().map(|n| n == "_templates").unwrap_or(false) {
                continue;
            }
            validate_notes_dir(&path, root, report)?;
        } else if kind.is_file() && path.extension().map(|e| e == "md").unwrap_or(false) {
            match fs::read_to_string(&path) {
                Ok(text) => {
                    report.notes_checked += 1;
                    report.sources_checked += crate::frontmatter::NoteFrontmatter::parse(&text)
                        .map(|fm| fm.sources.len())
                        .unwrap_or(0);
                    // Mensajes con la ruta relativa a la raíz del proyecto.
                    let rel = rel_path(root, &path);
                    for issue in validate_note(Path::new(&rel), &text, root) {
                        report.push(issue);
                    }
                    report
                        .repairable
                        .extend(plan_evidence_repairs(root, &rel, &text));
                }
                // Una nota ilegible (p. ej. no UTF-8) es un problema de esa
                // nota, no un motivo para abortar todo el `check`.
                Err(e) => report.push(NoteIssue {
                    path: rel_path(root, &path),
                    severity: IssueSeverity::Error,
                    message: if e.kind() == std::io::ErrorKind::InvalidData {
                        crate::t!("core.validate.note_not_utf8")
                    } else {
                        crate::t!("core.validate.note_unreadable", error = e)
                    },
                    kind: IssueKind::Unreadable,
                    line: None,
                }),
            }
        }
    }
    Ok(())
}

/// Detecta wikilinks `[[...]]` que no resuelven a ninguna nota del vault.
fn check_wikilinks(knowledge: &Path, root: &Path, report: &mut CheckReport) {
    if !knowledge.is_dir() {
        return;
    }
    let mut stems: Vec<String> = Vec::new();
    collect_stems(knowledge, knowledge, &mut stems);
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    collect_md(knowledge, &mut files);

    for file in files {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        // Por línea: así cada enlace roto indica dónde está.
        for (lineno, cap) in text
            .lines()
            .enumerate()
            .flat_map(|(n, l)| extract_wikilinks(l).into_iter().map(move |c| (n + 1, c)))
        {
            report.wikilinks_checked += 1;
            let target = cap.split('|').next().unwrap_or(&cap).trim();
            let normalized = target.trim_end_matches(".md");
            let found = stems.iter().any(|s| {
                s == normalized || s.ends_with(&format!("/{normalized}")) || {
                    // coincidencia por nombre de archivo
                    Path::new(s)
                        .file_stem()
                        .map(|f| f.to_string_lossy() == normalized)
                        .unwrap_or(false)
                }
            });
            if !found {
                report.push(NoteIssue {
                    path: rel_path(root, &file),
                    severity: IssueSeverity::Warning,
                    message: crate::t!("core.validate.broken_wikilink", target = target),
                    kind: IssueKind::Wikilink,
                    line: Some(lineno),
                });
            }
        }
    }
}

fn collect_stems(dir: &Path, base: &Path, out: &mut Vec<String>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            // Sin seguir symlinks (ciclos, rutas fuera del vault).
            let kind = e.file_type().ok();
            if kind.is_some_and(|t| t.is_dir()) {
                collect_stems(&p, base, out);
            } else if kind.is_some_and(|t| t.is_file())
                && p.extension().map(|x| x == "md").unwrap_or(false)
            {
                let rel = p.strip_prefix(base).unwrap_or(&p).with_extension("");
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

fn collect_md(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            // Sin seguir symlinks: evita ciclos y leer fuera del árbol.
            let kind = e.file_type().ok();
            if kind.is_some_and(|t| t.is_dir()) {
                collect_md(&p, out);
            } else if kind.is_some_and(|t| t.is_file())
                && p.extension().map(|x| x == "md").unwrap_or(false)
            {
                out.push(p);
            }
        }
    }
}

/// IDs repetidos (error, §11.1) y hallazgos equivalentes (aviso, RF-086):
/// mismo `kind` y mismas evidencias.
fn check_duplicates(project: &Project, report: &mut CheckReport) {
    use std::collections::BTreeMap;
    let mut ids: BTreeMap<String, String> = BTreeMap::new();
    let mut findings: BTreeMap<(String, Vec<String>), String> = BTreeMap::new();
    for note in crate::index::vault_notes(project) {
        let Some(fm) = fs::read_to_string(&note)
            .ok()
            .and_then(|t| crate::frontmatter::NoteFrontmatter::parse(&t))
        else {
            continue;
        };
        let rel = project.rel(&note);
        if let Some(id) = fm.id.clone().filter(|i| !i.is_empty()) {
            if let Some(first) = ids.get(&id) {
                report.push(NoteIssue {
                    path: rel.clone(),
                    severity: IssueSeverity::Error,
                    message: crate::t!("core.validate.duplicate_id", id = id, first = first),
                    kind: IssueKind::Duplicate,
                    line: None,
                });
            } else {
                ids.insert(id, rel.clone());
            }
        }
        if fm.note_type.as_deref() == Some("finding") && !fm.sources.is_empty() {
            let state = fm.get("state").and_then(|v| v.as_str()).unwrap_or("open");
            if state == "dismissed" {
                continue;
            }
            let kind = fm
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let mut sources = fm.sources.clone();
            sources.sort();
            match findings.get(&(kind.clone(), sources.clone())) {
                Some(first) => report.push(NoteIssue {
                    path: rel.clone(),
                    severity: IssueSeverity::Warning,
                    message: crate::t!("core.validate.duplicate_finding", first = first),
                    kind: IssueKind::Duplicate,
                    line: None,
                }),
                None => {
                    findings.insert((kind, sources), rel);
                }
            }
        }
    }
}

/// Tipo de problema de una salida declarada en `# Produce`: agrupa los
/// mensajes de `inspect` y la retroalimentación del siguiente intento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutputProblemKind {
    /// El archivo no existe.
    Missing,
    /// Carpeta de salida sin ninguna nota nueva ni previa.
    NoNotes,
    /// Nota desactualizada (`status: stale`) que la tarea no tocó.
    StaleNotReverified,
    /// Nota editada que sigue marcada `status: stale`.
    StillStale,
    /// Nota que sigue siendo la plantilla o el esqueleto de `scan`.
    Template,
    /// Frontmatter o evidencias inválidas.
    Invalid,
}

/// Problema (error) de una salida de la tarea.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutputProblem {
    pub kind: OutputProblemKind,
    /// Ruta relativa a la raíz del proyecto.
    pub path: String,
    /// Detalle (mensaje de validación de la nota), si lo hay.
    pub detail: Option<String>,
}

/// Resultado de validar las salidas de una tarea.
#[derive(Debug, Default)]
pub struct OutputCheck {
    /// Todos los problemas (errores y avisos), para logs y `task done`.
    pub issues: Vec<NoteIssue>,
    /// Errores clasificados por tipo.
    pub problems: Vec<OutputProblem>,
    /// Notas desactualizadas en carpetas de salida que siguen sin re-verificar
    /// (informativo: una carpeta puede ser compartida por varias tareas).
    pub stale_in_dirs: Vec<String>,
    /// Evidencias ajustadas automáticamente (rango que se pasaba del final
    /// del archivo): avisos, no fallos. También figuran en `issues`.
    pub repairs: Vec<EvidenceRepair>,
}

impl OutputCheck {
    fn error(&mut self, kind: OutputProblemKind, path: &str, message: String) {
        self.issues.push(NoteIssue {
            path: path.to_string(),
            severity: IssueSeverity::Error,
            message,
            kind: IssueKind::Output,
            line: None,
        });
        self.problems.push(OutputProblem {
            kind,
            path: path.to_string(),
            detail: None,
        });
    }
}

/// ¿Nota marcada como desactualizada? (`status: stale` o `stale: true`).
pub fn is_stale(fm: &crate::frontmatter::NoteFrontmatter) -> bool {
    fm.status.as_deref() == Some("stale")
        || matches!(fm.get("stale"), Some(serde_yaml::Value::Bool(true)))
}

/// ¿Sigue siendo la plantilla? Marcadores `{{ … }}` sin rellenar, o una nota
/// sin `sources` cuyo cuerpo (fuera de las regiones gestionadas) solo tiene
/// títulos y citas, como el esqueleto que escribe `scan` en el overview.
pub fn is_template(text: &str) -> bool {
    if text.contains("{{ ") && text.contains(" }}") {
        return true;
    }
    let Some(fm) = crate::frontmatter::NoteFrontmatter::parse(text) else {
        return false;
    };
    if !fm.sources.is_empty() {
        return false;
    }
    let body = crate::frontmatter::frontmatter_range(text)
        .map(|(_, start)| &text[start..])
        .unwrap_or(text);
    let mut in_region = false;
    let mut content = false;
    for line in body.lines() {
        let l = line.trim();
        if l.starts_with("<!--") && l.contains("-doc:begin ") {
            in_region = true;
            continue;
        }
        if l.starts_with("<!--") && l.contains("-doc:end ") {
            in_region = false;
            continue;
        }
        if in_region || l.is_empty() || l.starts_with('#') || l.starts_with('>') {
            continue;
        }
        content = true;
        break;
    }
    !content
}

/// Valida los artefactos que una tarea declara en `# Produce` (§14.2, RF-050).
///
/// Ver [`check_task_output`]; devuelve solo los problemas.
pub fn validate_task_output(
    project: &Project,
    produce: &[String],
    optional: &[String],
    since: Option<SystemTime>,
) -> Vec<NoteIssue> {
    check_task_output(project, produce, optional, since).issues
}

/// Valida las salidas de una tarea (§14.2, RF-050).
///
/// Archivos:
/// - no existe → error (salvo `(opcional)`);
/// - existe y no está desactualizado → válido aunque la tarea no lo tocara
///   (ya se documentó antes), si su frontmatter es válido;
/// - desactualizado (`status: stale`) → error: hay que corregirlo o
///   re-verificarlo (actualizar `updated` y quitar `status: stale`);
/// - sin tocar desde `since` y todavía la plantilla → error.
///
/// Carpetas (terminadas en `/`): al menos una nota modificada desde `since`,
/// o, si no se tocó ninguna, que ya tenga notas y ninguna desactualizada.
/// Las notas producidas dentro del vault se validan con [`validate_note`].
pub fn check_task_output(
    project: &Project,
    produce: &[String],
    optional: &[String],
    since: Option<SystemTime>,
) -> OutputCheck {
    let mut out = OutputCheck::default();
    let knowledge = project.knowledge_dir();
    let modified_since = |p: &Path| match since {
        None => true,
        Some(t) => fs::metadata(p)
            .and_then(|m| m.modified())
            .map(|m| m >= t)
            .unwrap_or(false),
    };
    let read_fm = |p: &Path| {
        fs::read_to_string(p)
            .ok()
            .map(|t| (crate::frontmatter::NoteFrontmatter::parse(&t), t))
    };

    for item in produce {
        let target = project.root.join(item.trim_end_matches('/'));
        let mut produced: Vec<PathBuf> = Vec::new();
        let is_optional = optional.iter().any(|o| o == item);
        if item.ends_with('/') {
            let mut files = Vec::new();
            collect_md(&target, &mut files);
            // Las notas índice (MOC) las escribe archeology-doc, no el agente.
            files.retain(|f| {
                f.file_name()
                    .map(|n| n != crate::index::MOC_FILE)
                    .unwrap_or(true)
            });
            files.sort();
            let stale: Vec<String> = files
                .iter()
                .filter(|f| {
                    read_fm(f)
                        .and_then(|(fm, _)| fm)
                        .map(|fm| is_stale(&fm))
                        .unwrap_or(false)
                })
                .map(|f| project.rel(f))
                .collect();
            produced.extend(files.iter().filter(|f| modified_since(f)).cloned());
            if produced.is_empty() && !is_optional {
                // Sin cambios: vale si la carpeta ya estaba documentada y
                // nada en ella quedó desactualizado.
                if files.is_empty() || !stale.is_empty() {
                    out.error(
                        OutputProblemKind::NoNotes,
                        item,
                        crate::t!("core.validate.no_notes_produced"),
                    );
                }
            }
            out.stale_in_dirs.extend(stale);
        } else if !target.is_file() {
            if is_optional {
                continue;
            }
            out.error(
                OutputProblemKind::Missing,
                item,
                crate::t!("core.validate.expected_file_missing"),
            );
        } else {
            let touched = modified_since(&target);
            let parsed = read_fm(&target);
            let fm = parsed.as_ref().and_then(|(fm, _)| fm.as_ref());
            if fm.is_some_and(is_stale) {
                if touched && since.is_some() {
                    out.error(
                        OutputProblemKind::StillStale,
                        item,
                        crate::t!("core.validate.still_stale"),
                    );
                } else {
                    out.error(
                        OutputProblemKind::StaleNotReverified,
                        item,
                        crate::t!("core.validate.stale_not_reverified"),
                    );
                }
            } else if !touched
                && target.starts_with(&knowledge)
                && parsed.as_ref().is_some_and(|(_, text)| is_template(text))
            {
                out.error(
                    OutputProblemKind::Template,
                    item,
                    crate::t!("core.validate.still_template"),
                );
            }
            produced.push(target);
        }

        for note in produced {
            if !note.starts_with(&knowledge) || note.extension().map(|e| e != "md").unwrap_or(true)
            {
                continue;
            }
            let rel = project.rel(&note);
            // Antes de validar: recortar los rangos que solo se pasan del
            // final del archivo (p. ej. `form.ejs:1-400` con 339 líneas).
            if let Ok(repairs) = repair_note_evidence(&project.root, &note) {
                for r in repairs {
                    out.issues.push(r.as_issue());
                    out.repairs.push(r);
                }
            }
            if let Ok(text) = fs::read_to_string(&note) {
                for issue in validate_note(Path::new(&rel), &text, &project.root) {
                    if issue.severity == IssueSeverity::Error {
                        out.problems.push(OutputProblem {
                            kind: OutputProblemKind::Invalid,
                            path: rel.clone(),
                            detail: Some(issue.message.clone()),
                        });
                    }
                    out.issues.push(issue);
                }
            }
        }
    }
    out.stale_in_dirs.sort();
    out.stale_in_dirs.dedup();
    out
}

/// Ruta relativa a `root` con `/` (o la ruta tal cual si está fuera).
fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Evidencia de una nota cuyo rango se recortó al final del archivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRepair {
    /// Nota (ruta relativa a la raíz del proyecto).
    pub note: String,
    /// Evidencia original (`src/a.ejs:1-400`).
    pub from: String,
    /// Evidencia ajustada (`src/a.ejs:1-339`).
    pub to: String,
    /// Líneas del archivo citado.
    pub total: usize,
}

impl EvidenceRepair {
    /// `ajustado: src/a.ejs:1-400 → 1-339 (el archivo tiene 339 líneas)`.
    pub fn message(&self) -> String {
        let range = self.to.rsplit_once(':').map(|(_, r)| r).unwrap_or(&self.to);
        crate::t!(
            "core.evidence.adjusted",
            from = self.from,
            to = range,
            total = self.total
        )
    }

    /// Aviso (no error) de la evidencia ajustada.
    pub fn as_issue(&self) -> NoteIssue {
        NoteIssue {
            path: self.note.clone(),
            severity: IssueSeverity::Warning,
            message: self.message(),
            kind: IssueKind::Evidence,
            line: None,
        }
    }
}

/// Evidencias de `text` (nota `note`, relativa a `root`) cuyo rango empieza
/// dentro del archivo pero termina después de su última línea. Solo
/// archivos existentes dentro del proyecto; las notas bloqueadas no se tocan.
pub fn plan_evidence_repairs(root: &Path, note: &str, text: &str) -> Vec<EvidenceRepair> {
    use crate::frontmatter::{clamp_source, file_line_count, resolve_source, SourceTarget};
    let Some(fm) = crate::frontmatter::NoteFrontmatter::parse(text) else {
        return Vec::new();
    };
    if fm.locked {
        return Vec::new();
    }
    let mut out: Vec<EvidenceRepair> = Vec::new();
    for src in &fm.sources {
        if out.iter().any(|r| &r.from == src) {
            continue;
        }
        let SourceTarget::File(full) = resolve_source(root, src) else {
            continue;
        };
        if crate::index::source_lines(src).is_none() {
            continue;
        }
        let total = file_line_count(&full);
        if let Some(to) = clamp_source(src, total) {
            out.push(EvidenceRepair {
                note: note.to_string(),
                from: src.clone(),
                to,
                total,
            });
        }
    }
    out
}

/// Ajusta en el frontmatter de `note` las evidencias que solo se pasan del
/// final del archivo citado (ver [`plan_evidence_repairs`]). Edición mínima
/// ([`crate::frontmatter::replace_sources`]) y escritura segura (sin seguir
/// symlinks, atómica). Devuelve las evidencias ajustadas; vacío si no hay
/// nada que ajustar o la edición no es segura.
pub fn repair_note_evidence(root: &Path, note: &Path) -> Result<Vec<EvidenceRepair>> {
    let Some(text) = crate::fsutil::read_to_string_nofollow(note)? else {
        return Ok(Vec::new());
    };
    let rel = rel_path(root, note);
    let repairs = plan_evidence_repairs(root, &rel, &text);
    if repairs.is_empty() {
        return Ok(repairs);
    }
    let changes: Vec<(String, String)> = repairs
        .iter()
        .map(|r| (r.from.clone(), r.to.clone()))
        .collect();
    let Some(updated) = crate::frontmatter::replace_sources(&text, &changes) else {
        return Ok(Vec::new());
    };
    crate::fsutil::write_in_root(root, note, updated.as_bytes())?;
    Ok(repairs)
}

/// `check --fix`: ajusta las evidencias reparables de todas las notas del
/// vault. Las notas que no se pueden escribir se omiten (siguen como error).
pub fn repair_vault(project: &Project) -> Vec<EvidenceRepair> {
    let knowledge = project.knowledge_dir();
    let mut files = Vec::new();
    collect_md(&knowledge, &mut files);
    files.retain(|f| {
        !f.strip_prefix(&knowledge)
            .map(|r| r.starts_with("_templates"))
            .unwrap_or(false)
    });
    files.sort();
    files
        .iter()
        .flat_map(|f| repair_note_evidence(&project.root, f).unwrap_or_default())
        .collect()
}

/// Extrae destinos de wikilinks `[[target]]` o `[[target|texto]]`.
pub fn extract_wikilinks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        match after.find("]]") {
            Some(end) => {
                out.push(after[..end].to_string());
                rest = &after[end + 2..];
            }
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Proyecto temporal (sin registro global) con `src/` y `knowledge/`.
    fn temp_project() -> Project {
        let dir = std::env::temp_dir().join(format!("adoc-rep-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("src/views")).unwrap();
        fs::create_dir_all(dir.join("knowledge/modules")).unwrap();
        let root = crate::paths::canonicalize(&dir).unwrap();
        Project {
            workspace: root.join(".ws"),
            root,
            config: crate::config::ArcheologyConfig::new("t", "es"),
        }
    }

    fn lines(n: usize) -> String {
        (1..=n).map(|i| format!("l{i}\n")).collect()
    }

    #[test]
    fn task_output_with_clampable_ranges_is_repaired_and_valid() {
        let p = temp_project();
        fs::write(p.root.join("src/views/form.ejs"), lines(339)).unwrap();
        fs::write(p.root.join("src/views/error.ejs"), lines(14)).unwrap();
        let note = p.root.join("knowledge/modules/src-views.md");
        fs::write(
            &note,
            "---\nid: mod-views\ntype: module\ntitle: Vistas\nsources:\n  - src/views/form.ejs:1-400\n  - src/views/error.ejs:14-50\n  - src/views/error.ejs:1-14\nstatus: draft\n---\n# Vistas\n",
        )
        .unwrap();
        let produce = vec!["knowledge/modules/src-views.md".to_string()];
        let check = check_task_output(&p, &produce, &[], None);
        assert!(check.problems.is_empty(), "{:?}", check.issues);
        assert_eq!(check.repairs.len(), 2);
        assert_eq!(check.repairs[0].to, "src/views/form.ejs:1-339");
        assert_eq!(check.repairs[1].to, "src/views/error.ejs:14");
        // Avisos (no errores) con la ruta relativa de la nota.
        assert!(check
            .issues
            .iter()
            .all(|i| i.severity == IssueSeverity::Warning
                && i.path == "knowledge/modules/src-views.md"));
        assert_eq!(
            check.issues[0].message,
            crate::t!(
                "core.evidence.adjusted",
                from = "src/views/form.ejs:1-400",
                to = "1-339",
                total = 339
            )
        );
        // Frontmatter reescrito con el resto de campos y el orden intactos.
        assert_eq!(
            fs::read_to_string(&note).unwrap(),
            "---\nid: mod-views\ntype: module\ntitle: Vistas\nsources:\n  - src/views/form.ejs:1-339\n  - src/views/error.ejs:14\n  - src/views/error.ejs:1-14\nstatus: draft\n---\n# Vistas\n"
        );
        // Idempotente.
        assert!(check_task_output(&p, &produce, &[], None)
            .repairs
            .is_empty());
        fs::remove_dir_all(&p.root).ok();
    }

    #[test]
    fn unrepairable_evidence_stays_an_error_with_relative_paths() {
        let p = temp_project();
        fs::write(p.root.join("src/views/error.ejs"), lines(14)).unwrap();
        fs::write(p.root.join("src/vacio.txt"), "").unwrap();
        let note = p.root.join("knowledge/modules/m.md");
        let text = "---\nid: m\ntype: module\nsources:\n  - src/views/error.ejs:20-50\n  - src/no-existe.ejs:1-5\n  - src/vacio.txt:1-5\n  - ../fuera.rs:1-2\n---\n";
        fs::write(&note, text).unwrap();
        let produce = vec!["knowledge/modules/".to_string()];
        let check = check_task_output(&p, &produce, &[], None);
        assert!(check.repairs.is_empty());
        assert_eq!(fs::read_to_string(&note).unwrap(), text);
        let errors: Vec<_> = check
            .issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Error)
            .collect();
        assert_eq!(errors.len(), 4, "{errors:?}");
        assert!(errors.iter().all(|i| i.path == "knowledge/modules/m.md"));
        assert!(check
            .problems
            .iter()
            .all(|pr| pr.path == "knowledge/modules/m.md"));
        // `check` también informa rutas relativas y nada reparable.
        let report = run_checks(&p, false).unwrap();
        assert!(
            report.errors.iter().all(|i| !i.path.starts_with('/')),
            "{:?}",
            report.errors
        );
        assert!(report.repairable.is_empty());
        fs::remove_dir_all(&p.root).ok();
    }

    #[test]
    fn repair_vault_fixes_every_note_and_skips_locked_ones() {
        let p = temp_project();
        fs::write(p.root.join("src/a.ejs"), "x\r\ny\r\nz").unwrap();
        let a = p.root.join("knowledge/modules/a.md");
        let locked = p.root.join("knowledge/modules/b.md");
        fs::write(
            &a,
            "---\nid: a\ntype: module\nsources: [src/a.ejs:2-9]\n---\n",
        )
        .unwrap();
        let locked_text = "---\nid: b\ntype: module\nlocked: true\nsources: [src/a.ejs:2-9]\n---\n";
        fs::write(&locked, locked_text).unwrap();
        let before = run_checks(&p, false).unwrap();
        assert_eq!(before.repairable.len(), 1);
        assert_eq!(before.repairable[0].note, "knowledge/modules/a.md");
        let fixed = repair_vault(&p);
        assert_eq!(fixed.len(), 1);
        assert_eq!(fixed[0].to, "src/a.ejs:2-3");
        assert!(fs::read_to_string(&a)
            .unwrap()
            .contains("sources: [src/a.ejs:2-3]"));
        assert_eq!(fs::read_to_string(&locked).unwrap(), locked_text);
        let after = run_checks(&p, false).unwrap();
        assert_eq!(after.errors.len(), 1, "{:?}", after.errors);
        assert_eq!(after.errors[0].path, "knowledge/modules/b.md");
        fs::remove_dir_all(&p.root).ok();
    }

    #[test]
    fn non_utf8_notes_become_issues() {
        let dir = std::env::temp_dir().join(format!("adoc-val-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("modules")).unwrap();
        fs::write(dir.join("modules/mala.md"), [0xff, 0xfe, b'#']).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&dir, dir.join("modules/ciclo")).unwrap();
        let mut report = CheckReport::default();
        validate_notes_dir(&dir, &dir, &mut report).unwrap();
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].message.contains("UTF-8"));
        fs::remove_dir_all(&dir).ok();
    }
}
