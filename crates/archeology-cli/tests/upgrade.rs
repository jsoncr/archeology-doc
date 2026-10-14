//! `adoc upgrade` contra un servidor HTTP local (sin red real): un hilo con
//! `TcpListener` sirve `latest.json`, `SHA256SUMS` y archivos `.tar.gz` con
//! binarios falsos (scripts de shell en Unix).
//!
//! Los tests que actualizan ejecutan una COPIA del `adoc` compilado desde una
//! carpeta temporal: el binario de `target/` nunca se sustituye.

use assert_cmd::Command;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// Versión "nueva" del servidor: siempre mayor que la del workspace.
const NEW: &str = "99.0.0";
const TARGETS: &[&str] = &[
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
];

/// Servidor HTTP mínimo: sirve `files` bajo `/download/` y anota cada ruta pedida.
struct Server {
    url: String,
    hits: Arc<Mutex<Vec<String>>>,
}

impl Server {
    fn start(files: HashMap<String, Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/download", listener.local_addr().unwrap());
        let hits = Arc::new(Mutex::new(Vec::new()));
        let log = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                if reader.read_line(&mut request).is_err() {
                    continue;
                }
                // Cabeceras: hasta la línea vacía.
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) if line == "\r\n" || line == "\n" => break,
                        Ok(_) => {}
                    }
                }
                let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
                log.lock().unwrap().push(path.clone());
                let name = path.strip_prefix("/download/").unwrap_or("");
                let (status, body) = match files.get(name) {
                    Some(b) => ("200 OK", b.clone()),
                    None => ("404 Not Found", b"not found".to_vec()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });
        Server { url, hits }
    }

    fn hits(&self) -> Vec<String> {
        self.hits.lock().unwrap().clone()
    }
}

/// Opciones de la publicación falsa.
#[derive(Default)]
struct Release {
    /// `SHA256SUMS` con otra suma (no coincide con latest.json).
    tamper_sums: bool,
    /// Archivo servido distinto del que se sumó (descarga alterada).
    corrupt_archive: bool,
    /// Entrada extra con esta ruta tal cual (p. ej. `../evil`).
    extra_entry: Option<&'static str>,
}

fn sha256(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

/// Script que responde a `--version` como lo haría el binario.
fn fake_binary(name: &str, version: &str) -> Vec<u8> {
    format!("#!/bin/sh\necho \"{name} {version}\"\n").into_bytes()
}

fn tar_gz(target: &str, version: &str, extra: Option<&str>) -> Vec<u8> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut b = tar::Builder::new(gz);
    let mut add = |path: &str, data: &[u8]| {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o755);
        h.set_entry_type(tar::EntryType::Regular);
        // Nombre escrito a mano: `set_path` rechaza `..` (justo lo que se prueba).
        let name = &mut h.as_old_mut().name;
        name[..path.len()].copy_from_slice(path.as_bytes());
        h.set_cksum();
        b.append(&h, data).unwrap();
    };
    for bin in ["adoc", "archeology-doc"] {
        add(&format!("adoc-{target}/{bin}"), &fake_binary(bin, version));
    }
    add(&format!("adoc-{target}/README.md"), b"# readme\n");
    if let Some(p) = extra {
        add(p, b"#!/bin/sh\necho evil\n");
    }
    b.into_inner().unwrap().finish().unwrap()
}

/// Archivos de /download para `version` (como los arma `WEBSITE/dist.sh`).
fn release(version: &str, opts: &Release) -> HashMap<String, Vec<u8>> {
    let mut files = HashMap::new();
    let mut sums = String::new();
    let mut entries = Vec::new();
    for t in TARGETS {
        let name = format!("adoc-{t}.tar.gz");
        let data = tar_gz(t, version, opts.extra_entry);
        let sum = sha256(&data);
        sums.push_str(&format!(
            "{}  {name}\n",
            if opts.tamper_sums {
                "0".repeat(64)
            } else {
                sum.clone()
            }
        ));
        entries.push(format!(
            r#"{{"file": "{name}", "sha256": "{sum}", "size": {}}}"#,
            data.len()
        ));
        let served = if opts.corrupt_archive {
            tar_gz(t, "6.6.6", opts.extra_entry)
        } else {
            data
        };
        files.insert(name, served);
    }
    files.insert("SHA256SUMS".into(), sums.into_bytes());
    files.insert(
        "latest.json".into(),
        format!(
            r#"{{"version": "{version}", "generated_at": "2026-01-01T00:00:00Z", "files": [{}]}}"#,
            entries.join(", ")
        )
        .into_bytes(),
    );
    files
}

/// Carpeta temporal con configuración aislada.
struct Sandbox {
    tmp: tempfile::TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Self {
            tmp: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self) -> &Path {
        self.tmp.path()
    }

    /// Copia del `adoc` compilado en `<tmp>/<rel>/adoc`.
    fn install_copy(&self, rel: &str) -> PathBuf {
        let dir = self.path().join(rel);
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join(format!("adoc{}", std::env::consts::EXE_SUFFIX));
        fs::copy(assert_cmd::cargo::cargo_bin("adoc"), &dest).unwrap();
        dest
    }

    /// `bin` con la URL del servidor, configuración aislada e interfaz en inglés.
    fn cmd(&self, bin: &Path, base: &str) -> Command {
        let mut cmd = Command::new(bin);
        cmd.current_dir(self.path())
            .env("ARCHEOLOGY_DOC_CONFIG_DIR", self.path().join("config"))
            .env("ARCHEOLOGY_DOC_DOWNLOAD", base)
            .env("ADOC_LANG", "en")
            .env("ADOC_NO_PROGRESS", "1")
            // Ningún `cargo install` real en juego.
            .env("CARGO_HOME", self.path().join("no-cargo"))
            .env_remove("ADOC_NO_UPDATE_CHECK");
        cmd
    }
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

fn adoc() -> PathBuf {
    assert_cmd::cargo::cargo_bin("adoc")
}

#[test]
fn check_reports_up_to_date() {
    let server = Server::start(release(CURRENT, &Release::default()));
    let sb = Sandbox::new();
    let out = sb
        .cmd(&adoc(), &server.url)
        .args(["upgrade", "--check"])
        .assert()
        .success();
    let text = stdout(out.get_output());
    assert!(
        text.contains(&format!("adoc {CURRENT} is up to date")),
        "{text}"
    );

    let out = sb
        .cmd(&adoc(), &server.url)
        .args(["upgrade", "--check", "--format", "json"])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_str(&stdout(out.get_output())).unwrap();
    assert_eq!(v["current"], CURRENT);
    assert_eq!(v["latest"], CURRENT);
    assert_eq!(v["update_available"], false);
    // Solo se consultó latest.json (nada de descargas).
    assert!(server.hits().iter().all(|h| h == "/download/latest.json"));

    // Sin --check y al día: lo dice y sale con 0 sin descargar nada.
    let out = sb
        .cmd(&adoc(), &server.url)
        .args(["upgrade"])
        .assert()
        .success();
    assert!(stdout(out.get_output()).contains("is up to date"));
    assert!(server.hits().iter().all(|h| h == "/download/latest.json"));
}

#[test]
fn check_reports_update_available() {
    let server = Server::start(release(NEW, &Release::default()));
    let sb = Sandbox::new();
    let out = sb
        .cmd(&adoc(), &server.url)
        .args(["upgrade", "--check"])
        .assert()
        .success();
    let text = stdout(out.get_output());
    assert!(
        text.contains(&format!("New version available: {CURRENT} → {NEW}")),
        "{text}"
    );
    assert!(text.contains("adoc upgrade"), "{text}");

    let out = sb
        .cmd(&adoc(), &server.url)
        .args(["upgrade", "--check", "--format", "json"])
        .assert()
        .success();
    let v: serde_json::Value = serde_json::from_str(&stdout(out.get_output())).unwrap();
    assert_eq!(v["latest"], NEW);
    assert_eq!(v["update_available"], true);
    if cfg!(any(
        target_os = "macos",
        all(target_os = "linux", target_env = "gnu")
    )) {
        let url = v["url"].as_str().unwrap();
        assert!(
            url.starts_with(&server.url) && url.ends_with(".tar.gz"),
            "{url}"
        );
    }
    // Las comprobaciones dejan la caché que usa `adoc doctor`.
    assert!(sb.path().join("config/update-check.json").is_file());
}

#[test]
fn non_loopback_http_is_rejected() {
    let sb = Sandbox::new();
    let out = sb
        .cmd(&adoc(), "http://example.com/download")
        .args(["upgrade", "--check"])
        .assert()
        .failure();
    let err = stderr(out.get_output());
    assert!(err.contains("must use https"), "{err}");
}

#[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
#[test]
fn non_interactive_upgrade_requires_yes() {
    let server = Server::start(release(NEW, &Release::default()));
    let sb = Sandbox::new();
    let bin = sb.install_copy("bin");
    let before = fs::read(&bin).unwrap();
    let out = sb
        .cmd(&bin, &server.url)
        .args(["upgrade"])
        .assert()
        .failure();
    let err = stderr(out.get_output());
    assert!(err.contains("adoc upgrade --yes"), "{err}");
    assert_eq!(fs::read(&bin).unwrap(), before);
}

#[cfg(all(
    unix,
    any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))
))]
mod unix {
    use super::*;

    /// Archivos que empiezan por `.` en la carpeta (restos de la descarga).
    fn leftovers(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with('.'))
            .collect()
    }

    #[test]
    fn upgrade_replaces_both_binaries() {
        let server = Server::start(release(NEW, &Release::default()));
        let sb = Sandbox::new();
        let bin = sb.install_copy("bin");
        let out = sb
            .cmd(&bin, &server.url)
            .args(["upgrade", "--yes"])
            .assert()
            .success();
        let text = stdout(out.get_output());
        assert!(text.contains(&format!("{CURRENT} → {NEW}")), "{text}");
        assert!(text.contains("SHA-256 verified"), "{text}");
        assert!(text.contains(&format!("Updated: adoc {NEW}")), "{text}");

        // El binario nuevo responde con la versión nueva.
        let v = std::process::Command::new(&bin)
            .arg("--version")
            .output()
            .unwrap();
        assert_eq!(stdout(&v).trim(), format!("adoc {NEW}"));
        let long = sb.path().join("bin/archeology-doc");
        let v = std::process::Command::new(&long)
            .arg("--version")
            .output()
            .unwrap();
        assert_eq!(stdout(&v).trim(), format!("archeology-doc {NEW}"));
        assert!(leftovers(&sb.path().join("bin")).is_empty());
        let hits = server.hits();
        assert!(
            hits.contains(&"/download/SHA256SUMS".to_string()),
            "{hits:?}"
        );
    }

    #[test]
    fn force_reinstalls_the_same_version() {
        let server = Server::start(release(CURRENT, &Release::default()));
        let sb = Sandbox::new();
        let bin = sb.install_copy("bin");
        let out = sb
            .cmd(&bin, &server.url)
            .args(["upgrade", "--yes", "--force"])
            .assert()
            .success();
        let text = stdout(out.get_output());
        assert!(
            text.contains(&format!("Reinstall adoc {CURRENT}")),
            "{text}"
        );
        // Ahora es el script falso de la "publicación".
        assert!(fs::read_to_string(&bin).unwrap().starts_with("#!/bin/sh"));
    }

    fn assert_aborts(opts: Release, expected: &str) {
        let server = Server::start(release(NEW, &opts));
        let sb = Sandbox::new();
        let bin = sb.install_copy("bin");
        let before = fs::read(&bin).unwrap();
        let out = sb
            .cmd(&bin, &server.url)
            .args(["upgrade", "--yes"])
            .assert()
            .failure();
        let err = stderr(out.get_output());
        assert!(err.contains(expected), "{err}");
        // Nada cambió: mismo binario, sin el segundo nombre ni restos.
        assert_eq!(fs::read(&bin).unwrap(), before);
        assert!(!sb.path().join("bin/archeology-doc").exists());
        assert!(leftovers(&sb.path().join("bin")).is_empty());
    }

    #[test]
    fn sums_that_disagree_abort_without_touching_files() {
        assert_aborts(
            Release {
                tamper_sums: true,
                ..Default::default()
            },
            "disagree on the SHA-256",
        );
    }

    #[test]
    fn tampered_archive_aborts_without_touching_files() {
        assert_aborts(
            Release {
                corrupt_archive: true,
                ..Default::default()
            },
            "does not match",
        );
    }

    #[test]
    fn zip_slip_archive_is_rejected() {
        assert_aborts(
            Release {
                extra_entry: Some("../evil"),
                ..Default::default()
            },
            "unsafe path (../evil)",
        );
    }

    #[test]
    fn cargo_install_is_not_replaced() {
        let server = Server::start(release(NEW, &Release::default()));
        let sb = Sandbox::new();
        let cargo_home = sb.path().join("cargo");
        let bin = sb.install_copy("cargo/bin");
        fs::write(cargo_home.join(".crates.toml"), "[v1]\n").unwrap();
        let before = fs::read(&bin).unwrap();
        let out = sb
            .cmd(&bin, &server.url)
            .env("CARGO_HOME", &cargo_home)
            .args(["upgrade", "--yes"])
            .assert()
            .success();
        let text = stdout(out.get_output());
        assert!(
            text.contains(
                "cargo install --git https://github.com/jsoncr/archeology-doc archeology-cli --force"
            ),
            "{text}"
        );
        assert_eq!(fs::read(&bin).unwrap(), before);
        // No se descargó ningún archivo de binarios.
        assert!(!server.hits().iter().any(|h| h.ends_with(".tar.gz")));
    }

    #[test]
    fn read_only_folder_gives_a_clear_error() {
        use std::os::unix::fs::PermissionsExt;
        let server = Server::start(release(NEW, &Release::default()));
        let sb = Sandbox::new();
        let bin = sb.install_copy("bin");
        let dir = sb.path().join("bin");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
        let out = sb
            .cmd(&bin, &server.url)
            .args(["upgrade", "--yes"])
            .assert()
            .failure();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        let err = stderr(out.get_output());
        assert!(err.contains("cannot write to"), "{err}");
        assert!(err.contains("install.sh"), "{err}");
    }
}

#[test]
fn doctor_update_notice_uses_cache_and_can_be_disabled() {
    let server = Server::start(release(NEW, &Release::default()));
    let sb = Sandbox::new();

    // Desactivado: ninguna petición.
    let out = sb
        .cmd(&adoc(), &server.url)
        .env("ADOC_NO_UPDATE_CHECK", "1")
        .args(["doctor", "--format", "json"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(!v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["name"] == "actualización"));
    assert!(server.hits().is_empty(), "{:?}", server.hits());

    // Activado: una consulta y el aviso (no es un fallo).
    let out = sb
        .cmd(&adoc(), &server.url)
        .args(["doctor", "--format", "json"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let check = v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "actualización")
        .cloned()
        .expect("sin aviso de actualización");
    assert_eq!(check["level"], "warn");
    assert!(check["detail"].as_str().unwrap().contains(NEW), "{check}");
    assert_eq!(server.hits(), vec!["/download/latest.json".to_string()]);

    // Segunda vez: de la caché, sin red.
    sb.cmd(&adoc(), &server.url)
        .args(["doctor", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(server.hits().len(), 1);
}

#[test]
fn doctor_survives_an_unreachable_server() {
    // Puerto cerrado: la comprobación falla en silencio y doctor sigue.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/download", listener.local_addr().unwrap());
    drop(listener);
    let sb = Sandbox::new();
    let out = sb
        .cmd(&adoc(), &url)
        .args(["doctor", "--format", "json"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(!v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["name"] == "actualización"));
}
