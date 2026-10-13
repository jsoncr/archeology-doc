//! Parsing y validación del frontmatter YAML de las notas (§11).

use crate::t;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// Frontmatter parseado de una nota: campos tipados comunes + resto en mapa.
#[derive(Debug, Clone)]
pub struct NoteFrontmatter {
    pub id: Option<String>,
    pub note_type: Option<String>,
    pub title: Option<String>,
    pub status: Option<String>,
    pub confidence: Option<String>,
    pub sources: Vec<String>,
    pub locked: bool,
    pub fields: serde_yaml::Mapping,
}

/// Tipos de nota válidos (§11.1).
pub const NOTE_TYPES: &[&str] = &[
    "overview",
    "module",
    "feature",
    "finding",
    "test-report",
    "spec-report",
    "glossary",
    "decision",
    "memory",
    "moc",
    "faq",
];

const STATUSES: &[&str] = &["draft", "reviewed", "stale", "locked"];
const CONFIDENCES: &[&str] = &["high", "medium", "low"];
const FINDING_KINDS: &[&str] = &[
    "contradiction",
    "duplication",
    "dead-code",
    "risk",
    "tech-debt",
    "stale-doc",
    "config",
    "question",
    "spec-drift",
    "unspecified-behavior",
    "stale-change",
];
const SEVERITIES: &[&str] = &["critical", "high", "medium", "low"];
const FINDING_STATES: &[&str] = &["open", "confirmed", "dismissed", "resolved"];

/// Extrae el bloque `--- ... ---` del inicio de un documento Markdown.
pub fn extract_frontmatter(text: &str) -> Option<(serde_yaml::Mapping, &str)> {
    let (yaml, body_start) = frontmatter_range(text)?;
    let mapping: serde_yaml::Mapping = serde_yaml::from_str(&text[yaml]).ok()?;
    Some((mapping, &text[body_start..]))
}

/// Rango en bytes del YAML dentro de `text` (entre la línea `---` de apertura
/// y la de cierre) y posición donde empieza el cuerpo. Admite BOM y líneas en
/// blanco antes de la apertura, CRLF y `...` como cierre.
pub fn frontmatter_range(text: &str) -> Option<(std::ops::Range<usize>, usize)> {
    let trimmed = text.trim_start_matches(['\u{feff}', '\n', '\r', ' ', '\t']);
    let lead = text.len() - trimmed.len();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after_open = &trimmed[3..];
    // La línea de apertura debe ser solo "---".
    let first_nl = after_open.find('\n')?;
    if !after_open[..first_nl].trim().is_empty() {
        return None;
    }
    let yaml_start = lead + 3 + first_nl + 1;
    let (yaml_end, body_start) = find_closing(&text[yaml_start..])?;
    Some((yaml_start..yaml_start + yaml_end, yaml_start + body_start))
}

/// Fija `key: value` en el frontmatter sin tocar nada fuera del YAML. Si la
/// clave no existe, se añade al final del frontmatter. Conserva el BOM y el
/// tipo de salto de línea. Sin frontmatter, devuelve el texto tal cual.
pub fn set_field(text: &str, key: &str, value: &str) -> String {
    let Some((yaml, _)) = frontmatter_range(text) else {
        return text.to_string();
    };
    let eol = if text[yaml.clone()].contains("\r\n") || text[yaml.end..].starts_with("---\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let prefix = format!("{key}:");
    let mut offset = yaml.start;
    for line in text[yaml.clone()].split_inclusive('\n') {
        // Solo claves de primer nivel (sin sangría).
        if line.starts_with(&prefix) {
            let content_end = offset + line.trim_end_matches(['\n', '\r']).len();
            return format!("{}{key}: {value}{}", &text[..offset], &text[content_end..]);
        }
        offset += line.len();
    }
    format!(
        "{}{key}: {value}{eol}{}",
        &text[..yaml.end],
        &text[yaml.end..]
    )
}

/// Devuelve (fin del YAML, inicio del body): posición donde empieza la
/// línea de cierre y donde termina.
fn find_closing(text: &str) -> Option<(usize, usize)> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let stripped = line.trim_end();
        if stripped == "---" || stripped == "..." {
            return Some((offset, offset + line.len()));
        }
        offset += line.len();
    }
    None
}

impl NoteFrontmatter {
    pub fn parse(text: &str) -> Option<Self> {
        let (map, _) = extract_frontmatter(text)?;
        let get_str = |key: &str| -> Option<String> {
            map.get(serde_yaml::Value::String(key.into()))
                .and_then(|v| v.as_str().map(String::from))
        };
        let sources = map
            .get(serde_yaml::Value::String("sources".into()))
            .and_then(|v| v.as_sequence())
            .map(|seq| {
                seq.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        // Ante la duda se bloquea: `locked: yes` o `"true"` también protegen la nota.
        let locked = match map.get(serde_yaml::Value::String("locked".into())) {
            Some(serde_yaml::Value::Bool(b)) => *b,
            Some(serde_yaml::Value::String(s)) => matches!(
                s.trim().to_lowercase().as_str(),
                "true" | "yes" | "y" | "on" | "si" | "sí"
            ),
            _ => false,
        };
        Some(Self {
            id: get_str("id"),
            note_type: get_str("type"),
            title: get_str("title"),
            status: get_str("status"),
            confidence: get_str("confidence"),
            sources,
            locked,
            fields: map,
        })
    }

    pub fn get(&self, key: &str) -> Option<&serde_yaml::Value> {
        self.fields.get(serde_yaml::Value::String(key.into()))
    }
}

/// Error de validación de una nota.
#[derive(Debug, Clone)]
pub struct NoteIssue {
    pub path: String,
    pub severity: IssueSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    Error,
    Warning,
}

/// Valida el frontmatter de una nota según su tipo (RF-130).
pub fn validate_note(path: &Path, text: &str, project_root: &Path) -> Vec<NoteIssue> {
    let display = path.display().to_string();
    let mut issues = Vec::new();
    let err = |display: &String, msg: String| NoteIssue {
        path: display.clone(),
        severity: IssueSeverity::Error,
        message: msg,
    };
    let warn = |display: &String, msg: String| NoteIssue {
        path: display.clone(),
        severity: IssueSeverity::Warning,
        message: msg,
    };

    let fm = match NoteFrontmatter::parse(text) {
        Some(f) => f,
        None => {
            issues.push(err(&display, t!("core.note.no_frontmatter")));
            return issues;
        }
    };

    if fm.id.as_deref().unwrap_or("").is_empty() {
        issues.push(err(&display, t!("core.note.missing_id")));
    }
    match fm.note_type.as_deref() {
        None => issues.push(err(&display, t!("core.note.missing_type"))),
        Some(t) if !NOTE_TYPES.contains(&t) => issues.push(err(
            &display,
            t!(
                "core.note.unknown_type",
                value = t,
                valid = NOTE_TYPES.join(", ")
            ),
        )),
        _ => {}
    }
    if let Some(s) = fm.status.as_deref() {
        if !STATUSES.contains(&s) {
            issues.push(warn(
                &display,
                t!("core.note.nonstandard_status", value = s),
            ));
        }
    }
    if let Some(c) = fm.confidence.as_deref() {
        if !CONFIDENCES.contains(&c) {
            issues.push(warn(
                &display,
                t!("core.note.nonstandard_confidence", value = c),
            ));
        }
    }

    // Memoria estable vs temporal (RF-103).
    if fm.note_type.as_deref() == Some("memory") {
        if let Some(m) = fm.get("memory").and_then(|v| v.as_str()) {
            if m != "stable" && m != "temporary" {
                issues.push(warn(
                    &display,
                    t!("core.note.nonstandard_memory", value = m),
                ));
            }
        }
    }

    // Validación por tipo.
    if fm.note_type.as_deref() == Some("finding") {
        match fm.get("kind").and_then(|v| v.as_str()) {
            Some(k) if FINDING_KINDS.contains(&k) => {}
            Some(k) => issues.push(err(&display, t!("core.note.invalid_kind", value = k))),
            None => issues.push(err(&display, t!("core.note.finding_without_kind"))),
        }
        match fm.get("severity").and_then(|v| v.as_str()) {
            Some(s) if SEVERITIES.contains(&s) => {}
            Some(s) => issues.push(err(&display, t!("core.note.invalid_severity", value = s))),
            None => issues.push(err(&display, t!("core.note.finding_without_severity"))),
        }
        match fm.get("state").and_then(|v| v.as_str()) {
            Some(s) if FINDING_STATES.contains(&s) => {}
            Some(s) => issues.push(err(&display, t!("core.note.invalid_state", value = s))),
            None => issues.push(err(&display, t!("core.note.finding_without_state"))),
        }
        if fm.sources.is_empty() {
            issues.push(err(&display, t!("core.note.finding_without_sources")));
        }
    }

    // Verificar que las evidencias existen: archivo y rango de líneas (RF-082).
    for src in &fm.sources {
        let file = crate::index::source_file(src);
        let full = project_root.join(file);
        // Rutas escritas por agentes: absolutas, con `..` o a través de un
        // symlink podrían apuntar fuera del proyecto (y leerse para contar
        // líneas). Se rechazan sin tocarlas.
        let lexically_inside = !Path::new(file).is_absolute()
            && Path::new(file).components().all(|c| {
                matches!(
                    c,
                    std::path::Component::Normal(_) | std::path::Component::CurDir
                )
            });
        if !lexically_inside || !crate::project::is_inside(project_root, &full) {
            issues.push(err(&display, t!("core.note.source_outside", source = src)));
            continue;
        }
        if !full.is_file() {
            issues.push(err(
                &display,
                t!("core.note.source_not_found", source = src),
            ));
            continue;
        }
        if let Some((a, b)) = crate::index::source_lines(src) {
            let total = std::fs::read(&full)
                .map(|bytes| {
                    let n = bytes.iter().filter(|&&c| c == b'\n').count();
                    if bytes.last().map(|&c| c != b'\n').unwrap_or(false) {
                        n + 1
                    } else {
                        n
                    }
                })
                .unwrap_or(0);
            if a == 0 || a > b || b > total {
                issues.push(err(
                    &display,
                    t!("core.note.source_out_of_range", source = src, total = total),
                ));
            }
        }
    }

    // Features y findings requieren al menos una evidencia (§17).
    if fm.note_type.as_deref() == Some("feature") && fm.sources.is_empty() {
        issues.push(err(&display, t!("core.note.feature_without_sources")));
    }

    issues
}

/// Fila de `review`: hallazgo abierto con su severidad.
#[derive(Debug, Deserialize)]
pub struct FindingRow {
    pub id: Option<String>,
    pub title: Option<String>,
    pub severity: Option<String>,
    pub state: Option<String>,
    pub kind: Option<String>,
}

/// Convierte `serde_yaml::Mapping` a un mapa plano de strings (para conteos).
pub fn mapping_to_map(map: &serde_yaml::Mapping) -> BTreeMap<String, serde_yaml::Value> {
    map.iter()
        .filter_map(|(k, v)| k.as_str().map(|ks| (ks.to_string(), v.clone())))
        .collect()
}

/// Devuelve `Error::Validation` si el texto contiene patrones de secretos
/// (heurística simple para no incluirlos en prompts, RNF-05).
pub fn looks_like_secret_path(path: &str, patterns: &[String]) -> bool {
    let name = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    for pat in patterns {
        if glob_match(&pat.to_lowercase(), &name) {
            return true;
        }
    }
    false
}

/// Coincidencia glob mínima: `*` en cualquier posición, `?`, sin directorios.
///
/// Iterativa, con retroceso solo hasta el último `*`: tiempo O(n·m) en el
/// peor caso, sin la explosión exponencial de la versión recursiva ante
/// patrones como `*a*a*a*…b` (vienen del TOML del repositorio).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    // Posición del último `*` visto y del texto cuando se vio.
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || (p[pi] != '*' && p[pi] == t[ti])) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            // El `*` absorbe un carácter más y se reintenta.
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_field_keeps_bom_crlf_and_body() {
        let text = "\u{feff}---\r\nid: x\r\nstatus: draft\r\n---\r\nstatus: no es frontmatter\r\n\r\n---\r\n";
        let out = set_field(text, "status", "stale");
        assert_eq!(
            out,
            "\u{feff}---\r\nid: x\r\nstatus: stale\r\n---\r\nstatus: no es frontmatter\r\n\r\n---\r\n"
        );
        let added = set_field("---\nid: x\n---\n# T\n---\n", "status", "stale");
        assert_eq!(added, "---\nid: x\nstatus: stale\n---\n# T\n---\n");
        assert_eq!(
            set_field("# sin frontmatter\n", "status", "x"),
            "# sin frontmatter\n"
        );
    }

    #[test]
    fn parses_frontmatter() {
        let text = "---\nid: f-1\ntype: feature\nsources:\n  - a.rs:1\n---\n# body";
        let fm = NoteFrontmatter::parse(text).unwrap();
        assert_eq!(fm.id.as_deref(), Some("f-1"));
        assert_eq!(fm.note_type.as_deref(), Some("feature"));
        assert_eq!(fm.sources, vec!["a.rs:1".to_string()]);
    }

    #[test]
    fn glob_matching() {
        assert!(glob_match("*.pem", "cert.pem"));
        assert!(glob_match(".env*", ".env.local"));
        assert!(!glob_match("*.key", "keystore.toml"));
        assert!(glob_match("*id_rsa*", "my_id_rsa.pub"));
        assert!(glob_match("a?c", "abc") && !glob_match("a?c", "ac"));
        assert!(glob_match("*", "") && !glob_match("?", ""));
        // Patrón patológico: debe terminar al instante.
        let pat = format!("{}b", "*a".repeat(30));
        let text = "a".repeat(60);
        let start = std::time::Instant::now();
        assert!(!glob_match(&pat, &text));
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }

    #[test]
    fn evidence_outside_project_is_reported() {
        let root = std::env::temp_dir().join(format!("adoc-ev-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.rs"), "x\n").unwrap();
        for src in ["../../etc/passwd:1", "/etc/passwd"] {
            let text = format!("---\nid: n\ntype: module\nsources:\n  - {src}\n---\n");
            let issues = validate_note(Path::new("n.md"), &text, &root);
            assert!(
                issues
                    .iter()
                    .any(|i| i.message == t!("core.note.source_outside", source = src)),
                "{src}: {issues:?}"
            );
        }
        let ok = "---\nid: n\ntype: module\nsources:\n  - ./a.rs:1\n---\n";
        assert!(validate_note(Path::new("n.md"), ok, &root).is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}
