//! Impor bookmark dari browser Chromium lain (Chrome, Edge, Brave/Brave Origin, Vivaldi,
//! Chromium). Berkas `Bookmarks` di profil pengguna berupa JSON dan bisa dibaca tanpa admin.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Browser yang ditemukan: (nama, berkas Bookmarks).
pub fn detect() -> Vec<(String, PathBuf)> {
    let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else { return Vec::new() };
    let mut roots: Vec<(String, PathBuf)> = vec![
        ("Google Chrome".into(), local.join(r"Google\Chrome\User Data")),
        ("Microsoft Edge".into(), local.join(r"Microsoft\Edge\User Data")),
        ("Vivaldi".into(), local.join(r"Vivaldi\User Data")),
        ("Chromium".into(), local.join(r"Chromium\User Data")),
    ];
    // Brave, Brave Origin, Brave Beta/Nightly: semua di bawah BraveSoftware.
    if let Ok(rd) = std::fs::read_dir(local.join("BraveSoftware")) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().replace("Brave-Browser", "Brave").replace('-', " ");
            roots.push((name, e.path().join("User Data")));
        }
    }
    let mut found = Vec::new();
    for (name, root) in roots {
        for profile in profiles(&root) {
            let f = root.join(&profile).join("Bookmarks");
            if f.is_file() {
                let label = if profile == "Default" { name.clone() } else { format!("{name} ({profile})") };
                found.push((label, f));
            }
        }
    }
    found
}

fn profiles(root: &Path) -> Vec<String> {
    let mut v = vec!["Default".to_string()];
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("Profile ") {
                v.push(n);
            }
        }
    }
    v
}

/// Semua bookmark (judul, alamat) dalam berkas Bookmarks Chromium.
pub fn read_bookmarks(file: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(file) else { return Vec::new() };
    let Ok(json) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    let mut out = Vec::new();
    if let Some(roots) = json["roots"].as_object() {
        for key in ["bookmark_bar", "other", "synced"] {
            if let Some(node) = roots.get(key) {
                walk(node, &mut out);
            }
        }
    }
    out
}

fn walk(node: &Value, out: &mut Vec<(String, String)>) {
    match node["type"].as_str() {
        Some("url") => {
            if let Some(u) = node["url"].as_str() {
                if u.starts_with("http://") || u.starts_with("https://") {
                    out.push((node["name"].as_str().unwrap_or(u).to_string(), u.to_string()));
                }
            }
        }
        _ => {
            if let Some(children) = node["children"].as_array() {
                for c in children {
                    walk(c, out);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nested_bookmarks() {
        let d = std::env::temp_dir().join(format!("ovner-import-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("Bookmarks");
        std::fs::write(
            &f,
            r#"{"roots":{"bookmark_bar":{"type":"folder","children":[
                {"type":"url","name":"Kompas","url":"https://kompas.com/"},
                {"type":"folder","name":"Kerja","children":[{"type":"url","name":"HR","url":"https://hr.kantor.id/"},
                 {"type":"url","name":"JS","url":"javascript:void(0)"}]}]},
              "other":{"type":"folder","children":[{"type":"url","name":"","url":"https://a.id/"}]},
              "synced":{"type":"folder","children":[]}}}"#,
        )
        .unwrap();
        let b = read_bookmarks(&f);
        assert_eq!(b.len(), 3);
        assert_eq!(b[1], ("HR".to_string(), "https://hr.kantor.id/".to_string()));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
