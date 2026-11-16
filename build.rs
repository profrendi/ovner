//! Tanam ikon dan informasi versi ke exe Windows, supaya ikon Start Menu, Desktop,
//! Explorer, dan Task Manager menampilkan Ovner.

#[allow(dead_code)]
#[path = "src/icon.rs"]
mod icon;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/icon.rs");

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
