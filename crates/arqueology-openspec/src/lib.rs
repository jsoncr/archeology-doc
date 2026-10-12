//! Spec bridge: detección y parsing de OpenSpec (§8.14).
//!
//! Lee `openspec/specs/<capability>/spec.md` y `openspec/changes/`.
//! Nunca escribe en `openspec/specs/` (RF-161).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecProject {
    /// Directorio openspec relativo a la raíz.
    pub path: PathBuf,
    pub capabilities: Vec<Capability>,
    pub active_changes: Vec<ChangeDir>,
    pub archived_changes: Vec<ChangeDir>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub name: String,
    pub spec_path: PathBuf,
    pub requirements: Vec<Requirement>,
    /// Propósito textual extraído de la sección Purpose, si existe.
    pub purpose: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    /// ID estable: `<capability>/<slug>`.
    pub id: String,
    pub title: String,
    pub text: String,
    pub scenarios: Vec<Scenario>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeDir {
    pub name: String,
    pub path: PathBuf,
    pub proposal: Option<String>,
    pub tasks_done: usize,
    pub tasks_total: usize,
    /// Días desde la última modificación (para `stale_change_days`, RF-154).
    pub days_since_update: Option<i64>,
}

/// Estado de cumplimiento de un requisito (RF-152).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComplianceStatus {
    Implemented,
    Partial,
    NotFound,
    Contradicted,
    /// Aún no evaluado por un agente.
    Unevaluated,
}

impl ComplianceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Implemented => "implemented",
            Self::Partial => "partial",
            Self::NotFound => "not-found",
            Self::Contradicted => "contradicted",
            Self::Unevaluated => "unevaluated",
        }
    }
}

/// Detecta y parsea el directorio openspec de un proyecto.
pub fn load(root: &Path, openspec_dir: &Path) -> Result<Option<SpecProject>> {
    let dir = root.join(openspec_dir);
    if !dir.is_dir() {
        return Ok(None);
    }

    let mut capabilities = Vec::new();
    let specs_dir = dir.join("specs");
    if specs_dir.is_dir() {
        for entry in fs::read_dir(&specs_dir)?.flatten() {
            let cap_dir = entry.path();
            if !cap_dir.is_dir() {
                continue;
            }
            let spec_file = cap_dir.join("spec.md");
            if !spec_file.is_file() {
                continue;
            }
            let name = cap_dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let text = fs::read_to_string(&spec_file)?;
            capabilities.push(parse_spec(&name, &spec_file, &text));
        }
    }
    capabilities.sort_by(|a, b| a.name.cmp(&b.name));

    let mut active = Vec::new();
    let mut archived = Vec::new();
    let changes_dir = dir.join("changes");
    if changes_dir.is_dir() {
        for entry in fs::read_dir(&changes_dir)?.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if name == "archive" {
                for ae in fs::read_dir(&p)?.flatten() {
                    let ap = ae.path();
                    if ap.is_dir() {
                        archived.push(parse_change(&ap)?);
                    }
                }
            } else {
                active.push(parse_change(&p)?);
            }
        }
    }
    active.sort_by(|a, b| a.name.cmp(&b.name));
    archived.sort_by(|a, b| a.name.cmp(&b.name));

    if capabilities.is_empty() && active.is_empty() && archived.is_empty() {
        return Ok(None);
    }

    Ok(Some(SpecProject {
        path: dir,
        capabilities,
        active_changes: active,
        archived_changes: archived,
    }))
}

fn parse_change(path: &Path) -> Result<ChangeDir> {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let proposal_path = path.join("proposal.md");
    let proposal = if proposal_path.is_file() {
        fs::read_to_string(&proposal_path).ok()
    } else {
        None
    };
    let mut done = 0usize;
    let mut total = 0usize;
    let tasks_path = path.join("tasks.md");
    if tasks_path.is_file() {
        if let Ok(text) = fs::read_to_string(&tasks_path) {
            for line in text.lines() {
                let t = line.trim_start();
                if t.starts_with("- [x]") || t.starts_with("- [X]") {
                    done += 1;
                    total += 1;
                } else if t.starts_with("- [ ]") {
                    total += 1;
                }
            }
        }
    }
    let days_since_update = last_activity_days(path);
    Ok(ChangeDir {
        name,
        path: path.to_path_buf(),
        proposal,
        tasks_done: done,
        tasks_total: total,
        days_since_update,
    })
}

/// Parsea `spec.md` según el formato OpenSpec:
/// `## Requirements` → `### Requirement: <título>` → `#### Scenario: <nombre>`.
pub fn parse_spec(capability: &str, spec_path: &Path, text: &str) -> Capability {
    let mut purpose = None;
    let mut requirements = Vec::new();
    let mut current_req: Option<Requirement> = None;
    let mut current_scen: Option<Scenario> = None;
    let mut in_purpose = false;
    let mut purpose_buf = String::new();

    let flush_scen = |scen: &mut Option<Scenario>, req: &mut Option<Requirement>| {
        if let (Some(s), Some(r)) = (scen.take(), req.as_mut()) {
            r.scenarios.push(s);
        }
    };
    let flush_req = |req: &mut Option<Requirement>, reqs: &mut Vec<Requirement>| {
        if let Some(r) = req.take() {
            reqs.push(r);
        }
    };

    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(h) = trimmed.strip_prefix("## ") {
            in_purpose = h.trim().eq_ignore_ascii_case("purpose");
            continue;
        }
        if in_purpose && !trimmed.starts_with('#') {
            purpose_buf.push_str(line);
            purpose_buf.push('\n');
        }
        if let Some(h) = trimmed.strip_prefix("### Requirement:") {
            flush_scen(&mut current_scen, &mut current_req);
            flush_req(&mut current_req, &mut requirements);
            let title = h.trim().to_string();
            let slug = slugify(&title);
            current_req = Some(Requirement {
                id: format!("{capability}/{slug}"),
                title,
                text: String::new(),
                scenarios: Vec::new(),
            });
            continue;
        }
        if let Some(h) = trimmed.strip_prefix("#### Scenario:") {
            flush_scen(&mut current_scen, &mut current_req);
            current_scen = Some(Scenario {
                name: h.trim().to_string(),
                body: String::new(),
            });
            continue;
        }
        if let Some(s) = current_scen.as_mut() {
            s.body.push_str(line);
            s.body.push('\n');
        } else if let Some(r) = current_req.as_mut() {
            r.text.push_str(line);
            r.text.push('\n');
        }
    }
    flush_scen(&mut current_scen, &mut current_req);
    flush_req(&mut current_req, &mut requirements);

    let purpose_trimmed = purpose_buf.trim().to_string();
    if !purpose_trimmed.is_empty() {
        purpose = Some(purpose_trimmed);
    }

    Capability {
        name: capability.to_string(),
        spec_path: spec_path.to_path_buf(),
        requirements,
        purpose,
    }
}

pub fn slugify(text: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for c in text.chars() {
        if c.is_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Días desde la última actividad de un cambio (RF-154).
///
/// Usa el último commit que tocó el directorio; si no hay git (o el cambio
/// no está versionado), el archivo modificado más recientemente.
pub fn last_activity_days(path: &Path) -> Option<i64> {
    let now = chrono::Utc::now().timestamp();
    let git = std::process::Command::new("git")
        .args(["log", "-1", "--format=%ct", "--", "."])
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<i64>()
                .ok()
        });
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain", "--", "."])
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(true);
    let ts = match git {
        // Cambios sin commitear = actividad reciente según el sistema de archivos.
        Some(t) if !dirty => t,
        _ => newest_mtime(path)?,
    };
    Some(((now - ts).max(0)) / 86_400)
}

fn newest_mtime(dir: &Path) -> Option<i64> {
    let mut newest: Option<i64> = None;
    for e in fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        // Sin seguir symlinks: un enlace a un directorio padre sería un ciclo.
        let t = if e.file_type().is_ok_and(|t| t.is_dir()) {
            newest_mtime(&p)
        } else {
            fs::metadata(&p)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
        };
        if let Some(t) = t {
            newest = Some(newest.map_or(t, |n: i64| n.max(t)));
        }
    }
    newest
}

impl ChangeDir {
    /// Cambio activo sin actividad en más de `days` días (RF-154).
    pub fn is_stale(&self, days: u64) -> bool {
        self.days_since_update
            .map(|d| d > days as i64)
            .unwrap_or(false)
    }
}

/// Evaluación de un requisito escrita por un agente en un `spec-report`
/// (`knowledge/specs/<capability>.md`, §11.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementReport {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub finding: Option<String>,
    /// Nota de la que se leyó.
    #[serde(skip_deserializing)]
    pub report: String,
}

/// Lee todos los `spec-report` del vault: id de requisito → evaluación.
pub fn load_reports(specs_dir: &Path) -> BTreeMap<String, RequirementReport> {
    let mut out = BTreeMap::new();
    let Ok(entries) = fs::read_dir(specs_dir) else {
        return out;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.extension().map(|x| x != "md").unwrap_or(true) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Some((map, _)) = arqueology_core::frontmatter::extract_frontmatter(&text) else {
            continue;
        };
        let is_report = map
            .get(serde_yaml::Value::String("type".into()))
            .and_then(|v| v.as_str())
            == Some("spec-report");
        if !is_report {
            continue;
        }
        let Some(reqs) = map.get(serde_yaml::Value::String("requirements".into())) else {
            continue;
        };
        // Entrada por entrada: un requisito mal escrito no descarta el reporte.
        for (i, item) in reqs.as_sequence().into_iter().flatten().enumerate() {
            match serde_yaml::from_value::<RequirementReport>(item.clone()) {
                Ok(mut r) => {
                    r.report = path.display().to_string();
                    out.insert(r.id.clone(), r);
                }
                Err(e) => eprintln!(
                    "aviso: {}: requirements[{i}] inválido, se ignora ({e})",
                    path.display()
                ),
            }
        }
    }
    out
}

/// Estado de cada requisito tras cruzar specs con reportes.
#[derive(Debug, Clone, Serialize)]
pub struct RequirementStatus {
    pub id: String,
    pub title: String,
    pub status: String,
    pub evidence: Vec<String>,
    pub finding: Option<String>,
}

/// Resumen de cumplimiento para `spec status` y `review` (RF-163).
#[derive(Debug, Default, Serialize)]
pub struct ComplianceSummary {
    /// capability → (status → count)
    pub by_capability: BTreeMap<String, BTreeMap<String, usize>>,
    /// status → count en todo el proyecto.
    pub totals: BTreeMap<String, usize>,
    pub total_requirements: usize,
    /// capability → requisitos con su estado.
    pub requirements: BTreeMap<String, Vec<RequirementStatus>>,
    /// IDs evaluados en reportes que no existen en las specs.
    pub unknown_ids: Vec<String>,
    /// Cambios activos sin actividad en `stale_change_days`.
    pub stale_changes: Vec<String>,
}

const VALID_STATUSES: &[&str] = &["implemented", "partial", "not-found", "contradicted"];

/// Cruza las specs con las evaluaciones de los agentes (RF-152, RF-163).
pub fn compliance(
    project: &SpecProject,
    reports: &BTreeMap<String, RequirementReport>,
    stale_days: u64,
) -> ComplianceSummary {
    let mut summary = ComplianceSummary::default();
    let mut known = std::collections::BTreeSet::new();
    for cap in &project.capabilities {
        let counts = summary.by_capability.entry(cap.name.clone()).or_default();
        let list = summary.requirements.entry(cap.name.clone()).or_default();
        for req in &cap.requirements {
            known.insert(req.id.clone());
            let (status, evidence, finding) = match reports.get(&req.id) {
                Some(r) if VALID_STATUSES.contains(&r.status.as_str()) => {
                    (r.status.clone(), r.evidence.clone(), r.finding.clone())
                }
                _ => (
                    ComplianceStatus::Unevaluated.as_str().to_string(),
                    vec![],
                    None,
                ),
            };
            *counts.entry(status.clone()).or_insert(0) += 1;
            *summary.totals.entry(status.clone()).or_insert(0) += 1;
            list.push(RequirementStatus {
                id: req.id.clone(),
                title: req.title.clone(),
                status,
                evidence,
                finding,
            });
            summary.total_requirements += 1;
        }
    }
    summary.unknown_ids = reports
        .keys()
        .filter(|id| !known.contains(*id))
        .cloned()
        .collect();
    summary.stale_changes = project
        .active_changes
        .iter()
        .filter(|c| c.is_stale(stale_days))
        .map(|c| c.name.clone())
        .collect();
    summary
}

/// Validación estructural mínima de un cambio cuando el CLI de OpenSpec no
/// está disponible (RF-159). Devuelve los problemas encontrados.
pub fn validate_change_structure(change_dir: &Path) -> Vec<String> {
    let mut issues = Vec::new();
    let proposal = change_dir.join("proposal.md");
    match fs::read_to_string(&proposal) {
        Ok(t) => {
            for section in ["## Why", "## What Changes"] {
                if !t.contains(section) {
                    issues.push(format!("proposal.md sin sección `{section}`"));
                }
            }
        }
        Err(_) => issues.push("falta proposal.md".into()),
    }
    let specs = change_dir.join("specs");
    let mut spec_files = Vec::new();
    if let Ok(entries) = fs::read_dir(&specs) {
        for e in entries.flatten() {
            let f = e.path().join("spec.md");
            if f.is_file() {
                spec_files.push(f);
            }
        }
    }
    if spec_files.is_empty() {
        issues.push("el cambio no tiene deltas en specs/<capability>/spec.md".into());
    }
    for f in spec_files {
        let rel = f
            .strip_prefix(change_dir)
            .unwrap_or(&f)
            .display()
            .to_string();
        let text = fs::read_to_string(&f).unwrap_or_default();
        let has_delta = ["ADDED", "MODIFIED", "REMOVED", "RENAMED"]
            .iter()
            .any(|k| text.contains(&format!("## {k} Requirements")));
        if !has_delta {
            issues.push(format!("{rel}: sin sección `## ADDED Requirements`"));
        }
        let cap = parse_spec("x", &f, &text);
        if cap.requirements.is_empty() {
            issues.push(format!("{rel}: sin requisitos"));
        }
        for r in &cap.requirements {
            if r.scenarios.is_empty() {
                issues.push(format!("{rel}: `{}` sin `#### Scenario:`", r.title));
            }
            if !(r.text.contains("SHALL") || r.text.contains("MUST")) {
                issues.push(format!("{rel}: `{}` sin SHALL/MUST", r.title));
            }
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC: &str = "# billing\n\n## Purpose\n\nFacturación.\n\n## Requirements\n\n\
### Requirement: Apply discount\n\nThe system SHALL cap discounts.\n\n\
#### Scenario: Cap\n\n- **WHEN** x\n- **THEN** y\n\n\
### Requirement: Add numbers\n\nThe system SHALL add.\n";

    #[test]
    fn parses_requirements_and_scenarios() {
        let cap = parse_spec("billing", Path::new("spec.md"), SPEC);
        assert_eq!(cap.purpose.as_deref(), Some("Facturación."));
        assert_eq!(cap.requirements.len(), 2);
        assert_eq!(cap.requirements[0].id, "billing/apply-discount");
        assert_eq!(cap.requirements[0].scenarios.len(), 1);
    }

    #[test]
    fn compliance_merges_reports() {
        let cap = parse_spec("billing", Path::new("spec.md"), SPEC);
        let project = SpecProject {
            path: PathBuf::new(),
            capabilities: vec![cap],
            active_changes: vec![],
            archived_changes: vec![],
        };
        let mut reports = BTreeMap::new();
        for (id, st) in [
            ("billing/apply-discount", "contradicted"),
            ("billing/ghost", "implemented"),
        ] {
            reports.insert(
                id.to_string(),
                RequirementReport {
                    id: id.into(),
                    status: st.into(),
                    evidence: vec![],
                    finding: None,
                    report: String::new(),
                },
            );
        }
        let s = compliance(&project, &reports, 30);
        assert_eq!(s.totals.get("contradicted"), Some(&1));
        assert_eq!(s.totals.get("unevaluated"), Some(&1));
        assert_eq!(s.unknown_ids, vec!["billing/ghost"]);
    }
}
