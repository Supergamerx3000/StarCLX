//! Baut libre und libbaresip (Git-Submodule unter `vendor/`, Version im
//! Submodul-Tag) statisch und die C-Schicht `src/shim.c` dazu.

use std::path::{Path, PathBuf};

/// In libbaresip einkompilierte Module. g722 braucht spandsp.
const MODULES: &str =
    "g711;g722;srtp;auconv;auresamp;pipewire;pulse;alsa;stun;netroam;ausine;aubridge";

/// Systembibliotheken per pkg-config (Namen der .pc-Dateien).
const SYSTEM_LIBS: &[&str] = &[
    "openssl",
    "zlib",
    "spandsp",
    "libpipewire-0.3",
    "libpulse",
    "alsa",
];

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let vendor = root.join("vendor");
    let re_src = vendor.join("re");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let baresip_src = vendor.join("baresip");

    if !re_src.join("CMakeLists.txt").exists() || !baresip_src.join("CMakeLists.txt").exists() {
        panic!("Submodule fehlen: `git submodule update --init --recursive` ausführen");
    }

    let re_out = cmake::Config::new(&re_src)
        .out_dir(out.join("re"))
        .profile("Release")
        .define("LIBRE_BUILD_SHARED", "OFF")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .build_target("re")
        .build();
    let re_lib = find_lib(&re_out.join("build"), "libre.a");

    let baresip_out = cmake::Config::new(&baresip_src)
        .out_dir(out.join("baresip"))
        .profile("Release")
        .define("STATIC", "ON")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("MODULES", MODULES)
        .define("re_DIR", re_src.join("cmake"))
        .define("RE_LIBRARY", &re_lib)
        .define("RE_INCLUDE_DIR", re_src.join("include"))
        .build_target("baresip")
        .build();
    let baresip_lib = find_lib(&baresip_out.join("build"), "libbaresip.a");

    let shim_out = cmake::Config::new(root.join("cmake"))
        .out_dir(out.join("shim"))
        .profile("Release")
        .define("re_DIR", re_src.join("cmake"))
        .define("SFSIP_SHIM", root.join("src/shim.c"))
        .define("RE_INCLUDE", re_src.join("include"))
        .define("BARESIP_INCLUDE", baresip_src.join("include"))
        .build();

    println!(
        "cargo:rustc-link-search=native={}",
        shim_out.join("lib").display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        baresip_lib.parent().unwrap().display()
    );
    println!(
        "cargo:rustc-link-search=native={}",
        re_lib.parent().unwrap().display()
    );
    // Reihenfolge: Nutzer vor Bibliothek.
    println!("cargo:rustc-link-lib=static=sfsip");
    println!("cargo:rustc-link-lib=static=baresip");
    println!("cargo:rustc-link-lib=static=re");

    for lib in SYSTEM_LIBS {
        pkg_config::Config::new().probe(lib).unwrap_or_else(|e| {
            panic!("{lib} nicht gefunden (Entwicklungspaket installieren): {e}")
        });
    }
    for lib in ["resolv", "m", "dl", "pthread"] {
        println!("cargo:rustc-link-lib={lib}");
    }

    println!("cargo:rerun-if-changed=src/shim.c");
    println!("cargo:rerun-if-changed=cmake/CMakeLists.txt");
}

fn find_lib(dir: &Path, name: &str) -> PathBuf {
    let candidates = [dir.join(name), dir.join("Release").join(name)];
    candidates
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_else(|| panic!("{name} nicht gebaut in {}", dir.display()))
}
