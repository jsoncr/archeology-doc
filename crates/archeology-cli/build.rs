//! Embebe el target de compilación (`ADOC_BUILD_TARGET`): `adoc upgrade` lo
//! usa para elegir el archivo de la versión publicada para esta plataforma.

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=ADOC_BUILD_TARGET={target}");
    println!("cargo:rerun-if-changed=build.rs");
}
