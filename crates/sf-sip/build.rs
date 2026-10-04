//! Baut libre und libbaresip (Git-Submodule unter `vendor/`, Version im
//! Submodul-Tag) statisch und die C-Schicht `src/shim.c` dazu.
//!
//! G.722: unter Linux über spandsp (Systempaket), unter macOS und Windows
//! über die mitgelieferte libg722 (`vendor/libg722`), damit dort keine
//! Zusatzpakete nötig sind.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Os {
    Linux,
    Mac,
    Windows,
}

impl Os {
    fn target() -> Self {
        match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
            Ok("macos") => Os::Mac,
            Ok("windows") => Os::Windows,
            _ => Os::Linux,
        }
    }

    /// In libbaresip einkompilierte Module
    fn modules(self) -> &'static str {
        match self {
            Os::Linux => {
                "g711;g722;srtp;auconv;auresamp;pipewire;pulse;alsa;stun;netroam;ausine;aubridge"
            }
            Os::Mac => "g711;libg722;srtp;auconv;auresamp;coreaudio;stun;netroam;ausine;aubridge",
            Os::Windows => "g711;libg722;srtp;auconv;auresamp;wasapi;stun;netroam;ausine;aubridge",
        }
    }

    /// Dateiname einer statischen Bibliothek
    fn lib(self, name: &str) -> String {
        if self == Os::Windows {
            format!("{name}.lib")
        } else {
            format!("lib{name}.a")
        }
    }
}

/// Linux: Systembibliotheken per pkg-config (Namen der .pc-Dateien)
const SYSTEM_LIBS_LINUX: &[&str] = &["openssl", "zlib", "libpipewire-0.3", "libpulse", "alsa"];

/// Windows-Systembibliotheken für libre, baresip und OpenSSL
const SYSTEM_LIBS_WINDOWS: &[&str] = &[
    "qwave", "iphlpapi", "wsock32", "ws2_32", "dbghelp", "winmm", "gdi32", "crypt32", "strmiids",
    "ole32", "oleaut32", "user32", "advapi32", "bcrypt", "avrt",
];

/// macOS-Frameworks für libre (DNS, Netz) und das coreaudio-Modul
const FRAMEWORKS_MACOS: &[&str] = &[
    "CoreFoundation",
    "SystemConfiguration",
    "CoreAudio",
    "AudioToolbox",
];

fn main() {
    let os = Os::target();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let vendor = root.join("vendor");
    let re_src = vendor.join("re");
    let baresip_src = vendor.join("baresip");
    let g722_src = vendor.join("libg722");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());

    if [&re_src, &baresip_src, &g722_src]
        .iter()
        .any(|d| !d.join("CMakeLists.txt").exists())
    {
        panic!("Submodule fehlen: `git submodule update --init --recursive` ausführen");
    }

    let openssl = openssl_root(os);

    let re_out = cmake::Config::new(&re_src)
        .out_dir(out.join("re"))
        .profile("Release")
        .define("LIBRE_BUILD_SHARED", "OFF")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("OPENSSL_ROOT_DIR", &openssl)
        .build_target("re")
        .build();
    let re_name = if os == Os::Windows { "re-static" } else { "re" };
    let re_lib = find_lib(&re_out.join("build"), &os.lib(re_name));

    let mut baresip = cmake::Config::new(&baresip_src);
    baresip
        .out_dir(out.join("baresip"))
        .profile("Release")
        .define("STATIC", "ON")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("OPENSSL_ROOT_DIR", &openssl)
        .define("MODULES", os.modules())
        .define("re_DIR", re_src.join("cmake"))
        .define("RE_LIBRARY", &re_lib)
        .define("RE_INCLUDE_DIR", re_src.join("include"));
    if os != Os::Linux {
        let g722_lib = build_libg722(&g722_src, &out);
        baresip
            .define("LIBG722_INCLUDE_DIR", &g722_src)
            .define("LIBG722_LIBRARY", &g722_lib);
    }
    let baresip_out = baresip.build_target("baresip").build();
    // OUTPUT_NAME ist schon "libbaresip": unter Unix ohne weiteres Präfix
    let baresip_file = if os == Os::Windows {
        "libbaresip.lib"
    } else {
        "libbaresip.a"
    };
    let baresip_lib = find_lib(&baresip_out.join("build"), baresip_file);

    let shim_out = cmake::Config::new(root.join("cmake"))
        .out_dir(out.join("shim"))
        .profile("Release")
        .define("re_DIR", re_src.join("cmake"))
        .define("OPENSSL_ROOT_DIR", &openssl)
        .define("SFSIP_SHIM", root.join("src/shim.c"))
        .define("RE_INCLUDE", re_src.join("include"))
        .define("BARESIP_INCLUDE", baresip_src.join("include"))
        .build();

    for dir in [
        shim_out.join("lib"),
        baresip_lib.parent().unwrap().to_owned(),
        re_lib.parent().unwrap().to_owned(),
    ] {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }
    // Reihenfolge: Nutzer vor Bibliothek.
    println!("cargo:rustc-link-lib=static=sfsip");
    println!(
        "cargo:rustc-link-lib=static={}",
        if os == Os::Windows {
            "libbaresip"
        } else {
            "baresip"
        }
    );
    println!("cargo:rustc-link-lib=static={re_name}");

    match os {
        Os::Linux => link_linux(),
        Os::Mac => link_macos(&openssl),
        Os::Windows => link_windows(&openssl),
    }

    println!("cargo:rerun-if-env-changed=OPENSSL_DIR");
    println!("cargo:rerun-if-changed=src/shim.c");
    println!("cargo:rerun-if-changed=cmake/CMakeLists.txt");
}

/// libg722 als statische Bibliothek `g722`; gibt den Pfad zurück.
fn build_libg722(src: &Path, out: &Path) -> PathBuf {
    let dir = out.join("g722");
    cc::Build::new()
        .files(["g722_encode.c", "g722_decode.c"].map(|f| src.join(f)))
        .include(src)
        .out_dir(&dir)
        .warnings(false)
        .compile("g722");
    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-lib=static=g722");
    let lib = dir.join(Os::target().lib("g722"));
    assert!(lib.exists(), "libg722 nicht gebaut: {}", lib.display());
    lib
}

fn link_linux() {
    // spandsp (nur für G.722) statisch, damit das Paket auf dem Zielsystem
    // nicht nachinstalliert werden muss. Die Fax-Teile, die libtiff
    // bräuchten, werden nicht mitgelinkt.
    let spandsp = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("spandsp")
        .unwrap_or_else(|e| panic!("spandsp nicht gefunden (libspandsp-dev installieren): {e}"));
    for dir in &spandsp.link_paths {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }
    println!("cargo:rustc-link-lib=static=spandsp");
    for lib in SYSTEM_LIBS_LINUX {
        pkg_config::Config::new().probe(lib).unwrap_or_else(|e| {
            panic!("{lib} nicht gefunden (Entwicklungspaket installieren): {e}")
        });
    }
    for lib in ["resolv", "m", "dl", "pthread"] {
        println!("cargo:rustc-link-lib={lib}");
    }
}

fn link_macos(openssl: &str) {
    // OpenSSL statisch (Homebrew), damit die App ohne Homebrew läuft; zlib
    // bringt macOS selbst mit.
    println!("cargo:rustc-link-search=native={openssl}/lib");
    println!("cargo:rustc-link-lib=static=ssl");
    println!("cargo:rustc-link-lib=static=crypto");
    println!("cargo:rustc-link-lib=z");
    for fw in FRAMEWORKS_MACOS {
        println!("cargo:rustc-link-lib=framework={fw}");
    }
    for lib in ["resolv", "m", "pthread"] {
        println!("cargo:rustc-link-lib={lib}");
    }
}

fn link_windows(openssl: &str) {
    // OpenSSL aus vcpkg (Triplet x64-windows-static-md)
    println!("cargo:rustc-link-search=native={openssl}/lib");
    println!("cargo:rustc-link-lib=static=libssl");
    println!("cargo:rustc-link-lib=static=libcrypto");
    for lib in SYSTEM_LIBS_WINDOWS {
        println!("cargo:rustc-link-lib={lib}");
    }
}

/// OpenSSL-Verzeichnis für CMake und den Linker. `OPENSSL_DIR` gilt überall;
/// sonst unter macOS Homebrew (ohne pkg-config), unter Linux leer
/// (Systempfade reichen).
fn openssl_root(os: Os) -> String {
    if let Ok(dir) = std::env::var("OPENSSL_DIR") {
        return dir;
    }
    match os {
        Os::Linux => String::new(),
        Os::Mac => ["/opt/homebrew/opt/openssl@3", "/usr/local/opt/openssl@3"]
            .into_iter()
            .find(|d| Path::new(d).join("lib/libssl.a").exists())
            .map(str::to_owned)
            .or_else(|| pkg_config::get_variable("openssl", "prefix").ok())
            .expect("OpenSSL nicht gefunden (brew install openssl@3 oder OPENSSL_DIR setzen)"),
        Os::Windows => {
            panic!("OPENSSL_DIR fehlt (vcpkg install openssl:x64-windows-static-md)")
        }
    }
}

fn find_lib(dir: &Path, name: &str) -> PathBuf {
    let candidates = [dir.join(name), dir.join("Release").join(name)];
    candidates
        .into_iter()
        .find(|p| p.exists())
        .unwrap_or_else(|| panic!("{name} nicht gebaut in {}", dir.display()))
}
