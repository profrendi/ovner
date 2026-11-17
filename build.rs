//! Tanam ikon dan informasi versi ke exe Windows, supaya ikon Start Menu, Desktop,
//! Explorer, dan Task Manager menampilkan Ovner.

#[allow(dead_code)]
#[path = "src/icon.rs"]
mod icon;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/icon.rs");
    // Nomor build dan kanal (stable/nightly) diisi oleh CI lewat option_env!.
    println!("cargo:rerun-if-env-changed=OVNER_BUILD");
    println!("cargo:rerun-if-env-changed=OVNER_CHANNEL");
    // Versi komponen utama (dari Cargo.lock) untuk "Tentang Ovner" dan halaman Diagnostik.
    println!("cargo:rerun-if-changed=Cargo.lock");
    let lock = std::fs::read_to_string("Cargo.lock").unwrap_or_default();
    let mut parts = Vec::new();
    for name in ["wry", "tao", "webview2-com", "windows", "adblock", "tray-icon", "ed25519-dalek"] {
        let mut found = None;
        let mut lines = lock.lines();
        while let Some(l) = lines.next() {
            if l.trim() == format!("name = \"{name}\"") {
                found = lines.next().and_then(|v| v.trim().strip_prefix("version = \"")).map(|v| v.trim_end_matches('"').to_string());
                // Ambil versi tertinggi bila ada beberapa (mis. windows 0.61 dan 0.62).
                if let Some(v) = &found {
                    parts.retain(|p: &String| !p.starts_with(&format!("{name} ")));
                    parts.push(format!("{name} {v}"));
                }
            }
        }
        let _ = found;
    }
    println!("cargo:rustc-env=OVNER_COMPONENTS={}", parts.join(", "));

    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("ovner.ico");
        std::fs::write(&out, icon::ico(&[16, 20, 24, 32, 40, 48, 64, 128, 256])).expect("tulis ikon");
        let mut res = winresource::WindowsResource::new();
        res.set_icon(out.to_str().unwrap())
            .set("ProductName", "Ovner")
            .set("FileDescription", "Ovner")
            .set("CompanyName", "profrendi")
            .set("LegalCopyright", "MIT");
        res.compile().expect("kompilasi resource Windows");
    }
}
