use std::path::{Path, PathBuf};

/// Nur die für den Client relevanten Protos; Edge-Node-, Identity- und
/// Media-Gateway-Dienste sind anlagenintern.
const DIRS: &[&str] = &["proxy", "client-gateway", "typedefs"];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../proto");
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in DIRS {
        for entry in std::fs::read_dir(root.join(dir))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "proto") {
                files.push(path);
            }
        }
    }
    files.sort();

    if std::env::var_os("PROTOC").is_none() {
        // SAFETY: build scripts run single-threaded.
        unsafe { std::env::set_var("PROTOC", protoc_bin_vendored::protoc_bin_path()?) };
    }

    tonic_prost_build::configure()
        .build_server(false)
        .include_file("onehub.rs")
        .compile_protos(&files, &[root])?;

    println!("cargo:rerun-if-changed=../../proto");
    Ok(())
}
