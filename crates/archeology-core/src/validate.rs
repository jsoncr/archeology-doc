//! Validaciones del comando `check` (RF-130, RF-131).

use crate::error::Result;
use crate::frontmatter::{validate_note, IssueSeverity, NoteIssue};
use crate::project::Project;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Default)]
pub struct CheckReport {
    pub errors: Vec<NoteIssue>,
    pub warnings: Vec<NoteIssue>,
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

    check_wikilinks(&knowledge, &mut report);
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
                    for issue in validate_note(&path, &text, root) {
                        report.push(issue);
                    }
                }
                // Una nota ilegible (p. ej. no UTF-8) es un problema de esa
                // nota, no un motivo para abortar todo el `check`.
                Err(e) => report.push(NoteIssue {
                    path: path.display().to_string(),
                    severity: IssueSeverity::Error,
                    message: if e.kind() == std::io::ErrorKind::InvalidData {
                        crate::t!("core.validate.note_not_utf8")
                    } else {
                        crate::t!("core.validate.note_unreadable", error = e)
                    },
                }),
            }
        }
    }
    Ok(())
}

/// Detecta wikilinks `[[...]]` que no resuelven a ninguna nota del vault.
fn check_wikilinks(knowledge: &Path, report: &mut CheckReport) {
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
        for cap in extract_wikilinks(&text) {
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
                    path: file.display().to_string(),
                    severity: IssueSeverity::Warning,
                    message: crate::t!("core.validate.broken_wikilink", target = target),
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
                }),
                None => {
                    findings.insert((kind, sources), rel);
                }
            }
        }
    }
}

/// Valida los artefactos que una tarea declara en `# Produce` (§14.2, RF-050).
///
/// - Rutas terminadas en `/`: deben contener al menos una nota `.md`
///   (modificada desde `since`, si se indica).
/// - Rutas de archivo: deben existir y, si se indica `since`, haberse
///   modificado desde entonces.
/// - Las notas producidas dentro del vault se validan con [`validate_note`].
pub fn validate_task_output(
    project: &Project,
    produce: &[String],
    optional: &[String],
    since: Option<SystemTime>,
) -> Vec<NoteIssue> {
    let mut issues = Vec::new();
    let knowledge = project.knowledge_dir();
    let modified_since = |p: &Path| match since {
        None => true,
        Some(t) => fs::metadata(p)
            .and_then(|m| m.modified())
            .map(|m| m >= t)
            .unwrap_or(false),
    };

    for item in produce {
        let target = project.root.join(item.trim_end_matches('/'));
        let mut produced: Vec<PathBuf> = Vec::new();
        let is_optional = optional.iter().any(|o| o == item);
        if item.ends_with('/') {
            let mut files = Vec::new();
            collect_md(&target, &mut files);
            // Las notas índice (MOC) las escribe archeology-doc, no el agente.
            produced.extend(files.into_iter().filter(|f| {
                modified_since(f)
                    && f.file_name()
                        .map(|n| n != crate::index::MOC_FILE)
                        .unwrap_or(true)
            }));
            if produced.is_empty() && !is_optional {
                issues.push(NoteIssue {
                    path: item.clone(),
                    severity: IssueSeverity::Error,
                    message: crate::t!("core.validate.no_notes_produced"),
                });
            }
        } else if !target.is_file() {
            if is_optional {
                continue;
            }
            issues.push(NoteIssue {
                path: item.clone(),
                severity: IssueSeverity::Error,
                message: crate::t!("core.validate.expected_file_missing"),
            });
        } else {
            if !modified_since(&target) {
                issues.push(NoteIssue {
                    path: item.clone(),
                    severity: IssueSeverity::Error,
                    message: crate::t!("core.validate.file_not_updated"),
                });
            }
            produced.push(target);
        }

        for note in produced {
            if !note.starts_with(&knowledge) || note.extension().map(|e| e != "md").unwrap_or(true)
            {
                continue;
            }
            if let Ok(text) = fs::read_to_string(&note) {
                issues.extend(validate_note(&note, &text, &project.root));
            }
        }
    }
    issues
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
