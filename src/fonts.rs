//! Pilihan font: 1 bawaan Windows + 9 font modern (OFL, versi Latin variabel dari Fontsource,
//! disimpan di dalam ovner.exe; lisensi di ui/fonts/LICENSES-OFL.txt). Hanya font yang dipilih
//! yang dimuat (sebagai data: URL di @font-face), jadi font lain tidak memakai RAM.

use base64::Engine;

/// (kunci setelan, nama tampilan, nama keluarga CSS, isi woff2).
pub const FONTS: &[(&str, &str, &str, &[u8])] = &[
    ("segoe", "Segoe UI Variable (bawaan Windows)", "", &[]),
    ("albert-sans", "Albert Sans", "Albert Sans Variable", include_bytes!("ui/fonts/albert-sans.woff2")),
    ("inter", "Inter", "Inter Variable", include_bytes!("ui/fonts/inter.woff2")),
    ("plus-jakarta-sans", "Plus Jakarta Sans", "Plus Jakarta Sans Variable", include_bytes!("ui/fonts/plus-jakarta-sans.woff2")),
    ("manrope", "Manrope", "Manrope Variable", include_bytes!("ui/fonts/manrope.woff2")),
    ("dm-sans", "DM Sans", "DM Sans Variable", include_bytes!("ui/fonts/dm-sans.woff2")),
    ("outfit", "Outfit", "Outfit Variable", include_bytes!("ui/fonts/outfit.woff2")),
    ("figtree", "Figtree", "Figtree Variable", include_bytes!("ui/fonts/figtree.woff2")),
    ("geist", "Geist", "Geist Variable", include_bytes!("ui/fonts/geist.woff2")),
    ("nunito-sans", "Nunito Sans", "Nunito Sans Variable", include_bytes!("ui/fonts/nunito-sans.woff2")),
];

/// Font bawaan (sebelum 0.8.25).
pub const SYSTEM_STACK: &str = "\"Segoe UI Variable Text\", \"Segoe UI\", system-ui, sans-serif";

pub fn is_known(key: &str) -> bool {
    FONTS.iter().any(|f| f.0 == key)
}

fn find(key: &str) -> Option<&'static (&'static str, &'static str, &'static str, &'static [u8])> {
    FONTS.iter().find(|f| f.0 == key && !f.3.is_empty())
}

/// Daftar untuk Pengaturan: [[kunci, nama], ...].
pub fn choices() -> serde_json::Value {
    serde_json::Value::Array(FONTS.iter().map(|f| serde_json::json!([f.0, f.1])).collect())
}

/// `font-family` untuk kunci ini (dengan cadangan font Windows).
pub fn stack(key: &str) -> String {
    match find(key) {
        Some(f) => format!("\"{}\", {SYSTEM_STACK}", f.2),
        None => SYSTEM_STACK.to_string(),
    }
}

/// @font-face untuk font ini ("" untuk font bawaan Windows). `local()` dicoba lebih dulu
/// (bila font itu kebetulan terpasang di Windows).
pub fn face(key: &str) -> String {
    let Some(f) = find(key) else { return String::new() };
    let b64 = base64::engine::general_purpose::STANDARD.encode(f.3);
    format!(
        "@font-face{{font-family:\"{fam}\";font-style:normal;font-display:swap;font-weight:100 900;src:local(\"{name}\"),url(data:font/woff2;base64,{b64}) format(\"woff2\")}}",
        fam = f.2,
        name = f.1
    )
}

/// CSS untuk tampilan Ovner (toolbar, panel, halaman internal).
pub fn ui_css(key: &str) -> String {
    format!("{}:root{{--ovner-font:{}}}", face(key), stack(key))
}

/// CSS yang dipaksakan ke halaman web ("" bila tidak dipaksakan). Ikon (font ikon), kode, dan
/// rumus tidak ikut diganti supaya simbol situs tidak berubah menjadi huruf.
pub fn page_css(key: &str) -> String {
    let Some(f) = find(key) else { return String::new() };
    format!(
        "{face}\nhtml body, html body :where(*:not(i,code,pre,kbd,samp,tt,var,math,svg,svg *,[class*=\"icon\" i],[class*=\"fa-\"],[class^=\"fa\"],[class*=\"glyph\" i],[class*=\"material\" i],[class*=\"symbol\" i],[class*=\"vjs-\"],[aria-hidden=\"true\"])){{font-family:\"{fam}\",{SYSTEM_STACK}!important}}",
        face = face(key),
        fam = f.2
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn fonts_are_real_woff2() {
        assert_eq!(super::FONTS.len(), 10);
        for f in &super::FONTS[1..] {
            assert_eq!(&f.3[..4], b"wOF2", "{}", f.0);
            assert!(f.3.len() > 10_000 && f.3.len() < 80_000, "{}", f.0);
        }
        assert!(super::ui_css("albert-sans").contains("Albert Sans Variable"));
        assert!(super::face("segoe").is_empty());
        assert!(super::page_css("segoe").is_empty());
        assert!(super::page_css("inter").contains("!important"));
    }
}
