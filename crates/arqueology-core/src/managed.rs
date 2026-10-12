//! Regiones gestionadas: contenido que arqueology-doc puede regenerar dentro
//! de archivos compartidos sin tocar el resto (§11.6, RF-123, RF-162).

use crate::error::{Error, Result};
use std::fs;
use std::path::Path;

const BEGIN_PREFIX: &str = "<!-- arqueology-doc:begin ";
const END_PREFIX: &str = "<!-- arqueology-doc:end ";
const SUFFIX: &str = " -->";

/// Envuelve `content` en una región gestionada llamada `name`.
pub fn region(name: &str, content: &str) -> String {
    format!("{BEGIN_PREFIX}{name}{SUFFIX}\n{content}\n{END_PREFIX}{name}{SUFFIX}")
}

/// Inserta o reemplaza la región `name` dentro de `existing`.
///
/// - Si la región existe, se reemplaza su contenido.
/// - Si no existe, se añade al final del archivo.
/// - Nunca toca texto fuera de las regiones (convivencia con OpenSpec, RF-162).
///
/// Solo cuentan los marcadores que ocupan una línea completa (no los citados
/// dentro de un párrafo). Si están desbalanceados o duplicados —por ejemplo,
/// alguien borró la línea `end`— se devuelve un error en vez de adivinar:
/// reemplazar desde un `begin` huérfano borraría texto del usuario.
pub fn upsert_region(existing: &str, name: &str, content: &str) -> Result<String> {
    let begin = format!("{BEGIN_PREFIX}{name}{SUFFIX}");
    let end = format!("{END_PREFIX}{name}{SUFFIX}");
    let block = region(name, content);

    let begins = marker_lines(existing, &begin);
    let ends = marker_lines(existing, &end);
    match (begins.as_slice(), ends.as_slice()) {
        ([], []) => {}
        ([(b, _)], [(e, e_end)]) if b < e => {
            let mut out = String::with_capacity(existing.len() + block.len());
            out.push_str(&existing[..*b]);
            out.push_str(&block);
            out.push_str(&existing[*e_end..]);
            return Ok(out);
        }
        _ => {
            return Err(Error::Validation(format!(
                "marcadores de la región '{name}' desbalanceados o duplicados \
                 ({} begin, {} end): corrige el archivo a mano; no se modificó",
                begins.len(),
                ends.len()
            )))
        }
    }

    let mut out = existing.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&block);
    out.push('\n');
    Ok(out)
}

/// Posiciones (inicio, fin del marcador) de las líneas que son exactamente
/// `marker`, ignorando espacios alrededor.
fn marker_lines(text: &str, marker: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed.trim() == marker {
            let lead = trimmed.len() - trimmed.trim_start().len();
            let start = offset + lead;
            out.push((start, start + marker.len()));
        }
        offset += line.len();
    }
    out
}

/// Lee un archivo que se va a actualizar. Solo si no existe se usa `default`:
/// cualquier otro error (permisos, codificación no UTF-8…) se propaga, porque
/// continuar con la plantilla sobrescribiría el archivo del usuario.
pub fn read_or_default(path: &Path, default: impl FnOnce() -> String) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(default()),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => Err(Error::Validation(format!(
            "{} no es UTF-8 válido; conviértelo a UTF-8 para que arqueology-doc lo actualice \
             (no se modificó)",
            path.display()
        ))),
        Err(e) => Err(e.into()),
    }
}

/// Lista los nombres de regiones presentes en un texto.
pub fn region_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(BEGIN_PREFIX) {
            if let Some(name) = rest.strip_suffix(SUFFIX) {
                names.push(name.trim().to_string());
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_preserves_surrounding_text() {
        let existing = "# Header\n\nUser text stays.\n";
        let out = upsert_region(existing, "summary", "generated").unwrap();
        assert!(out.contains("User text stays."));
        assert!(out.contains("generated"));
        let out2 = upsert_region(&out, "summary", "v2").unwrap();
        assert!(out2.contains("v2"));
        assert!(!out2.contains("\ngenerated\n"));
        assert!(out2.contains("User text stays."));
    }

    #[test]
    fn missing_end_marker_never_eats_user_text() {
        let out = upsert_region("# T\n", "ctx", "v1").unwrap();
        // El usuario borra la línea `end` y escribe debajo.
        let broken = out.replace("<!-- arqueology-doc:end ctx -->\n", "") + "Mi sección\n";
        assert!(upsert_region(&broken, "ctx", "v2").is_err());
    }

    #[test]
    fn duplicated_markers_are_rejected() {
        let one = region("ctx", "a");
        let text = format!("{one}\n\ntexto\n\n{one}\n");
        assert!(upsert_region(&text, "ctx", "b").is_err());
    }

    #[test]
    fn markers_quoted_inside_a_line_are_ignored() {
        let text = "Usa `<!-- arqueology-doc:begin ctx -->` para…\n";
        let out = upsert_region(text, "ctx", "x").unwrap();
        assert!(out.starts_with(text));
        assert!(out.ends_with("<!-- arqueology-doc:end ctx -->\n"));
    }

    #[test]
    fn crlf_files_are_supported() {
        let text = "# T\r\n\r\n<!-- arqueology-doc:begin ctx -->\r\nviejo\r\n<!-- arqueology-doc:end ctx -->\r\nfin\r\n";
        let out = upsert_region(text, "ctx", "nuevo").unwrap();
        assert!(out.contains("nuevo") && !out.contains("viejo") && out.ends_with("fin\r\n"));
    }

    #[test]
    fn unreadable_files_are_not_replaced_by_the_template() {
        let dir = std::env::temp_dir().join(format!("adoc-read-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("AGENTS.md");
        fs::write(&path, [0xff, 0xfe, b'#', 0]).unwrap();
        assert!(read_or_default(&path, || "plantilla".into()).is_err());
        let missing = dir.join("nuevo.md");
        assert_eq!(
            read_or_default(&missing, || "plantilla".into()).unwrap(),
            "plantilla"
        );
        fs::remove_dir_all(&dir).ok();
    }
}
