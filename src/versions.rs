//! Versi komponen Ovner dan pengecekan WebView2 terbaru dari sumber resmi Microsoft.
//!
//! WebView2 Evergreen Runtime mengikuti versi Microsoft Edge Stable, jadi versi terbarunya
//! diambil dari API pembaruan Edge milik Microsoft (`edgeupdates.microsoft.com`).

use std::path::Path;
use std::time::{Duration, SystemTime};

use serde_json::Value;

/// Komponen utama beserta versinya (diisi build.rs dari Cargo.lock).
pub const COMPONENTS: &str = env!("OVNER_COMPONENTS");

/// API resmi Microsoft untuk daftar rilis Edge (dan WebView2 Evergreen).
pub const EDGE_API: &str = "https://edgeupdates.microsoft.com/api/products";

/// Halaman unduhan resmi WebView2 Runtime.
pub const WEBVIEW2_PAGE: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";

/// Ambil versi Stable terbaru untuk Windows x64 dari JSON `EDGE_API`.
pub fn parse_edge_stable(json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(json).ok()?;
    let stable = v.as_array()?.iter().find(|p| p["Product"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("stable")))?;
    stable["Releases"]
        .as_array()?
        .iter()
        .filter(|r| r["Platform"] == "Windows" && r["Architecture"] == "x64")
        .filter_map(|r| r["ProductVersion"].as_str())
        .max_by(|a, b| cmp_version(a, b))
        .map(str::to_string)
}

/// Bandingkan versi "157.0.4341.46" per angka.
pub fn cmp_version(a: &str, b: &str) -> std::cmp::Ordering {
    let p = |s: &str| s.split('.').map(|x| x.trim().parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    p(a).cmp(&p(b))
}

/// Status WebView2 yang terpasang dibanding yang terbaru.
pub fn status(installed: &str, latest: &str, lang_id: bool) -> (bool, String) {
    let ord = cmp_version(installed, latest);
    let ok = ord != std::cmp::Ordering::Less;
    let msg = match (ord, lang_id) {
        (std::cmp::Ordering::Less, true) => format!("terpasang {installed}, terbaru {latest}: ada versi baru (Windows memperbaruinya otomatis lewat Edge Update)"),
        (std::cmp::Ordering::Less, false) => format!("installed {installed}, latest {latest}: a newer version exists (Windows updates it automatically via Edge Update)"),
        (_, true) => format!("terpasang {installed}, terbaru {latest}: sudah terbaru"),
        (_, false) => format!("installed {installed}, latest {latest}: up to date"),
    };
    (ok, msg)
}

fn cache_file(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("webview2-latest.txt")
}

/// Versi terbaru yang terakhir diketahui (dari cache), bila ada.
pub fn cached_latest(data_dir: &Path) -> Option<String> {
    let s = std::fs::read_to_string(cache_file(data_dir)).ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Ambil versi terbaru dari Microsoft lalu simpan ke cache. `max_age`: lewati bila cache masih baru.
pub fn refresh_latest(data_dir: &Path, max_age: Option<Duration>, download: impl Fn(&str, &Path) -> Result<(), String>) -> Result<String, String> {
    let file = cache_file(data_dir);
    if let Some(age) = max_age {
        let fresh = std::fs::metadata(&file).ok().and_then(|m| m.modified().ok()).and_then(|t| SystemTime::now().duration_since(t).ok()).is_some_and(|d| d < age);
        if fresh {
            if let Some(v) = cached_latest(data_dir) {
                return Ok(v);
            }
        }
    }
    let tmp = std::env::temp_dir().join(format!("ovner-edge-{}.json", std::process::id()));
    let r = download(EDGE_API, &tmp);
    let body = std::fs::read_to_string(&tmp).unwrap_or_default();
    let _ = std::fs::remove_file(&tmp);
    r?;
    let v = parse_edge_stable(&body).ok_or_else(|| "jawaban Microsoft tidak dikenali".to_string())?;
    let _ = std::fs::write(&file, &v);
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_stable_parse() {
        let json = r#"[
          {"Product":"Beta","Releases":[{"Platform":"Windows","Architecture":"x64","ProductVersion":"158.0.1.1"}]},
          {"Product":"Stable","Releases":[
            {"Platform":"Windows","Architecture":"x86","ProductVersion":"157.0.4341.99"},
            {"Platform":"Windows","Architecture":"x64","ProductVersion":"157.0.4341.42"},
            {"Platform":"Windows","Architecture":"x64","ProductVersion":"157.0.4341.46"},
            {"Platform":"MacOS","Architecture":"universal","ProductVersion":"157.0.4341.50"}
          ]}]"#;
        assert_eq!(parse_edge_stable(json).as_deref(), Some("157.0.4341.46"));
        assert!(parse_edge_stable("[]").is_none());
        assert!(parse_edge_stable("bukan json").is_none());
    }

    #[test]
    fn version_compare() {
        assert!(status("157.0.4341.46", "157.0.4341.46", true).0);
        assert!(status("157.0.4341.100", "157.0.4341.46", true).0);
        assert!(!status("156.0.1.1", "157.0.4341.46", true).0);
        assert!(COMPONENTS.contains("wry "), "{COMPONENTS}");
    }
}
